//! M6-1c (GUI): the window of the advanced searches opens from the three
//! actions, has the three tabs, turns the clicks and the typed fields into
//! the messages of the application and goes away on close (spec M6-1,
//! D3236, D3379-D3383).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use iced::widget::Id;
use mychess_app::{
    Action, AdvancedKind, AdvancedSearchMessage, AdvancedSearchNotice, Key, KeyInput,
    MaterialPiece, Message as AppMessage, Modifiers, NamedKey, SearchOp,
};
use mychess_core::Color;
use mychess_gui::windows::advanced_search_dialog::{
    ADVANCED_BOX_ID, ADVANCED_CLOSE_ID, ADVANCED_FEN_ID, ADVANCED_MOVES_ID, ADVANCED_START_ID,
    ADVANCED_STOP_ID, AdvancedUiMessage, NumField, tab_id,
};
use mychess_gui::windows::dialog_ui::DialogMessage;
use mychess_gui::{Message, Shell, UiMessage};

fn key(named: NamedKey) -> Message {
    Message::Dialog(DialogMessage::Key {
        input: KeyInput {
            key: Key::Named(named),
            modifiers: Modifiers::NONE,
        },
        text_focused: false,
        typed: None,
    })
}

fn open(shell: &mut Shell, action: Action) {
    drop(shell.update(Message::Ui(UiMessage::Action(action))));
}

fn send(shell: &mut Shell, message: AdvancedSearchMessage) {
    drop(shell.update(Message::AdvancedUi(AdvancedUiMessage::Send(message))));
}

fn render(shell: &Shell) -> iced_test::Simulator<'_, Message> {
    let element = mychess_gui::windows::main_window::view(shell);
    iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1300.0, 900.0),
        element,
    )
}

fn debug(messages: &[Message]) -> String {
    format!("{messages:?}")
}

fn tab(shell: &Shell) -> AdvancedKind {
    shell.app.advanced_search_view().expect("open").tab
}

#[test]
fn the_three_actions_open_the_window_on_their_tab() {
    for (action, kind) in [
        (Action::SearchBoard, AdvancedKind::Position),
        (Action::SearchMaterial, AdvancedKind::Material),
        (Action::SearchMoves, AdvancedKind::Moves),
    ] {
        let mut shell = Shell::new(None);
        assert!(!shell.modal_layer_open());
        open(&mut shell, action);
        assert_eq!(tab(&shell), kind);
        assert!(shell.modal_layer_open());
        let mut sim = render(&shell);
        assert!(sim.find(Id::new(ADVANCED_BOX_ID)).is_ok());
    }
}

#[test]
fn a_tab_button_selects_its_tab() {
    let mut shell = Shell::new(None);
    open(&mut shell, Action::SearchBoard);
    let mut sim = render(&shell);
    sim.click(Id::new(tab_id(AdvancedKind::Material)))
        .expect("the tab");
    let messages = debug(&sim.into_messages().collect::<Vec<_>>());
    assert!(messages.contains("SelectTab(Material)"), "{messages}");
    send(
        &mut shell,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Moves),
    );
    assert_eq!(tab(&shell), AdvancedKind::Moves);
}

#[test]
fn the_search_button_sends_start() {
    let mut shell = Shell::new(None);
    open(&mut shell, Action::SearchMoves);
    let mut sim = render(&shell);
    sim.click(Id::new(ADVANCED_START_ID)).expect("the button");
    let messages = debug(&sim.into_messages().collect::<Vec<_>>());
    assert!(messages.contains("Send(Start)"), "{messages}");
}

#[test]
fn stop_is_off_until_a_search_runs_and_then_sends_stop() {
    let mut shell = Shell::new(None);
    open(&mut shell, Action::SearchBoard);
    let mut sim = render(&shell);
    drop(sim.click(Id::new(ADVANCED_STOP_ID)));
    let messages = debug(&sim.into_messages().collect::<Vec<_>>());
    assert!(!messages.contains("Stop"), "{messages}");
    // The effect is not performed here, so the search stays in flight.
    send(&mut shell, AdvancedSearchMessage::Start);
    assert!(shell.app.advanced_search().is_running());
    let mut sim = render(&shell);
    sim.click(Id::new(ADVANCED_STOP_ID)).expect("the button");
    let messages = debug(&sim.into_messages().collect::<Vec<_>>());
    assert!(messages.contains("Send(Stop)"), "{messages}");
}

#[test]
fn escape_stops_a_running_search_before_it_closes_the_window() {
    let mut shell = Shell::new(None);
    open(&mut shell, Action::SearchBoard);
    send(&mut shell, AdvancedSearchMessage::Start);
    assert!(shell.app.advanced_search().is_running());
    drop(shell.update(key(NamedKey::Escape)));
    assert!(
        shell.app.advanced_search().is_open(),
        "the first Esc only stops"
    );
}

#[test]
fn escape_closes_the_window_when_nothing_runs_and_enter_searches() {
    let mut shell = Shell::new(None);
    open(&mut shell, Action::SearchMoves);
    drop(shell.update(key(NamedKey::Enter)));
    assert!(
        shell
            .app
            .advanced_search_view()
            .expect("open")
            .issue
            .is_some()
            || shell.app.advanced_search().is_running(),
        "Enter asked for a search (an empty list of moves is refused)"
    );
    send(&mut shell, AdvancedSearchMessage::Stop);
    let mut shell = Shell::new(None);
    open(&mut shell, Action::SearchBoard);
    drop(shell.update(key(NamedKey::Escape)));
    assert!(!shell.app.advanced_search().is_open());
    assert!(!shell.modal_layer_open());
}

#[test]
fn the_close_button_hides_the_window() {
    let mut shell = Shell::new(None);
    open(&mut shell, Action::SearchMaterial);
    let mut sim = render(&shell);
    sim.click(Id::new(ADVANCED_CLOSE_ID)).expect("the button");
    let messages: Vec<Message> = sim.into_messages().collect();
    assert!(debug(&messages).contains("Send(Close)"));
    for message in messages {
        drop(shell.update(message));
    }
    assert!(shell.app.advanced_search_view().is_none());
    let mut sim = render(&shell);
    assert!(sim.find(Id::new(ADVANCED_BOX_ID)).is_err());
}

#[test]
fn typing_in_the_moves_field_sends_the_text() {
    let mut shell = Shell::new(None);
    open(&mut shell, Action::SearchMoves);
    let mut sim = render(&shell);
    sim.click(Id::new(ADVANCED_MOVES_ID)).expect("the field");
    sim.typewrite("e4");
    let messages = debug(&sim.into_messages().collect::<Vec<_>>());
    assert!(messages.contains("SetMovesText(\"e4\")"), "{messages}");
}

#[test]
fn the_fen_field_appears_with_a_typed_fen() {
    let mut shell = Shell::new(None);
    open(&mut shell, Action::SearchBoard);
    assert!(render(&shell).find(Id::new(ADVANCED_FEN_ID)).is_err());
    send(
        &mut shell,
        AdvancedSearchMessage::SetFen("8/8/8/8/8/8/8/K6k w - - 0 1".to_owned()),
    );
    assert!(render(&shell).find(Id::new(ADVANCED_FEN_ID)).is_ok());
    send(&mut shell, AdvancedSearchMessage::UseCurrentPosition);
    assert!(render(&shell).find(Id::new(ADVANCED_FEN_ID)).is_err());
}

#[test]
fn the_fen_field_stays_when_it_is_emptied() {
    let mut shell = Shell::new(None);
    open(&mut shell, Action::SearchBoard);
    send(
        &mut shell,
        AdvancedSearchMessage::SetFen("8/8/8/8/8/8/8/K6k w - - 0 1".to_owned()),
    );
    send(&mut shell, AdvancedSearchMessage::SetFen(String::new()));
    assert!(render(&shell).find(Id::new(ADVANCED_FEN_ID)).is_ok());
    send(&mut shell, AdvancedSearchMessage::Start);
    assert_eq!(
        shell.app.advanced_search_view().expect("open").issue,
        Some(AdvancedSearchNotice::InvalidFen)
    );
    assert!(render(&shell).find(Id::new(ADVANCED_FEN_ID)).is_ok());
}

#[test]
fn the_numeric_fields_reach_the_application() {
    let mut shell = Shell::new(None);
    open(&mut shell, Action::SearchMaterial);
    let number = |shell: &mut Shell, field: NumField, text: &str| {
        drop(shell.update(Message::AdvancedUi(AdvancedUiMessage::Number(
            field,
            text.to_owned(),
        ))));
    };
    number(
        &mut shell,
        NumField::Min(Color::White, MaterialPiece::Rook),
        "1",
    );
    number(
        &mut shell,
        NumField::Max(Color::Black, MaterialPiece::Pawn),
        "3",
    );
    number(&mut shell, NumField::MinPlies, "5");
    number(&mut shell, NumField::MoveMin, "12");
    number(&mut shell, NumField::DiffMin, "-5");
    let view = shell.app.advanced_search_view().expect("open");
    assert_eq!(view.material.white.get(MaterialPiece::Rook).min, 1);
    assert_eq!(view.material.black.get(MaterialPiece::Pawn).max, 3);
    assert_eq!(view.material.min_plies, 5);
    assert_eq!(view.material.move_min, 12);
    assert_eq!(view.material.diff.map(|d| d.0), Some(-5));
    // Not a number: nothing changes.
    number(&mut shell, NumField::MinPlies, "x");
    assert_eq!(
        shell
            .app
            .advanced_search_view()
            .expect("open")
            .material
            .min_plies,
        5
    );
}

#[test]
fn the_operation_and_the_presets_are_edited_per_tab() {
    let mut shell = Shell::new(None);
    open(&mut shell, Action::SearchMaterial);
    send(
        &mut shell,
        AdvancedSearchMessage::SetOp(AdvancedKind::Material, SearchOp::And),
    );
    send(&mut shell, AdvancedSearchMessage::ApplyEnding(4));
    send(&mut shell, AdvancedSearchMessage::ApplyPatternPreset(0));
    let view = shell.app.advanced_search_view().expect("open");
    assert_eq!(view.material.op, SearchOp::And);
    assert_eq!(view.position.op, SearchOp::Reset);
    assert!(!view.endings.is_empty() && !view.patterns.is_empty());
    assert!(view.material.patterns.iter().any(|p| p.piece.is_some()));
}

#[test]
fn saving_without_a_folder_says_so_and_a_name_is_kept() {
    let mut shell = Shell::new(None);
    open(&mut shell, Action::SearchMoves);
    drop(shell.update(Message::AdvancedUi(AdvancedUiMessage::Name(
        "my search".to_owned(),
    ))));
    assert_eq!(shell.ui.dialogs.advanced.name(), "my search");
    send(
        &mut shell,
        AdvancedSearchMessage::SetMovesText("e4 e5".to_owned()),
    );
    drop(shell.update(Message::AdvancedUi(AdvancedUiMessage::SaveNamed)));
    let view = shell.app.advanced_search_view().expect("open");
    assert!(!view.can_save);
    assert_eq!(view.issue, Some(AdvancedSearchNotice::NoSavedFolder));
}

#[test]
fn the_progress_tick_message_is_the_one_the_application_reads() {
    // The subscription sends exactly this message while a search runs.
    let mut shell = Shell::new(None);
    open(&mut shell, Action::SearchBoard);
    send(&mut shell, AdvancedSearchMessage::Start);
    drop(shell.update(Message::App(AppMessage::AdvancedSearch(
        AdvancedSearchMessage::Progress,
    ))));
    assert!(shell.app.advanced_search().is_running());
    assert!(
        shell
            .app
            .advanced_search_view()
            .expect("open")
            .running
            .is_some()
    );
}
