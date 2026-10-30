//! M5-6d review fixes, app side (D2170-D2175): stopping an export waits for
//! its answer and tells the truth, a new export cannot start meanwhile, and
//! a name that is not Unicode gets its own message. Spec
//! `docs/specs/m5-6-janela-do-livro.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

use std::path::Path;

use mychess_app::{
    AppState, BookIssue, BookMessage, BookNotice, BookSide, Effect, GameMessage, Message, Nav,
    perform, update,
};
use mychess_core::Game;

const FIRST: BookSide = BookSide::First;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn run(rt: &tokio::runtime::Runtime, state: &mut AppState, message: Message) {
    let mut queue = vec![message];
    while let Some(message) = queue.pop() {
        for effect in update(state, message) {
            if effect.runs_on_host() || matches!(effect, Effect::ScheduleAutoplayTick { .. }) {
                continue;
            }
            queue.push(rt.block_on(perform(effect)));
        }
    }
}

fn rig(rt: &tokio::runtime::Runtime) -> AppState {
    let mut state = AppState::default();
    let mut game = Game::new();
    let mut node = game.root();
    for uci in ["e2e4", "e7e5"] {
        let mv = game
            .position_at(node)
            .expect("position")
            .parse_uci(uci)
            .expect("uci");
        node = game.add_move(node, mv).expect("legal");
    }
    state.session.replace_game(game);
    drop(state.session.update(GameMessage::Navigate(Nav::Start)));
    run(rt, &mut state, BookMessage::New { side: FIRST }.into());
    run(rt, &mut state, BookMessage::AddLine { side: FIRST }.into());
    state
}

fn start(state: &mut AppState, path: &Path) -> Vec<Effect> {
    update(
        state,
        BookMessage::ExportBranch {
            side: FIRST,
            path: path.to_path_buf(),
        }
        .into(),
    )
}

fn notice(state: &AppState) -> Option<BookNotice> {
    state.book_view().notice
}

#[test]
fn a_new_export_is_refused_while_the_stopping_one_has_not_answered() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = rig(&rt);
    let path = dir.path().join("b.pgn");
    let effects = start(&mut state, &path);
    drop(update(&mut state, BookMessage::CancelExport.into()));
    assert!(state.book_view().export_stopping);
    assert!(start(&mut state, &path).is_empty(), "same destination");
    assert_eq!(notice(&state), Some(BookNotice::Issue(BookIssue::Busy)));
    for effect in effects {
        let answer = rt.block_on(perform(effect));
        drop(update(&mut state, answer));
    }
    assert!(!state.book_view().export_stopping);
    assert_eq!(start(&mut state, &path).len(), 1, "free after the answer");
}

#[test]
fn a_stop_that_came_too_late_says_the_file_was_written() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = rig(&rt);
    let path = dir.path().join("b.pgn");
    let effects = start(&mut state, &path);
    // The job finishes before the stop is seen.
    let answers: Vec<Message> = effects
        .into_iter()
        .map(|e| rt.block_on(perform(e)))
        .collect();
    drop(update(&mut state, BookMessage::CancelExport.into()));
    for answer in answers {
        drop(update(&mut state, answer));
    }
    assert_eq!(
        notice(&state),
        Some(BookNotice::ExportFinishedAnyway { lines: 1 })
    );
    assert!(path.is_file(), "the file really exists");
    assert_eq!(
        BookNotice::ExportFinishedAnyway { lines: 1 }.key(),
        "book-notice-export-finished"
    );
}

#[test]
fn stopping_without_an_export_says_nothing() {
    let rt = runtime();
    let mut state = rig(&rt);
    let before = notice(&state);
    drop(update(&mut state, BookMessage::CancelExport.into()));
    assert_eq!(notice(&state), before);
}

#[cfg(unix)]
#[test]
fn a_name_that_is_not_unicode_has_its_own_message() {
    use std::os::unix::ffi::OsStrExt;
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = rig(&rt);
    let name = std::ffi::OsStr::from_bytes(b"bad\xff.pgn");
    let effects = start(&mut state, &dir.path().join(name));
    assert!(effects.is_empty());
    assert_eq!(
        notice(&state),
        Some(BookNotice::Issue(BookIssue::BadFileName))
    );
    assert_eq!(BookIssue::BadFileName.key(), "book-issue-bad-name");
}
