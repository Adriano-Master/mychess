//! M5-6d GUI contract: "Export branch to PGN" in the tools of the opening
//! book panel (D2160-D2169): the group, the icon-and-label button with its
//! tip, the stop button, the file dialog answer and the notices in both
//! languages. Spec `docs/specs/m5-6-janela-do-livro.md`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m5_2_gui_common;

use iced::advanced::widget::Id;
use iced::{Settings, Size};
use iced_test::Simulator;
use m5_2_gui_common::*;
use mychess_app::{BookMessage, BookSide, LanguagePref};
use mychess_core::Game;
use mychess_gui::windows::book_panel::{BookUiMessage, EXPORT_STOP_ID, TOOLS_ID, side_id};

fn book(message: BookMessage) -> AppMessage {
    AppMessage::Book(message)
}

fn rig_in(language: LanguagePref) -> TreeRig {
    let mut rig = TreeRig::with_language(language);
    let mut game = Game::new();
    let root = game.root();
    common::line(&mut game, root, &["e2e4", "e7e5"]);
    rig.rig.set_game(game);
    rig.nav(Nav::Start);
    rig.ui(UiMessage::WindowResized(Size::new(1280.0, 800.0)));
    rig.key(command_shift(Key::Char('k')));
    rig
}

fn with_book(language: LanguagePref) -> TreeRig {
    let mut rig = rig_in(language);
    rig.app_msg(book(BookMessage::New {
        side: BookSide::First,
    }));
    rig.app_msg(book(BookMessage::AddGameMove {
        side: BookSide::First,
    }));
    rig
}

fn panel(rig: &TreeRig, size: Size) -> Simulator<'_, Message> {
    Simulator::with_size(
        Settings::default(),
        size,
        mychess_gui::windows::book_panel::view(&rig.rig.shell),
    )
}

fn has_id(rig: &TreeRig, size: Size, id: &str) -> bool {
    panel(rig, size).find(Id::from(id.to_owned())).is_ok()
}

fn press(rig: &TreeRig, id: &str) -> Vec<Message> {
    let mut s = panel(rig, SIZE);
    if s.click(Id::from(id.to_owned())).is_err() {
        return Vec::new();
    }
    s.into_messages().collect()
}

fn open_tools(rig: &mut TreeRig) {
    for message in press(rig, TOOLS_ID) {
        rig.msg(message);
    }
}

#[test]
fn the_export_group_is_in_the_tools_and_only_when_they_are_open() {
    let mut rig = with_book(LanguagePref::EnUs);
    assert!(panel(&rig, SIZE).find("Export branch to PGN").is_err());
    open_tools(&mut rig);
    assert!(panel(&rig, SIZE).find("Export branch to PGN").is_ok());
    assert!(panel(&rig, SIZE).find("Export branch to PGN…").is_ok());
    assert!(has_id(&rig, SIZE, &side_id(BookSide::First, "export")));
    for id in [
        "book-export-plies-down",
        "book-export-lines-up",
        "book-export-weight-up",
    ] {
        assert!(has_id(&rig, SIZE, id), "{id}");
    }
}

#[test]
fn the_button_asks_for_the_file() {
    let mut rig = with_book(LanguagePref::EnUs);
    open_tools(&mut rig);
    let messages = press(&rig, &side_id(BookSide::First, "export"));
    assert!(
        messages.iter().any(|m| matches!(
            m,
            Message::Book(BookUiMessage::ExportBranch(BookSide::First))
        )),
        "{messages:?}"
    );
}

#[test]
fn the_button_is_off_without_moves_in_the_position() {
    let mut rig = rig_in(LanguagePref::EnUs);
    rig.app_msg(book(BookMessage::New {
        side: BookSide::First,
    }));
    open_tools(&mut rig);
    assert!(has_id(&rig, SIZE, &side_id(BookSide::First, "export")));
    assert!(press(&rig, &side_id(BookSide::First, "export")).is_empty());
}

#[test]
fn the_stop_button_is_off_until_an_export_runs() {
    let mut rig = with_book(LanguagePref::EnUs);
    open_tools(&mut rig);
    assert!(has_id(&rig, SIZE, EXPORT_STOP_ID));
    assert!(press(&rig, EXPORT_STOP_ID).is_empty());
}

#[test]
fn the_step_buttons_change_the_options() {
    let mut rig = with_book(LanguagePref::EnUs);
    open_tools(&mut rig);
    for message in press(&rig, "book-export-plies-up") {
        rig.msg(message);
    }
    for message in press(&rig, "book-export-lines-down") {
        rig.msg(message);
    }
    let o = rig.app().book_view().export;
    assert_eq!((o.max_plies, o.max_lines), (14, 150));
}

#[test]
fn the_answer_of_the_dialog_writes_the_file_and_the_panel_says_so() {
    let mut rig = with_book(LanguagePref::EnUs);
    open_tools(&mut rig);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("branch.pgn");
    rig.msg(Message::Book(BookUiMessage::ExportChosen(
        BookSide::First,
        Some(path.clone()),
    )));
    let text = std::fs::read_to_string(&path).expect("file written");
    assert!(text.contains("1. e4"));
    assert!(panel(&rig, SIZE).find("1 line exported.").is_ok());
}

#[test]
fn a_dismissed_dialog_does_nothing() {
    let mut rig = with_book(LanguagePref::EnUs);
    let before = rig.app().book_view().notice;
    rig.msg(Message::Book(BookUiMessage::ExportChosen(
        BookSide::First,
        None,
    )));
    assert_eq!(rig.app().book_view().notice, before);
    assert!(rig.app().book_view().export_progress.is_none());
}

#[test]
fn the_group_is_drawn_in_portuguese() {
    let mut rig = with_book(LanguagePref::PtBr);
    open_tools(&mut rig);
    assert!(panel(&rig, SIZE).find("Exportar ramo para PGN…").is_ok());
    let dir = tempfile::tempdir().unwrap();
    rig.msg(Message::Book(BookUiMessage::ExportChosen(
        BookSide::First,
        Some(dir.path().join("r.pgn")),
    )));
    assert!(panel(&rig, SIZE).find("1 linha exportada.").is_ok());
}

#[test]
fn the_button_fits_a_narrow_panel() {
    let mut rig = with_book(LanguagePref::EnUs);
    open_tools(&mut rig);
    assert!(has_id(
        &rig,
        Size::new(280.0, 700.0),
        &side_id(BookSide::First, "export")
    ));
}

fn ftl(language: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join("book.ftl");
    std::fs::read_to_string(path).expect("ftl")
}

#[test]
fn the_texts_exist_in_both_languages_with_the_tip_and_the_plural_forms() {
    for language in ["en-US", "pt-BR"] {
        let text = ftl(language);
        for key in [
            "book-export-tip",
            "book-export-stop-tip",
            "book-export-make",
            "book-notice-exported",
            "book-notice-exported-truncated",
            "book-issue-protected-file",
        ] {
            assert!(text.contains(&format!("{key} =")), "{language}: {key}");
        }
        let at = text.find("book-notice-exported =").expect("plural message");
        let block = &text[at..at + 200];
        assert!(block.contains("[0]") && block.contains("[one]") && block.contains("*[other]"));
        assert!(!text.to_lowercase().contains("variação"));
    }
}
