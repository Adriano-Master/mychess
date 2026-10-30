//! Contract tests of the header search (spec M3-3, "Busca por cabeçalho":
//! behaviour 8-12, rules 16-41, D328-D344 and D363-D375, plus the acceptance
//! criteria). Derived from the spec and from Scid's `sc_search_header`, not
//! from the implementation.
#![allow(
    clippy::assertions_on_result_states,
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::similar_names
)]

#[path = "common/m3_3a_support.rs"]
mod support;

use std::collections::HashMap;

use mychess_core::{Eco, Game, GameResult, Nag, PartialDate};
use mychess_db::si4::{GameFlags, PreparedGame, Si4Database, Si4Writer};
use mychess_db::{
    CancelToken, EcoRange, Filter, FilterOp, GameEndKind, HeaderSearch, MinMax, NoProgress,
    PlayerStats, ProgressSink, ResultSet, SearchError, Title, TitleProvider, TitleSet, Tri,
    complete_max_date, complete_min_date, search_header,
};
use mychess_pgn::write::{WriteOptions, game_to_string};
use pretty_assertions::assert_eq;
use rstest::rstest;
use support::{
    B, Fixture, break_entry, break_offset, build, fen_game, fools_mate, hits, hits_report, line, q,
    run, stalemate_game, zero_length,
};

fn eco(code: &str) -> Eco {
    Eco::parse_prefix(code).unwrap().0
}

fn d(y: Option<u16>, m: Option<u8>, day: Option<u8>) -> PartialDate {
    PartialDate::new(y, m, day).unwrap()
}

fn search_with(
    fx: &Fixture,
    query: &HeaderSearch,
    filter: &mut Filter,
    stats: &PlayerStats,
    titles: Option<&dyn TitleProvider>,
) -> mychess_db::SearchReport {
    let mut db = fx.open();
    search_header(
        &mut db,
        query,
        filter,
        stats,
        titles,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap()
}

fn plain(n: usize) -> Vec<(Game, GameFlags)> {
    (1..=n)
        .map(|i| B::new(&format!("Player {i}"), "Opponent").done())
        .collect()
}

// ====================================================================
// Defaults, report, filter operation
// ====================================================================

#[test]
fn default_query_is_the_scid_dialog_default() {
    let d = HeaderSearch::default();
    assert_eq!(d.op, FilterOp::Reset);
    assert_eq!(d.results, ResultSet::ALL);
    assert!(d.results.white && d.results.draw && d.results.black && d.results.none);
    assert_eq!((d.white_elo.min, d.white_elo.max), (0, 4000));
    assert_eq!((d.black_elo.min, d.black_elo.max), (0, 4000));
    assert_eq!((d.elo_diff.min, d.elo_diff.max), (-4000, 4000));
    assert_eq!((d.game_number.min, d.game_number.max), (1, -1));
    assert_eq!(d.date.min, PartialDate::UNKNOWN);
    assert_eq!(d.date.max, d_ymd(2047, 12, 31));
    assert_eq!(d.white_titles, TitleSet::all());
    assert_eq!(d.black_titles, TitleSet::all());
    assert!(d.end_white_to_move && d.end_black_to_move);
    assert_eq!(d.game_end, GameEndKind::Any);
    assert!(d.eco.is_none() && d.length.is_none());
    assert!(d.pgn_text.is_empty() && !d.ignore_case && !d.ignore_colors);
    assert!(!d.pre_comment && !d.post_comment);
    for s in [
        &d.white,
        &d.black,
        &d.event,
        &d.site,
        &d.round,
        &d.extra_tag,
        &d.extra_tag_value,
    ] {
        assert!(s.is_empty());
    }
}

fn d_ymd(y: u16, m: u8, day: u8) -> PartialDate {
    d(Some(y), Some(m), Some(day))
}

#[test]
fn default_search_matches_every_game_with_a_record_and_sets_one() {
    let fx = build(&plain(6));
    zero_length(&fx, 4);
    let mut f = Filter::new(6);
    f.fill(200);
    let r = run(&fx, &q(), &mut f);
    assert_eq!(f.iter_games().collect::<Vec<_>>(), [1, 2, 3, 5, 6]);
    assert!(
        (1..=6).filter(|g| *g != 4).all(|g| f.get(g) == 1),
        "ply reset to 1"
    );
    assert_eq!(f.get(4), 0, "Length = 0 leaves the filter (rule 12)");
    assert_eq!((r.matched, r.start_count), (5, 6));
    assert!(!r.interrupted);
    assert!(r.unreadable.is_empty());
}

#[test]
fn search_in_an_empty_database_matches_nothing() {
    let fx = build(&[]);
    let mut f = Filter::new(0);
    let r = run(&fx, &q(), &mut f);
    assert_eq!((r.matched, r.start_count, r.interrupted), (0, 0, false));
}

#[test]
fn and_keeps_the_ply_of_games_that_still_match_and_drops_the_rest() {
    let fx = build(&[
        B::new("Kasparov, Garry", "Karpov").done(),
        B::new("Smith", "Doe").done(),
        B::new("Kasparov, Garry", "Anand").done(),
        B::new("Kasparov, Garry", "Short").done(),
    ]);
    let mut f = Filter::new(4);
    f.set(1, 9);
    f.set(3, 255);
    f.set(4, 0); // matches the query but is out: AND must not bring it back
    let mut query = q();
    query.white = "Kasparov".into();
    query.op = FilterOp::And;
    let r = run(&fx, &query, &mut f);
    assert_eq!((f.get(1), f.get(2), f.get(3), f.get(4)), (9, 0, 255, 0));
    assert_eq!(r.start_count, 3, "AND: the count before is the filter's");
    assert_eq!(r.matched, 2);
}

#[test]
fn or_adds_matches_with_ply_one_and_leaves_existing_games_alone() {
    let fx = build(&[
        B::new("Kasparov, Garry", "Karpov").done(),
        B::new("Smith", "Doe").done(),
        B::new("Kasparov, Garry", "Anand").done(),
        B::new("Nobody", "Else").done(),
    ]);
    let mut f = Filter::new(4);
    f.fill(0);
    f.set(2, 7); // already in, does not match: stays as it was
    f.set(3, 40); // already in and matches: keeps its ply
    let mut query = q();
    query.white = "Kasparov".into();
    query.op = FilterOp::Or;
    let r = run(&fx, &query, &mut f);
    assert_eq!((f.get(1), f.get(2), f.get(3), f.get(4)), (1, 7, 40, 0));
    assert_eq!(f.count(), 3);
    assert_eq!(r.matched, 3);
    assert_eq!(
        r.start_count, 4,
        "OR: the count before is the number of games"
    );
}

#[test]
fn reset_forgets_every_ply_even_of_games_that_keep_matching() {
    let fx = build(&[
        B::new("Kasparov, Garry", "Karpov").done(),
        B::new("Smith", "Doe").done(),
    ]);
    let mut f = Filter::new(2);
    f.set(1, 200);
    f.set(2, 0);
    let mut query = q();
    query.white = "Kasparov".into();
    query.op = FilterOp::Reset;
    let r = run(&fx, &query, &mut f);
    assert_eq!((f.get(1), f.get(2)), (1, 0));
    assert_eq!(r.start_count, 2);
}

#[test]
fn or_with_a_game_range_does_not_remove_games_that_were_already_in() {
    let fx = build(&plain(5));
    let mut f = Filter::new(5);
    f.fill(0);
    f.set(1, 5);
    let mut query = q();
    query.op = FilterOp::Or;
    query.game_number = MinMax::new(3, 4);
    run(&fx, &query, &mut f);
    assert_eq!(f.iter_games().collect::<Vec<_>>(), [1, 3, 4]);
    assert_eq!(f.get(1), 5);
}

#[test]
fn and_with_a_game_range_removes_games_outside_it() {
    let fx = build(&plain(5));
    let mut f = Filter::new(5);
    f.set(2, 99);
    let mut query = q();
    query.op = FilterOp::And;
    query.game_number = MinMax::new(2, 3);
    run(&fx, &query, &mut f);
    assert_eq!(f.iter_games().collect::<Vec<_>>(), [2, 3]);
    assert_eq!(f.get(2), 99);
}

#[test]
fn search_report_has_elapsed_and_counts() {
    let fx = build(&plain(3));
    let (games, r) = hits_report(&fx, &q());
    assert_eq!(games.len() as u32, r.matched);
    assert_eq!(r.start_count, 3);
    assert!(r.elapsed.as_secs() < 60);
}

// ====================================================================
// Names
// ====================================================================

fn names_db() -> Fixture {
    build(&[
        B::new("Kasparov, Garry", "Karpov, Anatoly")
            .event("Linares")
            .site("Dallas USA")
            .round("1")
            .done(),
        B::new("Smith, John", "Doe, Jane")
            .event("Open")
            .site("Lausanne SUI")
            .round("10")
            .done(),
        B::new("Doe, Jane", "Smith, John")
            .event("Club")
            .site("cairo")
            .round("21")
            .done(),
        B::new("Nobody", "Else")
            .event("Linares Open")
            .site("bern")
            .round("2")
            .done(),
    ])
}

fn with(f: impl FnOnce(&mut HeaderSearch)) -> HeaderSearch {
    let mut query = q();
    f(&mut query);
    query
}

#[rstest]
#[case("th,j", vec![2, 3])] // spaces ignored on both sides
#[case("TH,J", vec![2, 3])] // case ignored
#[case("smith, john", vec![2, 3])]
#[case("Smith,", vec![2, 3])]
#[case("Smith, ", vec![2, 3])] // trailing space matches (fixes Scid rule 25)
#[case(" Smith", vec![2, 3])]
#[case("   ", vec![1, 2, 3, 4])] // only spaces = no restriction
#[case("", vec![1, 2, 3, 4])]
#[case("kasparov,garry", vec![1])]
#[case("zzz", vec![])]
#[case("\"Smith, John\"", vec![2, 3])] // quoted: whole-name glob
#[case("\"Smith\"", vec![])]
#[case("\"Smith*\"", vec![2, 3])]
#[case("\"smith*\"", vec![])] // quoted compares case
fn white_or_black_text_matching(#[case] text: &str, #[case] expect: Vec<u32>) {
    // The same text on White finds the games where that player has White;
    // on Black the games where it has Black. Here both colors are probed.
    let fx = names_db();
    let as_white = hits(&fx, &with(|s| s.white = text.into()));
    let as_black = hits(&fx, &with(|s| s.black = text.into()));
    let mut union: Vec<u32> = as_white.iter().chain(&as_black).copied().collect();
    union.sort_unstable();
    union.dedup();
    if text.trim().is_empty() {
        assert_eq!(as_white, expect);
        assert_eq!(as_black, expect);
    } else {
        assert_eq!(union, expect, "white={as_white:?} black={as_black:?}");
    }
}

#[test]
fn white_text_only_looks_at_the_white_player() {
    let fx = names_db();
    assert_eq!(hits(&fx, &with(|s| s.white = "th,j".into())), [2]);
    assert_eq!(hits(&fx, &with(|s| s.black = "th,j".into())), [3]);
    assert_eq!(
        hits(
            &fx,
            &with(|s| {
                s.white = "doe".into();
                s.black = "smith".into();
            })
        ),
        [3]
    );
}

#[test]
fn event_site_and_round_texts_are_substrings_without_case() {
    let fx = names_db();
    assert_eq!(hits(&fx, &with(|s| s.event = "LINARES".into())), [1, 4]);
    assert_eq!(hits(&fx, &with(|s| s.event = "\"Linares\"".into())), [1]);
    assert_eq!(hits(&fx, &with(|s| s.site = "usa".into())), [1, 2]);
    assert_eq!(hits(&fx, &with(|s| s.site = "dallasusa".into())), [1]);
    // unquoted "1" is a substring: 1, 10 and 21
    assert_eq!(hits(&fx, &with(|s| s.round = "1".into())), [1, 2, 3]);
}

#[test]
fn quoted_round_matches_only_the_whole_name() {
    let fx = names_db();
    assert_eq!(hits(&fx, &with(|s| s.round = "\"1\"".into())), [1]);
    assert_eq!(hits(&fx, &with(|s| s.round = "\"2\"".into())), [4]);
}

#[test]
fn quoted_star_usa_matches_names_ending_in_usa() {
    let fx = names_db();
    // acceptance: `"*USA"` matches "Dallas USA" and not "Lausanne SUI"
    assert_eq!(hits(&fx, &with(|s| s.site = "\"*USA\"".into())), [1]);
}

fn glob_db() -> Fixture {
    let sites = [
        "Dallas USA",
        "Lausanne SUI",
        "cairo",
        "bern",
        "a*b",
        "axb",
        "\u{c9}ire",
        "\u{e9}ire",
        "*",
    ];
    let games: Vec<_> = sites
        .iter()
        .map(|s| B::new("W", "B").site(s).done())
        .collect();
    build(&games)
}

#[rstest]
#[case("\"*USA\"", vec![1])]
#[case("\"*usa\"", vec![])] // glob is case-sensitive
#[case("\"usa\"", vec![])] // and over the whole name
#[case("usa", vec![1, 2])] // unquoted: case-insensitive substring ("LausannE" has "usa" too)
#[case("\"Dallas ???\"", vec![1])]
#[case("\"Dallas ??\"", vec![])]
#[case("\"[a-c]*\"", vec![3, 4, 5, 6])]
#[case("\"[bc]*\"", vec![3, 4])]
#[case("\"[^a]*\"", vec![5, 6])] // Tcl globs have no `^` negation: `^` and `a` are members
#[case("\"a\\*b\"", vec![5])] // escaped star is literal
#[case("\"\\*\"", vec![9])]
#[case("\"*\"", vec![1, 2, 3, 4, 5, 6, 7, 8, 9])]
#[case("\"Dallas\\ USA\"", vec![1])]
#[case("\"?ire\"", vec![7, 8])] // `?` is one character, not one byte
#[case("\"\u{c9}*\"", vec![7])]
#[case("\"\u{e9}*\"", vec![8])]
#[case("\u{e9}ire", vec![7, 8])] // unquoted folds É/é
#[case("\u{c9}IRE", vec![7, 8])]
#[case("a*b", vec![5])] // unquoted `*` is just a character
fn site_glob_and_unicode(#[case] text: &str, #[case] expect: Vec<u32>) {
    let fx = glob_db();
    assert_eq!(hits(&fx, &with(|s| s.site = text.into())), expect, "{text}");
}

#[test]
fn unclosed_or_lone_quote_is_an_ordinary_search_and_does_not_panic() {
    let fx = glob_db();
    for text in ["\"", "\"abc", "abc\"", "\"\"", "\"\"\""] {
        drop(hits(&fx, &with(|s| s.site = text.into())));
    }
    // An empty quoted pattern matches only an empty name.
    assert_eq!(
        hits(&fx, &with(|s| s.site = "\"\"".into())),
        Vec::<u32>::new()
    );
}

// ====================================================================
// Elo
// ====================================================================

fn elo_db() -> Fixture {
    build(&[
        B::new("Kasparov, Garry", "Karpov").elo(0, 2700).done(),
        B::new("Smith", "Doe").elo(2000, 1900).done(),
        B::new("Doe", "Kasparov, Garry").elo(0, 0).done(),
        B::new("Kasparov, Garry", "Anand").elo(2800, 2750).done(),
        B::new("Nobody", "Else").elo(0, 0).done(),
    ])
}

#[test]
fn missing_elo_falls_back_to_the_best_elo_of_the_player_in_the_database() {
    let fx = elo_db();
    // game 1: Kasparov has no Elo there but 2800 in game 4
    assert_eq!(
        hits(&fx, &with(|s| s.white_elo = MinMax::new(2500, 4000))),
        [1, 4]
    );
    assert_eq!(
        hits(&fx, &with(|s| s.black_elo = MinMax::new(2790, 4000))),
        [3]
    );
}

#[test]
fn welo_min_one_excludes_players_without_any_elo() {
    let fx = elo_db();
    assert_eq!(
        hits(&fx, &with(|s| s.white_elo = MinMax::new(1, 4000))),
        [1, 2, 3, 4]
    );
    assert_eq!(
        hits(&fx, &with(|s| s.black_elo = MinMax::new(1, 4000))),
        [1, 2, 3, 4]
    );
}

#[test]
fn without_the_fallback_stats_an_unrated_game_has_elo_zero() {
    let fx = elo_db();
    let mut f = Filter::new(5);
    search_with(
        &fx,
        &with(|s| s.white_elo = MinMax::new(1, 4000)),
        &mut f,
        &PlayerStats::new(),
        None,
    );
    assert_eq!(f.iter_games().collect::<Vec<_>>(), [2, 4]);
}

#[test]
fn elo_zero_range_finds_players_with_no_elo_anywhere() {
    let fx = elo_db();
    assert_eq!(hits(&fx, &with(|s| s.white_elo = MinMax::new(0, 0))), [5]);
}

#[test]
fn white_and_black_elo_extremes_are_not_swapped() {
    let fx = elo_db();
    assert_eq!(
        hits(&fx, &with(|s| s.white_elo = MinMax::new(2500, 2000))),
        Vec::<u32>::new()
    );
    assert_eq!(
        hits(&fx, &with(|s| s.black_elo = MinMax::new(2800, 1000))),
        Vec::<u32>::new()
    );
}

#[test]
fn delo_with_inverted_extremes_is_treated_as_swapped() {
    let fx = elo_db();
    // diffs (with fallback): g1 2800-2700=100, g2 100, g3 1900?? no: Doe=1900, K=2800 -> -900, g4 50, g5 0
    assert_eq!(
        hits(&fx, &with(|s| s.elo_diff = MinMax::new(100, 100))),
        [1, 2]
    );
    let straight = hits(&fx, &with(|s| s.elo_diff = MinMax::new(-100, 100)));
    let swapped = hits(&fx, &with(|s| s.elo_diff = MinMax::new(100, -100)));
    assert_eq!(straight, [1, 2, 4, 5]);
    assert_eq!(swapped, straight);
    assert_eq!(
        hits(&fx, &with(|s| s.elo_diff = MinMax::new(-500, -1000))),
        [3]
    );
}

#[test]
fn spelling_peak_raised_in_the_stats_counts_as_the_fallback_elo() {
    let fx = elo_db();
    let db = fx.open();
    let nobody = db.entry(5).unwrap().white_id;
    let mut stats = PlayerStats::from_database(&db);
    stats.raise(nobody, 2600);
    let mut f = Filter::new(5);
    search_with(
        &fx,
        &with(|s| s.white_elo = MinMax::new(2500, 4000)),
        &mut f,
        &stats,
        None,
    );
    assert_eq!(f.iter_games().collect::<Vec<_>>(), [1, 4, 5]);
}

#[test]
fn player_stats_hold_the_highest_elo_of_each_player() {
    let fx = elo_db();
    let db = fx.open();
    let stats = PlayerStats::from_database(&db);
    let kasparov = db.entry(1).unwrap().white_id;
    let karpov = db.entry(1).unwrap().black_id;
    let doe = db.entry(2).unwrap().black_id;
    let nobody = db.entry(5).unwrap().white_id;
    assert_eq!(stats.max_elo(kasparov), 2800);
    assert_eq!(stats.max_elo(karpov), 2700);
    assert_eq!(
        stats.max_elo(doe),
        1900,
        "Doe is Black in game 2 and unrated in game 3"
    );
    assert_eq!(stats.max_elo(nobody), 0);
    assert_eq!(stats.max_elo(999_999), 0);
}

#[test]
fn player_stats_add_game_matches_a_scan_of_the_index() {
    let fx = elo_db();
    let db = fx.open();
    let scanned = PlayerStats::from_database(&db);
    let mut incremental = PlayerStats::new();
    for e in db.entries() {
        incremental.add_game(&e.unwrap());
    }
    for id in 0..db.names().num_names(mychess_db::si4::NameKind::Player) {
        assert_eq!(incremental.max_elo(id), scanned.max_elo(id), "player {id}");
    }
}

#[test]
fn player_stats_raise_never_lowers_and_ignores_zero() {
    let mut s = PlayerStats::new();
    s.raise(2, 2500);
    s.raise(2, 2400);
    s.raise(2, 0);
    assert_eq!(s.max_elo(2), 2500);
}

#[test]
fn recompute_players_after_a_replace_lowers_the_stale_maximum() {
    let fx = elo_db();
    let kasparov = fx.open().entry(1).unwrap().white_id;
    let mut stats = PlayerStats::from_database(&fx.open());
    assert_eq!(stats.max_elo(kasparov), 2800);
    // replace game 4 (Kasparov 2800) by a game where he has 2100
    let mut w = Si4Writer::open(&fx.base).unwrap();
    w.replace_game(4, &B::new("Kasparov, Garry", "Anand").elo(2100, 2750).0)
        .unwrap();
    drop(w);
    let db = fx.open();
    let id = db.entry(4).unwrap().white_id;
    assert_eq!(id, kasparov, "the replacement keeps the name");
    stats.recompute_players(&db, &[id]);
    assert_eq!(stats.max_elo(id), 2100);
}

// ====================================================================
// Result, length, side to move
// ====================================================================

#[rstest]
#[case(ResultSet { white: true, draw: false, black: false, none: false }, vec![1, 5])]
#[case(ResultSet { white: false, draw: true, black: false, none: false }, vec![2])]
#[case(ResultSet { white: false, draw: false, black: true, none: false }, vec![3])]
#[case(ResultSet { white: false, draw: false, black: false, none: true }, vec![4])]
#[case(ResultSet { white: false, draw: true, black: true, none: false }, vec![2, 3])]
#[case(ResultSet { white: false, draw: false, black: false, none: false }, vec![])]
#[case(ResultSet::ALL, vec![1, 2, 3, 4, 5])]
fn result_set_order_is_white_draw_black_none(#[case] results: ResultSet, #[case] expect: Vec<u32>) {
    let fx = build(&[
        B::new("a", "b").result(GameResult::White).done(),
        B::new("a", "b").result(GameResult::Draw).done(),
        B::new("a", "b").result(GameResult::Black).done(),
        B::new("a", "b").result(GameResult::Unknown).done(),
        B::new("a", "b").result(GameResult::White).done(),
    ]);
    assert_eq!(hits(&fx, &with(|s| s.results = results)), expect);
}

fn length_db() -> Fixture {
    build(&[
        B::plies("a", "b", 0).done(),
        B::plies("a", "b", 2).done(),
        B::plies("a", "b", 3).done(),
        B::plies("a", "b", 10).done(),
        B::plies("a", "b", 1023).done(),
    ])
}

#[test]
fn no_length_restriction_includes_games_up_to_1023_half_moves() {
    // D334: the Scid default max of 999 would drop game 5.
    let fx = length_db();
    assert_eq!(hits(&fx, &q()), [1, 2, 3, 4, 5]);
}

#[rstest]
#[case(0, 0, vec![1])]
#[case(2, 3, vec![2, 3])]
#[case(3, 3, vec![3])]
#[case(10, 10, vec![4])]
#[case(4, 9, vec![])]
#[case(1000, 1023, vec![5])]
#[case(0, 1023, vec![1, 2, 3, 4, 5])]
fn length_is_inclusive_in_half_moves(#[case] min: u16, #[case] max: u16, #[case] expect: Vec<u32>) {
    let fx = length_db();
    assert_eq!(
        hits(&fx, &with(|s| s.length = Some(MinMax::new(min, max)))),
        expect
    );
}

#[rstest]
#[case(true, false, vec![1, 2, 4])] // even half moves: White to move
#[case(false, true, vec![3, 5])] // odd: Black to move
#[case(true, true, vec![1, 2, 3, 4, 5])]
#[case(false, false, vec![])]
fn side_to_move_at_the_end_follows_the_parity_of_half_moves(
    #[case] white: bool,
    #[case] black: bool,
    #[case] expect: Vec<u32>,
) {
    let fx = length_db();
    let got = hits(
        &fx,
        &with(|s| {
            s.end_white_to_move = white;
            s.end_black_to_move = black;
        }),
    );
    assert_eq!(got, expect);
}

// ====================================================================
// Ignore colors
// ====================================================================

fn colors_db() -> Fixture {
    build(&[
        B::new("Kasparov, Garry", "Karpov")
            .result(GameResult::White)
            .elo(2800, 2700)
            .done(),
        B::new("Karpov", "Kasparov, Garry")
            .result(GameResult::Black)
            .elo(2700, 2800)
            .done(),
        B::of({
            let mut x = Game::new();
            line(&mut x, &["e2e4", "e7e5", "g1f3"]);
            *x.tags_mut() = B::new("Karpov", "Kasparov, Garry")
                .result(GameResult::Black)
                .elo(2700, 2800)
                .0
                .tags()
                .clone();
            x
        })
        .done(),
        B::new("Smith", "Doe").result(GameResult::White).done(),
    ])
}

#[test]
fn ignore_colors_finds_kasparov_playing_black_too() {
    let fx = colors_db();
    assert_eq!(hits(&fx, &with(|s| s.white = "Kasparov".into())), [1]);
    assert_eq!(
        hits(
            &fx,
            &with(|s| {
                s.white = "Kasparov".into();
                s.ignore_colors = true;
            })
        ),
        [1, 2, 3]
    );
}

#[test]
fn ignore_colors_mirrors_the_result_the_elo_and_the_difference() {
    let fx = colors_db();
    let base = |s: &mut HeaderSearch| {
        s.white = "Kasparov".into();
        s.ignore_colors = true;
    };
    // 1-0 requested: game 2 (0-1 with Kasparov as Black) counts as a win for him
    let won = hits(
        &fx,
        &with(|s| {
            base(s);
            s.results = ResultSet {
                white: true,
                draw: false,
                black: false,
                none: false,
            };
        }),
    );
    assert_eq!(won, [1, 2, 3]);
    let lost = hits(
        &fx,
        &with(|s| {
            base(s);
            s.results = ResultSet {
                white: false,
                draw: false,
                black: true,
                none: false,
            };
        }),
    );
    assert_eq!(lost, Vec::<u32>::new());
    let strong = hits(
        &fx,
        &with(|s| {
            base(s);
            s.white_elo = MinMax::new(2750, 4000);
        }),
    );
    assert_eq!(strong, [1, 2, 3]);
    let diff = hits(
        &fx,
        &with(|s| {
            base(s);
            s.elo_diff = MinMax::new(50, 200);
        }),
    );
    assert_eq!(diff, [1, 2, 3], "+100 for Kasparov in every game");
    let negative = hits(
        &fx,
        &with(|s| {
            base(s);
            s.elo_diff = MinMax::new(-200, -50);
        }),
    );
    assert_eq!(negative, Vec::<u32>::new());
}

#[test]
fn ignore_colors_does_not_mirror_the_side_to_move_at_the_end() {
    // D333: game 3 has 3 half moves (Black to move). With a White-to-move
    // filter it must stay out even though it matched through the swap.
    let fx = colors_db();
    let base = |s: &mut HeaderSearch| {
        s.white = "Kasparov".into();
        s.ignore_colors = true;
    };
    let white_to_move = hits(
        &fx,
        &with(|s| {
            base(s);
            s.end_white_to_move = true;
            s.end_black_to_move = false;
        }),
    );
    assert_eq!(white_to_move, [1, 2]);
    let black_to_move = hits(
        &fx,
        &with(|s| {
            base(s);
            s.end_white_to_move = false;
            s.end_black_to_move = true;
        }),
    );
    assert_eq!(black_to_move, [3]);
}

#[test]
fn ignore_colors_does_not_swap_event_site_round_or_dates() {
    let fx = build(&[
        B::new("Kasparov, Garry", "Karpov").event("Linares").done(),
        B::new("Karpov", "Kasparov, Garry").event("Linares").done(),
        B::new("Karpov", "Kasparov, Garry").event("Other").done(),
    ]);
    let got = hits(
        &fx,
        &with(|s| {
            s.white = "Kasparov".into();
            s.event = "Linares".into();
            s.ignore_colors = true;
        }),
    );
    assert_eq!(got, [1, 2]);
}

// ====================================================================
// Dates
// ====================================================================

fn dates_db() -> Fixture {
    build(&[
        B::new("a", "b").date("2000.??.??").done(),
        B::new("a", "b").date("2000.05.10").done(),
        B::new("a", "b").date("1999.12.31").done(),
        B::new("a", "b").date("2001.01.01").done(),
        B::new("a", "b").date("????.??.??").done(),
        B::new("a", "b").date("2000.05.??").done(),
    ])
}

#[test]
fn complete_dates_follow_check_dates_of_the_dialog() {
    assert_eq!(complete_min_date(""), PartialDate::UNKNOWN);
    assert_eq!(complete_max_date(""), d_ymd(2047, 12, 31));
    assert_eq!(complete_min_date("2000"), d(Some(2000), None, None));
    assert_eq!(complete_max_date("2000"), d_ymd(2000, 12, 31));
    assert_eq!(complete_min_date("2000.05"), d(Some(2000), Some(5), None));
    assert_eq!(
        complete_max_date("2000.05"),
        d(Some(2000), Some(5), Some(31))
    );
    assert_eq!(complete_min_date("2000.01.01"), d(Some(2000), None, None));
    assert_eq!(complete_min_date("2000.03.01"), d_ymd(2000, 3, 1));
    assert_eq!(complete_max_date("2000.05.10"), d_ymd(2000, 5, 10));
    assert_eq!(complete_min_date("2000.05.10"), d_ymd(2000, 5, 10));
}

#[test]
fn default_dates_include_games_with_unknown_year() {
    let fx = dates_db();
    assert_eq!(hits(&fx, &q()), [1, 2, 3, 4, 5, 6]);
}

#[test]
fn year_only_range_includes_partial_dates_of_that_year() {
    let fx = dates_db();
    let got = hits(
        &fx,
        &with(|s| {
            s.date = MinMax::new(complete_min_date("2000"), complete_max_date("2000"));
        }),
    );
    assert_eq!(got, [1, 2, 6]);
}

#[test]
fn a_game_without_year_needs_a_minimum_of_0000() {
    let fx = dates_db();
    let got = hits(
        &fx,
        &with(|s| {
            s.date = MinMax::new(complete_min_date("1999"), complete_max_date(""));
        }),
    );
    assert_eq!(got, [1, 2, 3, 4, 6]);
}

#[test]
fn month_range_compares_the_packed_date() {
    let fx = dates_db();
    let got = hits(
        &fx,
        &with(|s| {
            s.date = MinMax::new(complete_min_date("2000.05"), complete_max_date("2000.05"));
        }),
    );
    assert_eq!(got, [2, 6]);
}

#[test]
fn exact_day_range_matches_only_that_day() {
    let fx = dates_db();
    let day = d_ymd(2000, 5, 10);
    assert_eq!(hits(&fx, &with(|s| s.date = MinMax::new(day, day))), [2]);
}

#[test]
fn minimum_jan_first_counts_games_with_unknown_month_and_day() {
    let fx = dates_db();
    let got = hits(
        &fx,
        &with(|s| {
            s.date = MinMax::new(complete_min_date("2000.01.01"), complete_max_date("2000"));
        }),
    );
    assert_eq!(got, [1, 2, 6]);
}

// ====================================================================
// ECO
// ====================================================================

fn eco_db() -> Fixture {
    build(&[
        B::new("a", "b").eco("B06").done(),
        B::new("a", "b").eco("B07").done(),
        B::new("a", "b").eco("B09").done(),
        B::new("a", "b").eco("B09z4").done(),
        B::new("a", "b").eco("B10").done(),
        B::new("a", "b").done(), // no ECO
        B::new("a", "b").eco("B07a").done(),
        B::new("a", "b").eco("B91a3").done(),
        B::new("a", "b").eco("B91b").done(),
    ])
}

fn eco_range(min: &str, max: &str, include_none: bool) -> EcoRange {
    EcoRange {
        min: eco(min),
        max: eco(max),
        include_none,
    }
}

#[test]
fn no_eco_restriction_includes_games_without_eco() {
    let fx = eco_db();
    assert_eq!(hits(&fx, &q()).len(), 9);
}

#[rstest]
#[case("B07", "B09", false, vec![2, 3, 4, 7])] // max expands to B09z4
#[case("B07", "B09", true, vec![2, 3, 4, 6, 7])]
#[case("B09", "B09", false, vec![3, 4])]
#[case("B07", "B08", false, vec![2, 7])]
#[case("B07a", "B09", false, vec![3, 4, 7])] // min with an extension excludes plain B07
#[case("B91", "B91a", false, vec![8])] // B91a expands to B91a4; B91b is out
#[case("B91", "B91b", false, vec![8, 9])]
#[case("A00", "E99", false, vec![1, 2, 3, 4, 5, 7, 8, 9])] // "no ECO" = No drops game 6
#[case("A00", "E99", true, vec![1, 2, 3, 4, 5, 6, 7, 8, 9])]
fn eco_range_expands_the_maximum_to_its_last_subcode(
    #[case] min: &str,
    #[case] max: &str,
    #[case] none: bool,
    #[case] expect: Vec<u32>,
) {
    let fx = eco_db();
    let got = hits(&fx, &with(|s| s.eco = Some(eco_range(min, max, none))));
    assert_eq!(got, expect, "{min}..{max} none={none}");
}

// ====================================================================
// Game number range
// ====================================================================

#[rstest]
#[case(1, -1, (1..=12).collect::<Vec<u32>>())]
#[case(-10, -1, (3..=12).collect::<Vec<u32>>())] // the 10 last games
#[case(5, 3, vec![3, 4, 5])]
#[case(3, 5, vec![3, 4, 5])]
#[case(0, 0, vec![])]
#[case(-1, -1, vec![12])]
#[case(-100, 5, vec![1, 2, 3, 4, 5])] // still negative after the shift: becomes 0
#[case(100, 200, vec![12])] // both clamp to the number of games
#[case(-1, 1, (1..=12).collect::<Vec<u32>>())] // swapped
#[case(7, 7, vec![7])]
#[case(0, 2, vec![1, 2])]
fn game_number_range_follows_scid(#[case] min: i64, #[case] max: i64, #[case] expect: Vec<u32>) {
    let fx = build(&plain(12));
    assert_eq!(
        hits(&fx, &with(|s| s.game_number = MinMax::new(min, max))),
        expect
    );
}

#[test]
fn game_number_range_extremes_do_not_overflow() {
    let fx = build(&plain(3));
    for (a, b) in [
        (i64::MIN, i64::MAX),
        (i64::MAX, i64::MIN),
        (i64::MIN, i64::MIN),
        (i64::MAX, i64::MAX),
    ] {
        drop(hits(&fx, &with(|s| s.game_number = MinMax::new(a, b))));
    }
}

// ====================================================================
// Flags
// ====================================================================

fn user_flags() -> Vec<GameFlags> {
    GameFlags::all()
        .difference(GameFlags::START | GameFlags::PROMOTION | GameFlags::UNDER_PROMOTION)
        .iter()
        .collect()
}

fn flags_db() -> (Fixture, Vec<GameFlags>) {
    let flags = user_flags();
    assert_eq!(flags.len(), 19);
    let mut games: Vec<_> = flags.iter().map(|f| B::new("a", "b").flags(*f)).collect();
    games.push(B::new("a", "b").done()); // game 20: no flags
    (build(&games), flags)
}

#[test]
fn each_user_flag_yes_no_both_selects_the_right_games() {
    let (fx, flags) = flags_db();
    let all: Vec<u32> = (1..=20).collect();
    for (i, flag) in flags.iter().enumerate() {
        let me = i as u32 + 1;
        let yes = hits(&fx, &with(|s| s.flags.set(*flag, Tri::Yes)));
        assert_eq!(yes, [me], "{flag:?} yes");
        let no = hits(&fx, &with(|s| s.flags.set(*flag, Tri::No)));
        assert_eq!(
            no,
            all.iter().copied().filter(|g| *g != me).collect::<Vec<_>>(),
            "{flag:?} no"
        );
        let both = hits(&fx, &with(|s| s.flags.set(*flag, Tri::Both)));
        assert_eq!(both, all, "{flag:?} both");
    }
}

#[test]
fn six_custom_flags_work_like_the_others() {
    let (fx, _) = flags_db();
    let customs = [
        GameFlags::CUSTOM1,
        GameFlags::CUSTOM2,
        GameFlags::CUSTOM3,
        GameFlags::CUSTOM4,
        GameFlags::CUSTOM5,
        GameFlags::CUSTOM6,
    ];
    let db = fx.open();
    for c in customs {
        let want: Vec<u32> = (1..=20)
            .filter(|g| db.entry(*g).unwrap().flags.contains(c))
            .collect();
        assert_eq!(want.len(), 1);
        assert_eq!(hits(&fx, &with(|s| s.flags.set(c, Tri::Yes))), want);
    }
}

#[test]
fn two_flags_required_together_intersect() {
    let (fx, _) = flags_db();
    let none = hits(
        &fx,
        &with(|s| {
            s.flags.set(GameFlags::TACTICS, Tri::Yes);
            s.flags.set(GameFlags::KINGSIDE, Tri::Yes);
        }),
    );
    assert_eq!(none, Vec::<u32>::new());
    let ok = hits(
        &fx,
        &with(|s| {
            s.flags.set(GameFlags::TACTICS, Tri::Yes);
            s.flags.set(GameFlags::KINGSIDE, Tri::No);
        }),
    );
    assert_eq!(ok.len(), 1);
}

#[test]
fn flag_filter_set_and_get_round_trip_and_accepts() {
    let mut f = mychess_db::FlagFilter::default();
    assert_eq!(f.get(GameFlags::TACTICS), Tri::Both);
    f.set(GameFlags::TACTICS, Tri::Yes);
    f.set(GameFlags::BLUNDER, Tri::No);
    assert_eq!(f.get(GameFlags::TACTICS), Tri::Yes);
    assert_eq!(f.get(GameFlags::BLUNDER), Tri::No);
    assert!(f.accepts(GameFlags::TACTICS));
    assert!(!f.accepts(GameFlags::TACTICS | GameFlags::BLUNDER));
    assert!(!f.accepts(GameFlags::empty()));
    f.set(GameFlags::TACTICS, Tri::Both);
    assert!(f.accepts(GameFlags::empty()));
}

#[rstest]
#[case("t", Some(Tri::Yes))]
#[case("Y", Some(Tri::Yes))]
#[case("yes", Some(Tri::Yes))]
#[case("j", Some(Tri::Yes))]
#[case("o", Some(Tri::Yes))]
#[case("s", Some(Tri::Yes))]
#[case("1", Some(Tri::Yes))]
#[case("f", Some(Tri::No))]
#[case("N", Some(Tri::No))]
#[case("no", Some(Tri::No))]
#[case("0", Some(Tri::No))]
#[case("b", Some(Tri::Both))]
#[case("Both", Some(Tri::Both))]
#[case("2", Some(Tri::Both))]
#[case("x", None)]
#[case("", None)]
#[case("9", None)]
fn tri_parses_str_get_flag_texts_and_rejects_others(
    #[case] text: &str,
    #[case] expect: Option<Tri>,
) {
    assert_eq!(Tri::from_scid_str(text), expect, "{text:?}");
}

fn promo_db() -> Fixture {
    const FEN: &str = "8/P6k/8/8/8/8/8/K7 w - - 0 1";
    build(&[
        B::new("a", "b").done(),                 // 1 plain, standard start
        B::of(fen_game(FEN, &["a1b1"])).done(),  // 2 own start, no promotion
        B::of(fen_game(FEN, &["a7a8q"])).done(), // 3 promotion to a queen
        B::of(fen_game(FEN, &["a7a8r"])).done(), // 4 under-promotion to a rook
        B::of(fen_game(FEN, &["a7a8n"])).done(), // 5 under-promotion to a knight
    ])
}

#[test]
fn derived_flags_start_promotion_and_under_promotion_are_searchable() {
    let fx = promo_db();
    assert_eq!(
        hits(&fx, &with(|s| s.flags.set(GameFlags::START, Tri::Yes))),
        [2, 3, 4, 5]
    );
    assert_eq!(
        hits(&fx, &with(|s| s.flags.set(GameFlags::START, Tri::No))),
        [1]
    );
    assert_eq!(
        hits(&fx, &with(|s| s.flags.set(GameFlags::PROMOTION, Tri::Yes))),
        [3, 4, 5]
    );
    assert_eq!(
        hits(
            &fx,
            &with(|s| s.flags.set(GameFlags::UNDER_PROMOTION, Tri::Yes))
        ),
        [4, 5]
    );
    assert_eq!(
        hits(
            &fx,
            &with(|s| s.flags.set(GameFlags::UNDER_PROMOTION, Tri::No))
        ),
        [1, 2, 3]
    );
    let queen_only = hits(
        &fx,
        &with(|s| {
            s.flags.set(GameFlags::PROMOTION, Tri::Yes);
            s.flags.set(GameFlags::UNDER_PROMOTION, Tri::No);
        }),
    );
    assert_eq!(queen_only, [3]);
}

#[test]
fn pgn_text_equals_r_with_the_promotions_flag_finds_rook_underpromotions() {
    let fx = promo_db();
    let got = hits(
        &fx,
        &with(|s| {
            s.pgn_text = vec!["=R".into()];
            s.flags.set(GameFlags::PROMOTION, Tri::Yes);
        }),
    );
    assert_eq!(got, [4]);
    assert_eq!(hits(&fx, &with(|s| s.pgn_text = vec!["=N".into()])), [5]);
    assert_eq!(hits(&fx, &with(|s| s.pgn_text = vec!["=Q".into()])), [3]);
    assert_eq!(hits(&fx, &with(|s| s.pgn_text = vec!["a8=R".into()])), [4]);
}

// ====================================================================
// Extra tag
// ====================================================================

fn extra_db() -> Fixture {
    build(&[
        B::new("a", "b").extra("Annotator", "Rogers, Ian").done(),
        B::new("a", "b").extra("Annotator", "2").done(),
        B::new("a", "b").extra("Annotator", "20").done(),
        B::new("a", "b").extra("Annotator", "2x").done(),
        B::new("a", "b").extra("annotator", "Rogers").done(),
        B::new("a", "b").done(),
        B::new("a", "b").extra("Annotator", "rogers").done(),
        B::new("a", "b").extra("Source", "x").done(),
    ])
}

#[test]
fn extra_tag_value_is_a_case_sensitive_prefix_when_it_starts_with_a_letter() {
    let fx = extra_db();
    let tag = |name: &str, value: &str| {
        hits(
            &fx,
            &with(|s| {
                s.extra_tag = name.into();
                s.extra_tag_value = value.into();
            }),
        )
    };
    assert_eq!(tag("Annotator", "Rog"), [1]);
    assert_eq!(tag("Annotator", "Rogers, Ian"), [1]);
    assert_eq!(tag("Annotator", "rog"), [7]);
    assert_eq!(tag("Annotator", "ogers"), Vec::<u32>::new());
}

#[test]
fn extra_tag_value_starting_with_a_digit_must_be_equal() {
    let fx = extra_db();
    let tag = |value: &str| {
        hits(
            &fx,
            &with(|s| {
                s.extra_tag = "Annotator".into();
                s.extra_tag_value = value.into();
            }),
        )
    };
    assert_eq!(tag("2"), [2]);
    assert_eq!(tag("20"), [3]);
    assert_eq!(tag("2x"), [4]);
    assert_eq!(tag("3"), Vec::<u32>::new());
}

#[test]
fn extra_tag_name_is_case_sensitive_and_value_may_be_empty() {
    let fx = extra_db();
    let by_name = |name: &str| hits(&fx, &with(|s| s.extra_tag = name.into()));
    assert_eq!(by_name("Annotator"), [1, 2, 3, 4, 7]);
    assert_eq!(by_name("annotator"), [5]);
    assert_eq!(by_name("Source"), [8]);
    assert_eq!(by_name("Nothing"), Vec::<u32>::new());
}

#[test]
fn extra_tag_value_without_a_name_is_ignored_and_standard_tags_never_match() {
    let fx = extra_db();
    assert_eq!(
        hits(&fx, &with(|s| s.extra_tag_value = "zzz".into())).len(),
        8
    );
    for std in [
        "Event",
        "Site",
        "White",
        "Black",
        "Round",
        "Date",
        "Result",
        "EventDate",
    ] {
        assert_eq!(
            hits(&fx, &with(|s| s.extra_tag = std.into())),
            Vec::<u32>::new(),
            "{std}"
        );
    }
}

// ====================================================================
// PGN text
// ====================================================================

fn text_db() -> Fixture {
    let mut g1 = Game::new();
    line(&mut g1, &["e2e4", "e7e5", "g1f3", "b8c6"]);
    {
        let t = g1.tags_mut();
        t.white = "Kasparov, Garry".into();
        t.black = "Karpov".into();
        t.event = "Linares".into();
        t.set_extra("Annotator", "Rogers, Ian").unwrap();
    }
    let ids: Vec<_> = g1.mainline().collect();
    g1.set_comment(ids[2], "great game").unwrap();
    g1.add_nag(ids[2], Nag::new(1).unwrap()).unwrap();

    // game 2: main line 1.d4 d5, variation 1.e4 e5 2.Qh5
    let mut g2 = Game::new();
    line(&mut g2, &["d2d4", "d7d5"]);
    let root = g2.root();
    let e4 = {
        let mv = g2.position_at(root).unwrap().parse_uci("e2e4").unwrap();
        g2.add_move(root, mv).unwrap()
    };
    let e5 = support::play(&mut g2, e4, "e7e5");
    support::play(&mut g2, e5, "d1h5");
    g2.tags_mut().event = "Open".into();

    let mut g3 = Game::new();
    line(&mut g3, &["d2d4", "d7d5"]);
    g3.tags_mut().event = "Linares".into();
    g3.tags_mut().black = "Smith".into();
    build(&[
        (g1, GameFlags::empty()),
        (g2, GameFlags::empty()),
        (g3, GameFlags::empty()),
    ])
}

fn text(phrases: &[&str], ignore_case: bool) -> HeaderSearch {
    with(|s| {
        s.pgn_text = phrases.iter().map(|p| (*p).to_owned()).collect();
        s.ignore_case = ignore_case;
    })
}

#[rstest]
#[case(&["Nf3"], false, vec![1])]
#[case(&["Qh5"], false, vec![2])] // moves inside a variation are searched
#[case(&["1.e4 e5", "Nf3"], false, vec![1])] // every phrase is required
#[case(&["1.e4 e5", "Linares"], false, vec![1])]
#[case(&["Linares", "d4"], false, vec![3])]
#[case(&["e4", "Nf3", "great"], false, vec![1])] // three phrases
#[case(&["e4", "Nf3", "great", "Nc6"], false, vec![1])] // the code accepts more
#[case(&["e4", "Nf3", "great", "Qh5"], false, vec![])]
#[case(&["kasparov"], false, vec![])] // case-sensitive by default
#[case(&["kasparov"], true, vec![1])]
#[case(&["NF3"], true, vec![1])]
#[case(&["[Event \"Linares\"]"], false, vec![1, 3])] // tags are part of the text
#[case(&["Annotator"], false, vec![1])] // extra tags too
#[case(&["Rogers, Ian"], false, vec![1])]
#[case(&["Nf3!"], false, vec![1])] // NAG as symbol, next to the move
#[case(&["great game"], false, vec![1])] // comments
#[case(&["e5 2.Nf3"], false, vec![1])] // inner spaces are kept
#[case(&["1. e4"], false, vec![])] // no space after the move number (D368)
#[case(&[""], false, vec![1, 2, 3])] // empty phrases are dropped
#[case(&["", "Qh5"], false, vec![2])]
#[case(&[], false, vec![1, 2, 3])]
#[case(&["zzzzz"], false, vec![])]
fn pgn_text_requires_all_phrases_as_substrings(
    #[case] phrases: &[&str],
    #[case] ignore_case: bool,
    #[case] expect: Vec<u32>,
) {
    let fx = text_db();
    assert_eq!(
        hits(&fx, &text(phrases, ignore_case)),
        expect,
        "{phrases:?}"
    );
}

#[test]
fn search_text_options_write_compact_moves_without_line_breaks() {
    let mut g = Game::new();
    let ucis: Vec<&str> = ["g1f3", "g8f6", "f3g1", "f6g8"]
        .iter()
        .copied()
        .cycle()
        .take(120)
        .collect();
    line(&mut g, &ucis);
    let ids: Vec<_> = g.mainline().collect();
    g.add_nag(ids[0], Nag::new(1).unwrap()).unwrap();
    g.set_comment(ids[1], "note").unwrap();
    let out = game_to_string(&g, &WriteOptions::search_text()).unwrap();
    let movetext = out.split("\n\n").nth(1).expect("movetext after the tags");
    assert!(movetext.starts_with("1.Nf3!"), "{movetext}");
    assert!(!out.contains("1. N"), "no space after the move number");
    assert!(movetext.contains("{note}"));
    assert!(
        !movetext.trim_end().contains('\n'),
        "no line wrapping: {movetext}"
    );
    assert!(out.contains("[White "));
    assert!(out.contains('\n'));
    assert!(!out.contains('\r'));
}

// ====================================================================
// Game end
// ====================================================================

const MATE_FEN: &str = "rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3";

fn end_db() -> Fixture {
    // game 4: main line 1.e4 e5, variation 1.f3 e5 2.g4 Qh4#
    let mut g4 = Game::new();
    line(&mut g4, &["e2e4", "e7e5"]);
    let root = g4.root();
    let mv = g4.position_at(root).unwrap().parse_uci("f2f3").unwrap();
    let f3 = g4.add_move(root, mv).unwrap();
    let e5 = support::play(&mut g4, f3, "e7e5");
    let g4n = support::play(&mut g4, e5, "g2g4");
    support::play(&mut g4, g4n, "d8h4");
    // game 5: no moves, starts from a checkmated position (D375)
    let g5 = fen_game(MATE_FEN, &[]);
    assert!(g5.start_position().is_checkmate());
    build(&[
        B::of(fools_mate()).done(),
        B::of(stalemate_game()).done(),
        B::new("a", "b").done(),
        (g4, GameFlags::empty()),
        (g5, GameFlags::empty()),
    ])
}

#[test]
fn game_end_checkmate_and_stalemate_use_the_main_line_only() {
    let fx = end_db();
    assert_eq!(
        hits(&fx, &with(|s| s.game_end = GameEndKind::Checkmate)),
        [1, 5]
    );
    assert_eq!(
        hits(&fx, &with(|s| s.game_end = GameEndKind::Stalemate)),
        [2]
    );
    assert_eq!(
        hits(&fx, &with(|s| s.game_end = GameEndKind::Any)),
        [1, 2, 3, 4, 5]
    );
}

// ====================================================================
// Pre and post comments
// ====================================================================

fn comments_db() -> Fixture {
    let two = || {
        let mut g = Game::new();
        line(&mut g, &["e2e4", "e7e5"]);
        g
    };
    let last = |g: &Game| g.mainline().last().unwrap();
    let first = |g: &Game| g.mainline().next().unwrap();
    let mut g1 = two();
    let l = last(&g1);
    g1.set_comment(l, "final words").unwrap();
    let mut g2 = two();
    let f = first(&g2);
    g2.set_comment(f, "early").unwrap();
    let mut g3 = two();
    g3.set_initial_comment("intro");
    let mut g4 = two();
    let l = last(&g4);
    g4.set_comment(l, "[hidden]").unwrap();
    let mut g5 = two();
    g5.set_initial_comment("[hidden intro]");
    let l = last(&g5);
    g5.set_comment(l, "[x] y").unwrap();
    let mut g6 = two();
    g6.set_initial_comment("intro");
    let l = last(&g6);
    g6.set_comment(l, "final").unwrap();
    let mut g7 = Game::new();
    g7.set_initial_comment("only intro");
    let all = [g1, g2, g3, g4, g5, g6, g7];
    build(&all.map(|g| (g, GameFlags::empty())))
}

#[test]
fn pre_comment_needs_a_visible_initial_comment() {
    let fx = comments_db();
    assert_eq!(hits(&fx, &with(|s| s.pre_comment = true)), [3, 6, 7]);
}

#[test]
fn post_comment_needs_a_visible_comment_on_the_last_move_and_ignores_bracketed_ones() {
    let fx = comments_db();
    // g4 "[hidden]" is hidden; g5 "[x] y" is not (does not end with `]`);
    // g7 has no moves: the root comment stands in (D375).
    assert_eq!(hits(&fx, &with(|s| s.post_comment = true)), [1, 5, 6, 7]);
}

#[test]
fn pre_and_post_comment_together_require_both() {
    let fx = comments_db();
    assert_eq!(
        hits(
            &fx,
            &with(|s| {
                s.pre_comment = true;
                s.post_comment = true;
            })
        ),
        [6, 7]
    );
}

// ====================================================================
// Titles
// ====================================================================

struct Provider(HashMap<&'static str, Title>);

impl TitleProvider for Provider {
    fn title(&self, name: &str) -> Option<Title> {
        self.0.get(name).copied()
    }
}

fn provider() -> Provider {
    Provider(HashMap::from([
        ("Kasparov, Garry", Title::Gm),
        ("Anand", Title::Gm),
        ("Polgar", Title::Wgm),
    ]))
}

fn titles_db() -> Fixture {
    build(&[
        B::new("Kasparov, Garry", "Smith").done(),
        B::new("Polgar", "Kasparov, Garry").done(),
        B::new("Smith", "Anand").done(),
        B::new("Smith", "Doe").done(),
    ])
}

fn hits_titles(fx: &Fixture, query: &HeaderSearch, p: Option<&dyn TitleProvider>) -> Vec<u32> {
    let db = fx.open();
    let stats = PlayerStats::from_database(&db);
    let mut f = Filter::new(db.len());
    search_with(fx, query, &mut f, &stats, p);
    f.iter_games().collect()
}

#[test]
fn white_titles_filter_uses_the_provider() {
    let fx = titles_db();
    let p = provider();
    let t = |set: TitleSet| hits_titles(&fx, &with(|s| s.white_titles = set), Some(&p));
    assert_eq!(t(TitleSet::GM), [1]);
    assert_eq!(t(TitleSet::WGM), [2]);
    assert_eq!(t(TitleSet::NONE), [3, 4]);
    assert_eq!(t(TitleSet::GM | TitleSet::NONE), [1, 3, 4]);
    assert_eq!(t(TitleSet::IM), Vec::<u32>::new());
    assert_eq!(
        t(TitleSet::all()),
        [1, 2, 3, 4],
        "all titles = no restriction"
    );
}

#[test]
fn black_titles_and_both_sides_combine() {
    let fx = titles_db();
    let p = provider();
    let black_gm = with(|s| s.black_titles = TitleSet::GM);
    assert_eq!(hits_titles(&fx, &black_gm, Some(&p)), [2, 3]);
    let both = with(|s| {
        s.white_titles = TitleSet::NONE;
        s.black_titles = TitleSet::GM;
    });
    assert_eq!(hits_titles(&fx, &both, Some(&p)), [3]);
}

#[test]
fn title_restriction_is_ignored_without_a_provider() {
    let fx = titles_db();
    let query = with(|s| {
        s.white_titles = TitleSet::GM;
        s.black_titles = TitleSet::WIM;
    });
    assert_eq!(hits_titles(&fx, &query, None), [1, 2, 3, 4]);
    // `hits` passes no provider either
    assert_eq!(hits(&fx, &query), [1, 2, 3, 4]);
}

#[rstest]
#[case("gm", Some(Title::Gm))]
#[case("GM", Some(Title::Gm))]
#[case("hgm", Some(Title::Gm))]
#[case("im", Some(Title::Im))]
#[case("fm", Some(Title::Fm))]
#[case("cm", Some(Title::Cm))]
#[case("wgm", Some(Title::Wgm))]
#[case("wim", Some(Title::Wim))]
#[case("wfm", Some(Title::Wfm))]
#[case("cgm", None)]
#[case("cim", None)]
#[case("", None)]
#[case("none", None)]
fn title_names_follow_parse_titles(#[case] text: &str, #[case] expect: Option<Title>) {
    assert_eq!(Title::from_scid_str(text), expect, "{text:?}");
}

#[test]
fn title_set_default_is_every_title_including_none() {
    let all = TitleSet::default();
    assert_eq!(all, TitleSet::all());
    assert!(
        all.contains(TitleSet::NONE) && all.contains(TitleSet::GM) && all.contains(TitleSet::WFM)
    );
}

// ====================================================================
// Errors, damaged files, cancellation, progress
// ====================================================================

#[test]
fn filter_of_another_size_is_refused_and_left_untouched() {
    let fx = build(&plain(5));
    for len in [0, 3, 6] {
        let mut db = fx.open();
        let stats = PlayerStats::from_database(&db);
        let mut f = Filter::new(len);
        f.set(1.min(len), 77);
        let before = f.clone();
        let err = search_header(
            &mut db,
            &q(),
            &mut f,
            &stats,
            None,
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            SearchError::FilterSize {
                filter: len,
                games: 5
            }
        );
        assert_eq!(f, before);
    }
}

#[test]
fn unreadable_index_entry_leaves_the_filter_and_is_reported() {
    let fx = build(&plain(4));
    break_entry(&fx, 2);
    let (got, r) = hits_report(&fx, &q());
    assert_eq!(got, [1, 3, 4]);
    assert_eq!(r.unreadable, [2]);
    assert_eq!(r.matched, 3);
}

#[test]
fn zero_length_record_is_removed_without_being_an_error() {
    let fx = build(&plain(3));
    zero_length(&fx, 2);
    let (got, r) = hits_report(&fx, &q());
    assert_eq!(got, [1, 3]);
    assert!(!r.interrupted);
}

#[test]
fn record_is_only_read_when_the_query_needs_it() {
    // game 2 has a record that cannot be read and carries the TACTICS flag
    let fx = build(&[
        B::new("a", "b").done(),
        B::new("a", "b").flags(GameFlags::TACTICS),
        B::new("a", "b").done(),
    ]);
    break_offset(&fx, 2);

    // header-only query: the entry is enough
    let (got, r) = hits_report(&fx, &with(|s| s.white = "a".into()));
    assert_eq!(got, [1, 2, 3]);
    assert!(r.unreadable.is_empty());

    // text, comment and end-of-game queries must read the record
    for query in [
        with(|s| s.pgn_text = vec!["e4".into()]),
        with(|s| s.post_comment = true),
        with(|s| s.pre_comment = true),
        with(|s| s.game_end = GameEndKind::Checkmate),
        with(|s| s.extra_tag = "Annotator".into()),
    ] {
        let (got, r) = hits_report(&fx, &query);
        assert!(!got.contains(&2), "{query:?}");
        assert_eq!(r.unreadable, [2], "{query:?}");
    }

    // header and flags are tested before the record: a game that fails them
    // is never decoded, so it is not "unreadable"
    let (got, r) = hits_report(
        &fx,
        &with(|s| {
            s.pgn_text = vec!["e4".into()];
            s.flags.set(GameFlags::TACTICS, Tri::No);
        }),
    );
    assert_eq!(got, [1, 3]);
    assert!(r.unreadable.is_empty());
}

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

#[rstest]
#[case(FilterOp::Reset)]
#[case(FilterOp::And)]
#[case(FilterOp::Or)]
fn cancelling_in_a_database_over_5000_games_keeps_the_old_filter(#[case] op: FilterOp) {
    let n = 5300;
    let fx = big_db(n);
    let mut db = fx.open();
    let stats = PlayerStats::from_database(&db);
    let mut f = Filter::new(n as u32);
    f.set(1, 33);
    f.set(2, 0);
    f.set(n as u32, 255);
    let before = f.clone();
    let cancel = CancelToken::new();
    let token = cancel.clone();
    let mut calls = 0u32;
    let mut sink = |_done: u64, _total: u64| {
        calls += 1;
        token.cancel();
    };
    let query = with(|s| {
        s.op = op;
        s.white = "Kasparov".into();
    });
    let r = search_header(&mut db, &query, &mut f, &stats, None, &mut sink, &cancel).unwrap();
    assert!(r.interrupted, "{op:?}");
    assert_eq!(f, before, "the filter must be the old one (D328)");
    assert_eq!(r.matched, before.count());
    assert!(calls >= 1);
}

#[test]
fn an_already_cancelled_token_stops_before_touching_anything() {
    let fx = build(&plain(10));
    let mut db = fx.open();
    let stats = PlayerStats::from_database(&db);
    let mut f = Filter::new(10);
    f.set(4, 0);
    f.set(5, 99);
    let before = f.clone();
    let cancel = CancelToken::new();
    cancel.cancel();
    let r = search_header(
        &mut db,
        &q(),
        &mut f,
        &stats,
        None,
        &mut NoProgress,
        &cancel,
    )
    .unwrap();
    assert!(r.interrupted);
    assert_eq!(f, before);
    assert_eq!(r.matched, 9);
}

#[test]
fn a_finished_search_reports_progress_up_to_n_of_n() {
    let n = 5300usize;
    let fx = big_db(n);
    let mut db = fx.open();
    let stats = PlayerStats::from_database(&db);
    let mut f = Filter::new(n as u32);
    let mut seen: Vec<(u64, u64)> = Vec::new();
    {
        let mut sink = |done: u64, total: u64| seen.push((done, total));
        let r = search_header(
            &mut db,
            &q(),
            &mut f,
            &stats,
            None,
            &mut sink,
            &CancelToken::new(),
        )
        .unwrap();
        assert!(!r.interrupted);
        assert_eq!(r.matched, n as u32);
    }
    assert!(
        seen.len() >= 2,
        "a report near game 5000 and the final one: {seen:?}"
    );
    assert!(seen.iter().all(|(_, t)| *t == n as u64));
    assert!(
        seen.windows(2).all(|w| w[0].0 <= w[1].0),
        "monotonic: {seen:?}"
    );
    assert_eq!(seen.last(), Some(&(n as u64, n as u64)));
    assert!(
        seen.iter().any(|(d, _)| *d >= 4000 && *d < n as u64),
        "progress by game 5000: {seen:?}"
    );
}

#[test]
fn progress_sink_trait_object_receives_calls() {
    struct Count(u32);
    impl ProgressSink for Count {
        fn progress(&mut self, _done: u64, _total: u64) {
            self.0 += 1;
        }
    }
    let fx = build(&plain(3));
    let mut db: Si4Database = fx.open();
    let stats = PlayerStats::new();
    let mut f = Filter::new(3);
    let mut sink = Count(0);
    search_header(
        &mut db,
        &q(),
        &mut f,
        &stats,
        None,
        &mut sink,
        &CancelToken::new(),
    )
    .unwrap();
    assert!(sink.0 >= 1, "at least the final N / N report");
}

// ====================================================================
// Hostile patterns
// ====================================================================

#[test]
fn malformed_globs_never_panic_and_match_sensibly() {
    let fx = glob_db();
    for pat in [
        "\"[abc\"",
        "\"[\"",
        "\"\\\"",
        "\"[]\"",
        "\"[z-a]*\"",
        "\"**??**\"",
        "\"[a-\"",
    ] {
        drop(hits(&fx, &with(|s| s.site = pat.into())));
    }
    // a trailing backslash cannot match anything
    assert_eq!(
        hits(&fx, &with(|s| s.site = "\"cairo\\\"".into())),
        Vec::<u32>::new()
    );
}

#[test]
fn pathological_glob_finishes_quickly() {
    // "*a*a*a*...*b" against a long run of 'a' is exponential for a naive
    // backtracking matcher.
    let long = "a".repeat(200);
    let fx = build(&[
        B::new("w", "b").site(&long).done(),
        B::new("w", "b").site("ab").done(),
    ]);
    let pattern = format!("\"{}b\"", "*a".repeat(12));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let got = hits(&fx, &with(|s| s.site = pattern));
        tx.send(got).ok();
    });
    let got = rx
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("glob matching must not blow up exponentially");
    assert_eq!(got, Vec::<u32>::new());
}

#[test]
fn game_range_on_an_empty_database_is_harmless() {
    let fx = build(&[]);
    for (a, b) in [(-1, -1), (0, 0), (1, -1), (5, 3)] {
        assert!(hits(&fx, &with(|s| s.game_number = MinMax::new(a, b))).is_empty());
    }
}

#[test]
fn long_names_up_to_255_bytes_are_matched_whole() {
    let name = "x".repeat(255);
    let fx = build(&[B::new(&name, "b").done(), B::new("short", "b").done()]);
    assert_eq!(hits(&fx, &with(|s| s.white = name.clone())), [1]);
    assert_eq!(hits(&fx, &with(|s| s.white = format!("\"{name}\""))), [1]);
}
