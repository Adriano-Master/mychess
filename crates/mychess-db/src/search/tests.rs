//! Sanity tests of the header search over a small database written to disk.
//! The full suite lives in `tests/`.

use std::path::PathBuf;

use mychess_core::{Elo, Game, GameResult, PartialDate, Rating, RatingType};

use crate::si4::{GameFlags, Si4Database, Si4Writer};
use crate::{
    CancelToken, EcoRange, Filter, FilterOp, GameEndKind, HeaderSearch, MinMax, NoProgress,
    PlayerStats, SearchError, Tri, search_header,
};

fn game(white: &str, black: &str, result: GameResult, date: &str, site: &str) -> Game {
    let mut g = Game::new();
    let mut at = g.root();
    for uci in ["e2e4", "e7e5"] {
        let mv = g.position_at(at).map(|p| p.parse_uci(uci));
        let Ok(Ok(mv)) = mv else { panic!("bad move") };
        at = g.add_move(at, mv).expect("legal move");
    }
    let t = g.tags_mut();
    t.white = white.into();
    t.black = black.into();
    t.result = result;
    t.date = PartialDate::parse_lenient(date);
    t.site = site.into();
    g
}

/// Four games; game 4 has a rated Kasparov, game 1 an unrated one.
fn build() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let base = dir.path().join("db");
    let mut w = Si4Writer::create(&base, 1).expect("create");
    let rate = |g: &mut Game, white: u16, black: u16| {
        let t = g.tags_mut();
        t.white_rating = Elo::new(white)
            .ok()
            .map(|e| Rating::new(e, RatingType::Elo));
        t.black_rating = Elo::new(black)
            .ok()
            .map(|e| Rating::new(e, RatingType::Elo));
    };
    let mut g1 = game(
        "Kasparov, Garry",
        "Karpov, Anatoly",
        GameResult::White,
        "1990.05.01",
        "Dallas USA",
    );
    rate(&mut g1, 0, 2700);
    let mut g2 = game(
        "Smith, John",
        "Doe, Jane",
        GameResult::Black,
        "2000.??.??",
        "Lausanne SUI",
    );
    rate(&mut g2, 2000, 1900);
    let g3 = game(
        "Doe, Jane",
        "Kasparov, Garry",
        GameResult::Draw,
        "1985",
        "Dallas USA",
    );
    let mut g4 = game(
        "Kasparov, Garry",
        "Anand, Viswanathan",
        GameResult::Draw,
        "1995.01.01",
        "NYC",
    );
    rate(&mut g4, 2800, 2750);
    for g in [&g1, &g2, &g3, &g4] {
        w.add_game(g, GameFlags::empty()).expect("add");
    }
    w.set_flag(2, GameFlags::TACTICS, crate::si4::FlagOp::Set)
        .expect("flag");
    drop(w);
    (dir, base)
}

fn run(query: &HeaderSearch, filter: &mut Filter) -> Vec<u32> {
    let (_dir, base) = build();
    let mut db = Si4Database::open(&base).expect("open");
    let stats = PlayerStats::from_database(&db);
    let report = search_header(
        &mut db,
        query,
        filter,
        &stats,
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .expect("search");
    assert_eq!(report.matched, filter.count());
    filter.iter_games().collect()
}

fn hits(query: &HeaderSearch) -> Vec<u32> {
    run(query, &mut Filter::new(4))
}

#[test]
fn defaults_match_every_game() {
    assert_eq!(hits(&HeaderSearch::default()), [1, 2, 3, 4]);
}

#[test]
fn name_search_and_ignore_colors() {
    let mut q = HeaderSearch {
        white: "kasparov".into(),
        ..HeaderSearch::default()
    };
    assert_eq!(hits(&q), [1, 4]);
    q.ignore_colors = true;
    assert_eq!(hits(&q), [1, 3, 4]);
    let q = HeaderSearch {
        site: "\"*USA\"".into(),
        ..HeaderSearch::default()
    };
    assert_eq!(hits(&q), [1, 3]);
}

#[test]
fn unrated_game_uses_the_players_best_elo() {
    // Game 1: Kasparov has no Elo of his own but 2800 in game 4.
    let q = HeaderSearch {
        white_elo: MinMax::new(2750, 4000),
        ..HeaderSearch::default()
    };
    assert_eq!(hits(&q), [1, 4]);
}

#[test]
fn results_dates_and_flags() {
    let mut q = HeaderSearch::default();
    q.results.draw = false;
    assert_eq!(hits(&q), [1, 2]);
    let q = HeaderSearch {
        date: MinMax::new(
            crate::complete_min_date("2000"),
            crate::complete_max_date("2000"),
        ),
        ..HeaderSearch::default()
    };
    assert_eq!(hits(&q), [2]);
    let mut q = HeaderSearch::default();
    q.flags.set(GameFlags::TACTICS, Tri::Yes);
    assert_eq!(hits(&q), [2]);
}

#[test]
fn and_or_and_game_range() {
    let mut f = Filter::new(4);
    f.set(1, 9);
    let q = HeaderSearch {
        white: "kasparov".into(),
        op: FilterOp::And,
        ..HeaderSearch::default()
    };
    assert_eq!(
        run(&q, &mut f),
        [1, 4]
            .iter()
            .copied()
            .filter(|g| f.contains(*g))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        f.get(1),
        9,
        "AND keeps the ply of a game that still matches"
    );

    let mut f = Filter::new(4);
    f.fill(0);
    f.set(2, 1);
    let q = HeaderSearch {
        white: "kasparov".into(),
        op: FilterOp::Or,
        ..HeaderSearch::default()
    };
    assert_eq!(run(&q, &mut f), [1, 2, 4]);

    let q = HeaderSearch {
        game_number: MinMax::new(-2, -1),
        ..HeaderSearch::default()
    };
    assert_eq!(hits(&q), [3, 4]);
}

#[test]
fn pgn_text_and_game_end() {
    let q = HeaderSearch {
        pgn_text: vec!["1.e4 e5".into(), "doe, jane".into()],
        ignore_case: true,
        ..HeaderSearch::default()
    };
    assert_eq!(hits(&q), [2, 3]);
    let q = HeaderSearch {
        game_end: GameEndKind::Checkmate,
        ..HeaderSearch::default()
    };
    assert_eq!(hits(&q), Vec::<u32>::new());
}

#[test]
fn eco_range_without_codes_needs_include_none() {
    let eco = |s: &str| s.parse().expect("eco");
    let mut q = HeaderSearch {
        eco: Some(EcoRange {
            min: eco("A00"),
            max: eco("E99"),
            include_none: false,
        }),
        ..HeaderSearch::default()
    };
    assert_eq!(hits(&q), Vec::<u32>::new());
    if let Some(r) = q.eco.as_mut() {
        r.include_none = true;
    }
    assert_eq!(hits(&q), [1, 2, 3, 4]);
}

#[test]
fn cancel_keeps_the_old_filter() {
    let (_dir, base) = build();
    let mut db = Si4Database::open(&base).expect("open");
    let stats = PlayerStats::from_database(&db);
    let mut filter = Filter::new(4);
    filter.set(3, 0);
    let before = filter.clone();
    let token = CancelToken::new();
    token.cancel();
    let report = search_header(
        &mut db,
        &HeaderSearch::default(),
        &mut filter,
        &stats,
        None,
        &mut NoProgress,
        &token,
    )
    .expect("search");
    assert!(report.interrupted);
    assert_eq!(filter, before);
}

#[test]
fn filter_size_mismatch_is_an_error() {
    let (_dir, base) = build();
    let mut db = Si4Database::open(&base).expect("open");
    let stats = PlayerStats::new();
    let mut filter = Filter::new(3);
    let err = search_header(
        &mut db,
        &HeaderSearch::default(),
        &mut filter,
        &stats,
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .expect_err("mismatch");
    assert_eq!(
        err,
        SearchError::FilterSize {
            filter: 3,
            games: 4
        }
    );
}

#[test]
fn textfind_and_textfilter() {
    let (_dir, base) = build();
    let db = Si4Database::open(&base).expect("open");
    let mut f = Filter::new(4);
    assert_eq!(f.textfind(&db, "dallas", true, 0), Some(1));
    assert_eq!(f.textfind(&db, "dallas", true, 1), Some(3));
    assert_eq!(f.textfind(&db, "dallas", false, 0), None);
    assert_eq!(f.textfind(&db, "1990.05", false, 0), Some(1));
    assert_eq!(f.textfilter(&db, "KASPAROV", true), 1);
    assert_eq!(f.iter_games().collect::<Vec<_>>(), [1, 3, 4]);
}
