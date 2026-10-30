//! Contract tests of the player table, the tournament finder, the player
//! information and the filter links (M6-3a): rules 75 to 100 of
//! `docs/specs/m6-3-jogadores-torneios-spelling.md`. Synthetic data only.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

#[path = "common/m6_3a_support.rs"]
mod support;

use std::path::Path;

use mychess_core::{GameResult, PartialDate};
use mychess_db::finder::{
    CountryFilter, PlayerFound, PlayerQuery, PlayerSort, PlayerTable, TournamentQuery,
    TournamentRow, TournamentSort, find_tournaments,
};
use mychess_db::player_info::{
    GameSelection, PlayerFilterMode, fide_id_of, fide_profile_url, player_games_filter,
    player_info, spelling_header,
};
use mychess_db::si4::{NameChange, NameKind, Si4Database, Si4Writer};
use mychess_db::spelling::{COUNTRY_TABLE, TITLE_TABLE, country_name, title_name};
use mychess_db::{CancelToken, Filter, NoProgress};
use pretty_assertions::assert_eq;
use rstest::rstest;
use support::{G, build, date, g, open, read, six_games};

const W: GameResult = GameResult::White;
const B: GameResult = GameResult::Black;
const D: GameResult = GameResult::Draw;
const U: GameResult = GameResult::Unknown;

type Db = Si4Database<std::fs::File>;

fn table(db: &Db, spelling: Option<&str>) -> PlayerTable {
    let set = spelling.map(read);
    PlayerTable::build(
        db,
        set.as_ref().map(|s| s.checker(NameKind::Player)),
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap()
}

fn names(rows: &[PlayerFound]) -> Vec<&str> {
    rows.iter().map(|r| &*r.name).collect()
}

fn dated(white: &str, black: &str, result: GameResult, when: &'static str) -> G {
    g(white, black, result).on("E", "S", when)
}

// ---------------------------------------------------------------------------
// PlayerTable::build (rule 75).
// ---------------------------------------------------------------------------

#[test]
fn a_game_against_oneself_counts_twice_and_updates_dates_once_per_game() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &[dated("Solo, S", "Solo, S", D, "2020.03.03")]);
    let t = table(&open(&base), None);
    assert_eq!(t.rows().len(), 1);
    assert_eq!(t.rows()[0].games, 2);
    assert_eq!(t.rows()[0].first.to_string(), "2020.03.03");
}

#[test]
fn players_without_games_are_not_listed() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &[dated("Old, A", "Other, O", W, "2020.01.01")]);
    let mut w = Si4Writer::open(&base).unwrap();
    w.apply_name_changes(
        NameKind::Player,
        &[NameChange::new("Old, A", "New, A")],
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    drop(w);
    let db = open(&base);
    assert_eq!(
        db.names().num_names(NameKind::Player),
        3,
        "the old name is still in the .sn4"
    );
    let t = table(&db, None);
    let listed: Vec<&str> = t.rows().iter().map(|r| &*r.name).collect();
    assert_eq!(listed, ["New, A", "Other, O"]);
    assert!(
        t.query(
            &PlayerQuery {
                games: 0..=100,
                ..PlayerQuery::default()
            },
            None
        )
        .iter()
        .all(|r| r.games > 0)
    );
}

#[test]
fn an_empty_database_gives_an_empty_table() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &[]);
    let t = table(&open(&base), None);
    assert!(t.is_empty());
    assert_eq!(t.query(&PlayerQuery::default(), None).len(), 0);
    assert!(!t.has_estimates());
}

#[test]
fn first_and_last_ignore_games_without_a_year_and_order_partial_dates_like_the_scid() {
    let dir = tempfile::tempdir().unwrap();
    let games = vec![
        dated("Pat, P", "Q, Q", W, "2020.05.01"),
        dated("Pat, P", "Q, Q", W, "2020.??.??"),
        dated("Pat, P", "Q, Q", W, "????.05.10"),
        dated("Pat, P", "Q, Q", W, "2021.01.??"),
        dated("Pat, P", "Q, Q", W, "????.??.??"),
    ];
    let base = build(dir.path(), &games);
    let t = table(&open(&base), None);
    let pat = t.rows().iter().find(|r| &*r.name == "Pat, P").unwrap();
    assert_eq!(pat.games, 5);
    assert_eq!(
        pat.first.to_string(),
        "2020.??.??",
        "unknown parts count as 0"
    );
    assert_eq!(pat.last.to_string(), "2021.01.??");
}

#[test]
fn the_elo_column_is_the_highest_rating_in_the_games_of_either_colour() {
    let dir = tempfile::tempdir().unwrap();
    let games = vec![
        dated("Pat, P", "Q, Q", W, "2020.01.01").elo(2300, 2000),
        dated("Q, Q", "Pat, P", W, "2020.01.02").elo(2100, 2450),
        dated("Pat, P", "Q, Q", W, "2020.01.03"),
    ];
    let base = build(dir.path(), &games);
    let t = table(&open(&base), None);
    let elo = |n: &str| t.rows().iter().find(|r| &*r.name == n).unwrap().max_elo;
    assert_eq!(elo("Pat, P"), 2450);
    assert_eq!(elo("Q, Q"), 2100);
}

#[test]
fn estimates_come_only_from_exact_names_with_a_real_peak_and_never_for_surnames() {
    let dir = tempfile::tempdir().unwrap();
    let games = vec![
        dated("Pat, P", "Q, Q", W, "2020.01.01"),
        dated("Ann, A", "Q, Q", W, "2020.01.01"),
        dated("Zed", "Q, Q", W, "2020.01.01"),
        dated("Hist, H", "Q, Q", W, "2020.01.01"),
        dated("Rated, R", "Q, Q", W, "2020.01.01").elo(2000, 0),
    ];
    let base = build(dir.path(), &games);
    let ssp = "@PLAYER\nPat, P #- BRA [....] 1980\nAnn, A #- BRA [2222] 1980\nZed #- BRA [2500]\nHist, H #- BRA [2600*] 1850\nRated, R #- BRA [2900]\n= Rated, Rx\n";
    let t = table(&open(&base), Some(ssp));
    let elo = |n: &str| t.rows().iter().find(|r| &*r.name == n).unwrap().max_elo;
    assert_eq!(elo("Pat, P"), 0, "[....] is no peak");
    assert_eq!(elo("Ann, A"), 2222);
    assert_eq!(elo("Zed"), 0, "a surname alone");
    assert_eq!(elo("Hist, H"), 2600, "the * estimate counts");
    assert_eq!(elo("Rated, R"), 2000, "the games win over the file");
    assert!(t.has_estimates());
}

#[test]
fn the_estimate_uses_the_exact_correct_name_not_the_loose_match() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &[dated("Pat, P", "Q, Q", W, "2020.01.01")]);
    // Only an alias and a longer name exist: "Pat, P" is not a correct name.
    let ssp = "@PLAYER\nPat, Paul #- BRA [2500] 1980\n= Pat, P\n";
    let t = table(&open(&base), Some(ssp));
    assert_eq!(
        t.rows()
            .iter()
            .find(|r| &*r.name == "Pat, P")
            .unwrap()
            .max_elo,
        0
    );
    assert!(!t.has_estimates());
}

#[test]
fn building_can_be_cancelled_and_reports_monotonic_progress() {
    let dir = tempfile::tempdir().unwrap();
    let games: Vec<G> = (0..6000)
        .map(|_| dated("A", "B", W, "2020.01.01"))
        .collect();
    let base = build(dir.path(), &games);
    let db = open(&base);
    let mut calls = Vec::new();
    let t = PlayerTable::build(&db, None, &CancelToken::new(), &mut |d, t| {
        calls.push((d, t));
    })
    .unwrap();
    assert_eq!(t.rows()[0].games, 6000);
    assert!(calls.windows(2).all(|w| w[0].0 <= w[1].0));
    assert_eq!(calls.last().map(|c| c.0 == c.1), Some(true));
    let token = CancelToken::new();
    let t2 = token.clone();
    let result = PlayerTable::build(&db, None, &token, &mut move |_, _| t2.cancel());
    assert!(result.is_err(), "cancelled at the first report");
}

// ---------------------------------------------------------------------------
// PlayerQuery (rules 76 to 78).
// ---------------------------------------------------------------------------

fn small_table(dir: &Path) -> (Db, PlayerTable) {
    let games = vec![
        dated("Ábel, A", "Zed, Z", W, "2020.01.01").elo(2000, 1000),
        dated("Ábel, A", "Zed, Z", D, "2019.06.06").elo(2100, 1100),
        dated("zeta, z", "Zed, Z", B, "2021.03.03").elo(3000, 1200),
        dated("Mid, M", "Zed, Z", B, "????.??.??").elo(1500, 1300),
        dated("Mid, M", "Ábel, A", D, "2018.02.02"),
        dated("Éva, E", "Zed, Z", W, "2022.02.02"),
    ];
    let base = build(dir, &games);
    let db = open(&base);
    let t = table(&db, None);
    (db, t)
}

fn run(t: &PlayerTable, f: impl FnOnce(&mut PlayerQuery)) -> Vec<PlayerFound> {
    let mut q = PlayerQuery::default();
    f(&mut q);
    t.query(&q, None)
}

#[test]
fn the_prefix_is_case_insensitive_for_unicode_and_does_not_ignore_spaces() {
    let dir = tempfile::tempdir().unwrap();
    let (_db, t) = small_table(dir.path());
    assert_eq!(names(&run(&t, |q| q.prefix = "ábel".into())), ["Ábel, A"]);
    assert_eq!(names(&run(&t, |q| q.prefix = "ÁBEL, ".into())), ["Ábel, A"]);
    assert_eq!(names(&run(&t, |q| q.prefix = "éVA".into())), ["Éva, E"]);
    assert_eq!(
        names(&run(&t, |q| q.prefix = "Z".into())).len(),
        2,
        "Zed, Z and zeta, z"
    );
    assert_eq!(
        names(&run(&t, |q| q.prefix = "ábel,a".into())).len(),
        0,
        "the space counts"
    );
    assert_eq!(run(&t, |q| q.prefix = "nobody".into()).len(), 0);
    assert_eq!(run(&t, |q| q.prefix = "\u{0}".into()).len(), 0);
}

#[test]
fn name_order_ignores_case_first_and_uses_it_to_break_ties() {
    let dir = tempfile::tempdir().unwrap();
    let games = vec![
        dated("b, b", "Zz", W, "2020.01.01"),
        dated("B, B", "Zz", W, "2020.01.01"),
        dated("a, a", "Zz", W, "2020.01.01"),
        dated("C, C", "Zz", W, "2020.01.01"),
    ];
    let base = build(dir.path(), &games);
    let t = table(&open(&base), None);
    let got = names(&run(&t, |_| {}))
        .iter()
        .map(|s| (*s).to_owned())
        .collect::<Vec<_>>();
    assert_eq!(got, ["a, a", "B, B", "b, b", "C, C", "Zz"]);
}

#[rstest]
#[case::name(PlayerSort::Name, None, &["Mid, M", "Zed, Z", "zeta, z", "Ábel, A", "Éva, E"])]
#[case::games(PlayerSort::Games, None, &["Zed, Z", "Ábel, A", "Mid, M", "zeta, z", "Éva, E"])]
#[case::elo(PlayerSort::Elo, None, &["zeta, z", "Ábel, A", "Mid, M", "Zed, Z", "Éva, E"])]
#[case::oldest(PlayerSort::Oldest, None, &["Mid, M", "Ábel, A", "Zed, Z", "zeta, z", "Éva, E"])]
#[case::newest(PlayerSort::Newest, None, &["Zed, Z", "Éva, E", "zeta, z", "Ábel, A", "Mid, M"])]
#[case::games_reversed(PlayerSort::Games, Some(false), &["zeta, z", "Éva, E", "Mid, M", "Ábel, A", "Zed, Z"])]
#[case::name_reversed(PlayerSort::Name, Some(true), &["Éva, E", "Ábel, A", "zeta, z", "Zed, Z", "Mid, M"])]
#[case::elo_ascending(PlayerSort::Elo, Some(false), &["Éva, E", "Zed, Z", "Mid, M", "Ábel, A", "zeta, z"])]
fn orders_and_their_directions(
    #[case] sort: PlayerSort,
    #[case] descending: Option<bool>,
    #[case] expected: &[&str],
) {
    let dir = tempfile::tempdir().unwrap();
    let (_db, t) = small_table(dir.path());
    let rows = run(&t, |q| {
        q.sort = sort;
        q.descending = descending;
    });
    assert_eq!(names(&rows), expected);
}

#[test]
fn oldest_puts_players_without_a_known_year_last_and_newest_too() {
    let dir = tempfile::tempdir().unwrap();
    let games = vec![
        dated("Dated, D", "Undated, U", W, "????.??.??"),
        dated("Dated, D", "Other, O", W, "2020.01.01"),
    ];
    let base = build(dir.path(), &games);
    let t = table(&open(&base), None);
    for sort in [PlayerSort::Oldest, PlayerSort::Newest] {
        let rows = run(&t, |q| q.sort = sort);
        assert_eq!(names(&rows).last(), Some(&"Undated, U"), "{sort:?}");
    }
}

#[test]
fn ranges_are_inclusive_and_an_inverted_range_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let (_db, t) = small_table(dir.path());
    // Games: Zed 5, Ábel 3, Mid 2, Éva 1, zeta 1.
    assert_eq!(names(&run(&t, |q| q.games = 3..=3)), ["Ábel, A"]);
    assert_eq!(run(&t, |q| q.games = 1..=1).len(), 2);
    assert_eq!(run(&t, |q| q.games = 6..=100).len(), 0);
    assert_eq!(run(&t, |q| q.games = 0..=0).len(), 0, "nobody has 0 games");
    let (high, low) = (5, 2);
    assert_eq!(run(&t, |q| q.games = high..=low).len(), 0);
    // Elo: Zed 1300, Ábel 2100, zeta 3000, Mid 1500, Éva 0.
    assert_eq!(names(&run(&t, |q| q.elo = 3000..=3000)), ["zeta, z"]);
    assert_eq!(names(&run(&t, |q| q.elo = 0..=0)), ["Éva, E"]);
    assert_eq!(run(&t, |q| q.elo = 1300..=1500).len(), 2);
    assert_eq!(run(&t, |q| q.elo = 3001..=4000).len(), 0);
}

#[test]
fn the_default_query_is_the_one_of_the_window() {
    let q = PlayerQuery::default();
    assert_eq!(
        (q.games.clone(), q.elo.clone(), q.limit),
        (1..=100_000, 0..=4000, 1000)
    );
    assert_eq!(
        (q.sort, q.descending, q.country),
        (PlayerSort::Name, None, CountryFilter::No)
    );
    assert_eq!(q.prefix, "");
}

#[test]
fn the_limit_cuts_after_ordering_and_zero_means_no_limit() {
    let dir = tempfile::tempdir().unwrap();
    let (_db, t) = small_table(dir.path());
    assert_eq!(
        names(&run(&t, |q| {
            q.sort = PlayerSort::Games;
            q.limit = 2;
        })),
        ["Zed, Z", "Ábel, A"]
    );
    assert_eq!(run(&t, |q| q.limit = 1).len(), 1);
    assert_eq!(run(&t, |q| q.limit = 0).len(), 5);
    assert_eq!(run(&t, |q| q.limit = 1_000_000).len(), 5);
}

#[test]
fn rows_carry_every_column() {
    let dir = tempfile::tempdir().unwrap();
    let (_db, t) = small_table(dir.path());
    let abel = run(&t, |q| q.prefix = "ábel".into()).remove(0);
    assert_eq!(abel.games, 3);
    assert_eq!(abel.first.to_string(), "2018.02.02");
    assert_eq!(abel.last.to_string(), "2020.01.01");
    assert_eq!(abel.max_elo, 2100);
    assert_eq!(abel.country, None);
}

#[test]
fn country_modes() {
    let dir = tempfile::tempdir().unwrap();
    let (_db, t) = small_table(dir.path());
    let ssp = "@PLAYER \"\"\nÁbel, A #- ARG/BRA [1] 1980\nMid, M #- - [1] 1980\nZed, Z #- ESP [1] 1980\nzeta, z #- BRA [1] 1980\n";
    let set = read(ssp);
    let sp = Some(set.checker(NameKind::Player));
    let q = |c: CountryFilter, sort: PlayerSort| PlayerQuery {
        country: c,
        sort,
        ..PlayerQuery::default()
    };
    // "All" shows the last country; no comment at all is "~~~"; a comment
    // without a country is empty.
    let rows = t.query(&q(CountryFilter::All, PlayerSort::Name), sp);
    let by_name: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (&*r.name, r.country.as_deref().unwrap()))
        .collect();
    assert_eq!(
        by_name,
        [
            ("Mid, M", ""),
            ("Zed, Z", "ESP"),
            ("zeta, z", "BRA"),
            ("Ábel, A", "BRA"),
            ("Éva, E", "~~~"),
        ]
    );
    // Sorting by country: empty, then codes, then ~~~; ties by name.
    let rows = t.query(&q(CountryFilter::All, PlayerSort::Country), sp);
    assert_eq!(
        names(&rows),
        ["Mid, M", "zeta, z", "Ábel, A", "Zed, Z", "Éva, E"]
    );
    // Prefix filter: any case, prefix only, nothing matches "" vs ~~~ unless empty.
    let only = |p: &str| {
        names(&t.query(&q(CountryFilter::Prefix(p.into()), PlayerSort::Name), sp))
            .iter()
            .map(|s| (*s).to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(only("bra"), ["zeta, z", "Ábel, A"]);
    assert_eq!(only("B"), ["zeta, z", "Ábel, A"]);
    assert_eq!(only("E"), ["Zed, Z"]);
    assert_eq!(only("~"), ["Éva, E"]);
    assert_eq!(only("BRAZ").len(), 0);
    assert_eq!(
        only("").len(),
        5,
        "an empty prefix matches everyone, ~~~ and empty included"
    );
}

#[test]
fn country_filter_no_never_looks_at_the_spelling() {
    let dir = tempfile::tempdir().unwrap();
    let (_db, t) = small_table(dir.path());
    let set = read("@PLAYER\nZed, Z #- ESP [1] 1980\n");
    let rows = t.query(&PlayerQuery::default(), Some(set.checker(NameKind::Player)));
    assert!(rows.iter().all(|r| r.country.is_none()));
}

// ---------------------------------------------------------------------------
// find_tournaments (rules 91 to 100).
// ---------------------------------------------------------------------------

fn wide() -> TournamentQuery {
    TournamentQuery {
        games: 1..=9999,
        players: 1..=999,
        limit: 0,
        ..TournamentQuery::default()
    }
}

fn tours(db: &Db, q: &TournamentQuery) -> Vec<TournamentRow> {
    find_tournaments(db, q, &CancelToken::new(), &mut NoProgress).unwrap()
}

fn groups(db: &Db, q: &TournamentQuery) -> Vec<Vec<u32>> {
    let mut v: Vec<Vec<u32>> = tours(db, q)
        .iter()
        .map(|r| r.game_numbers.to_vec())
        .collect();
    v.sort();
    v
}

fn at(date: &'static str) -> G {
    dated("A", "B", W, date)
}

#[test]
fn the_window_is_anchored_on_the_first_game_and_does_not_chain() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(
        dir.path(),
        &[at("2020.01.10"), at("2020.04.05"), at("2020.07.01")],
    );
    assert_eq!(groups(&open(&base), &wide()), [vec![1, 2], vec![3]]);
}

#[test]
fn the_window_bounds_are_exactly_plus_and_minus_three_months_keeping_the_day() {
    let dir = tempfile::tempdir().unwrap();
    let games = [
        at("2020.04.20"), // anchor of T1: window 2020.01.20 .. 2020.07.20
        at("2020.01.20"), // in (lower bound)
        at("2020.01.19"), // out: starts T2 (window 2019.10.19 .. 2020.04.19)
        at("2020.07.20"), // in T1 (upper bound)
        at("2020.07.21"), // out of both: T3
        at("2020.04.19"), // in T1 and in T2: the one of the lowest first game
    ];
    let base = build(dir.path(), &games);
    assert_eq!(
        groups(&open(&base), &wide()),
        [vec![1, 2, 4, 6], vec![3], vec![5]]
    );
}

#[test]
fn a_game_that_fits_two_tournaments_goes_to_the_one_that_began_first() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(
        dir.path(),
        &[at("2020.01.10"), at("2020.04.20"), at("2020.03.01")],
    );
    assert_eq!(groups(&open(&base), &wide()), [vec![1, 3], vec![2]]);
}

#[test]
fn year_wrap_of_the_window() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(
        dir.path(),
        &[
            at("2020.11.15"),
            at("2021.02.15"),
            at("2021.02.16"),
            at("2020.08.15"),
            at("2020.08.14"),
        ],
    );
    assert_eq!(
        groups(&open(&base), &wide()),
        [vec![1, 2, 4], vec![3], vec![5]]
    );
}

#[test]
fn an_unknown_month_moves_the_window_to_october_and_march() {
    let dir = tempfile::tempdir().unwrap();
    let games = [
        at("2020.??.??"), // window 2019.10.00 .. 2020.03.00
        at("2020.03.??"), // in (day 0)
        at("2020.03.01"), // out: the day 0 of the bound is kept
        at("2019.10.01"), // in
        at("2019.09.30"), // out
        at("2019.10.??"), // in
    ];
    let base = build(dir.path(), &games);
    assert_eq!(
        groups(&open(&base), &wide()),
        [vec![1, 2, 4, 6], vec![3], vec![5]]
    );
}

#[test]
fn only_the_same_event_and_the_same_site_belong_together() {
    let dir = tempfile::tempdir().unwrap();
    let games = vec![
        dated("A", "B", W, "2020.01.10"),
        g("A", "B", W).on("Other", "S", "2020.01.10"),
        g("A", "B", W).on("E", "Other", "2020.01.10"),
        dated("A", "B", W, "2020.01.11"),
        g("A", "B", W).on("e", "S", "2020.01.10"),
    ];
    let base = build(dir.path(), &games);
    assert_eq!(
        groups(&open(&base), &wide()),
        [vec![1, 4], vec![2], vec![3], vec![5]]
    );
}

#[test]
fn a_known_event_date_joins_games_far_apart_and_unknown_ones_never_match() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = at("2020.01.10");
    a.event_date = "2020.01.01";
    let mut b = at("2021.09.09");
    b.event_date = "2020.01.01";
    let mut c = at("2021.09.10");
    c.event_date = "2020.01.02";
    let d_ = at("2030.01.01");
    let e = at("2040.01.01");
    let base = build(dir.path(), &[a, b, c, d_, e]);
    assert_eq!(
        groups(&open(&base), &wide()),
        [vec![1, 2], vec![3], vec![4], vec![5]]
    );
}

#[test]
fn games_without_a_year_never_join_a_tournament_in_an_si4_database() {
    // Rule 91 lets a dateless game join through an equal EventDate, but the
    // SI4 index stores the EventDate as an offset from the year of the game
    // (docs/formatos/si4.md): with no game year it is lost, so each dateless
    // game is a tournament of its own. (A format limitation, not a defect.)
    let dir = tempfile::tempdir().unwrap();
    let mut a = at("????.??.??");
    a.event_date = "2020.01.01";
    let mut b = at("????.??.??");
    b.event_date = "2020.01.01";
    let c = at("????.??.??");
    let base = build(dir.path(), &[a, b, c]);
    let db = open(&base);
    assert!(db.entry(1).unwrap().event_date.is_unknown());
    assert_eq!(groups(&db, &wide()), [vec![1], vec![2], vec![3]]);
}

#[test]
fn the_start_date_is_the_earliest_and_ties_keep_the_first_game() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(
        dir.path(),
        &[at("2020.01.20"), at("2020.01.10"), at("2020.01.10")],
    );
    let rows = tours(&open(&base), &wide());
    assert_eq!(
        (rows[0].first_game, rows[0].date.to_string()),
        (2, "2020.01.10".to_owned())
    );
    // A tournament made of one dateless game: the date is unknown.
    let sub = dir.path().join("undated");
    std::fs::create_dir(&sub).unwrap();
    let base = build(&sub, &[at("????.??.??")]);
    let rows = tours(&open(&base), &wide());
    assert!(rows[0].date.is_unknown());
    assert_eq!(rows[0].first_game, 1);
}

#[test]
fn game_numbers_are_ascending_one_based_and_complete() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(
        dir.path(),
        &[
            at("2020.01.10"),
            dated("A", "B", W, "2020.01.12"),
            at("2020.01.11"),
        ],
    );
    let rows = tours(&open(&base), &wide());
    assert_eq!(&*rows[0].game_numbers, [1, 2, 3]);
    assert_eq!(rows[0].games, 3);
}

#[test]
fn mean_elo_counts_every_non_zero_rating_once_and_rounds_half_up() {
    let dir = tempfile::tempdir().unwrap();
    let cases: [(&[(u16, u16)], u16); 5] = [
        (&[(2400, 2401)], 2401),               // 2400.5 + 0.5
        (&[(2400, 2400), (2401, 0)], 2400),    // 7201 / 3 = 2400.33
        (&[(2400, 2401), (2401, 2402)], 2401), // 9604 / 4 = 2401
        (&[(0, 0), (0, 0)], 0),
        (&[(1000, 0), (1001, 0)], 1001), // 1000.5 -> 1001
    ];
    for (i, (elos, expected)) in cases.into_iter().enumerate() {
        let sub = dir.path().join(format!("case{i}"));
        std::fs::create_dir(&sub).unwrap();
        let games: Vec<G> = elos
            .iter()
            .map(|&(w, b)| at("2020.01.10").elo(w, b))
            .collect();
        let base = build(&sub, &games);
        let rows = tours(&open(&base), &wide());
        assert_eq!(rows[0].mean_elo, expected, "case {i}");
    }
}

#[test]
fn players_are_distinct_and_scores_give_one_point_for_draws_and_unfinished_games() {
    let dir = tempfile::tempdir().unwrap();
    let games = vec![
        dated("P1", "P2", W, "2020.01.10"),
        dated("P2", "P3", D, "2020.01.11"),
        dated("P3", "P1", U, "2020.01.12"),
        dated("P1", "P1", D, "2020.01.13"),
    ];
    let base = build(dir.path(), &games);
    let rows = tours(&open(&base), &wide());
    let t = &rows[0];
    assert_eq!((t.players, t.games), (3, 4));
    // P1: 2 (win) + 1 (unfinished as Black) + 1 + 1 (draw against itself, both sides) = 5.
    let w = t.winner.as_ref().unwrap();
    assert_eq!(
        (&*w.name, w.points2, w.points_text()),
        ("P1", 5, "2.5".to_owned())
    );
    // P3: a draw and an unfinished game = 2; P2: a loss and a draw = 1.
    let r = t.runner_up.as_ref().unwrap();
    assert_eq!((&*r.name, r.points2), ("P3", 2));
}

#[test]
fn winner_ties_go_to_the_higher_elo_then_to_the_player_added_last() {
    let dir = tempfile::tempdir().unwrap();
    // Same score, different Elo: the higher Elo wins even if added first.
    let base = build(
        dir.path(),
        &[dated("Low", "High", D, "2020.01.10").elo(2000, 2500)],
    );
    let t = &tours(&open(&base), &wide())[0];
    assert_eq!(&*t.winner.as_ref().unwrap().name, "High");
    assert_eq!(&*t.runner_up.as_ref().unwrap().name, "Low");
    // Same score and Elo: the player added last (Black of the first game).
    let sub = dir.path().join("t2");
    std::fs::create_dir(&sub).unwrap();
    let base = build(&sub, &[dated("First", "Second", D, "2020.01.10")]);
    let t = &tours(&open(&base), &wide())[0];
    assert_eq!(&*t.winner.as_ref().unwrap().name, "Second");
    assert_eq!(&*t.runner_up.as_ref().unwrap().name, "First");
    assert_eq!(t.winner.as_ref().unwrap().points_text(), "0.5");
}

#[test]
fn a_standing_carries_the_highest_elo_the_player_had_in_the_tournament() {
    let dir = tempfile::tempdir().unwrap();
    let games = vec![
        dated("Ann", "Bob", W, "2020.01.10").elo(2300, 2000),
        dated("Bob", "Ann", D, "2020.01.11").elo(2100, 2200),
    ];
    let base = build(dir.path(), &games);
    let t = &tours(&open(&base), &wide())[0];
    let w = t.winner.as_ref().unwrap();
    assert_eq!((&*w.name, w.elo, w.points2), ("Ann", 2300, 3));
    assert_eq!(t.runner_up.as_ref().unwrap().elo, 2100);
}

#[test]
fn a_one_player_tournament_has_no_runner_up() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &[dated("Solo", "Solo", D, "2020.01.10")]);
    let t = &tours(&open(&base), &wide())[0];
    assert_eq!(t.players, 1);
    assert!(t.winner.is_some() && t.runner_up.is_none());
}

fn varied(dir: &Path) -> Db {
    let games = vec![
        // Tournament 1: "Open" at "Zulu BRA", 2019, 2 games, 2 players.
        g("Mike, M", "Nina, N", W)
            .on("Open", "Zulu BRA", "2019.05.05")
            .elo(2400, 2300),
        g("Nina, N", "Mike, M", D)
            .on("Open", "Zulu BRA", "2019.05.06")
            .elo(2300, 2400),
        // Tournament 2: "open" at "alpha PER", 2021, 4 games, 3 players.
        g("Cora, C", "Dan, D", B)
            .on("open", "alpha PER", "2021.02.02")
            .elo(2600, 2650),
        g("Dan, D", "Eli, E", W)
            .on("open", "alpha PER", "2021.02.03")
            .elo(2650, 2000),
        g("Eli, E", "Cora, C", D)
            .on("open", "alpha PER", "2021.02.04")
            .elo(2000, 2600),
        g("Cora, C", "Dan, D", D)
            .on("open", "alpha PER", "2021.02.05")
            .elo(2600, 2650),
        // Tournament 3: "Cup" at "Zulu BRA", 2020, 1 game.
        g("Zack, Z", "Yves, Y", W)
            .on("Cup", "Zulu BRA", "2020.07.07")
            .elo(1800, 1700),
    ];
    open(&build(dir, &games))
}

#[rstest]
#[case::date(TournamentSort::Date, &[2021, 2020, 2019])]
#[case::players(TournamentSort::Players, &[2021, 2019, 2020])]
#[case::games(TournamentSort::Games, &[2021, 2019, 2020])]
#[case::elo(TournamentSort::Elo, &[2021, 2019, 2020])]
#[case::event(TournamentSort::Event, &[2020, 2019, 2021])]
#[case::site(TournamentSort::Site, &[2021, 2019, 2020])]
#[case::winner(TournamentSort::Winner, &[2021, 2019, 2020])]
fn tournament_orders(#[case] sort: TournamentSort, #[case] years: &[u16]) {
    let dir = tempfile::tempdir().unwrap();
    let db = varied(dir.path());
    let q = TournamentQuery { sort, ..wide() };
    let got: Vec<u16> = tours(&db, &q)
        .iter()
        .map(|r| r.date.year().unwrap())
        .collect();
    assert_eq!(got, years, "{sort:?}");
}

#[test]
fn tournament_filters_apply_to_the_whole_tournament() {
    let dir = tempfile::tempdir().unwrap();
    let db = varied(dir.path());
    let years = |f: &dyn Fn(&mut TournamentQuery)| -> Vec<u16> {
        let mut q = TournamentQuery {
            sort: TournamentSort::Date,
            ..wide()
        };
        f(&mut q);
        tours(&db, &q)
            .iter()
            .map(|r| r.date.year().unwrap())
            .collect()
    };
    assert_eq!(years(&|q| q.games = 2..=3), [2019]);
    assert_eq!(years(&|q| q.games = 1..=1), [2020]);
    assert_eq!(years(&|q| q.players = 3..=3), [2021]);
    assert_eq!(years(&|q| q.mean_elo = 2300..=2400), [2019]);
    assert_eq!(years(&|q| q.mean_elo = 2460..=2500), [2021]);
    assert_eq!(years(&|q| q.mean_elo = 0..=1800), [2020]);
    assert_eq!(years(&|q| q.player = "dAn".into()), [2021]);
    assert_eq!(
        years(&|q| q.player = "a, n".into()),
        [2019],
        "spaces are ignored"
    );
    assert_eq!(years(&|q| q.player = "nina,n".into()), [2019]);
    assert_eq!(years(&|q| q.event = "CUP".into()), [2020]);
    assert_eq!(years(&|q| q.event = "o p e n".into()), [2021, 2019]);
    assert_eq!(years(&|q| q.site = "zulu".into()), [2020, 2019]);
    assert_eq!(years(&|q| q.site = "z u l u".into()), [2020, 2019]);
    assert_eq!(years(&|q| q.country = Some(*b"BRA")), [2020, 2019]);
    assert_eq!(
        years(&|q| q.country = Some(*b"bra")).len(),
        0,
        "the code is compared as written"
    );
    assert_eq!(years(&|q| q.country = Some(*b"PER")), [2021]);
    assert_eq!(years(&|q| q.limit = 2), [2021, 2020]);
    assert_eq!(years(&|q| q.limit = 0).len(), 3);
    assert_eq!(years(&|q| q.player = "zzz".into()).len(), 0);
    assert_eq!(
        years(&|q| {
            q.games = std::hint::black_box(5)..=1;
        })
        .len(),
        0
    );
}

#[test]
fn the_player_filter_does_not_shrink_the_tournament() {
    let dir = tempfile::tempdir().unwrap();
    let db = varied(dir.path());
    let q = TournamentQuery {
        player: "eli".into(),
        ..wide()
    };
    let rows = tours(&db, &q);
    assert_eq!((rows[0].games, rows[0].players), (4, 3));
}

#[test]
fn date_limits_filter_games_before_grouping() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(
        dir.path(),
        &[
            at("2020.01.10"),
            at("2020.02.10"),
            at("2020.03.10"),
            at("????.??.??"),
        ],
    );
    let db = open(&base);
    let sel = |start: &str, end: &str| -> Vec<Vec<u32>> {
        let q = TournamentQuery {
            start: date(start),
            end: date(end),
            ..wide()
        };
        groups(&db, &q)
    };
    assert_eq!(sel("2020.02.??", "????.??.??"), [vec![2, 3]]);
    assert_eq!(
        sel("2020.02.10", "2020.02.10"),
        [vec![2]],
        "both limits are inclusive"
    );
    assert_eq!(sel("2020", "2020.01.31"), [vec![1]]);
    assert_eq!(
        sel("2020.01.??", "2020.??.??"),
        [] as [Vec<u32>; 0],
        "unknown parts of the end count as 0, so 2020.??.?? ends before 2020.01.10"
    );
    assert_eq!(
        sel("????.??.??", "????.??.??"),
        [vec![1, 2, 3], vec![4]],
        "no limits: the undated game too, as a tournament of its own"
    );
    assert_eq!(
        sel("????.??.??", "2020.02.10"),
        [vec![1, 2], vec![4]],
        "dateless is 0, below any end"
    );
    assert_eq!(sel("2021.??.??", "????.??.??").len(), 0);
}

#[test]
fn the_defaults_of_the_query_and_of_the_window() {
    let c = TournamentQuery::default();
    assert!(c.start.is_unknown() && c.end.is_unknown());
    assert_eq!(
        (
            c.players.clone(),
            c.games.clone(),
            c.mean_elo.clone(),
            c.limit
        ),
        (2..=999, 1..=9999, 0..=4000, 100)
    );
    assert_eq!(
        (
            c.sort,
            c.country,
            c.site.as_str(),
            c.event.as_str(),
            c.player.as_str()
        ),
        (TournamentSort::Date, None, "", "", "")
    );
    let w = TournamentQuery::window_defaults(2026);
    assert_eq!(w.start.to_string(), "1960.??.??");
    assert_eq!(w.end.to_string(), "2026.12.31");
    assert_eq!(
        (
            w.players.clone(),
            w.games.clone(),
            w.mean_elo.clone(),
            w.limit
        ),
        (4..=1999, 10..=9999, 0..=4000, 1000)
    );
    assert_eq!(w.sort, TournamentSort::Date);
}

#[test]
fn ties_of_every_order_fall_to_the_first_game_number() {
    let dir = tempfile::tempdir().unwrap();
    // Three identical-looking tournaments, far apart in time.
    let mut games = Vec::new();
    for year in ["2000", "2005", "2010"] {
        let when: &'static str = match year {
            "2000" => "2000.01.01",
            "2005" => "2005.01.01",
            _ => "2010.01.01",
        };
        games.push(g("A", "B", W).on("E", "S", when).elo(2000, 2000));
    }
    let base = build(dir.path(), &games);
    let db = open(&base);
    for sort in [
        TournamentSort::Players,
        TournamentSort::Games,
        TournamentSort::Elo,
        TournamentSort::Event,
        TournamentSort::Site,
        TournamentSort::Winner,
    ] {
        let rows = tours(&db, &TournamentQuery { sort, ..wide() });
        let firsts: Vec<u32> = rows.iter().map(|r| r.first_game).collect();
        assert_eq!(firsts, [1, 2, 3], "{sort:?}");
    }
}

#[test]
fn tournaments_are_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let db = varied(dir.path());
    let a = tours(&db, &wide());
    for _ in 0..5 {
        assert_eq!(tours(&db, &wide()), a);
    }
}

#[test]
fn finding_tournaments_can_be_cancelled_and_reports_progress() {
    let dir = tempfile::tempdir().unwrap();
    let games: Vec<G> = (0..6000).map(|_| at("2020.01.10")).collect();
    let base = build(dir.path(), &games);
    let db = open(&base);
    let mut calls = Vec::new();
    let rows = find_tournaments(&db, &wide(), &CancelToken::new(), &mut |d, t| {
        calls.push((d, t));
    })
    .unwrap();
    assert_eq!(rows[0].games, 6000);
    assert!(calls.windows(2).all(|w| w[0].0 <= w[1].0));
    assert_eq!(calls.last().map(|c| c.0 == c.1), Some(true));
    let token = CancelToken::new();
    let t2 = token.clone();
    assert!(find_tournaments(&db, &wide(), &token, &mut move |_, _| t2.cancel()).is_err());
}

// ---------------------------------------------------------------------------
// player_info (rules 81 to 87).
// ---------------------------------------------------------------------------

fn info(
    db: &Db,
    name: &str,
    filter: Option<&Filter>,
    opponent: Option<&str>,
) -> mychess_db::player_info::PlayerInfo {
    player_info(
        db,
        name,
        filter,
        opponent,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap()
}

#[test]
fn info_counts_from_the_point_of_view_of_the_player() {
    let dir = tempfile::tempdir().unwrap();
    let games = vec![
        dated("Pat", "Q", W, "2020.01.01"),
        dated("Pat", "Q", B, "2020.01.02"),
        dated("Pat", "Q", D, "2020.01.03"),
        dated("Pat", "Q", U, "2020.01.04"),
        dated("Q", "Pat", W, "2020.01.05"),
        dated("Q", "Pat", B, "2020.01.06"),
        dated("Q", "Pat", D, "2020.01.07"),
        dated("Q", "Pat", U, "2020.01.08"),
    ];
    let base = build(dir.path(), &games);
    let i = info(&open(&base), "Pat", None, None);
    let t = |x: mychess_db::player_info::Tally| (x.wins, x.draws, x.losses, x.unfinished);
    assert_eq!(t(i.all.white), (1, 1, 1, 1));
    assert_eq!(
        t(i.all.black),
        (1, 1, 1, 1),
        "a Black win is a win of the player"
    );
    assert_eq!(t(i.all.total), (2, 2, 2, 2));
    assert_eq!(i.all.total.points2(), 8);
    assert_eq!(i.all.total.percent_hundredths(), 5000);
    assert_eq!(i.all.total.points_text(), "4.0");
    assert!(i.filter.is_none() && i.versus.is_none());
}

#[test]
fn a_game_against_oneself_counts_once_for_white() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &[dated("Pat", "Pat", W, "2020.01.01")]);
    let i = info(&open(&base), "Pat", None, None);
    assert_eq!(
        (
            i.all.total.games(),
            i.all.white.games(),
            i.all.black.games()
        ),
        (1, 1, 0)
    );
    assert_eq!(i.all.white.wins, 1);
}

#[rstest]
#[case(0, 0, 0, 0, 0, "0.0")]
#[case(3, 1, 0, 0, 8750, "3.5")]
#[case(0, 0, 0, 1, 5000, "0.5")]
#[case(1, 0, 2, 0, 3333, "1.0")]
#[case(2, 0, 1, 0, 6666, "2.0")]
#[case(0, 0, 5, 0, 0, "0.0")]
#[case(7, 0, 0, 0, 10000, "7.0")]
#[case(0, 3, 0, 0, 5000, "1.5")]
fn tally_arithmetic(
    #[case] wins: u32,
    #[case] draws: u32,
    #[case] losses: u32,
    #[case] unfinished: u32,
    #[case] hundredths: u32,
    #[case] points: &str,
) {
    let t = mychess_db::player_info::Tally {
        wins,
        draws,
        losses,
        unfinished,
    };
    assert_eq!(t.percent_hundredths(), hundredths);
    assert_eq!(t.points_text(), points);
    assert_eq!(t.games(), wins + draws + losses + unfinished);
}

#[test]
fn info_dates_ignore_games_without_a_year() {
    let dir = tempfile::tempdir().unwrap();
    let games = vec![
        dated("Pat", "Q", W, "????.05.05"),
        dated("Pat", "Q", W, "2020.06.01"),
        dated("Pat", "Q", W, "2018.??.??"),
        dated("Pat", "Q", W, "????.??.??"),
    ];
    let base = build(dir.path(), &games);
    let i = info(&open(&base), "Pat", None, None);
    assert_eq!(i.first.to_string(), "2018.??.??");
    assert_eq!(i.last.to_string(), "2020.06.01");
    assert_eq!(i.all.total.games(), 4);
}

#[test]
fn info_openings_group_by_ten_limit_to_six_and_score_per_colour() {
    let dir = tempfile::tempdir().unwrap();
    let mut games = Vec::new();
    // Eight groups as White: A0 x1 ... H0 x1, plus extra games to order them.
    let ecos = ["A05", "B12", "C20", "D30", "E40", "A10", "B22", "C99"];
    for e in ecos {
        games.push(dated("Pat", "Q", W, "2020.01.01").eco(e));
    }
    // B1 group gets two more games (win, loss) -> 3 games, score (2+2+0)*50/3 = 66.
    games.push(dated("Pat", "Q", W, "2020.01.01").eco("B19"));
    games.push(dated("Pat", "Q", B, "2020.01.01").eco("B15"));
    // As Black: one draw in A00 and a game with no ECO that must not count.
    games.push(dated("Q", "Pat", D, "2020.01.01").eco("A00"));
    games.push(dated("Q", "Pat", D, "2020.01.01"));
    let base = build(dir.path(), &games);
    let i = info(&open(&base), "Pat", None, None);
    let white: Vec<(String, u32, u32)> = i.openings[0]
        .iter()
        .map(|o| (o.label(), o.games, o.score_percent))
        .collect();
    assert_eq!(white.len(), 6, "at most six groups");
    assert_eq!(white[0], ("B1".to_owned(), 3, 66));
    // The rest have one win each (100%), in group order: A0 A1 B2 C2 C9 ... first five by index.
    let rest: Vec<&str> = white[1..].iter().map(|w| w.0.as_str()).collect();
    assert_eq!(rest, ["A0", "A1", "B2", "C2", "C9"]);
    assert!(white[1..].iter().all(|w| w.1 == 1 && w.2 == 100));
    let black: Vec<(String, u32, u32)> = i.openings[1]
        .iter()
        .map(|o| (o.label(), o.games, o.score_percent))
        .collect();
    assert_eq!(black, [("A0".to_owned(), 1, 50)]);
}

#[test]
fn info_rating_history_uses_the_last_game_of_each_month_and_only_shows_changes() {
    let dir = tempfile::tempdir().unwrap();
    let games = vec![
        dated("Pat", "Q", W, "2020.01.05").elo(2400, 0),
        dated("Q", "Pat", W, "2020.01.25").elo(0, 2410),
        dated("Pat", "Q", W, "2020.02.01").elo(2410, 0),
        dated("Pat", "Q", W, "2020.03.01").elo(2500, 0),
        dated("Pat", "Q", W, "2020.04.01").elo(2500, 0),
        dated("Pat", "Q", W, "2020.05.??").elo(2450, 0),
        dated("Pat", "Q", W, "2020.06.01").elo(2600, 0),
        dated("Pat", "Q", W, "2020.06.02"),
        dated("Pat", "Q", W, "1850.01.01").elo(2000, 0),
        dated("Pat", "Q", W, "????.??.??").elo(2900, 0),
    ];
    let base = build(dir.path(), &games);
    let i = info(&open(&base), "Pat", None, None);
    assert_eq!(
        i.ratings,
        [(2020, 1, 2410), (2020, 3, 2500), (2020, 5, 2450)],
        "Feb repeats 2410, Apr repeats 2500, Jun's last game has no Elo; before 1900 and undated are out"
    );
}

#[test]
fn info_unknown_month_counts_as_january() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(
        dir.path(),
        &[dated("Pat", "Q", W, "2020.??.??").elo(2300, 0)],
    );
    assert_eq!(
        info(&open(&base), "Pat", None, None).ratings,
        [(2020, 1, 2300)]
    );
}

#[test]
fn info_for_a_missing_name_and_for_names_that_differ_in_case() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &[dated("Pat", "Q", W, "2020.01.01")]);
    let db = open(&base);
    for name in ["", "pat", "PAT", "Pat ", "Nobody"] {
        let i = info(&db, name, None, Some("Q"));
        assert!(!i.found, "{name:?}");
        assert_eq!(i.all.total.games(), 0);
        assert!(i.versus.is_none() && i.ratings.is_empty());
    }
}

#[test]
fn info_filter_section_counts_only_games_in_the_filter_and_flags_a_full_filter() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &six_games());
    let db = open(&base);
    let mut f = Filter::new(db.len());
    f.fill(0);
    f.set(2, 1);
    f.set(5, 1);
    let i = info(&db, "Alfa, A", Some(&f), None);
    let section = i.filter.unwrap();
    assert_eq!(
        section.total.games(),
        1,
        "only game 5 has Alfa in the filter"
    );
    assert_eq!(section.total.draws, 1);
    assert!(!i.filter_is_everything);
    f.fill(1);
    assert!(info(&db, "Alfa, A", Some(&f), None).filter_is_everything);
}

#[test]
fn info_against_an_opponent_that_never_played_is_an_empty_section() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &six_games());
    let db = open(&base);
    let i = info(&db, "Beta, B", None, Some("Delta, D"));
    let (name, versus) = i.versus.unwrap();
    assert_eq!(name, "Delta, D");
    assert_eq!(versus.total.games(), 0);
    assert_eq!(versus.total.percent_hundredths(), 0);
}

#[test]
fn info_can_be_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let games: Vec<G> = (0..6000).map(|_| at("2020.01.10")).collect();
    let base = build(dir.path(), &games);
    let db = open(&base);
    let token = CancelToken::new();
    let t2 = token.clone();
    assert!(player_info(&db, "A", None, None, &token, &mut move |_, _| t2.cancel()).is_err());
}

// ---------------------------------------------------------------------------
// player_games_filter (rule 88).
// ---------------------------------------------------------------------------

fn filter_with(db: &Db, games: &[u32]) -> Filter {
    let mut f = Filter::new(db.len());
    f.fill(0);
    for &n in games {
        f.set(n, 1);
    }
    f
}

fn link(
    db: &Db,
    name: &str,
    letters: &str,
    mode: PlayerFilterMode,
    start: &[u32],
    opponent: Option<&str>,
) -> Vec<u32> {
    let mut f = filter_with(db, start);
    let n = player_games_filter(
        db,
        name,
        GameSelection::from_letters(letters).unwrap(),
        mode,
        opponent,
        &mut f,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    let games: Vec<u32> = f.iter_games().collect();
    assert_eq!(usize::try_from(n).unwrap(), games.len());
    games
}

#[rstest]
#[case("a", &[1, 4, 5])]
#[case("A", &[3, 6])]
#[case("aA", &[1, 3, 4, 5, 6])]
#[case("w", &[1, 4])]
#[case("W", &[3])]
#[case("wW", &[1, 3, 4])]
#[case("d", &[5])]
#[case("D", &[])]
#[case("dD", &[5])]
#[case("l", &[])]
#[case("L", &[])]
#[case("lL", &[])]
#[case("wWdDlL", &[1, 3, 4, 5])]
#[case("", &[])]
#[case("wd", &[1, 4, 5])]
fn every_letter_set_for_alfa(#[case] letters: &str, #[case] expected: &[u32]) {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &six_games());
    let db = open(&base);
    let all = [1, 2, 3, 4, 5, 6];
    assert_eq!(
        link(&db, "Alfa, A", letters, PlayerFilterMode::Set, &all, None),
        expected,
        "-f{letters}"
    );
}

#[test]
fn a_result_of_none_is_only_reached_by_the_all_letters() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &six_games());
    let db = open(&base);
    let all = [1, 2, 3, 4, 5, 6];
    // Game 6 is `*` with Alfa as Black.
    assert!(link(&db, "Alfa, A", "A", PlayerFilterMode::Set, &all, None).contains(&6));
    assert!(!link(&db, "Alfa, A", "WDL", PlayerFilterMode::Set, &all, None).contains(&6));
}

#[test]
fn refine_removes_other_players_games_then_adds_the_selection() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &six_games());
    let db = open(&base);
    // No letters: only the removal.
    assert_eq!(
        link(
            &db,
            "Alfa, A",
            "",
            PlayerFilterMode::Refine,
            &[1, 2, 3, 4, 5, 6],
            None
        ),
        [1, 3, 4, 5, 6]
    );
    assert_eq!(
        link(&db, "Alfa, A", "", PlayerFilterMode::Refine, &[2], None).len(),
        0
    );
    // With letters: games that were not in the filter are added.
    assert_eq!(
        link(&db, "Alfa, A", "w", PlayerFilterMode::Refine, &[2], None),
        [1, 4]
    );
    assert_eq!(
        link(
            &db,
            "Alfa, A",
            "w",
            PlayerFilterMode::Refine,
            &[2, 3, 6],
            None
        ),
        [1, 3, 4, 6]
    );
}

#[test]
fn opponent_mode_replaces_the_filter_with_games_against_the_opponent() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &six_games());
    let db = open(&base);
    let all = [1, 2, 3, 4, 5, 6];
    assert_eq!(
        link(
            &db,
            "Alfa, A",
            "aA",
            PlayerFilterMode::Opponent,
            &all,
            Some("Beta, B")
        ),
        [1, 4]
    );
    assert_eq!(
        link(
            &db,
            "Alfa, A",
            "aA",
            PlayerFilterMode::Opponent,
            &all,
            Some("Gama, G")
        ),
        [3]
    );
    assert_eq!(
        link(
            &db,
            "Alfa, A",
            "w",
            PlayerFilterMode::Opponent,
            &all,
            Some("Gama, G")
        )
        .len(),
        0
    );
    assert_eq!(
        link(
            &db,
            "Alfa, A",
            "aA",
            PlayerFilterMode::Opponent,
            &all,
            Some("Nobody")
        )
        .len(),
        0
    );
    assert_eq!(
        link(&db, "Alfa, A", "aA", PlayerFilterMode::Opponent, &all, None).len(),
        0
    );
}

#[test]
fn an_unknown_player_selects_nothing_and_a_wrong_sized_filter_is_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &six_games());
    let db = open(&base);
    assert_eq!(
        link(&db, "Nobody", "aA", PlayerFilterMode::Set, &[1, 2], None).len(),
        0
    );
    let mut wrong = Filter::new(2);
    wrong.fill(1);
    let n = player_games_filter(
        &db,
        "Alfa, A",
        GameSelection::from_letters("aA").unwrap(),
        PlayerFilterMode::Set,
        None,
        &mut wrong,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!((n, wrong.count()), (2, 2), "left alone");
}

#[test]
fn a_cancelled_filter_link_leaves_the_filter_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let games: Vec<G> = (0..6000).map(|_| at("2020.01.10")).collect();
    let base = build(dir.path(), &games);
    let db = open(&base);
    let mut f = filter_with(&db, &[1, 2, 3]);
    let token = CancelToken::new();
    let t2 = token.clone();
    let r = player_games_filter(
        &db,
        "A",
        GameSelection::from_letters("aA").unwrap(),
        PlayerFilterMode::Set,
        None,
        &mut f,
        &token,
        &mut move |_, _| t2.cancel(),
    );
    assert!(r.is_err());
    assert_eq!(f.iter_games().collect::<Vec<_>>(), [1, 2, 3]);
}

#[test]
fn letters_outside_the_alphabet_of_the_scid_are_rejected() {
    for bad in ["x", "f", "aX", "1", " ", "é", "wW d"] {
        assert!(GameSelection::from_letters(bad).is_none(), "{bad:?}");
    }
    for good in ["", "a", "wWdDlLaA", "aaaa"] {
        assert!(GameSelection::from_letters(good).is_some(), "{good:?}");
    }
}

// ---------------------------------------------------------------------------
// spelling_header, FIDE link, the tables of titles and countries.
// ---------------------------------------------------------------------------

#[rstest]
#[case("FIDEID 4633865", Some("4633865"))]
#[case("FIDEID 1", Some("1"))]
#[case("FIDEID 123456789012", Some("123456789012"))]
#[case("FIDEID 1234567890123", None)]
#[case("FIDEID ", None)]
#[case("FIDEID", None)]
#[case("FIDEID 12a", None)]
#[case("FIDEID 12 3", None)]
#[case("FIDEID -1", None)]
#[case("FIDEID +1", None)]
#[case("FIDEID 1.5", None)]
#[case("FIDEID 123/../x", None)]
#[case("FIDEID ../../etc", None)]
#[case("FIDEID 12\u{0661}", None)]
#[case("fideid 123", None)]
#[case(" FIDEID 123", None)]
#[case("FIDEID\t123", None)]
#[case("FIDEID 0123", Some("0123"))]
#[case("World Champion 2007-2013", None)]
fn fide_ids(#[case] note: &str, #[case] expected: Option<&str>) {
    assert_eq!(fide_id_of(note), expected, "{note:?}");
}

#[test]
fn the_fide_url_is_only_made_from_a_validated_id() {
    assert_eq!(
        fide_profile_url("4633865"),
        "https://ratings.fide.com/profile/4633865"
    );
    let set = read(
        "@PLAYER\nA #- X [1] 1\n%Bio FIDEID 12x\n%Bio note\n%Bio FIDEID 77\n%Bio FIDEID 88\nB\n%Bio FIDEID ../etc\n",
    );
    let p = set.checker(NameKind::Player);
    assert_eq!(
        spelling_header(p, "A").unwrap().fide_id.as_deref(),
        Some("77"),
        "the first valid note"
    );
    assert_eq!(spelling_header(p, "B").unwrap().fide_id, None);
}

#[test]
fn the_header_lists_titles_and_countries_as_written_when_unknown() {
    let set = read(
        "@PLAYER\nA #im+cim URS/RUS/ZZZ [2520*] 1911.06.14--2004.02.29\nB #w - [....] ????\nC #xyz ARG [1500]\n",
    );
    let p = set.checker(NameKind::Player);
    let a = spelling_header(p, "A").unwrap();
    assert_eq!(a.title_codes, ["im", "cim"]);
    assert_eq!(a.title_names, ["International Master", "Correspondence IM"]);
    assert_eq!(
        a.country_names,
        ["URS", country_name("RUS").unwrap(), "ZZZ"]
    );
    assert_eq!((a.peak, a.peak_is_estimate), (2520, true));
    assert_eq!(a.born, "1911.06.14--2004.02.29");
    assert!(a.exact);
    let b = spelling_header(p, "B").unwrap();
    assert_eq!(
        b.title_names,
        ["w"],
        "w alone is not in the table: shown as written"
    );
    assert_eq!(b.country_names.len(), 0);
    assert_eq!(b.peak, 0);
    assert_eq!(b.born, "????");
    let c = spelling_header(p, "C").unwrap();
    assert_eq!(c.title_names, ["xyz"]);
    assert_eq!(c.country_names, [country_name("ARG").unwrap()]);
    assert!(spelling_header(p, "").is_none());
}

#[test]
fn the_header_marks_a_loose_match_as_approximate() {
    let set = read("@PLAYER\nPatterson, Paul #GM USA [2600] 1960\n= Pat, P\n");
    let p = set.checker(NameKind::Player);
    assert!(!spelling_header(p, "Patt").unwrap().exact);
    assert!(spelling_header(p, "Patterson, Paul").unwrap().exact);
    assert!(
        spelling_header(p, "Pat, P").unwrap().exact,
        "an alias key equal to the query"
    );
    assert_eq!(
        spelling_header(p, "Patt").unwrap().title_names,
        ["Grandmaster"]
    );
}

#[test]
fn tables_of_titles_and_countries() {
    assert_eq!(TITLE_TABLE.len(), 15);
    for (code, name) in [
        ("gm", "Grandmaster"),
        ("im", "International Master"),
        ("fm", "FIDE Master"),
        ("cm", "Candidate Master"),
        ("wgm", "Woman Grandmaster"),
        ("wim", "Woman International Master"),
        ("wfm", "Woman FIDE Master"),
        ("wcm", "Woman Candidate Master"),
        ("cgm", "Correspondence GM"),
        ("cim", "Correspondence IM"),
        ("csm", "Correspondence Senior Master"),
        ("clg", "Correspondence Woman Grandmaster"),
        ("clm", "Correspondence Woman Master"),
        ("hgm", "Honorary Grandmaster"),
        ("comp", "Computer"),
    ] {
        assert_eq!(title_name(code), Some(name), "{code}");
        assert_eq!(
            title_name(&code.to_uppercase()),
            Some(name),
            "{code} in capitals"
        );
    }
    assert_eq!(title_name("w"), None);
    assert_eq!(title_name("-"), None);
    assert_eq!(title_name(""), None);
    assert_eq!(country_name("ARG"), Some("Argentina"));
    assert_eq!(country_name("YUG"), Some("Yugoslavia"));
    assert_eq!(country_name("arg"), None, "codes are exact");
    assert_eq!(country_name("-"), None);
    assert_eq!(country_name(""), None);
    assert!(
        COUNTRY_TABLE
            .iter()
            .all(|(c, n)| c.len() == 3 && !n.is_empty())
    );
    assert!(
        COUNTRY_TABLE
            .iter()
            .all(|(_, n)| !n.contains("Columbia") && !n.contains("Herzogovina")),
        "D3358"
    );
}

/// The tables must agree with `countryTable` of `spellchk.cpp`, apart from
/// the two corrected spellings (D3358). Reads the C++ source of the
/// reference tree; run with `cargo xtask compat`.
#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_country_and_title_tables_match_spellchk_cpp() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/scid_vs_pc-4.27");
    assert!(
        dir.is_dir(),
        "Scid vs PC reference not found at {} (see docs/referencia-scid.md)",
        dir.display()
    );
    let source = std::fs::read_to_string(dir.join("src/spellchk.cpp")).unwrap();
    let entries = |table: &str| -> Vec<(String, String)> {
        let start = source.find(table).unwrap();
        let body = &source[start..];
        let body = &body[..body.find("};").unwrap()];
        body.lines()
            .filter_map(|l| {
                let l = l.trim().strip_prefix("{\"")?;
                let (code, rest) = l.split_once("\",\"").or_else(|| l.split_once("\", \""))?;
                let (name, _) = rest.split_once('"')?;
                Some((code.to_owned(), name.to_owned()))
            })
            .filter(|(c, _)| !c.is_empty())
            .collect()
    };
    let corrected = |name: &str| {
        name.replace("Columbia", "Colombia")
            .replace("Herzogovina", "Herzegovina")
    };
    let mut seen = std::collections::HashSet::new();
    // A code that appears twice in the original ("SCG") finds its first entry.
    for (code, name) in entries("countryTable[]") {
        if !seen.insert(code.clone()) {
            continue;
        }
        let ours = country_name(&code).unwrap_or_else(|| panic!("missing country {code}"));
        if country_name(&code) != Some(name.as_str()) {
            assert_eq!(ours, corrected(&name), "{code}");
        }
    }
    for (code, name) in entries("titleTable[]") {
        assert_eq!(title_name(&code), Some(name.as_str()), "{code}");
    }
}

#[test]
fn partial_dates_of_the_fixtures_are_what_the_tests_assume() {
    let d: PartialDate = date("2020.??.??");
    assert_eq!((d.year(), d.month()), (Some(2020), None));
}
