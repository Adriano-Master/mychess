//! Dragging a tab (or rows of the list) onto another tab of the strip of
//! databases copies games (spec M3-4, "Seletor de bancos", rule 8, D576,
//! D580, D584, D585; Scid `switcher.tcl` `pressMouseEvent` 840 and
//! `releaseMouseEvent` 847), headless with simulated mouse events.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

mod m3_4d_common;
mod render_common;

use std::hash::{Hash, Hasher};

use m3_4d_common::*;
use mychess_app::BasesNotice;
use mychess_gui::Message;
use mychess_gui::windows::game_list::{DragPayload, GameListMessage, TabDrag};
use mychess_gui::windows::layout::PaneKind;
use render_common::{SCALE, render};

fn drag(rig: &Rig) -> Option<TabDrag> {
    rig.shell.ui.games.drag
}

fn visible(rig: &Rig) -> bool {
    drag(rig).is_some_and(|d| d.is_dragging())
}

/// A hash of the pixels of the tab of `slot`.
fn tab_look(rig: &Rig, slot: SlotId) -> u64 {
    let shot = render(rig.shell.view(), SIZE);
    let b = bounds(rig, &format!("games-tab-{}", slot.number()));
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for y in (b.y * SCALE) as u32..((b.y + b.height) * SCALE) as u32 {
        for x in (b.x * SCALE) as u32..((b.x + b.width) * SCALE) as u32 {
            shot.pixel(x, y).hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn hint_visible(rig: &Rig, text: &str) -> bool {
    rig.sim(SIZE).find(text).is_ok()
}

// ---- tab to tab ----

#[test]
fn pressing_a_tab_starts_a_gesture_that_is_not_yet_visible_and_does_not_select() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    let before = rig.shell.app.bases.current();
    let at = tab_point(&rig, slot(2));
    let out = pressed(&rig, at, iced::mouse::Button::Left);
    assert!(
        out.iter()
            .any(|m| matches!(m, Message::Games(GameListMessage::TabPress(s)) if *s == slot(2)))
    );
    let sent = apply(&mut rig, out);
    assert!(
        !selected_slot(&sent, slot(2)),
        "selection waits for the release"
    );
    assert_eq!(rig.shell.app.bases.current(), before);
    let d = drag(&rig).expect("a gesture");
    assert_eq!((d.from, d.payload), (slot(2), DragPayload::Filter));
    assert!(!d.is_dragging());
}

#[test]
fn a_plain_click_on_a_tab_selects_it_and_copies_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    let at = Spot::Tab(slot(2));
    let mut sent = press(&mut rig, at);
    sent.extend(release(&mut rig, at));
    assert!(selected_slot(&sent, slot(2)), "{sent:?}");
    assert!(!any_copy(&sent));
    assert_eq!(rig.shell.app.bases.current(), slot(2));
    assert!(drag(&rig).is_none());
}

#[test]
fn dragging_a_tab_over_another_highlights_it_and_shows_the_hint() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    let (a, b) = (Spot::Tab(slot(1)), Spot::Tab(slot(2)));
    let idle_a = tab_look(&rig, slot(1));
    let idle_b = tab_look(&rig, slot(2));
    let idle_c = tab_look(&rig, slot(3));
    let hint = "Drop to copy 12 games from a.pgn to b.pgn";
    press(&mut rig, a);
    assert!(!hint_visible(&rig, hint), "no hint before moving");
    hover(&mut rig, b);
    assert!(visible(&rig));
    assert_eq!(drag(&rig).unwrap().over, Some(slot(2)));
    assert!(hint_visible(&rig, hint));
    assert_ne!(tab_look(&rig, slot(2)), idle_b, "the target is framed");
    assert_ne!(tab_look(&rig, slot(1)), idle_a, "the source is marked");
    assert_eq!(tab_look(&rig, slot(3)), idle_c, "the others do not change");
}

#[test]
fn the_hint_counts_the_filter_and_says_one_game_for_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    // Rows dragged: one selected game.
    let row = Spot::Row(3);
    press(&mut rig, row);
    hover(&mut rig, Spot::Tab(slot(2)));
    assert!(hint_visible(
        &rig,
        "Drop to copy one game from a.pgn to b.pgn"
    ));
    escape(&mut rig);
    assert!(!hint_visible(
        &rig,
        "Drop to copy one game from a.pgn to b.pgn"
    ));
}

#[test]
fn the_hint_is_translated() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    rig.shell.i18n = mychess_gui::i18n::I18n::new(mychess_gui::i18n::Language::PtBr);
    press(&mut rig, Spot::Tab(slot(1)));
    hover(&mut rig, Spot::Tab(slot(2)));
    assert!(hint_visible(
        &rig,
        "Solte para copiar 12 partidas de a.pgn para b.pgn"
    ));
}

#[test]
fn dropping_a_tab_on_another_sends_the_copy_of_the_filter() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    press(&mut rig, Spot::Tab(slot(1)));
    hover(&mut rig, Spot::Tab(slot(2)));
    let sent = release(&mut rig, Spot::Tab(slot(2)));
    assert!(
        has_copy(&sent, slot(1), slot(2), CopyScope::Filter),
        "{sent:?}"
    );
    assert!(!selected_slot(&sent, slot(2)));
    assert!(drag(&rig).is_none());
    assert_eq!(
        rig.shell.app.bases.current(),
        slot(1),
        "the current base stays"
    );
}

#[test]
fn dropping_on_a_read_only_pgn_reports_the_refusal_naming_both_databases() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    press(&mut rig, Spot::Tab(slot(1)));
    hover(&mut rig, Spot::Tab(slot(2)));
    release(&mut rig, Spot::Tab(slot(2)));
    match notice(&rig) {
        Some(mychess_app::AppNotice::Bases(BasesNotice::CopyRefused { reason, from, to })) => {
            assert_eq!(reason, mychess_app::CopyRefusal::TargetReadOnly);
            assert_eq!((from.name.as_str(), to.name.as_str()), ("a.pgn", "b.pgn"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn dropping_on_the_clipbase_copies_without_asking_even_with_the_question_on() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    assert!(rig.shell.app.prefs.editing.confirm_copy, "the default asks");
    press(&mut rig, Spot::Tab(slot(2)));
    hover(&mut rig, Spot::Tab(CLIP));
    let sent = release(&mut rig, Spot::Tab(CLIP));
    assert!(
        has_copy(&sent, slot(2), CLIP, CopyScope::Filter),
        "{sent:?}"
    );
    assert!(rig.shell.app.copy_prompt.is_none());
    let clip = rig.shell.app.bases.slot(CLIP).unwrap();
    assert_eq!((clip.total(), clip.filtered()), (8, 8));
    let whites: Vec<String> = rig
        .shell
        .app
        .bases
        .list_page(CLIP, 1, 100)
        .iter()
        .map(|r| r.white.clone())
        .collect();
    assert_eq!(whites.first().map(String::as_str), Some("Other 1"));
    assert_eq!(whites.last().map(String::as_str), Some("Other 8"));
    assert_eq!(
        rig.shell.app.bases.current(),
        slot(1),
        "the current base stays"
    );
}

#[test]
fn releasing_on_the_tab_it_came_from_after_wandering_only_selects_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    let (b, c) = (Spot::Tab(slot(2)), Spot::Tab(slot(3)));
    press(&mut rig, b);
    hover(&mut rig, c);
    assert!(visible(&rig));
    hover(&mut rig, b);
    assert!(!visible(&rig), "back over its own tab: nothing to drop");
    assert!(!hint_visible(
        &rig,
        "Drop to copy 8 games from b.pgn to c.pgn"
    ));
    let sent = release(&mut rig, b);
    assert!(!any_copy(&sent));
    assert!(selected_slot(&sent, slot(2)));
    assert_eq!(rig.shell.app.bases.current(), slot(2));
}

#[test]
fn escape_cancels_a_visible_drag_and_the_release_copies_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    let b = Spot::Tab(slot(2));
    press(&mut rig, Spot::Tab(slot(1)));
    hover(&mut rig, b);
    assert!(visible(&rig));
    escape(&mut rig);
    assert!(drag(&rig).is_none());
    assert!(!hint_visible(
        &rig,
        "Drop to copy 12 games from a.pgn to b.pgn"
    ));
    let sent = release(&mut rig, b);
    assert!(!any_copy(&sent), "{sent:?}");
    assert_eq!(rig.shell.app.bases.slot(slot(2)).unwrap().total(), 8);
}

#[test]
fn a_release_after_escape_does_not_activate_the_tab_under_the_pointer() {
    // The button is still down when Esc cancels; the release over tab B
    // then has no drag to decide it. A user who pressed Esc to avoid the
    // copy does not expect the active database to change.
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    let b = Spot::Tab(slot(2));
    press(&mut rig, Spot::Tab(slot(1)));
    hover(&mut rig, b);
    escape(&mut rig);
    release(&mut rig, b);
    assert_eq!(rig.shell.app.bases.current(), slot(1));
}

#[test]
fn escape_with_a_plain_click_is_not_swallowed() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    // The search field has the focus: a free Esc leaves it.
    rig.shell.ui.games.search_focused = true;
    press(&mut rig, Spot::Tab(slot(2)));
    assert!(drag(&rig).is_some() && !visible(&rig));
    escape(&mut rig);
    assert!(!rig.shell.ui.games.search_focused, "the Esc went on");
    // With a visible drag the Esc is spent on it.
    rig.shell.ui.games.search_focused = true;
    press(&mut rig, Spot::Tab(slot(1)));
    hover(&mut rig, Spot::Tab(slot(2)));
    escape(&mut rig);
    assert!(
        rig.shell.ui.games.search_focused,
        "the Esc cancelled the drag only"
    );
}

#[test]
fn releasing_outside_the_tabs_cancels_without_copying() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    press(&mut rig, Spot::Tab(slot(1)));
    hover(&mut rig, Spot::Tab(slot(2)));
    hover(&mut rig, Spot::Away);
    assert_eq!(drag(&rig).unwrap().over, None);
    assert!(!visible(&rig));
    let sent = release(&mut rig, Spot::Away);
    assert!(
        !any_copy(&sent) && !selected_slot(&sent, slot(1)),
        "{sent:?}"
    );
    assert!(drag(&rig).is_none());
    assert_eq!(rig.shell.app.bases.current(), slot(1));
    assert_eq!(rig.shell.app.bases.slot(CLIP).unwrap().total(), 0);
}

#[test]
fn a_drag_on_a_closed_panel_or_after_closing_the_panel_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    press(&mut rig, Spot::Tab(slot(1)));
    hover(&mut rig, Spot::Tab(CLIP));
    rig.shell.ui.close_pane(PaneKind::GameList);
    rig.shell.ui.games.close_with_panel(false);
    assert!(drag(&rig).is_none());
    assert!(rig.shell.ui.games.hover_tab.is_none());
}

#[test]
fn a_drop_while_a_job_runs_is_refused_with_busy() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    // A copy of b.pgn to the Clipbase is running (its answer not handled).
    let effects = mychess_app::update(
        &mut rig.shell.app,
        AppMessage::Bases(BasesMessage::List(ListMessage::CopyGames {
            from: slot(2),
            to: CLIP,
            scope: CopyScope::Filter,
        })),
    );
    assert_eq!(effects.len(), 1);
    press(&mut rig, Spot::Tab(slot(1)));
    hover(&mut rig, Spot::Tab(CLIP));
    release(&mut rig, Spot::Tab(CLIP));
    assert_eq!(
        notice(&rig),
        Some(mychess_app::AppNotice::Bases(BasesNotice::ListBusy))
    );
    assert_eq!(rig.shell.app.bases.slot(CLIP).unwrap().total(), 0);
}

// ---- rows to tab ----

fn sel_len(rig: &Rig) -> usize {
    rig.shell
        .app
        .bases
        .slot(slot(1))
        .unwrap()
        .list
        .selection
        .len()
}

fn select_rows(rig: &mut Rig, numbers: &[u32]) {
    rig.send(AppMessage::Bases(BasesMessage::List(
        ListMessage::SelectNumbers {
            slot: slot(1),
            numbers: numbers.to_vec(),
        },
    )));
}

#[test]
fn dragging_selected_rows_onto_a_tab_copies_the_selection() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    select_rows(&mut rig, &[2, 3, 5]);
    let press_sent = press(&mut rig, Spot::Row(3));
    assert!(
        press_sent.is_empty(),
        "a press inside the selection keeps it"
    );
    assert_eq!(drag(&rig).unwrap().payload, DragPayload::Selection);
    hover(&mut rig, Spot::Tab(CLIP));
    assert!(visible(&rig));
    assert!(hint_visible(
        &rig,
        "Drop to copy 3 games from a.pgn to Clipbase"
    ));
    let sent = release(&mut rig, Spot::Tab(CLIP));
    assert!(
        has_copy(&sent, slot(1), CLIP, CopyScope::Selection),
        "{sent:?}"
    );
    let whites: Vec<String> = rig
        .shell
        .app
        .bases
        .list_page(CLIP, 1, 100)
        .iter()
        .map(|r| r.white.clone())
        .collect();
    assert_eq!(whites, ["White 2", "White 3", "White 5"]);
    assert!(rig.shell.app.copy_prompt.is_none());
    // The selection of the source is intact.
    assert_eq!(
        rig.shell
            .app
            .bases
            .slot(slot(1))
            .unwrap()
            .list
            .selection
            .len(),
        3
    );
}

#[test]
fn pressing_a_row_outside_the_selection_selects_only_it_and_drags_only_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    select_rows(&mut rig, &[2, 3]);
    let sent = press(&mut rig, Spot::Row(7));
    assert!(sent.iter().any(|m| matches!(
        m,
        AppMessage::Bases(BasesMessage::List(ListMessage::Pick { number: 7, .. }))
    )));
    hover(&mut rig, Spot::Tab(CLIP));
    release(&mut rig, Spot::Tab(CLIP));
    let rows = rig.shell.app.bases.list_page(CLIP, 1, 10);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].white, "White 7");
}

#[test]
fn a_plain_click_on_a_row_of_a_multiple_selection_shrinks_it_only_on_release() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    select_rows(&mut rig, &[2, 3, 4]);
    let at = Spot::Row(3);
    press(&mut rig, at);
    assert_eq!(sel_len(&rig), 3, "still three while the button is down");
    release(&mut rig, at);
    assert_eq!(
        rig.shell
            .app
            .bases
            .slot(slot(1))
            .unwrap()
            .list
            .selection
            .len(),
        1
    );
    assert_eq!(
        rig.shell.app.bases.slot(slot(1)).unwrap().list.selected,
        Some(3)
    );
}

#[test]
fn releasing_on_another_row_keeps_the_selection() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    select_rows(&mut rig, &[2, 3, 4]);
    press(&mut rig, Spot::Row(3));
    hover(&mut rig, Spot::Row(4));
    release(&mut rig, Spot::Row(4));
    assert_eq!(sel_len(&rig), 3);
    assert!(drag(&rig).is_none());
}

#[test]
fn rows_released_on_their_own_tab_or_outside_copy_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    select_rows(&mut rig, &[2, 3]);
    press(&mut rig, Spot::Row(2));
    hover(&mut rig, Spot::Tab(slot(1)));
    let sent = release(&mut rig, Spot::Tab(slot(1)));
    assert!(
        !any_copy(&sent) && !selected_slot(&sent, slot(1)),
        "{sent:?}"
    );
    assert_eq!(sel_len(&rig), 2);
    // Outside.
    press(&mut rig, Spot::Row(2));
    hover(&mut rig, Spot::Tab(CLIP));
    hover(&mut rig, Spot::Away);
    let sent = release(&mut rig, Spot::Away);
    assert!(!any_copy(&sent));
    assert_eq!(rig.shell.app.bases.slot(CLIP).unwrap().total(), 0);
}

#[test]
fn escape_cancels_a_visible_row_drag() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    select_rows(&mut rig, &[2, 3]);
    press(&mut rig, Spot::Row(2));
    hover(&mut rig, Spot::Tab(CLIP));
    assert!(visible(&rig));
    escape(&mut rig);
    assert!(drag(&rig).is_none());
    let sent = release(&mut rig, Spot::Tab(CLIP));
    assert!(!any_copy(&sent));
    assert_eq!(rig.shell.app.bases.slot(CLIP).unwrap().total(), 0);
}

#[test]
fn a_drop_never_changes_the_filter_or_sort_of_any_other_database() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    select_rows(&mut rig, &[1]);
    rig.send(AppMessage::Bases(BasesMessage::List(
        ListMessage::NegateFilter { slot: slot(3) },
    )));
    let before = (
        rig.shell.app.bases.slot(slot(3)).unwrap().filtered(),
        rig.shell.app.bases.slot(slot(2)).unwrap().filtered(),
    );
    press(&mut rig, Spot::Tab(slot(1)));
    hover(&mut rig, Spot::Tab(CLIP));
    release(&mut rig, Spot::Tab(CLIP));
    let after = (
        rig.shell.app.bases.slot(slot(3)).unwrap().filtered(),
        rig.shell.app.bases.slot(slot(2)).unwrap().filtered(),
    );
    assert_eq!(before, after);
}

#[test]
fn a_running_copy_shows_its_progress_text_and_it_goes_away_with_the_job() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    assert!(!hint_visible(&rig, "Copying games: 0%"));
    let effects = mychess_app::update(
        &mut rig.shell.app,
        AppMessage::Bases(BasesMessage::List(ListMessage::CopyGames {
            from: slot(1),
            to: CLIP,
            scope: CopyScope::Filter,
        })),
    );
    assert!(hint_visible(&rig, "Copying games: 0%"));
    // The answer ends the job and the bar.
    for effect in effects {
        let answer = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(mychess_app::perform(effect));
        rig.send(answer);
    }
    assert!(!hint_visible(&rig, "Copying games: 0%"));
    assert_eq!(rig.shell.app.bases.slot(CLIP).unwrap().total(), 12);
}
