//! Regression tests of the review of the M8-4d in the GUI (spec D2158-D2189):
//! `Enter` of the training comes before the keys of the game list, the new
//! remark has its own text, and the count of the mates beyond the limit
//! reads well in the singular and the plural.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::collections::VecDeque;
use std::path::Path;

use fluent_bundle::FluentArgs;
use mychess_app::PaneKind;
use mychess_app::tactics::{
    TacticsConfig, TacticsEffect, TacticsMessage, TacticsNotice, TacticsPhase, find_next, judge,
    open_file,
};
use mychess_app::{
    AppState, BasesMessage, Effect, GameMessage, Key, KeyInput, Message as AppMessage, Modifiers,
    NamedKey, PlayOptions, SlotId, perform, update,
};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::dialog_ui::DialogMessage;
use mychess_gui::windows::game_list::GameListMessage;
use mychess_gui::windows::tactics_text::notice_text;
use mychess_gui::{Message, Shell, UiMessage};

const MATE1: &str = "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1";

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

/// Opens a PGN base in a slot, the way the application does, and selects its
/// first game in the list.
fn open_base_with_a_selected_game(shell: &mut Shell, dir: &Path) {
    let path = dir.join("base.pgn");
    std::fs::write(
        &path,
        "[Event \"E1\"]\n\n1. e4 e5 1-0\n\n[Event \"E2\"]\n\n1. d4 d5 *\n",
    )
    .unwrap();
    shell.app.open_pgn_in_slots = true;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let mut queue: VecDeque<Effect> =
        update(&mut shell.app, AppMessage::Bases(BasesMessage::Open(path))).into();
    while let Some(effect) = queue.pop_front() {
        if effect.runs_on_host() {
            continue;
        }
        let answer = runtime.block_on(perform(effect));
        queue.extend(update(&mut shell.app, answer));
    }
    let slot: SlotId = shell.app.bases.current();
    drop(update(
        &mut shell.app,
        AppMessage::Bases(BasesMessage::SelectRow {
            slot,
            number: Some(1),
        }),
    ));
}

/// The panel of the game list on screen, with the pointer over it.
fn open_the_list(shell: &mut Shell) {
    drop(shell.update(Message::Ui(UiMessage::TogglePane(PaneKind::GameList))));
    drop(shell.update(Message::Games(GameListMessage::Hover(true))));
    assert!(shell.ui.is_visible(PaneKind::GameList));
}

fn solved_shell(dir: &Path) -> Shell {
    let mut shell = Shell::new(None);
    open_base_with_a_selected_game(&mut shell, dir);
    let path = dir.join("p.pgn");
    std::fs::write(
        &path,
        format!(
            "[Event \"One\"]\n[SetUp \"1\"]\n[FEN \"{MATE1}\"]\n\n*\n\n\
             [Event \"Two\"]\n[SetUp \"1\"]\n[FEN \"{MATE1}\"]\n\n*\n"
        ),
    )
    .unwrap();
    let config = TacticsConfig {
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

fn enter() -> Message {
    Message::Dialog(DialogMessage::Key {
        input: KeyInput {
            key: Key::Named(NamedKey::Enter),
            modifiers: Modifiers::NONE,
        },
        text_focused: false,
        typed: None,
    })
}

#[test]
fn enter_with_the_pointer_over_the_game_list_goes_to_the_next_problem_not_to_the_game() {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = solved_shell(dir.path());
    // The list of the open base is on screen, a game is selected and the
    // pointer is over it: Enter would load that game.
    open_the_list(&mut shell);
    drop(shell.update(enter()));
    assert_eq!(
        shell.app.tactics_phase(),
        TacticsPhase::Searching,
        "the next problem was asked for, the game of the list was not loaded"
    );
    assert_eq!(shell.app.session.game().tags().event, "Tactics");
}

#[test]
fn the_remark_of_continue_without_an_engine_is_not_the_one_of_the_second_opinion() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        let play = notice_text(TacticsNotice::NoEngineToPlay, &i18n);
        let opinion = notice_text(TacticsNotice::NoEngine, &i18n);
        assert!(!play.is_empty() && !play.contains("tactics-"));
        assert_ne!(play, opinion, "{language:?}");
    }
}

#[test]
fn the_count_of_the_mates_beyond_the_limit_has_its_singular_and_its_plural() {
    let say = |language, count: i64, limit: i64| {
        let i18n = I18n::new(language);
        let mut args = FluentArgs::new();
        args.set("count", count);
        args.set("limit", limit);
        i18n.tr_args("tactics-beyond-limit", &args)
    };
    let one = say(Language::EnUs, 1, 3);
    let many = say(Language::EnUs, 5, 1);
    assert!(
        one.contains("1 mate problem ") && one.contains("3 moves") && one.contains("was"),
        "{one}"
    );
    assert!(
        many.contains("5 mate problems") && many.contains("1 move ") && many.contains("were"),
        "{many}"
    );
    let pt_one = say(Language::PtBr, 1, 3);
    let pt_many = say(Language::PtBr, 2, 1);
    assert!(
        pt_one.contains("1 problema ") && pt_one.contains("foi ignorado"),
        "{pt_one}"
    );
    assert!(
        pt_many.contains("2 problemas")
            && pt_many.contains("1 lance ")
            && pt_many.contains("foram"),
        "{pt_many}"
    );
    for text in [&one, &many, &pt_one, &pt_many] {
        assert!(!text.contains('{') && !text.contains("tactics-"), "{text}");
    }
}
