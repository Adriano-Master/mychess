//! M8-1b (GUI): the menu entry and shortcut of the game against an engine,
//! the configuration dialog and its keys (D1254-D1256).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::time::{Duration, Instant};

use mychess_app::engines::{EngineDraft, EngineMessage};
use mychess_app::{
    Action, Binding, DiscardChoice, GameMessage, Key, KeyInput, Message as AppMessage, Modifiers,
    NamedKey, PlayColorPref, PlayMessage, PlayModePref, PlayOptions, PlayPhase,
};
use mychess_engine::{Capabilities, EngineEvent, EngineIdentity, ExitInfo, ExitReason};
use mychess_gui::widgets::board::interaction::BoardEvent;
use mychess_gui::windows::dialog_ui::DialogMessage;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::windows::play_dialog::{PLAY_BOX_ID, PlayUiMessage};
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
        .add(EngineDraft::new("Fake", "fake-engine"))
        .expect("engine added");
    shell
}

fn play_entry(shell: &Shell) -> (Option<Binding>, bool) {
    let specs = menu_specs(&shell.app, &shell.ui, ViewOptions::default());
    for spec in specs.iter().filter(|s| s.label_key == "menu-play") {
        for entry in &spec.entries {
            if let MenuEntry::Item {
                label: Label::Key("menu-play-engine"),
                shortcut,
                action,
                ..
            } = entry
            {
                return (*shortcut, action.is_some());
            }
        }
    }
    panic!("the Play menu has no entry for the game against the engine");
}

#[test]
fn the_play_menu_offers_the_game_with_its_shortcut() {
    let shell = shell_with_engine();
    let (shortcut, enabled) = play_entry(&shell);
    assert!(enabled);
    let binding = shortcut.expect("a default shortcut");
    assert_eq!(binding.to_string(), "Command+Shift+P");
}

#[test]
fn the_default_shortcut_has_no_conflict() {
    let shell = Shell::new(None);
    let binding = shell
        .app
        .shortcuts
        .bindings(Action::PlayEngine)
        .first()
        .copied()
        .expect("bound");
    let mut owners = 0;
    for action in Action::all().chain([Action::PlayEngine, Action::ManageEngines]) {
        if shell.app.shortcuts.bindings(action).contains(&binding) {
            owners += 1;
        }
    }
    assert_eq!(owners, 1);
}

#[test]
fn the_action_opens_the_dialog_enter_plays_and_escape_cancels() {
    let mut shell = shell_with_engine();
    drop(shell.update(Message::Ui(UiMessage::Action(Action::PlayEngine))));
    assert!(shell.ui.dialogs.play.draft.is_some());
    drop(shell.update(key(NamedKey::Escape)));
    assert!(shell.ui.dialogs.play.draft.is_none());
    drop(shell.update(Message::PlayUi(PlayUiMessage::Open)));
    assert!(shell.ui.dialogs.play.draft.is_some());
    assert!(shell.modal_layer_open());
    // Enter is "Play": the form closes and the preferences keep the engine.
    drop(shell.update(key(NamedKey::Enter)));
    assert!(shell.ui.dialogs.play.draft.is_none());
    assert!(shell.app.prefs.play.engine.is_some());
}

#[test]
fn without_an_engine_enter_keeps_the_form_open() {
    let mut shell = Shell::new(None);
    drop(shell.update(Message::PlayUi(PlayUiMessage::Open)));
    drop(shell.update(key(NamedKey::Enter)));
    assert!(shell.ui.dialogs.play.draft.is_some());
    let element = mychess_gui::windows::main_window::view(&shell);
    let mut sim = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1300.0, 900.0),
        element,
    );
    assert!(sim.find(iced::widget::Id::new(PLAY_BOX_ID)).is_ok());
}

#[test]
fn the_form_is_drawn_over_the_window() {
    let mut shell = shell_with_engine();
    drop(shell.update(Message::PlayUi(PlayUiMessage::Open)));
    let element = mychess_gui::windows::main_window::view(&shell);
    let mut sim = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1300.0, 900.0),
        element,
    );
    assert!(sim.find(iced::widget::Id::new(PLAY_BOX_ID)).is_ok());
    assert!(sim.find(iced::widget::Id::new("board-pane")).is_ok());
}

// ---- the central flow (M8-1b review) ----

fn send(shell: &mut Shell, message: Message) {
    drop(shell.update(message));
}

fn ui(shell: &mut Shell, message: PlayUiMessage) {
    send(shell, Message::PlayUi(message));
}

fn phase(shell: &Shell) -> Option<PlayPhase> {
    shell.app.play_phase()
}

fn identity() -> EngineIdentity {
    EngineIdentity {
        name: "Fake".to_owned(),
        author: None,
        options: Vec::new(),
        capabilities: Capabilities::default(),
    }
}

/// The engine of the game finishes its handshake (the effect that starts
/// the process is not run in these tests).
fn engine_ready(shell: &mut Shell) {
    let engine = shell.app.engines.engines().first().expect("engine").id;
    let run = shell.app.engine_runs.of_engine(engine).expect("run").id;
    send(
        shell,
        Message::App(AppMessage::Engine(EngineMessage::Started {
            run,
            identity: Box::new(identity()),
            at: 1_700_000_000,
        })),
    );
}

fn configure(shell: &mut Shell, mode: PlayModePref) {
    ui(shell, PlayUiMessage::Open);
    ui(shell, PlayUiMessage::Color(PlayColorPref::White));
    ui(shell, PlayUiMessage::Mode(mode));
}

fn started(mode: PlayModePref) -> Shell {
    let mut shell = shell_with_engine();
    configure(&mut shell, mode);
    ui(&mut shell, PlayUiMessage::Submit);
    assert!(shell.app.play_live());
    assert_eq!(phase(&shell), None, "the engine is still starting");
    engine_ready(&mut shell);
    shell
}

fn board_move(shell: &mut Shell, from: &str, to: &str) {
    let mv = shell
        .app
        .session
        .position()
        .find_move(from.parse().unwrap(), to.parse().unwrap(), None)
        .expect("legal");
    send(
        shell,
        Message::Ui(UiMessage::Board(BoardEvent::PlayMove {
            mv,
            dragged: false,
        })),
    );
}

fn tick(shell: &mut Shell, after: Duration) {
    let at = Instant::now() + after;
    send(shell, Message::App(AppMessage::Play(PlayMessage::Tick(at))));
}

fn clocks(shell: &Shell) -> (Option<u32>, Option<u32>) {
    let strips = shell.ui.dialogs.play.strips.as_ref().expect("strips");
    (strips.white.clock, strips.black.clock)
}

fn moves_in_game(shell: &Shell) -> usize {
    shell.app.session.game().mainline().count()
}

#[test]
fn a_game_goes_from_the_form_to_the_end_box_and_out() {
    let mut shell = started(PlayModePref::TimeBonus);
    assert_eq!(phase(&shell), Some(PlayPhase::Player));
    assert!(shell.ui.dialogs.play.strips.is_some());
    assert!(shell.ui.dialogs.play.countdown);
    assert_eq!(clocks(&shell).0, Some(300));

    // The clock of the player runs down and the strip follows, rounding up.
    tick(&mut shell, Duration::from_secs(30));
    assert_eq!(clocks(&shell), (Some(270), Some(300)));

    // The player moves; the move is the game's, not an edit.
    let before = moves_in_game(&shell);
    board_move(&mut shell, "e2", "e4");
    assert_eq!(moves_in_game(&shell), before + 1);
    assert_eq!(phase(&shell), Some(PlayPhase::Opponent));

    // Out of turn: ignored.
    board_move(&mut shell, "e7", "e5");
    assert_eq!(moves_in_game(&shell), before + 1);
    assert_eq!(phase(&shell), Some(PlayPhase::Opponent));

    // Resign asks first; Esc is "No" and the game goes on.
    ui(&mut shell, PlayUiMessage::Resign);
    assert!(shell.ui.dialogs.play.confirm.is_some());
    assert!(shell.modal_layer_open());
    send(&mut shell, key(NamedKey::Escape));
    assert!(shell.ui.dialogs.play.confirm.is_none());
    assert_eq!(phase(&shell), Some(PlayPhase::Opponent));

    // Enter is "Yes": the game ends with the resignation.
    ui(&mut shell, PlayUiMessage::Resign);
    send(&mut shell, key(NamedKey::Enter));
    assert!(matches!(phase(&shell), Some(PlayPhase::Ended(_))));
    {
        let element = mychess_gui::windows::main_window::view(&shell);
        let mut sim = iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(1300.0, 900.0),
            element,
        );
        assert!(
            sim.find(iced::widget::Id::new(
                mychess_gui::windows::play_panel::PLAY_END_BOX_ID
            ))
            .is_ok()
        );
    }

    // Save game goes to the PGN flow; Enter (Close) leaves the game.
    ui(&mut shell, PlayUiMessage::SaveGame);
    assert!(matches!(phase(&shell), Some(PlayPhase::Ended(_))));
    send(&mut shell, key(NamedKey::Enter));
    assert_eq!(phase(&shell), None);
    assert!(!shell.app.play_live());
}

#[test]
fn quit_in_the_middle_of_a_game_asks_and_yes_leaves_it() {
    let mut shell = started(PlayModePref::TimeBonus);
    ui(&mut shell, PlayUiMessage::Quit);
    assert_eq!(phase(&shell), Some(PlayPhase::Player));
    ui(&mut shell, PlayUiMessage::Confirm(false));
    assert!(shell.ui.dialogs.play.confirm.is_none());
    assert_eq!(phase(&shell), Some(PlayPhase::Player));
    ui(&mut shell, PlayUiMessage::Quit);
    ui(&mut shell, PlayUiMessage::Confirm(true));
    assert_eq!(phase(&shell), None);
    assert!(!shell.app.play_live());
}

#[test]
fn resign_is_possible_in_a_paused_game() {
    let mut shell = started(PlayModePref::TimeBonus);
    // Navigating pauses the game (D1240).
    send(
        &mut shell,
        Message::App(AppMessage::Game(GameMessage::Navigate(
            mychess_app::Nav::Start,
        ))),
    );
    assert_eq!(phase(&shell), Some(PlayPhase::Paused { lost: false }));
    ui(&mut shell, PlayUiMessage::Resign);
    assert!(shell.ui.dialogs.play.confirm.is_some());
    ui(&mut shell, PlayUiMessage::Confirm(true));
    assert!(matches!(phase(&shell), Some(PlayPhase::Ended(_))));
}

#[test]
fn restart_in_the_middle_of_a_game_asks_first() {
    let mut shell = started(PlayModePref::TimeBonus);
    board_move(&mut shell, "e2", "e4");
    ui(&mut shell, PlayUiMessage::Restart);
    assert!(shell.ui.dialogs.play.confirm.is_some());
    assert_eq!(moves_in_game(&shell), 1);
    ui(&mut shell, PlayUiMessage::Confirm(true));
    assert!(shell.app.play_live(), "a new game is starting");
}

#[test]
fn a_game_that_ends_closes_the_confirmation() {
    let mut shell = started(PlayModePref::TimeBonus);
    ui(&mut shell, PlayUiMessage::Quit);
    assert!(shell.ui.dialogs.play.confirm.is_some());
    // The flag of White falls while the question is up.
    tick(&mut shell, Duration::from_secs(301));
    assert!(matches!(phase(&shell), Some(PlayPhase::Ended(_))));
    assert!(shell.ui.dialogs.play.confirm.is_none());
}

#[test]
fn the_clocks_that_only_count_round_down_and_are_never_low() {
    let mut shell = started(PlayModePref::MoveTime);
    assert!(!shell.ui.dialogs.play.countdown);
    tick(&mut shell, Duration::from_millis(5_500));
    let (white, black) = clocks(&shell);
    assert_eq!(white, Some(5), "5.5 s used shows 5");
    assert_eq!(black, Some(0));
}

#[test]
fn a_tick_inside_the_same_second_changes_nothing() {
    let mut shell = started(PlayModePref::TimeBonus);
    let at = Instant::now() + Duration::from_secs(10);
    assert!(shell.ui.dialogs.play.refresh_clocks(&shell.app, at));
    let strips = shell.ui.dialogs.play.strips.clone();
    assert!(!shell.ui.dialogs.play.refresh_clocks(&shell.app, at));
    assert_eq!(shell.ui.dialogs.play.strips, strips);
}

#[test]
fn the_clocks_ignore_the_preferences_that_hide_them_during_a_game() {
    use mychess_app::{PlayersPrefs, PrefChange, PrefsMessage};
    let mut shell = shell_with_engine();
    let prefs = PlayersPrefs {
        show_clocks: false,
        show_strips: false,
        ..PlayersPrefs::default()
    };
    send(
        &mut shell,
        Message::App(AppMessage::Prefs(PrefsMessage::Set(PrefChange::Players(
            prefs,
        )))),
    );
    configure(&mut shell, PlayModePref::TimeBonus);
    ui(&mut shell, PlayUiMessage::Submit);
    engine_ready(&mut shell);
    let element = mychess_gui::windows::main_window::view(&shell);
    let mut sim = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1300.0, 900.0),
        element,
    );
    assert!(
        sim.find(iced::widget::Id::new(
            mychess_gui::windows::board_column::TOP_STRIP_ID
        ))
        .is_ok()
    );
    assert!(sim.find("5:00").is_ok());
}

#[test]
fn typing_during_a_game_is_ignored_without_a_remark() {
    let mut shell = started(PlayModePref::TimeBonus);
    send(
        &mut shell,
        Message::Dialog(DialogMessage::Key {
            input: KeyInput {
                key: Key::Char('e'),
                modifiers: Modifiers::NONE,
            },
            text_focused: false,
            typed: Some('e'),
        }),
    );
    assert_eq!(shell.app.session.move_entry().text(), "");
    assert!(shell.app.notice.is_none());
}

#[test]
fn a_start_waits_for_the_discard_question_and_goes_ahead_after_it() {
    let mut shell = shell_with_engine();
    let mv = shell
        .app
        .session
        .position()
        .find_move("e2".parse().unwrap(), "e4".parse().unwrap(), None)
        .expect("legal");
    send(
        &mut shell,
        Message::App(AppMessage::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        })),
    );
    configure(&mut shell, PlayModePref::TimeBonus);
    ui(&mut shell, PlayUiMessage::Submit);
    assert!(shell.app.discard_prompt.is_some());
    assert!(!shell.app.play_live());
    send(
        &mut shell,
        Message::App(AppMessage::AnswerDiscard(DiscardChoice::DontSave)),
    );
    assert!(shell.app.play_live(), "the start went ahead");
    engine_ready(&mut shell);
    assert_eq!(phase(&shell), Some(PlayPhase::Player));
}

#[test]
fn cancelling_the_discard_question_drops_the_start_with_a_remark() {
    let mut shell = shell_with_engine();
    let mv = shell
        .app
        .session
        .position()
        .find_move("e2".parse().unwrap(), "e4".parse().unwrap(), None)
        .expect("legal");
    send(
        &mut shell,
        Message::App(AppMessage::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        })),
    );
    configure(&mut shell, PlayModePref::TimeBonus);
    ui(&mut shell, PlayUiMessage::Submit);
    send(
        &mut shell,
        Message::App(AppMessage::AnswerDiscard(DiscardChoice::Cancel)),
    );
    assert!(!shell.app.play_live());
    assert_eq!(shell.ui.dialogs.play.note, Some("play-start-dropped"));
}

#[test]
fn restarting_a_finished_unsaved_game_asks_before_discarding_it() {
    let mut shell = started(PlayModePref::TimeBonus);
    board_move(&mut shell, "e2", "e4");
    ui(&mut shell, PlayUiMessage::Resign);
    ui(&mut shell, PlayUiMessage::Confirm(true));
    assert!(matches!(phase(&shell), Some(PlayPhase::Ended(_))));
    assert!(shell.app.play_start_needs_discard());
    // Restart (end box): the discard question comes first.
    ui(&mut shell, PlayUiMessage::Restart);
    assert!(shell.app.discard_prompt.is_some());
    assert!(matches!(phase(&shell), Some(PlayPhase::Ended(_))));
    // Cancel: nothing restarts and the user is told.
    send(
        &mut shell,
        Message::App(AppMessage::AnswerDiscard(DiscardChoice::Cancel)),
    );
    assert!(matches!(phase(&shell), Some(PlayPhase::Ended(_))));
    assert_eq!(shell.ui.dialogs.play.note, Some("play-restart-dropped"));
    // Again, and this time "Don't save": the restart goes ahead.
    ui(&mut shell, PlayUiMessage::Restart);
    send(
        &mut shell,
        Message::App(AppMessage::AnswerDiscard(DiscardChoice::DontSave)),
    );
    assert!(shell.app.play_live(), "a new game is starting");
    assert_eq!(
        phase(&shell),
        Some(PlayPhase::Player),
        "the engine was ready"
    );
    assert_eq!(moves_in_game(&shell), 0);
}

#[test]
fn the_state_bar_says_the_engine_is_starting() {
    let mut shell = shell_with_engine();
    configure(&mut shell, PlayModePref::TimeBonus);
    ui(&mut shell, PlayUiMessage::Submit);
    assert!(shell.app.play_live());
    let element = mychess_gui::windows::main_window::view(&shell);
    let mut sim = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1300.0, 900.0),
        element,
    );
    // The language follows the machine: the text is the translated one.
    let text = shell.i18n.tr("play-status-starting");
    assert!(sim.find(text.as_str()).is_ok(), "{text}");
}

#[test]
fn a_removed_engine_gives_way_to_the_first_uci_engine_in_the_open_form() {
    let mut shell = shell_with_engine();
    ui(&mut shell, PlayUiMessage::Open);
    let first = shell.ui.dialogs.play.draft.as_ref().unwrap().engine;
    assert!(first.is_some());
    let second = shell
        .app
        .engines
        .add(EngineDraft::new("Second", "second-engine"))
        .expect("added");
    ui(&mut shell, PlayUiMessage::Engine(second));
    // The engine picked is removed (the Engines dialog); the form follows.
    shell.app.engines.remove(second).expect("removed");
    send(&mut shell, Message::Ui(UiMessage::FlipBoard));
    let now = shell.ui.dialogs.play.draft.as_ref().unwrap().engine;
    assert_eq!(now, first);
}

#[test]
fn a_lost_engine_box_is_not_closed_by_escape_or_enter_and_quit_ends_the_game() {
    let mut shell = started(PlayModePref::TimeBonus);
    board_move(&mut shell, "e2", "e4");
    let engine = shell.app.engines.engines().first().expect("engine").id;
    let run = shell.app.engine_runs.of_engine(engine).expect("run").id;
    send(
        &mut shell,
        Message::App(AppMessage::Engine(EngineMessage::Event {
            run,
            event: Box::new(EngineEvent::Exited(ExitInfo {
                reason: ExitReason::Crashed,
                status: None,
                stderr_tail: Vec::new(),
            })),
        })),
    );
    assert_eq!(phase(&shell), Some(PlayPhase::Paused { lost: true }));
    send(&mut shell, key(NamedKey::Escape));
    send(&mut shell, key(NamedKey::Enter));
    assert_eq!(phase(&shell), Some(PlayPhase::Paused { lost: true }));
    assert!(shell.modal_layer_open());
    // The box's own button leaves without asking again.
    ui(&mut shell, PlayUiMessage::Quit);
    assert_eq!(phase(&shell), None);
}

// ---- M8-1c slice A: take back, offer draw, Store clock (D1507) ----

#[test]
fn the_store_clock_checkbox_edits_the_draft_and_is_saved_with_the_game() {
    use mychess_gui::windows::play_dialog::PlayUiMessage as P;
    let mut shell = shell_with_engine();
    ui(&mut shell, P::Open);
    assert!(
        !shell
            .ui
            .dialogs
            .play
            .draft
            .as_ref()
            .unwrap()
            .prefs
            .store_clock
    );
    assert!(!shell.app.prefs.play.store_clock);
    ui(&mut shell, P::StoreClock(true));
    assert!(
        shell
            .ui
            .dialogs
            .play
            .draft
            .as_ref()
            .unwrap()
            .prefs
            .store_clock
    );
    ui(&mut shell, P::Submit);
    assert!(shell.app.prefs.play.store_clock);
    ui(&mut shell, P::Quit);
    ui(&mut shell, P::Confirm(true));
}

#[test]
fn take_back_without_a_move_of_the_player_warns_in_the_status_bar() {
    let mut shell = started(PlayModePref::TimeBonus);
    let view = shell.app.play_view(Instant::now()).expect("view");
    assert!(!view.can_take_back);
    ui(&mut shell, PlayUiMessage::TakeBack);
    assert_eq!(
        shell.ui.status_notice,
        Some(mychess_app::AppNotice::Play(
            mychess_app::PlayNotice::NothingToTakeBack
        ))
    );
}

#[test]
fn the_buttons_are_off_while_the_engine_thinks() {
    let mut shell = started(PlayModePref::TimeBonus);
    board_move(&mut shell, "e2", "e4");
    assert_eq!(phase(&shell), Some(PlayPhase::Opponent));
    let view = shell.app.play_view(Instant::now()).expect("view");
    assert!(!view.can_take_back);
    assert!(!view.can_offer_draw);
    ui(&mut shell, PlayUiMessage::OfferDraw);
    assert_eq!(
        shell.ui.status_notice,
        Some(mychess_app::AppNotice::Play(
            mychess_app::PlayNotice::DrawNotAllowed
        ))
    );
}
