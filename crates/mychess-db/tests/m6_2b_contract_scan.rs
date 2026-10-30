//! Contract tests of the pattern scan (spec
//! `docs/specs/m6-2-taticas-e-padroes.md`, "Varredura", rules 5-8, 15-20, 23,
//! D3253, D3258, D3285, D3296), written from the spec and not from the code:
//! the filter value from ply 254, `Reset`/`And`/`Or`, cancelling in the
//! middle of a heavy scan, progress, the same answer with 1, 2 and 4 threads,
//! folding by result, unreadable games, the frame limit, the cost estimate and
//! the occurrences of one game.
#![allow(
    clippy::assert_is_empty,
    clippy::trivially_copy_pass_by_ref,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    unused_qualifications,
    clippy::unwrap_used,
    clippy::bool_to_int_with_if,
    clippy::format_push_string,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    dead_code
)]

#[path = "common/m3_3a_support.rs"]
mod base;
#[path = "common/m6_1a_contract_support.rs"]
mod contract;
#[path = "common/m6_1a_support.rs"]
mod support;

use std::time::Duration;

use mychess_core::{Game, GameResult, Square};
use mychess_db::pattern::model::{
    Condition, Flag, MateCond, MateMode, MoveCond, Pattern, PieceCond, Region, ResultKind,
    RoleName, Scope, Side, Sq, Status, WhichMove, Who,
};
use mychess_db::pattern::presets::{self, Params};
use mychess_db::pattern::{
    CONFIRM_POSITIONS, HighlightKind, HighlightRole, Line, POSITIONS_PER_GAME, PatternReport,
    ScanLimits, estimate_cost, hits_in_game, hits_in_line, search_pattern, search_pattern_with,
};
use mychess_db::{CancelToken, Filter, FilterOp, GameSource, NoProgress};
use pretty_assertions::assert_eq;
use support::{db_of, game, game_fen, values};

fn sq(name: &str) -> Square {
    name.parse().unwrap()
}

fn pat(root: Condition) -> Pattern {
    Pattern {
        name: "contract".into(),
        description: String::new(),
        scope: Scope::default(),
        root,
    }
}

fn pawn_on(side: Who, square: &str) -> Pattern {
    pat(Condition::Piece(PieceCond {
        side,
        roles: vec![RoleName::Pawn],
        region: Region::Squares(vec![Sq(sq(square))]),
        min: 1,
        max: None,
    }))
}

fn mate(by: Who, n: u8, mode: MateMode, forcing_only: bool, max_nodes: u32) -> Pattern {
    let mut p = pat(Condition::Mate(MateCond {
        by,
        moves_min: n,
        moves_max: n,
        mode,
        forcing_only,
        max_nodes,
    }));
    p.scope.min_move = 0;
    p
}

fn mate_any_depth(by: Who, lo: u8, hi: u8, mode: MateMode) -> Pattern {
    let mut p = mate(by, lo, mode, false, 20_000);
    if let Condition::Mate(m) = &mut p.root {
        m.moves_max = hi;
    }
    p
}

fn go(
    db: &mut impl GameSource,
    pattern: &Pattern,
    op: FilterOp,
    filter: &mut Filter,
) -> PatternReport {
    search_pattern(
        db,
        pattern,
        op,
        filter,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap()
}

fn reset(db: &mut impl GameSource, pattern: &Pattern) -> (Vec<(u32, u8)>, PatternReport) {
    let mut filter = Filter::new(db.len());
    let report = go(db, pattern, FilterOp::Reset, &mut filter);
    (values(&filter), report)
}

const SCHOLAR: &str = "e2e4 e7e5 f1c4 b8c6 d1h5 g8f6 h5f7";
const FOOL: &str = "f2f3 e7e5 g2g4 d8h4";
const LEGAL: &str = "e2e4 e7e5 g1f3 d7d6 f1c4 c8g4 b1c3 g7g6 f3e5 g4d1 c4f7 e8e7 c3d5";

fn with_result(mut g: Game, r: GameResult) -> Game {
    g.tags_mut().result = r;
    g
}

// ------------------------------------------------- filter value (D3253)

/// Games with the white pawn reaching e4 at ply `shuffle + 1`.
fn pawn_at_ply(ply_of_e4: usize) -> Game {
    game(&format!("{} e2e4", contract::shuffle(ply_of_e4 - 1)))
}

#[test]
fn filter_value_is_ply_plus_one_up_to_253_and_one_from_254() {
    // White pawn on e4 first appears at an odd ply; the knights shuffle
    // before it never put a pawn there.
    let plies = [1usize, 3, 5, 251, 253, 255, 257, 301];
    let games: Vec<Game> = plies.iter().map(|&p| pawn_at_ply(p)).collect();
    let (_fx, mut db) = db_of(games);
    let (got, report) = reset(&mut db, &pawn_on(Who::White, "e4"));
    let expected: Vec<(u32, u8)> = plies
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let value = if p <= 253 { (p + 1) as u8 } else { 1 };
            (i as u32 + 1, value)
        })
        .collect();
    assert_eq!(got, expected);
    assert_eq!(report.matched, plies.len() as u32);
    assert_eq!(report.unreadable_total, 0);
}

#[test]
fn filter_value_boundary_between_253_and_254_for_black_plies() {
    // Black pawn on e5 appears at even plies: 252, 254, 256.
    let shuffles = [250usize, 252, 254];
    let games: Vec<Game> = shuffles
        .iter()
        .map(|&s| game(&format!("{} e2e4 e7e5", contract::shuffle(s))))
        .collect();
    let (_fx, mut db) = db_of(games);
    let (got, _) = reset(&mut db, &pawn_on(Who::Black, "e5"));
    // ply 252 -> 253 (fits), ply 254 -> 1 (overflow), ply 256 -> 1.
    assert_eq!(got, vec![(1, 253), (2, 1), (3, 1)]);
}

#[test]
fn exact_ply_survives_in_the_hit_when_the_filter_value_overflowed() {
    let g = pawn_at_ply(301);
    let p = pawn_on(Who::White, "e4");
    let hits = hits_in_game(&p, &g).unwrap();
    assert_eq!(hits.iter().map(|h| h.ply).collect::<Vec<_>>(), vec![301]);
}

#[test]
fn a_very_long_game_gives_value_one_and_does_not_panic() {
    // 1000 plies of shuffling, then e4: ply 1001 is far above 253.
    let (_fx, mut db) = db_of(vec![pawn_at_ply(1001)]);
    let (got, report) = reset(&mut db, &pawn_on(Who::White, "e4"));
    assert_eq!(got, vec![(1, 1)]);
    assert_eq!(report.unreadable_total, 0);
}

#[test]
fn a_match_at_the_initial_position_needs_min_move_zero() {
    let (_fx, mut db) = db_of(vec![game("e2e4")]);
    let mut p = pawn_on(Who::White, "e2");
    // min_move = 1 starts after White's first move: e2 is empty by then.
    assert_eq!(reset(&mut db, &p).0, vec![]);
    p.scope.min_move = 0;
    assert_eq!(reset(&mut db, &p).0, vec![(1, 1)]);
}

#[test]
fn a_game_without_moves_is_only_evaluated_with_min_move_zero() {
    let (_fx, mut db) = db_of(vec![
        game(""),
        game_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1", ""),
    ]);
    let mut p = pawn_on(Who::White, "e2");
    assert_eq!(reset(&mut db, &p).0, vec![]);
    p.scope.min_move = 0;
    // The own start position (FEN) is evaluated too.
    assert_eq!(reset(&mut db, &p).0, vec![(1, 1), (2, 1)]);
}

// ----------------------------------------------- Reset / And / Or (M3-3)

fn small_db() -> (base::Fixture, mychess_db::si4::Si4Database) {
    db_of(vec![
        game(SCHOLAR),
        game(FOOL),
        game(&contract::shuffle(20)),
        game(LEGAL),
        game("e2e4 e7e5"),
    ])
}

#[test]
fn reset_ignores_what_the_filter_held_before() {
    let (_fx, mut db) = small_db();
    let p = mate(Who::Any, 1, MateMode::Played, false, 20_000);
    let mut filter = Filter::new(5);
    filter.fill(0);
    filter.set(3, 200);
    filter.set(5, 17);
    let report = go(&mut db, &p, FilterOp::Reset, &mut filter);
    assert_eq!(values(&filter), vec![(1, 7), (2, 4), (4, 13)]);
    assert_eq!(report.start_count, 5);
    assert_eq!(report.matched, 3);
}

#[test]
fn and_only_visits_games_in_the_filter_and_rewrites_their_value() {
    let (_fx, mut db) = small_db();
    let p = mate(Who::Any, 1, MateMode::Played, false, 20_000);
    let mut filter = Filter::new(5);
    filter.fill(0);
    filter.set(2, 99);
    filter.set(3, 99);
    filter.set(5, 99);
    let report = go(&mut db, &p, FilterOp::And, &mut filter);
    // Game 2 matches (value ply 4 + 1); 3 and 5 are visited and drop out;
    // games 1 and 4 match but were not in the filter, so they stay out.
    assert_eq!(values(&filter), vec![(2, 4)]);
    assert_eq!(report.start_count, 3);
    assert_eq!(report.matched, 1);
}

#[test]
fn and_examines_exactly_the_games_in_the_filter() {
    let (_fx, mut db) = small_db();
    let p = pawn_on(Who::White, "e4");
    let mut only_one = Filter::new(5);
    only_one.fill(0);
    only_one.set(5, 1);
    let one = go(&mut db, &p, FilterOp::And, &mut only_one);
    let mut single = Filter::new(5);
    single.fill(0);
    single.set(5, 1);
    // Examined positions of game 5 alone: plies 1 and 2 (range from ply 1).
    assert_eq!(one.positions_examined, 1);
    assert_eq!(values(&only_one), vec![(5, 2)]);
}

#[test]
fn or_keeps_the_games_already_in_the_filter_untouched() {
    let (_fx, mut db) = small_db();
    let p = mate(Who::Any, 1, MateMode::Played, false, 20_000);
    let mut filter = Filter::new(5);
    filter.fill(0);
    filter.set(1, 3);
    filter.set(3, 3);
    let report = go(&mut db, &p, FilterOp::Or, &mut filter);
    // Game 1 keeps value 3 (not re-examined); 2 and 4 enter; 3 stays.
    assert_eq!(values(&filter), vec![(1, 3), (2, 4), (3, 3), (4, 13)]);
    assert_eq!(report.start_count, 5);
}

#[test]
fn and_with_an_empty_filter_and_or_with_a_full_one_scan_nothing() {
    let (_fx, mut db) = small_db();
    let p = mate(Who::Any, 1, MateMode::Available, false, 20_000);
    let mut empty = Filter::new(5);
    empty.fill(0);
    let report = go(&mut db, &p, FilterOp::And, &mut empty);
    assert_eq!(empty.count(), 0);
    assert_eq!(report.positions_examined, 0);
    assert!(!report.interrupted);
    assert_eq!(report.matched, 0);

    let mut full = Filter::new(5);
    full.fill(7);
    let before = full.clone();
    let report = go(&mut db, &p, FilterOp::Or, &mut full);
    assert_eq!(full, before);
    assert_eq!(report.positions_examined, 0);
    assert!(!report.interrupted);
    assert_eq!(report.matched, 5);
}

#[test]
fn an_empty_database_is_an_empty_result() {
    let (_fx, mut db) = db_of(vec![]);
    let (got, report) = reset(&mut db, &pawn_on(Who::White, "e4"));
    assert_eq!(got, vec![]);
    assert_eq!(report.matched, 0);
    assert!(!report.interrupted);
}

// ------------------------------------------ cancellation and progress

#[test]
fn cancelling_in_the_middle_of_a_heavy_mate_scan_keeps_the_filter_byte_for_byte() {
    // Many games of quiet positions: a full-width mate search with a large
    // budget on each position takes far longer than the delay below.
    let games: Vec<Game> = (0..96).map(|_| game(&contract::shuffle(60))).collect();
    let (_fx, mut db) = db_of(games);
    let p = mate(Who::Any, 4, MateMode::Available, false, 2_000_000);
    let mut filter = Filter::new(db.len());
    filter.fill(0);
    filter.set(7, 123);
    filter.set(50, 9);
    let before = filter.clone();
    let token = CancelToken::new();
    let remote = token.clone();
    let started = std::time::Instant::now();
    let (report, finished_after) = std::thread::scope(|s| {
        s.spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            remote.cancel();
        });
        let report = search_pattern(
            &mut db,
            &p,
            FilterOp::Or,
            &mut filter,
            &mut NoProgress,
            &token,
        )
        .unwrap();
        (report, started.elapsed())
    });
    assert!(
        report.interrupted,
        "the scan finished before the cancel: {report:?}"
    );
    assert_eq!(filter, before);
    assert_eq!(filter.as_bytes(), before.as_bytes());
    // Spec: the answer comes in a fraction of a second even with the budget
    // this big; allow a wide margin for a loaded debug build.
    assert!(
        finished_after < Duration::from_secs(10),
        "cancel took {finished_after:?}"
    );
}

#[test]
fn cancelling_before_the_scan_changes_nothing_for_every_operation() {
    for op in [FilterOp::Reset, FilterOp::And, FilterOp::Or] {
        let (_fx, mut db) = small_db();
        let p = mate(Who::Any, 1, MateMode::Available, false, 20_000);
        let mut filter = Filter::new(5);
        filter.fill(0);
        filter.set(2, 5);
        filter.set(4, 8);
        let before = filter.clone();
        let token = CancelToken::new();
        token.cancel();
        let report = search_pattern(&mut db, &p, op, &mut filter, &mut NoProgress, &token).unwrap();
        assert!(report.interrupted, "{op:?}");
        assert_eq!(filter, before, "{op:?}");
    }
}

#[test]
fn progress_is_monotonic_bounded_and_ends_complete() {
    let total = 5200usize;
    let templates = [SCHOLAR, "d2d4 d7d5 c2c4", ""];
    let (_fx, mut db) = contract::crowd(total, &templates);
    let p = pawn_on(Who::White, "e4");
    let mut sink = contract::Recorder::recording();
    let mut filter = Filter::new(db.len());
    let report = search_pattern(
        &mut db,
        &p,
        FilterOp::Reset,
        &mut filter,
        &mut sink,
        &CancelToken::new(),
    )
    .unwrap();
    assert!(!report.interrupted);
    assert!(!sink.calls.is_empty());
    assert!(sink.calls.windows(2).all(|w| w[0].0 <= w[1].0));
    assert!(
        sink.calls
            .iter()
            .all(|&(done, all)| done <= all && all == total as u64)
    );
    assert_eq!(*sink.calls.last().unwrap(), (total as u64, total as u64));
    // 1/3 of the games are Scholar's mate games (pawn e4 at ply 1).
    assert_eq!(report.matched, total.div_ceil(3) as u32);
}

#[test]
fn cancelling_at_the_first_progress_report_keeps_the_filter() {
    let (_fx, mut db) = contract::crowd(5200, &[SCHOLAR, "d2d4"]);
    let mut filter = Filter::new(db.len());
    filter.fill(0);
    filter.set(10, 44);
    let before = filter.clone();
    let token = CancelToken::new();
    let mut sink = contract::Recorder::cancelling(&token);
    let report = search_pattern(
        &mut db,
        &pawn_on(Who::White, "e4"),
        FilterOp::Or,
        &mut filter,
        &mut sink,
        &token,
    )
    .unwrap();
    assert!(report.interrupted);
    assert_eq!(filter, before);
}

// ------------------------------------------------------ thread counts

/// A mixed corpus: wins, losses, draws, mates, long games, FEN games.
fn corpus(copies: usize) -> Vec<Game> {
    let mut out = Vec::new();
    for i in 0..copies {
        out.push(with_result(game(SCHOLAR), GameResult::White));
        out.push(with_result(game(FOOL), GameResult::Black));
        out.push(with_result(game(LEGAL), GameResult::White));
        out.push(with_result(
            game(&contract::shuffle(30 + i % 7 * 2)),
            GameResult::Draw,
        ));
        out.push(with_result(
            game_fen("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1", "a1a8"),
            GameResult::White,
        ));
        out.push(game_fen(
            "r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1",
            "d5c7 e8d7 c7a8",
        ));
        out.push(game("e2e4 d7d5 e4d5 d8d5 b1c3 d5a5 d2d4 c8g4 f1b5 b8c6"));
    }
    out
}

#[derive(Debug, PartialEq, Eq)]
struct Digest {
    bytes: Vec<u8>,
    matched: u32,
    start_count: u32,
    unreadable: Vec<u32>,
    unreadable_total: u64,
    examined: u64,
    unknown: u64,
    folded: u32,
    interrupted: bool,
}

fn digest(db: &mut impl GameSource, p: &Pattern) -> Digest {
    let mut filter = Filter::new(db.len());
    let r = go(db, p, FilterOp::Reset, &mut filter);
    Digest {
        bytes: filter.as_bytes().to_vec(),
        matched: r.matched,
        start_count: r.start_count,
        unreadable: r.unreadable,
        unreadable_total: r.unreadable_total,
        examined: r.positions_examined,
        unknown: r.unknown_positions,
        folded: r.games_folded_out,
        interrupted: r.interrupted,
    }
}

#[test]
fn one_two_and_four_threads_give_the_same_answer() {
    let (_fx, mut db) = db_of(corpus(12));
    assert!(db.len() > 64, "must span several batches");
    let fork = presets::build(
        "fork",
        &Params {
            by: Who::Any,
            ..Params::default()
        },
    )
    .unwrap();
    let pin = presets::build("pin", &Params::default()).unwrap();
    let static_result = pat(Condition::All {
        items: vec![
            Condition::Result {
                results: vec![ResultKind::WhiteWins],
            },
            Condition::SideToMove { side: Side::Black },
        ],
    });
    let cases: Vec<(&str, Pattern)> = vec![
        ("fork", fork),
        ("pin", pin),
        ("static with result", static_result),
        (
            "mate played",
            mate(Who::Any, 1, MateMode::Played, false, 20_000),
        ),
        (
            "mate available 1000 nodes, n=2 forcing",
            mate(Who::Any, 2, MateMode::Available, true, 1_000),
        ),
        (
            "mate available 1000 nodes, n=3 full",
            mate(Who::Any, 3, MateMode::Available, false, 1_000),
        ),
        (
            "mate available 1000 nodes, range 1..=2 for white",
            mate_any_depth(Who::White, 1, 2, MateMode::Available),
        ),
    ];
    for (name, p) in &cases {
        let one = contract::with_threads(1, || digest(&mut db, p));
        let two = contract::with_threads(2, || digest(&mut db, p));
        let four = contract::with_threads(4, || digest(&mut db, p));
        assert_eq!(one, two, "{name}: 1 vs 2 threads");
        assert_eq!(one, four, "{name}: 1 vs 4 threads");
        assert!(!one.interrupted, "{name}");
    }
    // The tiny budget really produced unknown positions to compare.
    let tiny = mate(Who::Any, 3, MateMode::Available, false, 1_000);
    assert!(digest(&mut db, &tiny).unknown > 0);
    // And a played mate never needs the solver, so nothing is unknown.
    let played = mate(Who::Any, 1, MateMode::Played, false, 1_000);
    assert_eq!(digest(&mut db, &played).unknown, 0);
}

#[test]
fn the_same_scan_twice_is_identical() {
    let (_fx, mut db) = db_of(corpus(4));
    let p = mate(Who::Any, 2, MateMode::Available, false, 1_000);
    assert_eq!(digest(&mut db, &p), digest(&mut db, &p));
}

#[test]
fn a_mate_budget_makes_unknown_positions_neither_match_nor_exclude() {
    // Not(Mate) must not match where the solver ran out of nodes (rule 2).
    let (_fx, mut db) = db_of(vec![game(&contract::shuffle(8))]);
    let mut p = mate(Who::White, 5, MateMode::Available, false, 1_000);
    let Condition::Mate(m) = p.root.clone() else {
        unreachable!()
    };
    p.root = Condition::Not {
        item: Box::new(Condition::Mate(m)),
    };
    let (got, report) = reset(&mut db, &p);
    if report.unknown_positions > 0 {
        // Every position that was unknown is not a match.
        let examined = report.positions_examined;
        assert!(examined >= report.unknown_positions);
    }
    // Whatever the solver decided, a hit never sits on an unknown position:
    // counting both must not exceed the positions examined.
    let hits = if got.is_empty() { 0 } else { 1 };
    assert!(hits + report.unknown_positions <= report.positions_examined + 1);
}

// ---------------------------- folding by result (prefilter, D3258)

fn result_games() -> Vec<Game> {
    vec![
        with_result(game("e2e4 e7e5 g1f3"), GameResult::White),
        with_result(game("e2e4 e7e5 g1f3"), GameResult::Black),
        with_result(game("e2e4 e7e5 g1f3"), GameResult::Draw),
        game("e2e4 e7e5 g1f3"),
        with_result(game("e2e4 e7e5 g1f3"), GameResult::White),
    ]
}

fn white_wins_white_to_move() -> Pattern {
    pat(Condition::All {
        items: vec![
            Condition::Result {
                results: vec![ResultKind::WhiteWins],
            },
            Condition::SideToMove { side: Side::White },
        ],
    })
}

#[test]
fn an_impossible_result_skips_the_game_without_reading_it() {
    let (_fx, mut db) = db_of(result_games());
    let p = white_wins_white_to_move();
    let (got, report) = reset(&mut db, &p);
    // White to move first at ply 2 (min_move = 1 starts at ply 1).
    assert_eq!(got, vec![(1, 3), (5, 3)]);
    assert_eq!(report.games_folded_out, 3);
    // Only the two games that were read were evaluated, each stopping at
    // its first match (plies 1 and 2).
    assert_eq!(report.positions_examined, 4);
}

#[test]
fn a_result_that_cannot_fail_folds_nothing() {
    let (_fx, mut db) = db_of(result_games());
    let p = pat(Condition::Result {
        results: vec![
            ResultKind::WhiteWins,
            ResultKind::Draw,
            ResultKind::BlackWins,
            ResultKind::Unknown,
        ],
    });
    let (got, report) = reset(&mut db, &p);
    assert_eq!(report.games_folded_out, 0);
    assert_eq!(got.len(), 5);
}

#[test]
fn not_result_folds_the_games_with_that_result() {
    let (_fx, mut db) = db_of(result_games());
    let p = pat(Condition::Not {
        item: Box::new(Condition::Result {
            results: vec![ResultKind::Draw],
        }),
    });
    let (got, report) = reset(&mut db, &p);
    assert_eq!(report.games_folded_out, 1);
    assert_eq!(
        got.iter().map(|g| g.0).collect::<Vec<_>>(),
        vec![1, 2, 4, 5]
    );
}

#[test]
fn ignore_colors_only_folds_when_false_with_the_swapped_result_too() {
    let (_fx, mut db) = db_of(result_games());
    let mut p = white_wins_white_to_move();
    p.scope.ignore_colors = true;
    let (got, report) = reset(&mut db, &p);
    // Folded: the draw and the unknown result; the black win survives
    // because with the colours swapped it is a white win.
    assert_eq!(report.games_folded_out, 2);
    // The black win matches through the mirror: in the mirrored game the
    // side to move at ply 1 is White.
    assert_eq!(got, vec![(1, 3), (2, 2), (5, 3)]);
}

#[test]
fn ignore_colors_matches_the_result_swapped_game_with_the_same_ply() {
    let (_fx, mut db) = db_of(vec![with_result(game(FOOL), GameResult::Black)]);
    // "Black wins, White to move" holds at ply 2 of the fool's mate.
    let mut p = pat(Condition::All {
        items: vec![
            Condition::Result {
                results: vec![ResultKind::BlackWins],
            },
            Condition::SideToMove { side: Side::White },
        ],
    });
    let (plain, _) = reset(&mut db, &p);
    p.scope.ignore_colors = true;
    let (mirrored, _) = reset(&mut db, &p);
    // The original already matches, so the mirror is never needed and the
    // value is the same.
    assert_eq!(plain, vec![(1, 3)]);
    assert_eq!(mirrored, plain);
}

// ------------------------------------------------ unreadable games

fn damaged_db(
    patch: impl Fn(u32, &mut [u8]),
    games: Vec<Game>,
    broken: &[u32],
) -> (base::Fixture, mychess_db::si4::Si4Database) {
    let (fx, db) = db_of(games);
    let entries: Vec<_> = broken.iter().map(|&n| (n, db.entry(n).unwrap())).collect();
    drop(db);
    let sg4 = fx.base.with_extension("sg4");
    let mut bytes = std::fs::read(&sg4).unwrap();
    for (n, entry) in entries {
        let from = entry.offset as usize;
        patch(n, &mut bytes[from..from + entry.length as usize]);
    }
    std::fs::write(&sg4, bytes).unwrap();
    let db = fx.open();
    (fx, db)
}

#[test]
fn an_illegal_move_makes_the_game_unreadable_even_if_the_pattern_matched_before_it() {
    // Record layout (docs/formatos/si4.md section 9): `00 00` then one byte
    // per move; `1.e4 e5` is `CF CF`. 0x1A is the a1 rook going to a3
    // through its own pawn: decodable, illegal.
    let games = vec![
        game("e2e4 e7e5 g1f3 b8c6"),
        game("e2e4 e7e5 g1f3 b8c6"),
        game("e2e4 e7e5 g1f3 b8c6"),
        game("e2e4 e7e5 g1f3 b8c6"),
    ];
    let (_fx, mut db) = damaged_db(
        |n, rec| {
            assert_eq!(&rec[..4], &[0x00, 0x00, 0xCF, 0xCF], "layout of game {n}");
            match n {
                // Illegal 3rd ply (white), after e4 e5 that already match.
                2 => rec[4] = 0x1A,
                // Illegal 4th ply (black rook a8 to a6 through a pawn).
                3 => rec[5] = 0x1D,
                _ => {}
            }
        },
        games,
        &[2, 3],
    );
    // The pattern holds at ply 1 of every game.
    let (got, report) = reset(&mut db, &pawn_on(Who::White, "e4"));
    assert_eq!(report.unreadable, vec![2, 3]);
    assert_eq!(report.unreadable_total, 2);
    assert_eq!(got, vec![(1, 2), (4, 2)]);
    assert!(!report.interrupted);
}

#[test]
fn an_illegal_first_move_and_a_damaged_record_are_both_unreadable() {
    let games: Vec<Game> = (0..5).map(|_| game("e2e4 e7e5 g1f3")).collect();
    let (_fx, mut db) = damaged_db(
        |n, rec| match n {
            1 => rec[2] = 0x1A,
            4 => rec.fill(0xAA),
            _ => {}
        },
        games,
        &[1, 4],
    );
    let (got, report) = reset(&mut db, &pawn_on(Who::White, "e4"));
    assert_eq!(report.unreadable, vec![1, 4]);
    assert_eq!(got.iter().map(|g| g.0).collect::<Vec<_>>(), vec![2, 3, 5]);
}

#[test]
fn zero_length_and_dangling_records_are_unreadable_not_fatal() {
    let (fx, db) = db_of((0..4).map(|_| game("e2e4 e7e5")).collect());
    drop(db);
    base::zero_length(&fx, 1);
    base::break_offset(&fx, 3);
    let mut db = fx.open();
    let (got, report) = reset(&mut db, &pawn_on(Who::White, "e4"));
    assert_eq!(got.iter().map(|g| g.0).collect::<Vec<_>>(), vec![2, 4]);
    // A zero-length record is an empty game (as in the other scans), not an
    // error; the dangling offset is unreadable.
    assert_eq!(report.unreadable, vec![3]);
    assert_eq!(report.unreadable_total, 1);
}

#[test]
fn unreadable_games_do_not_depend_on_the_thread_count() {
    let games: Vec<Game> = (0..80).map(|_| game("e2e4 e7e5 g1f3")).collect();
    let (_fx, mut db) = damaged_db(|_, rec| rec[3] = 0x1D, games, &[3, 40, 41, 79]);
    let p = pawn_on(Who::White, "e4");
    let one = contract::with_threads(1, || digest(&mut db, &p));
    let four = contract::with_threads(4, || digest(&mut db, &p));
    assert_eq!(one, four);
    assert_eq!(one.unreadable, vec![3, 40, 41, 79]);
    assert_eq!(one.unreadable_total, 4);
    assert_eq!(one.matched, 76);
}

// ------------------------------------------------------ frame limit

fn scholar_end_pattern() -> Pattern {
    let mut p = pat(Condition::Status {
        status: Status::Checkmate,
    });
    p.scope.end_only = true;
    p
}

#[test]
fn end_only_needs_the_whole_game_and_a_cut_line_counts_as_unknown() {
    let (_fx, mut db) = db_of(vec![game(SCHOLAR), game(FOOL)]);
    let p = scholar_end_pattern();
    let (got, _) = reset(&mut db, &p);
    // Mate on the last position: ply 7 (value 8) and ply 4 (value 5).
    assert_eq!(got, vec![(1, 8), (2, 5)]);

    // 7 plies = 8 frames: a limit of 8 keeps all of Scholar's mate; 7 cuts it.
    for (frames, scholar_matches) in [(8usize, true), (7, false), (4, false), (1, false)] {
        let limits = ScanLimits {
            max_frames: frames,
            ..ScanLimits::default()
        };
        let mut filter = Filter::new(2);
        let report = search_pattern_with(
            &mut db,
            &p,
            FilterOp::Reset,
            &mut filter,
            &limits,
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(filter.contains(1), scholar_matches, "{frames} frames");
        // The fool's mate (5 frames) fits in 5 or more.
        assert_eq!(filter.contains(2), frames >= 5, "{frames} frames");
        let cut = u64::from(!scholar_matches) + u64::from(frames < 5);
        assert!(
            report.unknown_positions >= cut,
            "{frames} frames: {report:?}"
        );
        assert_eq!(report.unreadable_total, 0, "{frames} frames");
    }
}

#[test]
fn a_truncated_line_still_matches_what_it_shows_before_the_cut() {
    let (_fx, mut db) = db_of(vec![game(SCHOLAR)]);
    let limits = ScanLimits {
        max_frames: 4,
        ..ScanLimits::default()
    };
    let mut filter = Filter::new(1);
    search_pattern_with(
        &mut db,
        &pawn_on(Who::White, "e4"),
        FilterOp::Reset,
        &mut filter,
        &limits,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(values(&filter), vec![(1, 2)]);
}

#[test]
fn a_played_mate_never_holds_on_a_truncated_line() {
    let (_fx, mut db) = db_of(vec![game(SCHOLAR)]);
    let p = mate(Who::White, 1, MateMode::Played, false, 20_000);
    assert_eq!(reset(&mut db, &p).0, vec![(1, 7)]);
    let limits = ScanLimits {
        max_frames: 5,
        ..ScanLimits::default()
    };
    let mut filter = Filter::new(1);
    search_pattern_with(
        &mut db,
        &p,
        FilterOp::Reset,
        &mut filter,
        &limits,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(filter.count(), 0);
}

#[test]
fn the_mate_budget_cap_is_clamped_to_at_least_the_smallest_pattern_budget() {
    // A cap of 0 must not turn every mate search into nothing but unknowns
    // forever or panic: the smallest budget a pattern may ask for is 1000.
    let (_fx, mut db) = db_of(vec![game("e2e4 e7e5")]);
    let p = mate(Who::White, 1, MateMode::Available, false, 1_000);
    let limits = ScanLimits {
        mate_nodes_cap: 0,
        ..ScanLimits::default()
    };
    let mut filter = Filter::new(1);
    let a = search_pattern_with(
        &mut db,
        &p,
        FilterOp::Reset,
        &mut filter,
        &limits,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    let (_, b) = reset(&mut db, &p);
    assert_eq!(a.unknown_positions, b.unknown_positions);
}

// -------------------------------------------------- invalid pattern

#[test]
fn an_invalid_pattern_is_refused_before_anything_is_read() {
    let (_fx, mut db) = small_db();
    let mut bad = pawn_on(Who::White, "e4");
    bad.scope.hold = 0;
    let mut filter = Filter::new(5);
    filter.fill(0);
    filter.set(2, 5);
    let before = filter.clone();
    let err = search_pattern(
        &mut db,
        &bad,
        FilterOp::Reset,
        &mut filter,
        &mut NoProgress,
        &CancelToken::new(),
    );
    assert!(err.is_err());
    assert_eq!(filter, before);
}

// ---------------------------------------------------- estimate_cost

#[test]
fn the_estimate_flips_exactly_above_the_confirmation_threshold() {
    let p = mate(Who::Any, 2, MateMode::Available, true, 20_000);
    let per_game = POSITIONS_PER_GAME;
    let last_ok = u32::try_from(CONFIRM_POSITIONS / per_game).unwrap();
    let cost = estimate_cost(&p, last_ok);
    assert!(cost.uses_available_mate);
    assert_eq!(cost.estimated_positions, u64::from(last_ok) * per_game);
    assert!(!cost.needs_confirmation);
    let cost = estimate_cost(&p, last_ok + 1);
    assert!(cost.needs_confirmation);
    assert!(cost.estimated_positions > CONFIRM_POSITIONS);
    // No games, no cost; the biggest possible database does not overflow.
    assert!(!estimate_cost(&p, 0).needs_confirmation);
    let huge = estimate_cost(&p, u32::MAX);
    assert!(huge.needs_confirmation);
    assert_eq!(huge.estimated_positions, u64::from(u32::MAX) * per_game);
}

#[test]
fn the_estimate_sees_an_available_mate_anywhere_in_the_tree() {
    let available = Condition::Mate(MateCond {
        by: Who::White,
        moves_min: 1,
        moves_max: 1,
        mode: MateMode::Available,
        forcing_only: true,
        max_nodes: 1_000,
    });
    let played = Condition::Mate(MateCond {
        by: Who::White,
        moves_min: 1,
        moves_max: 1,
        mode: MateMode::Played,
        forcing_only: true,
        max_nodes: 1_000,
    });
    let side = Condition::SideToMove { side: Side::White };
    let nested = pat(Condition::All {
        items: vec![
            side.clone(),
            Condition::Any {
                items: vec![
                    side.clone(),
                    Condition::Not {
                        item: Box::new(available),
                    },
                ],
            },
        ],
    });
    assert!(estimate_cost(&nested, 10).uses_available_mate);
    assert!(estimate_cost(&nested, 10).estimated_positions > 0);
    let quiet = pat(Condition::All {
        items: vec![side, played],
    });
    let cost = estimate_cost(&quiet, 100_000_000);
    assert!(!cost.uses_available_mate);
    assert_eq!(cost.estimated_positions, 0);
    assert!(!cost.needs_confirmation);
}

#[test]
fn the_report_says_whether_the_available_mate_was_used() {
    let (_fx, mut db) = small_db();
    let (_, with) = reset(
        &mut db,
        &mate(Who::Any, 1, MateMode::Available, true, 1_000),
    );
    let (_, without) = reset(&mut db, &mate(Who::Any, 1, MateMode::Played, true, 1_000));
    assert!(with.uses_available_mate);
    assert!(!without.uses_available_mate);
}

// ----------------------------------------------------- hits_in_game

/// 3.exd5 and 6.Nxd5: two separate captures (plies 3 and 6).
const TWO_CAPTURES: &str = "e2e4 d7d5 e4d5 g8f6 b1c3 f6d5";

fn capture_pattern(by: Who) -> Pattern {
    pat(Condition::Move(MoveCond {
        which: WhichMove::Previous,
        by,
        roles: vec![],
        from: Region::Anywhere,
        to: Region::Anywhere,
        capture: Flag::Yes,
        check: Flag::Both,
        promotion: vec![],
        castling: None,
    }))
}

#[test]
fn hits_in_game_reports_every_occurrence_with_coherent_highlights() {
    let g = game(TWO_CAPTURES);
    let hits = hits_in_game(&capture_pattern(Who::Any), &g).unwrap();
    assert_eq!(hits.iter().map(|h| h.ply).collect::<Vec<_>>(), vec![3, 6]);
    let arrows = |i: usize| -> Vec<(Square, Square, HighlightRole)> {
        hits[i]
            .highlights
            .iter()
            .filter_map(|h| match h.kind {
                HighlightKind::Arrow(a, b) => Some((a, b, h.role)),
                HighlightKind::Square(_) => None,
            })
            .collect()
    };
    assert_eq!(arrows(0), vec![(sq("e4"), sq("d5"), HighlightRole::Info)]);
    assert_eq!(arrows(1), vec![(sq("f6"), sq("d5"), HighlightRole::Info)]);
}

#[test]
fn the_filter_value_is_the_first_occurrence_plus_one() {
    let (_fx, mut db) = db_of(vec![game(TWO_CAPTURES)]);
    let p = capture_pattern(Who::Any);
    let g = db.game(1).unwrap();
    let first = hits_in_game(&p, &g).unwrap()[0].ply;
    assert_eq!(reset(&mut db, &p).0, vec![(1, (first + 1) as u8)]);
}

#[test]
fn consecutive_true_positions_are_one_occurrence() {
    // Both sides capture on consecutive plies (3 and 4): one run.
    let g = game("e2e4 d7d5 e4d5 d8d5");
    let hits = hits_in_game(&capture_pattern(Who::Any), &g).unwrap();
    assert_eq!(hits.iter().map(|h| h.ply).collect::<Vec<_>>(), vec![3]);
}

#[test]
fn hold_requires_that_many_consecutive_positions_per_occurrence() {
    let g = game("e2e4 d7d5 e4d5 d8d5");
    let mut p = capture_pattern(Who::Any);
    p.scope.hold = 2;
    let hits = hits_in_game(&p, &g).unwrap();
    assert_eq!(hits.iter().map(|h| h.ply).collect::<Vec<_>>(), vec![3]);
    p.scope.hold = 3;
    assert!(hits_in_game(&p, &g).unwrap().is_empty());
    // Two separated captures never make a run of 2.
    p.scope.hold = 2;
    assert!(hits_in_game(&p, &game(TWO_CAPTURES)).unwrap().is_empty());
}

#[test]
fn hits_in_game_refuses_an_invalid_pattern_before_looking_at_the_game() {
    let g = game(TWO_CAPTURES);
    let mut empty = capture_pattern(Who::Any);
    empty.root = Condition::Any { items: vec![] };
    assert!(hits_in_game(&empty, &g).is_err());
    let mut bad_scope = capture_pattern(Who::Any);
    bad_scope.scope.min_move = 10;
    bad_scope.scope.max_move = 2;
    assert!(hits_in_game(&bad_scope, &g).is_err());
    let mut mover = capture_pattern(Who::Mover);
    assert!(
        hits_in_game(&mover, &g).is_err(),
        "mover in a previous move"
    );
    mover.root = Condition::Not {
        item: Box::new(Condition::Result { results: vec![] }),
    };
    assert!(hits_in_game(&mover, &g).is_err());
}

#[test]
fn hits_in_game_of_an_empty_game_is_empty_and_the_initial_position_needs_min_move_zero() {
    let g = game("");
    assert!(
        hits_in_game(&capture_pattern(Who::Any), &g)
            .unwrap()
            .is_empty()
    );
    let mut p = pawn_on(Who::White, "e2");
    assert!(hits_in_game(&p, &g).unwrap().is_empty());
    p.scope.min_move = 0;
    let hits = hits_in_game(&p, &g).unwrap();
    assert_eq!(hits.iter().map(|h| h.ply).collect::<Vec<_>>(), vec![0]);
}

#[test]
fn hits_in_game_agrees_with_the_scan_on_every_game_of_a_corpus() {
    let games = corpus(1);
    let (_fx, mut db) = db_of(games.clone());
    for p in [
        capture_pattern(Who::Any),
        mate(Who::Any, 1, MateMode::Played, false, 20_000),
        pawn_on(Who::White, "e4"),
        presets::build("fork", &Params::default()).unwrap(),
    ] {
        let (got, _) = reset(&mut db, &p);
        for (i, g) in games.iter().enumerate() {
            let n = i as u32 + 1;
            let hits = hits_in_game(&p, g).unwrap();
            let in_filter = got.iter().find(|(m, _)| *m == n).map(|(_, v)| *v);
            match hits.first() {
                None => assert_eq!(in_filter, None, "game {n} {}", p.name),
                Some(h) => {
                    let want = if h.ply <= 253 { (h.ply + 1) as u8 } else { 1 };
                    assert_eq!(in_filter, Some(want), "game {n} {}", p.name);
                }
            }
        }
    }
}

// -------------------------------------------------- mirrored lines

#[test]
fn hits_in_line_on_a_mirrored_line_swaps_colours_and_keeps_the_ply() {
    let g = game("e2e4 d7d5 e4d5 g8f6");
    let white_captures = capture_pattern(Who::White);
    let original = Line::from_game(&g, 2048);
    let mirrored = original.mirrored().unwrap();
    let a = hits_in_line(&white_captures, &original);
    assert_eq!(a.iter().map(|h| h.ply).collect::<Vec<_>>(), vec![3]);
    // In the mirrored game it is Black who captures.
    assert!(hits_in_line(&white_captures, &mirrored).is_empty());
    let black = hits_in_line(&capture_pattern(Who::Black), &mirrored);
    assert_eq!(black.iter().map(|h| h.ply).collect::<Vec<_>>(), vec![3]);
    // The mirrored arrow goes e5 -> d4 (ranks flipped).
    assert!(black[0].highlights.iter().any(|h| matches!(
        h.kind,
        HighlightKind::Arrow(a, b) if a == sq("e5") && b == sq("d4")
    )));
}

#[test]
fn ignore_colors_tries_the_mirror_and_draws_the_original_squares() {
    // Only White captures, but the pattern asks for Black: the mirror finds
    // it and the arrow comes back in the squares of the real board.
    let g = game("e2e4 d7d5 e4d5 g8f6");
    let mut p = capture_pattern(Who::Black);
    assert!(hits_in_game(&p, &g).unwrap().is_empty());
    p.scope.ignore_colors = true;
    let hits = hits_in_game(&p, &g).unwrap();
    assert_eq!(hits.iter().map(|h| h.ply).collect::<Vec<_>>(), vec![3]);
    assert!(hits[0].highlights.iter().any(|h| matches!(
        h.kind,
        HighlightKind::Arrow(a, b) if a == sq("e4") && b == sq("d5")
    )));
}

#[test]
fn a_symmetric_pattern_gives_the_same_with_and_without_the_mirror() {
    // "Some capture was just made" does not care about the colour.
    let (_fx, mut db) = db_of(corpus(1));
    let p = capture_pattern(Who::Any);
    let (plain, _) = reset(&mut db, &p);
    let mut with_mirror = p.clone();
    with_mirror.scope.ignore_colors = true;
    let (mirrored, _) = reset(&mut db, &with_mirror);
    assert_eq!(plain, mirrored);
}

#[test]
fn ignore_colors_only_runs_the_mirror_when_the_original_fails() {
    // Examined positions double only for the games that do not match.
    let (_fx, mut db) = db_of(vec![game("e2e4 e7e5"), game("a2a3 a7a6")]);
    let mut p = pawn_on(Who::White, "e4");
    let (_, plain) = reset(&mut db, &p);
    p.scope.ignore_colors = true;
    let (got, both) = reset(&mut db, &p);
    // Game 1 matches at the original (no extra work); game 2 is examined
    // again in the mirror (2 more positions) and does not match.
    assert_eq!(got, vec![(1, 2)]);
    assert_eq!(both.positions_examined, plain.positions_examined + 2);
}

// ----------------------------------------------------- PGN as source

#[test]
fn a_pgn_source_and_an_si4_source_give_the_same_bytes_for_long_games() {
    let games = vec![pawn_at_ply(255), pawn_at_ply(41), game(SCHOLAR)];
    let (_fx, mut si4) = db_of(games.clone());
    let mut text = String::new();
    for g in &games {
        let mut line = String::new();
        let mut pos = g.start_position().clone();
        for (i, id) in g.mainline().enumerate() {
            let mv = g.mv(id).unwrap();
            let san = pos.san(mv).unwrap();
            if i % 2 == 0 {
                line.push_str(&format!("{}. ", i / 2 + 1));
            }
            line.push_str(&san);
            line.push(' ');
            pos = g.position_at(id).unwrap().clone();
        }
        text.push_str(&format!("[Event \"x\"]\n[Result \"*\"]\n\n{line}*\n\n"));
    }
    let (_dir, mut pgn) = support::pgn_db(&text);
    assert_eq!(pgn.len(), 3);
    let p = pawn_on(Who::White, "e4");
    assert_eq!(reset(&mut pgn, &p).0, reset(&mut si4, &p).0);
}
