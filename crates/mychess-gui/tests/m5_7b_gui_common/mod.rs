//! Shared helpers of the `m5_7b_gui_*` contract tests (spec
//! `docs/specs/m5-7-eco-e-novidade.md`, M5-7b, GUI side), headless. Builds
//! on the tree rig (a shell whose tasks run to completion).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::cast_precision_loss,
    clippy::format_push_string
)]

#[path = "../m5_2_gui_common/mod.rs"]
pub mod base;

use std::fmt::Write as _;
use std::path::Path;

pub use base::{
    BasesMessage, GameMessage, Key, Message, Nav, TreeRig, UiMessage, ch, dbg, en, named, plain,
    pt, sent,
};
pub use mychess_app::{
    BrowserScope, ClassifyChoice, EcoMessage, EcoNotice, EcoWindow, LanguagePref, ListMessage,
    Message as AppMessage, SharedEco,
};
use mychess_core::EcoBook;

pub const BOOK: &str = "A00a \"Start\" *\n\
    B20 \"Sicilian Defence\" 1.e4 c5 *\n\
    C20 \"King's Pawn Game\" 1.e4 e5 *\n\
    C40 \"King's Knight Opening\" 1.e4 e5 2.Nf3 *\n\
    B02 \"Alekhine Variation\" 1.e4 Nf6 *\n";

pub fn book(text: &str) -> SharedEco {
    SharedEco::new(EcoBook::parse(text).expect("book"))
}

pub fn eco(message: EcoMessage) -> Message {
    Message::App(AppMessage::Eco(message))
}

/// The messages of the ECO browser and dialogs, as `Debug` text.
pub fn eco_dbg(message: EcoMessage) -> String {
    dbg(&eco(message))
}

/// One game of a synthetic PGN.
pub fn game_pgn(moves: &str, result: &str, date: &str, eco: Option<&str>) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "[Event \"T\"]\n[Site \"S\"]\n[Date \"{date}\"]\n[Round \"1\"]\n[White \"W\"]\n[Black \"B\"]\n[Result \"{result}\"]\n"
    );
    if let Some(eco) = eco {
        let _ = writeln!(out, "[ECO \"{eco}\"]");
    }
    let _ = write!(out, "\n{moves} {result}\n\n");
    out
}

/// The games of the ECO tests: B20 x4 (1-0 1-0 0-1 1/2), C40 x3 (1-0 1/2
/// 0-1), D10 x2 (1-0, unfinished) and one without a code.
pub fn eco_games() -> String {
    let mut pgn = String::new();
    for (result, code) in [
        ("1-0", "B20"),
        ("1-0", "B20"),
        ("0-1", "B20"),
        ("1/2-1/2", "B20"),
        ("1-0", "C40"),
        ("1/2-1/2", "C40"),
        ("0-1", "C40"),
        ("1-0", "D10"),
        ("*", "D10"),
    ] {
        pgn.push_str(&game_pgn("1. e4", result, "2020.01.01", Some(code)));
    }
    pgn.push_str(&game_pgn("1. d4", "1-0", "2020.01.01", None));
    pgn
}

/// A rig with the ECO games open as a PGN base and the standard book.
pub fn rig_with(language: LanguagePref, dir: &Path) -> TreeRig {
    let mut rig = TreeRig::with_language(language);
    rig.open_pgn(dir, "g.pgn", &eco_games());
    rig.rig.shell.app.eco_book = Some(book(BOOK));
    rig
}

pub fn open_browser(rig: &mut TreeRig) {
    rig.ui(UiMessage::OpenEcoBrowser);
}

pub fn window(rig: &TreeRig) -> Option<EcoWindow> {
    rig.app().eco_window()
}

use iced::{Settings, Size};
use iced_test::Simulator;
use mychess_gui::windows::{eco_browser, eco_dialogs};

pub const BOX_SIZE: Size = Size::new(1000.0, 1300.0);

/// Which box to draw alone.
#[derive(Clone, Copy)]
pub enum EcoBox {
    Browser,
    Classify,
    Novelty,
}

/// The box of the open ECO window alone (without the window behind it), so
/// texts that also exist elsewhere in the window are found only here.
pub fn box_sim(rig: &TreeRig, which: EcoBox) -> Simulator<'_, Message> {
    let app = rig.app();
    let i18n = &rig.rig.shell.i18n;
    let element = match which {
        EcoBox::Browser => eco_browser::view(app, i18n),
        EcoBox::Classify => eco_dialogs::classify(app, i18n),
        EcoBox::Novelty => eco_dialogs::novelty(app, i18n),
    }
    .expect("the box is open");
    Simulator::with_size(Settings::default(), BOX_SIZE, element)
}

pub fn box_shows(rig: &TreeRig, which: EcoBox, text: &str) -> bool {
    box_sim(rig, which).find(text).is_ok()
}

/// The messages a click on `text` inside the box sends.
pub fn box_click(rig: &TreeRig, which: EcoBox, text: &str) -> Vec<Message> {
    let mut sim = box_sim(rig, which);
    sim.click(text)
        .unwrap_or_else(|e| panic!("click {text}: {e:?}"));
    sim.into_messages().collect()
}

/// Clicks `text` in the box and runs what it starts.
pub fn box_press(rig: &mut TreeRig, which: EcoBox, text: &str) {
    for message in box_click(rig, which, text) {
        rig.msg(message);
    }
}

/// A key as the subscription reports it, with the focus in a text field.
pub fn key_in_field(rig: &mut TreeRig, input: mychess_app::KeyInput) {
    rig.msg(Message::Dialog(
        mychess_gui::windows::dialog_ui::DialogMessage::Key {
            input,
            text_focused: true,
            typed: None,
        },
    ));
}

/// The messages a click at `point` inside the box sends (radios and
/// checkboxes draw their own label, so they cannot be found by text).
pub fn box_click_at(rig: &TreeRig, which: EcoBox, point: iced::Point) -> Vec<Message> {
    let mut sim = box_sim(rig, which);
    sim.point_at(point);
    sim.simulate(iced_test::simulator::click());
    sim.into_messages().collect()
}

/// The centre of the radio `index` (0 is "only games with no code") of the
/// classification box: they sit in a column under "Games to classify" (the
/// English text of the heading is used only to find the column).
pub fn classify_radio(rig: &TreeRig, index: usize) -> iced::Point {
    let mut sim = box_sim(rig, EcoBox::Classify);
    let heading = sim.find("Games to classify").unwrap().bounds();
    iced::Point::new(heading.x + 8.0, heading.y + 33.0 + 24.2 * index as f32)
}

/// The classification choices in the order of the radios.
pub const CLASSIFY_RADIOS: [ClassifyChoice; 6] = [
    ClassifyChoice::NoCode,
    ClassifyChoice::All,
    ClassifyChoice::Filter,
    ClassifyChoice::LastYear,
    ClassifyChoice::LastMonth,
    ClassifyChoice::Since,
];

/// Clicks the radio of `choice` and runs what it sends.
pub fn pick_classify(rig: &mut TreeRig, choice: ClassifyChoice) {
    let index = CLASSIFY_RADIOS.iter().position(|c| *c == choice).unwrap();
    let point = classify_radio(rig, index);
    for message in box_click_at(rig, EcoBox::Classify, point) {
        rig.msg(message);
    }
}

/// The centre of the "only older games" checkbox of the novelty box (below
/// the row of databases, which is found by the name of the first one).
pub fn older_checkbox(rig: &TreeRig, first_base: &str) -> iced::Point {
    let mut sim = box_sim(rig, EcoBox::Novelty);
    let row = sim.find(first_base).unwrap().bounds();
    iced::Point::new(row.x + 8.0, row.y + 34.0)
}
