//! The tournament between engines in the state (spec M8-2, phase A).
//!
//! The rules are in [`crate::tournament`] (a pure state machine); this is the
//! glue. The tournament is **private** (D1278): its engines live in an
//! [`EnginePool`], not in the hub, and it never touches the game of the
//! editor. The analyses of the user are paused for its length and resumed at
//! the end. Nothing here blocks: every wait is a message answering an
//! [`Effect`].

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::AppState;
use crate::effect::Effect;
use crate::engines::{
    EngineEntry, EngineFailure, EngineId, EngineRunState, FailureKind, Protocol, RunId,
};
use crate::flow::AppNotice;
use crate::shared::SharedGame;
use crate::tournament::{
    Action, EnginePool, ExportFormat, ExportLabels, ExportTable, ExtraDestination, LaunchRequest,
    Loaded, ParallelLimits, Phase, PoolCommand, PrepareFailure, PreparePlan, SavedTournament,
    StoreFailure, SystemResources, TimeControl, Tournament, TournamentConfig, TournamentMessage,
    TournamentNotice, export_cross, export_ratings, export_standings, export_time, pgn_path,
    state_path,
};

/// The least time between two `info` reports of an engine of a tournament:
/// nobody watches them, so they cost nothing but CPU (D1288).
const INFO_INTERVAL: Duration = Duration::from_millis(250);
/// The most lines of the protocol log of an engine of a tournament.
const LOG_LINES: usize = 2000;

/// A tournament that is being prepared.
#[derive(Debug)]
struct Loading {
    request: u64,
    config: TournamentConfig,
    /// The saved state of a tournament that is being resumed.
    saved: Option<Box<SavedTournament>>,
    /// The slot the games are copied to, as it was when the tournament was
    /// asked for (D2519).
    dest: Option<(crate::bases::SlotId, crate::bases::SharedSource)>,
    /// The answer of the load, kept until the machine has been read: the
    /// lanes depend on it (D2501).
    waiting: Option<(Box<Result<Loaded, crate::tournament::LoadFailure>>, Instant)>,
}

/// A tournament that exists.
#[derive(Debug)]
struct Live {
    tournament: Box<Tournament>,
    pool: EnginePool,
    dir: PathBuf,
    /// The instant of the last message that carried one.
    at: Instant,
}

#[derive(Debug)]
enum Active {
    /// Reading the saved state (resume).
    ReadingState {
        request: u64,
        replacements: Vec<(usize, u64)>,
        /// The folder of the state file: the PGN is next to it (D1304).
        dir: Option<PathBuf>,
    },
    /// Preparing the book, the suite and the files.
    Loading(Box<Loading>),
    /// Playing, paused or over.
    Live(Live),
}

/// The instant a message carries, if it carries one.
fn message_time(message: &TournamentMessage) -> Option<Instant> {
    match message {
        TournamentMessage::Start { at, .. }
        | TournamentMessage::Resume { at, .. }
        | TournamentMessage::Loaded { at, .. }
        | TournamentMessage::StateLoaded { at, .. }
        | TournamentMessage::Prepared { at, .. }
        | TournamentMessage::Searched { at, .. }
        | TournamentMessage::Continue(at)
        | TournamentMessage::StopAfterGame(at)
        | TournamentMessage::Abort(at)
        | TournamentMessage::Adjudicate(_, at)
        | TournamentMessage::SkipCurrent(at)
        | TournamentMessage::Decide(_, at) => Some(*at),
        _ => None,
    }
}

/// The number in the option `name` (any case) of `options`, if there is one.
fn option_u32(options: &[(String, String)], name: &str) -> Option<u32> {
    options
        .iter()
        .rev()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .and_then(|(_, value)| value.trim().parse().ok())
}

/// Whether the message comes from the user acting on the tournament (as
/// opposed to the answers of tasks that were in flight).
fn is_user_command(message: &TournamentMessage) -> bool {
    matches!(
        message,
        TournamentMessage::Start { .. }
            | TournamentMessage::Resume { .. }
            | TournamentMessage::Pause
            | TournamentMessage::Continue(_)
            | TournamentMessage::StopAfterGame(_)
            | TournamentMessage::Abort(_)
            | TournamentMessage::Adjudicate(..)
            | TournamentMessage::SkipCurrent(_)
            | TournamentMessage::SkipPending(_)
            | TournamentMessage::Redo(_)
            | TournamentMessage::MoveToFront(_)
            | TournamentMessage::Decide(..)
            | TournamentMessage::Close
    )
}

/// The bookkeeping of the tournament.
#[derive(Debug, Default)]
pub(crate) struct TournamentFlow {
    active: Option<Active>,
    request_seq: u64,
    /// The analyses stopped for the tournament (D1278).
    pub(crate) paused: Vec<RunId>,
    /// The tournaments that can be resumed (the last listing).
    pub(crate) resumable: Vec<crate::tournament::ResumableTournament>,
    /// Tests put the directory of the files here.
    pub(crate) dir_override: Option<PathBuf>,
    /// The last reading of the machine (D2502).
    pub(crate) resources: Option<SystemResources>,
    /// The extra destination of the games (D2517).
    pub(crate) dest: super::tournament_dest::DestFlow,
    /// Writes of the PGN and of the state asked and not answered yet: the
    /// quit waits for them (D2528).
    writes: u32,
    /// The quit is waiting for `writes` to reach zero.
    pub(super) quit_wait: bool,
    /// The program is quitting (D2533): the engines were cancelled and the
    /// pool shut down, so a late `Searched`/`Prepared` must not reopen
    /// engines or record a game.
    pub(super) quitting: bool,
}

impl AppState {
    // ---- reading ----

    /// The tournament, once it is created (also after it ended, until the
    /// view is closed).
    pub fn tournament(&self) -> Option<&Tournament> {
        match self.flow.tournament.active.as_ref()? {
            Active::Live(live) => Some(&live.tournament),
            _ => None,
        }
    }

    /// Whether a tournament is being prepared or is on (playing, paused,
    /// waiting for a decision or stopped after a game). While it is, a game
    /// against an engine, an annotation and a training mode cannot start
    /// (D1278, D1311). A tournament stopped after a game keeps the
    /// exclusivity: its engines are closed but it goes on with "Continue",
    /// and nothing may take the game in between.
    pub fn tournament_live(&self) -> bool {
        match self.flow.tournament.active.as_ref() {
            None => false,
            Some(Active::ReadingState { .. } | Active::Loading(_)) => true,
            Some(Active::Live(live)) => matches!(
                live.tournament.phase(),
                Phase::Running
                    | Phase::Pausing
                    | Phase::Paused
                    | Phase::Decision(_)
                    | Phase::StoppedAfterGame
            ),
        }
    }

    /// The tournaments that can be resumed, from the last
    /// [`TournamentMessage::ListResumable`].
    pub fn tournament_resumable(&self) -> &[crate::tournament::ResumableTournament] {
        &self.flow.tournament.resumable
    }

    /// The text of a table of the tournament, for the clipboard or a file
    /// (D1287); `None` without a tournament.
    pub fn tournament_export_text(
        &self,
        table: ExportTable,
        format: ExportFormat,
        labels: &ExportLabels,
    ) -> Option<String> {
        let tournament = self.tournament()?;
        let names = tournament.config().names();
        let rows = tournament.standings();
        Some(match table {
            ExportTable::Standings => export_standings(&names, &rows, labels, format),
            ExportTable::Cross => {
                export_cross(&names, &tournament.cross_table(), &rows, labels, format)
            }
            ExportTable::Ratings => export_ratings(&names, &tournament.ratings(), labels, format),
            ExportTable::Time => export_time(&names, &tournament.time_report(), labels, format),
        })
    }

    /// Whether the adjudication by tablebase can be offered (D2520): the
    /// user has local Syzygy folders turned on. Without them the rule is
    /// hidden by the form.
    pub fn tournament_tablebase_available(&self) -> bool {
        self.prefs.tablebase.local && !self.prefs.tablebase.directories.is_empty()
    }

    /// The Syzygy folders the load of `config` opens (D2538): the ones of
    /// the preferences when the rule is on, else none.
    fn tournament_tablebase_dirs(&self, config: &TournamentConfig) -> Vec<PathBuf> {
        if config.adjudication.tablebase && self.tournament_tablebase_available() {
            self.prefs.tablebase.paths()
        } else {
            Vec::new()
        }
    }

    /// What the machine offers, as last read; the cautious default before
    /// the first reading (D2502).
    pub fn tournament_resources(&self) -> SystemResources {
        self.flow.tournament.resources.unwrap_or_default()
    }

    /// The games at once that fit the machine for `config` (D2501). The
    /// engines count at their heaviest: the largest `Hash` and `Threads`
    /// among the participants, as each would really start (its own options,
    /// else the uniform ones of the tournament, else the ones saved with the
    /// engine).
    pub fn tournament_limits(&self, config: &TournamentConfig) -> ParallelLimits {
        let (mut hash, mut threads): (Option<u32>, Option<u32>) = (None, None);
        for spec in &config.participants {
            let saved: &[(String, String)] = self
                .engines
                .get(EngineId(spec.engine))
                .map_or(&[], |entry| entry.options.as_slice());
            let effective = |name: &str, uniform: Option<u32>| {
                option_u32(&spec.options, name)
                    .or(uniform)
                    .or_else(|| option_u32(saved, name))
            };
            hash = hash.max(effective("Hash", config.hash_mb));
            threads = threads.max(effective("Threads", config.threads));
        }
        ParallelLimits::compute(self.tournament_resources(), threads, hash, config.ponder)
    }

    /// The games at once that really run: what the configuration asks,
    /// brought down to what the machine takes. The configuration is **not**
    /// changed, so a saved tournament keeps what the user asked for and a
    /// resume on a bigger machine runs more games (D2503, D2509). Returns the
    /// lanes and `(asked, set)` when they differ.
    fn tournament_lanes(&self, config: &TournamentConfig) -> (usize, Option<(u32, u32)>) {
        let limits = self.tournament_limits(config);
        let asked = config.concurrency;
        let set = limits.clamp(asked);
        (
            usize::try_from(set).unwrap_or(1),
            (set != asked).then_some((asked, set)),
        )
    }

    fn tournament_notice(&mut self, notice: TournamentNotice) {
        self.notice = Some(AppNotice::Tournament(notice));
    }

    // ---- messages ----

    /// Opens the PGN of the running tournament as a database.
    fn tournament_open_pgn(&mut self, effects: &mut Vec<Effect>) {
        if self.modal_open() || self.refuse_edit() {
            return;
        }
        let path = match self.flow.tournament.active.as_ref() {
            Some(Active::Live(live)) => Some(pgn_path(&live.dir, live.tournament.slug())),
            _ => None,
        };
        match path {
            Some(path) => self.open_path(path, effects),
            None => self.tournament_notice(TournamentNotice::NotApplicable),
        }
    }

    #[allow(clippy::too_many_lines)] // one flat match over the messages reads best
    pub(crate) fn tournament_message(
        &mut self,
        message: TournamentMessage,
        effects: &mut Vec<Effect>,
    ) {
        if let Some(at) = message_time(&message)
            && let Some(Active::Live(live)) = self.flow.tournament.active.as_mut()
        {
            live.at = at;
        }
        // The user acts on a tournament whose engines the quit ended, so the
        // quit did not exit: undo it first (D2538).
        if self.flow.tournament.quitting
            && !self.flow.tournament.quit_wait
            && is_user_command(&message)
        {
            self.tournament_quit_undone(effects);
            if matches!(message, TournamentMessage::Abort(_)) {
                return;
            }
        }
        match message {
            TournamentMessage::Start { config, .. } => self.tournament_start(*config, effects),
            TournamentMessage::Resume {
                path, replacements, ..
            } => self.tournament_resume(path, replacements, effects),
            TournamentMessage::ListResumable => effects.push(Effect::TournamentList {
                dir: self.flow.tournament.dir_override.clone(),
            }),
            TournamentMessage::Resumable(list) => self.flow.tournament.resumable = list,
            TournamentMessage::StateLoaded {
                request,
                result,
                at,
            } => self.tournament_state_loaded(request, *result, at, effects),
            TournamentMessage::Loaded {
                request,
                result,
                at,
            } => self.tournament_loaded(request, *result, at, effects),
            TournamentMessage::Prepared { .. } | TournamentMessage::Searched { .. }
                if self.flow.tournament.quitting => {}
            TournamentMessage::Prepared { epoch, result, at } => {
                self.tournament_run(effects, |t| t.prepared(epoch, result, at));
            }
            TournamentMessage::Searched { epoch, outcome, at } => {
                self.tournament_run(effects, |t| t.searched(epoch, outcome, at));
            }
            TournamentMessage::GameSaved { ok, .. } => {
                self.tournament_write_done(effects);
                if !ok {
                    self.tournament_notice(TournamentNotice::GameNotSaved);
                }
            }
            TournamentMessage::StateSaved { ok } => {
                self.tournament_write_done(effects);
                if !ok {
                    self.tournament_notice(TournamentNotice::StateNotSaved);
                }
            }
            TournamentMessage::PoolDone => {}
            TournamentMessage::ProbeResources => effects.push(Effect::TournamentProbe),
            TournamentMessage::Resources(resources) => {
                self.flow.tournament.resources = Some(resources);
                let waiting = match self.flow.tournament.active.as_mut() {
                    Some(Active::Loading(loading)) => {
                        loading.waiting.take().map(|w| (loading.request, w))
                    }
                    _ => None,
                };
                if let Some((request, (result, at))) = waiting {
                    self.tournament_loaded(request, *result, at, effects);
                }
            }
            TournamentMessage::SlotAdded { result } => {
                self.tournament_dest_written(*result, effects);
                self.tournament_quit_ready(effects);
            }
            TournamentMessage::Watch(lane) => {
                if let Some(Active::Live(live)) = self.flow.tournament.active.as_mut() {
                    live.tournament.watch(lane);
                }
            }
            TournamentMessage::Pause => self.tournament_run(effects, Tournament::pause),
            TournamentMessage::Continue(at) => {
                if self.play_live()
                    || self.annotation_running()
                    || self.training_playing()
                    || self.epd_running()
                {
                    return self.tournament_notice(TournamentNotice::PlayBusy);
                }
                if self.tournament_stopped() {
                    self.tournament_pause_analyses(effects);
                }
                self.tournament_run(effects, |t| t.resume_play(at));
            }
            TournamentMessage::StopAfterGame(at) => {
                self.tournament_run(effects, |t| t.stop_after_game(at));
            }
            TournamentMessage::Abort(at) => {
                // A tournament still preparing has nothing to abort: drop it,
                // so that its late `Loaded`/`StateLoaded` is ignored (D1362).
                if matches!(
                    self.flow.tournament.active,
                    Some(Active::Loading(_) | Active::ReadingState { .. })
                ) {
                    self.flow.tournament.active = None;
                } else if self.flow.tournament.quitting {
                    // The pool is closed: nothing would answer the cancel.
                    self.tournament_run(effects, Tournament::abort_now);
                } else {
                    self.tournament_run(effects, |t| t.abort(at));
                }
            }
            TournamentMessage::Adjudicate(result, at) => {
                self.tournament_run(effects, |t| t.adjudicate(result, at));
            }
            TournamentMessage::SkipCurrent(at) => {
                self.tournament_run(effects, |t| t.skip_current(at));
            }
            TournamentMessage::SkipPending(number) => {
                self.tournament_run(effects, |t| t.skip_pending(number));
            }
            TournamentMessage::Redo(number) => self.tournament_run(effects, |t| t.redo(number)),
            TournamentMessage::MoveToFront(number) => {
                self.tournament_run(effects, |t| t.move_to_front(number));
            }
            TournamentMessage::Decide(decision, at) => {
                self.tournament_run(effects, |t| t.decide(decision, at));
            }
            TournamentMessage::OpenPgn => self.tournament_open_pgn(effects),
            TournamentMessage::Close => self.tournament_close(effects),
            TournamentMessage::Export {
                table,
                format,
                labels,
                path,
            } => match self.tournament_export_text(table, format, &labels) {
                Some(text) => effects.push(Effect::TournamentExport { path, text }),
                None => self.tournament_notice(TournamentNotice::NotApplicable),
            },
            TournamentMessage::Exported { ok } => {
                self.tournament_notice(if ok {
                    TournamentNotice::Exported
                } else {
                    TournamentNotice::ExportFailed
                });
            }
        }
    }

    /// The tournament stopped after a game (its engines are closed).
    fn tournament_stopped(&self) -> bool {
        self.tournament()
            .is_some_and(|t| t.phase() == Phase::StoppedAfterGame)
    }

    // ---- start and resume ----

    /// Whether a new tournament can replace what is there.
    fn tournament_can_replace(&self) -> bool {
        match self.flow.tournament.active.as_ref() {
            None => true,
            Some(Active::Live(live)) => {
                matches!(live.tournament.phase(), Phase::Finished | Phase::Aborted)
            }
            Some(_) => false,
        }
    }

    fn tournament_precheck(&mut self) -> bool {
        if !self.tournament_can_replace() {
            self.tournament_notice(TournamentNotice::AlreadyRunning);
            return false;
        }
        if self.play_live() {
            self.tournament_notice(TournamentNotice::PlayBusy);
            return false;
        }
        // The annotation and the training modes command the game and an
        // engine of the hub (D1311).
        if self.annotation_running() || self.training_playing() || self.epd_running() {
            self.tournament_notice(TournamentNotice::Busy);
            return false;
        }
        // A training session or the calculation exercise holds the game of
        // the user aside: it ends first and the game comes back (D2358).
        self.tactics_halt();
        true
    }

    /// Checks that every engine exists and can play the time control: UCI
    /// and XBoard both play (D2521), but the CECP has no node limit (D2522).
    fn tournament_check_engines(&mut self, config: &TournamentConfig) -> bool {
        let mut missing = Vec::new();
        for participant in &config.participants {
            match self.engines.get(EngineId(participant.engine)) {
                None => missing.push(participant.name.clone()),
                Some(entry)
                    if entry.protocol == Protocol::XBoard
                        && matches!(config.time, TimeControl::Nodes(_)) =>
                {
                    self.tournament_notice(TournamentNotice::UnsupportedTime(
                        participant.name.clone(),
                    ));
                    return false;
                }
                Some(_) => {}
            }
        }
        if missing.is_empty() {
            true
        } else {
            self.tournament_notice(TournamentNotice::MissingEngines(missing));
            false
        }
    }

    fn tournament_start(&mut self, mut config: TournamentConfig, effects: &mut Vec<Effect>) {
        if !self.tournament_precheck() {
            return;
        }
        if let Err(error) = config.validate() {
            return self.tournament_notice(TournamentNotice::InvalidConfig(error));
        }
        if !self.tournament_check_engines(&config) {
            return;
        }
        for participant in &mut config.participants {
            if let Some(entry) = self.engines.get(EngineId(participant.engine)) {
                participant.engine_name.clone_from(&entry.name);
                if participant.elo.is_none() {
                    participant.elo = entry.elo;
                }
            }
        }
        self.flow.tournament.request_seq += 1;
        self.flow.tournament.quitting = false;
        let request = self.flow.tournament.request_seq;
        effects.push(Effect::TournamentLoad {
            request,
            config: Box::new(config.clone()),
            slug: None,
            dir: self.flow.tournament.dir_override.clone(),
            tablebase_dirs: self.tournament_tablebase_dirs(&config),
        });
        self.tournament_probe_if_unknown(effects);
        let dest = self.tournament_dest_capture(config.destination);
        self.flow.tournament.active = Some(Active::Loading(Box::new(Loading {
            request,
            config,
            saved: None,
            dest,
            waiting: None,
        })));
    }

    /// Reads the machine when it has not been read yet: the lanes of the
    /// tournament wait for the answer instead of guessing the cores (D2501).
    fn tournament_probe_if_unknown(&self, effects: &mut Vec<Effect>) {
        if self.flow.tournament.resources.is_none() {
            effects.push(Effect::TournamentProbe);
        }
    }

    fn tournament_resume(
        &mut self,
        path: PathBuf,
        replacements: Vec<(usize, u64)>,
        effects: &mut Vec<Effect>,
    ) {
        if !self.tournament_precheck() {
            return;
        }
        self.flow.tournament.request_seq += 1;
        self.flow.tournament.quitting = false;
        let request = self.flow.tournament.request_seq;
        let dir = path.parent().map(|d| {
            if d.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                d.to_path_buf()
            }
        });
        effects.push(Effect::TournamentLoadState { request, path });
        self.tournament_probe_if_unknown(effects);
        self.flow.tournament.active = Some(Active::ReadingState {
            request,
            replacements,
            dir,
        });
    }

    fn tournament_state_loaded(
        &mut self,
        request: u64,
        result: Result<SavedTournament, StoreFailure>,
        _at: Instant,
        effects: &mut Vec<Effect>,
    ) {
        if self.assist_locked() {
            self.flow.tournament.active = None;
            return;
        }
        let Some(Active::ReadingState {
            request: current,
            replacements,
            dir,
        }) = self.flow.tournament.active.as_ref()
        else {
            return;
        };
        if *current != request {
            return;
        }
        let (replacements, dir) = (replacements.clone(), dir.clone());
        self.flow.tournament.active = None;
        self.flow.tournament.quitting = false;
        let Ok(mut saved) = result else {
            return self.tournament_notice(TournamentNotice::ResumeFailed);
        };
        // The engines are found again by identity and, failing that, by
        // name (D1285).
        let mut missing = Vec::new();
        for (i, participant) in saved.config.participants.iter_mut().enumerate() {
            let by_id = self.engines.get(EngineId(participant.engine)).map(|e| e.id);
            let by_name = || {
                self.engines
                    .engines()
                    .iter()
                    .find(|e| e.name.eq_ignore_ascii_case(&participant.engine_name))
                    .map(|e| e.id)
            };
            let replacement = || {
                replacements
                    .iter()
                    .find(|(index, _)| *index == i)
                    .and_then(|(_, id)| self.engines.get(EngineId(*id)).map(|e| e.id))
            };
            match by_id.or_else(by_name).or_else(replacement) {
                Some(id) => participant.engine = id.0,
                None => missing.push(participant.name.clone()),
            }
        }
        if !missing.is_empty() {
            return self.tournament_notice(TournamentNotice::MissingEngines(missing));
        }
        if !self.tournament_check_engines(&saved.config) {
            return;
        }
        self.flow.tournament.request_seq += 1;
        let request = self.flow.tournament.request_seq;
        effects.push(Effect::TournamentLoad {
            request,
            config: Box::new(saved.config.clone()),
            slug: Some(saved.slug.clone()),
            // The files are where the state was read from, not where the
            // preferences point (D1304).
            dir,
            tablebase_dirs: self.tournament_tablebase_dirs(&saved.config),
        });
        let resumable_destination = match saved.config.destination {
            ExtraDestination::Clipbase => ExtraDestination::Clipbase,
            _ => ExtraDestination::None,
        };
        self.flow.tournament.active = Some(Active::Loading(Box::new(Loading {
            request,
            config: saved.config.clone(),
            // Only the Clipbase can be found again: "the current slot" of a
            // tournament that is resumed is not the one it started with
            // (D2519).
            dest: self.tournament_dest_capture(resumable_destination),
            saved: Some(Box::new(saved)),
            waiting: None,
        })));
    }

    fn tournament_loaded(
        &mut self,
        request: u64,
        result: Result<Loaded, crate::tournament::LoadFailure>,
        at: Instant,
        effects: &mut Vec<Effect>,
    ) {
        if self.assist_locked() {
            self.flow.tournament.active = None;
            return;
        }
        let Some(Active::Loading(loading)) = self.flow.tournament.active.as_ref() else {
            return;
        };
        if loading.request != request {
            return;
        }
        if self.flow.tournament.resources.is_none() {
            // The machine has not been read: wait for it.
            if let Some(Active::Loading(loading)) = self.flow.tournament.active.as_mut() {
                loading.waiting = Some((Box::new(result), at));
            }
            return;
        }
        let Some(Active::Loading(loading)) = self.flow.tournament.active.take() else {
            return;
        };
        let loaded = match result {
            Ok(loaded) => loaded,
            Err(crate::tournament::LoadFailure::Config(error)) => {
                return self.tournament_notice(TournamentNotice::InvalidConfig(error));
            }
            Err(crate::tournament::LoadFailure::Suite) => {
                return self.tournament_notice(TournamentNotice::SuiteUnreadable);
            }
            Err(crate::tournament::LoadFailure::Directory) => {
                return self.tournament_notice(TournamentNotice::StateNotSaved);
            }
        };
        if loaded.book_unreadable {
            self.tournament_notice(TournamentNotice::BookUnreadable);
        }
        let Loading {
            mut config,
            saved,
            dest,
            ..
        } = *loading;
        let (lanes, reduced) = self.tournament_lanes(&config);
        let built = if let Some(saved) = saved {
            let saved = *saved;
            Tournament::resume(saved, loaded.book, loaded.suite)
                .map(|t| (t, true))
                .map_err(|_| TournamentNotice::ResumeFailed)
        } else {
            config.seed = loaded.seed;
            Tournament::new(
                config,
                &loaded.slug,
                loaded.today,
                loaded.book,
                loaded.suite,
            )
            .map(|t| (t, false))
            .map_err(TournamentNotice::InvalidConfig)
        };
        let (mut tournament, resumed) = match built {
            Ok(pair) => pair,
            Err(notice) => return self.tournament_notice(notice),
        };
        tournament.set_lanes(lanes);
        // The notice waits for the start, which has its own notice (D2538).
        let mut tablebase_missing = false;
        if tournament.config().adjudication.tablebase {
            match loaded.tablebase {
                Some(tablebase) => tournament.set_tablebase(Arc::new(tablebase)),
                None => tablebase_missing = true,
            }
        }
        let destination = tournament.config().destination;
        let pool = EnginePool::new(self.engine_hub.launcher());
        // A new pool: a quit that was called off must not mute it.
        self.flow.tournament.quitting = false;
        self.flow.tournament.active = Some(Active::Live(Live {
            tournament: Box::new(tournament),
            pool,
            dir: loaded.dir,
            at,
        }));
        self.tournament_pause_analyses(effects);
        if resumed {
            self.tournament_run(effects, |t| t.resume_play(at));
        } else {
            self.tournament_run(effects, |t| t.begin(at));
        }
        if tablebase_missing {
            self.tournament_notice(TournamentNotice::TablebaseUnavailable);
        }
        if let Some((from, to)) = reduced {
            self.tournament_notice(TournamentNotice::ConcurrencyReduced { from, to });
        }
        // After the start, so that a refused destination is the remark that
        // stays (D2517).
        self.tournament_dest_setup(destination, dest);
    }

    /// Closes the view of the tournament. A tournament that is paused is
    /// over for the engines: they are ended and the analyses of the user come
    /// back (D1307).
    fn tournament_close(&mut self, effects: &mut Vec<Effect>) {
        let over = self
            .flow
            .tournament
            .active
            .as_ref()
            .is_none_or(|a| match a {
                Active::Live(live) => !matches!(
                    live.tournament.phase(),
                    Phase::Running | Phase::Pausing | Phase::Decision(_)
                ),
                _ => false,
            });
        if !over {
            return;
        }
        if let Some(Active::Live(live)) = self.flow.tournament.active.as_ref()
            && live.tournament.phase() == Phase::Paused
        {
            effects.push(Effect::TournamentPool {
                pool: live.pool.clone(),
                command: PoolCommand::Cancel(live.tournament.epoch()),
            });
            effects.push(Effect::TournamentPool {
                pool: live.pool.clone(),
                command: PoolCommand::Shutdown,
            });
        }
        self.flow.tournament.active = None;
        self.tournament_resume_analyses(effects);
    }

    /// Saves the state and ends the engines when the program quits (D1284).
    pub(crate) fn tournament_quit(&mut self, effects: &mut Vec<Effect>) {
        // A tournament still preparing is dropped: its late `Loaded` must not
        // open engines in a program that is leaving (D2538).
        if matches!(
            self.flow.tournament.active,
            Some(Active::Loading(_) | Active::ReadingState { .. })
        ) {
            self.flow.tournament.active = None;
            self.flow.tournament.dest.drop_queue();
            return;
        }
        let Some(Active::Live(live)) = self.flow.tournament.active.as_ref() else {
            // No tournament: games still queued for the PGN do not wait for
            // the exit (D2379).
            self.flow.tournament.dest.drop_queue();
            return;
        };
        self.flow.tournament.quitting = true;
        // The games waiting for the slot stay out: the PGN has them, and the
        // write in flight is waited for (D2533).
        self.flow.tournament.dest.drop_queue();
        effects.push(Effect::TournamentSaveState {
            path: state_path(&live.dir, live.tournament.slug()),
            state: Box::new(live.tournament.to_saved()),
        });
        self.flow.tournament.writes += 1;
        // A search in flight is cancelled first: it comes back as
        // `Cancelled`, and the pool (closed by the shutdown) ends its engine
        // instead of keeping it (D1307).
        effects.push(Effect::TournamentPool {
            pool: live.pool.clone(),
            command: PoolCommand::Cancel(live.tournament.epoch()),
        });
        effects.push(Effect::TournamentPool {
            pool: live.pool.clone(),
            command: PoolCommand::Shutdown,
        });
    }

    /// The quit did not end the program (the question about unsaved changes
    /// was cancelled, or the user went on with the tournament): its work is
    /// undone as far as it can be (D2538). The engines are gone and the pool
    /// is closed, so the tournament cannot go on in place: it ends as an
    /// aborted one, its games in progress back in the queue and its state
    /// saved, and a resume (or "New") takes it from there.
    pub(crate) fn tournament_quit_undone(&mut self, effects: &mut Vec<Effect>) {
        let t = &mut self.flow.tournament;
        if !t.quitting {
            return;
        }
        t.quitting = false;
        t.quit_wait = false;
        self.tournament_run(effects, Tournament::abort_now);
    }

    // ---- writes and quit (D2528) ----

    /// A write of the PGN or of the state was answered.
    fn tournament_write_done(&mut self, effects: &mut Vec<Effect>) {
        let t = &mut self.flow.tournament;
        t.writes = t.writes.saturating_sub(1);
        self.tournament_quit_ready(effects);
    }

    /// Finishes the quit when it was waiting and nothing is in flight any
    /// more: no write of the PGN or of the state, no write to the slot.
    fn tournament_quit_ready(&mut self, effects: &mut Vec<Effect>) {
        let t = &mut self.flow.tournament;
        if t.writes == 0 && !t.dest.is_busy() && std::mem::take(&mut t.quit_wait) {
            self.finish_quit(effects);
        }
    }

    /// Whether the quit of the tournament is going on (its engines were
    /// ended or its writes are waited for).
    pub(super) fn tournament_quit_pending(&self) -> bool {
        self.flow.tournament.quitting || self.flow.tournament.quit_wait
    }

    /// Part of quitting: the writes in flight finish first (a game appended
    /// to the PGN and the state must reach the disk). Returns whether the
    /// quit must wait; the answer of the last write calls `finish_quit`
    /// again, or the deadline gives the writes up.
    pub(crate) fn tournament_quit_wait(&mut self, effects: &mut Vec<Effect>) -> bool {
        let t = &mut self.flow.tournament;
        if t.writes == 0 && !t.dest.is_busy() {
            return false;
        }
        if !t.quit_wait {
            t.quit_wait = true;
            effects.push(Effect::QuitDeadline {
                delay: Duration::from_secs(5),
            });
        }
        true
    }

    /// The deadline of [`AppState::tournament_quit_wait`] ran out. Returns
    /// whether the exit was waiting.
    pub(crate) fn tournament_quit_deadline(&mut self) -> bool {
        let t = &mut self.flow.tournament;
        if !std::mem::take(&mut t.quit_wait) {
            return false;
        }
        t.writes = 0;
        t.dest.give_up();
        true
    }

    // ---- analyses (D1278) ----

    fn tournament_pause_analyses(&mut self, effects: &mut Vec<Effect>) {
        if !self.flow.tournament.paused.is_empty() {
            return;
        }
        let runs: Vec<RunId> = self
            .engine_runs
            .as_slice()
            .iter()
            .filter(|r| r.state == EngineRunState::Running && r.analyzing)
            .map(|r| r.id)
            .collect();
        for run in &runs {
            self.analysis_toggle(*run, effects);
        }
        self.flow.tournament.paused = runs;
    }

    fn tournament_resume_analyses(&mut self, effects: &mut Vec<Effect>) {
        for run in std::mem::take(&mut self.flow.tournament.paused) {
            let stopped = self
                .engine_runs
                .get(run)
                .is_some_and(|r| r.state == EngineRunState::Running && !r.analyzing);
            if stopped {
                self.analysis_toggle(run, effects);
            }
        }
    }

    // ---- the actions of the state machine ----

    /// Runs `f` on the tournament and carries out what it asks.
    fn tournament_run(
        &mut self,
        effects: &mut Vec<Effect>,
        f: impl FnOnce(&mut Tournament) -> Vec<Action>,
    ) {
        let Some(Active::Live(live)) = self.flow.tournament.active.as_mut() else {
            return;
        };
        let actions = f(&mut live.tournament);
        self.tournament_actions(actions, effects);
    }

    fn tournament_actions(&mut self, actions: Vec<Action>, effects: &mut Vec<Effect>) {
        let mut queue: std::collections::VecDeque<Action> = actions.into();
        while let Some(action) = queue.pop_front() {
            let Some(Active::Live(live)) = self.flow.tournament.active.as_ref() else {
                return;
            };
            let pool = live.pool.clone();
            match action {
                Action::Prepare {
                    lane,
                    sides,
                    restart,
                    prune,
                    epoch,
                } => match self.tournament_plan(lane, sides, restart, prune) {
                    Ok(plan) => effects.push(Effect::TournamentPrepare {
                        pool,
                        plan: Box::new(plan),
                        epoch,
                    }),
                    Err(participant) => {
                        // The engine left the list while the tournament
                        // ran: it fails to start like any other.
                        let failure = PrepareFailure {
                            participant,
                            failure: EngineFailure {
                                kind: FailureKind::CannotStart,
                                exit_code: None,
                                stderr_tail: Vec::new(),
                            },
                        };
                        if let Some(Active::Live(live)) = self.flow.tournament.active.as_mut() {
                            let at = live.at;
                            queue.extend(live.tournament.prepared(epoch, Err(failure), at));
                        }
                    }
                },
                Action::Search(job) => effects.push(Effect::TournamentSearch { pool, job }),
                Action::Cancel { lane, epoch } => effects.push(Effect::TournamentPool {
                    pool,
                    command: PoolCommand::CancelLane { lane, epoch },
                }),
                Action::StopPonder { lane } => effects.push(Effect::TournamentPool {
                    pool,
                    command: PoolCommand::StopPonderLane(lane),
                }),
                Action::GameOver {
                    lane,
                    participants,
                    result,
                    reason,
                } => effects.push(Effect::TournamentPool {
                    pool,
                    command: PoolCommand::GameOver {
                        lane,
                        participants,
                        result,
                        reason,
                    },
                }),
                Action::Discard(participant) => effects.push(Effect::TournamentPool {
                    pool,
                    command: PoolCommand::Discard(participant),
                }),
                Action::Shutdown => {
                    effects.push(Effect::TournamentPool {
                        pool,
                        command: PoolCommand::Shutdown,
                    });
                    self.tournament_resume_analyses(effects);
                }
                Action::SaveGame { number, game } => {
                    let path = pgn_path(&live.dir, live.tournament.slug());
                    // The PGN first; the extra destination is the copy
                    // (D2517).
                    effects.push(Effect::TournamentSaveGame {
                        path,
                        number,
                        game: SharedGame::new((*game).clone()),
                    });
                    self.flow.tournament.writes += 1;
                    self.tournament_dest_add(number, &game, effects);
                }
                Action::SaveState(state) => {
                    let path = state_path(&live.dir, live.tournament.slug());
                    effects.push(Effect::TournamentSaveState { path, state });
                    self.flow.tournament.writes += 1;
                }
                Action::Notice(notice) => self.tournament_notice(notice),
            }
        }
    }

    /// How to start the engines of a game (D1263, D1288). `Err` is the
    /// participant whose engine is gone from the list.
    fn tournament_plan(
        &self,
        lane: usize,
        sides: [usize; 2],
        restart: [bool; 2],
        prune: bool,
    ) -> Result<PreparePlan, usize> {
        let Some(Active::Live(live)) = self.flow.tournament.active.as_ref() else {
            return Err(sides[0]);
        };
        let config = live.tournament.config();
        let mut defaults = self.prefs.analysis.run_defaults();
        defaults.info_interval = defaults.info_interval.max(INFO_INTERVAL);
        defaults.log_lines = defaults.log_lines.min(LOG_LINES);
        let launch = |participant: usize| -> Result<LaunchRequest, usize> {
            let spec = config.participants.get(participant).ok_or(participant)?;
            let entry: EngineEntry = self
                .engines
                .get(EngineId(spec.engine))
                .cloned()
                .ok_or(participant)?;
            // The uniform options first, then the own ones of the
            // participant, which win (D1288).
            let mut options: Vec<(String, String)> = Vec::new();
            if let Some(hash) = config.hash_mb {
                options.push(("Hash".to_owned(), hash.to_string()));
            }
            if let Some(threads) = config.threads {
                options.push(("Threads".to_owned(), threads.to_string()));
            }
            if let Some(path) = &config.syzygy_path {
                options.push(("SyzygyPath".to_owned(), path.clone()));
            }
            for own in &spec.options {
                options.retain(|(name, _)| !name.eq_ignore_ascii_case(&own.0));
                options.push(own.clone());
            }
            Ok(LaunchRequest {
                participant,
                entry,
                options,
                defaults,
            })
        };
        let name = |participant: usize| {
            config
                .participants
                .get(participant)
                .map(|p| p.name.clone())
                .unwrap_or_default()
        };
        Ok(PreparePlan {
            lane,
            sides,
            restart,
            prune_lane: prune,
            opponents: [name(sides[0]), name(sides[1])],
            launches: [launch(sides[0])?, launch(sides[1])?],
        })
    }
}
