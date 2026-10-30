//! The analysis panel (spec M4-4, D437): one section per running engine
//! with its buttons, the MultiPV lines, the history of a single line, the
//! information lines, the exclusion box and the protocol log.
//!
//! The application keeps everything the panel shows ([`AnalysisView`], with
//! the SAN of every line already formatted), so `view` only lays it out:
//! the panel redraws about twenty times a second per engine. The visual
//! state (which section was used last, the text of the exclusion box, the
//! "Auto" switch of the log) is [`AnalysisUi`].

use std::path::PathBuf;

use iced::widget::text::Wrapping;
use iced::widget::{
    Id, button, checkbox, column, container, mouse_area, rich_text, row, scrollable, space, span,
    text, text_input, tooltip,
};
use iced::{Alignment, Element, Font, Length};
use mychess_app::engines::{EngineRun, EngineRunState, Protocol, RunId};
use mychess_app::{
    AllLinesMode, AnalysisMessage, AnalysisView, AnnotateMessage, AppState, AutomoveState, Display,
    EngineMode, Key, KeyInput, Message as AppMessage, ModeAvailability, ModeMessage, Modifiers,
    NamedKey, format_score,
};
use mychess_core::Move;

use super::analysis_cache::ViewCache;
use super::analysis_text::{self, pv_words};
use super::engine_text::failure_text;
use super::panes;
use crate::app_api;
use crate::fonts::NOTO_SANS_BOLD;
use crate::i18n::I18n;
use crate::shell::{Message, Shell, UiMessage};
use crate::theme::{self, ButtonLook};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::widgets::right_click::RightClick;

/// The id of the text field of the exclusion box.
pub const EXCLUDE_INPUT_ID: &str = "analysis-exclude-input";
/// The id of the field of the thinking time of the training.
pub const TRAINING_INPUT_ID: &str = "analysis-training-input";
/// The id of the search field of the log.
pub const LOG_SEARCH_ID: &str = "analysis-log-search";
/// How many lines of the history are drawn (the application keeps 200): a
/// longer list costs more to lay out than the eye can use.
pub const HISTORY_SHOWN: usize = 60;
/// How many log entries are drawn (the newest ones that match).
pub const LOG_SHOWN: usize = 400;
/// The largest value of the "Lines" stepper (the Scid's default range is
/// 1-8 when the engine does not say).
pub const STEPPER_MAX: u8 = 8;
/// The keys `1` to `5` fix the number of lines (D448).
pub const KEY_MULTIPV_MAX: u8 = 5;

/// A move of a PV that was clicked: the line number and the index of the
/// move in the PV.
type PvLink = (u8, usize);

/// What the panel remembers between frames (visual state only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisUi {
    /// The section the user used last: the keys of the panel act on it.
    pub focus: Option<RunId>,
    /// The pointer is over the panel: its keys work (D448, D811).
    pub hovered: bool,
    /// The text of the exclusion box.
    pub exclude_text: String,
    /// The log updates itself ("Auto").
    pub log_auto: bool,
    /// The short box of the thinking time of the training is open for this
    /// section, with the seconds typed so far (spec M4-8, D493).
    pub training: Option<(RunId, String)>,
    /// The text field of the panel that has the focus, if any (one of
    /// [`AnalysisUi::FIELD_IDS`]).
    pub focused_field: Option<&'static str>,
    /// The strings of the history and of the log, kept between frames
    /// (not state: see [`ViewCache`]).
    pub cache: ViewCache,
}

impl Default for AnalysisUi {
    fn default() -> AnalysisUi {
        AnalysisUi {
            focus: None,
            hovered: false,
            exclude_text: String::new(),
            log_auto: true,
            training: None,
            focused_field: None,
            cache: ViewCache::default(),
        }
    }
}

impl AnalysisUi {
    /// The panel was closed.
    pub fn pane_closed(&mut self) {
        self.focus = None;
        self.hovered = false;
        self.training = None;
        self.focused_field = None;
    }

    /// Whether one of the text fields of the panel has the focus.
    pub fn field_focused(&self) -> bool {
        self.focused_field.is_some()
    }

    /// Every text field of the panel, for the focus probe.
    pub const FIELD_IDS: [&'static str; 3] = [EXCLUDE_INPUT_ID, TRAINING_INPUT_ID, LOG_SEARCH_ID];
}

/// An event of the panel that only the front-end can finish: it needs the
/// modifier keys, a file dialog, the widget focus or the shell's view of
/// the game. The rest of the buttons send [`AnalysisMessage`] directly.
#[derive(Debug, Clone, PartialEq)]
pub enum AnalysisUiMessage {
    /// The pointer entered or left the panel.
    Hover(bool),
    /// "Add all lines": the mode comes from the button and the keys held.
    AddAll {
        /// The execution.
        run: RunId,
        /// The right button was used.
        secondary: bool,
    },
    /// Opens the exclusion box with the current exclusion as its text.
    OpenExclude(RunId),
    /// The text of the exclusion box changed.
    ExcludeInput(String),
    /// The "Training" button: asks the thinking time first, or stops the
    /// training when it is on (spec M4-8, D493).
    OpenTraining(RunId),
    /// The seconds field of the training box changed.
    TrainingInput(String),
    /// The training box was confirmed: sets the time and starts.
    TrainingSubmit(RunId),
    /// The training box was dismissed.
    TrainingCancel,
    /// The exclusion box was confirmed.
    ExcludeSubmit(RunId),
    /// Enter in the log filter: it only needs to be captured, so it does not
    /// reach the keys of the panel (D921).
    LogFilterSubmit,
    /// The answer to "does this field of the panel have the focus?".
    FieldFocus(&'static str, bool),
    /// The "Auto" switch of the log.
    LogAuto(bool),
    /// Time to read the new log entries of every open log.
    LogTickAll,
    /// "Save as...": shows the file dialog.
    PickLogFile(RunId),
    /// The answer of the dialog (`None`: cancelled).
    LogFilePicked(RunId, Option<PathBuf>),
    /// A move of a PV was clicked: plays the line up to it.
    PlayPv {
        /// The execution.
        run: RunId,
        /// The number of the line, from 1.
        line: u8,
        /// The index of the move in the PV.
        index: usize,
    },
}

/// The run an analysis message is about, if it names one.
pub fn run_of(message: &AnalysisMessage) -> Option<RunId> {
    use AnalysisMessage::{
        AddAllLines, AddLine, AddLineNumber, AddMove, AddScore, CloseExclude, CloseLog,
        CycleDisplay, LogTick, OpenExclude, OpenLog, RequestStats, SaveLog, SetDisplay,
        SetExclusion, SetLogFilter, SetMultiPv, Toggle, ToggleInfo, ToggleLock, ToggleLowPriority,
    };
    match message {
        Toggle(run)
        | AddMove(run)
        | AddScore(run)
        | AddLine(run)
        | AddLineNumber(run, _)
        | AddAllLines(run, _)
        | SetMultiPv(run, _)
        | ToggleLock(run)
        | OpenExclude(run)
        | CloseExclude(run)
        | SetExclusion(run, _)
        | ToggleLowPriority(run)
        | ToggleInfo(run)
        | SetDisplay(run, _)
        | CycleDisplay(run)
        | RequestStats(run)
        | OpenLog(run)
        | CloseLog(run)
        | LogTick(run)
        | SetLogFilter(run, _)
        | SaveLog(run, _) => Some(*run),
        _ => None,
    }
}

/// The message a key means in the panel (D448), for the section `run`.
/// `max_lines` is the largest number of lines the engine accepts. The digits
/// come from the key code; [`key_message_typed`] prefers the character typed.
pub fn key_message(input: &KeyInput, run: RunId, max_lines: Option<u8>) -> Option<AnalysisMessage> {
    key_message_typed(input, None, run, max_lines)
}

/// Like [`key_message`], with the character the key press typed: the digits
/// `1`-`5` are decided by it, so they work on layouts where the digit row
/// needs Shift (AZERTY). With a character typed the modifier test is only
/// "no Ctrl/Cmd and no Alt" (D922).
pub fn key_message_typed(
    input: &KeyInput,
    typed: Option<char>,
    run: RunId,
    max_lines: Option<u8>,
) -> Option<AnalysisMessage> {
    let plain = input.modifiers == Modifiers::NONE;
    match input.key {
        Key::Named(NamedKey::Enter) if plain => Some(AnalysisMessage::AddMove(run)),
        Key::Named(NamedKey::Enter) if input.modifiers == Modifiers::COMMAND => {
            Some(AnalysisMessage::AddLine(run))
        }
        Key::Named(NamedKey::Space) if plain => Some(AnalysisMessage::Toggle(run)),
        Key::Named(_) => None,
        Key::Char(key_char) => {
            let c = match typed {
                Some(c) if !input.modifiers.command && !input.modifiers.alt => c,
                None if plain => key_char,
                _ => return None,
            };
            let n = u8::try_from(c.to_digit(10).filter(|d| (1..=5).contains(d))?).ok()?;
            Some(AnalysisMessage::SetMultiPv(
                run,
                n.min(max_lines.unwrap_or(KEY_MULTIPV_MAX)).max(1),
            ))
        }
    }
}

/// The most lines the stepper and the keys allow for an engine.
pub fn max_lines(run: &EngineRun) -> Option<u8> {
    let range = run.identity.as_ref()?.capabilities.multipv.clone()?;
    Some((*range.end()).clamp(1, STEPPER_MAX))
}

/// What a section reads of an [`EngineRun`] (plain data, so a test can
/// build one).
#[derive(Debug, Clone, Copy)]
pub struct RunInfo<'a> {
    /// The execution.
    pub id: RunId,
    /// Where it is.
    pub state: &'a EngineRunState,
    /// The analysis is on.
    pub analyzing: bool,
    /// The analysis is locked on a position.
    pub locked: bool,
    /// The engine runs at low CPU priority.
    pub low_priority: bool,
    /// The largest number of lines it accepts; `None` without MultiPV.
    pub max_lines: Option<u8>,
    /// It can be told which moves to search.
    pub search_moves: bool,
    /// Its arrows are drawn on the board (the button is pressed, M4-5).
    pub arrows: bool,
}

impl<'a> RunInfo<'a> {
    /// The data of an execution.
    pub fn of(run: &'a EngineRun) -> RunInfo<'a> {
        RunInfo {
            id: run.id,
            state: &run.state,
            analyzing: run.analyzing,
            locked: run.locked,
            low_priority: run.low_priority,
            max_lines: max_lines(run),
            search_moves: run
                .identity
                .as_ref()
                .is_some_and(|i| i.capabilities.search_moves),
            arrows: false,
        }
    }
}

/// What a section shows of the engine modes and of the annotation (specs
/// M4-6 and M4-8). Plain data, so a test can build one; [`ModeView::of`]
/// reads it from the application. A section without one (`section`) has
/// none of those buttons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModeView {
    /// What the engine of the section is doing besides analysing the board.
    pub mode: EngineMode,
    /// Which of the three mode buttons can be used.
    pub avail: ModeAvailability,
    /// The engine is played by a shoot-out or a training: its MultiPV,
    /// exclusion and lock are out of reach (D852).
    pub commandeered: bool,
    /// An annotation is running.
    pub annotating: bool,
    /// The side to move is in check (no threat can be shown).
    pub in_check: bool,
}

impl ModeView {
    /// What the application says about the section of `run`.
    pub fn of(app: &AppState, run: RunId) -> ModeView {
        ModeView {
            mode: app.engine_mode(run),
            avail: app.mode_availability(run),
            commandeered: app.engine_commandeered(run),
            annotating: app.annotation_running(),
            in_check: app.session.position().is_check(),
        }
    }

    fn shoot_out(&self) -> bool {
        matches!(self.mode, EngineMode::ShootOut { .. })
    }
}

fn mode(message: ModeMessage) -> Message {
    Message::App(AppMessage::Mode(message))
}

/// The seconds typed in the box of the training, as a thinking time in
/// range; `None` when it is not a positive number. A comma is accepted as
/// the decimal mark.
pub fn parse_training_seconds(text_: &str) -> Option<std::time::Duration> {
    let seconds: f64 = text_.trim().replace(',', ".").parse().ok()?;
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    let range = mychess_app::AUTOMOVE_RANGE;
    let (low, high) = (range.start().as_secs_f64(), range.end().as_secs_f64());
    Some(std::time::Duration::from_secs_f64(seconds.clamp(low, high)))
}

fn am(message: AnalysisMessage) -> Message {
    Message::App(AppMessage::Analysis(message))
}

fn ui(message: AnalysisUiMessage) -> Message {
    Message::Ui(UiMessage::Analysis(message))
}

// ---- small widgets ----

fn tip<'a>(
    content: impl Into<Element<'a, Message>>,
    text_: String,
    position: tooltip::Position,
) -> Element<'a, Message> {
    tooltip(
        content,
        container(text(text_).size(12))
            .padding([4, 8])
            .style(theme::tooltip_box),
        position,
    )
    .into()
}

/// A flat text button of the bar. A disabled one is dimmed and its tooltip
/// says why.
fn bar_button<'a>(
    label: String,
    tip_text: String,
    message: Option<Message>,
    toggled: bool,
) -> Element<'a, Message> {
    let enabled = message.is_some();
    let mut label = text(label).size(12);
    if !enabled {
        label = label.style(theme::muted_text);
    }
    let body = button(label)
        .padding([3, 8])
        .style(theme::command_button(ButtonLook::Secondary, toggled))
        .on_press_maybe(message);
    tip(body, tip_text, tooltip::Position::Bottom)
}

/// A button of icon only.
fn icon_button<'a>(
    glyph: Icon,
    tip_text: String,
    message: Option<Message>,
    toggled: bool,
) -> Element<'a, Message> {
    let tone = if message.is_none() {
        IconTone::Disabled
    } else if toggled {
        IconTone::OnAccent
    } else {
        IconTone::Text
    };
    let body = button(icon(glyph, 14.0, tone))
        .padding([4, 8])
        .style(theme::command_button(ButtonLook::Secondary, toggled))
        .on_press_maybe(message);
    tip(body, tip_text, tooltip::Position::Bottom)
}

/// `button` with a right-click alternative.
fn with_right(button: Element<'_, Message>, right: Option<Message>) -> Element<'_, Message> {
    match right {
        Some(message) => RightClick::new(button, move |_, _| message.clone()).into(),
        None => button,
    }
}

// ---- the section ----

/// What a section needs besides the view of the execution.
pub struct Ctx<'a> {
    /// The texts.
    pub i18n: &'a I18n,
    /// The visual state.
    pub ui: &'a AnalysisUi,
    /// The log is kept at all (`log_lines > 0`).
    pub log_enabled: bool,
}

/// The data of one section.
pub struct Section<'a> {
    /// The execution.
    pub run: RunInfo<'a>,
    /// The name to show.
    pub name: &'a str,
    /// The protocol of the engine.
    pub protocol: Protocol,
    /// What the application shows for it.
    pub view: &'a AnalysisView,
}

/// The state of the section in words, and whether the lines may be acted on.
fn state_text(i18n: &I18n, section: &Section<'_>) -> String {
    let (run, view) = (section.run, section.view);
    if view.restarting {
        return i18n.tr("analysis-state-restarting");
    }
    match run.state {
        EngineRunState::Starting => i18n.tr("analysis-state-starting"),
        EngineRunState::Stopping => i18n.tr("analysis-state-stopping"),
        EngineRunState::Failed(failure) => {
            let mut args = fluent_bundle::FluentArgs::new();
            args.set("reason", failure_text(i18n, failure));
            i18n.tr_args("analysis-state-failed", &args)
        }
        _ => {
            if let Some(terminal) = view.terminal {
                i18n.tr(analysis_text::terminal_key(terminal))
            } else if !run.analyzing {
                i18n.tr("analysis-state-stopped")
            } else {
                String::new()
            }
        }
    }
}

fn header<'a>(section: &Section<'a>, ctx: &Ctx<'a>) -> Element<'a, Message> {
    let (run, view) = (section.run, section.view);
    let mut line = row![
        text(section.name.to_owned()).font(NOTO_SANS_BOLD).size(14),
        text(state_text(ctx.i18n, section))
            .size(12)
            .style(theme::muted_text),
        space::horizontal(),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if *run.state == EngineRunState::Running
        && let Some((score, bound)) = view.main_score()
    {
        line = line.push(
            text(format_score(score, bound))
                .font(NOTO_SANS_BOLD)
                .size(14),
        );
    }
    line = line.push(icon_button(
        Icon::Close,
        ctx.i18n.tr("analysis-tip-close"),
        Some(am(AnalysisMessage::Close(run.id))),
        false,
    ));
    line.into()
}

/// The stepper of the number of lines.
fn lines_stepper<'a>(section: &Section<'a>, ctx: &Ctx<'a>, editable: bool) -> Element<'a, Message> {
    let (run, view) = (section.run, section.view);
    let max = run.max_lines;
    let usable = editable && max.is_some();
    let n = view.multipv;
    let step = |label: &str, to: Option<u8>| {
        bar_button(
            label.to_owned(),
            ctx.i18n.tr("analysis-tip-lines"),
            to.filter(|_| usable)
                .map(|n| am(AnalysisMessage::SetMultiPv(run.id, n))),
            false,
        )
    };
    let down = (n > 1).then(|| n - 1);
    let up = max.filter(|m| n < *m).map(|_| n + 1);
    let body = row![
        text(ctx.i18n.tr("analysis-lines")).size(12),
        step("\u{2212}", down),
        text(n.to_string()).size(12).width(Length::Fixed(14.0)),
        step("+", up),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    let note = if max.is_none() {
        format!(
            "{} {}",
            ctx.i18n.tr("analysis-tip-lines"),
            ctx.i18n.tr("analysis-unsupported")
        )
    } else {
        ctx.i18n.tr("analysis-tip-lines")
    };
    tip(body, note, tooltip::Position::Bottom)
}

/// What a button of the bar needs besides the engine being ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// Nothing more.
    Nothing,
    /// An engine that speaks UCI.
    Uci,
    /// Lines to act on.
    Lines,
}

/// The Fluent key of the sentence that says why a button of the bar is
/// disabled, if there is a reason to say (the tooltip appends it). The first
/// that applies wins: the engine speaks XBoard and the button needs UCI
/// (`need == Need::Uci`), the position is locked, the engine is commanded by a mode
/// (shoot-out, training), there are no lines to act on yet (`need == Need::Lines`).
pub fn disabled_reason(
    protocol: Protocol,
    need: Need,
    locked: bool,
    commandeered: bool,
    no_lines: bool,
) -> Option<&'static str> {
    if need == Need::Uci && protocol != Protocol::Uci {
        Some("analysis-unsupported")
    } else if locked {
        Some("analysis-unavailable-locked")
    } else if commandeered {
        Some("analysis-tip-commandeered")
    } else if need == Need::Lines && no_lines {
        Some("analysis-unavailable-no-lines")
    } else {
        None
    }
}

// One flat list of the buttons of the bar.
#[allow(clippy::too_many_lines)]
fn button_bar<'a>(
    section: &Section<'a>,
    ctx: &Ctx<'a>,
    modes: Option<&ModeView>,
) -> Element<'a, Message> {
    let (run, view) = (section.run, section.view);
    let i18n = ctx.i18n;
    let id = run.id;
    let ready = *run.state == EngineRunState::Running && !view.restarting;
    let locked = run.locked;
    let commandeered = modes.is_some_and(|m| m.commandeered);
    let shoot_out = modes.is_some_and(ModeView::shoot_out);
    let can_edit = ready && run.analyzing && !locked && !view.lines.is_empty() && !shoot_out;
    let may_exclude = ready
        && run.analyzing
        && !locked
        && !commandeered
        && run.search_moves
        && section.protocol == Protocol::Uci;
    let with_reason = |text_: String, need: Need| match disabled_reason(
        section.protocol,
        need,
        locked,
        commandeered,
        view.lines.is_empty(),
    ) {
        Some(key) => format!("{text_} {}", i18n.tr(key)),
        None => text_,
    };
    // The buttons that add the lines to the game.
    let when_locked = |text_: String| with_reason(text_, Need::Lines);
    let go = |message: AnalysisMessage, on: bool| on.then(|| am(message));

    let toggle = if run.analyzing {
        icon_button(
            Icon::Pause,
            i18n.tr("analysis-tip-stop"),
            go(AnalysisMessage::Toggle(id), ready),
            false,
        )
    } else {
        icon_button(
            Icon::Play,
            i18n.tr("analysis-tip-start"),
            go(AnalysisMessage::Toggle(id), ready),
            false,
        )
    };
    let add_move = with_right(
        bar_button(
            i18n.tr("analysis-btn-add-move"),
            when_locked(i18n.tr("analysis-tip-add-move")),
            go(AnalysisMessage::AddMove(id), can_edit),
            false,
        ),
        go(AnalysisMessage::AddScore(id), can_edit),
    );
    let add_line = with_right(
        bar_button(
            i18n.tr("analysis-btn-add-line"),
            when_locked(i18n.tr("analysis-tip-add-line")),
            go(AnalysisMessage::AddLine(id), can_edit),
            false,
        ),
        go(AnalysisMessage::AddLineNumber(id, 2), can_edit),
    );
    let add_all_enabled = can_edit && view.multipv > 1;
    let add_all = with_right(
        bar_button(
            i18n.tr("analysis-btn-add-all"),
            when_locked(i18n.tr("analysis-tip-add-all")),
            add_all_enabled.then(|| {
                ui(AnalysisUiMessage::AddAll {
                    run: id,
                    secondary: false,
                })
            }),
            false,
        ),
        add_all_enabled.then(|| {
            ui(AnalysisUiMessage::AddAll {
                run: id,
                secondary: true,
            })
        }),
    );
    let lock_tip = match &view.lock {
        Some(lock) => {
            let mut args = fluent_bundle::FluentArgs::new();
            args.set("description", analysis_text::lock_description(i18n, lock));
            i18n.tr_args("analysis-tip-unlock", &args)
        }
        None => i18n.tr("analysis-tip-lock"),
    };
    let lock_tip = if commandeered {
        format!("{lock_tip} {}", i18n.tr("analysis-tip-commandeered"))
    } else {
        lock_tip
    };
    let lock = bar_button(
        i18n.tr(if locked {
            "analysis-btn-locked"
        } else {
            "analysis-btn-lock"
        }),
        lock_tip,
        go(
            AnalysisMessage::ToggleLock(id),
            ready && run.analyzing && !commandeered,
        ),
        locked,
    );
    let exclude_tip = if view.exclusion_san.is_empty() {
        i18n.tr("analysis-tip-exclude")
    } else {
        let mut args = fluent_bundle::FluentArgs::new();
        args.set("moves", view.exclusion_san.join(" "));
        i18n.tr_args("analysis-tip-excluded", &args)
    };
    let exclude = bar_button(
        i18n.tr("analysis-btn-exclude"),
        // The exclusion does not need lines: only the engine can be a reason.
        if section.protocol == Protocol::Uci {
            with_reason(exclude_tip, Need::Uci)
        } else {
            with_reason(i18n.tr("analysis-tip-exclude"), Need::Uci)
        },
        may_exclude.then(|| ui(AnalysisUiMessage::OpenExclude(id))),
        !view.exclusion.is_empty(),
    );
    let priority = bar_button(
        i18n.tr("analysis-btn-priority"),
        i18n.tr("analysis-tip-priority"),
        go(AnalysisMessage::ToggleLowPriority(id), ready),
        run.low_priority,
    );
    let info = bar_button(
        i18n.tr("analysis-btn-info"),
        i18n.tr("analysis-tip-info"),
        Some(am(AnalysisMessage::ToggleInfo(id))),
        view.show_info,
    );
    let display = bar_button(
        i18n.tr(match view.display {
            Display::Hidden => "analysis-btn-display-hidden",
            Display::NoWrap => "analysis-btn-display-nowrap",
            Display::Wrap => "analysis-btn-display-wrap",
        }),
        i18n.tr("analysis-tip-display"),
        Some(am(AnalysisMessage::CycleDisplay(id))),
        false,
    );
    let arrows = bar_button(
        i18n.tr("analysis-btn-arrows"),
        i18n.tr("analysis-tip-arrows"),
        ready.then(|| {
            Message::App(app_api::eval_bar_command(
                app_api::EvalBarMessage::ToggleArrows(id),
            ))
        }),
        run.arrows,
    );
    let mut bar = row![
        toggle,
        add_move,
        add_line,
        add_all,
        lines_stepper(section, ctx, ready && !locked && !commandeered),
        lock,
        exclude,
        priority,
        info,
        display,
        arrows,
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    if let Some(modes) = modes {
        bar = mode_buttons(bar, ctx, id, ready, modes);
    }
    if section.protocol != Protocol::Uci {
        bar = bar.push(bar_button(
            i18n.tr("analysis-btn-stats"),
            i18n.tr("analysis-tip-stats"),
            go(AnalysisMessage::RequestStats(id), ready),
            false,
        ));
    }
    if ctx.log_enabled {
        bar = bar.push(bar_button(
            i18n.tr("analysis-btn-log"),
            i18n.tr("analysis-tip-log"),
            Some(am(AnalysisMessage::OpenLog(id))),
            false,
        ));
    }
    bar.wrap().into()
}

/// The buttons of the annotation and of the engine modes (specs M4-6 and
/// M4-8). A button of a mode that is on stays enabled: it stops the mode.
fn mode_buttons<'a>(
    bar: iced::widget::Row<'a, Message>,
    ctx: &Ctx<'a>,
    id: RunId,
    ready: bool,
    modes: &ModeView,
) -> iced::widget::Row<'a, Message> {
    let i18n = ctx.i18n;
    let threat_on = modes.mode == EngineMode::Threat;
    let finish_on = modes.shoot_out();
    let training_on = matches!(modes.mode, EngineMode::Automove { .. });
    let threat_tip = if modes.in_check && !threat_on {
        i18n.tr("analysis-tip-threat-check")
    } else {
        i18n.tr("analysis-tip-threat")
    };
    // The training is on: the button stops it. Off: it asks the time first.
    let training_message = if training_on {
        mode(ModeMessage::ToggleAutomove(id))
    } else {
        ui(AnalysisUiMessage::OpenTraining(id))
    };
    let annotate_on = modes.annotating;
    let annotate_free = ready && (annotate_on || modes.mode == EngineMode::Idle);
    bar.push(bar_button(
        i18n.tr("analysis-btn-threat"),
        threat_tip,
        modes
            .avail
            .threat
            .then(|| mode(ModeMessage::ToggleThreat(id))),
        threat_on,
    ))
    .push(bar_button(
        i18n.tr("analysis-btn-finish-game"),
        i18n.tr("analysis-tip-finish-game"),
        modes
            .avail
            .finish_game
            .then(|| mode(ModeMessage::ToggleFinishGame(id))),
        finish_on,
    ))
    .push(bar_button(
        i18n.tr("analysis-btn-training"),
        i18n.tr("analysis-tip-training"),
        modes.avail.training.then_some(training_message),
        training_on,
    ))
    .push(bar_button(
        i18n.tr("analysis-btn-annotate"),
        i18n.tr(if annotate_on {
            "analysis-tip-annotate-stop"
        } else {
            "analysis-tip-annotate"
        }),
        annotate_free.then(|| Message::App(AppMessage::Annotate(AnnotateMessage::Open))),
        annotate_on,
    ))
}

/// The short box that asks the thinking time of the training (D493).
fn training_box<'a>(id: RunId, text_: &str, ctx: &Ctx<'a>) -> Element<'a, Message> {
    let i18n = ctx.i18n;
    let valid = parse_training_seconds(text_).is_some();
    let field = text_input(&i18n.tr("analysis-training-seconds"), text_)
        .id(Id::new(TRAINING_INPUT_ID))
        .on_input(|t| ui(AnalysisUiMessage::TrainingInput(t)))
        // Always present: without it the widget does not capture Enter and
        // it would reach the keys of the panel (D921). An invalid text is
        // ignored by the update, with the box open.
        .on_submit(ui(AnalysisUiMessage::TrainingSubmit(id)))
        .size(13)
        .padding(4)
        .width(Length::Fixed(90.0));
    container(
        column![
            text(i18n.tr("analysis-training-title"))
                .font(NOTO_SANS_BOLD)
                .size(12),
            text(i18n.tr("analysis-training-help"))
                .size(12)
                .style(theme::muted_text),
            row![
                text(i18n.tr("analysis-training-seconds")).size(12),
                field,
                bar_button(
                    i18n.tr("analysis-training-start"),
                    i18n.tr("analysis-tip-training"),
                    valid.then(|| ui(AnalysisUiMessage::TrainingSubmit(id))),
                    false,
                ),
                bar_button(
                    i18n.tr("dialog-cancel"),
                    i18n.tr("analysis-training-title"),
                    Some(ui(AnalysisUiMessage::TrainingCancel)),
                    false,
                ),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        ]
        .spacing(4),
    )
    .padding(8)
    .style(theme::list_header)
    .into()
}

/// The words of a PV as clickable spans: each move is a link to its index.
fn pv_spans(text_: &str, line: u8) -> Vec<text::Span<'_, PvLink>> {
    // A word and a space per move: one allocation for the whole line.
    let mut spans = Vec::with_capacity(text_.len() / 3 + 2);
    for (word, index) in pv_words(text_) {
        if !spans.is_empty() {
            spans.push(span(" "));
        }
        spans.push(match index {
            Some(index) => span(word).link((line, index)),
            None => span(word),
        });
    }
    spans
}

fn pv_text(
    run: RunId,
    text_: &str,
    line: u8,
    wrapping: Wrapping,
    bold: bool,
) -> Element<'_, Message> {
    let mut rich = rich_text(pv_spans(text_, line))
        .size(13)
        .wrapping(wrapping)
        .on_link_click(move |(line, index)| ui(AnalysisUiMessage::PlayPv { run, line, index }));
    if bold {
        rich = rich.font(NOTO_SANS_BOLD);
    }
    rich.into()
}

fn line_rows<'a>(section: &Section<'a>, ctx: &Ctx<'a>) -> Element<'a, Message> {
    let (run, view) = (section.run, section.view);
    let wrapping = if view.display == Display::Wrap {
        Wrapping::Word
    } else {
        Wrapping::None
    };
    let usable =
        *run.state == EngineRunState::Running && run.analyzing && !run.locked && !view.restarting;
    let mut rows = column![].spacing(3);
    for line in &view.lines {
        let first = line.multipv == 1;
        let weight = |t: iced::widget::Text<'a>| if first { t.font(NOTO_SANS_BOLD) } else { t };
        let add = bar_button(
            "+".to_owned(),
            ctx.i18n.tr("analysis-tip-add-this-line"),
            usable.then(|| am(AnalysisMessage::AddLineNumber(run.id, line.multipv))),
            false,
        );
        let moves = pv_text(run.id, &line.text, line.multipv, wrapping, first);
        let body = row![
            add,
            weight(text(line.multipv.to_string()).size(13)).width(Length::Fixed(14.0)),
            weight(text(format_score(line.score, line.bound)).size(13)).width(Length::Fixed(64.0)),
            weight(text(line.depth.to_string()).size(13)).width(Length::Fixed(30.0)),
            if wrapping == Wrapping::Word {
                container(moves).width(Length::Fill)
            } else {
                container(moves)
            },
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        rows = rows.push(body);
    }
    rows.into()
}

fn history_rows<'a>(section: &Section<'a>, ctx: &Ctx<'a>) -> Option<Element<'a, Message>> {
    let view = section.view;
    if view.multipv != 1 || view.history.is_empty() {
        return None;
    }
    let skip = view.history.len().saturating_sub(HISTORY_SHOWN);
    let strings = ctx.ui.cache.history_lines(
        section.run.id,
        ctx.i18n.language(),
        &view.history[skip..],
        |entry| {
            let mut args = fluent_bundle::FluentArgs::new();
            args.set("depth", i64::from(entry.depth));
            args.set("score", format_score(entry.score, entry.bound));
            args.set("moves", entry.text.as_str());
            args.set("time", mychess_app::format_seconds(entry.time_ms));
            ctx.i18n.tr_args("analysis-history-line", &args)
        },
    );
    let mut rows = column![].spacing(1);
    for line in strings {
        rows = rows.push(text(line).size(12));
    }
    Some(
        scrollable(rows)
            .anchor_bottom()
            .height(Length::Fixed(110.0))
            .width(Length::Fill)
            .into(),
    )
}

fn lines_area<'a>(section: &Section<'a>, ctx: &Ctx<'a>) -> Element<'a, Message> {
    let (run, view) = (section.run, section.view);
    if view.display == Display::Hidden {
        return button(text(ctx.i18n.tr("analysis-click-to-see")).size(12))
            .style(button::text)
            .on_press(am(AnalysisMessage::CycleDisplay(run.id)))
            .into();
    }
    let lines = line_rows(section, ctx);
    let lines: Element<'a, Message> = if view.display == Display::Wrap {
        lines
    } else {
        scrollable(lines)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(6).scroller_width(6),
            ))
            .width(Length::Fill)
            .into()
    };
    let mut page = column![lines].spacing(6);
    if let Some(history) = history_rows(section, ctx) {
        page = page.push(history);
    }
    // The middle click goes to the next display mode (`toggleMovesDisplay`).
    mouse_area(page)
        .on_middle_press(am(AnalysisMessage::CycleDisplay(run.id)))
        .into()
}

fn exclude_box<'a>(section: &Section<'a>, ctx: &Ctx<'a>) -> Element<'a, Message> {
    let id = section.run.id;
    let i18n = ctx.i18n;
    let field = text_input(
        &i18n.tr("analysis-exclude-placeholder"),
        &ctx.ui.exclude_text,
    )
    .id(Id::new(EXCLUDE_INPUT_ID))
    .on_input(|t| ui(AnalysisUiMessage::ExcludeInput(t)))
    .on_submit(ui(AnalysisUiMessage::ExcludeSubmit(id)))
    .size(13)
    .padding(4);
    container(
        column![
            text(i18n.tr("analysis-exclude-title"))
                .font(NOTO_SANS_BOLD)
                .size(12),
            text(i18n.tr("analysis-exclude-help"))
                .size(12)
                .style(theme::muted_text),
            field,
            row![
                bar_button(
                    i18n.tr("dialog-ok"),
                    i18n.tr("analysis-exclude-title"),
                    Some(ui(AnalysisUiMessage::ExcludeSubmit(id))),
                    false
                ),
                bar_button(
                    i18n.tr("dialog-cancel"),
                    i18n.tr("analysis-exclude-title"),
                    Some(am(AnalysisMessage::CloseExclude(id))),
                    false
                ),
            ]
            .spacing(6),
        ]
        .spacing(4),
    )
    .padding(8)
    .style(theme::list_header)
    .into()
}

fn log_view<'a>(section: &Section<'a>, ctx: &Ctx<'a>) -> Element<'a, Message> {
    let (run, view) = (section.run, section.view);
    let i18n = ctx.i18n;
    let id = run.id;
    let bar = row![
        checkbox(ctx.ui.log_auto)
            .label(i18n.tr("analysis-log-auto"))
            .size(14)
            .text_size(12)
            .on_toggle(|on| ui(AnalysisUiMessage::LogAuto(on))),
        bar_button(
            i18n.tr("analysis-log-update"),
            i18n.tr("analysis-log-update"),
            Some(am(AnalysisMessage::LogTick(id))),
            false
        ),
        text_input(&i18n.tr("analysis-log-find"), &view.log_filter)
            .id(Id::new(LOG_SEARCH_ID))
            .on_input(move |t| am(AnalysisMessage::SetLogFilter(id, t)))
            .on_submit(ui(AnalysisUiMessage::LogFilterSubmit))
            .size(12)
            .padding(3)
            .width(Length::Fill),
        bar_button(
            i18n.tr("analysis-log-save"),
            i18n.tr("analysis-log-save"),
            Some(ui(AnalysisUiMessage::PickLogFile(id))),
            false
        ),
        bar_button(
            i18n.tr("analysis-log-close"),
            i18n.tr("analysis-log-close"),
            Some(am(AnalysisMessage::CloseLog(id))),
            false
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let shown = ctx.ui.cache.log_lines(id, view, LOG_SHOWN);
    let total = shown.len();
    let mut lines = column![].spacing(0);
    for line in shown {
        lines = lines.push(
            text(line)
                .font(Font::MONOSPACE)
                .size(11)
                .wrapping(Wrapping::None),
        );
    }
    let body: Element<'a, Message> = if total == 0 {
        text(i18n.tr("analysis-log-empty"))
            .size(12)
            .style(theme::muted_text)
            .into()
    } else {
        scrollable(lines)
            .direction(scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::new().width(6).scroller_width(6),
                horizontal: scrollable::Scrollbar::new().width(6).scroller_width(6),
            })
            .anchor_bottom()
            .height(Length::Fixed(220.0))
            .width(Length::Fill)
            .into()
    };
    column![bar, body].spacing(6).into()
}

/// One section: the header, the buttons and the lines (or the log).
pub fn section<'a>(section: &Section<'a>, ctx: &Ctx<'a>) -> Element<'a, Message> {
    section_with_modes(section, ctx, None)
}

/// [`section`] with the buttons of the annotation and of the engine modes.
#[allow(clippy::too_many_lines)] // one flat list of the parts of a section
pub fn section_with_modes<'a>(
    section: &Section<'a>,
    ctx: &Ctx<'a>,
    modes: Option<&ModeView>,
) -> Element<'a, Message> {
    let (run, view) = (section.run, section.view);
    let mut page = column![header(section, ctx), button_bar(section, ctx, modes)].spacing(6);
    if let Some((target, draft)) = &ctx.ui.training
        && *target == run.id
    {
        page = page.push(training_box(run.id, draft, ctx));
    }
    match modes.map(|m| m.mode) {
        Some(EngineMode::Automove { state, .. }) => {
            page = page.push(
                text(ctx.i18n.tr(match state {
                    AutomoveState::EngineThinking => "analysis-training-thinking",
                    AutomoveState::UserToMove => "analysis-training-your-move",
                }))
                .size(12)
                .style(theme::accent_text),
            );
        }
        Some(EngineMode::Threat) => {
            // The arrows of M4-5 are not coloured as a threat (debt): the
            // label alone says that the line is the opponent's (D491).
            page = page.push(
                text(ctx.i18n.tr("analysis-threat-label"))
                    .size(12)
                    .style(theme::accent_text),
            );
        }
        _ => {}
    }
    if let Some(lock) = &view.lock {
        let mut args = fluent_bundle::FluentArgs::new();
        args.set(
            "description",
            analysis_text::lock_description(ctx.i18n, lock),
        );
        page = page.push(
            text(ctx.i18n.tr_args("analysis-locked-hint", &args))
                .size(12)
                .style(theme::accent_text),
        );
    }
    if view.exclude_open {
        page = page.push(exclude_box(section, ctx));
    }
    if view.log_open {
        return container(page.push(log_view(section, ctx)))
            .padding(8)
            .style(theme::list_header)
            .into();
    }
    if view.show_info && view.terminal.is_none() {
        let (first, second) =
            analysis_text::info_lines(ctx.i18n, view, section.protocol == Protocol::Uci);
        page = page.push(text(first).size(12).style(theme::muted_text));
        if let Some(second) = second {
            page = page.push(text(second).size(12).style(theme::muted_text));
        }
    }
    if *run.state == EngineRunState::Running && !view.lines.is_empty() {
        page = page.push(lines_area(section, ctx));
    }
    container(page)
        .padding(8)
        .width(Length::Fill)
        .style(theme::list_header)
        .into()
}

/// The sections of the running engines in a column.
pub fn sections<'a>(
    app: &'a AppState,
    ui_state: &'a AnalysisUi,
    i18n: &'a I18n,
) -> Element<'a, Message> {
    let ctx = Ctx {
        i18n,
        ui: ui_state,
        log_enabled: app.prefs.analysis.log_lines > 0,
    };
    let mut page = column![].spacing(10).padding(8);
    let mut any = false;
    for run in app.engine_runs() {
        let Some(view) = app.analysis_view(run.id) else {
            continue;
        };
        let entry = app.engines.get(run.engine);
        let name = run
            .identity
            .as_ref()
            .map(|i| i.name.as_str())
            .or_else(|| entry.map(|e| e.name.as_str()))
            .unwrap_or("?");
        let protocol = entry.map_or(Protocol::Uci, |e| e.protocol);
        any = true;
        let modes = ModeView::of(app, run.id);
        page = page.push(section_with_modes(
            &Section {
                run: RunInfo {
                    arrows: app_api::arrows_engine_is(app, run.id),
                    ..RunInfo::of(run)
                },
                name,
                protocol,
                view,
            },
            &ctx,
            Some(&modes),
        ));
    }
    if !any {
        page = page.push(
            text(i18n.tr("analysis-empty"))
                .size(13)
                .style(theme::muted_text),
        );
    }
    page.into()
}

/// The analysis pane.
pub fn view(shell: &Shell) -> Element<'_, Message> {
    let page = scrollable(sections(&shell.app, &shell.ui.analysis, &shell.i18n));
    // The keys of the panel work while the pointer is over it, like the
    // game list's (a `mouse_area` does not capture enter and exit).
    panes::body(
        mouse_area(page)
            .on_enter(ui(AnalysisUiMessage::Hover(true)))
            .on_exit(ui(AnalysisUiMessage::Hover(false))),
        "analysis-pane",
    )
}

/// The mode of "Add all lines" for a click: the right button is the
/// second line only; `Ctrl`/`Cmd` with the left one is the first moves only
/// (`rightclick == 2` in the Scid).
pub fn all_lines_mode(secondary: bool, command_held: bool) -> AllLinesMode {
    if secondary {
        AllLinesMode::SecondOnly
    } else if command_held {
        AllLinesMode::FirstMovesOnly
    } else {
        AllLinesMode::All
    }
}

/// The moves of `pv` up to and including `index`; empty if out of range.
pub fn pv_prefix(pv: &[Move], index: usize) -> &[Move] {
    pv.get(..=index).unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key, modifiers: Modifiers) -> KeyInput {
        KeyInput { key, modifiers }
    }

    #[test]
    fn keys_of_the_panel_follow_the_scid() {
        let run = RunId(4);
        let enter = Key::Named(NamedKey::Enter);
        assert_eq!(
            key_message(&key(enter, Modifiers::NONE), run, None),
            Some(AnalysisMessage::AddMove(run))
        );
        assert_eq!(
            key_message(&key(enter, Modifiers::COMMAND), run, None),
            Some(AnalysisMessage::AddLine(run))
        );
        assert_eq!(
            key_message(
                &key(Key::Named(NamedKey::Space), Modifiers::NONE),
                run,
                None
            ),
            Some(AnalysisMessage::Toggle(run))
        );
        assert_eq!(
            key_message(&key(Key::Char('3'), Modifiers::NONE), run, None),
            Some(AnalysisMessage::SetMultiPv(run, 3))
        );
        // Limited to what the engine accepts.
        assert_eq!(
            key_message(&key(Key::Char('5'), Modifiers::NONE), run, Some(2)),
            Some(AnalysisMessage::SetMultiPv(run, 2))
        );
        // Other digits, and any digit with a modifier, are not the panel's.
        assert_eq!(
            key_message(&key(Key::Char('6'), Modifiers::NONE), run, None),
            None
        );
        assert_eq!(
            key_message(&key(Key::Char('2'), Modifiers::COMMAND), run, None),
            None
        );
        assert_eq!(
            key_message(
                &key(
                    enter,
                    Modifiers {
                        shift: true,
                        ..Modifiers::NONE
                    }
                ),
                run,
                None
            ),
            None
        );
    }

    #[test]
    fn add_all_modes_follow_the_button_and_the_keys() {
        assert_eq!(all_lines_mode(false, false), AllLinesMode::All);
        assert_eq!(all_lines_mode(true, false), AllLinesMode::SecondOnly);
        assert_eq!(all_lines_mode(false, true), AllLinesMode::FirstMovesOnly);
        assert_eq!(all_lines_mode(true, true), AllLinesMode::SecondOnly);
    }

    #[test]
    fn a_prefix_of_a_pv_never_goes_out_of_range() {
        let pv: Vec<Move> = Vec::new();
        assert_eq!(pv_prefix(&pv, 0), []);
    }

    // ---- rendering ----

    use iced::{Settings, Size};
    use iced_test::Simulator;
    use mychess_app::AnalysisPrefs;
    use mychess_app::PvLine;
    use mychess_engine::{Bound, LogEntry, LogKind, Score};

    use crate::i18n::Language;

    const SIZE: Size = Size::new(900.0, 600.0);

    fn pv_line(multipv: u8, cp: i32, text: &str) -> PvLine {
        PvLine {
            multipv,
            depth: 18,
            seldepth: None,
            score: Score::Cp(cp),
            bound: Bound::Exact,
            wdl: None,
            pv: Vec::new(),
            time_ms: 1000,
            text: text.to_owned(),
        }
    }

    fn view_with(lines: usize) -> AnalysisView {
        let mut view = AnalysisView::new(RunId(1), 3, &AnalysisPrefs::default());
        let all = [
            pv_line(1, 35, "1.e4 e5 2.Nf3"),
            pv_line(2, 20, "1.d4 d5"),
            pv_line(3, -10, "1.c4 e5"),
        ];
        view.lines = all.into_iter().take(lines).collect();
        view
    }

    fn info(state: &EngineRunState, locked: bool) -> RunInfo<'_> {
        RunInfo {
            id: RunId(1),
            state,
            analyzing: true,
            locked,
            low_priority: false,
            max_lines: Some(8),
            search_moves: true,
            arrows: false,
        }
    }

    fn sim<'a>(
        run: RunInfo<'a>,
        protocol: Protocol,
        view: &'a AnalysisView,
        ui_state: &'a AnalysisUi,
        i18n: &'a I18n,
    ) -> Simulator<'a, Message> {
        let ctx = Ctx {
            i18n,
            ui: ui_state,
            log_enabled: true,
        };
        let element = section(
            &Section {
                run,
                name: "Stockfish",
                protocol,
                view,
            },
            &ctx,
        );
        Simulator::with_size(Settings::default(), SIZE, element)
    }

    fn clicked(mut s: Simulator<'_, Message>, label: &str) -> Vec<Message> {
        s.click(label)
            .unwrap_or_else(|e| panic!("a clickable {label:?}: {e:?}"));
        s.into_messages().collect()
    }

    fn is_analysis(message: &Message, wanted: &AnalysisMessage) -> bool {
        matches!(message, Message::App(AppMessage::Analysis(m)) if m == wanted)
    }

    #[test]
    fn a_section_shows_every_line_with_its_score_and_text() {
        let (state, view, ui_state) =
            (EngineRunState::Running, view_with(3), AnalysisUi::default());
        let i18n = I18n::new(Language::EnUs);
        let mut s = sim(info(&state, false), Protocol::Uci, &view, &ui_state, &i18n);
        for text in ["+0.35", "+0.20", "-0.10", "Stockfish"] {
            assert!(s.find(text).is_ok(), "{text}");
        }
        let messages = clicked(s, "+ Move");
        assert!(
            messages
                .iter()
                .any(|m| is_analysis(m, &AnalysisMessage::AddMove(RunId(1)))),
            "{messages:?}"
        );
    }

    #[test]
    fn the_edit_buttons_do_nothing_while_locked_or_without_lines() {
        let (state, ui_state) = (EngineRunState::Running, AnalysisUi::default());
        let i18n = I18n::new(Language::EnUs);
        let locked_view = view_with(3);
        let s = sim(
            info(&state, true),
            Protocol::Uci,
            &locked_view,
            &ui_state,
            &i18n,
        );
        assert!(clicked(s, "+ Line").is_empty(), "locked");
        let empty = view_with(0);
        let s = sim(info(&state, false), Protocol::Uci, &empty, &ui_state, &i18n);
        assert!(clicked(s, "+ Move").is_empty(), "no lines yet");
    }

    #[test]
    fn xboard_has_stats_and_no_exclusion() {
        let (state, view, ui_state) =
            (EngineRunState::Running, view_with(1), AnalysisUi::default());
        let i18n = I18n::new(Language::EnUs);
        let mut run = info(&state, false);
        run.search_moves = false;
        run.max_lines = None;
        let s = sim(run, Protocol::XBoard, &view, &ui_state, &i18n);
        assert!(clicked(s, "Exclude").is_empty());
        let s = sim(run, Protocol::XBoard, &view, &ui_state, &i18n);
        let messages = clicked(s, "Update");
        assert!(
            messages
                .iter()
                .any(|m| is_analysis(m, &AnalysisMessage::RequestStats(RunId(1)))),
            "{messages:?}"
        );
        // A UCI engine has no such button.
        let mut s = sim(info(&state, false), Protocol::Uci, &view, &ui_state, &i18n);
        assert!(s.find("Update").is_err());
    }

    #[test]
    fn states_are_written_in_words() {
        let ui_state = AnalysisUi::default();
        let i18n = I18n::new(Language::PtBr);
        let view = view_with(0);
        for (state, word) in [
            (EngineRunState::Starting, "iniciando..."),
            (EngineRunState::Stopping, "parando..."),
        ] {
            let mut s = sim(info(&state, false), Protocol::Uci, &view, &ui_state, &i18n);
            assert!(s.find(word).is_ok(), "{word}");
        }
        let running = EngineRunState::Running;
        let mut restarting = view_with(0);
        restarting.restarting = true;
        let mut s = sim(
            info(&running, false),
            Protocol::Uci,
            &restarting,
            &ui_state,
            &i18n,
        );
        assert!(s.find("reiniciando...").is_ok());
        let mut mate = view_with(0);
        mate.terminal = Some(mychess_app::Terminal::Checkmate);
        let mut s = sim(
            info(&running, false),
            Protocol::Uci,
            &mate,
            &ui_state,
            &i18n,
        );
        assert!(s.find("(xeque-mate)").is_ok());
        let mut idle = info(&running, false);
        idle.analyzing = false;
        let mut s = sim(idle, Protocol::Uci, &view, &ui_state, &i18n);
        assert!(s.find("parada").is_ok());
    }

    #[test]
    fn the_hidden_mode_shows_a_hint_that_brings_the_lines_back() {
        let (state, ui_state) = (EngineRunState::Running, AnalysisUi::default());
        let i18n = I18n::new(Language::EnUs);
        let mut view = view_with(2);
        view.display = Display::Hidden;
        let mut s = sim(info(&state, false), Protocol::Uci, &view, &ui_state, &i18n);
        assert!(s.find("+0.20").is_err(), "the lines are hidden");
        let messages = clicked(s, "Click here to see the moves");
        assert!(
            messages
                .iter()
                .any(|m| is_analysis(m, &AnalysisMessage::CycleDisplay(RunId(1)))),
            "{messages:?}"
        );
    }

    #[test]
    fn the_log_view_has_its_buttons_and_filters_the_entries() {
        let (state, ui_state) = (EngineRunState::Running, AnalysisUi::default());
        let i18n = I18n::new(Language::EnUs);
        let mut view = view_with(0);
        view.log_open = true;
        view.log = ["uci", "uciok", "isready", "readyok"]
            .iter()
            .zip(0..)
            .map(|(text, seq)| LogEntry {
                seq,
                kind: LogKind::Received,
                text: (*text).to_owned(),
            })
            .collect();
        view.log_filter = "ready".to_owned();
        let mut s = sim(info(&state, false), Protocol::Uci, &view, &ui_state, &i18n);
        for text in [
            "Auto",
            "Update",
            "Save as...",
            "Close",
            "< isready",
            "< readyok",
        ] {
            assert!(s.find(text).is_ok(), "{text}");
        }
        assert!(s.find("< uciok").is_err(), "filtered out");
        let messages = clicked(s, "Close");
        assert!(
            messages
                .iter()
                .any(|m| is_analysis(m, &AnalysisMessage::CloseLog(RunId(1)))),
            "{messages:?}"
        );
    }

    #[test]
    fn the_exclusion_box_confirms_with_ok() {
        let (state, ui_state) = (EngineRunState::Running, AnalysisUi::default());
        let i18n = I18n::new(Language::EnUs);
        let mut view = view_with(1);
        view.exclude_open = true;
        let s = sim(info(&state, false), Protocol::Uci, &view, &ui_state, &i18n);
        let messages = clicked(s, "OK");
        assert!(
            messages.iter().any(|m| matches!(
                m,
                Message::Ui(UiMessage::Analysis(AnalysisUiMessage::ExcludeSubmit(
                    RunId(1)
                )))
            )),
            "{messages:?}"
        );
    }

    #[test]
    fn a_failed_engine_says_why_and_offers_only_close() {
        let ui_state = AnalysisUi::default();
        let i18n = I18n::new(Language::EnUs);
        let state = EngineRunState::Failed(mychess_app::engines::EngineFailure {
            kind: mychess_app::engines::FailureKind::Crashed,
            exit_code: Some(1),
            stderr_tail: Vec::new(),
        });
        let view = view_with(2);
        let mut s = sim(info(&state, false), Protocol::Uci, &view, &ui_state, &i18n);
        assert!(s.find("+0.35").is_err(), "no lines of a dead search");
        assert!(clicked(s, "Lock").is_empty());
    }

    #[test]
    fn spaces_and_digits_of_the_log_clock_follow_the_switch() {
        let ui_state = AnalysisUi::default();
        assert!(ui_state.log_auto, "the log refreshes itself by default");
    }

    #[test]
    fn messages_that_name_a_run_are_recognised() {
        let run = RunId(2);
        assert_eq!(run_of(&AnalysisMessage::Toggle(run)), Some(run));
        assert_eq!(
            run_of(&AnalysisMessage::SetLogFilter(run, String::new())),
            Some(run)
        );
        assert_eq!(
            run_of(&AnalysisMessage::LogSaved {
                run,
                result: Ok(())
            }),
            None
        );
    }

    // ---- annotation and engine modes (specs M4-6, M4-8) ----

    fn sim_modes<'a>(
        run: RunInfo<'a>,
        view: &'a AnalysisView,
        ui_state: &'a AnalysisUi,
        i18n: &'a I18n,
        modes: &'a ModeView,
    ) -> Simulator<'a, Message> {
        let ctx = Ctx {
            i18n,
            ui: ui_state,
            log_enabled: true,
        };
        let element = section_with_modes(
            &Section {
                run,
                name: "Stockfish",
                protocol: Protocol::Uci,
                view,
            },
            &ctx,
            Some(modes),
        );
        Simulator::with_size(Settings::default(), SIZE, element)
    }

    fn idle_modes() -> ModeView {
        ModeView {
            mode: EngineMode::Idle,
            avail: ModeAvailability {
                threat: true,
                finish_game: true,
                training: true,
            },
            commandeered: false,
            annotating: false,
            in_check: false,
        }
    }

    fn mode_messages(messages: &[Message]) -> Vec<ModeMessage> {
        messages
            .iter()
            .filter_map(|m| match m {
                Message::App(AppMessage::Mode(m)) => Some(*m),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_mode_buttons_send_their_messages() {
        let (state, ui_state) = (EngineRunState::Running, AnalysisUi::default());
        let i18n = I18n::new(Language::EnUs);
        let (view, modes) = (view_with(1), idle_modes());
        let click = |label: &str| {
            let mut s = sim_modes(info(&state, false), &view, &ui_state, &i18n, &modes);
            s.click(label).expect("a button");
            s.into_messages().collect::<Vec<_>>()
        };
        assert_eq!(
            mode_messages(&click("Threat")),
            [ModeMessage::ToggleThreat(RunId(1))]
        );
        assert_eq!(
            mode_messages(&click("Finish game")),
            [ModeMessage::ToggleFinishGame(RunId(1))]
        );
        // The training asks the time first.
        let training = click("Training");
        assert_eq!(mode_messages(&training), []);
        assert!(training.iter().any(|m| matches!(
            m,
            Message::Ui(UiMessage::Analysis(AnalysisUiMessage::OpenTraining(RunId(
                1
            ))))
        )));
        let annotate = click("Annotate");
        assert!(
            annotate
                .iter()
                .any(|m| matches!(m, Message::App(AppMessage::Annotate(AnnotateMessage::Open))))
        );
    }

    #[test]
    fn a_disabled_mode_button_sends_nothing_and_the_active_one_stops_its_mode() {
        let (state, ui_state) = (EngineRunState::Running, AnalysisUi::default());
        let i18n = I18n::new(Language::EnUs);
        let view = view_with(1);
        // A shoot-out: the others are off, "Finish game" still stops it.
        let shoot = ModeView {
            mode: EngineMode::ShootOut { generation: 1 },
            avail: ModeAvailability {
                threat: false,
                finish_game: true,
                training: false,
            },
            commandeered: true,
            ..idle_modes()
        };
        for (label, sends) in [
            ("Threat", false),
            ("Training", false),
            ("Finish game", true),
        ] {
            let mut s = sim_modes(info(&state, false), &view, &ui_state, &i18n, &shoot);
            s.click(label).expect("a button");
            let sent: Vec<Message> = s.into_messages().collect();
            assert_eq!(!sent.is_empty(), sends, "{label}: {sent:?}");
        }
        // The training running: its button stops it at once.
        let training = ModeView {
            mode: EngineMode::Automove {
                generation: 2,
                thinking_ms: 3000,
                state: AutomoveState::UserToMove,
            },
            avail: ModeAvailability {
                threat: false,
                finish_game: false,
                training: true,
            },
            commandeered: true,
            ..idle_modes()
        };
        let mut s = sim_modes(info(&state, false), &view, &ui_state, &i18n, &training);
        assert!(s.find("Your move.....").is_ok());
        s.click("Training").expect("a button");
        let sent: Vec<Message> = s.into_messages().collect();
        assert_eq!(
            mode_messages(&sent),
            [ModeMessage::ToggleAutomove(RunId(1))]
        );
    }

    #[test]
    fn lines_exclusion_and_lock_are_out_of_reach_while_the_engine_plays() {
        let (state, ui_state) = (EngineRunState::Running, AnalysisUi::default());
        let i18n = I18n::new(Language::EnUs);
        let view = view_with(2);
        let commandeered = ModeView {
            mode: EngineMode::ShootOut { generation: 1 },
            commandeered: true,
            ..idle_modes()
        };
        for label in ["Lock", "Exclude", "+", "+ Move", "+ Line"] {
            let mut s = sim_modes(info(&state, false), &view, &ui_state, &i18n, &commandeered);
            s.click(label).expect("a button");
            let sent: Vec<Message> = s.into_messages().collect();
            assert!(sent.is_empty(), "{label}: {sent:?}");
        }
        // Not commandeered: the lock works.
        let idle = idle_modes();
        let mut s = sim_modes(info(&state, false), &view, &ui_state, &i18n, &idle);
        s.click("Lock").expect("a button");
        assert_eq!(s.into_messages().count(), 1);
    }

    #[test]
    fn the_threat_shows_its_label_and_check_disables_the_button() {
        let (state, ui_state) = (EngineRunState::Running, AnalysisUi::default());
        let i18n = I18n::new(Language::PtBr);
        let view = view_with(1);
        let threat = ModeView {
            mode: EngineMode::Threat,
            ..idle_modes()
        };
        let mut s = sim_modes(info(&state, false), &view, &ui_state, &i18n, &threat);
        assert!(s.find("Ameaça do adversário:").is_ok());
        let in_check = ModeView {
            avail: ModeAvailability {
                threat: false,
                ..idle_modes().avail
            },
            in_check: true,
            ..idle_modes()
        };
        let mut s = sim_modes(info(&state, false), &view, &ui_state, &i18n, &in_check);
        s.click("Ameaça").expect("a button");
        assert_eq!(s.into_messages().count(), 0);
    }

    #[test]
    fn the_annotation_in_progress_keeps_its_button_for_stopping() {
        let (state, ui_state) = (EngineRunState::Running, AnalysisUi::default());
        let i18n = I18n::new(Language::EnUs);
        let view = view_with(1);
        let annotating = ModeView {
            annotating: true,
            avail: ModeAvailability::default(),
            ..idle_modes()
        };
        let mut s = sim_modes(info(&state, false), &view, &ui_state, &i18n, &annotating);
        s.click("Annotate").expect("a button");
        assert_eq!(s.into_messages().count(), 1);
        let mut s = sim_modes(info(&state, false), &view, &ui_state, &i18n, &annotating);
        s.click("Training").expect("a button");
        assert_eq!(s.into_messages().count(), 0);
    }

    #[test]
    fn the_training_box_checks_the_seconds_before_it_starts() {
        let state = EngineRunState::Running;
        let ui_state = AnalysisUi {
            training: Some((RunId(1), "2.5".to_owned())),
            ..AnalysisUi::default()
        };
        let i18n = I18n::new(Language::EnUs);
        let (view, modes) = (view_with(1), idle_modes());
        let mut s = sim_modes(info(&state, false), &view, &ui_state, &i18n, &modes);
        assert!(s.find("Engine thinking time").is_ok());
        s.click("Start").expect("a button");
        assert!(s.into_messages().any(|m| matches!(
            m,
            Message::Ui(UiMessage::Analysis(AnalysisUiMessage::TrainingSubmit(
                RunId(1)
            )))
        )));
        let bad = AnalysisUi {
            training: Some((RunId(1), "abc".to_owned())),
            ..AnalysisUi::default()
        };
        let mut s = sim_modes(info(&state, false), &view, &bad, &i18n, &modes);
        s.click("Start").expect("a button");
        assert_eq!(s.into_messages().count(), 0);
    }

    #[test]
    fn the_thinking_time_is_read_with_a_dot_or_a_comma_and_kept_in_range() {
        use std::time::Duration;
        assert_eq!(parse_training_seconds("3"), Some(Duration::from_secs(3)));
        assert_eq!(
            parse_training_seconds(" 0,5 "),
            Some(Duration::from_millis(500))
        );
        assert_eq!(
            parse_training_seconds("0.001"),
            Some(Duration::from_millis(100))
        );
        assert_eq!(
            parse_training_seconds("100000"),
            Some(Duration::from_secs(300))
        );
        for bad in ["", "abc", "0", "-2", "NaN", "inf"] {
            assert_eq!(parse_training_seconds(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_section_without_modes_has_none_of_their_buttons() {
        let (state, view, ui_state) =
            (EngineRunState::Running, view_with(1), AnalysisUi::default());
        let i18n = I18n::new(Language::EnUs);
        let mut s = sim(info(&state, false), Protocol::Uci, &view, &ui_state, &i18n);
        for label in ["Threat", "Finish game", "Training", "Annotate"] {
            assert!(s.find(label).is_err(), "{label}");
        }
    }
}
