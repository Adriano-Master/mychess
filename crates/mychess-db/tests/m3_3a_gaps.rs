//! Gaps left by the M3-3a contract tests (spec M3-3: D329, D343, D367, D374,
//! D375, D388-D390): extreme rating differences with "ignore colors",
//! `textfind_with` / `textfilter_with` on a real database (cancellation,
//! progress, unreadable entries), the cap of `unreadable`, `textfind` from a
//! position beyond the filter and `post_comment` on games without moves.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::similar_names
)]

#[path = "common/m3_3a_support.rs"]
mod support;

use mychess_core::{Game, GameResult};
use mychess_db::si4::{GameFlags, PreparedGame, Si4Writer};
use mychess_db::{
    CancelToken, Filter, FilterOp, GameEndKind, HeaderSearch, MAX_UNREADABLE, MinMax, NoProgress,
    PlayerStats, search_header,
};
use pretty_assertions::assert_eq;
use rstest::rstest;
use support::{B, Fixture, break_entry, build, fen_game, hits, hits_report, q};

fn with(f: impl FnOnce(&mut HeaderSearch)) -> HeaderSearch {
    let mut s = q();
    f(&mut s);
    s
}

/// `n` identical games "Kasparov, Garry" - "Karpov" written in one batch.
fn big_db(n: usize) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("big");
    let one = PreparedGame::from_game(&B::new("Kasparov, Garry", "Karpov").0, GameFlags::empty())
        .unwrap();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_prepared_batch(&vec![one; n]).unwrap();
    drop(w);
    Fixture { dir, base }
}

// ====================================================================
// elo_diff extremes with ignore_colors
// ====================================================================

fn rated_db() -> Fixture {
    build(&[
        // Kasparov as White, +200
        B::new("Kasparov", "Karpov")
            .elo(2800, 2600)
            .result(GameResult::White)
            .done(),
        // Kasparov as Black, -200
        B::new("Karpov", "Kasparov")
            .elo(2600, 2800)
            .result(GameResult::Black)
            .done(),
        // no Elo anywhere for these two
        B::new("Nobody", "Else").done(),
    ])
}

#[rstest]
#[case(i32::MIN, i32::MAX)]
#[case(i32::MAX, i32::MIN)]
fn extreme_elo_diff_is_no_restriction_and_does_not_panic_with_ignore_colors(
    #[case] min: i32,
    #[case] max: i32,
) {
    let fx = rated_db();
    for ignore_colors in [false, true] {
        let got = hits(
            &fx,
            &with(|s| {
                s.elo_diff = MinMax::new(min, max);
                s.ignore_colors = ignore_colors;
                s.white = "Kasparov".into();
            }),
        );
        // plain: only the game where he is White; ignoring colors: both
        let expected: &[u32] = if ignore_colors { &[1, 2] } else { &[1] };
        assert_eq!(got, expected, "ignore_colors = {ignore_colors}");
    }
}

#[rstest]
#[case(i32::MIN, i32::MIN)]
#[case(i32::MAX, i32::MAX)]
fn degenerate_extreme_elo_diff_matches_nothing_and_does_not_panic(
    #[case] min: i32,
    #[case] max: i32,
) {
    let fx = rated_db();
    for ignore_colors in [false, true] {
        let got = hits(
            &fx,
            &with(|s| {
                s.elo_diff = MinMax::new(min, max);
                s.ignore_colors = ignore_colors;
            }),
        );
        assert_eq!(got, Vec::<u32>::new(), "ignore_colors = {ignore_colors}");
    }
}

#[test]
fn elo_diff_bound_at_i32_min_with_ignore_colors_still_selects_by_difference() {
    // lower end at i32::MIN, upper end -1: only games where White is weaker
    // (diff < 0), or, ignoring colors, those that are mirrored into it.
    let fx = rated_db();
    let strict = hits(&fx, &with(|s| s.elo_diff = MinMax::new(i32::MIN, -1)));
    assert_eq!(strict, [2]);
    let mirrored = hits(
        &fx,
        &with(|s| {
            s.elo_diff = MinMax::new(i32::MIN, -1);
            s.ignore_colors = true;
        }),
    );
    // game 1 has diff +200, its mirror -200 is inside; game 3 has no Elo
    // (diff 0) and is outside both ways.
    assert_eq!(mirrored, [1, 2]);
}

// ====================================================================
// textfind / textfilter with progress and cancellation
// ====================================================================

#[test]
fn textfind_from_a_position_beyond_count_finds_nothing() {
    let fx = build(&[
        B::new("Kasparov", "Karpov").done(),
        B::new("Kasparov", "Karpov").done(),
        B::new("Kasparov", "Karpov").done(),
    ]);
    let db = fx.open();
    let filter = Filter::new(db.len());
    assert_eq!(filter.textfind(&db, "Kasparov", false, 1), Some(2));
    assert_eq!(filter.textfind(&db, "Kasparov", false, 2), Some(3));
    // from the last one there is nothing after it
    assert_eq!(filter.textfind(&db, "Kasparov", false, 3), None);
    // beyond the filter: no match, no panic, not a wrap-around
    assert_eq!(filter.textfind(&db, "Kasparov", false, 4), None);
    assert_eq!(filter.textfind(&db, "Kasparov", false, 1_000), None);
    assert_eq!(filter.textfind(&db, "Kasparov", false, u32::MAX), None);
    let report = filter.textfind_with(
        &db,
        "Kasparov",
        false,
        99,
        &mut NoProgress,
        &CancelToken::new(),
    );
    assert_eq!(report.position, None);
    assert!(!report.interrupted);
    assert_eq!(report.unreadable_total, 0);
}

#[test]
fn textfind_beyond_count_in_a_partial_filter() {
    let fx = build(&[
        B::new("Kasparov", "Karpov").done(),
        B::new("Kasparov", "Karpov").done(),
        B::new("Kasparov", "Karpov").done(),
        B::new("Kasparov", "Karpov").done(),
    ]);
    let db = fx.open();
    let mut filter = Filter::new(db.len());
    filter.set(1, 0);
    filter.set(3, 0);
    // count = 2 (games 2 and 4): positions 1 and 2
    assert_eq!(filter.textfind(&db, "Kasparov", false, 0), Some(1));
    assert_eq!(filter.textfind(&db, "Kasparov", false, 1), Some(2));
    assert_eq!(filter.textfind(&db, "Kasparov", false, 2), None);
    assert_eq!(filter.textfind(&db, "Kasparov", false, 3), None);
}

#[test]
fn textfind_with_progress_is_monotonic_and_bounded_by_the_count() {
    let n = 12_000u32;
    let fx = big_db(n as usize);
    let db = fx.open();
    let filter = Filter::new(n);
    let mut seen: Vec<(u64, u64)> = Vec::new();
    let mut sink = |d: u64, t: u64| seen.push((d, t));
    let r = filter.textfind_with(
        &db,
        "no such text",
        false,
        0,
        &mut sink,
        &CancelToken::new(),
    );
    assert_eq!(r.position, None);
    assert!(!r.interrupted);
    assert!(!seen.is_empty(), "5000-game threshold must report progress");
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "{seen:?}");
    for (done, total) in &seen {
        assert_eq!(*total, u64::from(n));
        assert!(done <= total);
    }
}

#[test]
fn textfind_with_cancel_stops_without_a_position() {
    let n = 12_000u32;
    let fx = big_db(n as usize);
    let db = fx.open();
    let filter = Filter::new(n);
    let token = CancelToken::new();
    let t2 = token.clone();
    let mut sink = move |_: u64, _: u64| t2.cancel();
    let r = filter.textfind_with(&db, "no such text", false, 0, &mut sink, &token);
    assert!(r.interrupted);
    assert_eq!(r.position, None);
    // textfind never changes the filter
    assert_eq!(filter.count(), n);
}

#[test]
fn textfind_with_lists_unreadable_entries() {
    let fx = build(&[
        B::new("a", "b").done(),
        B::new("a", "b").done(),
        B::new("Kasparov", "b").done(),
        B::new("a", "b").done(),
    ]);
    break_entry(&fx, 1);
    break_entry(&fx, 2);
    let db = fx.open();
    let filter = Filter::new(db.len());
    let r = filter.textfind_with(
        &db,
        "Kasparov",
        false,
        0,
        &mut NoProgress,
        &CancelToken::new(),
    );
    assert_eq!(r.position, Some(3));
    assert_eq!(r.unreadable, [1, 2]);
    assert_eq!(r.unreadable_total, 2);
    assert!(!r.interrupted);
    // the plain wrapper gives the same position and does not report
    assert_eq!(filter.textfind(&db, "Kasparov", false, 0), Some(3));
}

#[test]
fn textfilter_with_progress_ends_at_count_count() {
    let n = 12_000u32;
    let fx = big_db(n as usize);
    let db = fx.open();
    let mut filter = Filter::new(n);
    // leave a partial filter so that count != db.len()
    filter.set(2, 0);
    filter.set(7, 0);
    let count = u64::from(filter.count());
    let mut seen: Vec<(u64, u64)> = Vec::new();
    let mut sink = |d: u64, t: u64| seen.push((d, t));
    let r = filter.textfilter_with(&db, "Kasparov", false, &mut sink, &CancelToken::new());
    assert_eq!(r.removed, 0);
    assert!(!r.interrupted);
    assert_eq!(seen.last().copied(), Some((count, count)), "{seen:?}");
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "{seen:?}");
    assert!(seen.iter().all(|(d, t)| *t == count && d <= t));
    assert!(
        seen.len() >= 2,
        "one report at 5000 games and the final one"
    );
}

#[test]
fn textfilter_with_cancel_leaves_the_filter_intact_and_removed_zero() {
    let n = 12_000u32;
    let fx = big_db(n as usize);
    let db = fx.open();
    let mut filter = Filter::new(n);
    filter.set(10, 77);
    let before = filter.clone();
    let token = CancelToken::new();
    let t2 = token.clone();
    let mut sink = move |_: u64, _: u64| t2.cancel();
    // "zzz" matches nothing: an uncancelled run would remove every game
    let r = filter.textfilter_with(&db, "zzz", false, &mut sink, &token);
    assert!(r.interrupted);
    assert_eq!(r.removed, 0);
    assert_eq!(filter, before);
}

#[test]
fn textfilter_with_already_cancelled_token_does_nothing() {
    let fx = build(&[B::new("a", "b").done(), B::new("c", "d").done()]);
    let db = fx.open();
    let mut filter = Filter::new(db.len());
    let before = filter.clone();
    let token = CancelToken::new();
    token.cancel();
    let r = filter.textfilter_with(&db, "zzz", false, &mut NoProgress, &token);
    assert!(r.interrupted);
    assert_eq!(r.removed, 0);
    assert_eq!(filter, before);
}

#[test]
fn textfilter_with_reports_unreadable_entries_and_removes_them() {
    let fx = build(&[
        B::new("Kasparov", "x").done(),
        B::new("Kasparov", "x").done(),
        B::new("Kasparov", "x").done(),
        B::new("Other", "x").done(),
    ]);
    break_entry(&fx, 2);
    let db = fx.open();
    let mut filter = Filter::new(db.len());
    filter.set(1, 9);
    let r = filter.textfilter_with(&db, "Kasparov", false, &mut NoProgress, &CancelToken::new());
    assert_eq!(r.unreadable, [2]);
    assert_eq!(r.unreadable_total, 1);
    // game 2 (unreadable: "does not match") and game 4 ("Other") go
    assert_eq!(r.removed, 2);
    assert_eq!(filter.iter_games().collect::<Vec<_>>(), [1, 3]);
    assert_eq!(filter.ply(1), 8, "ply of the survivors is kept");
}

#[test]
fn textfilter_with_caps_unreadable_numbers_but_counts_them_all() {
    let total_games = MAX_UNREADABLE + 50;
    let fx = big_db(total_games);
    for g in 1..=total_games as u32 {
        break_entry(&fx, g);
    }
    let db = fx.open();
    let mut filter = Filter::new(db.len());
    let r = filter.textfilter_with(&db, "Kasparov", false, &mut NoProgress, &CancelToken::new());
    assert_eq!(r.unreadable.len(), MAX_UNREADABLE);
    assert_eq!(r.unreadable_total, total_games as u64);
    assert_eq!(r.unreadable[0], 1, "the first ones are kept");
    assert_eq!(r.removed as usize, total_games);
}

#[test]
fn textfind_with_caps_unreadable_numbers_but_counts_them_all() {
    let total_games = MAX_UNREADABLE + 50;
    let fx = big_db(total_games);
    for g in 1..=total_games as u32 {
        break_entry(&fx, g);
    }
    let db = fx.open();
    let filter = Filter::new(db.len());
    let r = filter.textfind_with(
        &db,
        "Kasparov",
        false,
        0,
        &mut NoProgress,
        &CancelToken::new(),
    );
    assert_eq!(r.position, None);
    assert_eq!(r.unreadable.len(), MAX_UNREADABLE);
    assert_eq!(r.unreadable_total, total_games as u64);
}

// ====================================================================
// SearchReport::unreadable cap (D390)
// ====================================================================

#[test]
fn search_report_caps_unreadable_at_max_and_counts_the_total() {
    assert_eq!(MAX_UNREADABLE, 1000);
    let total_games = 1100usize;
    let fx = big_db(total_games);
    // break all but game 1000..=1100 -> 1100 broken entries in this run
    for g in 1..=total_games as u32 {
        break_entry(&fx, g);
    }
    let (got, r) = hits_report(&fx, &q());
    assert!(got.is_empty(), "no readable entry is left");
    assert_eq!(r.unreadable.len(), 1000);
    assert_eq!(r.unreadable_total, 1100);
    let expected: Vec<u32> = (1..=1000).collect();
    assert_eq!(
        r.unreadable, expected,
        "the first numbers are the kept ones"
    );
    assert!(!r.interrupted);
}

#[test]
fn search_report_exactly_at_the_cap_has_matching_list_and_total() {
    let fx = big_db(1001);
    for g in 1..=1000u32 {
        break_entry(&fx, g);
    }
    let (got, r) = hits_report(&fx, &q());
    assert_eq!(got, [1001]);
    assert_eq!(r.unreadable.len(), 1000);
    assert_eq!(r.unreadable_total, 1000);
}

#[test]
fn search_report_one_over_the_cap_is_counted_but_not_listed() {
    let fx = big_db(1001);
    for g in 1..=1001u32 {
        break_entry(&fx, g);
    }
    let (_, r) = hits_report(&fx, &q());
    assert_eq!(r.unreadable.len(), 1000);
    assert_eq!(r.unreadable_total, 1001);
    assert!(!r.unreadable.contains(&1001));
}

#[test]
fn search_with_and_reset_over_a_damaged_big_db_still_reports_the_same_way() {
    // Same cap under OR and AND, not only under RESET.
    let fx = big_db(1100);
    for g in 1..=1100u32 {
        break_entry(&fx, g);
    }
    for op in [FilterOp::And, FilterOp::Or] {
        let mut db = fx.open();
        let stats = PlayerStats::from_database(&db);
        let mut f = Filter::new(db.len());
        if op == FilterOp::Or {
            // OR only visits games that are still outside the filter
            f.fill(0);
        }
        let r = search_header(
            &mut db,
            &with(|s| s.op = op),
            &mut f,
            &stats,
            None,
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(r.unreadable.len(), 1000, "{op:?}");
        assert_eq!(r.unreadable_total, 1100, "{op:?}");
    }
}

// ====================================================================
// post_comment on games without moves (D375)
// ====================================================================

fn no_moves_db() -> Fixture {
    let mut visible = Game::new();
    visible.set_initial_comment("root words");
    let mut hidden = Game::new();
    hidden.set_initial_comment("[hidden]");
    let bare = Game::new();
    build(&[
        (visible, GameFlags::empty()),
        (hidden, GameFlags::empty()),
        (bare, GameFlags::empty()),
    ])
}

#[test]
fn post_comment_on_games_without_moves_uses_the_root_comment() {
    let fx = no_moves_db();
    // only the visible root comment counts; hidden and absent do not
    assert_eq!(hits(&fx, &with(|s| s.post_comment = true)), [1]);
    // the same root comment is also the "pre-game" comment
    assert_eq!(hits(&fx, &with(|s| s.pre_comment = true)), [1]);
    assert_eq!(
        hits(
            &fx,
            &with(|s| {
                s.pre_comment = true;
                s.post_comment = true;
            })
        ),
        [1]
    );
}

#[test]
fn game_end_on_a_game_without_moves_tests_the_start_position() {
    // D375: the end of a game with no moves is its root position.
    let stalemate = fen_game("7k/5Q2/8/8/8/8/8/1K6 b - - 0 1", &[]);
    let mate = fen_game("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1", &[]);
    let plain = Game::new();
    let fx = build(&[
        (stalemate, GameFlags::empty()),
        (mate, GameFlags::empty()),
        (plain, GameFlags::empty()),
    ]);
    assert_eq!(
        hits(&fx, &with(|s| s.game_end = GameEndKind::Stalemate)),
        [1]
    );
    assert_eq!(
        hits(&fx, &with(|s| s.game_end = GameEndKind::Checkmate)),
        [2]
    );
    assert_eq!(
        hits(&fx, &with(|s| s.game_end = GameEndKind::Any)),
        [1, 2, 3]
    );
}
