//! The automatic annotation in the state (spec M4-6, phase A).
//!
//! The decisions live in [`crate::annotate`]; this module is the glue: it
//! picks the engine, prepares it, hands the events of its searches to the
//! run, applies the run's edits to the session as **one** undo point (D469)
//! and puts the engine back as it was. Nothing here blocks: every wait is an
//! engine event, and every command to the engine is an [`Effect`].

use mychess_core::{Game, NodeId};
use std::path::PathBuf;
use std::time::Duration;

use mychess_engine::{EngineCommand, EngineEvent, OptionKind, OptionValue, Protocol, SearchId};

use super::AppState;
use crate::annotate::{
    AnnotateCaps, AnnotateDialog, AnnotateMessage, AnnotateNotice, AnnotateOptions, AnnotateReport,
    AnnotationError, AnnotationProgress, AnnotationRun, BatchSave, BatchStep, CLEAR_HASH, Control,
    Informant, Outcome, PositionEval, RunOutput, RunSetup, append_tag, apply_actions,
    is_annotation_tag, plan,
};
use crate::bases::{BaseError, BaseKind, BasesNotice, FileStamp, SlotId};
use crate::effect::Effect;
use crate::engines::{CommandKind, EngineRunState, Hotkey, RunId};
use crate::files::GameOrigin;
use crate::flow::AppNotice;
use crate::shared::SharedGame;

/// The number of lines asked of the engine when exercises are marked
/// (`multiPVCount` 4, `okAnnotation` 1194-1199).
const EXERCISE_LINES: u8 = 4;

/// The number of lines of the game report (D1422).
const REPORT_LINES: u8 = 2;

/// How long past its time limit a search may stay silent before the run
/// gives up on the engine (D895). Any report of the search counts as life.
pub(crate) const STALL_GRACE: Duration = Duration::from_secs(30);

/// How long a search under a depth limit (no time to add the grace to) may
/// stay silent.
pub(crate) const DEPTH_STALL: Duration = Duration::from_secs(120);

/// The absolute ceiling of one search under a depth limit (D900): the
/// engine may report all the time and still be given up after this long.
pub(crate) const DEPTH_CEILING: Duration = Duration::from_secs(600);

/// How long the engine has to answer a `stop` of a cancelled run.
pub(crate) const CANCEL_GRACE: Duration = Duration::from_secs(10);

/// The oldest searches of ended runs kept in `annotate_stale`.
const STALE_LIMIT: usize = 16;

/// Where a run is.
#[derive(Debug)]
pub(crate) enum Stage {
    /// Waiting for the engine to finish its handshake.
    Starting,
    /// Walking the game.
    Running(Box<AnnotationRun>),
}

/// What to put back in the engine when the run ends (D478, rule 14).
#[derive(Debug, Default)]
pub(crate) struct Restore {
    /// Whether the engine was analysing the board.
    analyzing: bool,
    /// The number of lines it had, if the run changed it.
    multipv: Option<u8>,
    /// The saved value of `OwnBook`, if the run turned it off.
    own_book: Option<bool>,
}

/// A run in progress.
#[derive(Debug)]
pub(crate) struct Active {
    stage: Stage,
    engine_run: RunId,
    options: AnnotateOptions,
    informant: Informant,
    /// The game the run works on, and the counter of its content after the
    /// run's own last edit: anything else that moves them stops the run.
    generation: u64,
    expected_changes: u64,
    restore: Restore,
    variations: u32,
    exercises: u32,
    /// The generation of the watchdog timer in flight (D895).
    watch: u64,
    /// Events of the run's searches seen so far, and at the last tick.
    progress: u64,
    seen: u64,
    /// What is left of the absolute deadline of the search in flight, and
    /// the length of the timer in flight (D900). Ticks are the only clock.
    remaining: Duration,
    period: Duration,
    /// The run is the game report (M4-10b): it collects evaluations and
    /// never edits the game.
    pub(crate) report: bool,
    /// The evaluations a report run goes on from (D2110).
    resume: Vec<PositionEval>,
}

impl Active {
    /// The execution whose engine the run uses.
    pub(crate) fn engine_run(&self) -> RunId {
        self.engine_run
    }
}

/// A batch over the filter of a database in progress (phase B, D1520).
#[derive(Debug)]
pub(crate) struct Batch {
    slot: SlotId,
    path: PathBuf,
    /// The games of the filter, ascending, as they were when it started.
    numbers: Vec<u32>,
    /// The game being annotated (index into `numbers`).
    index: usize,
    options: AnnotateOptions,
    informant: Informant,
    /// The engine of the first game, reused for the others.
    engine: Option<RunId>,
    expected_total: u32,
    stamp: Option<FileStamp>,
    /// Games saved, and games skipped for having nothing to annotate.
    saved: u32,
    skipped: u32,
    /// Whether the step in flight saves a game.
    saving: bool,
    pub(crate) cancelled: bool,
}

impl AppState {
    /// Pretends a batch annotation is running (tests of other flows that must
    /// stay silent or inert while one runs).
    #[cfg(test)]
    pub(crate) fn fake_annotation_batch(&mut self) {
        self.flow.annotate_batch = Some(Batch {
            slot: SlotId::CLIPBASE,
            path: PathBuf::new(),
            numbers: Vec::new(),
            index: 0,
            options: AnnotateOptions::default(),
            informant: Informant::default(),
            engine: None,
            expected_total: 0,
            stamp: None,
            saved: 0,
            skipped: 0,
            saving: false,
            cancelled: false,
        });
    }
}

/// The node the run starts from: the cursor, or the start of the game when
/// the cursor is at the end of the main line (`analysis.tcl` 1209-1213).
fn base_node(game: &Game, cursor: NodeId) -> NodeId {
    let leaf = game.children(cursor).map_or(true, <[NodeId]>::is_empty);
    if leaf && (cursor == game.root() || game.mainline().last() == Some(cursor)) {
        game.root()
    } else {
        cursor
    }
}

fn bool_text(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

impl AppState {
    /// Whether an annotation run is in progress (starting its engine
    /// included). While it is, the game cannot be edited (D469).
    pub fn annotation_running(&self) -> bool {
        self.flow.annotate.is_some() || self.flow.annotate_batch.is_some()
    }

    /// Whether the run in progress is the game report (spec M4-10b).
    pub fn report_running(&self) -> bool {
        self.flow.annotate.as_ref().is_some_and(|a| a.report)
    }

    /// The progress of the run, for the status bar; `None` without a run.
    pub fn annotation_progress(&self) -> Option<AnnotationProgress> {
        let batch = self.flow.annotate_batch.as_ref();
        let mut progress = match self.flow.annotate.as_ref() {
            Some(active) => match &active.stage {
                Stage::Running(run) => run.progress(),
                Stage::Starting => AnnotationProgress {
                    done_plies: 0,
                    total_plies: 0,
                    game_index: 1,
                    game_total: 1,
                },
            },
            // Between two games of a batch the bar stays.
            None => AnnotationProgress {
                done_plies: 0,
                total_plies: 0,
                game_index: 1,
                game_total: 1,
            },
        };
        if let Some(batch) = batch {
            let total = u32::try_from(batch.numbers.len()).unwrap_or(u32::MAX);
            progress.game_index = u32::try_from(batch.index.saturating_add(1))
                .unwrap_or(u32::MAX)
                .min(total);
            progress.game_total = total;
        } else if self.flow.annotate.is_none() {
            return None;
        }
        Some(progress)
    }

    /// Whether `run` is the engine of the annotation run.
    pub(crate) fn annotation_uses(&self, run: RunId) -> bool {
        // Between two games of a batch the engine stays reserved (D1555).
        self.flow
            .annotate
            .as_ref()
            .is_some_and(|a| a.engine_run == run)
            || self
                .flow
                .annotate_batch
                .as_ref()
                .is_some_and(|b| b.engine == Some(run))
    }

    /// What the engine of `run` (the default one for `None`) can do for the
    /// annotation (D477): XBoard engines only take a time limit, no missed
    /// mates and no exercises.
    pub fn annotate_caps(&self, run: Option<RunId>) -> AnnotateCaps {
        let run = run.and_then(|id| self.engine_runs.get(id)).or_else(|| {
            self.annotate_default_run()
                .and_then(|id| self.engine_runs.get(id))
        });
        let xboard = run
            .and_then(|r| self.engines.get(r.engine))
            .is_some_and(|e| e.protocol == Protocol::XBoard);
        let multipv = run.and_then(|r| r.identity.as_ref()).is_some_and(|i| {
            i.capabilities
                .multipv
                .as_ref()
                .is_some_and(|m| *m.end() >= 2)
        });
        AnnotateCaps {
            depth: !xboard,
            missed_mates: !xboard,
            exercises: !xboard && multipv,
        }
    }

    /// The first execution in the engine list that is ready.
    fn annotate_default_run(&self) -> Option<RunId> {
        self.engines.engines().iter().find_map(|entry| {
            self.engine_runs
                .of_engine(entry.id)
                .filter(|r| r.state == EngineRunState::Running && !self.play_uses(r.id))
                .map(|r| r.id)
        })
    }

    pub(crate) fn annotate_notice(&mut self, notice: AnnotateNotice) {
        self.notice = Some(AppNotice::Annotate(notice));
    }

    fn annotate_send(&self, run: RunId, command: EngineCommand, effects: &mut Vec<Effect>) {
        effects.push(Effect::EngineCommand {
            run,
            command: Box::new(command),
            hub: self.engine_hub.clone(),
        });
    }

    /// How many moves a run with `options` would annotate from the cursor.
    fn annotate_moves(&self, options: &AnnotateOptions) -> u32 {
        let game = self.session.game();
        let base = base_node(game, self.session.cursor());
        u32::try_from(plan(game, base, options).len()).unwrap_or(u32::MAX)
    }

    pub(crate) fn annotate_message(
        &mut self,
        message: &AnnotateMessage,
        effects: &mut Vec<Effect>,
    ) {
        match message {
            AnnotateMessage::Open => {
                if self.annotation_running() {
                    return self.annotate_cancel(effects);
                }
                if self.annotate_dialog.is_none() {
                    let options = self.annotate_options;
                    self.annotate_dialog = Some(AnnotateDialog {
                        options,
                        informant: self.annotate_informant,
                        engine: None,
                        moves: self.annotate_moves(&options),
                        book_available: self.book.opening.is_some(),
                    });
                }
            }
            AnnotateMessage::Close => self.annotate_dialog = None,
            AnnotateMessage::SetOptions(options) => {
                let moves = self.annotate_moves(options);
                if let Some(dialog) = &mut self.annotate_dialog {
                    dialog.options = *options;
                    dialog.moves = moves;
                }
            }
            AnnotateMessage::SetInformant(informant) => {
                if let Some(dialog) = &mut self.annotate_dialog {
                    dialog.informant = *informant;
                }
            }
            AnnotateMessage::SetEngine(run) => {
                if let Some(dialog) = &mut self.annotate_dialog {
                    dialog.engine = *run;
                }
            }
            AnnotateMessage::Start => self.annotate_start(effects),
            AnnotateMessage::Cancel => self.annotate_cancel(effects),
            AnnotateMessage::Tick { run, generation } => {
                self.annotate_tick(*run, *generation, effects);
            }
            AnnotateMessage::BatchStep(result) => self.batch_step_done(result, effects),
            // `AnnotateMessage` is non-exhaustive; nothing else exists.
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }

    // ---- start ----

    /// The checks every run (annotation or report) passes before it starts;
    /// sets the notice and returns `false` when one fails.
    pub(crate) fn annotate_preflight(&mut self) -> bool {
        if self.annotation_running() {
            self.annotate_notice(AnnotateNotice::AlreadyRunning);
            return false;
        }
        if self.session.in_trial() {
            self.annotate_notice(AnnotateNotice::TrialMode);
            return false;
        }
        // A database is being opened: the progress bar of the run would hide
        // the one of the opening and its cancel button (D931). Opening a
        // base during a run is already refused by `refuse_edit`.
        if self.flow.open_base.is_some() {
            self.notice = Some(AppNotice::Bases(BasesNotice::OpenInProgress));
            return false;
        }
        // The game is commanded by a shoot-out or an automove (D495).
        if self.training_playing() {
            self.notice = Some(AppNotice::Mode(crate::training::ModeNotice::Conflict));
            return false;
        }
        // The game against an engine owns the game and its engine (M8-1).
        if self.play_live() {
            self.play_busy();
            return false;
        }
        // The tournament holds the analyses and the exclusivity (D1311).
        if self.tournament_live() {
            self.notice = Some(AppNotice::Tournament(
                crate::tournament::TournamentNotice::AlreadyRunning,
            ));
            return false;
        }
        // The analysis of the positions of an EPD file holds an engine.
        if self.epd_running() {
            self.notice = Some(AppNotice::Epd(crate::epd_window::EpdNotice::EngineBusy));
            return false;
        }
        // The game to annotate is the one of the user, not the problem of
        // the tactics training (D1964).
        self.tactics_halt();
        true
    }

    fn annotate_start(&mut self, effects: &mut Vec<Effect>) {
        if !self.annotate_preflight() {
            return;
        }

        let dialog = self.annotate_dialog.take();
        if let Some(dialog) = &dialog {
            self.annotate_options = dialog.options;
            self.annotate_informant = dialog.informant;
        }
        let options = self.annotate_options.normalized();
        let informant = self.annotate_informant.normalized();
        self.annotate_options = options;
        self.annotate_informant = informant;
        let wanted = dialog.and_then(|d| d.engine);
        if options.batch {
            return self.batch_start(options, informant, wanted, effects);
        }
        self.annotate_launch(options, informant, wanted, None, effects);
    }

    /// Picks (or starts) the engine and begins a run over the current game.
    pub(crate) fn annotate_launch(
        &mut self,
        options: AnnotateOptions,
        informant: Informant,
        wanted: Option<RunId>,
        report: Option<Vec<PositionEval>>,
        effects: &mut Vec<Effect>,
    ) {
        let ready = wanted
            .and_then(|id| {
                self.engine_runs
                    .get(id)
                    .filter(|r| r.state == EngineRunState::Running && !self.play_uses(r.id))
                    .map(|r| r.id)
            })
            .or_else(|| self.annotate_default_run());
        let (engine_run, stage_ready) = if let Some(run) = ready {
            (run, true)
        } else {
            // No engine runs: start the one of F2 (D480).
            let Some(entry) = self
                .engines
                .hotkey_owner(Hotkey::F2)
                .and_then(|id| self.engines.get(id))
                .cloned()
            else {
                return self.annotate_notice(AnnotateNotice::NoEngine);
            };
            let existing = self
                .engine_runs
                .of_engine(entry.id)
                .map(|r| (r.id, r.state.clone()));
            match existing {
                Some((run, EngineRunState::Starting)) => (run, false),
                Some((run, EngineRunState::Failed(_))) => {
                    drop(self.engine_runs.remove(run));
                    let defaults = self.prefs.analysis.run_defaults();
                    (self.begin_run(entry, false, defaults, effects), false)
                }
                Some(_) => return self.annotate_notice(AnnotateNotice::NoEngine),
                None => {
                    let defaults = self.prefs.analysis.run_defaults();
                    (self.begin_run(entry, false, defaults, effects), false)
                }
            }
        };
        self.flow.annotate = Some(Active {
            stage: Stage::Starting,
            engine_run,
            options,
            informant,
            generation: self.session.generation(),
            expected_changes: self.session.changes(),
            restore: Restore::default(),
            variations: 0,
            exercises: 0,
            watch: 0,
            progress: 0,
            seen: 0,
            remaining: Duration::ZERO,
            period: Duration::ZERO,
            report: report.is_some(),
            resume: report.unwrap_or_default(),
        });
        if stage_ready {
            self.annotate_begin(effects);
        }
    }

    /// The engine of a run in `Starting` is ready.
    pub(crate) fn annotate_engine_ready(&mut self, run: RunId, effects: &mut Vec<Effect>) {
        let waiting = self
            .flow
            .annotate
            .as_ref()
            .is_some_and(|a| a.engine_run == run && matches!(a.stage, Stage::Starting));
        if waiting {
            self.annotate_begin(effects);
        }
    }

    /// Sets the engine up and sends the first search.
    #[allow(clippy::too_many_lines)] // one sequence: checks, setup, tags, start
    fn annotate_begin(&mut self, effects: &mut Vec<Effect>) {
        let Some(active) = self.flow.annotate.as_ref() else {
            return;
        };
        let engine_run = active.engine_run;
        let (mut options, informant) = (active.options, active.informant);
        let report = active.report;
        // The game was swapped while the engine warmed up (D896).
        if self.session.generation() != active.generation
            || self.session.changes() != active.expected_changes
        {
            return self.annotate_finish(Outcome::GameChanged, true, effects);
        }
        let Some(run) = self.engine_runs.get(engine_run) else {
            return self.annotate_abort(AnnotateNotice::NoEngine, effects);
        };
        if run.state != EngineRunState::Running {
            return self.annotate_abort(AnnotateNotice::NoEngine, effects);
        }
        if self.session.in_trial() {
            return self.annotate_abort(AnnotateNotice::TrialMode, effects);
        }
        let identity = run.identity.clone();
        let analyzing = run.analyzing;
        let entry = self.engines.get(run.engine).cloned();
        let xboard = entry
            .as_ref()
            .is_some_and(|e| e.protocol == Protocol::XBoard);
        let caps = identity.as_ref().map(|i| i.capabilities.clone());
        // What the engine cannot do is turned off (D477).
        if xboard {
            options.control = Control::Time;
            options.missed_mates = false;
            options.mark_exercises = false;
        }
        let lines_max = caps
            .as_ref()
            .and_then(|c| c.multipv.as_ref().map(|r| *r.end()));
        let exercises = options.mark_exercises && lines_max.is_some_and(|m| m >= 2);
        let lines_wanted = match lines_max {
            Some(max) if exercises => EXERCISE_LINES.min(max),
            // The report needs the second line (D1422, D1427).
            Some(max) if report => REPORT_LINES.min(max),
            _ => 1,
        };
        let game = self.session.game();
        // The report always walks the whole main line.
        let base = if report {
            game.root()
        } else {
            base_node(game, self.session.cursor())
        };
        let clear_hash = identity.as_ref().is_some_and(|i| {
            i.options
                .iter()
                .any(|o| o.name == CLEAR_HASH && matches!(o.kind, OptionKind::Button))
        });
        let setup = RunSetup {
            options,
            informant,
            base,
            exercises,
            clear_hash,
            lines: lines_wanted,
            collect: report,
        };
        let resume = self
            .flow
            .annotate
            .as_mut()
            .map(|a| std::mem::take(&mut a.resume))
            .unwrap_or_default();
        let annotation = match AnnotationRun::new(game, &setup) {
            Ok(run) => match self.book.opening.as_ref() {
                // The moves of the opening book are not searched (D1665).
                Some(book) if options.use_book => run.with_book(game, book),
                _ => run,
            }
            .with_known(resume),
            Err(AnnotationError::NothingToAnnotate) => {
                return self.annotate_abort(AnnotateNotice::NothingToAnnotate, effects);
            }
            Err(_) => return self.annotate_abort(AnnotateNotice::NoEngine, effects),
        };

        // The engine: no analysis of the board, one line (four for the
        // exercises), no opening book (D478).
        let mut restore = Restore {
            analyzing,
            ..Restore::default()
        };
        if let Some(r) = self.engine_runs.get_mut(engine_run) {
            r.analyzing = false;
            r.sent = None;
        }
        if let (Some(_), Some(entry)) = (lines_max, entry.as_ref()) {
            // With exercises the probes lower the lines and a run that ends
            // among them must put them back, so they are always restored.
            if entry.multipv != lines_wanted || exercises {
                restore.multipv = Some(entry.multipv);
            }
            if entry.multipv != lines_wanted {
                self.annotate_send(engine_run, EngineCommand::SetMultiPv(lines_wanted), effects);
            }
        }
        if let (Some(identity), Some(entry)) = (identity.as_ref(), entry.as_ref())
            && let Some(own_book) = identity
                .options
                .iter()
                .find(|o| o.name == "OwnBook")
                .and_then(|o| match o.kind {
                    OptionKind::Check { default } => Some(default),
                    _ => None,
                })
        {
            let saved = entry
                .options
                .iter()
                .find(|(name, _)| name == "OwnBook")
                .and_then(|(_, value)| bool_text(value))
                .unwrap_or(own_book);
            if saved {
                restore.own_book = Some(true);
                self.annotate_send(
                    engine_run,
                    EngineCommand::SetOption {
                        name: "OwnBook".to_owned(),
                        value: OptionValue::Bool(false),
                    },
                    effects,
                );
            }
        }

        // One undo point for the whole run; the tags are its first edit. The
        // report never touches the game (D1447).
        if !report {
            self.session.begin_edit_group();
        }
        if options.add_tag && !report {
            let name = identity
                .as_ref()
                .map(|i| i.name.clone())
                .or_else(|| entry.as_ref().map(|e| e.name.clone()))
                .unwrap_or_default();
            let depth = (options.control == Control::Depth).then(|| options.depth.to_string());
            drop(self.session.edit(|game, _| {
                let tags = game.tags_mut();
                let old = tags.get_extra("Annotator").map(str::to_owned);
                // A value the tag cannot hold (a line break) is dropped.
                tags.set_extra("Annotator", &append_tag(old.as_deref(), &name))
                    .ok();
                if let Some(depth) = &depth {
                    let old = tags.get_extra("Depth").map(str::to_owned);
                    tags.set_extra("Depth", &append_tag(old.as_deref(), depth))
                        .ok();
                }
                Ok(())
            }));
        }

        let generation = self.session.generation();
        let expected = self.session.changes();
        let mut annotation = Box::new(annotation);
        let outputs = annotation.start(self.session.game());
        if let Some(active) = self.flow.annotate.as_mut() {
            active.stage = Stage::Running(annotation);
            active.options = options;
            active.generation = generation;
            active.expected_changes = expected;
            active.restore = restore;
        }
        self.annotate_outputs(outputs, effects);
    }

    /// Ends a run that never started (nothing was changed in the game).
    fn annotate_abort(&mut self, notice: AnnotateNotice, effects: &mut Vec<Effect>) {
        self.flow.annotate = None;
        if self.flow.annotate_batch.is_some() {
            // A game without moves is skipped; anything else stops the batch.
            if notice == AnnotateNotice::NothingToAnnotate {
                return self.batch_advance(false, effects);
            }
            self.batch_end(false, effects);
            // The reason (no engine...) is worth more than the count of
            // games saved, which the status bar still shows (D1553).
            return self.annotate_notice(notice);
        }
        self.annotate_notice(notice);
    }

    // ---- the run ----

    /// Gives an event of an engine to the run. Returns whether the run took
    /// it (then it is not for the analysis window).
    pub(crate) fn annotate_event(
        &mut self,
        run: RunId,
        event: &EngineEvent,
        effects: &mut Vec<Effect>,
    ) -> bool {
        if self.annotate_is_stale(run, event) {
            return true;
        }
        self.annotate_live_event(run, event, effects) || self.annotate_note_stale(run, event)
    }

    fn annotate_live_event(
        &mut self,
        run: RunId,
        event: &EngineEvent,
        effects: &mut Vec<Effect>,
    ) -> bool {
        let game = self.session.game();
        let Some(active) = self.flow.annotate.as_mut() else {
            return false;
        };
        if active.engine_run != run {
            return false;
        }
        let Stage::Running(annotation) = &mut active.stage else {
            return false;
        };
        let Some(outputs) = annotation.on_event(game, event) else {
            return false;
        };
        active.progress = active.progress.wrapping_add(1);
        self.annotate_outputs(outputs, effects);
        true
    }

    fn annotate_cancel(&mut self, effects: &mut Vec<Effect>) {
        if let Some(batch) = self.flow.annotate_batch.as_mut() {
            batch.cancelled = true;
        }
        let Some(active) = self.flow.annotate.as_mut() else {
            return;
        };
        match &mut active.stage {
            // The engine is still starting: there is nothing to undo.
            Stage::Starting => self.annotate_finish(Outcome::Cancelled, true, effects),
            Stage::Running(run) => {
                let outputs = run.cancel();
                self.annotate_outputs(outputs, effects);
                // A `stop` the engine never answers must not hold the run.
                if self.annotation_running() {
                    self.annotate_arm(CANCEL_GRACE, effects);
                }
            }
        }
    }

    // ---- the watchdog (D895) ----

    /// Starts a timer of `delay`; the one before it becomes stale.
    fn annotate_arm(&mut self, delay: Duration, effects: &mut Vec<Effect>) {
        let generation = self.new_request().0;
        let Some(active) = self.flow.annotate.as_mut() else {
            return;
        };
        active.watch = generation;
        active.seen = active.progress;
        active.period = delay;
        effects.push(Effect::ScheduleAnnotateTick {
            run: active.engine_run,
            generation,
            delay,
        });
    }

    /// The timer fired: if the engine said nothing since it was set, it is
    /// given up (the run fails, or ends cancelled when the user had asked);
    /// otherwise the wait starts again.
    fn annotate_tick(&mut self, run: RunId, generation: u64, effects: &mut Vec<Effect>) {
        let Some(active) = self.flow.annotate.as_ref() else {
            return;
        };
        if active.engine_run != run || active.watch != generation {
            return;
        }
        let cancelling = matches!(&active.stage, Stage::Running(r) if r.is_cancelling());
        // The only timer alive after a cancel is the one of `CANCEL_GRACE`:
        // whatever the engine says, the run ends with it (D900).
        if cancelling {
            return self.annotate_finish(Outcome::Cancelled, true, effects);
        }
        let remaining = active.remaining.saturating_sub(active.period);
        let silent = active.progress == active.seen;
        if silent || remaining.is_zero() {
            let outcome = Outcome::Failed(AnnotationError::EngineFailed);
            return self.annotate_finish(outcome, true, effects);
        }
        if let Some(active) = self.flow.annotate.as_mut() {
            active.remaining = remaining;
        }
        self.annotate_arm(remaining.min(DEPTH_STALL), effects);
    }

    /// A command to the engine of the run was refused or found its queue
    /// full. A search that never starts would hold the run for ever: it
    /// fails. A refused option only warns (the engine goes on with what it
    /// had).
    pub(crate) fn annotate_command_failed(
        &mut self,
        run: RunId,
        kind: CommandKind,
        effects: &mut Vec<Effect>,
    ) {
        if kind != CommandKind::Search || !self.annotation_uses(run) {
            return;
        }
        let Some(Active {
            stage: Stage::Running(annotation),
            ..
        }) = self.flow.annotate.as_mut()
        else {
            return;
        };
        if annotation.awaits_start() {
            let outputs = annotation.refused();
            self.annotate_outputs(outputs, effects);
        }
    }

    /// A search of an ended run that was never adopted: remembers it so
    /// what it reports is dropped. Returns whether `event` was its label.
    fn annotate_note_stale(&mut self, run: RunId, event: &EngineEvent) -> bool {
        let EngineEvent::SearchTagged { search, tag } = event else {
            return false;
        };
        if !is_annotation_tag(*tag) {
            // A search of the analysis window is coming: an untagged start
            // can no longer be the stopped one's.
            if self.flow.annotate_untagged == Some(run) {
                self.flow.annotate_untagged = None;
            }
            return false;
        }
        self.annotate_remember(run, *search);
        true
    }

    /// Remembers a search whose reports must be dropped (at most
    /// `STALE_LIMIT`, the oldest forgotten first).
    fn annotate_remember(&mut self, run: RunId, search: SearchId) {
        let stale = &mut self.flow.annotate_stale;
        if stale.len() >= STALE_LIMIT {
            stale.remove(0);
        }
        stale.push((run, search));
    }

    /// What a search of an ended run still reports is dropped.
    fn annotate_is_stale(&mut self, run: RunId, event: &EngineEvent) -> bool {
        let id = match event {
            EngineEvent::SearchStarted(id) => *id,
            EngineEvent::Info(info) => info.search,
            EngineEvent::BestMove(best) => best.search,
            _ => return false,
        };
        let stale = &mut self.flow.annotate_stale;
        let Some(at) = stale.iter().position(|entry| *entry == (run, id)) else {
            // The engine that lost the label of the stopped search: its
            // next start without one is that search's (D900).
            if matches!(event, EngineEvent::SearchStarted(_))
                && self.flow.annotate_untagged == Some(run)
            {
                self.flow.annotate_untagged = None;
                self.annotate_remember(run, id);
                return true;
            }
            return false;
        };
        if matches!(event, EngineEvent::BestMove(_)) {
            stale.remove(at);
        }
        true
    }

    /// Whether the game must not be edited, replaced or navigated by anyone
    /// but the run (D885, D897). Every way in checks this one predicate.
    pub(crate) fn edits_blocked(&self) -> bool {
        self.annotation_running() || self.play_live() || self.assist_locked()
    }

    /// Refuses an edit while the run owns the game: sets the notice.
    /// Returns whether it was refused.
    pub(crate) fn refuse_edit(&mut self) -> bool {
        if self.edits_blocked() {
            if self.annotation_running() {
                self.annotate_notice(AnnotateNotice::Busy);
            } else if self.assist_locked() {
                self.notice = Some(AppNotice::Online(
                    crate::online::OnlineNotice::AssistanceDisabled,
                ));
            } else {
                self.notice = Some(AppNotice::Play(crate::play::PlayNotice::Locked));
            }
            true
        } else {
            false
        }
    }

    fn annotate_outputs(&mut self, outputs: Vec<RunOutput>, effects: &mut Vec<Effect>) {
        for output in outputs {
            let Some(engine_run) = self.flow.annotate.as_ref().map(|a| a.engine_run) else {
                return;
            };
            match output {
                RunOutput::Send(command) => {
                    let limit = match &*command {
                        EngineCommand::Search(request) => Some(request.limits.movetime),
                        _ => None,
                    };
                    self.annotate_send(engine_run, *command, effects);
                    if let Some(movetime) = limit {
                        // The absolute deadline of the search, and the
                        // first silence period inside it (D900).
                        let (total, period) = match movetime {
                            Some(t) => {
                                let total = t.saturating_add(STALL_GRACE);
                                (total, total)
                            }
                            None => (DEPTH_CEILING, DEPTH_STALL),
                        };
                        if let Some(active) = self.flow.annotate.as_mut() {
                            active.remaining = total;
                        }
                        self.annotate_arm(period, effects);
                    }
                }
                RunOutput::Apply(actions) => {
                    let result = self.session.edit(|game, _| apply_actions(game, &actions));
                    let Ok(stats) = result else {
                        let outcome = Outcome::Failed(AnnotationError::StaleGame);
                        return self.annotate_finish(outcome, true, effects);
                    };
                    if stats.exercises > 0 {
                        self.session.mark_tactics();
                    }
                    let changes = self.session.changes();
                    if let Some(active) = self.flow.annotate.as_mut() {
                        active.variations = active.variations.saturating_add(stats.variations);
                        active.exercises = active.exercises.saturating_add(stats.exercises);
                        active.expected_changes = changes;
                    }
                }
                RunOutput::Done(outcome) => return self.annotate_finish(outcome, true, effects),
            }
        }
    }

    /// Ends the run: closes the undo group, puts the engine back as it was
    /// (when it is alive) and reports. The edits made stay (D469).
    pub(crate) fn annotate_finish(
        &mut self,
        outcome: Outcome,
        engine_alive: bool,
        effects: &mut Vec<Effect>,
    ) {
        let Some(mut active) = self.flow.annotate.take() else {
            return;
        };
        // The group exists only once the run started to edit.
        if matches!(active.stage, Stage::Running(_)) && !active.report {
            self.session.end_edit_group();
        }
        let (plies, total) = match &active.stage {
            Stage::Running(run) => {
                let progress = run.progress();
                (progress.done_plies, progress.total_plies)
            }
            Stage::Starting => (0, 0),
        };
        let engine_run = active.engine_run;
        let ready = engine_alive
            && self
                .engine_runs
                .get(engine_run)
                .is_some_and(|r| r.state == EngineRunState::Running);
        if ready && let Stage::Running(run) = &active.stage {
            // A search still going (the run ended for another reason than
            // its own end) is stopped before the settings come back.
            if !run.is_finished() {
                // What the stopped search still reports is not for the
                // analysis window (D900).
                match run.pending_search() {
                    Some(id) => self.annotate_remember(engine_run, id),
                    None if run.awaits_start() => {
                        self.flow.annotate_untagged = Some(engine_run);
                    }
                    None => {}
                }
                self.annotate_send(engine_run, EngineCommand::Stop, effects);
            }
            if let Some(lines) = active.restore.multipv {
                self.annotate_send(engine_run, EngineCommand::SetMultiPv(lines), effects);
            }
            if let Some(value) = active.restore.own_book {
                let command = EngineCommand::SetOption {
                    name: "OwnBook".to_owned(),
                    value: OptionValue::Bool(value),
                };
                self.annotate_send(engine_run, command, effects);
            }
        }
        if ready
            && matches!(active.stage, Stage::Running(_))
            && let Some(r) = self.engine_runs.get_mut(engine_run)
        {
            // The board's analysis comes back: the next refresh sends it.
            r.analyzing = active.restore.analyzing;
            r.sent = None;
        }
        if active.report {
            let evals = match &mut active.stage {
                Stage::Running(run) => run.take_collected(),
                Stage::Starting => Vec::new(),
            };
            let effort = crate::report::ReportEffort {
                by_time: matches!(active.options.control, Control::Time),
                depth: active.options.depth,
                movetime_ms: u32::try_from(active.options.movetime.as_millis()).unwrap_or(u32::MAX),
                engine: Some(engine_run),
            };
            return self.report_run_ended(outcome, evals, total, effort);
        }
        let report = AnnotateReport {
            outcome,
            plies,
            total,
            variations: active.variations,
            exercises: active.exercises,
        };
        self.annotate_report = Some(report);
        if let Some(batch) = self.flow.annotate_batch.as_mut() {
            batch.engine = Some(engine_run);
            let go_on = outcome == Outcome::Finished && !batch.cancelled;
            if go_on {
                return self.batch_advance(true, effects);
            }
            return self.batch_end(false, effects);
        }
        self.annotate_notice(AnnotateNotice::of_report(&report));
    }

    /// The engine of the run ended, failed to start or was stopped.
    pub(crate) fn annotate_engine_gone(
        &mut self,
        run: RunId,
        engine_alive: bool,
        effects: &mut Vec<Effect>,
    ) {
        if self.annotation_uses(run) {
            let outcome = Outcome::Failed(AnnotationError::EngineFailed);
            self.annotate_finish(outcome, engine_alive, effects);
        }
    }

    /// Stops a run whose game was replaced, undone or edited by someone
    /// else (rule 15 of the spec: "partida alterada durante a execução").
    /// Called at the end of every update.
    pub(crate) fn annotate_guard(&mut self, effects: &mut Vec<Effect>) {
        let Some(active) = self.flow.annotate.as_ref() else {
            return;
        };
        let changed = self.session.generation() != active.generation
            || self.session.changes() != active.expected_changes;
        if changed {
            self.annotate_finish(Outcome::GameChanged, true, effects);
        }
    }
}

// ---- the batch over the filter (phase B, D1520-D1529) ----

impl AnnotateOptions {
    /// Whether the options ask for a batch.
    const fn is_batch(&self) -> bool {
        self.batch
    }
}

impl AppState {
    /// Starts the batch: checks the database, then reads the first game of
    /// the filter. The games come one at a time; each annotated game is
    /// saved before the next is read (D479).
    fn batch_start(
        &mut self,
        options: AnnotateOptions,
        informant: Informant,
        engine: Option<RunId>,
        effects: &mut Vec<Effect>,
    ) {
        debug_assert!(options.is_batch());
        // The current game is replaced by every game of the batch.
        if self.session.is_dirty() {
            return self.annotate_notice(AnnotateNotice::BatchDirty);
        }
        let slot_id = self.bases.current();
        let Some(slot) = self.bases.slot(slot_id) else {
            return self.annotate_notice(AnnotateNotice::BatchUnavailable);
        };
        // Only an editable SI4 database can keep the annotations (D1521).
        let path = match (&slot.path, slot.kind) {
            (Some(path), BaseKind::Si4) if !slot.read_only && !slot.needs_reload => path.clone(),
            _ => return self.annotate_notice(AnnotateNotice::BatchUnavailable),
        };
        let numbers: Vec<u32> = slot.filter.iter_games().collect();
        let Some(first) = numbers.first().copied() else {
            return self.annotate_notice(AnnotateNotice::NothingToAnnotate);
        };
        let (expected_total, stamp) = (slot.total(), slot.stamp);
        if self.list_busy() {
            return;
        }
        // The game of the editor now belongs to the batch: it has no file to
        // be saved over (D1552).
        self.origin = GameOrigin::New;
        self.collection = None;
        self.flow.annotate_batch = Some(Batch {
            slot: slot_id,
            path: path.clone(),
            numbers,
            index: 0,
            options,
            informant,
            engine,
            expected_total,
            stamp,
            saved: 0,
            skipped: 0,
            saving: false,
            cancelled: false,
        });
        effects.push(Effect::AnnotateBatchStep {
            path,
            save: None,
            next: Some(first),
            expected_total,
            stamp,
        });
    }

    /// The current game is done (or skipped): saves it when `save` and goes
    /// to the next one.
    fn batch_advance(&mut self, save: bool, effects: &mut Vec<Effect>) {
        let Some(batch) = self.flow.annotate_batch.as_ref() else {
            return;
        };
        if batch.cancelled {
            return self.batch_end(false, effects);
        }
        let save = if save {
            batch.numbers.get(batch.index).map(|number| BatchSave {
                number: *number,
                game: SharedGame::new(self.session.game().clone()),
                tactics: self.session.tactics_pending(),
            })
        } else {
            None
        };
        let Some(batch) = self.flow.annotate_batch.as_mut() else {
            return;
        };
        batch.saving = save.is_some();
        if !batch.saving {
            batch.skipped = batch.skipped.saturating_add(1);
        }
        batch.index = batch.index.saturating_add(1);
        let next = batch.numbers.get(batch.index).copied();
        effects.push(Effect::AnnotateBatchStep {
            path: batch.path.clone(),
            save,
            next,
            expected_total: batch.expected_total,
            stamp: batch.stamp,
        });
    }

    /// The answer to a batch step: puts the reopened database in the slot
    /// and goes on with the next game, or ends.
    fn batch_step_done(
        &mut self,
        result: &Result<BatchStep, BaseError>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(batch) = self.flow.annotate_batch.as_mut() else {
            return;
        };
        let step = match result {
            Ok(step) => step,
            Err(error) => {
                let error = error.clone();
                self.flow.annotate_batch = None;
                self.annotate_options.batch = false;
                self.notice = Some(AppNotice::Bases(BasesNotice::Failed(error)));
                return self.batch_closed(effects);
            }
        };
        if batch.saving {
            batch.saved = batch.saved.saturating_add(1);
            batch.saving = false;
        }
        batch.stamp = step.stamp;
        let slot_id = batch.slot;
        let cancelled = batch.cancelled;
        // The index entry of the game changed: the slot reads the file again.
        let total = mychess_db::GameSource::len(&*step.source.read());
        if let Some(slot) = self.bases.slot_mut(slot_id) {
            if total != slot.filter.len() {
                // Someone changed the file under the batch (D573); the slot
                // is left as it was (D1554).
                self.flow.annotate_batch = None;
                self.annotate_options.batch = false;
                self.notice = Some(AppNotice::Bases(BasesNotice::Failed(BaseError::Stale)));
                return self.batch_closed(effects);
            }
            slot.source = step.source.clone();
            slot.stamp = step.stamp;
            slot.clear_sort();
        }
        let Some(game) = step.game.as_ref() else {
            return self.batch_end(true, effects);
        };
        if cancelled {
            return self.batch_end(false, effects);
        }
        let Some(batch) = self.flow.annotate_batch.as_mut() else {
            return;
        };
        let (options, informant, engine) = (batch.options, batch.informant, batch.engine);
        let index = batch.index;
        self.session.replace_game(game.to_game());
        self.origin = GameOrigin::New;
        self.annotate_launch(options, informant, engine, None, effects);
        // The launch failed at once (no engine): the batch stops. A skipped
        // game (nothing to annotate) already moved the batch on instead.
        let advanced = self
            .flow
            .annotate_batch
            .as_ref()
            .is_some_and(|batch| batch.index != index);
        if self.flow.annotate.is_none() && self.flow.annotate_batch.is_some() && !advanced {
            self.batch_end(false, effects);
        }
    }

    /// Ends the batch with its report. A finished batch leaves the last
    /// game, already saved, as not altered.
    fn batch_end(&mut self, finished: bool, effects: &mut Vec<Effect>) {
        let Some(batch) = self.flow.annotate_batch.take() else {
            return;
        };
        // "Batch" is a choice of this run: a later "annotate this game" must
        // not write the whole filter without asking (D1553).
        self.annotate_options.batch = false;
        if finished {
            self.session.mark_saved();
        }
        let notice = if finished {
            AnnotateNotice::BatchFinished {
                games: batch.saved,
                skipped: batch.skipped,
            }
        } else {
            AnnotateNotice::BatchStopped {
                games: batch.saved,
                total: u32::try_from(batch.numbers.len()).unwrap_or(u32::MAX),
            }
        };
        self.annotate_notice(notice);
        self.batch_closed(effects);
    }

    /// The batch is gone: a quit that waited for its step goes on (D1550).
    fn batch_closed(&mut self, effects: &mut Vec<Effect>) {
        if self.flow.quit_waiting_batch && self.flow.annotate_batch.is_none() {
            self.flow.quit_waiting_batch = false;
            self.finish_quit(effects);
        }
    }
}
