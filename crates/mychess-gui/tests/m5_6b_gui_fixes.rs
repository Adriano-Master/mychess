//! M5-6b review fixes (GUI): the question about the unsaved books is never
//! skipped (D1795), the save before quitting can be aborted and has a limit
//! (D1796), and the question is never hidden behind another dialog (D1797).
//! Spec `docs/specs/m5-6-janela-do-livro.md`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m5_2_gui_common;

use m5_2_gui_common::*;
use mychess_app::{BookMessage, BookSide, NamedKey};
use mychess_core::Game;
use mychess_gui::windows::book_panel::BookUiMessage;
use mychess_gui::windows::dialog_ui::ActiveDialog;
use mychess_gui::windows::online_ui::{OnlineConfirm, OnlineUiMessage};
use mychess_gui::windows::tournament_ui::{TournamentConfirm, TournamentUiMessage};
use tempfile::tempdir;

fn book(message: BookMessage) -> AppMessage {
    AppMessage::Book(message)
}

fn asking(rig: &TreeRig) -> bool {
    ActiveDialog::of(&rig.rig.shell.app, &rig.rig.shell.ui.dialogs) == Some(ActiveDialog::BookQuit)
}

fn rig() -> TreeRig {
    let mut rig = TreeRig::new();
    let mut game = Game::new();
    let root = game.root();
    common::line(&mut game, root, &["e2e4", "e7e5"]);
    rig.rig.set_game(game);
    rig.nav(Nav::Start);
    rig.ui(UiMessage::WindowResized(iced::Size::new(1280.0, 800.0)));
    rig
}

fn quit_started(rig: &TreeRig) -> bool {
    rig.app().prefs.window.width == Some(1280)
}

fn dirty_book(rig: &mut TreeRig) {
    rig.app_msg(book(BookMessage::New {
        side: BookSide::First,
    }));
    rig.app_msg(book(BookMessage::AddGameMove {
        side: BookSide::First,
    }));
    assert!(rig.app().book_has_unsaved());
}

/// Runs the messages of a click in the shell without running the tasks they
/// start: the jobs they open stay in flight.
fn click_without_tasks(rig: &mut TreeRig, text: &str) {
    for message in rig.click_messages(text) {
        drop(rig.rig.shell.update(message));
    }
}

// ---- D1795 ----

#[test]
fn yes_in_the_online_question_asks_about_the_books() {
    let mut rig = rig();
    dirty_book(&mut rig);
    rig.rig.shell.ui.dialogs.online.confirm = Some(OnlineConfirm::Quit);
    rig.msg(Message::Online(OnlineUiMessage::Confirm(true)));
    assert!(asking(&rig));
    assert!(!quit_started(&rig));
}

#[test]
fn yes_in_the_tournament_question_asks_about_the_books() {
    let mut rig = rig();
    dirty_book(&mut rig);
    rig.rig.shell.ui.dialogs.tournament.confirm = Some(TournamentConfirm::Quit);
    rig.msg(Message::Tournament(TournamentUiMessage::Confirm(true)));
    assert!(asking(&rig));
    assert!(!quit_started(&rig));
}

#[test]
fn enter_in_the_tournament_question_asks_about_the_books() {
    let mut rig = rig();
    dirty_book(&mut rig);
    rig.rig.shell.ui.dialogs.tournament.confirm = Some(TournamentConfirm::Quit);
    rig.key(named(NamedKey::Enter));
    assert!(asking(&rig), "Enter did not skip the question");
    assert!(!quit_started(&rig));
}

#[test]
fn enter_in_the_tournament_question_quits_when_the_books_are_clean() {
    let mut rig = rig();
    rig.rig.shell.ui.dialogs.tournament.confirm = Some(TournamentConfirm::Quit);
    rig.key(named(NamedKey::Enter));
    assert!(!asking(&rig));
    assert!(quit_started(&rig));
}

// ---- D1796 ----

#[test]
fn a_second_close_request_aborts_the_save_run() {
    let mut rig = rig();
    dirty_book(&mut rig);
    rig.ui(UiMessage::CloseRequested);
    // The file dialog is open and never answers.
    click_without_tasks(&mut rig, "Save");
    assert!(!asking(&rig));
    rig.ui(UiMessage::CloseRequested);
    assert!(!quit_started(&rig));
    // The run is over: a late answer of the dialog does not quit.
    let dir = tempdir().unwrap();
    rig.msg(Message::Book(BookUiMessage::SavedAs(
        BookSide::First,
        Some(dir.path().join("late.bin")),
    )));
    assert!(!quit_started(&rig));
    // And asking again works.
    rig.ui(UiMessage::CloseRequested);
    assert!(asking(&rig) || quit_started(&rig));
}

#[test]
fn a_second_close_request_with_the_question_open_is_ignored() {
    let mut rig = rig();
    dirty_book(&mut rig);
    rig.ui(UiMessage::CloseRequested);
    assert!(asking(&rig));
    rig.ui(UiMessage::CloseRequested);
    assert!(asking(&rig));
}

/// A book with a file whose second save never answers.
fn stuck_save(rig: &mut TreeRig, dir: &std::path::Path) {
    dirty_book(rig);
    rig.app_msg(book(BookMessage::SaveAs {
        side: BookSide::First,
        path: dir.join("mine.bin"),
    }));
    rig.nav(Nav::Start);
    let d4 = rig.app().session.position().parse_uci("d2d4").unwrap();
    rig.app_msg(book(BookMessage::AddMove {
        side: BookSide::First,
        mv: d4,
    }));
    rig.ui(UiMessage::CloseRequested);
    click_without_tasks(rig, "Save");
    assert!(rig.app().book().is_saving(), "the save is in flight");
}

#[test]
fn a_save_that_never_ends_stops_the_run_after_five_seconds() {
    let dir = tempdir().unwrap();
    let mut rig = rig();
    stuck_save(&mut rig, dir.path());
    for _ in 0..4 {
        rig.ui(UiMessage::BookQuitTick);
    }
    assert!(!quit_started(&rig));
    assert!(!rig.rig.shell.ui.is_open(PaneKind::Book), "still waiting");
    rig.ui(UiMessage::BookQuitTick);
    assert!(rig.rig.shell.ui.is_open(PaneKind::Book), "the panel shows");
    assert!(!quit_started(&rig));
    assert!(rig.app().book_has_unsaved());
}

#[test]
fn the_time_of_the_file_dialog_does_not_count() {
    let mut rig = rig();
    dirty_book(&mut rig);
    rig.ui(UiMessage::CloseRequested);
    click_without_tasks(&mut rig, "Save");
    for _ in 0..20 {
        rig.ui(UiMessage::BookQuitTick);
    }
    // Still in the run: the answer of the dialog finishes it.
    let dir = tempdir().unwrap();
    rig.msg(Message::Book(BookUiMessage::SavedAs(
        BookSide::First,
        Some(dir.path().join("slow.bin")),
    )));
    assert!(quit_started(&rig));
}

// ---- D1797 ----

#[test]
fn the_question_stays_on_top_of_another_dialog() {
    let mut rig = rig();
    dirty_book(&mut rig);
    rig.ui(UiMessage::CloseRequested);
    assert!(asking(&rig));
    rig.rig.shell.ui.dialogs.tournament.confirm = Some(TournamentConfirm::Abort);
    assert!(asking(&rig), "the question is not hidden");
}
