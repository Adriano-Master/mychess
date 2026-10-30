//! The window of the electronic board: the settings, the connection, the
//! console and the last move of the board (spec M8-10c, D2742, D2743).

use std::path::PathBuf;

use fluent_bundle::FluentArgs;
use iced::widget::{
    button, column, container, pick_list, radio, row, scrollable, space, text, text_input,
};
use iced::{Element, Length};
use mychess_app::AppState;
use mychess_app::eboard::{
    BoardChoice, ConsoleDirection, EboardNotice, EboardPhase, EboardPrefs, FilterChoice,
};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::widgets::modal::{Card, modal};

/// Id of the box.
pub const EBOARD_BOX_ID: &str = "eboard-box";
/// Id of the console scrollable.
pub const EBOARD_CONSOLE_ID: &str = "eboard-console";
/// Id of the free command line.
pub const EBOARD_LINE_ID: &str = "eboard-line";

/// What the window keeps while it is open: the settings being edited and
/// the line being typed (the connection itself is in the application).
#[derive(Debug, Clone, PartialEq)]
pub struct EboardUi {
    /// The settings in the form.
    pub draft: EboardPrefs,
    /// The free command being typed.
    pub line: String,
}

impl EboardUi {
    /// The window as it opens: the saved settings.
    #[must_use]
    pub fn open(app: &AppState) -> EboardUi {
        EboardUi {
            draft: (*app.prefs.eboard).clone(),
            line: String::new(),
        }
    }

    /// The settings are usable (the Connect button is enabled).
    #[must_use]
    pub fn can_connect(&self) -> bool {
        self.draft.clone().sanitized().validate().is_ok()
    }
}

/// An event of the window.
#[derive(Debug, Clone, PartialEq)]
pub enum EboardUiMessage {
    /// Opens (or focuses) the window.
    Open,
    /// Closes the window; the connection stays.
    Close,
    /// The kind of board.
    Kind(BoardChoice),
    /// The port, typed or picked.
    Port(String),
    /// The driver program.
    Command(String),
    /// The driver parameters.
    Params(String),
    /// Asks for the file dialog of the driver program.
    Browse,
    /// The answer of the file dialog.
    Browsed(Option<PathBuf>),
    /// Connects with the settings of the form.
    Connect,
    /// Ends the connection.
    Disconnect,
    /// Lists the ports again.
    Refresh,
    /// The button Info.
    Info,
    /// The button Rotate.
    Rotate,
    /// The button Synchronise.
    Synchronise,
    /// Empties the console.
    Clear,
    /// "Moves sent for".
    Filter(FilterChoice),
    /// The free line is being typed.
    Line(String),
    /// The free line is sent.
    Send,
}

fn send(message: EboardUiMessage) -> Message {
    Message::EboardUi(message)
}

/// The text of the state line.
fn state_text(app: &AppState, i18n: &I18n) -> String {
    let state = app.eboard();
    let key = match state.phase() {
        EboardPhase::Connected => "eboard-state-connected",
        EboardPhase::Connecting => "eboard-state-connecting",
        _ if state.issue().is_some() => "eboard-state-error",
        EboardPhase::Idle => "eboard-state-idle",
    };
    i18n.tr(key)
}

/// The text of a remark of the board for the status bar and the window.
#[must_use]
pub fn notice_text(i18n: &I18n, notice: &EboardNotice) -> String {
    let mut args = FluentArgs::new();
    if let Some(text) = notice.text() {
        args.set("text", text);
    }
    i18n.tr_args(notice.message_key(), &args)
}

/// `m:ss` or `h:mm:ss`.
fn clock_text(seconds: u32) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn kind_row<'a>(ui: &EboardUi, locked: bool, i18n: &I18n) -> Element<'a, Message> {
    let choose = |label: &str, kind: BoardChoice| -> Element<'a, Message> {
        let mut r = radio(label.to_owned(), kind, Some(ui.draft.kind), |k| {
            send(EboardUiMessage::Kind(k))
        });
        if locked {
            // A radio has no disabled state; ignore the click instead.
            r = radio(label.to_owned(), kind, Some(ui.draft.kind), |_| {
                send(EboardUiMessage::Kind(ui.draft.kind))
            });
        }
        r.into()
    };
    column![
        text(i18n.tr("eboard-kind")),
        row![
            choose(&i18n.tr("eboard-kind-none"), BoardChoice::None),
            choose(&i18n.tr("eboard-kind-novag"), BoardChoice::Novag),
            choose(&i18n.tr("eboard-kind-input"), BoardChoice::InputEngine),
        ]
        .spacing(16),
    ]
    .spacing(4)
    .into()
}

fn port_row<'a>(app: &AppState, ui: &EboardUi, locked: bool, i18n: &I18n) -> Element<'a, Message> {
    let mut field = text_input("", &ui.draft.port).padding(5);
    let mut refresh = button(text(i18n.tr("eboard-ports-refresh"))).style(button::secondary);
    if !locked {
        field = field.on_input(|t| send(EboardUiMessage::Port(t)));
        refresh = refresh.on_press(send(EboardUiMessage::Refresh));
    }
    let names: Vec<String> = app
        .eboard()
        .ports()
        .iter()
        .map(|p| p.name.clone())
        .collect();
    let mut line = row![field.width(Length::Fill)].spacing(8);
    if !names.is_empty() && !locked {
        line = line.push(
            pick_list(names, None::<String>, |n| send(EboardUiMessage::Port(n)))
                .placeholder(i18n.tr("eboard-ports-found")),
        );
    }
    line = line.push(refresh);
    let mut col = column![text(i18n.tr("eboard-port")), line].spacing(4);
    if let Some(error) = app.eboard().ports_error() {
        let mut args = FluentArgs::new();
        args.set("text", error);
        col = col.push(
            text(i18n.tr_args("eboard-ports-error", &args))
                .size(12)
                .style(text::danger),
        );
    } else if app.eboard().ports().is_empty() {
        col = col.push(text(i18n.tr("eboard-ports-none")).size(12));
    }
    col.into()
}

fn driver_rows<'a>(ui: &EboardUi, locked: bool, i18n: &I18n) -> Element<'a, Message> {
    let mut command = text_input("", &ui.draft.engine_command).padding(5);
    let mut browse = button(text(i18n.tr("eboard-driver-browse"))).style(button::secondary);
    let mut params = text_input("la", &ui.draft.engine_params).padding(5);
    if !locked {
        command = command.on_input(|t| send(EboardUiMessage::Command(t)));
        browse = browse.on_press(send(EboardUiMessage::Browse));
        params = params.on_input(|t| send(EboardUiMessage::Params(t)));
    }
    column![
        text(i18n.tr("eboard-driver")),
        row![command.width(Length::Fill), browse].spacing(8),
        text(i18n.tr("eboard-params")),
        params,
        text(i18n.tr("eboard-params-help")).size(12),
    ]
    .spacing(4)
    .into()
}

/// The text of a console line: the traffic as it came, or the translated
/// remark of the program (its `text` is the argument `$text`).
fn console_text(i18n: &I18n, line: &mychess_app::eboard::ConsoleLine) -> String {
    match line.note {
        Some(note) => {
            let mut args = FluentArgs::new();
            if note == mychess_app::eboard::ConsoleNote::LinesOmitted {
                // A number, so that Fluent picks the plural form.
                args.set("count", line.text.trim().parse::<i64>().unwrap_or(0));
            } else {
                args.set("text", line.text.as_str());
            }
            i18n.tr_args(note.message_key(), &args)
        }
        None => line.text.clone(),
    }
}

fn console<'a>(app: &AppState, i18n: &I18n) -> Element<'a, Message> {
    let mut lines = column![].spacing(1);
    let mut any = false;
    for line in app.eboard().console() {
        any = true;
        let mark = match line.direction {
            ConsoleDirection::In => "<",
            ConsoleDirection::Out => ">",
            ConsoleDirection::Note => "#",
        };
        lines = lines.push(
            text(format!("{mark} {}", console_text(i18n, line)))
                .size(12)
                .font(iced::Font::MONOSPACE),
        );
    }
    if !any {
        lines = lines.push(text(i18n.tr("eboard-console-empty")).size(12));
    }
    container(
        scrollable(lines)
            .id(EBOARD_CONSOLE_ID)
            .anchor_bottom()
            .width(Length::Fill)
            .height(Length::Fixed(170.0)),
    )
    .padding(4)
    .style(container::bordered_box)
    .into()
}

#[allow(clippy::too_many_lines)] // one column of widgets reads best flat
fn connected_part<'a>(app: &AppState, ui: &EboardUi, i18n: &I18n) -> Element<'a, Message> {
    use mychess_core::Color;
    let state = app.eboard();
    let last = match state.last_move() {
        None => text(i18n.tr("eboard-last-move-none")),
        Some(m) => {
            let mut args = FluentArgs::new();
            args.set("text", m.text.as_str());
            let key = if m.accepted() {
                "eboard-last-move-ok"
            } else {
                "eboard-last-move-ignored"
            };
            let t = text(i18n.tr_args(key, &args));
            if m.accepted() {
                t.style(text::success)
            } else {
                t.style(text::danger)
            }
        }
    };
    let mut status = column![row![text(i18n.tr("eboard-last-move")), last].spacing(8)].spacing(4);
    if let Some(name) = state.board_name() {
        let mut args = FluentArgs::new();
        args.set("text", name);
        status = status.push(text(i18n.tr_args("eboard-board-name", &args)).size(12));
    }
    let clocks: Vec<String> = [
        (Color::White, "eboard-clock-white"),
        (Color::Black, "eboard-clock-black"),
    ]
    .into_iter()
    .filter_map(|(color, key)| {
        state.clock(color).map(|s| {
            let mut args = FluentArgs::new();
            args.set("time", clock_text(s));
            i18n.tr_args(key, &args)
        })
    })
    .collect();
    if !clocks.is_empty() {
        status = status.push(text(clocks.join("   ")).size(12));
    }
    match state.in_sync() {
        Some(true) => status = status.push(text(i18n.tr("eboard-in-sync")).size(12)),
        Some(false) => {
            status = status.push(
                text(i18n.tr("eboard-out-of-sync"))
                    .size(12)
                    .style(text::danger),
            );
        }
        None => {}
    }
    let tool = |key: &str, message: EboardUiMessage| {
        button(text(i18n.tr(key)))
            .on_press(send(message))
            .style(button::secondary)
    };
    let mut tools = row![].spacing(8);
    if state.connected_kind() == BoardChoice::InputEngine {
        tools = tools
            .push(tool("eboard-info", EboardUiMessage::Info))
            .push(tool("eboard-rotate", EboardUiMessage::Rotate))
            .push(tool("eboard-sync", EboardUiMessage::Synchronise));
    }
    tools = tools.push(tool("eboard-clear", EboardUiMessage::Clear));
    let mut body = column![status, tools].spacing(8);
    if state.connected_kind() == BoardChoice::InputEngine {
        let filter = row![
            text(i18n.tr("eboard-filter")),
            radio(
                i18n.tr("eboard-filter-both"),
                FilterChoice::Both,
                Some(state.filter()),
                |f| send(EboardUiMessage::Filter(f)),
            ),
            radio(
                i18n.tr("eboard-filter-white"),
                FilterChoice::White,
                Some(state.filter()),
                |f| send(EboardUiMessage::Filter(f)),
            ),
            radio(
                i18n.tr("eboard-filter-black"),
                FilterChoice::Black,
                Some(state.filter()),
                |f| send(EboardUiMessage::Filter(f)),
            ),
        ]
        .spacing(12);
        body = body.push(filter);
    }
    let line = text_input(&i18n.tr("eboard-command-placeholder"), &ui.line)
        .id(EBOARD_LINE_ID)
        .on_input(|t| send(EboardUiMessage::Line(t)))
        .on_submit(send(EboardUiMessage::Send))
        .padding(5);
    body.push(text(i18n.tr("eboard-console")))
        .push(console(app, i18n))
        .push(
            row![
                line.width(Length::Fill),
                button(text(i18n.tr("eboard-send")))
                    .on_press_maybe(
                        (!ui.line.trim().is_empty()).then(|| send(EboardUiMessage::Send))
                    )
                    .style(button::secondary),
            ]
            .spacing(8),
        )
        .into()
}

/// The window.
#[must_use]
pub fn view<'a>(app: &AppState, ui: &EboardUi, i18n: &I18n) -> Element<'a, Message> {
    let state = app.eboard();
    let active = state.is_active();
    let connected = state.is_connected();
    let mut body = column![text(state_text(app, i18n))].spacing(10);
    if let Some(issue) = state.issue() {
        body = body.push(text(notice_text(i18n, issue)).style(text::danger));
    } else if !active
        && !ui.can_connect()
        && ui.draft.kind != BoardChoice::None
        && let Err(issue) = ui.draft.clone().sanitized().validate()
    {
        body = body.push(text(i18n.tr(issue.message_key())).size(12));
    }
    body = body.push(kind_row(ui, active, i18n));
    if ui.draft.kind != BoardChoice::None {
        body = body.push(port_row(app, ui, active, i18n));
    }
    if ui.draft.kind == BoardChoice::InputEngine {
        body = body.push(driver_rows(ui, active, i18n));
    }
    if connected {
        body = body.push(connected_part(app, ui, i18n));
    }
    let action = if active {
        button(text(i18n.tr("eboard-disconnect"))).on_press(send(EboardUiMessage::Disconnect))
    } else {
        button(text(i18n.tr("eboard-connect")))
            .on_press_maybe(ui.can_connect().then(|| send(EboardUiMessage::Connect)))
    };
    let title = if active {
        i18n.tr("eboard-title-connected")
    } else {
        i18n.tr("eboard-title")
    };
    modal(Card {
        id: EBOARD_BOX_ID,
        title,
        body: scrollable(container(body).padding([0, 8]))
            .height(Length::Shrink)
            .into(),
        buttons: vec![
            space::horizontal().into(),
            action.into(),
            button(text(i18n.tr("eboard-close")))
                .on_press(send(EboardUiMessage::Close))
                .style(button::secondary)
                .into(),
        ],
        width: 640.0,
    })
}

/// The message that closes the window (Esc).
#[must_use]
pub fn close_message() -> Message {
    send(EboardUiMessage::Close)
}

#[cfg(test)]
mod tests {
    use iced::{Settings, Size};
    use iced_test::Simulator;

    use super::*;
    use crate::i18n::Language;
    use crate::shell::Shell;

    fn sim<'a>(app: &AppState, ui: &EboardUi, lang: Language) -> Simulator<'a, Message> {
        Simulator::with_size(
            Settings::default(),
            Size::new(900.0, 800.0),
            view(app, ui, &I18n::new(lang)),
        )
    }

    fn app_and_ui() -> (AppState, EboardUi) {
        let app = AppState::default();
        let ui = EboardUi::open(&app);
        (app, ui)
    }

    #[test]
    fn the_window_opens_with_connect_disabled_until_the_settings_are_usable() {
        let (app, mut ui) = app_and_ui();
        let mut s = sim(&app, &ui, Language::EnUs);
        assert!(s.find("Electronic board").is_ok());
        assert!(s.find("Disconnected").is_ok());
        drop(s.click("Connect"));
        assert_eq!(s.into_messages().count(), 0, "kind none: disabled");
        ui.draft.kind = BoardChoice::Novag;
        ui.draft.port = "COM3".to_owned();
        let mut s = sim(&app, &ui, Language::EnUs);
        s.click("Connect").expect("button");
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [Message::EboardUi(EboardUiMessage::Connect)]
        ));
    }

    #[test]
    fn the_driver_needs_a_program() {
        let (app, mut ui) = app_and_ui();
        ui.draft.kind = BoardChoice::InputEngine;
        ui.draft.port = "COM3".to_owned();
        assert!(!ui.can_connect());
        let mut s = sim(&app, &ui, Language::EnUs);
        assert!(s.find("Choose the driver program.").is_ok());
        assert!(s.find("Driver parameters").is_ok());
        ui.draft.engine_command = "dgtdrv2".to_owned();
        assert!(ui.can_connect());
    }

    #[test]
    fn the_texts_exist_in_both_languages() {
        let (app, mut ui) = app_and_ui();
        ui.draft.kind = BoardChoice::InputEngine;
        let mut s = sim(&app, &ui, Language::PtBr);
        assert!(s.find("Tabuleiro eletrônico").is_ok());
        assert!(s.find("Parâmetros do driver").is_ok());
        assert!(s.find(iced::widget::Id::new(EBOARD_BOX_ID)).is_ok());
    }

    #[test]
    fn every_console_note_has_its_text_in_both_languages() {
        use mychess_app::eboard::{ConsoleDirection, ConsoleLine, ConsoleNote};
        // A new variant fails to compile in the match below: add it to the list.
        let all = [
            ConsoleNote::Disconnected,
            ConsoleNote::SetupMode,
            ConsoleNote::IllegalMove,
            ConsoleNote::MoveNotApplied,
            ConsoleNote::Refused,
            ConsoleNote::UnreadableMove,
            ConsoleNote::InputEngineOnly,
            ConsoleNote::DriverStderr,
            ConsoleNote::LinesOmitted,
        ];
        for note in all {
            match note {
                ConsoleNote::Disconnected
                | ConsoleNote::SetupMode
                | ConsoleNote::IllegalMove
                | ConsoleNote::MoveNotApplied
                | ConsoleNote::Refused
                | ConsoleNote::UnreadableMove
                | ConsoleNote::InputEngineOnly
                | ConsoleNote::DriverStderr
                | ConsoleNote::LinesOmitted => {}
                _ => panic!("list the new variant"),
            }
            for lang in [Language::EnUs, Language::PtBr] {
                let i18n = I18n::new(lang);
                let argument = if note == ConsoleNote::LinesOmitted {
                    "3"
                } else {
                    "e2e5"
                };
                let line = ConsoleLine {
                    direction: ConsoleDirection::Note,
                    text: argument.to_owned(),
                    note: Some(note),
                };
                let shown = console_text(&i18n, &line);
                assert_ne!(shown, note.message_key(), "{note:?} {lang:?}: key missing");
                let takes_text = !matches!(
                    note,
                    ConsoleNote::Disconnected
                        | ConsoleNote::SetupMode
                        | ConsoleNote::InputEngineOnly
                );
                if takes_text {
                    assert!(shown.contains(argument), "{note:?} {lang:?}: {shown}");
                }
                assert!(!shown.to_lowercase().contains("variação"));
            }
        }
        // The plural form follows the count in both languages.
        for (lang, one, many) in [
            (Language::EnUs, "1 line omitted", "2 lines omitted"),
            (Language::PtBr, "1 linha omitida", "2 linhas omitidas"),
        ] {
            let i18n = I18n::new(lang);
            for (count, expected) in [("1", one), ("2", many)] {
                let line = ConsoleLine {
                    direction: ConsoleDirection::Note,
                    text: count.to_owned(),
                    note: Some(ConsoleNote::LinesOmitted),
                };
                let shown = console_text(&i18n, &line);
                assert!(shown.contains(expected), "{lang:?}: {shown}");
            }
        }
        let traffic = ConsoleLine {
            direction: ConsoleDirection::In,
            text: "< M 1".to_owned(),
            note: None,
        };
        assert_eq!(console_text(&I18n::new(Language::EnUs), &traffic), "< M 1");
    }

    #[test]
    fn clocks_are_formatted() {
        assert_eq!(clock_text(75), "1:15");
        assert_eq!(clock_text(3725), "1:02:05");
    }

    #[test]
    fn the_window_opens_connects_shows_the_console_and_closes() {
        use mychess_app::eboard::EboardMessage;
        use mychess_app::eboard::{ConsoleDirection as Direction, EboardEvent};

        let mut shell = Shell::new(None);
        assert!(!shell.modal_layer_open());
        drop(shell.update(Message::EboardUi(EboardUiMessage::Open)));
        assert!(shell.modal_layer_open(), "the window is a modal layer");
        for edit in [
            EboardUiMessage::Kind(BoardChoice::Novag),
            EboardUiMessage::Port("COM3".to_owned()),
            EboardUiMessage::Connect,
        ] {
            drop(shell.update(Message::EboardUi(edit)));
        }
        assert!(shell.app.eboard().is_active(), "connecting");
        let epoch = shell.app.eboard().epoch();
        for event in [
            EboardEvent::Ready,
            EboardEvent::Log {
                direction: Direction::In,
                text: "M 1, e2-e4".to_owned(),
            },
        ] {
            drop(shell.update(Message::App(mychess_app::Message::Eboard(
                EboardMessage::Event { epoch, event },
            ))));
        }
        assert!(
            shell.app.eboard().is_connected(),
            "{:?}",
            shell.app.eboard().phase()
        );
        {
            let element = crate::windows::main_window::view(&shell);
            let mut s =
                Simulator::with_size(Settings::default(), Size::new(1300.0, 900.0), element);
            assert!(s.find(iced::widget::Id::new(EBOARD_BOX_ID)).is_ok());
            assert!(s.find("< M 1, e2-e4").is_ok(), "console line");
        }
        drop(shell.update(Message::EboardUi(EboardUiMessage::Disconnect)));
        assert!(!shell.app.eboard().is_active());
        drop(shell.update(Message::EboardUi(EboardUiMessage::Close)));
        assert!(!shell.modal_layer_open());
    }
}
