//! M5-6c GUI contract: the redesigned opening book panel (D2040-D2069):
//! one book by default, a second one to compare, the tools closed until
//! asked for, every button still reachable, nothing lost when the second
//! book is closed, and short texts in both languages. Spec
//! `docs/specs/m5-6-janela-do-livro.md`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m5_2_gui_common;

use iced::advanced::widget::Id;
use iced::{Settings, Size};
use iced_test::Simulator;
use m5_2_gui_common::*;
use mychess_app::{BookMessage, BookSide, LanguagePref};
use mychess_core::Game;
use mychess_gui::windows::book_panel::{
    CLEAR_OPENING_ID, COMPARE_ID, MERGE_ID, STOP_ID, TOOLS_ID, side_id,
};
use mychess_gui::windows::dialog_ui::ActiveDialog;

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

fn rig() -> TreeRig {
    rig_in(LanguagePref::EnUs)
}

fn dirty(rig: &mut TreeRig, side: BookSide) {
    rig.app_msg(book(BookMessage::New { side }));
    rig.app_msg(book(BookMessage::AddGameMove { side }));
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

fn click_id(rig: &mut TreeRig, id: &str) {
    let messages = {
        let mut s = panel(rig, SIZE);
        s.click(Id::from(id.to_owned()))
            .unwrap_or_else(|e| panic!("{id}: {e:?}"));
        s.into_messages().collect::<Vec<_>>()
    };
    assert!(!messages.is_empty(), "{id} sent nothing");
    for message in messages {
        rig.msg(message);
    }
}

fn second(rig: &TreeRig) -> bool {
    rig.app().book_view().sides[1].is_some()
}

fn shows_in_panel(rig: &TreeRig, text: &str) -> bool {
    panel(rig, SIZE).find(text).is_ok()
}

// ---- D2040-D2042: one book, a second one to compare ----

#[test]
fn the_panel_shows_one_book_by_default_and_the_app_keeps_two_sides() {
    let mut rig = rig();
    dirty(&mut rig, BookSide::First);
    assert_eq!(rig.app().book_view().sides.len(), 2);
    assert!(shows_in_panel(&rig, "Compare"));
    assert!(shows_in_panel(&rig, "e4"));
    for text in ["Book 1", "Book 2", "Into book 2"] {
        assert!(!shows_in_panel(&rig, text), "{text}");
    }
    assert!(!has_id(&rig, SIZE, &side_id(BookSide::Second, "open")));
    assert!(!rig.rig.shell.ui.dialogs.book.compare);
}

#[test]
fn compare_opens_the_second_book_and_the_x_closes_it_again() {
    let mut rig = rig();
    dirty(&mut rig, BookSide::First);
    click_id(&mut rig, COMPARE_ID);
    assert!(rig.rig.shell.ui.dialogs.book.compare);
    assert!(shows_in_panel(&rig, "Book 1") && shows_in_panel(&rig, "Book 2"));
    assert!(has_id(&rig, SIZE, &side_id(BookSide::Second, "open")));
    assert!(!shows_in_panel(&rig, "Compare"), "the button gives way");
    // The empty second book is closed without a question.
    click_id(&mut rig, COMPARE_ID);
    assert!(!rig.rig.shell.ui.dialogs.book.compare);
    assert!(!shows_in_panel(&rig, "Book 2"));
    assert!(shows_in_panel(&rig, "Compare"));
    assert!(rig.app().book_has_unsaved(), "book 1 is untouched");
}

#[test]
fn a_clean_second_book_closes_with_the_comparison() {
    let mut rig = rig();
    click_id(&mut rig, COMPARE_ID);
    rig.app_msg(book(BookMessage::New {
        side: BookSide::Second,
    }));
    assert!(second(&rig) && shows_in_panel(&rig, "Book 2"));
    click_id(&mut rig, COMPARE_ID);
    assert!(!second(&rig), "the clean book was closed");
    assert!(!shows_in_panel(&rig, "Book 2"));
}

#[test]
fn closing_the_comparison_with_a_changed_book_asks_and_loses_nothing() {
    let mut rig = rig();
    click_id(&mut rig, COMPARE_ID);
    dirty(&mut rig, BookSide::Second);
    assert!(second(&rig));
    click_id(&mut rig, COMPARE_ID);
    // The book is still there, with the question of the application.
    assert!(second(&rig) && rig.app().book_has_unsaved());
    assert_eq!(rig.app().book_view().confirm_close, Some(BookSide::Second));
    assert!(shows_in_panel(&rig, "Book 2"), "a changed book never hides");
    assert!(shows_in_panel(&rig, "Close and discard"));
    // Keep: nothing happens to the book.
    click_id(&mut rig, "book-confirm-no");
    assert!(second(&rig) && rig.app().book_has_unsaved());
    assert!(shows_in_panel(&rig, "Book 2"));
    // Ask again and discard: the column goes away with the book.
    click_id(&mut rig, COMPARE_ID);
    click_id(&mut rig, "book-confirm-yes");
    assert!(!second(&rig));
    assert!(!shows_in_panel(&rig, "Book 2"));
    assert!(shows_in_panel(&rig, "Compare"));
}

#[test]
fn a_book_that_arrives_in_the_second_side_shows_without_asking() {
    let mut rig = rig();
    dirty(&mut rig, BookSide::Second);
    assert!(!rig.rig.shell.ui.dialogs.book.compare);
    assert!(shows_in_panel(&rig, "Book 2"), "never an invisible book");
}

#[test]
fn quitting_still_asks_about_the_second_book() {
    let mut rig = rig();
    click_id(&mut rig, COMPARE_ID);
    dirty(&mut rig, BookSide::Second);
    click_id(&mut rig, COMPARE_ID);
    rig.ui(UiMessage::CloseRequested);
    assert_eq!(
        ActiveDialog::of(&rig.rig.shell.app, &rig.rig.shell.ui.dialogs),
        Some(ActiveDialog::BookQuit)
    );
    assert!(rig.rig.shell.modal_layer_open());
}

#[test]
fn the_compare_state_is_visual_and_survives_closing_the_panel() {
    let mut rig = rig();
    click_id(&mut rig, COMPARE_ID);
    rig.key(command_shift(Key::Char('k')));
    rig.key(command_shift(Key::Char('k')));
    assert!(rig.rig.shell.ui.dialogs.book.compare);
    assert!(shows_in_panel(&rig, "Book 2"));
}

// ---- D2044: the tools ----

#[test]
fn the_tools_start_closed_and_open_on_demand() {
    let mut rig = rig();
    dirty(&mut rig, BookSide::First);
    assert!(shows_in_panel(&rig, "Tools"));
    for text in [
        "Make from the filter",
        "Edit this position",
        "Opening book for annotation",
        "Add line",
    ] {
        assert!(!shows_in_panel(&rig, text), "{text} must start hidden");
    }
    // The table is read-only: no weight field, no x2 button.
    assert!(!has_id(&rig, SIZE, &side_id(BookSide::First, "double-e4")));
    click_id(&mut rig, TOOLS_ID);
    for text in [
        "Make from the filter",
        "Edit this position",
        "Opening book for annotation",
        "Add line",
    ] {
        assert!(shows_in_panel(&rig, text), "{text}");
    }
    assert!(has_id(&rig, SIZE, &side_id(BookSide::First, "double-e4")));
    assert!(!shows_in_panel(&rig, "Merge books"), "needs two books");
    click_id(&mut rig, TOOLS_ID);
    assert!(!shows_in_panel(&rig, "Make from the filter"));
}

#[test]
fn the_tools_open_by_themselves_while_a_book_is_being_made() {
    let rig = {
        let dir = tempfile::tempdir().unwrap();
        let mut rig = TreeRig::with_std(dir.path());
        rig.key(command_shift(Key::Char('k')));
        drop(
            rig.rig
                .shell
                .update(Message::App(book(BookMessage::CreateFromFilter {
                    side: BookSide::First,
                }))),
        );
        assert!(rig.app().book_view().creating);
        rig
    };
    assert!(!rig.rig.shell.ui.dialogs.book.tools_open);
    assert!(has_id(&rig, SIZE, STOP_ID), "the stop button is in reach");
}

// ---- every button stays reachable ----

#[test]
fn every_button_of_the_panel_is_reachable_with_two_books_and_the_tools_open() {
    let mut rig = rig();
    click_id(&mut rig, COMPARE_ID);
    dirty(&mut rig, BookSide::First);
    dirty(&mut rig, BookSide::Second);
    click_id(&mut rig, TOOLS_ID);
    let mut ids = vec![
        TOOLS_ID.to_owned(),
        COMPARE_ID.to_owned(),
        STOP_ID.to_owned(),
        MERGE_ID.to_owned(),
        "book-create-1".to_owned(),
        "book-create-2".to_owned(),
    ];
    for name in ["plies", "games", "score"] {
        for way in ["down", "up"] {
            ids.push(format!("book-step-{name}-{way}"));
        }
    }
    for side in BookSide::ALL {
        for name in [
            "open",
            "new",
            "save",
            "save-as",
            "close",
            "add-move",
            "add-line",
            "remove-line",
            "import-branch",
            "copy",
            "double-e4",
            "half-e4",
            "remove-e4",
        ] {
            ids.push(side_id(side, name));
        }
    }
    for id in &ids {
        for size in [SIZE, Size::new(800.0, 560.0), Size::new(280.0, 700.0)] {
            assert!(has_id(&rig, size, id), "{id} at {size:?}");
        }
    }
    // The opening book buttons need a chosen book or a file; the clear
    // button follows the opening book.
    assert!(!has_id(&rig, SIZE, CLEAR_OPENING_ID));
    assert!(shows_in_panel(&rig, "Merge books"));
}

#[test]
fn the_icon_buttons_send_the_messages_of_the_old_panel() {
    let mut rig = rig();
    dirty(&mut rig, BookSide::First);
    for (name, expected) in [
        ("close", "Close"),
        ("save-as", "SaveAs"),
        ("open", "Open"),
        ("save", "Save"),
        ("new", "New"),
    ] {
        let messages = {
            let mut s = panel(&rig, SIZE);
            s.click(Id::from(side_id(BookSide::First, name)))
                .unwrap_or_else(|e| panic!("{name}: {e:?}"));
            s.into_messages().collect::<Vec<_>>()
        };
        assert!(
            messages.iter().any(|m| {
                let text = dbg(m);
                text.contains(&format!("{expected}(First"))
                    || text.contains(&format!("{expected} {{"))
            }),
            "{name}: {messages:?}"
        );
    }
}

// ---- narrow panel, texts and tooltips ----

/// The keys of the labels of the buttons that show text.
const LABEL_KEYS: [&str; 22] = [
    "book-compare",
    "book-tools-title",
    "book-group-edit",
    "book-add-game-move",
    "book-add-line",
    "book-remove-line",
    "book-import-branch",
    "book-copy-from-other",
    "book-create-title",
    "book-create-make",
    "book-create-into-first",
    "book-create-into-second",
    "book-create-stop",
    "book-merge-title",
    "book-merge-button",
    "book-opening-title",
    "book-opening-clear",
    "book-use-this",
    "book-use-first",
    "book-use-second",
    "book-confirm-yes",
    "book-confirm-no",
];

fn message_of(language: &str, key: &str) -> String {
    let text = ftl_text(language, "book.ftl");
    let prefix = format!("{key} = ");
    text.lines()
        .find_map(|l| l.strip_prefix(prefix.as_str()))
        .unwrap_or_else(|| panic!("{key} in {language}"))
        .to_owned()
}

#[test]
fn the_labels_of_the_buttons_are_short_in_both_languages() {
    for language in ["en-US", "pt-BR"] {
        for key in LABEL_KEYS {
            let text = message_of(language, key);
            assert!(
                text.chars().count() <= 32,
                "{language} {key}: {text:?} is too long for a narrow panel"
            );
        }
    }
}

#[test]
fn the_panel_draws_at_the_narrowest_width_in_both_languages() {
    for language in [LanguagePref::EnUs, LanguagePref::PtBr] {
        let mut rig = rig_in(language);
        click_id(&mut rig, COMPARE_ID);
        dirty(&mut rig, BookSide::First);
        dirty(&mut rig, BookSide::Second);
        click_id(&mut rig, TOOLS_ID);
        for size in [Size::new(280.0, 560.0), Size::new(800.0, 560.0)] {
            let mut s = panel(&rig, size);
            let tools = if language == LanguagePref::EnUs {
                "Tools"
            } else {
                "Ferramentas"
            };
            assert!(s.find(tools).is_ok(), "{language:?} {size:?}");
        }
    }
}

#[test]
fn the_icon_only_buttons_have_a_tooltip_with_text_in_both_languages() {
    // `icon_button` cannot be built without a tooltip key (it is a required
    // argument): the tooltip of every icon button is made from these keys.
    for language in ["en-US", "pt-BR"] {
        for key in [
            "book-open",
            "book-new",
            "book-save",
            "book-save-as",
            "book-close-book",
            "book-compare-close-tip",
            "book-compare-tip",
            "book-dismiss",
            "book-remove-move-tip",
            "book-double-tip",
            "book-half-tip",
            "book-tools-show-tip",
            "book-tools-hide-tip",
        ] {
            assert!(
                !message_of(language, key).trim().is_empty(),
                "{language} {key}"
            );
        }
    }
}

#[test]
fn the_texts_of_the_panel_are_paired_and_pt_says_variante() {
    let en = ftl_text("en-US", "book.ftl");
    let pt = ftl_text("pt-BR", "book.ftl");
    let ids = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|l| !l.starts_with([' ', '#', '[', '*', '}']) && l.contains(" ="))
            .filter_map(|l| l.split(" =").next())
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(ids(&en), ids(&pt));
    assert!(!pt.to_lowercase().contains("variaç"));
}
