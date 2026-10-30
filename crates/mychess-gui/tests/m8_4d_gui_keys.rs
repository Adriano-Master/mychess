//! M8-4d (GUI, spec D2130-D2159): Enter goes to the next problem without
//! leaking out of modals and text fields, the unsaved-game question over the
//! start, and the new controls of the panel and of the dialog.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::collections::VecDeque;
use std::path::Path;

use mychess_app::tactics::{
    TacticsConfig, TacticsEffect, TacticsMessage, TacticsMode, TacticsNotice, TacticsPhase,
    find_next, judge, open_file,
};
use mychess_app::{
    AppState, Effect, GameMessage, Key, KeyInput, Message as AppMessage, Modifiers, NamedKey,
    PlayOptions, update,
};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::dialog_ui::DialogMessage;
use mychess_gui::windows::dialogs::DISCARD_BOX_ID;
use mychess_gui::windows::tactics_dialog::{TACTICS_BOX_ID, TacticsEdit, TacticsUiMessage};
use mychess_gui::windows::tactics_text::notice_text;
use mychess_gui::{Message, Shell};

const MATE1: &str = "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1";
const MATE2: &str = "kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1";

fn settle(app: &mut AppState, effects: Vec<Effect>) {
    let mut queue: VecDeque<Effect> = effects.into();
    while let Some(effect) = queue.pop_front() {
        let Effect::Tactics(effect) = effect else {
            continue;
        };
        let answer = match effect {
            TacticsEffect::OpenFile { request, path } => TacticsMessage::Opened {
                request,
                result: open_file(&path).map(Box::new),
            },
            TacticsEffect::Find {
                request,
                source,
                query,
            } => TacticsMessage::Found {
                request,
                outcome: Box::new(find_next(&source, &query)),
            },
            TacticsEffect::Judge { request, job } => TacticsMessage::Judged {
                request,
                verdict: Box::new(judge(&job)),
            },
            _ => continue,
        };
        queue.extend(update(app, AppMessage::Tactics(answer)));
    }
}

fn problems(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("p.pgn");
    let games = [("one", MATE1), ("two", MATE2)]
        .map(|(e, f)| format!("[Event \"{e}\"]\n[SetUp \"1\"]\n[FEN \"{f}\"]\n\n*\n\n"))
        .concat();
    std::fs::write(&path, games).unwrap();
    path
}

fn start(shell: &mut Shell, file: &Path) {
    let config = TacticsConfig {
        file: Some(file.to_path_buf()),
        ..TacticsConfig::default()
    };
    let effects = update(
        &mut shell.app,
        AppMessage::Tactics(TacticsMessage::Start {
            config: Box::new(config),
            seed: 1,
        }),
    );
    settle(&mut shell.app, effects);
}

fn solved_shell(dir: &Path) -> Shell {
    let mut shell = Shell::new(None);
    start(&mut shell, &problems(dir));
    assert_eq!(shell.app.tactics_phase(), TacticsPhase::Solving);
    let mv = shell
        .app
        .session
        .position()
        .find_move("a1".parse().unwrap(), "a8".parse().unwrap(), None)
        .unwrap();
    let effects = update(
        &mut shell.app,
        AppMessage::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }),
    );
    settle(&mut shell.app, effects);
    assert!(matches!(shell.app.tactics_phase(), TacticsPhase::Solved(_)));
    shell
}

fn enter(text_focused: bool) -> Message {
    Message::Dialog(DialogMessage::Key {
        input: KeyInput {
            key: Key::Named(NamedKey::Enter),
            modifiers: Modifiers::NONE,
        },
        text_focused,
        typed: None,
    })
}

/// Gives a message to the shell; the effects go to the host, which these
/// tests are not.
fn send(shell: &mut Shell, message: Message) {
    drop(shell.update(message));
}

// ---- (3) Enter = next problem (D2143) ----

#[test]
fn enter_goes_to_the_next_problem_when_the_problem_is_over() {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = solved_shell(dir.path());
    send(&mut shell, enter(false));
    // The search of the next problem was asked for: the session is looking.
    assert_eq!(shell.app.tactics_phase(), TacticsPhase::Searching);
}

#[test]
fn enter_does_nothing_while_the_problem_is_open() {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = Shell::new(None);
    start(&mut shell, &problems(dir.path()));
    send(&mut shell, enter(false));
    assert_eq!(shell.app.tactics_phase(), TacticsPhase::Solving);
}

#[test]
fn enter_in_a_text_field_does_not_skip_the_problem() {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = solved_shell(dir.path());
    send(&mut shell, enter(true));
    assert!(matches!(shell.app.tactics_phase(), TacticsPhase::Solved(_)));
}

#[test]
fn enter_behind_the_about_box_does_not_leak() {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = solved_shell(dir.path());
    shell.ui.show_about = true;
    assert!(shell.modal_layer_open());
    send(&mut shell, enter(false));
    assert!(matches!(shell.app.tactics_phase(), TacticsPhase::Solved(_)));
}

#[test]
fn enter_with_a_move_being_typed_commits_the_move_not_the_next_problem() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("push.pgn");
    std::fs::write(
        &path,
        "[Event \"push\"]\n[SetUp \"1\"]\n[FEN \"4k3/8/8/8/8/8/4P3/4K3 w - - 0 1\"]\n\n1. e4 *\n",
    )
    .unwrap();
    let mut shell = Shell::new(None);
    let config = TacticsConfig {
        mode: TacticsMode::BestMove,
        file: Some(path),
        ..TacticsConfig::default()
    };
    let effects = update(
        &mut shell.app,
        AppMessage::Tactics(TacticsMessage::Start {
            config: Box::new(config),
            seed: 1,
        }),
    );
    settle(&mut shell.app, effects);
    let mv = shell
        .app
        .session
        .position()
        .find_move("e2".parse().unwrap(), "e4".parse().unwrap(), None)
        .unwrap();
    let effects = update(
        &mut shell.app,
        AppMessage::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }),
    );
    settle(&mut shell.app, effects);
    assert!(matches!(shell.app.tactics_phase(), TacticsPhase::Solved(_)));
    // A character of a move of Black typed over the board.
    drop(update(
        &mut shell.app,
        AppMessage::Game(GameMessage::MoveEntryChar('K')),
    ));
    assert_ne!(shell.app.session.move_entry().text(), "");
    send(&mut shell, enter(false));
    assert!(matches!(shell.app.tactics_phase(), TacticsPhase::Solved(_)));
}

// ---- (4) the question about the unsaved game (D2145) ----

#[test]
fn the_start_with_an_unsaved_game_opens_the_question_as_a_modal_layer() {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = Shell::new(None);
    let mv = shell
        .app
        .session
        .position()
        .find_move("e2".parse().unwrap(), "e4".parse().unwrap(), None)
        .unwrap();
    drop(
        shell.update(Message::App(AppMessage::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }))),
    );
    assert!(shell.app.is_dirty());
    let config = TacticsConfig {
        file: Some(problems(dir.path())),
        ..TacticsConfig::default()
    };
    drop(
        shell.update(Message::App(AppMessage::Tactics(TacticsMessage::Start {
            config: Box::new(config),
            seed: 1,
        }))),
    );
    assert!(shell.modal_layer_open());
    let element = mychess_gui::windows::main_window::view(&shell);
    let mut sim = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1300.0, 900.0),
        element,
    );
    assert!(sim.find(iced::widget::Id::new(DISCARD_BOX_ID)).is_ok());
}

// ---- the new controls ----

#[test]
fn the_panel_offers_to_continue_against_the_engine_once_the_problem_is_over() {
    let dir = tempfile::tempdir().unwrap();
    // A push that is not a mate: there is a game to play on.
    let path = dir.path().join("push.pgn");
    std::fs::write(
        &path,
        "[Event \"push\"]\n[SetUp \"1\"]\n[FEN \"4k3/8/8/8/8/8/4P3/4K3 w - - 0 1\"]\n\n1. e4 *\n",
    )
    .unwrap();
    let mut shell = Shell::new(None);
    let config = TacticsConfig {
        mode: TacticsMode::BestMove,
        file: Some(path),
        ..TacticsConfig::default()
    };
    let effects = update(
        &mut shell.app,
        AppMessage::Tactics(TacticsMessage::Start {
            config: Box::new(config),
            seed: 1,
        }),
    );
    settle(&mut shell.app, effects);
    let finds = |shell: &Shell, text: &str| {
        let i18n = I18n::new(Language::EnUs);
        let element = mychess_gui::windows::tactics_panel::bar(&shell.app, &i18n).expect("a panel");
        let mut sim = iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(1300.0, 200.0),
            element,
        );
        sim.find(text).is_ok()
    };
    assert!(
        finds(&shell, "Continue"),
        "the button is there, disabled while solving"
    );
    assert!(!shell.app.tactics_view().can_continue);
    let mv = shell
        .app
        .session
        .position()
        .find_move("e2".parse().unwrap(), "e4".parse().unwrap(), None)
        .unwrap();
    let effects = update(
        &mut shell.app,
        AppMessage::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }),
    );
    settle(&mut shell.app, effects);
    assert!(shell.app.tactics_view().can_continue);
    assert!(finds(&shell, "Continue"));
}

#[test]
fn the_dialog_has_the_filter_of_the_flag_and_the_question_to_the_engine_in_best_move() {
    let mut shell = Shell::new(None);
    drop(shell.update(Message::TacticsUi(TacticsUiMessage::Open)));
    let texts = |shell: &Shell, wanted: &[&str]| -> Vec<bool> {
        let i18n = I18n::new(Language::EnUs);
        let element = mychess_gui::windows::tactics_dialog::view(
            &shell.ui.dialogs.tactics,
            &shell.app.tactics_view(),
            &i18n,
        );
        let mut sim = iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(1300.0, 1200.0),
            element,
        );
        wanted.iter().map(|t| sim.find(*t).is_ok()).collect()
    };
    let wanted = [
        "Only games with the Tactics flag (databases)",
        "Ask the engine when my move is not the one of the game",
        "Look for mates in the games without markers",
    ];
    // Mate in N: the search for mates and the flag, not the engine.
    assert_eq!(texts(&shell, &wanted), [true, false, true]);
    drop(shell.update(Message::TacticsUi(TacticsUiMessage::Edit(
        TacticsEdit::Mode(TacticsMode::BestMove),
    ))));
    assert_eq!(texts(&shell, &wanted), [true, true, false]);
    assert!(shell.app.tactics_dialog_open());
    let _ = TACTICS_BOX_ID;
}

#[test]
fn the_new_remarks_have_texts_in_both_languages() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for notice in [
            TacticsNotice::NoEngine,
            TacticsNotice::OpinionUnavailable,
            TacticsNotice::ParkedUnsaved,
        ] {
            let text = notice_text(notice, &i18n);
            assert!(!text.is_empty() && !text.contains("tactics-"), "{text}");
        }
    }
}
