//! Contract tests of the sorted view of the game list (spec M3-4, slice 4c:
//! D506, D558, rule 9; Scid `gamelist.tcl` `SortBy` 1064, `tkscid.cpp`
//! `sc_base_sort` 1922, `index.cpp` comparison 690-820).
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::format_push_string,
    clippy::unwrap_used,
    clippy::expect_used
)]
mod m3_4c_common;

use m3_4c_common::*;
use mychess_app::GameRow;
use proptest::prelude::*;

fn white_names(names: &[&str]) -> Vec<G> {
    names.iter().map(|n| G::new(n, "x")).collect()
}

fn click(rig: &mut Rig, slot: SlotId, column: ListColumn) {
    rig.send(l(ListMessage::SortBy { slot, column }));
}

fn setup(games: &[G]) -> (Rig, SlotId, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let slot = rig.open_games(dir.path(), "s.pgn", games);
    (rig, slot, dir)
}

fn rows_in_db_order(rig: &Rig, slot: SlotId) -> Vec<GameRow> {
    rig.state.bases.list_page(slot, 1, u32::MAX / 2)
}

// ---- header clicks ----

#[test]
fn clicking_a_header_sorts_the_view_ascending() {
    let (mut rig, slot, _d) = setup(&white_names(&["Delta", "alpha", "Charlie", "Bravo"]));
    click(&mut rig, slot, ListColumn::White);
    assert_eq!(
        rig.state.bases.slot(slot).expect("slot").sort(),
        Some((ListColumn::White, SortDir::Ascending))
    );
    // Case is folded: alpha, Bravo, Charlie, Delta.
    assert_eq!(rig.view(slot), [2, 4, 3, 1]);
}

#[test]
fn the_second_click_flips_the_direction_without_a_new_job() {
    let (mut rig, slot, _d) = setup(&white_names(&["Delta", "alpha", "Charlie", "Bravo"]));
    click(&mut rig, slot, ListColumn::White);
    let effects = rig.raw(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    assert!(effects.is_empty(), "flipping must not recompute (D558)");
    assert!(rig.state.list_job().is_none());
    assert_eq!(
        rig.state.bases.slot(slot).expect("slot").sort(),
        Some((ListColumn::White, SortDir::Descending))
    );
    assert_eq!(rig.view(slot), [1, 3, 4, 2]);
    click(&mut rig, slot, ListColumn::White);
    assert_eq!(rig.view(slot), [2, 4, 3, 1], "third click: ascending again");
}

#[test]
fn clicking_another_column_starts_again_in_ascending_order() {
    let games = vec![
        G::new("B", "z").event("e2"),
        G::new("A", "y").event("e3"),
        G::new("C", "x").event("e1"),
    ];
    let (mut rig, slot, _d) = setup(&games);
    click(&mut rig, slot, ListColumn::White);
    click(&mut rig, slot, ListColumn::White);
    click(&mut rig, slot, ListColumn::Event);
    assert_eq!(
        rig.state.bases.slot(slot).expect("slot").sort(),
        Some((ListColumn::Event, SortDir::Ascending))
    );
    assert_eq!(rig.view(slot), [3, 1, 2]);
}

#[test]
fn columns_that_do_not_sort_do_not_respond() {
    let (mut rig, slot, _d) = setup(&white_names(&["B", "A", "C"]));
    for column in [
        ListColumn::Number,
        ListColumn::Opening,
        ListColumn::Flags,
        ListColumn::Annos,
        ListColumn::Start,
        ListColumn::EndMaterial,
    ] {
        let effects = rig.raw(l(ListMessage::SortBy { slot, column }));
        assert!(effects.is_empty(), "{column:?} must not start a sort");
        assert!(rig.state.list_job().is_none(), "{column:?}");
        assert_eq!(
            rig.state.bases.slot(slot).expect("slot").sort(),
            None,
            "{column:?}"
        );
        assert_eq!(rig.view(slot), [1, 2, 3], "{column:?}");
    }
}

#[test]
fn a_non_sorting_column_keeps_the_current_sort() {
    let (mut rig, slot, _d) = setup(&white_names(&["B", "A", "C"]));
    click(&mut rig, slot, ListColumn::White);
    click(&mut rig, slot, ListColumn::Number);
    assert_eq!(
        rig.state.bases.slot(slot).expect("slot").sort(),
        Some((ListColumn::White, SortDir::Ascending))
    );
    assert_eq!(rig.view(slot), [2, 1, 3]);
}

#[test]
fn exactly_sixteen_columns_sort() {
    let sortable = ListColumn::ALL.iter().filter(|c| c.is_sortable()).count();
    assert_eq!(sortable, 16);
}

#[test]
fn every_sortable_column_yields_a_permutation_and_its_flip_is_the_reverse() {
    let games = vec![
        G::new("B", "z")
            .site("Rome ITA")
            .round("2")
            .date("2020.01.02"),
        G::new("A", "y")
            .site("Oslo NOR")
            .round("10")
            .date("2020.??.??"),
        G::new("C", "x").site("Kyiv").round("1").date("????.??.??"),
        G::new("A", "w")
            .site("Paris FRA")
            .round("3.2")
            .date("2019.12.31"),
    ];
    let (mut rig, slot, _d) = setup(&games);
    for column in ListColumn::ALL.into_iter().filter(|c| c.is_sortable()) {
        click(&mut rig, slot, column);
        let asc = rig.view(slot);
        let mut sorted = asc.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, [1, 2, 3, 4], "{column:?} is a permutation");
        click(&mut rig, slot, column);
        let mut back = rig.view(slot);
        back.reverse();
        assert_eq!(back, asc, "{column:?}: flip reverses the view");
        rig.send(l(ListMessage::ResetFilter { slot }));
    }
}

// ---- ordering rules, one column at a time ----

#[test]
fn equal_keys_keep_the_database_order() {
    let games = vec![G::new("same", "1"), G::new("A", "2"), G::new("same", "3")];
    let (mut rig, slot, _d) = setup(&games);
    click(&mut rig, slot, ListColumn::White);
    assert_eq!(rig.view(slot), [2, 1, 3]);
}

#[test]
fn rounds_sort_numerically_inside_the_text() {
    let games: Vec<G> = ["10", "2", "3.10", "3.2", "1"]
        .iter()
        .map(|r| G::new("w", "b").round(r))
        .collect();
    let (mut rig, slot, _d) = setup(&games);
    click(&mut rig, slot, ListColumn::Round);
    // Scid strCompareRound: 1, 2, 3.2, 3.10, 10.
    assert_eq!(rig.view(slot), [5, 2, 4, 3, 1]);
}

#[test]
fn a_game_without_a_round_sorts_before_the_numbered_rounds_like_the_scid() {
    // Scid strCompareRound (misc.cpp 323): the integer at the start decides
    // first and "?" or "" read as 0, so they come before round 1.
    let games = vec![
        G::new("w", "b").round("2"),
        G::new("w", "b").round("?"),
        G::new("w", "b").round("1"),
        G::new("w", "b").round(""),
    ];
    let (mut rig, slot, _d) = setup(&games);
    click(&mut rig, slot, ListColumn::Round);
    let view = rig.view(slot);
    let position = |n: u32| view.iter().position(|v| *v == n).expect("listed");
    assert!(
        position(2) < position(3),
        "round ? before round 1: {view:?}"
    );
    assert!(
        position(4) < position(3),
        "empty round before round 1: {view:?}"
    );
    assert!(
        position(3) < position(1),
        "round 1 before round 2: {view:?}"
    );
}

#[test]
fn partial_dates_sort_with_unknown_parts_as_zero() {
    let games: Vec<G> = [
        "2020.05.??",
        "2019.12.31",
        "2020.??.??",
        "????.??.??",
        "2020.01.15",
    ]
    .iter()
    .map(|d| G::new("w", "b").date(d))
    .collect();
    let (mut rig, slot, _d) = setup(&games);
    click(&mut rig, slot, ListColumn::Date);
    // Scid encodes year<<9 | month<<5 | day, unknown = 0.
    assert_eq!(rig.view(slot), [4, 2, 3, 5, 1]);
}

#[test]
fn names_sort_with_the_empty_name_first() {
    let games = vec![G::new("Bob", "x"), G::new("", "x"), G::new("Al", "x")];
    let (mut rig, slot, _d) = setup(&games);
    let rows = rows_in_db_order(&rig, slot);
    let empty_is_stored_empty = rows[1].white.is_empty();
    click(&mut rig, slot, ListColumn::White);
    if empty_is_stored_empty {
        assert_eq!(rig.view(slot), [2, 3, 1]);
    } else {
        // The importer stored a placeholder ("?"): it still sorts before letters.
        assert_eq!(rig.view(slot), [2, 3, 1], "{:?}", rows[1].white);
    }
}

#[test]
fn text_order_folds_ascii_case_and_orders_non_ascii_after_letters() {
    let (mut rig, slot, _d) = setup(&white_names(&["zed", "Émile", "Bravo", "alpha"]));
    click(&mut rig, slot, ListColumn::White);
    assert_eq!(rig.view(slot), [4, 3, 1, 2]);
}

#[test]
fn length_sorts_by_the_number_of_moves() {
    let games = vec![
        G::new("a", "b").moves("1. e4 e5 2. Nf3 Nc6 *"),
        G::new("a", "b").moves("1. e4 *"),
        G::new("a", "b").moves("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 *"),
        G::new("a", "b").moves("1. e4 e5 *"),
    ];
    let (mut rig, slot, _d) = setup(&games);
    click(&mut rig, slot, ListColumn::Length);
    assert_eq!(rig.view(slot), [2, 4, 1, 3]);
}

#[test]
fn eco_sorts_by_code() {
    let games = vec![
        G::new("a", "b").eco("C20"),
        G::new("a", "b").eco("A45"),
        G::new("a", "b"),
        G::new("a", "b").eco("B90"),
    ];
    let (mut rig, slot, _d) = setup(&games);
    click(&mut rig, slot, ListColumn::Eco);
    let rows = rows_in_db_order(&rig, slot);
    assert_eq!(rows[1].eco, "A45", "the importer keeps the ECO tag");
    assert_eq!(rig.view(slot), [3, 2, 4, 1]);
}

#[test]
fn result_puts_unfinished_games_after_the_draws_like_the_scid() {
    // index.cpp 731-740: "draws and no-results sort together": None is
    // mapped to Draw + 1, so the order is 1-0, 0-1, 1/2-1/2, *.
    let games = vec![
        G::new("a", "b").result("*"),
        G::new("a", "b").result("1/2-1/2"),
        G::new("a", "b").result("0-1"),
        G::new("a", "b").result("1-0"),
    ];
    let (mut rig, slot, _d) = setup(&games);
    click(&mut rig, slot, ListColumn::Result);
    assert_eq!(rig.view(slot), [4, 3, 2, 1]);
}

#[test]
fn elo_first_click_puts_the_highest_rating_first_like_the_scid() {
    // index.cpp 748-754: `ie->WhiteElo - WhiteElo`, i.e. the first click
    // ("sortup") lists the highest rating first.
    let games = vec![
        G::new("a", "b").elo(2100, 2300),
        G::new("a", "b").elo(2700, 1900),
        G::new("a", "b").elo(0, 2500),
    ];
    let (mut rig, slot, _d) = setup(&games);
    click(&mut rig, slot, ListColumn::WhiteElo);
    assert_eq!(rig.view(slot), [2, 1, 3], "WElo: 2700, 2100, none");
    click(&mut rig, slot, ListColumn::BlackElo);
    assert_eq!(rig.view(slot), [3, 1, 2], "BElo: 2500, 2300, 1900");
}

#[test]
fn comments_and_variations_first_click_puts_the_largest_count_first_like_the_scid() {
    // index.cpp 795-801: `ie->CommentCount - CommentCount` (and the same for
    // variations): the first click lists the games with most first.
    let games = vec![
        G::new("a", "b").moves("1. e4 e5 *"),
        G::new("a", "b").moves("1. e4 {c} e5 {c} 2. Nf3 {c} Nc6 {c} *"),
        G::new("a", "b").moves("1. e4 {c} e5 *"),
    ];
    let (mut rig, slot, _d) = setup(&games);
    click(&mut rig, slot, ListColumn::Comments);
    assert_eq!(rig.view(slot), [2, 3, 1], "most comments first");
    let games = vec![
        G::new("a", "b").moves("1. e4 e5 *"),
        G::new("a", "b").moves("1. e4 (1. d4) (1. c4) e5 *"),
        G::new("a", "b").moves("1. e4 (1. d4) e5 *"),
    ];
    let (mut rig, slot, _d) = setup(&games);
    click(&mut rig, slot, ListColumn::Variations);
    assert_eq!(rig.view(slot), [2, 3, 1], "most variations first");
}

#[test]
fn country_groups_the_sites_that_end_in_the_same_code() {
    let games = vec![
        G::new("a", "b").site("Rome ITA"),
        G::new("a", "b").site("Oslo NOR"),
        G::new("a", "b").site("Milan ITA"),
        G::new("a", "b").site("Bergen NOR"),
        G::new("a", "b").site("Rome ITA"),
    ];
    let (mut rig, slot, _d) = setup(&games);
    click(&mut rig, slot, ListColumn::Country);
    // ITA before NOR, database order inside each country.
    assert_eq!(rig.view(slot), [1, 3, 5, 2, 4]);
}

#[test]
fn site_and_event_sort_by_their_names() {
    let games = vec![
        G::new("a", "b").site("Rome").event("Zeta"),
        G::new("a", "b").site("Oslo").event("alpha"),
        G::new("a", "b").site("Paris").event("Mu"),
    ];
    let (mut rig, slot, _d) = setup(&games);
    click(&mut rig, slot, ListColumn::Site);
    assert_eq!(rig.view(slot), [2, 3, 1]);
    click(&mut rig, slot, ListColumn::Event);
    assert_eq!(rig.view(slot), [2, 3, 1]);
}

// ---- the view API ----

#[test]
fn the_number_column_shows_the_real_game_number_in_a_sorted_view() {
    let (mut rig, slot, _d) = setup(&white_names(&["C", "A", "B"]));
    click(&mut rig, slot, ListColumn::White);
    let rows = rig.state.bases.list_page(slot, 1, 3);
    let numbers: Vec<u32> = rows.iter().map(|r| r.number).collect();
    let whites: Vec<&str> = rows.iter().map(|r| r.white.as_str()).collect();
    assert_eq!(numbers, [2, 3, 1]);
    assert_eq!(whites, ["A", "B", "C"]);
}

#[test]
fn list_page_pages_through_the_sorted_view() {
    let (mut rig, slot, _d) = setup(&white_names(&["E", "D", "C", "B", "A"]));
    click(&mut rig, slot, ListColumn::White);
    let page = |first, count| -> Vec<u32> {
        rig.state
            .bases
            .list_page(slot, first, count)
            .iter()
            .map(|r| r.number)
            .collect()
    };
    assert_eq!(page(1, 2), [5, 4]);
    assert_eq!(page(2, 3), [4, 3, 2]);
    assert_eq!(page(4, 10), [2, 1]);
    assert_eq!(page(6, 3), Vec::<u32>::new());
    assert_eq!(page(1, 0), Vec::<u32>::new());
}

#[test]
fn a_sorted_view_shows_only_the_games_of_the_filter() {
    let (mut rig, slot, _d) = setup(&white_names(&["E", "D", "C", "B", "A"]));
    click(&mut rig, slot, ListColumn::White);
    rig.select_all(slot, &[1, 3, 4]);
    rig.send(l(ListMessage::KeepSelected { slot }));
    assert_eq!(rig.view(slot), [4, 3, 1]);
    rig.send(l(ListMessage::ResetFilter { slot }));
    assert_eq!(rig.view(slot), [5, 4, 3, 2, 1], "reset keeps the sort");
    click(&mut rig, slot, ListColumn::White);
    assert_eq!(rig.view(slot), [1, 2, 3, 4, 5]);
}

#[test]
fn nth_visible_position_of_and_visible_range_agree_with_the_view() {
    let (mut rig, slot, _d) = setup(&white_names(&["C", "A", "D", "B"]));
    click(&mut rig, slot, ListColumn::White);
    let s = rig.state.bases.slot(slot).expect("slot");
    assert_eq!(s.visible_range(1, 4), [2, 4, 1, 3]);
    assert_eq!(s.visible_range(2, 3), [4, 1]);
    assert_eq!(s.visible_range(3, 99), [1, 3]);
    assert_eq!(s.nth_visible(1), Some(2));
    assert_eq!(s.nth_visible(4), Some(3));
    assert_eq!(s.nth_visible(5), None);
    assert_eq!(s.nth_visible(0), None);
    assert_eq!(s.position_of(3), Some(4));
    assert_eq!(s.position_of(9), None);
    for (i, n) in s.visible_range(1, 4).into_iter().enumerate() {
        assert_eq!(s.position_of(n), Some(u32::try_from(i).unwrap() + 1));
    }
}

#[test]
fn an_unsorted_slot_has_the_database_order_in_the_same_api() {
    let (rig, slot, _d) = setup(&white_names(&["C", "A", "D", "B"]));
    let s = rig.state.bases.slot(slot).expect("slot");
    assert_eq!(s.sort(), None);
    assert_eq!(s.visible_range(1, 9), [1, 2, 3, 4]);
    assert_eq!(s.nth_visible(3), Some(3));
    assert_eq!(s.position_of(4), Some(4));
}

#[test]
fn sorting_goes_to_the_top_and_keeps_the_selection() {
    let (mut rig, slot, _d) = setup(&white_names(&["E", "D", "C", "B", "A"]));
    rig.select_all(slot, &[2, 3]);
    rig.send(BasesMessage::SetListFirst { slot, first: 4 });
    click(&mut rig, slot, ListColumn::White);
    assert_eq!(rig.first(slot), 1);
    assert_eq!(rig.selection(slot), [2, 3]);
}

// ---- per-slot, read-only, Clipbase ----

#[test]
fn the_sort_of_one_slot_does_not_touch_another() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let a = rig.open_games(dir.path(), "a.pgn", &white_names(&["B", "A", "C"]));
    let b = rig.open_games(dir.path(), "b.pgn", &white_names(&["B", "A", "C"]));
    click(&mut rig, a, ListColumn::White);
    assert_eq!(rig.view(a), [2, 1, 3]);
    assert_eq!(rig.view(b), [1, 2, 3]);
    assert_eq!(rig.state.bases.slot(b).expect("slot").sort(), None);
    click(&mut rig, b, ListColumn::White);
    click(&mut rig, b, ListColumn::White);
    assert_eq!(rig.view(a), [2, 1, 3], "a keeps its own direction");
    assert_eq!(rig.view(b), [3, 1, 2]);
    // Switching tabs keeps each sort.
    rig.send(BasesMessage::Select(a));
    rig.send(BasesMessage::Select(b));
    assert_eq!(
        rig.state.bases.slot(a).expect("slot").sort(),
        Some((ListColumn::White, SortDir::Ascending))
    );
    assert_eq!(
        rig.state.bases.slot(b).expect("slot").sort(),
        Some((ListColumn::White, SortDir::Descending))
    );
}

#[test]
fn a_read_only_pgn_sorts_its_view_without_an_error() {
    let (mut rig, slot, _d) = setup(&white_names(&["B", "A"]));
    assert!(rig.state.bases.slot(slot).expect("slot").read_only);
    rig.clear_notice();
    click(&mut rig, slot, ListColumn::White);
    assert_eq!(rig.view(slot), [2, 1]);
    assert_eq!(rig.notice(), None);
}

#[test]
fn a_database_of_one_game_or_an_empty_clipbase_sorts_without_panicking() {
    let (mut rig, slot, _d) = setup(&white_names(&["Only"]));
    click(&mut rig, slot, ListColumn::White);
    assert_eq!(rig.view(slot), [1]);
    click(&mut rig, SlotId::CLIPBASE, ListColumn::White);
    assert_eq!(rig.view(SlotId::CLIPBASE), Vec::<u32>::new());
    click(&mut rig, SlotId::CLIPBASE, ListColumn::White);
}

#[test]
fn the_clipbase_sorts_and_loses_the_sort_when_it_grows() {
    let (mut rig, slot, _d) = setup(&white_names(&["D", "B", "C", "A"]));
    rig.select_all(slot, &[1, 2]);
    rig.send(l(ListMessage::CopySelected { slot }));
    assert_eq!(rig.view(SlotId::CLIPBASE), [1, 2]);
    click(&mut rig, SlotId::CLIPBASE, ListColumn::White);
    assert_eq!(rig.view(SlotId::CLIPBASE), [2, 1], "B before D");
    rig.select_all(slot, &[3, 4]);
    rig.send(l(ListMessage::CopySelected { slot }));
    let clip = rig.state.bases.slot(SlotId::CLIPBASE).expect("clipbase");
    assert_eq!(clip.total(), 4);
    assert_eq!(clip.sort(), None, "the Clipbase lost its sort (D558)");
    assert_eq!(rig.view(SlotId::CLIPBASE), [1, 2, 3, 4]);
}

// ---- jobs ----

#[test]
fn a_cancelled_sort_keeps_the_previous_view_and_leaves_no_job() {
    let (mut rig, slot, _d) = setup(&white_names(&["C", "A", "B"]));
    click(&mut rig, slot, ListColumn::White);
    assert_eq!(rig.view(slot), [2, 3, 1]);
    let mut effects = rig.raw(l(ListMessage::SortBy {
        slot,
        column: ListColumn::Event,
    }));
    assert_eq!(
        rig.state.list_job().map(|(k, _)| k),
        Some(ListJobKind::Sort(ListColumn::Event))
    );
    rig.raw(l(ListMessage::CancelJob));
    let answer = rig.perform(effects.remove(0));
    rig.send(answer);
    assert!(rig.state.list_job().is_none());
    assert_eq!(
        rig.state.bases.slot(slot).expect("slot").sort(),
        Some((ListColumn::White, SortDir::Ascending))
    );
    assert_eq!(rig.view(slot), [2, 3, 1]);
}

#[test]
fn a_sort_in_flight_blocks_a_second_sort_and_filter_changes() {
    let (mut rig, slot, _d) = setup(&white_names(&["C", "A", "B"]));
    let _held = rig.raw(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    for message in [
        ListMessage::SortBy {
            slot,
            column: ListColumn::Event,
        },
        ListMessage::ResetFilter { slot },
        ListMessage::NegateFilter { slot },
    ] {
        rig.clear_notice();
        assert!(rig.raw(l(message.clone())).is_empty(), "{message:?}");
        assert_eq!(rig.notice(), Some(&BasesNotice::ListBusy), "{message:?}");
    }
}

#[test]
fn closing_the_slot_during_a_sort_drops_the_answer() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let slot = rig.open_games(dir.path(), "a.pgn", &white_names(&["C", "A", "B"]));
    let mut effects = rig.raw(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    let answer = rig.perform(effects.remove(0));
    rig.send(BasesMessage::Close(slot));
    let again = rig.open_games(dir.path(), "b.pgn", &white_names(&["Z", "Y", "X"]));
    assert_eq!(again, slot);
    rig.send(answer);
    assert_eq!(
        rig.state.bases.slot(slot).expect("slot").sort(),
        None,
        "a stale order must not be applied to the new database"
    );
    assert_eq!(rig.view(slot), [1, 2, 3]);
}

#[test]
fn a_database_over_16384_games_sorts_in_several_chunks_exactly() {
    // 17,000 games (more than one chunk of 16,384 index entries) whose
    // White names are a permutation: i * 7919 mod 17000 (7919 is prime and
    // coprime to 17000).
    const N: u32 = 17_000;
    let games: Vec<G> = (0..N)
        .map(|i| G::new(&format!("W{:05}", (i * 7919) % N), "b"))
        .collect();
    let (mut rig, slot, _d) = setup(&games);
    assert_eq!(rig.state.bases.slot(slot).expect("slot").total(), N);
    let mut effects = rig.raw(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    let Effect::SortList { control, .. } = &effects[0] else {
        panic!("expected SortList");
    };
    let control = control.clone();
    let answer = rig.perform(effects.remove(0));
    assert_eq!(control.progress(), (u64::from(N), u64::from(N)));
    rig.send(answer);
    let mut model: Vec<u32> = (1..=N).collect();
    model.sort_by_key(|n| ((n - 1) * 7919) % N);
    assert_eq!(rig.view(slot), model);
}

#[test]
fn a_sort_over_16384_games_can_be_cancelled() {
    const N: u32 = 17_000;
    let games: Vec<G> = (0..N).map(|i| G::new(&format!("W{i:05}"), "b")).collect();
    let (mut rig, slot, _d) = setup(&games);
    let mut effects = rig.raw(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    rig.raw(l(ListMessage::CancelJob));
    let answer = rig.perform(effects.remove(0));
    rig.send(answer);
    assert_eq!(rig.state.bases.slot(slot).expect("slot").sort(), None);
    assert!(rig.state.list_job().is_none());
}

// ---- property: the sort equals a naive model ----

const NAMES: [&str; 8] = [
    "", "?", "alpha", "Bravo", "charlie", "Delta", "Émile", "zed",
];
const SITES: [&str; 6] = [
    "",
    "Rome ITA",
    "Oslo NOR",
    "Paris FRA",
    "Kyiv",
    "Bergen NOR",
];
const ROUNDS: [&str; 5] = ["1", "2", "10", "3.2", "3.10"];
const DATES: [&str; 6] = [
    "????.??.??",
    "2020.??.??",
    "2020.05.??",
    "2020.05.17",
    "2019.12.31",
    "1999.01.01",
];
/// (movetext, half moves, comments, variations)
const MOVES: [(&str, u32); 4] = [
    ("1. e4 *", 1),
    ("1. e4 e5 *", 2),
    ("1. e4 e5 2. Nf3 Nc6 *", 4),
    ("1. d4 d5 2. c4 e6 3. Nc3 Nf6 *", 6),
];
const ECOS: [&str; 5] = ["", "A00", "B90", "C20", "E99"];

#[derive(Debug, Clone)]
struct Spec {
    white: usize,
    black: usize,
    event: usize,
    site: usize,
    round: usize,
    date: usize,
    moves: usize,
    welo: u16,
    belo: u16,
    eco: usize,
    comments: usize,
    variations: usize,
}

impl Spec {
    fn game(&self) -> G {
        let comments = "{c} ".repeat(self.comments);
        let variation = if self.variations > 0 {
            "(1. c4) ".repeat(self.variations)
        } else {
            String::new()
        };
        let moves = format!("1. e4 {comments}{variation}e5 *");
        let mut g = G::new(NAMES[self.white], NAMES[self.black])
            .event(NAMES[self.event])
            .site(SITES[self.site])
            .round(ROUNDS[self.round])
            .date(DATES[self.date])
            .elo(self.welo, self.belo)
            .eco(ECOS[self.eco]);
        g.moves = if self.comments + self.variations > 0 {
            moves
        } else {
            MOVES[self.moves].0.into()
        };
        g
    }

    fn half_moves(&self) -> u32 {
        if self.comments + self.variations > 0 {
            2
        } else {
            MOVES[self.moves].1
        }
    }
}

fn spec() -> impl Strategy<Value = Spec> {
    (
        (
            0..NAMES.len(),
            0..NAMES.len(),
            0..NAMES.len(),
            0..SITES.len(),
        ),
        (
            0..ROUNDS.len(),
            0..DATES.len(),
            0..MOVES.len(),
            0..ECOS.len(),
        ),
        (
            prop_oneof![Just(0u16), 1000u16..2800],
            prop_oneof![Just(0u16), 1000u16..2800],
        ),
        (0usize..3, 0usize..3),
    )
        .prop_map(
            |(
                (white, black, event, site),
                (round, date, moves, eco),
                (welo, belo),
                (comments, variations),
            )| Spec {
                white,
                black,
                event,
                site,
                round,
                date,
                moves,
                welo,
                belo,
                eco,
                comments,
                variations,
            },
        )
}

fn text_key(s: &str) -> Vec<i64> {
    s.bytes()
        .map(|b| i64::from(b.to_ascii_lowercase()))
        .collect()
}

fn leading_int(s: &str) -> i64 {
    s.chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

fn date_key(s: &str) -> Vec<i64> {
    let mut parts: Vec<i64> = s.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    parts.resize(3, 0);
    parts
}

/// Direction-agnostic columns: the Scid lists the largest first.
fn either_direction(column: ListColumn) -> bool {
    matches!(
        column,
        ListColumn::WhiteElo | ListColumn::BlackElo | ListColumn::Variations | ListColumn::Comments
    )
}

fn key(column: ListColumn, row: &GameRow, spec: &Spec) -> Vec<i64> {
    match column {
        ListColumn::White => text_key(&row.white),
        ListColumn::Black => text_key(&row.black),
        ListColumn::Event => text_key(&row.event),
        ListColumn::Site => text_key(&row.site),
        ListColumn::Round => {
            let (a, b) = row
                .round
                .split_once('.')
                .unwrap_or((row.round.as_str(), ""));
            vec![leading_int(a), leading_int(b)]
        }
        ListColumn::Date => date_key(&row.date),
        ListColumn::Length => vec![i64::from(spec.half_moves())],
        ListColumn::WhiteElo => vec![i64::from(row.white_elo)],
        ListColumn::BlackElo => vec![i64::from(row.black_elo)],
        ListColumn::Eco => text_key(&row.eco),
        ListColumn::Variations => vec![i64::from(row.variations)],
        ListColumn::Comments => vec![i64::from(row.comments)],
        other => panic!("not modelled: {other:?}"),
    }
}

const MODELLED: [ListColumn; 12] = [
    ListColumn::White,
    ListColumn::Black,
    ListColumn::Event,
    ListColumn::Site,
    ListColumn::Round,
    ListColumn::Date,
    ListColumn::Length,
    ListColumn::WhiteElo,
    ListColumn::BlackElo,
    ListColumn::Eco,
    ListColumn::Variations,
    ListColumn::Comments,
];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn the_sorted_view_equals_a_naive_stable_sort_of_the_rows(
        specs in proptest::collection::vec(spec(), 2..14),
        column_index in 0..MODELLED.len(),
    ) {
        let column = MODELLED[column_index];
        let games: Vec<G> = specs.iter().map(Spec::game).collect();
        let (mut rig, slot, _d) = setup(&games);
        let rows = rows_in_db_order(&rig, slot);
        prop_assert_eq!(rows.len(), specs.len());
        let mut asc: Vec<(Vec<i64>, u32)> = rows
            .iter()
            .zip(&specs)
            .map(|(row, spec)| (key(column, row, spec), row.number))
            .collect();
        let mut desc = asc.clone();
        asc.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        desc.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let asc: Vec<u32> = asc.into_iter().map(|(_, n)| n).collect();
        let desc: Vec<u32> = desc.into_iter().map(|(_, n)| n).collect();

        click(&mut rig, slot, column);
        let first = rig.view(slot);
        let flipped_expected: Vec<u32> = {
            let mut v = first.clone();
            v.reverse();
            v
        };
        if either_direction(column) {
            prop_assert!(first == asc || first == desc, "{:?}: {:?} vs {:?} / {:?}", column, first, asc, desc);
        } else {
            prop_assert_eq!(&first, &asc, "{:?}", column);
        }
        click(&mut rig, slot, column);
        prop_assert_eq!(rig.view(slot), flipped_expected);
    }
}
