//! M5-6c review fixes (GUI, D2049-D2059): no button of the panel is cut in a
//! narrow panel (measured with the simulator), the weight edits stay
//! reachable, the drafts of the weight fields do not depend on SAN strings.
//! Spec `docs/specs/m5-6-janela-do-livro.md`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m5_2_gui_common;

use iced::advanced::widget::Id;
use iced::{Settings, Size};
use iced_test::Simulator;
use m5_2_gui_common::*;
use mychess_app::{BookMessage, BookSide, LanguagePref};
use mychess_core::Game;
use mychess_gui::windows::book_panel::{
    BookUiMessage, COMPARE_ID, MERGE_ID, STOP_ID, TOOLS_ID, side_id,
};

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
    for side in BookSide::ALL {
        rig.app_msg(book(BookMessage::New { side }));
        rig.app_msg(book(BookMessage::AddGameMove { side }));
    }
    rig.msg(Message::Book(BookUiMessage::ToggleTools));
    rig
}

fn panel(rig: &TreeRig, size: Size) -> Simulator<'_, Message> {
    Simulator::with_size(
        Settings::default(),
        size,
        mychess_gui::windows::book_panel::view(&rig.rig.shell),
    )
}

/// Every id of the panel with both books and the tools open.
fn all_ids() -> Vec<String> {
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
    ids
}

#[test]
fn no_button_is_cut_in_a_narrow_panel_in_both_languages() {
    for language in [LanguagePref::EnUs, LanguagePref::PtBr] {
        let rig = rig_in(language);
        for width in [280.0_f32, 240.0] {
            let size = Size::new(width, 1400.0);
            let mut s = panel(&rig, size);
            for id in all_ids() {
                let bounds = s
                    .find(Id::from(id.clone()))
                    .unwrap_or_else(|e| panic!("{id}: {e:?}"))
                    .bounds();
                assert!(
                    bounds.x >= 0.0 && bounds.x + bounds.width <= width + 0.5,
                    "{id} at {width} ({language:?}): {bounds:?}"
                );
            }
        }
    }
}

#[test]
fn the_weight_buttons_are_a_second_line_under_the_move() {
    let rig = rig_in(LanguagePref::EnUs);
    let mut s = panel(&rig, Size::new(420.0, 900.0));
    let double = s
        .find(Id::from(side_id(BookSide::First, "double-e4")))
        .expect("x2")
        .bounds();
    let half = s
        .find(Id::from(side_id(BookSide::First, "half-e4")))
        .expect("/2")
        .bounds();
    let remove = s
        .find(Id::from(side_id(BookSide::First, "remove-e4")))
        .expect("remove")
        .bounds();
    assert!(half.x > double.x && remove.x > half.x, "left to right");
    assert!((double.y - remove.y).abs() < 6.0, "one line");
    let e4 = s.find("e4").expect("the move").bounds();
    assert!(double.y > e4.y, "under the first line of the row");
}

#[test]
fn the_tools_header_does_nothing_while_a_book_is_being_made() {
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
    let mut s = panel(&rig, SIZE);
    s.click(Id::from(TOOLS_ID.to_owned())).expect("header");
    assert!(
        s.into_messages().next().is_none(),
        "the header is disabled while creating"
    );
}

#[test]
fn typing_in_a_weight_field_keeps_the_text_per_move() {
    let mut rig = rig_in(LanguagePref::EnUs);
    let e4 = mychess_core::Position::new().parse_uci("e2e4").unwrap();
    rig.msg(Message::Book(BookUiMessage::WeightInput {
        side: BookSide::First,
        mv: e4,
        text: "37".to_owned(),
    }));
    assert!(panel(&rig, SIZE).find("37").is_ok());
    // The other book keeps its own weight.
    let second = side_id(BookSide::Second, "double-e4");
    assert!(panel(&rig, SIZE).find(Id::from(second)).is_ok());
}
