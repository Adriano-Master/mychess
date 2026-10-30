//! Multiple selection, tabs and the sort arrow of the game list (spec M3-4,
//! slice 4c: D557, rule 5, criteria on selection and sort), headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

mod m3_4c_common;
mod render_common;

use std::hash::{Hash, Hasher};

use iced::{Rectangle, Size, widget};
use m3_4c_common::*;
use mychess_app::BasesMessage;
use mychess_gui::Message;
use mychess_gui::windows::game_list::{Column, GameListMessage};
use render_common::{SCALE, render};

fn selection(rig: &Rig) -> Vec<u32> {
    current(rig).list.selection.iter().copied().collect()
}

fn modifiers(rig: &mut Rig, command: bool, shift: bool) {
    rig.msg(Message::Games(GameListMessage::Modifiers {
        command,
        shift,
    }));
}

fn click(rig: &mut Rig, number: u32) {
    list(
        rig,
        GameListMessage::Click {
            slot: slot(1),
            number,
        },
    );
}

#[test]
fn the_modifiers_held_decide_how_a_click_selects() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 30);
    click(&mut rig, 3);
    modifiers(&mut rig, true, false);
    click(&mut rig, 6);
    click(&mut rig, 8);
    assert_eq!(selection(&rig), [3, 6, 8], "Ctrl/Cmd toggles");
    click(&mut rig, 6);
    assert_eq!(selection(&rig), [3, 8]);
    modifiers(&mut rig, false, true);
    click(&mut rig, 5);
    // The anchor is the last toggled game (8): range 5..=8.
    assert_eq!(selection(&rig), [5, 6, 7, 8]);
    modifiers(&mut rig, false, false);
    click(&mut rig, 12);
    assert_eq!(selection(&rig), [12], "a plain click starts again");
}

#[test]
fn a_range_covers_the_view_order_after_a_sort() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 12);
    list(&mut rig, GameListMessage::Header(Column::White));
    // View: White 1, 10, 11, 12, 2, 3, ...
    click(&mut rig, 1);
    modifiers(&mut rig, false, true);
    click(&mut rig, 12);
    assert_eq!(selection(&rig), [1, 10, 11, 12]);
}

#[test]
fn each_tab_keeps_its_own_selection_and_filter() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 30);
    open_more(&mut rig, dir.path(), "b.pgn", 30);
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(2),
            number: 9,
        },
    );
    rig.send(BasesMessage::Select(slot(1)));
    click(&mut rig, 4);
    list(
        &mut rig,
        GameListMessage::Act(mychess_gui::windows::game_list::ListAction::RemoveSelected),
    );
    rig.send(BasesMessage::Select(slot(2)));
    assert_eq!(selection(&rig), [9]);
    assert_eq!(current(&rig).filtered(), 30, "slot 2 was not filtered");
    rig.send(BasesMessage::Select(slot(1)));
    assert_eq!(current(&rig).filtered(), 29);
    assert!(
        selection(&rig).is_empty(),
        "the removed game left the selection"
    );
}

#[test]
fn every_selected_row_is_painted_as_selected() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 12);
    click(&mut rig, 2);
    modifiers(&mut rig, true, false);
    click(&mut rig, 3);
    click(&mut rig, 6);
    modifiers(&mut rig, false, false);
    let size = Size::new(1280.0, 800.0);
    let shot = render(rig.shell.view(), size);
    let mut sim = rig.sim(size);
    let mut probe = |n: u32| {
        let b = sim
            .find(widget::Id::from(format!("games-row-{n}")))
            .expect("row")
            .bounds();
        shot.pixel(
            ((b.x + 2.0) * SCALE) as u32,
            ((b.y + b.height / 2.0) * SCALE) as u32,
        )
    };
    let selected = [probe(2), probe(3), probe(6)];
    assert_eq!(selected[0], selected[1]);
    assert_eq!(selected[1], selected[2]);
    for plain in [1, 4, 5, 7, 8] {
        assert_ne!(probe(plain), selected[0], "row {plain} is not selected");
    }
}

fn fingerprint(rig: &Rig, size: Size, id: &'static str) -> u64 {
    let shot = render(rig.shell.view(), size);
    let mut sim = rig.sim(size);
    let b: Rectangle = sim.find(widget::Id::new(id)).expect("the header").bounds();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for y in (b.y * SCALE) as u32..((b.y + b.height) * SCALE) as u32 {
        for x in (b.x * SCALE) as u32..((b.x + b.width) * SCALE) as u32 {
            shot.pixel(x, y).hash(&mut hasher);
        }
    }
    hasher.finish()
}

#[test]
fn the_header_shows_an_arrow_for_the_sorted_column_that_flips_with_the_direction() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 12);
    let size = Size::new(1280.0, 800.0);
    let unsorted = fingerprint(&rig, size, "games-header");
    list(&mut rig, GameListMessage::Header(Column::White));
    let ascending = fingerprint(&rig, size, "games-header");
    list(&mut rig, GameListMessage::Header(Column::White));
    let descending = fingerprint(&rig, size, "games-header");
    assert_ne!(unsorted, ascending, "an arrow appears");
    assert_ne!(ascending, descending, "the arrow turns");
    assert_ne!(unsorted, descending);
}

#[test]
fn sorting_a_tab_does_not_draw_an_arrow_in_another_tab() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 12);
    open_more(&mut rig, dir.path(), "b.pgn", 12);
    let size = Size::new(1280.0, 800.0);
    let plain = fingerprint(&rig, size, "games-header");
    rig.send(BasesMessage::Select(slot(1)));
    list(&mut rig, GameListMessage::Header(Column::White));
    rig.send(BasesMessage::Select(slot(2)));
    assert_eq!(fingerprint(&rig, size, "games-header"), plain);
}
