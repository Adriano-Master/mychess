//! M6-4d review fixes (GUI): the keys of a focused field of a panel are
//! text, the crosstable builds only the rows near the viewport, the typed
//! texts are forgotten when the field loses the focus and the decimals
//! follow the language.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::format_push_string
)]

mod m6_4d_common;

use m6_4d_common::*;
use mychess_app::{Key, KeyInput, Modifiers, NamedKey};
use mychess_gui::windows::crosstable_panel::{HEADER_H, ROW_H, row_window, slot_height};
use mychess_gui::windows::data_panels::{
    AXIS_MAX_ID, DataPanelsUi, RATING_PLAYER_ID, TRACKER_START_ID,
};
use mychess_gui::windows::dialog_ui::DialogMessage;

fn press(rig: &mut Rig, key: NamedKey) {
    rig.ui(Message::Dialog(DialogMessage::Key {
        input: KeyInput {
            key: Key::Named(key),
            modifiers: Modifiers::NONE,
        },
        text_focused: false,
        typed: None,
    }));
}

fn at_ply_two() -> Rig {
    let mut rig = Rig::new();
    let slot = rig.open_games(&GAMES);
    rig.load(slot, 1);
    rig.app(mychess_app::GameMessage::Navigate(
        mychess_app::Nav::Forward(2),
    ));
    rig
}

fn cursor(rig: &Rig) -> String {
    format!("{:?}", rig.shell.app.session.cursor())
}

#[test]
fn up_down_pages_and_enter_are_text_while_a_panel_field_has_the_focus() {
    let mut rig = at_ply_two();
    rig.toggle(PaneKind::RatingGraph);
    // Control: without the focus, Up moves along the game.
    let start = cursor(&rig);
    press(&mut rig, NamedKey::Up);
    assert_ne!(
        cursor(&rig),
        start,
        "Up moves the game when no field is focused"
    );
    let mut rig = at_ply_two();
    rig.toggle(PaneKind::RatingGraph);
    rig.ui(Message::Panels(DataPanelsMessage::FieldFocus(
        RATING_PLAYER_ID,
        true,
    )));
    assert_eq!(rig.shell.ui.dialogs.panels.focused, Some(RATING_PLAYER_ID));
    let start = cursor(&rig);
    for key in [
        NamedKey::Up,
        NamedKey::Down,
        NamedKey::PageUp,
        NamedKey::PageDown,
        NamedKey::Enter,
    ] {
        press(&mut rig, key);
        assert_eq!(cursor(&rig), start, "{key:?}");
        assert!(
            rig.shell.app.session.move_entry().text().is_empty(),
            "{key:?}"
        );
    }
    // Losing the focus gives the keys back.
    rig.ui(Message::Panels(DataPanelsMessage::FieldFocus(
        RATING_PLAYER_ID,
        false,
    )));
    assert_eq!(rig.shell.ui.dialogs.panels.focused, None);
    press(&mut rig, NamedKey::Up);
    assert_ne!(cursor(&rig), start);
}

#[test]
fn a_stale_focus_of_a_hidden_panel_does_not_swallow_the_keys() {
    let mut rig = at_ply_two();
    rig.ui(Message::Panels(DataPanelsMessage::FieldFocus(
        RATING_PLAYER_ID,
        true,
    )));
    let start = cursor(&rig);
    press(&mut rig, NamedKey::Up);
    assert_ne!(cursor(&rig), start, "no panel is visible");
}

#[test]
fn the_typed_texts_are_forgotten_when_the_field_loses_the_focus() {
    let mut rig = Rig::new();
    rig.ui(Message::Panels(DataPanelsMessage::RowColorEdit(
        "#12".into(),
    )));
    assert_eq!(
        rig.shell.ui.dialogs.panels.cross_color_text.as_deref(),
        Some("#12")
    );
    rig.ui(Message::Panels(DataPanelsMessage::FieldFocus(
        mychess_gui::windows::data_panels::CROSS_COLOR_ID,
        false,
    )));
    assert_eq!(rig.shell.ui.dialogs.panels.cross_color_text, None);
    rig.ui(Message::Panels(DataPanelsMessage::TrackerEdit(
        mychess_gui::windows::data_panels::TrackerField::Start,
        "12".into(),
    )));
    assert_eq!(rig.shell.ui.dialogs.panels.focused, Some(TRACKER_START_ID));
    assert!(rig.shell.ui.dialogs.panels.tracker_text[0].is_some());
    rig.ui(Message::Panels(DataPanelsMessage::FieldFocus(
        TRACKER_START_ID,
        false,
    )));
    assert_eq!(rig.shell.ui.dialogs.panels.tracker_text, [None, None]);
}

#[test]
fn only_the_focused_field_keeps_its_text() {
    let mut ui = DataPanelsUi {
        focused: Some(AXIS_MAX_ID),
        ..DataPanelsUi::default()
    };
    ui.axis_text[1] = [Some("1".into()), Some("2".into()), Some("3".into())];
    ui.drop_unfocused_text();
    assert_eq!(ui.axis_text[1], [None, Some("2".into()), None]);
}

#[test]
fn a_second_export_click_is_ignored_while_the_save_dialog_is_open() {
    use mychess_app::crosstable::ExportFormat;
    let mut rig = Rig::new();
    let format = ExportFormat::Text;
    rig.ui(Message::Panels(DataPanelsMessage::PickExport(format)));
    assert!(rig.shell.ui.dialogs.panels.export_pending);
    rig.ui(Message::Panels(DataPanelsMessage::PickExport(format)));
    rig.ui(Message::Panels(DataPanelsMessage::ExportPicked(
        format, None,
    )));
    assert!(!rig.shell.ui.dialogs.panels.export_pending);
}

#[test]
fn the_crosstable_window_builds_a_bounded_number_of_rows() {
    let heights = vec![slot_height(false); 100_000];
    let (first, count, before, after) = row_window(&heights, 0.0, 800.0);
    assert_eq!(first, 0);
    assert!(count < 80, "{count}");
    assert!(before.abs() < f32::EPSILON);
    let total: f32 = heights.iter().sum();
    assert!((before + count as f32 * slot_height(false) + after - total).abs() < 1.0);
    // Scrolled far down, only the rows around the viewport are built and
    // the spacers add up to the whole.
    let scroll = 50_000.0 * slot_height(false) + HEADER_H;
    let (first, count, before, after) = row_window(&heights, scroll, 800.0);
    assert!((49_900..50_000).contains(&first), "{first}");
    assert!(count < 80);
    let built: f32 = heights[first..first + count].iter().sum();
    assert!((before + built + after - total).abs() < 1.0);
    // Past the end it clamps to the last rows.
    let (first, count, _, after) = row_window(&heights, f32::MAX, 800.0);
    assert!(first + count == 100_000 && after.abs() < 1.0);
    // Group breaks are taller; empty and NaN are safe.
    assert!(slot_height(true) > slot_height(false) && ROW_H > 0.0);
    assert_eq!(row_window(&[], f32::NAN, 800.0).1, 0);
}

#[test]
fn a_big_tournament_renders_with_a_bounded_widget_tree() {
    // 120 players in a round robin would be 14 400 cells; the panel only
    // draws the rows near the viewport.
    let mut games = Vec::new();
    let names: Vec<String> = (0..60).map(|i| format!("P{i:02}")).collect();
    let mut text = String::new();
    for (i, white) in names.iter().enumerate() {
        for black in names.iter().skip(i + 1) {
            text.push_str(&format!(
                "[Event \"Big\"]\n[Site \"S\"]\n[Date \"2020.01.01\"]\n[Round \"1\"]\n[White \"{white}\"]\n[Black \"{black}\"]\n[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n"
            ));
        }
    }
    games.push(text);
    let mut rig = Rig::new();
    let path = rig.dir.path().join("big.pgn");
    std::fs::write(&path, &games[0]).unwrap();
    rig.app(BasesMessage::Open(path));
    let slot = rig.shell.app.bases.current();
    rig.load(slot, 1);
    rig.toggle(PaneKind::Crosstable);
    rig.app(CrosstableMessage::Refresh);
    let mut sim = rig.sim(1400.0, 900.0);
    // The first rows are there; the last one is not built.
    assert!(sim.find("P00").is_ok());
    assert!(sim.find("P59").is_err(), "the far rows are not widgets");
    // Scrolling moves the window.
    drop(sim);
    rig.ui(Message::Panels(DataPanelsMessage::CrossScrolled {
        offset: 1.0e9,
        height: 600.0,
    }));
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find("P59").is_ok());
}
