//! M5-6b GUI contract: the shortcut of the book panel (D1790), the stop
//! button of "make a book from the filter" (D1791) and the question before
//! the program quits with unsaved books (D1792). Spec
//! `docs/specs/m5-6-janela-do-livro.md`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m5_2_gui_common;

use m5_2_gui_common::*;
use mychess_app::{BookMessage, BookSide, LanguagePref};
use mychess_core::Game;
use mychess_gui::windows::book_panel::BookUiMessage;
use mychess_gui::windows::dialog_ui::ActiveDialog;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use tempfile::tempdir;

fn book(message: BookMessage) -> AppMessage {
    AppMessage::Book(message)
}

fn open(rig: &TreeRig) -> bool {
    rig.rig.shell.ui.is_open(PaneKind::Book)
}

fn asking(rig: &TreeRig) -> bool {
    ActiveDialog::of(&rig.rig.shell.app, &rig.rig.shell.ui.dialogs) == Some(ActiveDialog::BookQuit)
}

/// A rig whose window is 1280 wide (what quitting records) and whose game
/// is 1.e4 e5.
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

/// Whether the program went on to quit (the geometry is recorded then).
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

// ---- D1790 ----

#[test]
fn command_shift_k_shows_and_hides_the_panel_and_the_menu_says_so() {
    let mut rig = TreeRig::new();
    assert!(!open(&rig));
    rig.key(command_shift(Key::Char('k')));
    assert!(open(&rig) && rig.app().book().is_open());
    assert!(rig.shows("Compare"), "the panel is drawn");
    rig.key(command_shift(Key::Char('k')));
    assert!(!open(&rig) && !rig.app().book().is_open());
    // Plain Command+K is not the panel.
    rig.key(command(Key::Char('k')));
    assert!(!open(&rig));
    let shell = &rig.rig.shell;
    let tools = menu_specs(&shell.app, &shell.ui, shell.options)
        .into_iter()
        .find(|s| s.label_key == "menu-tools")
        .expect("tools menu");
    let shortcut = tools.entries.iter().find_map(|e| match e {
        MenuEntry::Item {
            label: Label::Key("menu-tools-book"),
            shortcut,
            ..
        } => Some(shortcut.is_some()),
        _ => None,
    });
    assert_eq!(shortcut, Some(true));
}

// ---- D1791 ----

#[test]
fn the_stop_button_is_live_only_while_a_book_is_being_made() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    rig.key(command_shift(Key::Char('k')));
    // The tools start closed (D2044): open them to reach the button.
    rig.msg(Message::Book(BookUiMessage::ToggleTools));
    assert!(rig.shows("Stop"));
    assert!(
        rig.click_messages("Stop").is_empty(),
        "disabled: nothing to stop"
    );
    // Start the work but do not run it: the job stays in flight.
    drop(
        rig.rig
            .shell
            .update(Message::App(book(BookMessage::CreateFromFilter {
                side: BookSide::First,
            }))),
    );
    assert!(rig.app().book_view().creating);
    let messages = rig.click_messages("Stop");
    assert!(
        sent(&messages, &Message::App(book(BookMessage::CancelCreate))),
        "{messages:?}"
    );
    for message in messages {
        rig.msg(message);
    }
    let view = rig.app().book_view();
    assert!(!view.creating && view.sides[0].is_none());
    assert!(rig.shows("Stopped. No book was made."));
}

#[test]
fn the_stop_button_has_a_portuguese_label() {
    let mut rig = TreeRig::with_language(LanguagePref::PtBr);
    rig.key(command_shift(Key::Char('k')));
    rig.msg(Message::Book(BookUiMessage::ToggleTools));
    assert!(rig.shows("Parar"));
}

// ---- D1792 ----

#[test]
fn closing_with_clean_books_quits_at_once() {
    let mut rig = rig();
    rig.app_msg(book(BookMessage::New {
        side: BookSide::First,
    }));
    rig.ui(UiMessage::CloseRequested);
    assert!(!asking(&rig));
    assert!(quit_started(&rig));
}

#[test]
fn closing_with_a_changed_book_asks_and_the_layer_owns_the_keyboard() {
    let mut rig = rig();
    dirty_book(&mut rig);
    rig.ui(UiMessage::CloseRequested);
    assert!(asking(&rig) && rig.rig.shell.modal_layer_open());
    assert!(!quit_started(&rig));
    for text in [
        "Opening books with unsaved changes",
        "Save",
        "Discard",
        "Cancel",
    ] {
        assert!(rig.shows(text), "{text}");
    }
    // A second close request does not stack anything or quit.
    rig.ui(UiMessage::CloseRequested);
    assert!(asking(&rig) && !quit_started(&rig));
}

#[test]
fn cancel_esc_and_enter_all_stay_in_the_program() {
    for how in 0..3 {
        let mut rig = rig();
        dirty_book(&mut rig);
        rig.ui(UiMessage::CloseRequested);
        match how {
            0 => rig.click("Cancel"),
            1 => rig.key(named(NamedKey::Escape)),
            _ => rig.key(named(NamedKey::Enter)),
        }
        assert!(!asking(&rig), "{how}");
        assert!(!quit_started(&rig), "{how}");
        assert!(rig.app().book_has_unsaved(), "the book is kept ({how})");
    }
}

#[test]
fn discard_quits_without_saving() {
    let dir = tempdir().unwrap();
    let mut rig = rig();
    dirty_book(&mut rig);
    rig.ui(UiMessage::CloseRequested);
    rig.click("Discard");
    assert!(!asking(&rig) && quit_started(&rig));
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
}

#[test]
fn save_writes_the_book_that_has_a_file_and_then_quits() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mine.bin");
    let mut rig = rig();
    dirty_book(&mut rig);
    rig.app_msg(book(BookMessage::SaveAs {
        side: BookSide::First,
        path: path.clone(),
    }));
    assert!(!rig.app().book_has_unsaved());
    let first = std::fs::metadata(&path).unwrap().len();
    // Another edit: a second move in the same position.
    rig.nav(Nav::Start);
    let start = rig.app().session.position().clone();
    let d4 = start.parse_uci("d2d4").unwrap();
    rig.app_msg(book(BookMessage::AddMove {
        side: BookSide::First,
        mv: d4,
    }));
    assert!(rig.app().book_has_unsaved());
    rig.ui(UiMessage::CloseRequested);
    assert!(asking(&rig));
    rig.click("Save");
    assert!(!asking(&rig));
    assert!(!rig.app().book_has_unsaved());
    assert!(
        std::fs::metadata(&path).unwrap().len() > first,
        "d4 was written"
    );
    assert!(quit_started(&rig), "quits once the book is on disk");
}

#[test]
fn a_book_without_a_file_gets_the_dialog_and_the_answer_saves_it() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("new.bin");
    let mut rig = rig();
    dirty_book(&mut rig);
    rig.ui(UiMessage::CloseRequested);
    // The file dialog is a task that is not run here.
    let messages = rig.click_messages("Save");
    for message in messages {
        drop(rig.rig.shell.update(message));
    }
    assert!(!asking(&rig) && !quit_started(&rig), "waits for the dialog");
    // The dialog answers.
    rig.msg(Message::Book(BookUiMessage::SavedAs(
        BookSide::First,
        Some(path.clone()),
    )));
    assert!(path.exists());
    assert!(quit_started(&rig));
}

#[test]
fn dismissing_the_save_dialog_keeps_the_program_open() {
    let mut rig = rig();
    dirty_book(&mut rig);
    rig.ui(UiMessage::CloseRequested);
    for message in rig.click_messages("Save") {
        drop(rig.rig.shell.update(message));
    }
    rig.msg(Message::Book(BookUiMessage::SavedAs(BookSide::First, None)));
    assert!(!asking(&rig) && !quit_started(&rig));
    assert!(rig.app().book_has_unsaved());
    // Asking again works.
    rig.ui(UiMessage::CloseRequested);
    assert!(asking(&rig));
}

#[test]
fn a_failed_save_stops_the_quit_and_shows_the_panel() {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("gone");
    std::fs::create_dir(&sub).unwrap();
    let path = sub.join("mine.bin");
    let mut rig = rig();
    dirty_book(&mut rig);
    rig.app_msg(book(BookMessage::SaveAs {
        side: BookSide::First,
        path,
    }));
    rig.nav(Nav::Start);
    let d4 = rig.app().session.position().parse_uci("d2d4").unwrap();
    rig.app_msg(book(BookMessage::AddMove {
        side: BookSide::First,
        mv: d4,
    }));
    std::fs::remove_dir_all(&sub).unwrap();
    rig.ui(UiMessage::CloseRequested);
    rig.click("Save");
    assert!(!quit_started(&rig));
    assert!(rig.app().book_has_unsaved());
    assert!(open(&rig), "the panel shows the reason");
    assert!(rig.shows("The book could not be written."));
}

#[test]
fn the_question_is_in_portuguese_too() {
    let mut rig = TreeRig::with_language(LanguagePref::PtBr);
    let mut game = Game::new();
    let root = game.root();
    common::line(&mut game, root, &["e2e4"]);
    rig.rig.set_game(game);
    rig.nav(Nav::Start);
    dirty_book(&mut rig);
    rig.ui(UiMessage::CloseRequested);
    for text in [
        "Livros de abertura com alterações não salvas",
        "Salvar",
        "Descartar",
        "Cancelar",
    ] {
        assert!(rig.shows(text), "{text}");
    }
}
