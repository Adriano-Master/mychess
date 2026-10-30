//! The visual state of the tournament between engines and the events of its
//! form, panel and boxes (spec M8-2b, D1360-D1379).
//!
//! The application owns the tournament (`mychess_app::tournament`); what is
//! here is the form being edited ([`TournamentDraft`]), the tab shown, the
//! questions waiting for an answer, and a **snapshot** of the derived data
//! (standings, crosstable, the moves of the live game in SAN) that is rebuilt
//! only when the tournament changed, never on a tick of the clocks (D1366).
//!
//! [`TournamentUi::update`] never touches the application: it returns the
//! messages to send in an [`Outcome`]. The file dialogs, the clipboard and the
//! quit are the shell's business (`shell/tournament_glue.rs`).

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use mychess_app::engines::EngineId;
use mychess_app::tournament::{
    Decision, ExportFormat, ExportTable, GameScore, LaneView, Participant, Phase, RatingRow,
    Scheduling, StandingRow, TimeRow, Tournament, TournamentMessage,
};
use mychess_app::{AppState, Message as AppMessage};
use mychess_core::{Color, GameResult, Position};

use crate::app_api;
use crate::widgets::board::geometry::Orientation;
use crate::widgets::board::marks::MarkBrush;
use crate::widgets::board::pieces::PieceSet;
use crate::widgets::board::{BoardCaches, BoardData, BoardOptions};
use crate::windows::dialog_ui::Outcome;
use crate::windows::tournament_draft::{DraftEdit, TournamentDraft, playable_engines};

/// The tabs of the panel (D1283).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TournamentTab {
    /// The table of standings, live.
    #[default]
    Standings,
    /// The crosstable.
    Cross,
    /// The list of games.
    Pairings,
    /// The game being played.
    Live,
    /// The log.
    Log,
    /// Elo and likelihood of superiority (D2513).
    Ratings,
    /// The report of time (D2516).
    Time,
}

impl TournamentTab {
    /// Every tab, in order.
    pub const ALL: [TournamentTab; 7] = [
        TournamentTab::Standings,
        TournamentTab::Ratings,
        TournamentTab::Time,
        TournamentTab::Cross,
        TournamentTab::Pairings,
        TournamentTab::Live,
        TournamentTab::Log,
    ];

    /// The Fluent key of the label.
    pub fn label_key(self) -> &'static str {
        match self {
            TournamentTab::Standings => "tournament-tab-standings",
            TournamentTab::Cross => "tournament-tab-cross",
            TournamentTab::Pairings => "tournament-tab-pairings",
            TournamentTab::Live => "tournament-tab-live",
            TournamentTab::Log => "tournament-tab-log",
            TournamentTab::Ratings => "tournament-tab-ratings",
            TournamentTab::Time => "tournament-tab-time",
        }
    }
}

/// What waits for a Yes or a No (the lesson of the review of M8-1b: nothing
/// that loses work happens on one click).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TournamentConfirm {
    /// End the tournament (the game in progress is dropped).
    Abort,
    /// Decide the game in progress.
    Adjudicate(GameResult),
    /// Drop the game in progress without a result.
    SkipCurrent,
    /// Leave a tournament that is paused or stopped: its engines end (the
    /// state stays on disk and can be resumed).
    CloseTournament,
    /// Quit the program with a tournament on.
    Quit,
}

/// The answer to the question about a participant that failed too often.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionChoice {
    /// Try again.
    Retry,
    /// The participant leaves.
    Withdraw,
    /// Another engine takes the place.
    Replace,
}

/// A message of the tournament for the visual state.
#[derive(Debug, Clone, PartialEq)]
pub enum TournamentUiMessage {
    /// Opens the form.
    Open,
    /// Opens the list of tournaments that can be resumed.
    OpenResume,
    /// A tournament of the list was chosen.
    ResumePick(PathBuf),
    /// Closes the list.
    ResumeClose,
    /// An edit of the form.
    Edit(DraftEdit),
    /// Shows the dialog that picks the book.
    PickBook,
    /// Shows the dialog that picks the suite of openings.
    PickSuite,
    /// The book the dialog chose (`None`: cancelled).
    BookPicked(Option<PathBuf>),
    /// The suite the dialog chose (`None`: cancelled).
    SuitePicked(Option<PathBuf>),
    /// Opens the Engines dialog.
    OpenEngines,
    /// "Start".
    Submit,
    /// "Cancel".
    Cancel,
    /// A tab was chosen.
    Tab(TournamentTab),
    /// Pause after the move in progress.
    Pause,
    /// Go on.
    Continue,
    /// Stop when the game ends.
    StopAfter,
    /// A button that needs a confirmation.
    Request(TournamentConfirm),
    /// The answer to the confirmation.
    Confirm(bool),
    /// The answer to the question about a participant.
    Decide(DecisionChoice),
    /// Close the box of the question without answering (the tournament
    /// waits).
    DecisionLater,
    /// Open the box of the question again.
    DecisionReopen,
    /// The engine that replaces the participant.
    ReplaceWith(EngineId),
    /// Play a game again.
    Redo(u32),
    /// Skip a game that waits.
    SkipGame(u32),
    /// Move a game that waits to the front.
    ToFront(u32),
    /// A game was clicked in the list.
    Select(u32),
    /// A cell of the crosstable was clicked: shows its games in the list.
    Cell(usize, usize),
    /// The list of games was scrolled (pixels).
    Scrolled(f32),
    /// The format of the exports.
    ExportFormat(ExportFormat),
    /// Save a table to a file (shows the dialog).
    Save(ExportTable),
    /// The file the dialog chose (`None`: cancelled) and the format that was
    /// current when the dialog opened.
    Saved(ExportTable, ExportFormat, Option<PathBuf>),
    /// Copy a table to the clipboard.
    Copy(ExportTable),
    /// Back to the form with the same configuration.
    Restart,
    /// Open the PGN of the tournament as a database.
    OpenPgn,
    /// Leave the tournament (finished or ended).
    CloseTournament,
    /// A tick of the clocks of the live tab.
    Tick(Instant),
    /// Show the game of this lane in the live tab (D2508).
    Watch(usize),
    /// An event nobody needs (the read-only board).
    Noop,
}

/// The moves of the game being played, written in SAN, one line per full
/// move (D1366).
#[derive(Debug, Clone, Default)]
pub struct LiveLines {
    key: Option<(u32, u8)>,
    done: usize,
    replay: Option<Position>,
    white_open: bool,
    /// The lines `12. Nf3 +0.31/18  Nc6 -0.20/17`.
    pub lines: Vec<String>,
}

/// What the tables show, rebuilt only when the tournament changed.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    key: Option<(String, usize, usize, usize, usize)>,
    /// The table of standings.
    pub standings: Vec<StandingRow>,
    /// The crosstable, `cross[i][j]` = games of `i` against `j`.
    pub cross: Vec<Vec<Vec<(u32, GameScore)>>>,
    /// The table of Elo and LOS (D2513).
    pub ratings: Vec<RatingRow>,
    /// The LOS of every participant over every other (D2512).
    pub los: Vec<Vec<Option<f64>>>,
    /// The report of time (D2516).
    pub time: Vec<TimeRow>,
    /// The games in progress, one per busy lane (D2508).
    pub lanes: Vec<LaneView>,
    /// The names of the participants (kept after the tournament closes, for
    /// the remarks that are still on the status bar).
    pub names: Vec<String>,
    /// The game being played.
    pub live: LiveLines,
}

/// The list of tournaments that can be resumed.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResumeUi {
    /// The listing was asked for and the answer arrived.
    pub listed: bool,
}

/// Everything the screen keeps for the tournament.
#[derive(Debug)]
pub struct TournamentUi {
    /// The form, when open.
    pub draft: Option<Box<TournamentDraft>>,
    /// The tab shown.
    pub tab: TournamentTab,
    /// The question waiting for an answer.
    pub confirm: Option<TournamentConfirm>,
    /// The list of tournaments to resume, when open.
    pub resume: Option<ResumeUi>,
    /// The question about a participant is on screen.
    pub decision_open: bool,
    /// The engine chosen to replace a participant.
    pub replace_with: Option<EngineId>,
    /// The format of the exports.
    pub export_format: ExportFormat,
    /// The last instant seen by an update or a tick; the views read it
    /// instead of the clock (D1366).
    pub now: Instant,
    /// The game selected in the list.
    pub selected: Option<u32>,
    /// The scroll of the list of games, in pixels.
    pub scroll: f32,
    /// The derived data.
    pub snapshot: Snapshot,
    caches: BoardCaches,
    brush: MarkBrush,
    had_tournament: bool,
    was_decision: bool,
}

impl Default for TournamentUi {
    fn default() -> TournamentUi {
        TournamentUi {
            draft: None,
            tab: TournamentTab::default(),
            confirm: None,
            resume: None,
            decision_open: false,
            replace_with: None,
            export_format: ExportFormat::Csv,
            now: Instant::now(),
            selected: None,
            scroll: 0.0,
            snapshot: Snapshot::default(),
            caches: BoardCaches::default(),
            brush: MarkBrush::default(),
            had_tournament: false,
            was_decision: false,
        }
    }
}

/// Height of a row of the list of games, in pixels.
pub const ROW_HEIGHT: f32 = 26.0;
/// Height of the list of games assumed when the real one is not known.
pub const LIST_HEIGHT: f32 = 640.0;
/// Rows built beyond the visible ones, each side.
const ROW_MARGIN: usize = 6;
/// Most lines of the log shown.
pub const LOG_LINES: usize = 300;

impl TournamentUi {
    /// The rows of the list of games to build for `total` rows (first and
    /// count): the visible ones plus a margin (D1366).
    pub fn window(&self, total: usize) -> (usize, usize) {
        self.window_for(total, LIST_HEIGHT)
    }

    /// Like [`TournamentUi::window`] for a list `height` pixels tall (the
    /// height the panel gives it).
    pub fn window_for(&self, total: usize, height: f32) -> (usize, usize) {
        // The offset is a non-negative pixel count.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let first_visible = (self.scroll.max(0.0) / ROW_HEIGHT) as usize;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let visible = (height.max(0.0) / ROW_HEIGHT).ceil() as usize + 1;
        let first = first_visible.saturating_sub(ROW_MARGIN).min(total);
        let count = (visible + 2 * ROW_MARGIN).min(total - first);
        (first, count)
    }

    /// Whether the board of the live game is drawn with the black pieces at
    /// the bottom: in a gauntlet the board follows the tested participant
    /// (Scid `comp.tcl` 562-567).
    pub fn orientation(app: &AppState) -> Orientation {
        let Some(tournament) = app.tournament() else {
            return Orientation::White;
        };
        let Some(live) = tournament.live() else {
            return Orientation::White;
        };
        if tournament.scheduling() == Scheduling::Gauntlet
            && live.black == tournament.config().tested
        {
            Orientation::Black
        } else {
            Orientation::White
        }
    }

    /// Whether the tick of the clocks is needed: the live tab is shown and
    /// a game is on (D1366: no timer otherwise).
    pub fn wants_ticks(&self, app: &AppState, tab_visible: bool) -> bool {
        tab_visible
            && self.tab == TournamentTab::Live
            && app
                .tournament()
                .is_some_and(|t| matches!(t.phase(), Phase::Running | Phase::Pausing))
    }

    /// Whether the question about a participant is on screen (the
    /// tournament waits for it and the user did not close the box).
    pub fn decision_visible(&self, app: &AppState) -> bool {
        self.decision_open
            && app
                .tournament()
                .is_some_and(|t| matches!(t.phase(), Phase::Decision(_)))
    }

    /// The board of the live game for the view.
    pub fn board_parts(&self) -> (&BoardCaches, &MarkBrush) {
        (&self.caches, &self.brush)
    }

    /// The data the board of the live game draws, if a game is on.
    pub fn board_data<'a>(
        &'a self,
        app: &'a AppState,
        pieces: &'a PieceSet,
        options: BoardOptions,
    ) -> Option<BoardData<'a>> {
        let tournament = app.tournament()?;
        let live = tournament.live()?;
        let position = live.position?;
        Some(BoardData {
            position,
            last_move: live.moves.last().map(|m| m.mv),
            marks: &[],
            orientation: TournamentUi::orientation(app),
            options,
            accepts_moves: false,
            brush: &self.brush,
            pieces,
            animation: None,
        })
    }

    /// Brings the derived data up to date with the application (called
    /// after every update, never on a tick). Returns whether the pane has to
    /// be shown because a tournament just started.
    pub fn sync(
        &mut self,
        app: &AppState,
        pieces: &PieceSet,
        options: BoardOptions,
    ) -> TournamentSync {
        let mut out = TournamentSync::default();
        let Some(tournament) = app.tournament() else {
            if self.had_tournament {
                *self = TournamentUi {
                    draft: self.draft.take(),
                    resume: self.resume.take(),
                    export_format: self.export_format,
                    now: self.now,
                    snapshot: Snapshot {
                        names: std::mem::take(&mut self.snapshot.names),
                        ..Snapshot::default()
                    },
                    ..TournamentUi::default()
                };
            }
            self.confirm = self
                .confirm
                .filter(|c| *c == TournamentConfirm::Quit && app.tournament_live());
            return out;
        };
        if !self.had_tournament {
            self.had_tournament = true;
            self.draft = None;
            out.show_pane = true;
            self.snapshot = Snapshot::default();
            self.tab = TournamentTab::Live;
        }
        let (done, total) = tournament.progress();
        let withdrawn = tournament.withdrawn().iter().filter(|w| **w).count();
        let key = (
            tournament.slug().to_owned(),
            done,
            total,
            tournament.log().len(),
            withdrawn,
        );
        if self.snapshot.key.as_ref() != Some(&key) {
            self.snapshot.standings = tournament.standings();
            self.snapshot.cross = tournament.cross_table();
            self.snapshot.ratings = tournament.ratings();
            self.snapshot.los = tournament.los_matrix();
            self.snapshot.time = tournament.time_report();
            self.snapshot.names = tournament.config().names();
            self.snapshot.key = Some(key);
        }
        self.snapshot.lanes = tournament.lanes();
        self.sync_live(app);
        let decision = matches!(tournament.phase(), Phase::Decision(_));
        if decision && !self.was_decision {
            self.decision_open = true;
            self.replace_with = None;
        }
        if !decision {
            self.decision_open = false;
        }
        self.was_decision = decision;
        self.sync_board(app, pieces, options);
        self.close_stale_confirm(app);
        out
    }

    /// A confirmation does not outlive what it is about.
    fn close_stale_confirm(&mut self, app: &AppState) {
        let Some(confirm) = self.confirm else { return };
        let phase = app.tournament().map(Tournament::phase);
        let live = app.tournament().is_some_and(|t| t.live().is_some());
        let valid = match confirm {
            TournamentConfirm::Quit => app.tournament_live(),
            TournamentConfirm::Abort => {
                !matches!(phase, None | Some(Phase::Finished | Phase::Aborted))
            }
            TournamentConfirm::Adjudicate(_) | TournamentConfirm::SkipCurrent => {
                live && phase == Some(Phase::Running)
            }
            TournamentConfirm::CloseTournament => phase.is_some(),
        };
        if !valid {
            self.confirm = None;
        }
    }

    fn sync_board(&mut self, app: &AppState, pieces: &PieceSet, options: BoardOptions) {
        let Some(tournament) = app.tournament() else {
            return;
        };
        let Some(live) = tournament.live() else {
            return;
        };
        let Some(position) = live.position else {
            return;
        };
        let data = BoardData {
            position,
            last_move: live.moves.last().map(|m| m.mv),
            marks: &[],
            orientation: TournamentUi::orientation(app),
            options,
            accepts_moves: false,
            brush: &self.brush,
            pieces,
            animation: None,
        };
        self.caches.sync_data(&data);
    }

    /// Replays the new moves of the live game in SAN (only the new ones).
    fn sync_live(&mut self, app: &AppState) {
        let live_view = app.tournament().and_then(Tournament::live);
        let Some(live) = live_view else {
            self.snapshot.live = LiveLines::default();
            return;
        };
        let (Some(start), moves) = (live.start, live.moves) else {
            self.snapshot.live = LiveLines::default();
            return;
        };
        let lines = &mut self.snapshot.live;
        let key = Some((live.number, live.attempt));
        if lines.key != key || lines.done > moves.len() {
            *lines = LiveLines {
                key,
                replay: Some(start.clone()),
                ..LiveLines::default()
            };
        }
        while lines.done < moves.len() {
            let Some(played) = moves.get(lines.done) else {
                break;
            };
            let Some(position) = lines.replay.as_mut() else {
                break;
            };
            let number = position.fullmove_number();
            let side = position.turn();
            let mut san = position.san(played.mv).unwrap_or_else(|_| "?".to_owned());
            if let Some(score) = played.score {
                // The note is from the point of view of the mover.
                let cp = score.score.as_centipawns();
                san.push(' ');
                san.push_str(&crate::windows::tournament_text::score_text(cp));
                if let Some(depth) = score.depth {
                    let _ = write!(san, "/{depth}");
                }
            }
            if position.play(played.mv).is_err() {
                lines.replay = None;
            }
            match side {
                Color::White => {
                    lines.lines.push(format!("{number}. {san}"));
                    lines.white_open = true;
                }
                Color::Black => {
                    match lines.lines.last_mut().filter(|_| lines.white_open) {
                        Some(last) => {
                            last.push_str("   ");
                            last.push_str(&san);
                        }
                        None => lines.lines.push(format!("{number}... {san}")),
                    }
                    lines.white_open = false;
                }
            }
            lines.done += 1;
        }
    }

    /// Applies an event of the form, the panel or the boxes.
    pub fn update(&mut self, message: TournamentUiMessage, app: &AppState) -> Outcome {
        use TournamentUiMessage as M;
        let at = Instant::now();
        match message {
            M::Open | M::Restart => {
                if app.tournament_live() || app.play_live() {
                    return Outcome::default();
                }
                let fen = app_api::board_view(app).position.fen();
                self.draft = Some(Box::new(TournamentDraft::new(app, fen)));
                // The limits of the machine for the field of lanes (D2502).
                return Outcome::send(AppMessage::Tournament(TournamentMessage::ProbeResources));
            }
            M::Cancel => self.draft = None,
            M::Edit(edit) => {
                if let Some(draft) = self.draft.as_mut() {
                    draft.edit(edit, app);
                }
            }
            M::Submit => return self.submit(at),
            M::OpenEngines => {
                return Outcome::send(AppMessage::Engine(
                    mychess_app::engines::EngineMessage::OpenDialog,
                ));
            }
            M::OpenResume => {
                if app.tournament_live() || app.play_live() {
                    return Outcome::default();
                }
                self.resume = Some(ResumeUi::default());
                return Outcome::send(AppMessage::Tournament(TournamentMessage::ListResumable));
            }
            M::ResumeClose => self.resume = None,
            M::ResumePick(path) => {
                self.resume = None;
                self.draft = None;
                return Outcome::send(AppMessage::Tournament(TournamentMessage::Resume {
                    path,
                    replacements: Vec::new(),
                    at,
                }));
            }
            M::Tab(tab) => self.tab = tab,
            M::Watch(lane) => {
                return Outcome::send(AppMessage::Tournament(TournamentMessage::Watch(lane)));
            }
            M::Scrolled(y) => self.scroll = y,
            M::Select(number) => self.selected = Some(number),
            M::Cell(i, j) => {
                let first = self
                    .snapshot
                    .cross
                    .get(i)
                    .and_then(|row| row.get(j))
                    .and_then(|games| games.first())
                    .map(|(number, _)| *number);
                if let Some(number) = first {
                    self.selected = Some(number);
                    self.tab = TournamentTab::Pairings;
                    // Rows are `ROW_HEIGHT` apart and numbered from 1.
                    #[allow(clippy::cast_precision_loss)]
                    let offset = (number.saturating_sub(1) as f32) * ROW_HEIGHT;
                    self.scroll = offset;
                }
            }
            M::ExportFormat(format) => self.export_format = format,
            M::ReplaceWith(id) => self.replace_with = Some(id),
            M::DecisionLater => self.decision_open = false,
            M::DecisionReopen => self.decision_open = true,
            M::Noop | M::Tick(_) | M::BookPicked(_) | M::SuitePicked(_) => {}
            other => return self.command(other, app, at),
        }
        Outcome::default()
    }

    /// "Start": the form becomes preferences (saved at once) and a start.
    fn submit(&mut self, at: Instant) -> Outcome {
        let Some(draft) = self.draft.as_ref() else {
            return Outcome::default();
        };
        if draft.error().is_some() {
            return Outcome::default();
        }
        let config = draft.to_config();
        self.draft = None;
        Outcome {
            messages: vec![
                app_api::set_tournament_prefs(config.clone()),
                AppMessage::Tournament(TournamentMessage::Start {
                    config: Box::new(config),
                    at,
                }),
            ],
            ..Outcome::default()
        }
    }

    /// The buttons of the panel and the answers of the boxes.
    #[allow(clippy::needless_pass_by_value)] // the arms take the payloads
    fn command(&mut self, message: TournamentUiMessage, app: &AppState, at: Instant) -> Outcome {
        use TournamentUiMessage as M;
        let send = |m: TournamentMessage| Outcome::send(AppMessage::Tournament(m));
        match message {
            M::Pause => send(TournamentMessage::Pause),
            M::Continue => send(TournamentMessage::Continue(at)),
            M::StopAfter => send(TournamentMessage::StopAfterGame(at)),
            M::Request(confirm) => {
                self.confirm = Some(confirm);
                Outcome::default()
            }
            M::Confirm(yes) => {
                let Some(confirm) = self.confirm.take() else {
                    return Outcome::default();
                };
                if !yes {
                    return Outcome::default();
                }
                match confirm {
                    TournamentConfirm::Abort => send(TournamentMessage::Abort(at)),
                    TournamentConfirm::Adjudicate(result) => {
                        send(TournamentMessage::Adjudicate(result, at))
                    }
                    TournamentConfirm::SkipCurrent => send(TournamentMessage::SkipCurrent(at)),
                    TournamentConfirm::CloseTournament => send(TournamentMessage::Close),
                    // The shell quits (it owns the window).
                    TournamentConfirm::Quit => Outcome {
                        quit: true,
                        ..Outcome::default()
                    },
                }
            }
            M::Decide(choice) => {
                let Some(Phase::Decision(participant)) = app.tournament().map(Tournament::phase)
                else {
                    return Outcome::default();
                };
                let decision = match choice {
                    DecisionChoice::Retry => Decision::Retry,
                    DecisionChoice::Withdraw => Decision::Withdraw,
                    DecisionChoice::Replace => {
                        let Some(new) = self.replacement(app, participant) else {
                            return Outcome::default();
                        };
                        Decision::Replace(Box::new(new))
                    }
                };
                self.decision_open = false;
                send(TournamentMessage::Decide(decision, at))
            }
            M::Redo(n) => send(TournamentMessage::Redo(n)),
            M::SkipGame(n) => send(TournamentMessage::SkipPending(n)),
            M::ToFront(n) => send(TournamentMessage::MoveToFront(n)),
            M::OpenPgn => send(TournamentMessage::OpenPgn),
            M::CloseTournament => {
                let over = app
                    .tournament()
                    .is_none_or(|t| matches!(t.phase(), Phase::Finished | Phase::Aborted));
                if over {
                    send(TournamentMessage::Close)
                } else {
                    self.confirm = Some(TournamentConfirm::CloseTournament);
                    Outcome::default()
                }
            }
            _ => Outcome::default(),
        }
    }

    /// The participant that takes the place of `participant`: the engine
    /// chosen, with the name and the options of the one that leaves when
    /// that is possible.
    fn replacement(&self, app: &AppState, participant: usize) -> Option<Participant> {
        let id = self.replace_with?;
        let entry = playable_engines(app).find(|e| e.id == id)?;
        let tournament = app.tournament()?;
        let old = tournament.config().participants.get(participant)?;
        let taken = |name: &str| {
            tournament
                .config()
                .participants
                .iter()
                .any(|p| p.name.eq_ignore_ascii_case(name))
        };
        let name = if taken(&entry.name) {
            (2..=u32::MAX)
                .map(|n| format!("{} {n}", entry.name))
                .find(|c| !taken(c))?
        } else {
            entry.name.clone()
        };
        let mut new = Participant::new(id.0, &name);
        new.engine_name.clone_from(&entry.name);
        new.elo = entry.elo;
        new.options = old
            .options
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case("Hash") || n.eq_ignore_ascii_case("Threads"))
            .cloned()
            .collect();
        Some(new)
    }
}

/// What a synchronization asks of the shell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TournamentSync {
    /// A tournament just appeared: show the panel.
    pub show_pane: bool,
}
