//! Contract tests of the quick search of the game list (spec M3-4, slice 4c:
//! rules 9 and 10, D328, D561, D566; Scid `gamelist.tcl` `FindText` 140 and
//! `FilterText` 110, `tkscid.cpp` `sc_filter_textfind` 5821 and
//! `sc_filter_textfilter` 5870).
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::format_push_string,
    clippy::unwrap_used,
    clippy::expect_used
)]
mod m3_4c_common;

use m3_4c_common::*;

/// Five games, database order 1..=5; see the comments for what each holds.
fn dataset() -> Vec<G> {
    vec![
        G::new("Carlsen, Magnus", "Anand, Viswanathan")
            .event("Wijk Open")
            .site("Wijk aan Zee NED")
            .date("2013.01.12")
            .round("1"),
        // "Carlsen" only in the round: the round is never searched.
        G::new("smith", "Jones")
            .event("Club Cup")
            .site("London ENG")
            .date("2019.05.20")
            .round("Carlsen"),
        G::new("Kasparov", "Karpov")
            .event("WCh")
            .site("Moscow URS")
            .date("2013.??.??"),
        // Lower-case "carlsen" as Black.
        G::new("Nakamura", "carlsen, magnus")
            .event("Blitz")
            .site("Rome ITA")
            .date("2020.07.04"),
        // "Carlsen" in the event only.
        G::new("Zed", "Yan")
            .event("Carlsen Memorial")
            .site("Oslo NOR")
            .date("1999.12.31"),
    ]
}

fn setup() -> (Rig, SlotId, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let slot = rig.open_games(dir.path(), "s.pgn", &dataset());
    assert_eq!(rig.state.bases.slot(slot).expect("slot").total(), 5);
    (rig, slot, dir)
}

/// Runs Find and returns the game the cursor is on, or `None` for "not found".
fn find(rig: &mut Rig, slot: SlotId, text: &str) -> Option<u32> {
    rig.set_search(slot, text);
    rig.clear_notice();
    rig.send(l(ListMessage::Find { slot }));
    if rig.notice() == Some(&BasesNotice::NotFound) {
        None
    } else {
        rig.anchor(slot)
    }
}

fn filter_by(rig: &mut Rig, slot: SlotId, text: &str) {
    rig.set_search(slot, text);
    rig.clear_notice();
    rig.send(l(ListMessage::FilterByText { slot }));
}

// ---- Find ----

#[test]
fn find_without_a_selection_starts_at_the_top_row() {
    let (mut rig, slot, _d) = setup();
    assert_eq!(find(&mut rig, slot, "Smith"), None, "case-sensitive: smith");
    assert_eq!(find(&mut rig, slot, "smith"), Some(2));
    assert_eq!(rig.selection(slot), [2]);
}

#[test]
fn find_moves_the_cursor_to_the_hit_and_scrolls_to_it() {
    let (mut rig, slot, _d) = setup();
    assert_eq!(find(&mut rig, slot, "Oslo"), Some(5));
    assert_eq!(rig.first(slot), 5, "the hit becomes the first visible row");
}

#[test]
fn find_goes_to_the_next_hit_after_the_selected_game_not_the_selected_game() {
    let (mut rig, slot, _d) = setup();
    rig.set_search(slot, "ITA");
    rig.pick(slot, 4, PickMode::Only);
    // Game 4 itself matches, but Find looks after it.
    rig.clear_notice();
    rig.send(l(ListMessage::Find { slot }));
    assert_eq!(rig.notice(), Some(&BasesNotice::NotFound));
}

#[test]
fn find_advances_hit_by_hit_and_never_looks_around_the_end() {
    let (mut rig, slot, _d) = setup();
    rig.send(l(ListMessage::SetIgnoreCase {
        slot,
        ignore_case: true,
    }));
    assert_eq!(find(&mut rig, slot, "carlsen"), Some(1));
    assert_eq!(find(&mut rig, slot, "carlsen"), Some(4));
    assert_eq!(find(&mut rig, slot, "carlsen"), Some(5));
    // No hit after game 5: the list returns to the start and says so; it
    // does not wrap round to game 1 (D566).
    rig.clear_notice();
    rig.send(l(ListMessage::Find { slot }));
    assert_eq!(rig.notice(), Some(&BasesNotice::NotFound));
    assert_eq!(rig.first(slot), 1, "Scid FindText: glstart 1");
}

#[test]
fn find_without_a_hit_from_the_start_reports_not_found_and_keeps_the_filter() {
    let (mut rig, slot, _d) = setup();
    assert_eq!(find(&mut rig, slot, "nobody-plays-here"), None);
    assert_eq!(rig.first(slot), 1);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5], "Find only moves");
}

#[test]
fn find_ignores_case_only_when_asked() {
    let (mut rig, slot, _d) = setup();
    assert_eq!(find(&mut rig, slot, "KASPAROV"), None);
    rig.send(l(ListMessage::SetIgnoreCase {
        slot,
        ignore_case: true,
    }));
    assert_eq!(find(&mut rig, slot, "KASPAROV"), Some(3));
}

#[test]
fn find_searches_white_black_event_site_and_date() {
    let (mut rig, slot, _d) = setup();
    let mut from_top = |text: &str| {
        rig.send(BasesMessage::SelectRow { slot, number: None });
        rig.send(BasesMessage::SetListFirst { slot, first: 1 });
        find(&mut rig, slot, text)
    };
    assert_eq!(from_top("Kasparov"), Some(3), "White");
    assert_eq!(from_top("Jones"), Some(2), "Black");
    assert_eq!(from_top("Club Cup"), Some(2), "Event");
    assert_eq!(from_top("Moscow"), Some(3), "Site");
    assert_eq!(from_top("2019.05"), Some(2), "Date");
    assert_eq!(from_top("1999.12.31"), Some(5), "full date");
}

#[test]
fn find_does_not_search_the_round() {
    let (mut rig, slot, _d) = setup();
    // Only game 2 has "Carlsen" as its round, and it must not be reported
    // before the real hit of game 1.
    assert_eq!(find(&mut rig, slot, "Carlsen"), Some(1));
    // From game 1 the next hit is the event of game 5, never game 2.
    assert_eq!(find(&mut rig, slot, "Carlsen"), Some(5));
}

#[test]
fn find_text_is_a_substring_and_keeps_inner_spaces() {
    let (mut rig, slot, _d) = setup();
    assert_eq!(find(&mut rig, slot, "ijk aan Ze"), Some(1), "substring");
    assert_eq!(
        find(&mut rig, slot, "Wijkaan"),
        None,
        "spaces are not ignored"
    );
}

#[test]
fn find_skips_games_that_are_out_of_the_filter() {
    let (mut rig, slot, _d) = setup();
    // Remove game 3 ("Moscow") from the filter.
    rig.pick(slot, 3, PickMode::Only);
    rig.send(l(ListMessage::RemoveSelected { slot }));
    assert_eq!(rig.filter_numbers(slot), [1, 2, 4, 5]);
    rig.pick(slot, 1, PickMode::Only);
    assert_eq!(find(&mut rig, slot, "Moscow"), None, "game 3 is not listed");
}

#[test]
fn find_with_empty_text_does_nothing() {
    let (mut rig, slot, _d) = setup();
    rig.set_search(slot, "");
    rig.pick(slot, 2, PickMode::Only);
    rig.clear_notice();
    let effects = rig.raw(l(ListMessage::Find { slot }));
    assert!(effects.is_empty(), "no search for an empty text");
    assert!(rig.state.list_job().is_none());
    assert_eq!(rig.anchor(slot), Some(2));
    rig.set_search(slot, "   ");
    let effects = rig.raw(l(ListMessage::Find { slot }));
    assert!(effects.is_empty(), "blank text is empty too");
}

#[test]
fn find_in_a_sorted_view_follows_the_view_order() {
    let (mut rig, slot, _d) = setup();
    // By White ascending (case-folded): Carlsen(1) Kasparov(3) Nakamura(4)
    // smith(2) Zed(5).
    rig.send(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    assert_eq!(rig.view(slot), [1, 3, 4, 2, 5]);
    rig.send(l(ListMessage::SetIgnoreCase {
        slot,
        ignore_case: true,
    }));
    // "carlsen" hits games 1, 4 and 5; in the view order that is 1, 4, 5.
    assert_eq!(find(&mut rig, slot, "carlsen"), Some(1));
    assert_eq!(find(&mut rig, slot, "carlsen"), Some(4));
    assert_eq!(find(&mut rig, slot, "carlsen"), Some(5));
    // Descending view: 5, 2, 4, 3, 1. Start from the top.
    rig.send(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    assert_eq!(rig.view(slot), [5, 2, 4, 3, 1]);
    rig.pick(slot, 5, PickMode::Only);
    assert_eq!(find(&mut rig, slot, "carlsen"), Some(4));
    assert_eq!(find(&mut rig, slot, "carlsen"), Some(1));
}

#[test]
fn find_in_a_sorted_view_does_not_find_hits_above_the_cursor() {
    let (mut rig, slot, _d) = setup();
    rig.send(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    // View: 1 3 4 2 5. Cursor on game 2 (position 4): "Karpov" is game 3,
    // which sits above the cursor in the view, so it is not found.
    rig.pick(slot, 2, PickMode::Only);
    assert_eq!(find(&mut rig, slot, "Karpov"), None);
}

#[test]
fn find_runs_as_a_job_that_is_gone_when_it_answers() {
    let (mut rig, slot, _d) = setup();
    rig.set_search(slot, "Rome");
    let mut effects = rig.raw(l(ListMessage::Find { slot }));
    assert_eq!(effects.len(), 1);
    let (kind, _) = rig.state.list_job().expect("job in flight");
    assert_eq!(kind, ListJobKind::Find);
    let answer = rig.perform(effects.remove(0));
    rig.send(answer);
    assert!(rig.state.list_job().is_none());
    assert_eq!(rig.anchor(slot), Some(4));
}

// ---- Filter ----

#[test]
fn filter_by_text_removes_the_games_that_do_not_match() {
    let (mut rig, slot, _d) = setup();
    filter_by(&mut rig, slot, "Carlsen");
    assert_eq!(rig.filter_numbers(slot), [1, 5]);
    assert_eq!(rig.notice(), Some(&BasesNotice::Filtered { removed: 3 }));
    assert_eq!(rig.view(slot), [1, 5]);
}

#[test]
fn filter_by_text_does_not_touch_the_database_or_other_games_data() {
    let (mut rig, slot, _d) = setup();
    filter_by(&mut rig, slot, "Moscow");
    let slot_ref = rig.state.bases.slot(slot).expect("slot");
    assert_eq!(slot_ref.total(), 5, "the database keeps its games");
    assert_eq!(slot_ref.filtered(), 1);
}

#[test]
fn filter_by_text_with_plus_applies_a_logical_and() {
    let (mut rig, slot, _d) = setup();
    ignore_case(&mut rig, slot);
    // "carlsen" hits 1, 4, 5; "magnus" hits 1, 4; both: 1, 4.
    filter_by(&mut rig, slot, "carlsen+magnus");
    assert_eq!(rig.filter_numbers(slot), [1, 4]);
    // Each piece is trimmed: spaces around "+" do not matter.
    rig.send(l(ListMessage::ResetFilter { slot }));
    filter_by(&mut rig, slot, "  carlsen  +  anand ");
    assert_eq!(rig.filter_numbers(slot), [1]);
}

fn ignore_case(rig: &mut Rig, slot: SlotId) {
    rig.send(l(ListMessage::SetIgnoreCase {
        slot,
        ignore_case: true,
    }));
}

#[test]
fn filter_by_text_with_a_contradictory_and_leaves_nothing() {
    let (mut rig, slot, _d) = setup();
    filter_by(&mut rig, slot, "Kasparov+Oslo");
    assert_eq!(rig.filter_numbers(slot), Vec::<u32>::new());
    assert_eq!(rig.state.bases.slot(slot).expect("slot").filtered(), 0);
}

#[test]
fn filter_by_text_ignores_empty_terms() {
    let (mut rig, slot, _d) = setup();
    filter_by(&mut rig, slot, "Kasparov++ +");
    assert_eq!(rig.filter_numbers(slot), [3]);
}

#[test]
fn filter_by_text_only_ever_reduces_the_filter() {
    let (mut rig, slot, _d) = setup();
    // Take game 1 out of the filter by hand; a text that matches it must
    // not bring it back.
    rig.pick(slot, 1, PickMode::Only);
    rig.send(l(ListMessage::RemoveSelected { slot }));
    filter_by(&mut rig, slot, "Carlsen");
    assert_eq!(rig.filter_numbers(slot), [5]);
    // The same text again changes nothing.
    filter_by(&mut rig, slot, "Carlsen");
    assert_eq!(rig.filter_numbers(slot), [5]);
    assert_eq!(rig.notice(), Some(&BasesNotice::Filtered { removed: 0 }));
}

#[test]
fn filter_by_text_honours_the_case_flag() {
    let (mut rig, slot, _d) = setup();
    filter_by(&mut rig, slot, "carlsen");
    assert_eq!(rig.filter_numbers(slot), [4], "exact case: only the black");
    rig.send(l(ListMessage::ResetFilter { slot }));
    ignore_case(&mut rig, slot);
    filter_by(&mut rig, slot, "carlsen");
    assert_eq!(rig.filter_numbers(slot), [1, 4, 5]);
}

#[test]
fn filter_by_text_with_empty_text_does_nothing() {
    let (mut rig, slot, _d) = setup();
    rig.set_search(slot, "");
    let effects = rig.raw(l(ListMessage::FilterByText { slot }));
    assert!(effects.is_empty());
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5]);
}

#[test]
fn filter_by_text_drops_removed_games_from_the_selection_and_goes_to_the_top() {
    let (mut rig, slot, _d) = setup();
    rig.select_all(slot, &[1, 2, 3]);
    rig.send(BasesMessage::SetListFirst { slot, first: 4 });
    filter_by(&mut rig, slot, "Carlsen");
    assert_eq!(rig.selection(slot), [1], "2 and 3 left the filter");
    assert_eq!(rig.first(slot), 1, "Scid FilterText: glstart 1");
}

#[test]
fn filter_by_text_in_a_sorted_view_keeps_the_view_order() {
    let (mut rig, slot, _d) = setup();
    rig.send(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    ignore_case(&mut rig, slot);
    filter_by(&mut rig, slot, "carlsen");
    assert_eq!(
        rig.view(slot),
        [1, 4, 5],
        "view by White: Carlsen, Nakamura, Zed"
    );
}

#[test]
fn a_cancelled_filter_keeps_the_previous_filter_and_leaves_no_job() {
    let (mut rig, slot, _d) = setup();
    rig.set_search(slot, "Carlsen");
    let mut effects = rig.raw(l(ListMessage::FilterByText { slot }));
    assert!(rig.state.list_job().is_some());
    rig.raw(l(ListMessage::CancelJob));
    rig.clear_notice();
    let answer = rig.perform(effects.remove(0));
    rig.send(answer);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5], "D328/D561");
    assert!(rig.state.list_job().is_none(), "no job left pending");
    assert_eq!(rig.notice(), None, "cancelling is silent");
    // The list works again.
    filter_by(&mut rig, slot, "Carlsen");
    assert_eq!(rig.filter_numbers(slot), [1, 5]);
}

#[test]
fn a_second_search_while_one_runs_says_the_list_is_busy() {
    let (mut rig, slot, _d) = setup();
    rig.set_search(slot, "Carlsen");
    let effects = rig.raw(l(ListMessage::FilterByText { slot }));
    assert_eq!(effects.len(), 1);
    rig.clear_notice();
    for message in [
        ListMessage::FilterByText { slot },
        ListMessage::Find { slot },
        ListMessage::ResetFilter { slot },
        ListMessage::NegateFilter { slot },
        ListMessage::RemoveSelected { slot },
        ListMessage::RemoveAbove { slot, number: 3 },
        ListMessage::RemoveBelow { slot, number: 3 },
        ListMessage::SortBy {
            slot,
            column: ListColumn::White,
        },
    ] {
        rig.clear_notice();
        let more = rig.raw(l(message.clone()));
        assert!(more.is_empty(), "{message:?} must not start another job");
        assert_eq!(rig.notice(), Some(&BasesNotice::ListBusy), "{message:?}");
    }
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5], "nothing changed");
}

#[test]
fn keep_selected_and_copy_are_refused_while_a_job_runs() {
    let (mut rig, slot, _d) = setup();
    rig.pick(slot, 2, PickMode::Only);
    rig.set_search(slot, "Carlsen");
    let _held = rig.raw(l(ListMessage::FilterByText { slot }));
    for message in [
        ListMessage::KeepSelected { slot },
        ListMessage::CopySelected { slot },
    ] {
        rig.clear_notice();
        let more = rig.raw(l(message.clone()));
        assert!(more.is_empty(), "{message:?}");
        assert_eq!(rig.notice(), Some(&BasesNotice::ListBusy), "{message:?}");
    }
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5]);
}

#[test]
fn selection_clicks_still_work_while_a_job_runs() {
    let (mut rig, slot, _d) = setup();
    rig.set_search(slot, "Carlsen");
    let _held = rig.raw(l(ListMessage::FilterByText { slot }));
    rig.pick(slot, 3, PickMode::Only);
    assert_eq!(rig.selection(slot), [3]);
}

#[test]
fn closing_the_slot_during_a_job_cancels_it_and_drops_its_answer() {
    let (mut rig, slot, dir) = setup();
    // A second file so that closing the first leaves something to see.
    let other = rig.open_games(dir.path(), "o.pgn", &numbered(3));
    assert_ne!(other, slot);
    rig.set_search(slot, "Carlsen");
    let mut effects = rig.raw(l(ListMessage::FilterByText { slot }));
    let control = match &effects[0] {
        Effect::SearchList { control, .. } => control.clone(),
        other => panic!("expected SearchList, got {other:?}"),
    };
    // The search finishes in the background before the user closes the slot.
    let answer = rig.perform(effects.remove(0));
    rig.send(BasesMessage::Close(slot));
    assert!(control.is_cancelled(), "closing the slot cancels its job");
    assert!(rig.state.list_job().is_none());
    // A new file takes the freed slot.
    let reopened = rig.open_games(dir.path(), "n.pgn", &numbered(4));
    assert_eq!(reopened, slot, "first free slot");
    rig.send(answer);
    assert_eq!(
        rig.filter_numbers(slot),
        [1, 2, 3, 4],
        "the stale answer must not shrink the new database's filter"
    );
}

#[test]
fn closing_another_slot_does_not_cancel_the_job() {
    let (mut rig, slot, dir) = setup();
    let other = rig.open_games(dir.path(), "o.pgn", &numbered(3));
    rig.set_search(slot, "Carlsen");
    let effects = rig.raw(l(ListMessage::FilterByText { slot }));
    let Effect::SearchList { control, .. } = &effects[0] else {
        panic!("expected SearchList");
    };
    let control = control.clone();
    rig.send(BasesMessage::Close(other));
    assert!(!control.is_cancelled());
    assert!(rig.state.list_job().is_some());
}

#[test]
fn an_answer_for_an_unknown_request_is_ignored() {
    let (mut rig, slot, _d) = setup();
    rig.set_search(slot, "Carlsen");
    let mut effects = rig.raw(l(ListMessage::FilterByText { slot }));
    let answer = rig.perform(effects.remove(0));
    // Cancel through a close of the job is not needed: answer it twice. The
    // second copy has no job to belong to.
    rig.send(answer.clone());
    assert_eq!(rig.filter_numbers(slot), [1, 5]);
    rig.send(l(ListMessage::ResetFilter { slot }));
    rig.send(answer);
    assert_eq!(
        rig.filter_numbers(slot),
        [1, 2, 3, 4, 5],
        "a repeated answer must not be applied again"
    );
}

#[test]
fn filtering_a_database_with_over_5000_games_reports_progress_and_is_exact() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let mut games = numbered(6000);
    games[5999] = G::new("Needle", "Haystack");
    let slot = rig.open_games(dir.path(), "big.pgn", &games);
    assert_eq!(rig.state.bases.slot(slot).expect("slot").total(), 6000);
    rig.set_search(slot, "Needle");
    let mut effects = rig.raw(l(ListMessage::FilterByText { slot }));
    let Effect::SearchList { control, .. } = &effects[0] else {
        panic!("expected SearchList");
    };
    let control = control.clone();
    let answer = rig.perform(effects.remove(0));
    assert_eq!(control.progress(), (6000, 6000), "progress ends complete");
    rig.send(answer);
    assert_eq!(rig.filter_numbers(slot), [6000]);
    assert_eq!(rig.notice(), Some(&BasesNotice::Filtered { removed: 5999 }));
}

#[test]
fn find_in_a_database_with_over_5000_games_reaches_the_last_game() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let mut games = numbered(5500);
    games[5499] = G::new("Needle", "Haystack");
    let slot = rig.open_games(dir.path(), "big.pgn", &games);
    assert_eq!(find(&mut rig, slot, "Needle"), Some(5500));
    assert_eq!(rig.first(slot), 5500);
}

// ---- property: Find and Filter equal a naive scan of the rows ----

mod model {
    use super::*;
    use proptest::prelude::*;

    const NAMES: [&str; 6] = ["Anna", "anna lee", "Bob Stone", "Carl", "STONE", "dora"];
    const PLACES: [&str; 5] = ["Rome ITA", "oslo NOR", "Paris FRA", "Bern SUI", "Rome ESP"];
    const DATES: [&str; 4] = ["2001.02.03", "2001.12.31", "1999.05.06", "2020.02.20"];
    const NEEDLES: [&str; 14] = [
        "anna", "Anna", "stone", "STONE", "Rome", "ITA", "oslo", "2001", "02.0", "20", "l", "ar",
        "zzz", "bo",
    ];

    fn game() -> impl Strategy<Value = G> {
        (
            0..NAMES.len(),
            0..NAMES.len(),
            0..PLACES.len(),
            0..PLACES.len(),
            0..DATES.len(),
        )
            .prop_map(|(w, b, e, s, d)| {
                G::new(NAMES[w], NAMES[b])
                    .event(PLACES[e])
                    .site(PLACES[s])
                    .date(DATES[d])
            })
    }

    fn hit(game: &G, needle: &str, ignore_case: bool) -> bool {
        let fold = |s: &str| {
            if ignore_case {
                s.to_ascii_lowercase()
            } else {
                s.to_owned()
            }
        };
        let needle = fold(needle);
        [
            &game.white,
            &game.black,
            &game.event,
            &game.site,
            &game.date,
        ]
        .iter()
        .any(|field| fold(field).contains(&needle))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(40))]

        #[test]
        fn find_equals_a_naive_scan_from_the_anchor(
            games in proptest::collection::vec(game(), 2..12),
            needle in 0..NEEDLES.len(),
            anchor in proptest::option::of(1u32..12),
            ignore_case in any::<bool>(),
        ) {
            let dir = tempfile::tempdir().expect("dir");
            let mut rig = Rig::new();
            let slot = rig.open_games(dir.path(), "m.pgn", &games);
            let anchor = anchor.filter(|a| *a as usize <= games.len());
            if let Some(a) = anchor {
                rig.pick(slot, a, PickMode::Only);
            }
            rig.send(l(ListMessage::SetIgnoreCase { slot, ignore_case }));
            let needle = NEEDLES[needle];
            let expected = games
                .iter()
                .enumerate()
                .map(|(i, g)| (i as u32 + 1, g))
                .filter(|(n, _)| anchor.is_none_or(|a| *n > a))
                .find(|(_, g)| hit(g, needle, ignore_case))
                .map(|(n, _)| n);
            let got = find(&mut rig, slot, needle);
            prop_assert_eq!(got, expected, "needle {:?} anchor {:?}", needle, anchor);
        }

        #[test]
        fn filter_equals_a_naive_conjunction_of_terms(
            games in proptest::collection::vec(game(), 1..12),
            terms in proptest::collection::vec(0..NEEDLES.len(), 1..4),
            ignore_case in any::<bool>(),
        ) {
            let dir = tempfile::tempdir().expect("dir");
            let mut rig = Rig::new();
            let slot = rig.open_games(dir.path(), "m.pgn", &games);
            rig.send(l(ListMessage::SetIgnoreCase { slot, ignore_case }));
            let needles: Vec<&str> = terms.iter().map(|t| NEEDLES[*t]).collect();
            let expected: Vec<u32> = games
                .iter()
                .enumerate()
                .filter(|(_, g)| needles.iter().all(|n| hit(g, n, ignore_case)))
                .map(|(i, _)| i as u32 + 1)
                .collect();
            filter_by(&mut rig, slot, &needles.join(" + "));
            prop_assert_eq!(rig.filter_numbers(slot), expected, "terms {:?}", needles);
        }
    }
}
