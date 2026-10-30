//! The "Play against the engine" dialog and the state of the game against an
//! engine on screen (spec M8-1b, D1220-D1253, D1254-D1262).
//!
//! The application owns the game (`mychess_app::play`); what is here is the
//! form being edited ([`PlayDraft`], a copy of the `[play]` preferences with
//! the numeric fields kept as text), the start that waits for the "discard
//! changes?" question ([`PlayUi::pending`]) and the strips with the clocks
//! ([`PlayUi::strips`]). The form is built from the Scid's dialog
//! (`sergame.tcl` `config`, 31-324): engine, colour, time control, start
//! position, book, specific opening and strength.
//!
//! [`PlayUi::update`] never touches the application: it returns the
//! messages to send in an [`Outcome`].

use std::path::PathBuf;
use std::time::{Duration, Instant};

use fluent_bundle::FluentArgs;
use iced::widget::{
    button, column, container, row, scrollable, text, text_editor, text_input, tooltip,
};
use iced::{Alignment, Element, Length};
use mychess_app::engines::{EngineEntry, EngineId, EngineMessage, Protocol};
use mychess_app::tactical_game::{EngineCaps, TACTICAL_ELO_RANGE, TacticalStartPref};
use mychess_app::{
    AppState, BONUS_RANGE, COACH_DEPTH_RANGE, COACH_SECONDS_RANGE, Command, MINUTES_RANGE,
    MOVETIME_RANGE, Message as AppMessage, NODES_RANGE, OpeningLine, PLAY_DEPTH_RANGE,
    PlayBookPref, PlayColorPref, PlayConfig, PlayMessage, PlayModePref, PlayPhase, PlayPrefs,
    PlayStartPref, PlayerStrips, SetupMessage, StartFrom,
};

use crate::app_api;
use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::widgets::modal::{Card, modal};
use crate::windows::annotate_dialog::{LABEL_WIDTH, check, choice, group, labelled};
use crate::windows::dialog_ui::Outcome;
use crate::windows::tactical_dialog;

/// Id of the configuration box.
pub const PLAY_BOX_ID: &str = "play-box";

const BODY_HEIGHT: f32 = 480.0;
const FIELD_WIDTH: f32 = 72.0;
const LINES_HEIGHT: f32 = 90.0;
/// Largest value typed in a numeric field (the text is cut to this many
/// digits).
const MAX_DIGITS: usize = 5;
/// Limits of `UCI_Elo` and `Skill Level` when the engine has not declared
/// its own (they are the limits of `PlayPrefs::sanitized`).
const ELO_RANGE: (u32, u32) = (0, 10_000);
const SKILL_RANGE: (u32, u32) = (0, 100);

/// A numeric field of the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayNumber {
    /// Minutes of White (`timebonus`).
    WhiteMinutes,
    /// Increment of White in seconds.
    WhiteBonus,
    /// Minutes of Black.
    BlackMinutes,
    /// Increment of Black in seconds.
    BlackBonus,
    /// Depth of the `depth` mode.
    Depth,
    /// Nodes of the `nodes` mode.
    Nodes,
    /// Seconds of the `movetime` mode.
    MoveTime,
    /// `UCI_Elo`.
    Elo,
    /// `Skill Level`.
    Skill,
    /// Depth of each search of the coach (M8-1d).
    CoachDepth,
    /// Seconds each search of the coach may last (M8-1d).
    CoachSeconds,
    /// The Elo of the engine in a tactical game (M8-7, D2470).
    TacticalElo,
}

impl PlayNumber {
    fn slot(self) -> usize {
        self as usize
    }

    /// The range the application accepts for the field.
    pub fn range(self) -> (u32, u32) {
        match self {
            PlayNumber::WhiteMinutes | PlayNumber::BlackMinutes => MINUTES_RANGE,
            PlayNumber::WhiteBonus | PlayNumber::BlackBonus => BONUS_RANGE,
            PlayNumber::Depth => PLAY_DEPTH_RANGE,
            PlayNumber::Nodes => NODES_RANGE,
            PlayNumber::MoveTime => MOVETIME_RANGE,
            PlayNumber::Elo => ELO_RANGE,
            PlayNumber::Skill => SKILL_RANGE,
            PlayNumber::CoachDepth => COACH_DEPTH_RANGE,
            PlayNumber::CoachSeconds => COACH_SECONDS_RANGE,
            PlayNumber::TacticalElo => TACTICAL_ELO_RANGE,
        }
    }
}

const FIELDS: usize = 12;

/// A message of the game against an engine for the visual state.
#[derive(Debug, Clone)]
pub enum PlayUiMessage {
    /// Opens the configuration dialog.
    Open,
    /// Opens the configuration dialog of the tactical game (M8-7, D2470).
    OpenTactical,
    /// Where the tactical game starts.
    TacticalStart(TacticalStartPref),
    /// The FEN typed for the tactical game.
    TacticalFen(String),
    /// Change the Elo after each tactical game.
    TacticalAdjust(bool),
    /// The step of that change.
    TacticalAdjustStep(u32),
    /// Shows the native dialog that picks the file of the drawing.
    TacticalPickFile,
    /// The answer of that dialog (`None`: cancelled).
    TacticalFilePicked(Option<PathBuf>),
    /// "Reset the score": the first press asks, the second does it.
    TacticalResetStats,
    /// The engine was picked.
    Engine(EngineId),
    /// The colour was picked.
    Color(PlayColorPref),
    /// The time mode was picked.
    Mode(PlayModePref),
    /// A numeric field was edited.
    Number(PlayNumber, String),
    /// Where the game starts.
    Start(PlayStartPref),
    /// The specific opening, by name.
    Opening(String),
    /// The list of openings was edited.
    OpeningsEdit(text_editor::Action),
    /// Play from a book (needs a file).
    UseBook(bool),
    /// How the book move is picked.
    BookSelection(PlayBookPref),
    /// Shows the native dialog that picks the book file.
    PickBook,
    /// The answer of that dialog (`None`: cancelled).
    BookPicked(Option<PathBuf>),
    /// Limit the strength (`UCI_Elo`).
    LimitStrength(bool),
    /// Use `Skill Level`.
    UseSkill(bool),
    /// Opens the Engines dialog over this one.
    OpenEngines,
    /// "Play": saves the preferences and starts.
    Submit,
    /// "Cancel".
    Cancel,
    /// The button Resign.
    Resign,
    /// The button Restart.
    Restart,
    /// The button Rematch: again, with the colours swapped (M8-12, D3101).
    Rematch,
    /// The button Quit (also closes the end box).
    Quit,
    /// The button Resume.
    Resume,
    /// The button Take back (M8-1c, D1507).
    TakeBack,
    /// The button Offer draw (M8-1c, D1507).
    OfferDraw,
    /// Write the clock in the comments of the moves (`Store clock`).
    StoreClock(bool),
    /// Think on the player's time (M8-1d, D2400).
    Ponder(bool),
    /// The coach watches the moves (M8-1d, D2429).
    Coach(bool),
    /// The engine of the coach.
    CoachEngine(EngineId),
    /// The coach names the better move.
    CoachHint(bool),
    /// The coach shows the loss in pawns.
    CoachNumbers(bool),
    /// The answer to a question of the game.
    Answer(bool),
    /// The answer to the confirmation of Resign, Restart or Quit in the
    /// middle of a game (D1480).
    Confirm(bool),
    /// After the engine was lost: another engine takes over.
    SwitchEngine(EngineId),
    /// "Save game" of the end box.
    SaveGame,
}

/// What a game in progress asks to confirm before it is given up (D1480).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayConfirm {
    /// Resign: the game is lost.
    Resign,
    /// Restart: the game in progress is abandoned.
    Restart,
    /// Quit: the game is left without a result and without saving.
    Quit,
}

impl PlayConfirm {
    /// The message that carries the action out.
    fn message(self, at: Instant) -> AppMessage {
        app_play(match self {
            PlayConfirm::Resign => PlayMessage::Resign(at),
            PlayConfirm::Restart => PlayMessage::Restart(at),
            PlayConfirm::Quit => PlayMessage::Quit(at),
        })
    }
}

/// Whether the game is in progress: giving it up loses something and so
/// asks first. Paused counts (the engine is still there); a lost engine and
/// a finished game have their own boxes.
pub fn in_progress(phase: Option<PlayPhase>) -> bool {
    matches!(
        phase,
        Some(
            PlayPhase::Player
                | PlayPhase::Opponent
                | PlayPhase::Asking(_)
                | PlayPhase::Paused { lost: false }
        )
    )
}

/// The seconds a clock shows: rounded up while counting down (the flag
/// falls at zero), down in the modes that only count (D1258).
pub fn shown_seconds(time: Duration, countdown: bool) -> u32 {
    let whole = time.as_secs() + u64::from(countdown && time.subsec_nanos() > 0);
    u32::try_from(whole).unwrap_or(u32::MAX)
}

/// The form being edited.
#[derive(Debug)]
pub struct PlayDraft {
    /// The preferences being edited (the numbers are in `texts`).
    pub prefs: PlayPrefs,
    /// The form is the one of the tactical game (M8-7, D2470).
    pub tactical: bool,
    /// "Reset the score" was pressed once and waits for the second press.
    pub reset_armed: bool,
    /// The engine picked.
    pub engine: Option<EngineId>,
    texts: [String; FIELDS],
    /// The list of openings, one `Name: moves` per line.
    pub openings: text_editor::Content,
}

fn uci_engines(app: &AppState) -> impl Iterator<Item = &EngineEntry> {
    app.engines
        .engines()
        .iter()
        .filter(|e| e.protocol == Protocol::Uci)
}

/// The UCI engines that can coach next to `opponent` (all but it, D2416).
pub fn coach_candidates(app: &AppState, opponent: Option<EngineId>) -> Vec<&EngineEntry> {
    uci_engines(app)
        .filter(|e| Some(e.id) != opponent)
        .collect()
}

impl PlayDraft {
    /// The form with what the preferences remember. The engine is the
    /// remembered one when it is still a UCI engine of the list, else the
    /// first one (D1223).
    pub fn new(app: &AppState) -> PlayDraft {
        PlayDraft::build(app, false)
    }

    /// The form of the tactical game (M8-7, D2470): the same fields, the
    /// Elo instead of the strength options and more places to start from.
    pub fn new_tactical(app: &AppState) -> PlayDraft {
        PlayDraft::build(app, true)
    }

    fn build(app: &AppState, tactical: bool) -> PlayDraft {
        let mut prefs = (*app.prefs.play).clone();
        // The problem of the training is only there while a training is on.
        if tactical
            && prefs.tactical.start == TacticalStartPref::Problem
            && app.tactical_problem_position().is_none()
        {
            prefs.tactical.start = TacticalStartPref::New;
        }
        let engine = prefs
            .engine
            .map(EngineId)
            .filter(|id| uci_engines(app).any(|e| e.id == *id))
            .or_else(|| uci_engines(app).next().map(|e| e.id));
        let mut texts: [String; FIELDS] = Default::default();
        let mut set = |n: PlayNumber, v: u32| texts[n.slot()] = v.to_string();
        set(PlayNumber::WhiteMinutes, prefs.white_minutes);
        set(PlayNumber::WhiteBonus, prefs.white_bonus);
        set(PlayNumber::BlackMinutes, prefs.black_minutes);
        set(PlayNumber::BlackBonus, prefs.black_bonus);
        set(PlayNumber::Depth, prefs.depth);
        set(PlayNumber::Nodes, prefs.nodes);
        set(PlayNumber::MoveTime, prefs.movetime);
        set(PlayNumber::Elo, prefs.elo);
        set(PlayNumber::Skill, prefs.skill);
        set(PlayNumber::CoachDepth, prefs.coach_depth);
        set(PlayNumber::CoachSeconds, prefs.coach_seconds);
        set(PlayNumber::TacticalElo, prefs.tactical.elo);
        let openings = text_editor::Content::with_text(&prefs.openings.join("\n"));
        PlayDraft {
            prefs,
            tactical,
            reset_armed: false,
            engine,
            texts,
            openings,
        }
    }

    /// Keeps the engine picked among the UCI engines that still exist: one
    /// removed or changed to another protocol in the Engines dialog gives
    /// way to the first UCI engine, if any (D1482).
    pub fn revalidate(&mut self, app: &AppState) {
        if !self
            .engine
            .is_some_and(|id| uci_engines(app).any(|e| e.id == id))
        {
            self.engine = uci_engines(app).next().map(|e| e.id);
        }
    }

    /// The text of a numeric field.
    pub fn text(&self, field: PlayNumber) -> &str {
        &self.texts[field.slot()]
    }

    fn set_text(&mut self, field: PlayNumber, text: &str) {
        self.texts[field.slot()] = text
            .chars()
            .filter(char::is_ascii_digit)
            .take(MAX_DIGITS)
            .collect();
    }

    /// The lines of the openings list that parse.
    pub fn opening_lines(&self) -> Vec<OpeningLine> {
        OpeningLine::parse_list(&self.openings.text())
    }

    /// The preferences the form stands for: numbers read from the text
    /// (an empty field keeps the old value) and limited to their ranges.
    pub fn to_prefs(&self) -> PlayPrefs {
        let mut p = self.prefs.clone();
        p.engine = self.engine.map(|id| id.0);
        let read = |n: PlayNumber, old: u32| self.text(n).parse::<u32>().unwrap_or(old);
        p.white_minutes = read(PlayNumber::WhiteMinutes, p.white_minutes);
        p.white_bonus = read(PlayNumber::WhiteBonus, p.white_bonus);
        p.black_minutes = read(PlayNumber::BlackMinutes, p.black_minutes);
        p.black_bonus = read(PlayNumber::BlackBonus, p.black_bonus);
        p.depth = read(PlayNumber::Depth, p.depth);
        p.nodes = read(PlayNumber::Nodes, p.nodes);
        p.movetime = read(PlayNumber::MoveTime, p.movetime);
        p.elo = read(PlayNumber::Elo, p.elo);
        p.skill = read(PlayNumber::Skill, p.skill);
        p.coach_depth = read(PlayNumber::CoachDepth, p.coach_depth);
        p.coach_seconds = read(PlayNumber::CoachSeconds, p.coach_seconds);
        p.tactical.elo = read(PlayNumber::TacticalElo, p.tactical.elo);
        p.openings = self
            .openings
            .text()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect();
        p.sanitized()
    }
}

/// A start that waits for the question "discard the changes?" of the game
/// in the editor to be answered (D1252).
#[derive(Debug, Clone, PartialEq)]
pub struct PendingStart {
    /// The configuration to start with.
    pub config: PlayConfig,
}

/// Everything the screen keeps for the game against an engine.
#[derive(Debug)]
pub struct PlayUi {
    /// The configuration dialog, when open.
    pub draft: Option<Box<PlayDraft>>,
    /// The start that waits for the discard question.
    pub pending: Option<PendingStart>,
    /// The last time seen by an update; the views read it instead of the
    /// clock.
    pub now: Instant,
    /// The strips of the board with the clocks of the game, while it is on.
    pub strips: Option<PlayerStrips>,
    /// Whether the clocks of the strips count down (the "low time" tone
    /// only makes sense then, D1482).
    pub countdown: bool,
    /// Resign, Restart or Quit waiting for a Yes or No (D1480).
    pub confirm: Option<PlayConfirm>,
    /// A restart of a finished game waits for the discard question.
    pub pending_restart: bool,
    /// The same for a rematch (M8-12).
    pub pending_rematch: bool,
    /// A short remark of the game for the status bar (Fluent key).
    pub note: Option<&'static str>,
    /// Which premoves the board takes while the engine thinks (D4452),
    /// rebuilt by [`PlayUi::sync`].
    pub premove_rule: crate::windows::online_ui::PremoveRule,
}

impl Default for PlayUi {
    fn default() -> PlayUi {
        PlayUi {
            draft: None,
            pending: None,
            now: Instant::now(),
            strips: None,
            countdown: true,
            confirm: None,
            pending_restart: false,
            pending_rematch: false,
            note: None,
            premove_rule: crate::windows::online_ui::PremoveRule::default(),
        }
    }
}

fn app_play(message: PlayMessage) -> AppMessage {
    AppMessage::Play(message)
}

impl PlayUi {
    /// Applies an event of the form or of the panel of the game.
    pub fn update(&mut self, message: PlayUiMessage, app: &AppState) -> Outcome {
        let at = Instant::now();
        if let Some(out) = self.panel(&message, app, at) {
            return out;
        }
        match message {
            PlayUiMessage::Open => {
                if app.play_live() {
                    return Outcome::default();
                }
                self.draft = Some(Box::new(PlayDraft::new(app)));
            }
            PlayUiMessage::OpenTactical => {
                if app.play_live() {
                    return Outcome::default();
                }
                self.draft = Some(Box::new(PlayDraft::new_tactical(app)));
            }
            PlayUiMessage::TacticalResetStats => return self.reset_stats(app),
            PlayUiMessage::Cancel => self.draft = None,
            PlayUiMessage::Submit => return self.submit(app),
            PlayUiMessage::OpenEngines => {
                return Outcome::send(AppMessage::Engine(EngineMessage::OpenDialog));
            }
            other => {
                if let Some(draft) = self.draft.as_mut() {
                    draft.edit(other);
                }
            }
        }
        Outcome::default()
    }

    /// The buttons of the panel and of the boxes of the game itself.
    fn panel(&mut self, message: &PlayUiMessage, app: &AppState, at: Instant) -> Option<Outcome> {
        let sent = match message {
            PlayUiMessage::Resign => return Some(self.request(PlayConfirm::Resign, app, at)),
            PlayUiMessage::Restart => return Some(self.request(PlayConfirm::Restart, app, at)),
            PlayUiMessage::Quit => return Some(self.request(PlayConfirm::Quit, app, at)),
            PlayUiMessage::Rematch => return Some(self.rematch(app, at)),
            PlayUiMessage::Confirm(yes) => {
                let kind = self.confirm.take()?;
                if !*yes {
                    return Some(Outcome::default());
                }
                kind.message(at)
            }
            PlayUiMessage::Resume => app_play(PlayMessage::Resume(at)),
            PlayUiMessage::TakeBack => app_play(PlayMessage::TakeBack(at)),
            PlayUiMessage::OfferDraw => app_play(PlayMessage::OfferDraw(at)),
            PlayUiMessage::Answer(yes) => app_play(PlayMessage::Answer { yes: *yes, at }),
            PlayUiMessage::SwitchEngine(id) => app_play(PlayMessage::SwitchEngine(*id, at)),
            PlayUiMessage::SaveGame => AppMessage::Command(Command::Save),
            _ => return None,
        };
        Some(Outcome::send(sent))
    }

    /// Resign, Restart or Quit asked on the panel or in a box (D1480). In
    /// the middle of a game they ask for a confirmation first. A finished
    /// game that was not saved goes through the "discard changes?" question
    /// before it is restarted, as a new game does (D1255).
    fn request(&mut self, kind: PlayConfirm, app: &AppState, at: Instant) -> Outcome {
        let phase = app_api::play_phase(app);
        if in_progress(phase) {
            self.confirm = Some(kind);
            return Outcome::default();
        }
        if kind == PlayConfirm::Restart
            && matches!(phase, Some(PlayPhase::Ended(_)))
            && app.play_start_needs_discard()
        {
            self.pending_restart = true;
            return Outcome::send(AppMessage::Command(Command::NewGame));
        }
        Outcome::send(kind.message(at))
    }

    /// The rematch of a finished game (M8-12, D3101): it goes through the
    /// "discard changes?" question as a restart does, and does nothing when
    /// the game is not over.
    fn rematch(&mut self, app: &AppState, at: Instant) -> Outcome {
        if !matches!(app_api::play_phase(app), Some(PlayPhase::Ended(_))) {
            return Outcome::default();
        }
        if app.play_start_needs_discard() {
            self.pending_rematch = true;
            return Outcome::send(AppMessage::Command(Command::NewGame));
        }
        Outcome::send(app_play(PlayMessage::Rematch(at)))
    }

    /// "Reset the score" (D2463): the first press asks, the second clears
    /// the score of the application.
    fn reset_stats(&mut self, app: &AppState) -> Outcome {
        let Some(draft) = self.draft.as_mut() else {
            return Outcome::default();
        };
        if !draft.reset_armed {
            draft.reset_armed = true;
            return Outcome::default();
        }
        draft.reset_armed = false;
        let mut prefs = (*app.prefs.play).clone();
        prefs.tactical.stats.reset();
        Outcome::send(app_api::set_play_prefs(prefs))
    }

    /// "Play": the form becomes preferences (saved at once) and a start.
    /// A game with unsaved changes is replaced through the question that
    /// already exists (D1255): the start follows its answer.
    fn submit(&mut self, app: &AppState) -> Outcome {
        let Some(draft) = self.draft.as_ref() else {
            return Outcome::default();
        };
        let Some(engine) = draft
            .engine
            .filter(|id| uci_engines(app).any(|e| e.id == *id))
        else {
            return Outcome::default();
        };
        let mut prefs = draft.to_prefs();
        // The coach engine is one of the others; without one, no coach
        // (D2416, D2429).
        let candidates = coach_candidates(app, Some(engine));
        match prefs
            .coach_engine
            .filter(|id| candidates.iter().any(|e| e.id.0 == *id))
            .or_else(|| candidates.first().map(|e| e.id.0))
        {
            Some(id) => prefs.coach_engine = Some(id),
            None => prefs.coach = false,
        }
        let tactical = draft.tactical;
        let config = if tactical {
            // The score belongs to the application: the form only reads it
            // (D2464).
            prefs.tactical.stats = app.prefs.play.tactical.stats;
            let start = prefs
                .tactical
                .start_from(&prefs, app.tactical_problem_position());
            let Ok(start) = start else {
                // The form shows what is wrong; "Play" is off meanwhile.
                return Outcome::default();
            };
            prefs.tactical.to_config(&prefs, engine, start)
        } else {
            prefs.to_config(engine)
        };
        self.draft = None;
        let mut messages = vec![app_api::set_play_prefs(prefs)];
        if app.play_start_needs_discard() {
            // The question belongs to the replacement of the game: a new
            // game, or the position on the board when that is where the
            // game starts (it goes through "set up position", whose OK
            // replaces the game with that position).
            if config.start == StartFrom::CurrentPosition {
                messages.push(AppMessage::Command(Command::SetupBoard));
                messages.push(AppMessage::Setup(SetupMessage::Ok));
            } else {
                messages.push(AppMessage::Command(Command::NewGame));
            }
            self.pending = Some(PendingStart { config });
        } else {
            messages.push(app_play(PlayMessage::Start(Box::new(config))));
        }
        Outcome {
            messages,
            ..Outcome::default()
        }
    }

    /// Called after every update: once the discard question is answered,
    /// the waiting start (or restart) goes ahead if the game was really
    /// replaced, and is dropped if the user cancelled (or the save is still
    /// under way). A dropped one leaves a remark saying the button must be
    /// pressed again (D1481).
    pub fn resume_pending(&mut self, app: &AppState) -> Option<AppMessage> {
        if app.discard_prompt.is_some() {
            return None;
        }
        if std::mem::take(&mut self.pending_restart) {
            if app.play_start_needs_discard() {
                self.note = Some("play-restart-dropped");
                return None;
            }
            return Some(app_play(PlayMessage::Restart(Instant::now())));
        }
        if std::mem::take(&mut self.pending_rematch) {
            if app.play_start_needs_discard() {
                self.note = Some("play-rematch-dropped");
                return None;
            }
            return Some(app_play(PlayMessage::Rematch(Instant::now())));
        }
        let pending = self.pending.take()?;
        if app.play_start_needs_discard() {
            self.note = Some("play-start-dropped");
            return None;
        }
        Some(app_play(PlayMessage::Start(Box::new(pending.config))))
    }

    /// Keeps the visual state in step with the application: the engine of
    /// the form follows the list of engines (also when the Engines dialog
    /// closes) and a confirmation does not outlive the game in progress.
    pub fn sync(&mut self, app: &AppState) {
        if let Some(draft) = self.draft.as_mut() {
            draft.revalidate(app);
        }
        if self.confirm.is_some() && !in_progress(app_api::play_phase(app)) {
            self.confirm = None;
        }
        if app_api::play_phase(app).is_none() {
            self.pending_restart = false;
            self.pending_rematch = false;
        }
        self.rebuild_premove_rule(app);
    }

    /// The board takes a premove only while the engine thinks, the
    /// premove is on and the board shows the position of the game (D4452).
    fn rebuild_premove_rule(&mut self, app: &AppState) {
        let rule = app
            .play_premove_context()
            .filter(|(position, _)| {
                crate::windows::online_ui::shown_is_current(app, position.zobrist())
            })
            .map(|(position, color)| {
                Box::new(move |from, to| {
                    mychess_app::online::is_pseudo_legal(&position, color, from, to)
                })
                    as Box<dyn Fn(mychess_core::Square, mychess_core::Square) -> bool + Send + Sync>
            });
        self.premove_rule = crate::windows::online_ui::PremoveRule::new(rule);
    }

    /// The strips of the board with the clocks of the game (D1258): the
    /// strips of the application with the clock of each side in the strip
    /// of that side. Built when the game or the position changes; the
    /// ticks only change the clocks ([`PlayUi::refresh_clocks`]).
    pub fn rebuild_strips(&mut self, app: &AppState) {
        let clocks = app_api::play_clocks(app, self.now);
        self.strips = clocks.map(|(white, black, countdown)| {
            let mut strips = app_api::player_strips(app).clone();
            strips.clock_slot = true;
            strips.white.clock = Some(shown_seconds(white, countdown));
            strips.black.clock = Some(shown_seconds(black, countdown));
            strips
        });
        if let Some((_, _, countdown)) = clocks {
            self.countdown = countdown;
        }
    }

    /// A tick of the clocks: changes the seconds shown, in place, only when
    /// they differ (D1483). Returns whether anything changed.
    pub fn refresh_clocks(&mut self, app: &AppState, now: Instant) -> bool {
        self.now = now;
        let Some((white, black, countdown)) = app_api::play_clocks(app, now) else {
            return false;
        };
        let Some(strips) = self.strips.as_mut() else {
            self.rebuild_strips(app);
            return true;
        };
        let white = Some(shown_seconds(white, countdown));
        let black = Some(shown_seconds(black, countdown));
        let changed = strips.white.clock != white || strips.black.clock != black;
        strips.white.clock = white;
        strips.black.clock = black;
        self.countdown = countdown;
        changed
    }
}

impl PlayDraft {
    fn edit(&mut self, message: PlayUiMessage) {
        match message {
            PlayUiMessage::Engine(id) => self.engine = Some(id),
            PlayUiMessage::Color(color) => self.prefs.color = color,
            PlayUiMessage::Mode(mode) => self.prefs.mode = mode,
            PlayUiMessage::Number(field, text) => self.set_text(field, &text),
            PlayUiMessage::Start(start) => self.prefs.start = start,
            PlayUiMessage::Opening(name) => {
                self.prefs.opening = name;
                self.prefs.start = PlayStartPref::Opening;
            }
            PlayUiMessage::OpeningsEdit(action) => self.openings.perform(action),
            PlayUiMessage::UseBook(on) => {
                self.prefs.use_book = on && self.prefs.book_path.is_some();
            }
            PlayUiMessage::BookSelection(selection) => self.prefs.book_selection = selection,
            PlayUiMessage::BookPicked(Some(path)) => {
                self.prefs.book_path = Some(path);
                self.prefs.use_book = true;
            }
            PlayUiMessage::TacticalStart(start) => self.prefs.tactical.start = start,
            PlayUiMessage::TacticalFen(text) => {
                self.prefs.tactical.fen = text
                    .chars()
                    .take(mychess_app::tactical_game::MAX_FEN_PREF_CHARS)
                    .collect();
            }
            PlayUiMessage::TacticalAdjust(on) => self.prefs.tactical.adjust = on,
            PlayUiMessage::TacticalAdjustStep(step) => self.prefs.tactical.adjust_step = step,
            PlayUiMessage::TacticalFilePicked(Some(path)) => {
                self.prefs.tactical.random_file = Some(path);
                self.prefs.tactical.start = TacticalStartPref::RandomFile;
            }
            PlayUiMessage::LimitStrength(on) => self.prefs.limit_strength = on,
            PlayUiMessage::UseSkill(on) => self.prefs.use_skill = on,
            PlayUiMessage::StoreClock(on) => self.prefs.store_clock = on,
            PlayUiMessage::Ponder(on) => self.prefs.ponder = on,
            PlayUiMessage::Coach(on) => self.prefs.coach = on,
            PlayUiMessage::CoachEngine(id) => self.prefs.coach_engine = Some(id.0),
            PlayUiMessage::CoachHint(on) => self.prefs.coach_hint = on,
            PlayUiMessage::CoachNumbers(on) => self.prefs.coach_numbers = on,
            _ => {}
        }
    }
}

// ---- the view ----

pub(super) fn send(message: PlayUiMessage) -> Message {
    Message::PlayUi(message)
}

/// What the engine declares about its strength options (D1237): the ranges
/// of `UCI_Elo` and `Skill Level`, or `None` when it has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strength {
    /// The engine started and told its options.
    pub known: bool,
    /// `UCI_LimitStrength` with the range of `UCI_Elo`.
    pub elo: Option<(i64, i64)>,
    /// The range of `Skill Level`.
    pub skill: Option<(i64, i64)>,
}

/// The strength options of `engine`, from its identity when it has run.
pub fn declared_strength(app: &AppState, engine: Option<EngineId>) -> Strength {
    let identity = engine
        .and_then(|id| app.engine_runs().iter().find(|r| r.engine == id))
        .and_then(|run| run.identity.as_ref());
    let Some(identity) = identity else {
        return Strength {
            known: false,
            elo: None,
            skill: None,
        };
    };
    // One reading of the options for the form and for the plan of the
    // tactical game (M8-7, D2452): `EngineCaps`.
    let caps = EngineCaps::from_options(&identity.options);
    Strength {
        known: true,
        elo: caps.limit_elo,
        skill: caps.skill,
    }
}

pub(super) fn range_hint(i18n: &I18n, (min, max): (u32, u32)) -> String {
    let mut args = FluentArgs::new();
    args.set("min", i64::from(min));
    args.set("max", i64::from(max));
    i18n.tr_args("play-range", &args)
}

pub(super) fn number_field<'a>(draft: &PlayDraft, field: PlayNumber) -> Element<'a, Message> {
    text_input("", draft.text(field))
        .on_input(move |t| send(PlayUiMessage::Number(field, t)))
        .on_submit(send(PlayUiMessage::Submit))
        .padding(4)
        .width(Length::Fixed(FIELD_WIDTH))
        .into()
}

pub(super) fn number_row<'a>(
    i18n: &I18n,
    draft: &PlayDraft,
    label: String,
    unit: &str,
    field: PlayNumber,
) -> Element<'a, Message> {
    labelled(
        label,
        row![
            number_field(draft, field),
            text(unit.to_owned()).size(13),
            text(range_hint(i18n, field.range()))
                .size(12)
                .style(theme::muted_text),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
    )
}

fn engine_section<'a>(app: &AppState, draft: &PlayDraft, i18n: &I18n) -> Element<'a, Message> {
    let engines: Vec<&EngineEntry> = uci_engines(app).collect();
    if engines.is_empty() {
        return column![
            text(i18n.tr("play-no-engine")).style(text::danger),
            button(text(i18n.tr("play-open-engines")))
                .style(button::secondary)
                .on_press(send(PlayUiMessage::OpenEngines)),
        ]
        .spacing(6)
        .into();
    }
    let current = draft.engine.unwrap_or(EngineId(u64::MAX));
    let mut items: Vec<Element<'a, Message>> = engines
        .iter()
        .map(|e| {
            choice(e.name.clone(), e.id, current, true, |id: EngineId| {
                send(PlayUiMessage::Engine(id))
            })
        })
        .collect();
    items.push(
        tooltip(
            button(text(i18n.tr("play-configure-engine")).size(13))
                .padding([3, 8])
                .style(button::secondary)
                .on_press(send(PlayUiMessage::OpenEngines)),
            text(i18n.tr("play-configure-engine-tip")).size(12),
            tooltip::Position::Bottom,
        )
        .style(theme::tooltip_box)
        .into(),
    );
    labelled(i18n.tr("play-engine"), group(items))
}

fn color_section<'a>(draft: &PlayDraft, i18n: &I18n) -> Element<'a, Message> {
    let pick = |c: PlayColorPref| send(PlayUiMessage::Color(c));
    labelled(
        i18n.tr("play-color"),
        group(vec![
            choice(
                i18n.tr("play-color-white"),
                PlayColorPref::White,
                draft.prefs.color,
                true,
                pick,
            ),
            choice(
                i18n.tr("play-color-black"),
                PlayColorPref::Black,
                draft.prefs.color,
                true,
                pick,
            ),
            choice(
                i18n.tr("play-color-random"),
                PlayColorPref::Random,
                draft.prefs.color,
                true,
                pick,
            ),
        ]),
    )
}

fn time_section<'a>(draft: &PlayDraft, i18n: &I18n) -> Element<'a, Message> {
    let pick = |m: PlayModePref| send(PlayUiMessage::Mode(m));
    let mode = draft.prefs.mode;
    let modes = labelled(
        i18n.tr("play-mode"),
        group(vec![
            choice(
                i18n.tr("play-mode-timebonus"),
                PlayModePref::TimeBonus,
                mode,
                true,
                pick,
            ),
            choice(
                i18n.tr("play-mode-movetime"),
                PlayModePref::MoveTime,
                mode,
                true,
                pick,
            ),
            choice(
                i18n.tr("play-mode-depth"),
                PlayModePref::Depth,
                mode,
                true,
                pick,
            ),
            choice(
                i18n.tr("play-mode-nodes"),
                PlayModePref::Nodes,
                mode,
                true,
                pick,
            ),
        ]),
    );
    let mut page = column![modes].spacing(6);
    match mode {
        PlayModePref::TimeBonus => {
            let minutes = i18n.tr("play-unit-minutes");
            let seconds = i18n.tr("play-unit-bonus");
            page = page
                .push(number_row(
                    i18n,
                    draft,
                    i18n.tr("play-white-minutes"),
                    &minutes,
                    PlayNumber::WhiteMinutes,
                ))
                .push(number_row(
                    i18n,
                    draft,
                    i18n.tr("play-white-bonus"),
                    &seconds,
                    PlayNumber::WhiteBonus,
                ))
                .push(number_row(
                    i18n,
                    draft,
                    i18n.tr("play-black-minutes"),
                    &minutes,
                    PlayNumber::BlackMinutes,
                ))
                .push(number_row(
                    i18n,
                    draft,
                    i18n.tr("play-black-bonus"),
                    &seconds,
                    PlayNumber::BlackBonus,
                ));
        }
        PlayModePref::Depth => {
            page = page.push(number_row(
                i18n,
                draft,
                i18n.tr("play-depth"),
                &i18n.tr("play-unit-plies"),
                PlayNumber::Depth,
            ));
        }
        PlayModePref::Nodes => {
            page = page.push(number_row(
                i18n,
                draft,
                i18n.tr("play-nodes"),
                &i18n.tr("play-unit-nodes"),
                PlayNumber::Nodes,
            ));
        }
        _ => {
            page = page.push(number_row(
                i18n,
                draft,
                i18n.tr("play-movetime"),
                &i18n.tr("play-unit-seconds"),
                PlayNumber::MoveTime,
            ));
        }
    }
    page.into()
}

fn start_section<'a>(draft: &'a PlayDraft, i18n: &I18n) -> Element<'a, Message> {
    let pick = |s: PlayStartPref| send(PlayUiMessage::Start(s));
    let start = draft.prefs.start;
    let mut page = column![labelled(
        i18n.tr("play-start"),
        group(vec![
            choice(
                i18n.tr("play-start-new"),
                PlayStartPref::NewGame,
                start,
                true,
                pick,
            ),
            choice(
                i18n.tr("play-start-current"),
                PlayStartPref::Current,
                start,
                true,
                pick,
            ),
            choice(
                i18n.tr("play-start-opening"),
                PlayStartPref::Opening,
                start,
                true,
                pick,
            ),
        ]),
    )]
    .spacing(6);
    if start == PlayStartPref::Opening {
        page = page.push(opening_editor(draft, i18n));
    }
    page.into()
}

/// The list of named lines: the pick by name, the help and the text of the
/// list (shared with the tactical form).
pub(super) fn opening_editor<'a>(draft: &'a PlayDraft, i18n: &I18n) -> Element<'a, Message> {
    let mut page = column![].spacing(6);
    {
        let lines = draft.opening_lines();
        let mut names: Vec<Element<'a, Message>> = Vec::new();
        let all: Vec<String> = lines.iter().map(|l| l.name.clone()).collect();
        let current = all
            .iter()
            .position(|n| *n == draft.prefs.opening)
            .unwrap_or(usize::MAX);
        for (index, name) in all.iter().enumerate() {
            let all = all.clone();
            names.push(choice(
                name.clone(),
                index,
                current,
                true,
                move |i: usize| {
                    send(PlayUiMessage::Opening(
                        all.get(i).cloned().unwrap_or_default(),
                    ))
                },
            ));
        }
        if lines.is_empty() {
            names.push(
                text(i18n.tr("play-openings-empty"))
                    .size(12)
                    .style(theme::muted_text)
                    .into(),
            );
        }
        page = page
            .push(labelled(i18n.tr("play-openings-pick"), group(names)))
            .push(
                text(i18n.tr("play-openings-help"))
                    .size(12)
                    .style(theme::muted_text),
            )
            .push(
                text_editor(&draft.openings)
                    .placeholder("Sicilian: 1.e4 c5 2.Nf3 d6")
                    .on_action(|a| send(PlayUiMessage::OpeningsEdit(a)))
                    .height(Length::Fixed(LINES_HEIGHT)),
            );
    }
    page.into()
}

fn book_section<'a>(draft: &PlayDraft, i18n: &I18n) -> Element<'a, Message> {
    let prefs = &draft.prefs;
    let file = prefs
        .book_path
        .as_ref()
        .map_or_else(|| i18n.tr("play-book-none"), |p| p.display().to_string());
    let pick = |s: PlayBookPref| send(PlayUiMessage::BookSelection(s));
    let on = prefs.use_book && prefs.book_path.is_some();
    column![
        check(
            i18n.tr("play-book-use"),
            on,
            prefs.book_path.is_some(),
            |v| send(PlayUiMessage::UseBook(v)),
        ),
        labelled(
            i18n.tr("play-book-file"),
            row![
                button(text(i18n.tr("play-book-choose")).size(13))
                    .padding([3, 8])
                    .style(button::secondary)
                    .on_press(send(PlayUiMessage::PickBook)),
                text(file).size(12).style(theme::muted_text),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
        ),
        labelled(
            i18n.tr("play-book-selection"),
            group(vec![
                choice(
                    i18n.tr("play-book-weighted"),
                    PlayBookPref::Weighted,
                    prefs.book_selection,
                    on,
                    pick,
                ),
                choice(
                    i18n.tr("play-book-best"),
                    PlayBookPref::Best,
                    prefs.book_selection,
                    on,
                    pick,
                ),
            ]),
        ),
    ]
    .spacing(6)
    .into()
}

fn strength_section<'a>(app: &AppState, draft: &PlayDraft, i18n: &I18n) -> Element<'a, Message> {
    let strength = declared_strength(app, draft.engine);
    let mut page = column![].spacing(6);
    if strength.known && strength.elo.is_none() && strength.skill.is_none() {
        return text(i18n.tr("play-strength-none"))
            .size(12)
            .style(theme::muted_text)
            .into();
    }
    let range_of = |declared: Option<(i64, i64)>, fallback: (u32, u32)| {
        declared
            .and_then(|(lo, hi)| Some((u32::try_from(lo).ok()?, u32::try_from(hi).ok()?)))
            .unwrap_or(fallback)
    };
    if !strength.known || strength.elo.is_some() {
        let range = range_of(strength.elo, ELO_RANGE);
        page = page.push(
            row![
                check(
                    i18n.tr("play-strength-elo"),
                    draft.prefs.limit_strength,
                    true,
                    |v| send(PlayUiMessage::LimitStrength(v)),
                ),
                number_field(draft, PlayNumber::Elo),
                text(range_hint(i18n, range))
                    .size(12)
                    .style(theme::muted_text),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }
    if !strength.known || strength.skill.is_some() {
        let range = range_of(strength.skill, SKILL_RANGE);
        page = page.push(
            row![
                check(
                    i18n.tr("play-strength-skill"),
                    draft.prefs.use_skill,
                    true,
                    |v| send(PlayUiMessage::UseSkill(v)),
                ),
                number_field(draft, PlayNumber::Skill),
                text(range_hint(i18n, range))
                    .size(12)
                    .style(theme::muted_text),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }
    if !strength.known {
        page = page.push(
            text(i18n.tr("play-strength-unknown"))
                .size(12)
                .style(theme::muted_text),
        );
    }
    page.into()
}

fn help_section<'a>(app: &AppState, draft: &PlayDraft, i18n: &I18n) -> Element<'a, Message> {
    let candidates = coach_candidates(app, draft.engine);
    let mut page = column![
        check(i18n.tr("play-ponder"), draft.prefs.ponder, true, |v| {
            send(PlayUiMessage::Ponder(v))
        }),
        text(i18n.tr("play-ponder-unsupported"))
            .size(12)
            .style(theme::muted_text),
    ]
    .spacing(6);
    page = page.push(check(
        i18n.tr("play-coach"),
        draft.prefs.coach && !candidates.is_empty(),
        !candidates.is_empty(),
        |v| send(PlayUiMessage::Coach(v)),
    ));
    if candidates.is_empty() {
        return page
            .push(
                text(i18n.tr("play-coach-no-engine"))
                    .size(12)
                    .style(theme::muted_text),
            )
            .into();
    }
    if draft.prefs.coach {
        let current = draft
            .prefs
            .coach_engine
            .filter(|id| candidates.iter().any(|e| e.id.0 == *id))
            .map_or_else(|| candidates[0].id, EngineId);
        let items: Vec<Element<'a, Message>> = candidates
            .iter()
            .map(|e| {
                choice(e.name.clone(), e.id, current, true, |id: EngineId| {
                    send(PlayUiMessage::CoachEngine(id))
                })
            })
            .collect();
        page = page
            .push(labelled(i18n.tr("play-coach-engine"), group(items)))
            .push(number_row(
                i18n,
                draft,
                i18n.tr("play-coach-depth"),
                &i18n.tr("play-unit-plies"),
                PlayNumber::CoachDepth,
            ))
            .push(number_row(
                i18n,
                draft,
                i18n.tr("play-coach-seconds"),
                &i18n.tr("play-unit-seconds"),
                PlayNumber::CoachSeconds,
            ))
            .push(check(
                i18n.tr("play-coach-hint"),
                draft.prefs.coach_hint,
                true,
                |v| send(PlayUiMessage::CoachHint(v)),
            ))
            .push(check(
                i18n.tr("play-coach-numbers"),
                draft.prefs.coach_numbers,
                true,
                |v| send(PlayUiMessage::CoachNumbers(v)),
            ));
    }
    page.into()
}

fn section_title<'a>(i18n: &I18n, key: &str) -> Element<'a, Message> {
    text(i18n.tr(key))
        .size(14)
        .style(theme::accent_text)
        .width(Length::Fixed(LABEL_WIDTH * 2.0))
        .into()
}

/// The configuration dialog, when open.
pub fn view<'a>(app: &AppState, ui: &'a PlayUi, i18n: &I18n) -> Option<Element<'a, Message>> {
    let draft = ui.draft.as_deref()?;
    let tactical = draft.tactical;
    let can_play = draft.engine.is_some() && tactical_dialog::start_problem(app, draft).is_none();
    let (start_title, start, strength_title, strength) = if tactical {
        (
            "play-tactical-section-start",
            tactical_dialog::start_section(app, draft, i18n),
            "play-tactical-section-strength",
            tactical_dialog::strength_section(app, draft, i18n),
        )
    } else {
        (
            "play-section-start",
            start_section(draft, i18n),
            "play-section-strength",
            strength_section(app, draft, i18n),
        )
    };
    let body = column![
        engine_section(app, draft, i18n),
        color_section(draft, i18n),
        section_title(i18n, "play-section-time"),
        time_section(draft, i18n),
        section_title(i18n, start_title),
        start,
        section_title(i18n, "play-section-book"),
        book_section(draft, i18n),
        section_title(i18n, strength_title),
        strength,
        section_title(i18n, "play-section-help"),
        help_section(app, draft, i18n),
        section_title(i18n, "play-section-record"),
        check(
            i18n.tr("play-store-clock"),
            draft.prefs.store_clock,
            true,
            |v| send(PlayUiMessage::StoreClock(v)),
        ),
    ]
    .spacing(10);
    let play = button(text(i18n.tr("play-submit")))
        .style(button::primary)
        .on_press_maybe(can_play.then(|| send(PlayUiMessage::Submit)));
    Some(modal(Card {
        id: PLAY_BOX_ID,
        title: i18n.tr(if tactical {
            "play-tactical-title"
        } else {
            "play-title"
        }),
        body: scrollable(container(body).padding([0, 12]))
            .height(Length::Fixed(BODY_HEIGHT))
            .into(),
        buttons: vec![
            button(text(i18n.tr("dialog-cancel")))
                .style(button::secondary)
                .on_press(send(PlayUiMessage::Cancel))
                .into(),
            play.into(),
        ],
        width: 640.0,
    }))
}

#[cfg(test)]
mod tests {
    use iced::{Settings, Size};
    use iced_test::Simulator;
    use mychess_app::engines::EngineDraft;

    use super::*;
    use crate::i18n::Language;
    use crate::shell::Shell;

    fn shell_with_engine() -> Shell {
        let mut shell = Shell::new(None);
        // Registers a UCI engine straight in the list.
        let draft = EngineDraft::new("Fake", "fake-engine");
        shell.app.engines.add(draft).expect("engine added");
        shell
    }

    #[test]
    fn the_form_reads_the_preferences_and_picks_the_first_uci_engine() {
        let shell = shell_with_engine();
        let draft = PlayDraft::new(&shell.app);
        assert_eq!(
            draft.engine,
            shell.app.engines.engines().first().map(|e| e.id)
        );
        assert_eq!(draft.text(PlayNumber::MoveTime), "6");
        assert_eq!(draft.text(PlayNumber::WhiteMinutes), "5");
    }

    #[test]
    fn numbers_keep_only_digits_and_are_limited_when_read() {
        let shell = shell_with_engine();
        let mut draft = PlayDraft::new(&shell.app);
        draft.edit(PlayUiMessage::Number(
            PlayNumber::MoveTime,
            "9a9b9".to_owned(),
        ));
        assert_eq!(draft.text(PlayNumber::MoveTime), "999");
        assert_eq!(draft.to_prefs().movetime, MOVETIME_RANGE.1);
        draft.edit(PlayUiMessage::Number(PlayNumber::Depth, String::new()));
        assert_eq!(draft.to_prefs().depth, 3, "empty keeps the old value");
        draft.edit(PlayUiMessage::Number(
            PlayNumber::Nodes,
            "12345678".to_owned(),
        ));
        assert_eq!(draft.text(PlayNumber::Nodes).len(), MAX_DIGITS);
        assert_eq!(draft.to_prefs().nodes, NODES_RANGE.1);
    }

    #[test]
    fn a_book_needs_a_file_and_a_picked_file_turns_it_on() {
        let shell = shell_with_engine();
        let mut draft = PlayDraft::new(&shell.app);
        draft.edit(PlayUiMessage::UseBook(true));
        assert!(!draft.prefs.use_book);
        draft.edit(PlayUiMessage::BookPicked(None));
        assert!(draft.prefs.book_path.is_none());
        draft.edit(PlayUiMessage::BookPicked(Some(PathBuf::from("a.bin"))));
        assert!(draft.prefs.use_book);
        draft.edit(PlayUiMessage::UseBook(false));
        assert!(!draft.prefs.use_book);
        assert!(draft.prefs.book_path.is_some());
    }

    #[test]
    fn the_list_of_openings_is_saved_without_blank_lines() {
        let shell = shell_with_engine();
        let mut draft = PlayDraft::new(&shell.app);
        draft.openings = text_editor::Content::with_text("A: e4 e5\n\n  B: d4 d5  \n");
        assert_eq!(draft.to_prefs().openings, ["A: e4 e5", "B: d4 d5"]);
        assert_eq!(draft.opening_lines().len(), 2);
        draft.edit(PlayUiMessage::Opening("B".to_owned()));
        assert_eq!(draft.prefs.start, PlayStartPref::Opening);
    }

    #[test]
    fn submit_saves_the_preferences_and_starts() {
        let shell = shell_with_engine();
        let mut ui = PlayUi::default();
        drop(ui.update(PlayUiMessage::Open, &shell.app));
        assert!(ui.draft.is_some());
        let out = ui.update(PlayUiMessage::Submit, &shell.app);
        assert!(ui.draft.is_none());
        assert!(ui.pending.is_none());
        assert!(matches!(
            out.messages.as_slice(),
            [
                AppMessage::Prefs(_),
                AppMessage::Play(PlayMessage::Start(_))
            ]
        ));
    }

    #[test]
    fn without_an_engine_the_form_cannot_start() {
        let shell = Shell::new(None);
        let mut ui = PlayUi::default();
        drop(ui.update(PlayUiMessage::Open, &shell.app));
        assert!(ui.draft.as_ref().is_some_and(|d| d.engine.is_none()));
        let out = ui.update(PlayUiMessage::Submit, &shell.app);
        assert_eq!(out.messages.len(), 0);
        assert!(ui.draft.is_some(), "the form stays open");
    }

    #[test]
    fn unsaved_changes_chain_the_discard_question_before_the_start() {
        let mut shell = shell_with_engine();
        let mv = shell
            .app
            .session
            .position()
            .find_move(
                "e2".parse().expect("square"),
                "e4".parse().expect("square"),
                None,
            )
            .expect("legal");
        drop(shell.update(Message::App(app_api::play_move(mv))));
        assert!(shell.app.play_start_needs_discard());
        let mut ui = PlayUi::default();
        drop(ui.update(PlayUiMessage::Open, &shell.app));
        let out = ui.update(PlayUiMessage::Submit, &shell.app);
        assert!(ui.pending.is_some());
        assert!(matches!(
            out.messages.as_slice(),
            [AppMessage::Prefs(_), AppMessage::Command(Command::NewGame)]
        ));
        // The question is up: nothing starts yet.
        for message in out.messages {
            drop(shell.update(Message::App(message)));
        }
        assert!(shell.app.discard_prompt.is_some());
        assert!(ui.resume_pending(&shell.app).is_none());
        assert!(ui.pending.is_some());
        // "Don't save": the game was replaced and the start goes ahead.
        drop(shell.update(Message::App(app_api::answer_discard(
            mychess_app::DiscardChoice::DontSave,
        ))));
        let resumed = ui.resume_pending(&shell.app);
        assert!(matches!(
            resumed,
            Some(AppMessage::Play(PlayMessage::Start(_)))
        ));
        assert!(ui.pending.is_none());
    }

    #[test]
    fn cancelling_the_discard_question_drops_the_start() {
        let mut shell = shell_with_engine();
        let mv = shell
            .app
            .session
            .position()
            .find_move(
                "d2".parse().expect("square"),
                "d4".parse().expect("square"),
                None,
            )
            .expect("legal");
        drop(shell.update(Message::App(app_api::play_move(mv))));
        let mut ui = PlayUi::default();
        drop(ui.update(PlayUiMessage::Open, &shell.app));
        let out = ui.update(PlayUiMessage::Submit, &shell.app);
        for message in out.messages {
            drop(shell.update(Message::App(message)));
        }
        drop(shell.update(Message::App(app_api::answer_discard(
            mychess_app::DiscardChoice::Cancel,
        ))));
        assert!(ui.resume_pending(&shell.app).is_none());
        assert!(ui.pending.is_none());
    }

    #[test]
    fn the_current_position_goes_through_the_setup_when_the_game_is_dirty() {
        let mut shell = shell_with_engine();
        let mv = shell
            .app
            .session
            .position()
            .find_move(
                "e2".parse().expect("square"),
                "e4".parse().expect("square"),
                None,
            )
            .expect("legal");
        drop(shell.update(Message::App(app_api::play_move(mv))));
        let mut ui = PlayUi::default();
        drop(ui.update(PlayUiMessage::Open, &shell.app));
        drop(ui.update(PlayUiMessage::Start(PlayStartPref::Current), &shell.app));
        let out = ui.update(PlayUiMessage::Submit, &shell.app);
        assert!(matches!(
            out.messages.as_slice(),
            [
                AppMessage::Prefs(_),
                AppMessage::Command(Command::SetupBoard),
                AppMessage::Setup(SetupMessage::Ok)
            ]
        ));
        for message in out.messages {
            drop(shell.update(Message::App(message)));
        }
        assert!(shell.app.discard_prompt.is_some());
        drop(shell.update(Message::App(app_api::answer_discard(
            mychess_app::DiscardChoice::DontSave,
        ))));
        // The game was replaced by the position that was on the board.
        assert_eq!(
            shell.app.session.position().fen(),
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1"
        );
        assert!(ui.resume_pending(&shell.app).is_some());
    }

    #[test]
    fn the_panel_buttons_send_the_messages_of_the_game() {
        let shell = Shell::new(None);
        let mut ui = PlayUi::default();
        let cases = [
            (PlayUiMessage::Resign, 0),
            (PlayUiMessage::Restart, 1),
            (PlayUiMessage::Quit, 2),
            (PlayUiMessage::Resume, 3),
            (PlayUiMessage::TakeBack, 4),
            (PlayUiMessage::OfferDraw, 5),
        ];
        for (message, which) in cases {
            let out = ui.update(message, &shell.app);
            let ok = matches!(
                (out.messages.as_slice(), which),
                ([AppMessage::Play(PlayMessage::Resign(_))], 0)
                    | ([AppMessage::Play(PlayMessage::Restart(_))], 1)
                    | ([AppMessage::Play(PlayMessage::Quit(_))], 2)
                    | ([AppMessage::Play(PlayMessage::Resume(_))], 3)
                    | ([AppMessage::Play(PlayMessage::TakeBack(_))], 4)
                    | ([AppMessage::Play(PlayMessage::OfferDraw(_))], 5)
            );
            assert!(ok, "{which}: {:?}", out.messages);
        }
        let out = ui.update(PlayUiMessage::Answer(true), &shell.app);
        assert!(matches!(
            out.messages.as_slice(),
            [AppMessage::Play(PlayMessage::Answer { yes: true, .. })]
        ));
    }

    #[test]
    fn clocks_round_up_while_counting_down_and_down_otherwise() {
        let t = Duration::from_millis(269_400);
        assert_eq!(shown_seconds(t, true), 270);
        assert_eq!(shown_seconds(t, false), 269);
        assert_eq!(shown_seconds(Duration::from_secs(5), true), 5);
        assert_eq!(shown_seconds(Duration::ZERO, true), 0);
        assert_eq!(shown_seconds(Duration::MAX, false), u32::MAX);
    }

    #[test]
    fn only_a_game_in_progress_asks_for_a_confirmation() {
        assert!(in_progress(Some(PlayPhase::Player)));
        assert!(in_progress(Some(PlayPhase::Opponent)));
        assert!(in_progress(Some(PlayPhase::Paused { lost: false })));
        assert!(!in_progress(Some(PlayPhase::Paused { lost: true })));
        assert!(!in_progress(Some(PlayPhase::Ended(
            mychess_app::PlayEnd::Stalemate
        ))));
        assert!(!in_progress(None));
    }

    #[test]
    fn the_engine_of_the_form_follows_the_list_of_engines() {
        let mut shell = shell_with_engine();
        let first = shell.app.engines.engines().first().map(|e| e.id);
        let second = shell
            .app
            .engines
            .add(EngineDraft::new("Second", "second"))
            .expect("added");
        let mut draft = PlayDraft::new(&shell.app);
        draft.engine = Some(second);
        draft.revalidate(&shell.app);
        assert_eq!(draft.engine, Some(second), "a valid choice is kept");
        shell.app.engines.remove(second).expect("removed");
        draft.revalidate(&shell.app);
        assert_eq!(draft.engine, first);
        let empty = Shell::new(None);
        draft.revalidate(&empty.app);
        assert_eq!(draft.engine, None);
    }

    #[test]
    fn the_dialog_is_drawn_in_both_languages_and_cancel_closes_it() {
        let shell = shell_with_engine();
        for (language, title, play) in [
            (Language::EnUs, "Play against the engine", "Play"),
            (Language::PtBr, "Jogar contra a engine", "Jogar"),
        ] {
            let ui = PlayUi {
                draft: Some(Box::new(PlayDraft::new(&shell.app))),
                ..PlayUi::default()
            };
            let i18n = I18n::new(language);
            let element = view(&shell.app, &ui, &i18n).expect("open");
            let mut sim =
                Simulator::with_size(Settings::default(), Size::new(900.0, 700.0), element);
            assert!(sim.find(title).is_ok(), "{title}");
            sim.click(play).expect("play button");
            let sent: Vec<Message> = sim.into_messages().collect();
            assert!(
                sent.iter()
                    .any(|m| matches!(m, Message::PlayUi(PlayUiMessage::Submit))),
                "{sent:?}"
            );
        }
    }

    #[test]
    fn with_no_uci_engine_the_form_warns_and_links_to_the_engines_dialog() {
        let shell = Shell::new(None);
        let ui = PlayUi {
            draft: Some(Box::new(PlayDraft::new(&shell.app))),
            ..PlayUi::default()
        };
        let i18n = I18n::new(Language::EnUs);
        let element = view(&shell.app, &ui, &i18n).expect("open");
        let mut sim = Simulator::with_size(Settings::default(), Size::new(900.0, 700.0), element);
        assert!(sim.find("Open Engines...").is_ok());
        sim.click("Open Engines...").expect("link");
        let sent: Vec<Message> = sim.into_messages().collect();
        assert!(
            sent.iter()
                .any(|m| matches!(m, Message::PlayUi(PlayUiMessage::OpenEngines)))
        );
    }

    #[test]
    fn strength_options_follow_what_the_engine_declares() {
        let shell = shell_with_engine();
        let none = declared_strength(&shell.app, None);
        assert!(!none.known);
        let id = shell.app.engines.engines().first().map(|e| e.id);
        let unknown = declared_strength(&shell.app, id);
        assert!(!unknown.known, "the engine has not run yet");
    }
}
