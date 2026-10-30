//! M5-6b contract tests, app side: the keyboard shortcut of the book panel
//! (D1790), the unsaved-books query behind the quit question (D1792) and the
//! cancellation of "make a book from the filter" (D1791). Spec
//! `docs/specs/m5-6-janela-do-livro.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

use mychess_app::{
    Action, AppState, BasesMessage, BookIssue, BookMessage, BookNotice, BookSide, Effect,
    GameMessage, Message, Nav, Shortcuts, perform, update,
};
use mychess_core::Game;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

/// `update` plus every effect that does not run on the host, answers fed
/// back; returns the effects that were run.
fn pump(rt: &tokio::runtime::Runtime, state: &mut AppState, message: Message) -> Vec<Effect> {
    let mut done = Vec::new();
    let mut queue = vec![message];
    while let Some(message) = queue.pop() {
        for effect in update(state, message) {
            if effect.runs_on_host() || matches!(effect, Effect::ScheduleAutoplayTick { .. }) {
                continue;
            }
            done.push(effect.clone());
            queue.push(rt.block_on(perform(effect)));
        }
    }
    done
}

fn game_of(ucis: &[&str]) -> Game {
    let mut game = Game::new();
    let mut node = game.root();
    for uci in ucis {
        let mv = game
            .position_at(node)
            .expect("position")
            .parse_uci(uci)
            .expect("uci");
        node = game.add_move(node, mv).expect("legal");
    }
    game
}

const PGN: &str = "[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 1-0\n\n\
    [Result \"1/2-1/2\"]\n\n1. e4 c5 1/2-1/2\n\n\
    [Result \"1-0\"]\n\n1. e4 e5 2. Nf3 1-0\n\n";

fn with_base(rt: &tokio::runtime::Runtime, dir: &std::path::Path) -> AppState {
    let mut state = AppState::default();
    state.open_pgn_in_slots = true;
    let path = dir.join("games.pgn");
    std::fs::write(&path, PGN).expect("write");
    pump(rt, &mut state, BasesMessage::Open(path).into());
    state
}

fn files(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

// ---- D1790: the shortcut ----

#[test]
fn the_book_pane_has_its_own_free_shortcut() {
    let shortcuts = Shortcuts::defaults();
    let keys = shortcuts.bindings(Action::ToggleBookPane);
    assert_eq!(keys.len(), 1);
    // No other action shares the key.
    let shared = shortcuts
        .iter()
        .filter(|(_, bound)| bound.iter().any(|b| keys.contains(b)))
        .map(|(action, _)| action)
        .collect::<Vec<_>>();
    assert_eq!(shared, vec![Action::ToggleBookPane]);
    assert_eq!(
        Action::from_name(Action::ToggleBookPane.name()),
        Some(Action::ToggleBookPane)
    );
    assert_eq!(
        Action::ToggleBookPane.message_key(),
        "action-toggle-book-pane"
    );
}

#[test]
fn the_book_shortcut_is_left_to_the_front_end() {
    let mut state = AppState::default();
    assert!(update(&mut state, Message::Action(Action::ToggleBookPane)).is_empty());
    assert!(
        !state.book().is_open(),
        "the layout decides, not the action"
    );
}

// ---- D1792: unsaved books ----

#[test]
fn the_unsaved_query_follows_the_edits_and_the_made_books() {
    let rt = runtime();
    let mut state = AppState::default();
    state.session.replace_game(game_of(&["e2e4", "e7e5"]));
    drop(state.session.update(GameMessage::Navigate(Nav::Start)));
    assert!(!state.book_has_unsaved());
    pump(
        &rt,
        &mut state,
        BookMessage::New {
            side: BookSide::First,
        }
        .into(),
    );
    assert!(!state.book_has_unsaved(), "an empty new book loses nothing");
    pump(
        &rt,
        &mut state,
        BookMessage::AddGameMove {
            side: BookSide::First,
        }
        .into(),
    );
    assert!(state.book_has_unsaved());
    pump(&rt, &mut state, BookMessage::ConfirmClose(true).into());
    pump(
        &rt,
        &mut state,
        BookMessage::Close {
            side: BookSide::First,
        }
        .into(),
    );
    pump(&rt, &mut state, BookMessage::ConfirmClose(true).into());
    assert!(!state.book_has_unsaved(), "closed after the confirmation");
}

#[test]
fn saving_leaves_nothing_unsaved() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = AppState::default();
    state.session.replace_game(game_of(&["e2e4"]));
    drop(state.session.update(GameMessage::Navigate(Nav::Start)));
    pump(
        &rt,
        &mut state,
        BookMessage::New {
            side: BookSide::Second,
        }
        .into(),
    );
    pump(
        &rt,
        &mut state,
        BookMessage::AddGameMove {
            side: BookSide::Second,
        }
        .into(),
    );
    assert!(state.book_has_unsaved());
    let path = dir.path().join("out.bin");
    pump(
        &rt,
        &mut state,
        BookMessage::SaveAs {
            side: BookSide::Second,
            path: path.clone(),
        }
        .into(),
    );
    assert!(!state.book_has_unsaved());
    assert!(path.exists());
}

// ---- D1791: cancel the creation ----

#[test]
fn a_book_is_made_from_the_filter_when_nobody_cancels() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = with_base(&rt, dir.path());
    pump(
        &rt,
        &mut state,
        BookMessage::CreateFromFilter {
            side: BookSide::First,
        }
        .into(),
    );
    let view = state.book_view();
    assert!(!view.creating && !view.busy);
    assert!(view.sides[0].is_some(), "the book is there");
    assert!(state.book_has_unsaved(), "made from games: not saved yet");
}

#[test]
fn cancelling_drops_the_book_and_leaves_no_file() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = with_base(&rt, dir.path());
    let before = files(dir.path());
    let effects = update(
        &mut state,
        BookMessage::CreateFromFilter {
            side: BookSide::First,
        }
        .into(),
    );
    let [effect] = effects.as_slice() else {
        panic!("one effect, got {effects:?}");
    };
    assert!(matches!(effect, Effect::BookCreate { .. }));
    assert!(state.book_view().creating);
    assert!(state.book_view().busy);
    // Stop.
    assert!(update(&mut state, BookMessage::CancelCreate.into()).is_empty());
    let view = state.book_view();
    assert!(!view.creating && !view.busy, "the side is free at once");
    assert_eq!(view.notice, Some(BookNotice::CreateCancelled));
    // The work still runs, sees the flag and answers "cancelled"; the answer
    // finds no job and changes nothing.
    let answer = rt.block_on(perform(effect.clone()));
    assert!(matches!(
        answer,
        Message::Book(BookMessage::Created {
            result: Err(BookIssue::Cancelled),
            ..
        })
    ));
    assert!(update(&mut state, answer).is_empty());
    let view = state.book_view();
    assert!(view.sides[0].is_none());
    assert_eq!(view.notice, Some(BookNotice::CreateCancelled));
    assert!(!state.book_has_unsaved());
    assert_eq!(files(dir.path()), before, "no file was left");
}

#[test]
fn a_late_answer_of_a_cancelled_job_does_not_replace_a_later_book() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = with_base(&rt, dir.path());
    let first = update(
        &mut state,
        BookMessage::CreateFromFilter {
            side: BookSide::First,
        }
        .into(),
    );
    update(&mut state, BookMessage::CancelCreate.into());
    // A second run is allowed right away.
    pump(
        &rt,
        &mut state,
        BookMessage::CreateFromFilter {
            side: BookSide::First,
        }
        .into(),
    );
    let entries = state.book_view().sides[0].as_ref().expect("book").entries;
    assert!(entries > 0);
    // The first job answers late: ignored.
    let late = rt.block_on(perform(first[0].clone()));
    update(&mut state, late);
    assert_eq!(
        state.book_view().sides[0].as_ref().expect("book").entries,
        entries
    );
}

#[test]
fn cancel_without_a_job_is_harmless() {
    let mut state = AppState::default();
    assert!(update(&mut state, BookMessage::CancelCreate.into()).is_empty());
    assert_eq!(state.book_view().notice, None);
}
