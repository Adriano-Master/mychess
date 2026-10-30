//! The glue of the tablebase panel and of the endgame training with
//! [`AppState`] (spec M8-8): the refresh at the end of every `update`, the
//! answers of the probes, the watchdog and the training.

use mychess_core::Position;
use mychess_tablebase::generate::random_position;
use mychess_tablebase::{
    ONLINE_MAX_PIECES, OpenReport, PositionReport, Source, TablebaseError, check_applicable,
    position_key,
};

use crate::effect::Effect;
use crate::error::RequestId;
use crate::flow::{AppNotice, Pending, Replacement};
use crate::host::PickKind;
use crate::message::Message;
use crate::prefs::{PrefChange, PrefsMessage};
use crate::session::{GameMessage, PlayOptions};
use crate::state::AppState;
use crate::tablebase::{
    DEBOUNCE_MS, DelayKind, DirectoryView, EndReason, IssueView, LOCAL_WATCHDOG_MS, LocalHandle,
    LocalSummary, ONLINE_WATCHDOG_MS, OPEN_WATCHDOG_MS, OnlineHandle, OpenedSet, REPLY_DELAY_MS,
    RowView, TablebaseEffect, TablebaseMessage, TablebaseNotice, TablebasePrefs, TablebaseStatus,
    TablebaseView, TrainingOutcome, TrainingView, issue_of,
};
use crate::tablebase::{Training, grade_of, grade_of_end};

/// Where a query is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// The online query waits for the position to stay still.
    Debouncing,
    /// The probe was sent.
    Probing,
}

/// The query in flight (at most one, D2563).
#[derive(Debug)]
struct InFlight {
    request: RequestId,
    key: String,
    stage: Stage,
    source: Source,
}

/// The report on screen and the position it is for.
#[derive(Debug)]
struct Shown {
    key: String,
    report: PositionReport,
}

/// What to ask, for a position.
enum Plan {
    Local(LocalHandle),
    Online,
    /// The tables are being read.
    Wait,
    /// Nothing is turned on.
    NotConfigured,
    /// This source cannot answer, and there is no other.
    Fail(TablebaseError),
}

/// The state of the panel and of the training.
#[derive(Debug, Default)]
pub(crate) struct TablebaseFlow {
    open: bool,
    local: Option<LocalHandle>,
    report: Option<OpenReport>,
    /// The folders of the open in flight and its request.
    opening: Option<(RequestId, Vec<String>)>,
    /// The folders the tables were last read for.
    opened_for: Option<Vec<String>>,
    online: OnlineHandle,
    in_flight: Option<InFlight>,
    shown: Option<Shown>,
    /// The key of the position the issue or the list is final for.
    resolved: Option<String>,
    issue: Option<IssueView>,
    locked: bool,
    training: Option<Training>,
    outcome: Option<TrainingOutcome>,
    /// A new position was asked for and the game is not replaced yet: the
    /// generation of the game it will replace (D2577).
    start_on_replace: Option<u64>,
}

impl AppState {
    /// Whether the panel is open.
    pub fn tablebase_open(&self) -> bool {
        self.flow.tablebase.open
    }

    /// Whether the training is on.
    pub fn tablebase_training(&self) -> bool {
        self.flow.tablebase.training.is_some()
    }

    /// Uses another online source (tests and embedders; the real client is
    /// built on first use).
    pub fn set_tablebase_online(&mut self, handle: OnlineHandle) {
        self.flow.tablebase.online = handle;
    }

    /// What the panel draws.
    pub fn tablebase_view(&self) -> TablebaseView {
        let tb = &self.flow.tablebase;
        let prefs = &*self.prefs.tablebase;
        let key = tb.open.then(|| position_key(self.session.position()));
        let shown = tb.shown.as_ref();
        let current = shown.filter(|s| key.as_deref() == Some(s.key.as_str()));
        let hidden = tb.training.is_some();
        let status = if tb.locked || self.assist_locked() {
            TablebaseStatus::Locked
        } else if !prefs.local && !prefs.online {
            TablebaseStatus::NotConfigured
        } else if current.is_some() {
            TablebaseStatus::Ready
        } else if shown.is_some() {
            TablebaseStatus::Stale
        } else if tb.opening.is_some() {
            TablebaseStatus::Loading
        } else if tb.issue.is_some() {
            TablebaseStatus::Ready
        } else {
            TablebaseStatus::Waiting
        };
        let report = shown.map(|s| &s.report);
        let rows = match report {
            Some(report) if !hidden => {
                let group = report.best_group().len();
                report
                    .moves
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| RowView {
                        san: entry.san.clone(),
                        category: entry.evaluation.category,
                        dtz: entry.evaluation.dtz,
                        dtz_rounded: entry.evaluation.dtz_rounded,
                        dtm: entry.evaluation.dtm,
                        best: index < group,
                        zeroing: entry.zeroing,
                        checkmate: entry.checkmate,
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        let opened_matches = tb.opened_for.as_deref() == Some(prefs.directories.as_slice());
        let directories = prefs
            .directories
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let found = tb
                    .report
                    .as_ref()
                    .filter(|_| opened_matches)
                    .and_then(|r| r.directories.get(index));
                DirectoryView {
                    path: path.clone(),
                    wdl_files: found.map(|d| d.wdl_files),
                    dtz_files: found.map(|d| d.dtz_files),
                    unreadable: found
                        .is_some_and(|d| d.status != mychess_tablebase::DirStatus::Read),
                    bad_files: found.map_or(0, |d| d.issues.len()),
                    uppercase_files: found.map_or(0, |d| d.uppercase_files),
                }
            })
            .collect();
        let turn = self.session.position().turn();
        TablebaseView {
            open: tb.open,
            status,
            local_enabled: prefs.local,
            online_enabled: prefs.online,
            directories,
            summary: tb
                .report
                .as_ref()
                .filter(|_| opened_matches && prefs.local)
                .map(|r| LocalSummary {
                    wdl_files: r.wdl_files,
                    dtz_files: r.dtz_files,
                    max_pieces: r.max_pieces,
                }),
            source: report.map(|r| r.source),
            // The training hides every result of the position (D2574).
            position: report.filter(|_| !hidden).map(|r| r.evaluation),
            counts: report
                .filter(|_| !hidden)
                .map(PositionReport::counts)
                .unwrap_or_default(),
            has_dtm: !hidden && report.is_some_and(PositionReport::has_dtm),
            rows,
            stale: shown.is_some() && current.is_none(),
            issue: tb.issue.clone(),
            training: tb.training.as_ref().map(|t| TrainingView {
                user: t.user,
                user_moves: t.user_moves,
                slips: t.slips,
                start: t.start,
                tablebase_to_move: turn != t.user,
            }),
            outcome: tb.outcome,
        }
    }

    pub(crate) fn tablebase_message(
        &mut self,
        message: TablebaseMessage,
        effects: &mut Vec<Effect>,
    ) {
        match message {
            TablebaseMessage::Toggle => {
                let open = !self.flow.tablebase.open;
                self.tablebase_set_open(open);
            }
            TablebaseMessage::SetOpen(open) => self.tablebase_set_open(open),
            TablebaseMessage::SetLocal(value) => {
                self.tablebase_set_prefs(effects, |p| p.local = value);
            }
            TablebaseMessage::SetOnline(value) => {
                self.tablebase_set_prefs(effects, |p| p.online = value);
            }
            TablebaseMessage::PickDirectory => {
                self.ask_for_file(PickKind::TablebaseDir, None, effects);
            }
            TablebaseMessage::AddDirectory(path) => self.tablebase_add_directory(&path, effects),
            TablebaseMessage::RemoveDirectory(index) => {
                self.tablebase_set_prefs(effects, |p| {
                    if index < p.directories.len() {
                        p.directories.remove(index);
                    }
                });
            }
            TablebaseMessage::Reopen => {
                let tb = &mut self.flow.tablebase;
                tb.opened_for = None;
                tb.opening = None;
                tb.resolved = None;
            }
            TablebaseMessage::PlayRow(index) => self.tablebase_play_row(index, effects),
            TablebaseMessage::TrainStart => self.tablebase_train_start(),
            TablebaseMessage::TrainStop => {
                self.flow.tablebase.training = None;
                self.flow.tablebase.outcome = None;
            }
            TablebaseMessage::DismissOutcome => self.flow.tablebase.outcome = None,
            TablebaseMessage::NewPosition { material, seed } => {
                self.tablebase_new_position(&material, seed, effects);
            }
            TablebaseMessage::Opened { request, result } => self.tablebase_opened(request, *result),
            TablebaseMessage::Probed {
                request,
                source,
                result,
            } => self.tablebase_probed(request, source, *result, effects),
            TablebaseMessage::DelayElapsed { request, kind } => {
                self.tablebase_delay_elapsed(request, kind, effects);
            }
        }
    }

    /// Opens or closes the panel. Opening asks again for the position, so a
    /// failure (no connection, a pause asked by the server) is tried anew.
    fn tablebase_set_open(&mut self, open: bool) {
        let tb = &mut self.flow.tablebase;
        if open && !tb.open {
            tb.resolved = None;
            tb.issue = None;
        }
        tb.open = open;
    }

    /// A folder chosen in the dialog (called by `file_picked`).
    pub(crate) fn tablebase_add_directory(
        &mut self,
        path: &std::path::Path,
        effects: &mut Vec<Effect>,
    ) {
        let Some(text) = path.to_str() else {
            self.notice = Some(AppNotice::Tablebase(TablebaseNotice::FolderNotSaved));
            return;
        };
        let text = text.to_owned();
        if self.prefs.tablebase.directories.len() >= crate::tablebase::MAX_DIRECTORIES
            && !self.prefs.tablebase.directories.contains(&text)
        {
            self.notice = Some(AppNotice::Tablebase(TablebaseNotice::TooManyFolders));
            return;
        }
        self.tablebase_set_prefs(effects, |p| {
            if !p.directories.contains(&text) {
                p.directories.push(text);
            }
            // Adding a folder is asking for the tables.
            p.local = true;
        });
    }

    fn tablebase_set_prefs(
        &mut self,
        effects: &mut Vec<Effect>,
        change: impl FnOnce(&mut TablebasePrefs),
    ) {
        let mut prefs = (*self.prefs.tablebase).clone();
        change(&mut prefs);
        let prefs = prefs.sanitized();
        if prefs == *self.prefs.tablebase {
            return;
        }
        self.prefs_message(
            PrefsMessage::Set(PrefChange::Tablebase(Box::new(prefs))),
            effects,
        );
        // The sources changed: an answer on screen or on its way may come
        // from one that is now off.
        let tb = &mut self.flow.tablebase;
        tb.resolved = None;
        tb.issue = None;
        tb.in_flight = None;
        tb.shown = None;
    }

    fn tablebase_busy(&self) -> bool {
        self.annotation_running()
            || self.play_live()
            || self.tournament_live()
            || self.tactics_live()
            || self.opening_live()
            || self.calvar_live()
            || self.modal_open()
    }

    fn tablebase_at_end_of_line(&self) -> bool {
        self.session
            .game()
            .children(self.session.cursor())
            .is_ok_and(<[_]>::is_empty)
    }

    fn tablebase_play_row(&mut self, index: usize, effects: &mut Vec<Effect>) {
        let tb = &self.flow.tablebase;
        if tb.training.is_some() {
            return;
        }
        let key = position_key(self.session.position());
        let Some(shown) = tb.shown.as_ref().filter(|s| s.key == key) else {
            return;
        };
        let Some(entry) = shown.report.moves.get(index) else {
            return;
        };
        let mv = entry.mv;
        if self.tablebase_busy() {
            self.notice = Some(AppNotice::Tablebase(TablebaseNotice::Busy));
            return;
        }
        // The same path as a move on the board, so every guard applies.
        effects.extend(crate::state::update(
            self,
            Message::Game(GameMessage::PlayMove {
                mv,
                options: PlayOptions::default(),
            }),
        ));
    }

    // ---- training ----

    fn tablebase_train_start(&mut self) {
        if self.tablebase_busy() {
            self.notice = Some(AppNotice::Tablebase(TablebaseNotice::Busy));
            return;
        }
        if !self.tablebase_at_end_of_line() {
            self.notice = Some(AppNotice::Tablebase(TablebaseNotice::NotAtEndOfLine));
            return;
        }
        let position = self.session.position();
        let configured = self.prefs.tablebase.local || self.prefs.tablebase.online;
        if !configured || position.game_end(&[]).is_some() {
            self.notice = Some(AppNotice::Tablebase(TablebaseNotice::NothingToTrain));
            return;
        }
        let user = position.turn();
        let generation = self.session.generation();
        let tb = &mut self.flow.tablebase;
        tb.training = Some(Training::new(user, generation));
        tb.outcome = None;
        tb.resolved = None;
        tb.open = true;
    }

    fn tablebase_new_position(&mut self, material: &str, seed: u64, effects: &mut Vec<Effect>) {
        let Ok(position) = random_position(material, seed) else {
            self.notice = Some(AppNotice::Tablebase(TablebaseNotice::BadMaterial));
            return;
        };
        // There must be something to train against before the game goes.
        if !(self.prefs.tablebase.local || self.prefs.tablebase.online) {
            self.notice = Some(AppNotice::Tablebase(TablebaseNotice::NothingToTrain));
            return;
        }
        let before = self.session.generation();
        let had_prompt = self.discard_prompt.is_some();
        self.begin_replace(Replacement::FromPosition(position.clone()), effects);
        if self.session.generation() != before {
            // The old training and its result go only now that the game was
            // replaced.
            self.tablebase_train_start();
        } else if !had_prompt && Self::asks_to_replace_with(self.discard_prompt.as_ref(), &position)
        {
            // The question is the one of this replacement: the training
            // starts when it is answered with a yes.
            self.flow.tablebase.start_on_replace = Some(before);
        }
    }

    /// Whether `prompt` asks about replacing the game by `position`.
    fn asks_to_replace_with(
        prompt: Option<&crate::flow::DiscardPrompt>,
        position: &Position,
    ) -> bool {
        matches!(
            prompt.map(|p| &p.pending),
            Some(Pending::Replace(Replacement::FromPosition(p))) if p == position
        )
    }

    /// Ends the training; shows the result when the game ended by itself.
    fn tablebase_end_training(&mut self, outcome: Option<TrainingOutcome>) {
        let tb = &mut self.flow.tablebase;
        tb.training = None;
        tb.outcome = outcome;
    }

    /// The rules that end a training: another game, the end of the game, the
    /// cursor leaving the end of the line.
    fn tablebase_check_training(&mut self) {
        if let Some(generation) = self.flow.tablebase.start_on_replace {
            if self.session.generation() != generation {
                self.flow.tablebase.start_on_replace = None;
                self.tablebase_train_start();
            } else if !matches!(
                self.discard_prompt.as_ref().map(|p| &p.pending),
                Some(Pending::Replace(Replacement::FromPosition(_)))
            ) {
                // Cancelled, or the question became another one.
                self.flow.tablebase.start_on_replace = None;
            }
        }
        let Some(training) = self.flow.tablebase.training.as_ref() else {
            return;
        };
        if self.session.generation() != training.generation {
            self.tablebase_end_training(None);
            return;
        }
        if let Some(end) = self.session.position().game_end(&[]) {
            let ended = grade_of_end(end, training.user);
            let outcome = training.outcome(Some(ended), ended.1);
            self.tablebase_end_training(Some(outcome));
            return;
        }
        if !self.tablebase_at_end_of_line() {
            self.tablebase_end_training(None);
        }
    }

    fn tablebase_training_step(&mut self, effects: &mut Vec<Effect>) {
        let key = position_key(self.session.position());
        let turn = self.session.position().turn();
        let Some(grade_and_negated) = self
            .flow
            .tablebase
            .shown
            .as_ref()
            .filter(|s| s.key == key)
            .map(|s| {
                (
                    grade_of(s.report.evaluation.category),
                    grade_of(s.report.evaluation.category.negate()),
                )
            })
        else {
            return;
        };
        let Some(user) = self.flow.tablebase.training.as_ref().map(|t| t.user) else {
            return;
        };
        if turn == user {
            let unknown = {
                let Some(t) = self.flow.tablebase.training.as_mut() else {
                    return;
                };
                if t.start.is_none() {
                    t.start = grade_and_negated.0;
                    t.current = grade_and_negated.0;
                }
                t.start.is_none()
            };
            if unknown {
                self.tablebase_out_of_range();
            }
            return;
        }
        // The user moved before the first answer told what the position was
        // worth: there is nothing to compare his move with.
        if self
            .flow
            .tablebase
            .training
            .as_ref()
            .is_some_and(|t| t.start.is_none())
        {
            self.tablebase_out_of_range();
            return;
        }
        let request = self.new_request();
        let Some(t) = self.flow.tablebase.training.as_mut() else {
            return;
        };
        t.count_user_move(&key, grade_and_negated.1);
        if t.reply.as_ref().map(|(_, k)| k) != Some(&key) {
            t.reply = Some((request, key));
            effects.push(
                TablebaseEffect::Delay {
                    request,
                    kind: DelayKind::Reply,
                    ms: REPLY_DELAY_MS,
                }
                .into_effect(),
            );
        }
    }

    fn tablebase_out_of_range(&mut self) {
        let outcome = self
            .flow
            .tablebase
            .training
            .as_ref()
            .map(|t| t.outcome(None, EndReason::OutOfRange));
        self.tablebase_end_training(outcome);
    }

    fn tablebase_reply(&mut self, effects: &mut Vec<Effect>) {
        // Something else holds the game (a dialog, a game against an engine,
        // the annotation): the training ends instead of writing into it.
        if self.edits_blocked() || self.tablebase_busy() {
            self.tablebase_end_training(None);
            return;
        }
        let position = self.session.position();
        let key = position_key(position);
        let seed = position.zobrist().value();
        let Some(mv) = self
            .flow
            .tablebase
            .shown
            .as_ref()
            .filter(|s| s.key == key)
            .and_then(|s| s.report.recommended(seed))
            .map(|entry| entry.mv)
        else {
            self.tablebase_out_of_range();
            return;
        };
        effects.extend(self.session.update(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }));
    }

    // ---- queries ----

    /// Brings the panel up to date with the position on the board (called
    /// after every message, D2569).
    pub(crate) fn refresh_tablebase(&mut self, effects: &mut Vec<Effect>) {
        if self.assist_locked() {
            let tb = &mut self.flow.tablebase;
            tb.locked = true;
            tb.in_flight = None;
            tb.shown = None;
            tb.resolved = None;
            tb.issue = None;
            tb.training = None;
            tb.start_on_replace = None;
            return;
        }
        self.flow.tablebase.locked = false;
        self.tablebase_check_training();
        let tb = &mut self.flow.tablebase;
        if !tb.open && tb.training.is_none() {
            // The files stay open while the tables are held (and Windows will
            // not rename or delete an open file): a closed panel holds none.
            // A probe still in flight keeps its own handle until it ends, at
            // most the watchdog's time (D2584).
            if tb.local.is_some() || tb.opened_for.is_some() || tb.opening.is_some() {
                tb.local = None;
                tb.report = None;
                tb.opened_for = None;
                tb.opening = None;
                tb.in_flight = None;
                tb.shown = None;
                tb.resolved = None;
            }
            return;
        }
        self.tablebase_reconcile_open(effects);
        self.tablebase_query(effects);
        if self.flow.tablebase.training.is_some() {
            self.tablebase_training_step(effects);
        }
    }

    /// Opens the folders when the preferences ask for tables that are not
    /// open yet, and lets go of them when they are turned off (D2584).
    fn tablebase_reconcile_open(&mut self, effects: &mut Vec<Effect>) {
        let local = self.prefs.tablebase.local;
        let dirs = self.prefs.tablebase.directories.clone();
        if !local {
            let tb = &mut self.flow.tablebase;
            if tb.local.is_some() || tb.opened_for.is_some() || tb.opening.is_some() {
                tb.local = None;
                tb.report = None;
                tb.opened_for = None;
                tb.opening = None;
                tb.resolved = None;
            }
            return;
        }
        let tb = &self.flow.tablebase;
        if tb.opening.is_some() || tb.opened_for.as_ref() == Some(&dirs) {
            return;
        }
        let request = self.new_request();
        let paths = self.prefs.tablebase.paths();
        let tb = &mut self.flow.tablebase;
        tb.opening = Some((request, dirs));
        tb.resolved = None;
        effects.push(
            TablebaseEffect::Open {
                request,
                dirs: paths,
            }
            .into_effect(),
        );
        effects.push(
            TablebaseEffect::Delay {
                request,
                kind: DelayKind::OpenWatchdog,
                ms: OPEN_WATCHDOG_MS,
            }
            .into_effect(),
        );
    }

    fn tablebase_opened(&mut self, request: RequestId, result: Result<OpenedSet, TablebaseError>) {
        let tb = &mut self.flow.tablebase;
        let Some((expected, dirs)) = tb.opening.take() else {
            return;
        };
        if expected != request {
            tb.opening = Some((expected, dirs));
            return;
        }
        tb.resolved = None;
        tb.opened_for = Some(dirs);
        match result {
            Ok(set) => {
                tb.local = Some(set.handle);
                tb.report = Some(set.report);
            }
            Err(error) => {
                tracing::warn!(?error, "the tablebase folders could not be opened");
                tb.local = None;
                tb.report = None;
            }
        }
    }

    fn tablebase_plan(&self, position: &Position) -> Plan {
        let prefs = &*self.prefs.tablebase;
        let tb = &self.flow.tablebase;
        let mut local_error = None;
        if prefs.local {
            match &tb.local {
                Some(handle) if handle.max_pieces() > 0 => {
                    match check_applicable(position, handle.max_pieces()) {
                        Ok(_) => return Plan::Local(handle.clone()),
                        Err(error) => local_error = Some(error),
                    }
                }
                None if tb.opening.is_some()
                    || tb.opened_for.as_deref() != Some(prefs.directories.as_slice()) =>
                {
                    return Plan::Wait;
                }
                // No tables found, or the folders could not be opened.
                _ => local_error = Some(TablebaseError::NoTables),
            }
        }
        if prefs.online {
            return match check_applicable(position, ONLINE_MAX_PIECES) {
                Ok(_) => Plan::Online,
                Err(error) => Plan::Fail(match local_error {
                    Some(local @ TablebaseError::TooManyPieces { .. }) => local,
                    _ => error,
                }),
            };
        }
        match local_error {
            Some(error) => Plan::Fail(error),
            None => Plan::NotConfigured,
        }
    }

    fn tablebase_query(&mut self, effects: &mut Vec<Effect>) {
        let position = self.session.position().clone();
        let key = position_key(&position);
        let tb = &mut self.flow.tablebase;
        // The position moved on while the query waited or was out: start
        // over. The answer of the old one is dropped by its request (D2563).
        if tb.in_flight.as_ref().is_some_and(|f| f.key != key) {
            tb.in_flight = None;
        }
        if tb.resolved.as_deref() == Some(key.as_str()) || tb.in_flight.is_some() {
            return;
        }
        match self.tablebase_plan(&position) {
            Plan::Wait => {}
            Plan::NotConfigured => {
                let tb = &mut self.flow.tablebase;
                tb.resolved = Some(key);
                tb.shown = None;
                tb.issue = None;
            }
            Plan::Fail(error) => self.tablebase_fail(&key, &error),
            Plan::Local(handle) => {
                let request = self.new_request();
                self.flow.tablebase.in_flight = Some(InFlight {
                    request,
                    key,
                    stage: Stage::Probing,
                    source: Source::Local,
                });
                effects.push(
                    TablebaseEffect::ProbeLocal {
                        request,
                        handle,
                        position,
                    }
                    .into_effect(),
                );
                effects.push(watchdog(request, LOCAL_WATCHDOG_MS));
            }
            Plan::Online => self.tablebase_start_online(&key, effects),
        }
    }

    fn tablebase_start_online(&mut self, key: &str, effects: &mut Vec<Effect>) {
        let request = self.new_request();
        // A training position is not skimmed over: no wait.
        let training = self.flow.tablebase.training.is_some();
        self.flow.tablebase.in_flight = Some(InFlight {
            request,
            key: key.to_owned(),
            stage: if training {
                Stage::Probing
            } else {
                Stage::Debouncing
            },
            source: Source::Online,
        });
        if training {
            effects.push(
                TablebaseEffect::ProbeOnline {
                    request,
                    handle: self.flow.tablebase.online.clone(),
                    position: self.session.position().clone(),
                }
                .into_effect(),
            );
            effects.push(watchdog(request, ONLINE_WATCHDOG_MS));
        } else {
            effects.push(
                TablebaseEffect::Delay {
                    request,
                    kind: DelayKind::Debounce,
                    ms: DEBOUNCE_MS,
                }
                .into_effect(),
            );
        }
    }

    /// A query ended without a list (D2566).
    fn tablebase_fail(&mut self, key: &str, error: &TablebaseError) {
        if matches!(
            error,
            TablebaseError::Corrupt { .. } | TablebaseError::Internal | TablebaseError::Network(_)
        ) {
            tracing::warn!(kind = ?std::mem::discriminant(error), "a tablebase query failed");
        }
        let not_covered = error.is_not_covered();
        let tb = &mut self.flow.tablebase;
        tb.issue = Some(issue_of(error));
        tb.resolved = Some(key.to_owned());
        if not_covered {
            tb.shown = None;
        }
        if not_covered && tb.training.is_some() {
            self.tablebase_out_of_range();
        }
    }

    fn tablebase_probed(
        &mut self,
        request: RequestId,
        source: Source,
        result: Result<PositionReport, TablebaseError>,
        effects: &mut Vec<Effect>,
    ) {
        let tb = &mut self.flow.tablebase;
        if tb.in_flight.as_ref().map(|f| f.request) != Some(request) {
            return;
        }
        let Some(flight) = tb.in_flight.take() else {
            return;
        };
        let position = self.session.position();
        let key = position_key(position);
        if flight.key != key {
            // The position moved on: the refresh asks again.
            return;
        }
        match result {
            Ok(report) => {
                let tb = &mut self.flow.tablebase;
                tb.shown = Some(Shown {
                    key: key.clone(),
                    report,
                });
                tb.resolved = Some(key);
                tb.issue = None;
            }
            Err(error) => {
                let fall_back = flight.source == Source::Local
                    && source == Source::Local
                    && error.is_not_covered()
                    && self.prefs.tablebase.online
                    && check_applicable(position, ONLINE_MAX_PIECES).is_ok();
                if fall_back {
                    self.tablebase_start_online(&key, effects);
                } else {
                    self.tablebase_fail(&key, &error);
                }
            }
        }
    }

    fn tablebase_delay_elapsed(
        &mut self,
        request: RequestId,
        kind: DelayKind,
        effects: &mut Vec<Effect>,
    ) {
        match kind {
            DelayKind::Debounce => {
                let position = self.session.position().clone();
                let key = position_key(&position);
                let tb = &mut self.flow.tablebase;
                let Some(flight) = tb.in_flight.as_mut() else {
                    return;
                };
                if flight.request != request || flight.stage != Stage::Debouncing {
                    return;
                }
                if flight.key != key {
                    tb.in_flight = None;
                    return;
                }
                flight.stage = Stage::Probing;
                let handle = tb.online.clone();
                effects.push(
                    TablebaseEffect::ProbeOnline {
                        request,
                        handle,
                        position,
                    }
                    .into_effect(),
                );
                effects.push(watchdog(request, ONLINE_WATCHDOG_MS));
            }
            DelayKind::Watchdog => {
                let key = position_key(self.session.position());
                let tb = &mut self.flow.tablebase;
                if tb.in_flight.as_ref().map(|f| f.request) != Some(request) {
                    return;
                }
                let flight = tb.in_flight.take();
                if flight.is_some_and(|f| f.key == key) {
                    self.tablebase_fail(&key, &TablebaseError::Timeout);
                }
            }
            DelayKind::OpenWatchdog => {
                let tb = &mut self.flow.tablebase;
                if tb.opening.as_ref().map(|(r, _)| *r) != Some(request) {
                    return;
                }
                // Give up: no tables, not asked again until `Reopen`. The
                // late answer finds no open in flight and is ignored.
                let dirs = tb.opening.take().map(|(_, dirs)| dirs);
                tb.local = None;
                tb.report = None;
                tb.opened_for = dirs;
                let key = position_key(self.session.position());
                self.tablebase_fail(&key, &TablebaseError::Timeout);
            }
            DelayKind::Reply => {
                let Some(t) = self.flow.tablebase.training.as_mut() else {
                    return;
                };
                let Some((expected, key)) = t.reply.clone() else {
                    return;
                };
                if expected != request {
                    return;
                }
                t.reply = None;
                let user = t.user;
                let position = self.session.position();
                if position_key(position) != key || position.turn() == user {
                    return;
                }
                if !self.tablebase_at_end_of_line() {
                    self.tablebase_end_training(None);
                    return;
                }
                self.tablebase_reply(effects);
            }
        }
    }
}

fn watchdog(request: RequestId, ms: u32) -> Effect {
    TablebaseEffect::Delay {
        request,
        kind: DelayKind::Watchdog,
        ms,
    }
    .into_effect()
}
