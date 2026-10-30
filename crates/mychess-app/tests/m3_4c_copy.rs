//! Contract tests of "copy the selected games to the Clipbase" (spec M3-4,
//! slice 4c: D510, D511, D567, rules 7 and 8; Scid `switcher.tcl` `copyFilter`
//! 1139, `tkscid.cpp` `sc_filter_copy` 5245).
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::format_push_string,
    clippy::unwrap_used,
    clippy::expect_used
)]
mod m3_4c_common;

use m3_4c_common::*;

fn setup(n: usize) -> (Rig, SlotId, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let slot = rig.open_games(dir.path(), "s.pgn", &numbered(n));
    (rig, slot, dir)
}

fn clip_whites(rig: &Rig) -> Vec<String> {
    rig.state
        .bases
        .list_page(SlotId::CLIPBASE, 1, 10_000)
        .iter()
        .map(|r| r.white.clone())
        .collect()
}

fn clip_counts(rig: &Rig) -> (u32, u32) {
    let clip = rig.state.bases.slot(SlotId::CLIPBASE).expect("clipbase");
    (clip.total(), clip.filtered())
}

#[test]
fn only_the_selected_games_are_copied() {
    let (mut rig, slot, _d) = setup(6);
    rig.select_all(slot, &[2, 5]);
    rig.send(l(ListMessage::CopySelected { slot }));
    assert_eq!(clip_whites(&rig), ["P2", "P5"]);
    assert_eq!(rig.notice(), Some(&BasesNotice::Copied { count: 2 }));
}

#[test]
fn the_numbers_are_a_proper_list_not_a_concatenation() {
    // D510: the Scid builds the list of numbers by concatenating them
    // ("2" "10" "11" becomes 21011).
    let (mut rig, slot, _d) = setup(12);
    rig.select_all(slot, &[2, 10, 11]);
    rig.send(l(ListMessage::CopySelected { slot }));
    assert_eq!(clip_whites(&rig), ["P2", "P10", "P11"]);
}

#[test]
fn games_enter_in_database_order_whatever_the_order_of_the_clicks() {
    let (mut rig, slot, _d) = setup(8);
    rig.pick(slot, 7, PickMode::Only);
    rig.pick(slot, 3, PickMode::Toggle);
    rig.pick(slot, 5, PickMode::Toggle);
    rig.send(l(ListMessage::CopySelected { slot }));
    assert_eq!(clip_whites(&rig), ["P3", "P5", "P7"]);
}

#[test]
fn games_enter_in_database_order_even_when_the_view_is_sorted_descending() {
    let (mut rig, slot, _d) = setup(5);
    rig.send(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    rig.send(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    rig.select_all(slot, &[1, 4, 2]);
    rig.send(l(ListMessage::CopySelected { slot }));
    assert_eq!(clip_whites(&rig), ["P1", "P2", "P4"]);
}

#[test]
fn the_copied_games_keep_their_content() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let games = vec![
        G::new("Alice", "Bob")
            .event("Open")
            .site("Rome ITA")
            .date("2021.03.04")
            .round("5")
            .result("1-0")
            .moves("1. d4 d5 2. c4 *")
            .elo(2400, 2300),
        G::new("C", "D"),
    ];
    let slot = rig.open_games(dir.path(), "s.pgn", &games);
    rig.select_all(slot, &[1]);
    rig.send(l(ListMessage::CopySelected { slot }));
    let rows = rig.state.bases.list_page(SlotId::CLIPBASE, 1, 5);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(
        (row.white.as_str(), row.black.as_str(), row.event.as_str()),
        ("Alice", "Bob", "Open")
    );
    assert_eq!(
        (row.site.as_str(), row.date.as_str()),
        ("Rome ITA", "2021.03.04")
    );
    assert_eq!((row.white_elo, row.black_elo), (2400, 2300));
    assert_eq!(row.length, 2, "3 plies = 2 moves in the Scid count");
    // And the game itself can be loaded back from the Clipbase.
    rig.send(BasesMessage::LoadGame {
        slot: SlotId::CLIPBASE,
        number: 1,
    });
    assert_eq!(rig.state.session.game().tags().white, "Alice");
    assert_eq!(rig.state.session.game().mainline().count(), 3);
}

#[test]
fn the_source_filter_selection_and_database_do_not_change() {
    let (mut rig, slot, _d) = setup(5);
    rig.select_all(slot, &[2, 3]);
    rig.send(l(ListMessage::CopySelected { slot }));
    assert_eq!(rig.selection(slot), [2, 3]);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5]);
    assert_eq!(rig.state.bases.slot(slot).expect("slot").total(), 5);
    assert_eq!(rig.state.bases.current(), slot, "the current slot stays");
}

#[test]
fn the_filter_of_the_clipbase_grows_to_include_the_new_games() {
    let (mut rig, slot, _d) = setup(6);
    rig.select_all(slot, &[1, 2, 3]);
    rig.send(l(ListMessage::CopySelected { slot }));
    assert_eq!(clip_counts(&rig), (3, 3));
    // Take the first game of the Clipbase out of its filter, then copy more:
    // the new games come in (`append(1)`), the removed one stays out.
    rig.pick(SlotId::CLIPBASE, 1, PickMode::Only);
    rig.send(l(ListMessage::RemoveSelected {
        slot: SlotId::CLIPBASE,
    }));
    assert_eq!(clip_counts(&rig), (3, 2));
    rig.select_all(slot, &[5, 6]);
    rig.send(l(ListMessage::CopySelected { slot }));
    assert_eq!(clip_counts(&rig), (5, 4));
    assert_eq!(rig.filter_numbers(SlotId::CLIPBASE), [2, 3, 4, 5]);
}

#[test]
fn successive_copies_append_at_the_end() {
    let (mut rig, slot, _d) = setup(4);
    rig.select_all(slot, &[4]);
    rig.send(l(ListMessage::CopySelected { slot }));
    rig.select_all(slot, &[1]);
    rig.send(l(ListMessage::CopySelected { slot }));
    assert_eq!(clip_whites(&rig), ["P4", "P1"]);
    // The same game twice is allowed (the Scid copies, it does not merge).
    rig.send(l(ListMessage::CopySelected { slot }));
    assert_eq!(clip_whites(&rig), ["P4", "P1", "P1"]);
}

#[test]
fn many_games_are_copied_in_one_batch_in_order() {
    let (mut rig, slot, _d) = setup(300);
    let all: Vec<u32> = (1..=300).collect();
    rig.select_all(slot, &all);
    rig.send(l(ListMessage::CopySelected { slot }));
    assert_eq!(clip_counts(&rig), (300, 300));
    let whites = clip_whites(&rig);
    assert_eq!(whites.first().map(String::as_str), Some("P1"));
    assert_eq!(whites.last().map(String::as_str), Some("P300"));
    assert!(rig.state.list_job().is_none());
}

#[test]
fn copying_with_nothing_selected_says_so_and_starts_no_job() {
    let (mut rig, slot, _d) = setup(3);
    let effects = rig.raw(l(ListMessage::CopySelected { slot }));
    assert!(effects.is_empty());
    assert_eq!(rig.notice(), Some(&BasesNotice::NothingSelected));
    assert!(rig.state.list_job().is_none());
    assert_eq!(clip_counts(&rig), (0, 0));
}

#[test]
fn copying_from_inside_the_clipbase_is_refused() {
    let (mut rig, slot, _d) = setup(3);
    rig.select_all(slot, &[1, 2]);
    rig.send(l(ListMessage::CopySelected { slot }));
    rig.select_all(SlotId::CLIPBASE, &[1]);
    rig.clear_notice();
    let effects = rig.raw(l(ListMessage::CopySelected {
        slot: SlotId::CLIPBASE,
    }));
    assert!(effects.is_empty());
    assert_eq!(rig.notice(), Some(&BasesNotice::AlreadyInClipbase));
    assert_eq!(clip_counts(&rig), (2, 2), "nothing was copied");
}

#[test]
fn the_clipbase_limit_stops_a_copy_that_does_not_fit_and_copies_nothing() {
    let (mut rig, slot, _d) = setup(6);
    rig.state.bases.set_clipbase_limit(2);
    rig.select_all(slot, &[1, 2, 3]);
    rig.send(l(ListMessage::CopySelected { slot }));
    assert_eq!(
        rig.notice(),
        Some(&BasesNotice::Failed(BaseError::ClipbaseLimit { limit: 2 }))
    );
    assert_eq!(clip_counts(&rig), (0, 0), "all or nothing (push_many)");
    assert_eq!(clip_whites(&rig), Vec::<String>::new());
    assert!(rig.state.list_job().is_none());
}

#[test]
fn a_copy_that_exactly_fills_the_clipbase_succeeds_and_the_next_one_fails() {
    let (mut rig, slot, _d) = setup(6);
    rig.state.bases.set_clipbase_limit(3);
    rig.select_all(slot, &[1, 2]);
    rig.send(l(ListMessage::CopySelected { slot }));
    rig.select_all(slot, &[3]);
    rig.send(l(ListMessage::CopySelected { slot }));
    assert_eq!(clip_counts(&rig), (3, 3));
    assert_eq!(rig.notice(), Some(&BasesNotice::Copied { count: 1 }));
    rig.select_all(slot, &[4]);
    rig.send(l(ListMessage::CopySelected { slot }));
    assert_eq!(
        rig.notice(),
        Some(&BasesNotice::Failed(BaseError::ClipbaseLimit { limit: 3 }))
    );
    assert_eq!(clip_counts(&rig), (3, 3));
    assert_eq!(clip_whites(&rig), ["P1", "P2", "P3"], "still consistent");
}

#[test]
fn a_cancelled_copy_copies_nothing_and_leaves_no_job() {
    let (mut rig, slot, _d) = setup(6);
    rig.select_all(slot, &[1, 2, 3]);
    let mut effects = rig.raw(l(ListMessage::CopySelected { slot }));
    assert_eq!(
        rig.state.list_job().map(|(k, _)| k),
        Some(ListJobKind::Copy)
    );
    rig.raw(l(ListMessage::CancelJob));
    rig.clear_notice();
    let answer = rig.perform(effects.remove(0));
    rig.send(answer);
    assert!(rig.state.list_job().is_none());
    assert_eq!(clip_counts(&rig), (0, 0));
    assert_eq!(rig.notice(), None, "cancelling is silent");
    // And the list can copy again.
    rig.send(l(ListMessage::CopySelected { slot }));
    assert_eq!(clip_counts(&rig), (3, 3));
}

#[test]
fn a_copy_in_flight_blocks_other_list_actions() {
    let (mut rig, slot, _d) = setup(4);
    rig.select_all(slot, &[1]);
    let _held = rig.raw(l(ListMessage::CopySelected { slot }));
    rig.clear_notice();
    assert!(rig.raw(l(ListMessage::NegateFilter { slot })).is_empty());
    assert_eq!(rig.notice(), Some(&BasesNotice::ListBusy));
    assert!(rig.raw(l(ListMessage::CopySelected { slot })).is_empty());
}

#[test]
fn closing_the_source_slot_during_the_copy_keeps_the_clipbase_consistent() {
    let (mut rig, slot, dir) = setup(4);
    let _other = rig.open_games(dir.path(), "o.pgn", &numbered(2));
    rig.select_all(slot, &[1, 2]);
    let mut effects = rig.raw(l(ListMessage::CopySelected { slot }));
    let answer = rig.perform(effects.remove(0));
    rig.send(BasesMessage::Close(slot));
    rig.send(answer);
    // The copy already happened in the Clipbase; its filter must match.
    let (total, filtered) = clip_counts(&rig);
    assert_eq!(total, filtered, "filter as long as the Clipbase");
    assert!(rig.state.list_job().is_none());
}

#[test]
fn copying_from_an_si4_database_works_too() {
    let dir = tempfile::tempdir().expect("dir");
    let path = make_si4(dir.path(), "db", &pgn(&numbered(4)));
    let mut rig = Rig::new();
    let slot = rig.open(&path);
    assert_eq!(
        rig.state.bases.slot(slot).expect("slot").kind,
        BaseKind::Si4
    );
    rig.select_all(slot, &[2, 4]);
    rig.send(l(ListMessage::CopySelected { slot }));
    assert_eq!(clip_whites(&rig), ["P2", "P4"]);
}

#[test]
fn a_copy_resets_the_sort_of_the_clipbase_but_not_of_the_source() {
    let (mut rig, slot, _d) = setup(4);
    rig.select_all(slot, &[1, 2]);
    rig.send(l(ListMessage::CopySelected { slot }));
    rig.send(l(ListMessage::SortBy {
        slot: SlotId::CLIPBASE,
        column: ListColumn::White,
    }));
    rig.send(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    rig.send(l(ListMessage::CopySelected { slot }));
    assert_eq!(
        rig.state.bases.slot(SlotId::CLIPBASE).expect("c").sort(),
        None
    );
    assert_eq!(
        rig.state.bases.slot(slot).expect("s").sort(),
        Some((ListColumn::White, SortDir::Ascending))
    );
}
