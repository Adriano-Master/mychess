//! M8-6c (GUI, spec D2325-D2329): the menu entry and shortcut, the dialog
//! and its keys, the panel under the board and the arrow of the missed move.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use mychess_app::calvar::{
    Assessment, CalvarConfig, CalvarEffect, CalvarMessage, CalvarNotice, CalvarPhase, Eval,
    LineProbes, Probe, RootLine,
};
use mychess_app::engines::EngineDraft;
use mychess_app::{
    Action, Effect, GameMessage, Key, KeyInput, Message as AppMessage, Modifiers, NamedKey, update,
};
use mychess_gui::windows::calvar_dialog::{
    CALVAR_BOX_ID, CALVAR_ENTRY_ID, CalvarEdit, CalvarUiMessage,
};
use mychess_gui::windows::calvar_panel::CALVAR_BAR_ID;
use mychess_gui::windows::dialog_ui::DialogMessage;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::{Message, Shell, UiMessage, ViewOptions};

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

fn shell_with_engine() -> Shell {
    let mut shell = Shell::new(None);
    shell
        .app
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .expect("engine");
    shell
}

fn quick() -> CalvarConfig {
    CalvarConfig {
        lines_required: 1,
        root_ms: 200,
        line_ms: 200,
        ..CalvarConfig::default()
    }
}

/// Starts an exercise straight in the application and keeps the jobs.
fn start(shell: &mut Shell) -> Vec<Effect> {
    let effects = update(
        &mut shell.app,
        AppMessage::Calvar(CalvarMessage::Start {
            config: Box::new(quick()),
            now: 1,
        }),
    );
    // Any message of the shell brings the visual state up to date.
    drop(
        shell.update(Message::CalvarUi(CalvarUiMessage::EntryChanged(
            String::new(),
        ))),
    );
    effects
}

fn root_request(effects: &[Effect]) -> (mychess_app::RequestId, mychess_app::calvar::RootJob) {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::Calvar(CalvarEffect::AnalyzeRoot { request, job }) => {
                Some((*request, (**job).clone()))
            }
            _ => None,
        })
        .expect("a root job")
}

fn menu_entry(shell: &Shell) -> (Option<mychess_app::Binding>, bool) {
    let specs = menu_specs(&shell.app, &shell.ui, ViewOptions::default());
    for spec in specs.iter().filter(|s| s.label_key == "menu-play") {
        for entry in &spec.entries {
            if let MenuEntry::Item {
                label: Label::Key("menu-play-calvar"),
                shortcut,
                action,
                ..
            } = entry
            {
                return (*shortcut, action.is_some());
            }
        }
    }
    panic!("the Play menu has no entry for the calculation of variants");
}

fn render(shell: &Shell) -> iced_test::Simulator<'_, Message> {
    let element = mychess_gui::windows::main_window::view(shell);
    iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1300.0, 900.0),
        element,
    )
}

#[test]
fn the_menu_offers_the_exercise_with_a_command_shortcut_nobody_else_has() {
    let shell = Shell::new(None);
    let (shortcut, enabled) = menu_entry(&shell);
    assert!(enabled);
    let binding = shortcut.expect("a default shortcut");
    assert_eq!(binding.to_string(), "Command+U");
    assert!(binding.modifiers.command, "uses Command, not a fixed Ctrl");
    let owners = Action::all()
        .chain([Action::CalvarTrainer])
        .filter(|a| shell.app.shortcuts.bindings(*a).contains(&binding))
        .count();
    assert_eq!(owners, 1);
}

#[test]
fn the_action_opens_the_dialog_as_a_modal_layer_and_escape_closes_it() {
    let mut shell = Shell::new(None);
    assert!(!shell.modal_layer_open());
    drop(shell.update(Message::Ui(UiMessage::Action(Action::CalvarTrainer))));
    assert!(shell.app.calvar_dialog_open());
    assert!(shell.modal_layer_open());
    drop(shell.update(key(NamedKey::Escape)));
    assert!(!shell.app.calvar_dialog_open());
    assert!(!shell.modal_layer_open());
}

#[test]
fn enter_without_an_engine_keeps_the_dialog_open_and_says_why() {
    let mut shell = Shell::new(None);
    drop(shell.update(Message::CalvarUi(CalvarUiMessage::Open)));
    drop(shell.update(key(NamedKey::Enter)));
    assert!(shell.app.calvar_dialog_open());
    assert_eq!(shell.app.calvar_view().notice, Some(CalvarNotice::NoEngine));
}

#[test]
fn enter_with_an_engine_starts_the_exercise_and_closes_the_dialog() {
    let mut shell = shell_with_engine();
    drop(shell.update(Message::CalvarUi(CalvarUiMessage::Open)));
    drop(shell.update(key(NamedKey::Enter)));
    assert_eq!(shell.app.calvar_phase(), CalvarPhase::Entering);
    assert!(!shell.app.calvar_dialog_open());
}

#[test]
fn the_dialog_is_drawn_over_the_window() {
    let mut shell = shell_with_engine();
    drop(shell.update(Message::CalvarUi(CalvarUiMessage::Open)));
    let mut sim = render(&shell);
    assert!(sim.find(iced::widget::Id::new(CALVAR_BOX_ID)).is_ok());
    assert!(sim.find(iced::widget::Id::new("board-pane")).is_ok());
}

#[test]
fn the_form_edits_a_draft_and_the_application_keeps_its_own_until_the_start() {
    let mut shell = shell_with_engine();
    drop(shell.update(Message::CalvarUi(CalvarUiMessage::Open)));
    for edit in [
        CalvarEdit::Lines(5),
        CalvarEdit::RootSeconds(60),
        CalvarEdit::LineSeconds(20),
        CalvarEdit::TimeLimit(600),
        CalvarEdit::FollowBoard(true),
        CalvarEdit::Fen("abc def".to_owned()),
    ] {
        drop(shell.update(Message::CalvarUi(CalvarUiMessage::Edit(edit))));
    }
    let view = shell.app.calvar_view();
    let start = shell.ui.dialogs.calvar.to_start(&view);
    assert_eq!(start.lines_required, 5);
    assert_eq!(start.root_ms, 60_000);
    assert_eq!(start.line_ms, 20_000);
    assert_eq!(start.time_limit_s, 600);
    assert!(start.follow_board);
    assert_eq!(start.fen, "abc def", "the FEN stays as typed");
    assert_eq!(
        view.config.lines_required, 3,
        "the application keeps its own"
    );
}

#[test]
fn the_panel_appears_under_the_board_with_an_exercise_and_goes_with_it() {
    let mut shell = shell_with_engine();
    start(&mut shell);
    let mut sim = render(&shell);
    assert!(sim.find(iced::widget::Id::new(CALVAR_BAR_ID)).is_ok());
    assert!(sim.find(iced::widget::Id::new(CALVAR_ENTRY_ID)).is_ok());
    drop(sim);
    drop(shell.update(Message::App(AppMessage::Calvar(CalvarMessage::Close))));
    assert_eq!(shell.app.calvar_phase(), CalvarPhase::Closed);
    let mut sim = render(&shell);
    assert!(sim.find(iced::widget::Id::new(CALVAR_BAR_ID)).is_err());
}

#[test]
fn typing_in_the_panel_and_pressing_enter_makes_a_line_without_moving_the_board() {
    let mut shell = shell_with_engine();
    start(&mut shell);
    drop(
        shell.update(Message::CalvarUi(CalvarUiMessage::EntryChanged(
            "e4 e5 Nf3".to_owned(),
        ))),
    );
    assert_eq!(shell.ui.dialogs.calvar.entry(), "e4 e5 Nf3");
    drop(shell.update(Message::CalvarUi(CalvarUiMessage::EntrySubmit)));
    assert_eq!(shell.ui.dialogs.calvar.entry(), "", "the field was emptied");
    let view = shell.app.calvar_view();
    assert_eq!(view.current_san, "1. e4 e5 2. Nf3");
    assert_eq!(shell.app.session.game().mainline().count(), 0);
    // An empty field does nothing.
    drop(shell.update(Message::CalvarUi(CalvarUiMessage::EntrySubmit)));
    assert_eq!(shell.app.calvar_view().current_plies, 3);
}

#[test]
fn the_evaluation_buttons_end_the_line_and_the_result_draws_the_missed_move() {
    let mut shell = shell_with_engine();
    let effects = start(&mut shell);
    let (request, job) = root_request(&effects);
    drop(
        shell.update(Message::CalvarUi(CalvarUiMessage::EntryChanged(
            "d4".to_owned(),
        ))),
    );
    drop(shell.update(Message::CalvarUi(CalvarUiMessage::EntrySubmit)));
    drop(
        shell.update(Message::App(AppMessage::Calvar(CalvarMessage::EndLine(
            Assessment::Equal,
        )))),
    );
    assert_eq!(shell.app.calvar_view().lines.len(), 1);
    let p = job.position.clone();
    let lines = vec![
        RootLine {
            multipv: 1,
            depth: 14,
            eval: Eval::centipawns(45),
            pv: vec![p.parse_uci("e2e4").unwrap()],
        },
        RootLine {
            multipv: 2,
            depth: 14,
            eval: Eval::centipawns(30),
            pv: vec![p.parse_uci("d2d4").unwrap()],
        },
    ];
    drop(shell.update(Message::App(AppMessage::Calvar(
        CalvarMessage::RootAnalyzed {
            request,
            result: Ok(lines),
        },
    ))));
    assert!(shell.app.calvar_view().root_ready);
    let effects = update(&mut shell.app, AppMessage::Calvar(CalvarMessage::Done));
    let (request, _) = effects
        .iter()
        .find_map(|e| match e {
            Effect::Calvar(CalvarEffect::AnalyzeLines { request, job }) => {
                Some((*request, job.clone()))
            }
            _ => None,
        })
        .expect("a lines job");
    let probes = vec![LineProbes {
        first: Some(Probe {
            eval: Eval::centipawns(30),
            depth: 12,
            pv: Vec::new(),
        }),
        end: None,
    }];
    drop(shell.update(Message::App(AppMessage::Calvar(
        CalvarMessage::LinesAnalyzed {
            request,
            result: Ok(probes),
        },
    ))));
    assert!(matches!(shell.app.calvar_phase(), CalvarPhase::Reported(_)));
    let arrows = shell.ui.dialogs.calvar.arrows();
    assert_eq!(arrows.len(), 1);
    assert_eq!(arrows[0].from.to_string(), "e2");
    assert_eq!(arrows[0].to.to_string(), "e4");
    // The panel shows the result and still renders.
    let mut sim = render(&shell);
    assert!(sim.find(iced::widget::Id::new(CALVAR_BAR_ID)).is_ok());
}

#[test]
fn the_exercise_locks_the_game_messages_the_panel_does_not_use() {
    let mut shell = shell_with_engine();
    start(&mut shell);
    drop(shell.update(Message::App(AppMessage::Game(GameMessage::TruncateEnd))));
    assert_eq!(shell.app.calvar_view().notice, Some(CalvarNotice::Locked));
}
