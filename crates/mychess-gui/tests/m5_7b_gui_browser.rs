//! M5-7b GUI contract: the ECO browser window (spec
//! `docs/specs/m5-7-eco-e-novidade.md`, D1700-D1709, D1721, D1722, D1725).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_push_string
)]

mod m5_7b_gui_common;

use m5_7b_gui_common::*;
use tempfile::tempdir;

#[test]
fn the_menu_command_opens_the_browser_with_the_counts_in_plain_view() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::EnUs, dir.path());
    assert_eq!(window(&rig), None);
    open_browser(&mut rig);
    assert_eq!(window(&rig), Some(EcoWindow::Browser));
    assert!(rig.shows("ECO browser"));
    assert!(rig.shows("All ECO sections"));
    // 9 coded games: +4 (B20 x2, C40, D10) =3 (B20, C40, unfinished) -2.
    assert!(rig.shows("9 games: +4 =3 -2 (61.1%)"), "summary line");
    assert!(rig.shows("Database g.pgn (9 of 10 games classified)"));
    for letter in ["A", "B", "C", "D", "E"] {
        assert!(rig.shows(letter), "bar {letter}");
    }
}

/// A point inside the chart, in the column of the bar under `label` (the
/// label is laid out at the foot of the bar, so a point above it is inside
/// the bar).
fn bar_point(rig: &TreeRig, label: &str) -> iced::Point {
    let mut sim = box_sim(rig, EcoBox::Browser);
    let at = sim.find(label).unwrap().bounds();
    iced::Point::new(at.x, at.y - 40.0)
}

#[test]
fn clicking_a_bar_goes_down_and_up_goes_back() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::EnUs, dir.path());
    open_browser(&mut rig);
    let point = bar_point(&rig, "B");
    let messages = {
        let mut sim = box_sim(&rig, EcoBox::Browser);
        sim.point_at(point);
        sim.simulate(iced_test::simulator::click());
        sim.into_messages().collect::<Vec<_>>()
    };
    assert!(
        sent(&messages, &eco(EcoMessage::BrowseTo("B".to_owned()))),
        "{messages:?}"
    );
    for message in messages {
        rig.msg(message);
    }
    assert_eq!(rig.app().eco_browser_view().unwrap().prefix, "B");
    assert!(rig.shows("ECO section B"));
    assert!(rig.shows("4 games: +2 =1 -1 (62.5%)"));
    box_press(&mut rig, EcoBox::Browser, "Up");
    assert_eq!(rig.app().eco_browser_view().unwrap().prefix, "");
    assert!(rig.shows("All ECO sections"));
}

#[test]
fn the_labels_and_counts_under_the_bars_have_a_visible_size() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::EnUs, dir.path());
    open_browser(&mut rig);
    let mut sim = box_sim(&rig, EcoBox::Browser);
    for label in ["A", "B", "C", "D", "E"] {
        let bounds = sim.find(label).unwrap().bounds();
        assert!(
            bounds.width > 0.0 && bounds.height > 0.0,
            "label {label} is laid out with an empty box: {bounds:?}"
        );
    }
}

#[test]
fn up_is_disabled_at_the_root() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::EnUs, dir.path());
    open_browser(&mut rig);
    assert!(box_click(&rig, EcoBox::Browser, "Up").is_empty());
}

#[test]
fn below_a_subcode_there_are_no_bars_and_the_page_says_so() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::EnUs, dir.path());
    open_browser(&mut rig);
    rig.app_msg(AppMessage::Eco(EcoMessage::BrowseTo("B90a".to_owned())));
    assert!(rig.shows("ECO code B90a"));
    assert!(rig.shows("Nothing below this code: the openings of the ECO file are listed."));
}

#[test]
fn the_list_shows_the_openings_and_clicking_one_imports_its_line() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::EnUs, dir.path());
    open_browser(&mut rig);
    rig.app_msg(AppMessage::Eco(EcoMessage::BrowseTo("C40".to_owned())));
    assert!(rig.shows("King's Knight Opening"));
    assert!(rig.shows("1.e4 e5 2.Nf3"));
    let messages = box_click(&rig, EcoBox::Browser, "King's Knight Opening");
    assert!(
        sent(&messages, &eco(EcoMessage::ImportLine(0))),
        "{messages:?}"
    );
    box_press(&mut rig, EcoBox::Browser, "King's Knight Opening");
    assert_eq!(window(&rig), None, "the browser closes");
    assert_eq!(rig.app().session.game().mainline().count(), 3);
}

#[test]
fn typing_in_the_field_and_enter_sends_the_search() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::EnUs, dir.path());
    open_browser(&mut rig);
    let messages = {
        let mut sim = box_sim(&rig, EcoBox::Browser);
        sim.click("Find an opening by name or code").unwrap();
        sim.typewrite("SICILIAN");
        sim.tap_key(iced::keyboard::key::Named::Enter);
        sim.into_messages().collect::<Vec<_>>()
    };
    assert!(
        sent(&messages, &eco(EcoMessage::SetFind("SICILIAN".to_owned()))),
        "{messages:?}"
    );
    assert!(
        sent(&messages, &eco(EcoMessage::SubmitFind)),
        "{messages:?}"
    );
    for message in messages {
        rig.msg(message);
    }
    let view = rig.app().eco_browser_view().unwrap();
    assert_eq!(view.finding, Some("SICILIAN"));
    assert_eq!(view.entries.len(), 1);
    assert!(rig.shows("Openings that contain \"SICILIAN\" (click one to import its line)"));
}

#[test]
fn enter_outside_the_field_and_the_button_also_search() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::EnUs, dir.path());
    open_browser(&mut rig);
    rig.app_msg(AppMessage::Eco(EcoMessage::SetFind("king".to_owned())));
    let messages = box_click(&rig, EcoBox::Browser, "Find");
    assert!(
        sent(&messages, &eco(EcoMessage::SubmitFind)),
        "{messages:?}"
    );
    rig.key(named(mychess_app::NamedKey::Enter));
    let view = rig.app().eco_browser_view().unwrap();
    assert_eq!(view.finding, Some("king"));
    assert_eq!(view.entries.len(), 2);
}

#[test]
fn escape_closes_the_browser_and_the_game_is_not_edited_under_it() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::EnUs, dir.path());
    rig.nav(Nav::Start);
    open_browser(&mut rig);
    assert!(rig.app().modal_open());
    // A game key does nothing while the box is open.
    let before = rig.app().session.game().node_count();
    rig.key(ch('e'));
    assert_eq!(rig.app().session.game().node_count(), before);
    rig.key(named(mychess_app::NamedKey::Escape));
    assert_eq!(window(&rig), None);
    assert!(!rig.app().modal_open());
}

#[test]
fn without_a_book_the_page_says_so_and_offers_to_load_one() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::EnUs, dir.path());
    rig.rig.shell.app.eco_book = None;
    open_browser(&mut rig);
    assert!(rig.shows("No ECO file is loaded."));
    assert!(rig.shows("Load ECO file..."));
    // The statistics still come from the index.
    assert!(rig.shows("Database g.pgn (9 of 10 games classified)"));
    let messages = box_click(&rig, EcoBox::Browser, "Load ECO file...");
    assert!(
        sent(&messages, &eco(EcoMessage::LoadBookFile)),
        "{messages:?}"
    );
}

#[test]
fn the_scope_buttons_switch_and_the_filter_one_waits_for_a_filter() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::EnUs, dir.path());
    open_browser(&mut rig);
    assert!(rig.shows("All games") && rig.shows("Only the filter"));
    assert!(
        box_click(&rig, EcoBox::Browser, "Only the filter").is_empty(),
        "no game is excluded: the button is off"
    );
    rig.app_msg(AppMessage::Eco(EcoMessage::CloseBrowser));
    let slot = rig.app().bases.current();
    rig.app_msg(BasesMessage::List(ListMessage::SelectNumbers {
        slot,
        numbers: vec![1, 2, 3, 4],
    }));
    rig.app_msg(BasesMessage::List(ListMessage::KeepSelected { slot }));
    open_browser(&mut rig);
    let messages = box_click(&rig, EcoBox::Browser, "Only the filter");
    assert!(
        sent(&messages, &eco(EcoMessage::SetScope(BrowserScope::Filter))),
        "{messages:?}"
    );
    box_press(&mut rig, EcoBox::Browser, "Only the filter");
    assert_eq!(rig.app().eco_browser_view().unwrap().stats.count, 4);
    assert!(rig.shows("4 games: +2 =1 -1 (62.5%)"));
}

#[test]
fn a_cut_list_tells_the_user_how_many_openings_are_shown() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::EnUs, dir.path());
    rig.rig.shell.app.eco_book = Some(big_book());
    open_browser(&mut rig);
    rig.app_msg(AppMessage::Eco(EcoMessage::BrowseTo("B90".to_owned())));
    assert!(rig.app().eco_browser_view().unwrap().truncated);
    assert!(rig.shows("Only the first 500 openings are shown."));
}

fn big_book() -> SharedEco {
    let white = [
        "a3", "a4", "b3", "b4", "c3", "c4", "d3", "d4", "e3", "e4", "f3", "f4", "g3", "g4", "h3",
        "h4", "Na3", "Nc3", "Nf3", "Nh3",
    ];
    let black = [
        "a6", "a5", "b6", "b5", "c6", "c5", "d6", "d5", "e6", "e5", "f6", "f5", "g6", "g5", "h6",
        "h5", "Na6", "Nc6", "Nf6", "Nh6",
    ];
    let mut text = String::new();
    let mut n = 0;
    for w in white {
        for b in black {
            n += 1;
            text.push_str(&format!("B90a \"Line {n}\" 1.{w} {b} *\n"));
            for t in ["a3", "b3", "c3", "d3", "e3", "f3", "g3", "h3", "Nf3", "Nc3"] {
                n += 1;
                text.push_str(&format!("B90a \"Line {n}\" 1.{w} {b} 2.{t} *\n"));
            }
        }
    }
    let report = EcoBook::parse_with(&text, mychess_core::EcoParseOptions::skip_entry()).unwrap();
    assert!(report.book.len() > 500);
    SharedEco::new(report.book)
}
use mychess_core::EcoBook;

// ---- Portuguese ----

#[test]
fn in_portuguese_the_texts_and_the_opening_names_are_translated() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::PtBr, dir.path());
    open_browser(&mut rig);
    assert!(rig.shows("Navegador ECO"));
    assert!(rig.shows("Todas as seções do ECO"));
    assert!(rig.shows("Subir"));
    rig.app_msg(AppMessage::Eco(EcoMessage::BrowseTo("B".to_owned())));
    assert!(rig.shows("Seção do ECO B"));
    assert!(rig.shows("Defesa Siciliana"), "translated by the glossary");
    assert!(!rig.shows("Sicilian Defence"));
    // The summary uses the decimal comma of the language.
    rig.app_msg(AppMessage::Eco(EcoMessage::BrowseTo(String::new())));
    assert!(rig.shows("9 partidas: +4 =3 -2 (61,1%)"));
}

#[test]
fn in_portuguese_a_variation_is_a_variante_everywhere_on_screen() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::PtBr, dir.path());
    open_browser(&mut rig);
    rig.app_msg(AppMessage::Eco(EcoMessage::BrowseTo("B".to_owned())));
    assert!(
        rig.shows("Alekhine Variante"),
        "{}",
        "the glossary maps Variation"
    );
    assert!(!rig.shows("Alekhine Variation"));
    assert!(!rig.shows("Alekhine Variação"));
}

#[test]
fn in_portuguese_the_search_finds_the_translated_name_ignoring_case() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::PtBr, dir.path());
    open_browser(&mut rig);
    rig.app_msg(AppMessage::Eco(EcoMessage::SetFind(
        "defesa SICILIANA".to_owned(),
    )));
    rig.key(named(mychess_app::NamedKey::Enter));
    let view = rig.app().eco_browser_view().unwrap();
    assert_eq!(view.entries.len(), 1);
    assert_eq!(view.entries[0].code, "B20");
    assert_eq!(view.entries[0].name, "Defesa Siciliana");
}

#[test]
fn english_shows_the_names_as_written_in_the_file() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::EnUs, dir.path());
    open_browser(&mut rig);
    rig.app_msg(AppMessage::Eco(EcoMessage::BrowseTo("B".to_owned())));
    assert!(rig.shows("Sicilian Defence"));
    assert!(rig.shows("Alekhine Variation"));
}
