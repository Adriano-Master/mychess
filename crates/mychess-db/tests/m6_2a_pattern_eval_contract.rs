//! M6-2a contract tests for the pattern evaluator (`eval`): three-state
//! logic (D3257), scope rules 5-9, constant folding (D3258), mirror
//! (D3259), highlights (D3275), the frame limit and the per-condition rules
//! of the spec ("Regras e casos de borda" 3, 4, 5, 10, 13, 14, 15, 25).
//! Every position and game below is written by us.

#![allow(
    clippy::too_many_lines,
    clippy::needless_pass_by_value,
    clippy::many_single_char_names
)]

use std::thread;
use std::time::Duration;

use mychess_core::{Color, Game, GameResult, Position, SanMode, Square, tactics};
use mychess_db::CancelToken;
use mychess_db::pattern::*;
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;

// ------------------------------------------------------------- helpers

fn touch<T>(_: T) {}

fn sq(s: &str) -> Sq {
    Sq(s.parse().expect("square"))
}

fn square(s: &str) -> Square {
    s.parse().expect("square")
}

fn game(fen: Option<&str>, sans: &str, result: GameResult) -> Game {
    let start = fen.map_or_else(Position::new, |f| {
        Position::from_fen(f).unwrap_or_else(|e| panic!("fen {f}: {e}"))
    });
    let mut g = Game::from_position(start);
    let mut node = g.root();
    for san in sans.split_whitespace() {
        let pos = g.position_at(node).expect("node").clone();
        let mv = pos
            .parse_san(san, SanMode::Tolerant)
            .unwrap_or_else(|e| panic!("san {san} at {}: {e}", pos.fen()))
            .mv;
        node = g.add_move(node, mv).expect("legal");
    }
    g.tags_mut().result = result;
    g
}

fn line(fen: Option<&str>, sans: &str, result: GameResult) -> Line {
    Line::from_game(&game(fen, sans, result), MAX_FRAMES)
}

fn fen_line(fen: &str) -> Line {
    line(Some(fen), "", GameResult::Unknown)
}

fn ev(c: &Condition, l: &Line, i: usize) -> Tri {
    eval_position(c, l, i, None)
}

/// The condition at the position of `fen` (a one-frame line).
fn at(c: &Condition, fen: &str) -> Tri {
    ev(c, &fen_line(fen), 0)
}

fn root(c: Condition) -> Pattern {
    Pattern {
        name: "t".into(),
        description: String::new(),
        scope: Scope::default(),
        root: c,
    }
}

fn first(p: &Pattern, l: &Line) -> Option<u16> {
    match_line(p, l, None).ply
}

fn piece(side: Who, roles: &[RoleName], region: Region, min: u8, max: Option<u8>) -> Condition {
    Condition::Piece(PieceCond {
        side,
        roles: roles.to_vec(),
        region,
        min,
        max,
    })
}

fn squares(list: &[&str]) -> Region {
    Region::Squares(list.iter().map(|s| sq(s)).collect())
}

fn mc(which: WhichMove) -> MoveCond {
    MoveCond {
        which,
        by: Who::Any,
        roles: vec![],
        from: Region::Anywhere,
        to: Region::Anywhere,
        capture: Flag::Both,
        check: Flag::Both,
        promotion: vec![],
        castling: None,
    }
}

fn rel(kind: RelKind, side: Who, actor: &[RoleName], target: &[RoleName]) -> RelationCond {
    RelationCond {
        kind,
        actor: PieceSel {
            side,
            roles: actor.to_vec(),
            region: Region::Anywhere,
        },
        target: TargetSel {
            roles: target.to_vec(),
            region: Region::Anywhere,
        },
        behind: None,
        gain: Gain::Any,
        min_targets: 2,
        when: When::Any,
        new_this_move: false,
    }
}

fn mate(by: Who, min: u8, max: u8, mode: MateMode, forcing_only: bool, nodes: u32) -> Condition {
    Condition::Mate(MateCond {
        by,
        moves_min: min,
        moves_max: max,
        mode,
        forcing_only,
        max_nodes: nodes,
    })
}

use RoleName::{Bishop, King, Knight, Pawn, Queen, Rook};

/// A condition that is `True` at the initial position of any line.
fn t() -> Condition {
    Condition::Phase {
        phase: Phase::Opening,
    }
}

/// A condition that is `False` at the initial position.
fn f() -> Condition {
    Condition::Phase {
        phase: Phase::Endgame,
    }
}

/// `Unknown` at the initial position: a mate in 4 cannot be settled with
/// 1000 nodes.
fn u() -> Condition {
    mate(Who::Mover, 4, 4, MateMode::Available, false, 1_000)
}

fn all_of(items: Vec<Condition>) -> Condition {
    Condition::All { items }
}

fn any_of(items: Vec<Condition>) -> Condition {
    Condition::Any { items }
}

fn not(c: Condition) -> Condition {
    Condition::Not { item: Box::new(c) }
}

// ------------------------------------------------------ three-state logic

#[test]
fn tri_helpers() {
    assert_eq!(Tri::from_bool(true), Tri::True);
    assert_eq!(Tri::from_bool(false), Tri::False);
    assert_eq!(Tri::True.not(), Tri::False);
    assert_eq!(Tri::False.not(), Tri::True);
    assert_eq!(Tri::Unknown.not(), Tri::Unknown);
    assert!(Tri::True.is_true());
    assert!(!Tri::False.is_true());
    assert!(!Tri::Unknown.is_true());
}

#[test]
fn the_three_probe_conditions_really_are_true_false_unknown() {
    let l = Line::from_game(&Game::new(), MAX_FRAMES);
    assert_eq!(ev(&t(), &l, 0), Tri::True);
    assert_eq!(ev(&f(), &l, 0), Tri::False);
    assert_eq!(ev(&u(), &l, 0), Tri::Unknown);
}

fn kleene_and(a: Tri, b: Tri) -> Tri {
    match (a, b) {
        (Tri::False, _) | (_, Tri::False) => Tri::False,
        (Tri::Unknown, _) | (_, Tri::Unknown) => Tri::Unknown,
        _ => Tri::True,
    }
}

fn kleene_or(a: Tri, b: Tri) -> Tri {
    match (a, b) {
        (Tri::True, _) | (_, Tri::True) => Tri::True,
        (Tri::Unknown, _) | (_, Tri::Unknown) => Tri::Unknown,
        _ => Tri::False,
    }
}

fn probe(v: Tri) -> Condition {
    match v {
        Tri::True => t(),
        Tri::False => f(),
        Tri::Unknown => u(),
    }
}

const ALL3: [Tri; 3] = [Tri::True, Tri::False, Tri::Unknown];

#[test]
fn all_any_not_follow_kleene_for_every_combination_and_order() {
    let l = Line::from_game(&Game::new(), MAX_FRAMES);
    for a in ALL3 {
        assert_eq!(ev(&not(probe(a)), &l, 0), a.not(), "not {a:?}");
        for b in ALL3 {
            assert_eq!(
                ev(&all_of(vec![probe(a), probe(b)]), &l, 0),
                kleene_and(a, b),
                "all[{a:?},{b:?}]"
            );
            assert_eq!(
                ev(&any_of(vec![probe(a), probe(b)]), &l, 0),
                kleene_or(a, b),
                "any[{a:?},{b:?}]"
            );
            for c in ALL3 {
                assert_eq!(
                    ev(&all_of(vec![probe(a), probe(b), probe(c)]), &l, 0),
                    kleene_and(kleene_and(a, b), c),
                    "all[{a:?},{b:?},{c:?}]"
                );
                assert_eq!(
                    ev(&any_of(vec![probe(a), probe(b), probe(c)]), &l, 0),
                    kleene_or(kleene_or(a, b), c),
                    "any[{a:?},{b:?},{c:?}]"
                );
            }
        }
    }
}

#[test]
fn double_negation_and_de_morgan_hold_with_unknown() {
    let l = Line::from_game(&Game::new(), MAX_FRAMES);
    for a in ALL3 {
        for b in ALL3 {
            assert_eq!(ev(&not(not(probe(a))), &l, 0), a);
            let lhs = not(all_of(vec![probe(a), probe(b)]));
            let rhs = any_of(vec![not(probe(a)), not(probe(b))]);
            assert_eq!(ev(&lhs, &l, 0), ev(&rhs, &l, 0), "{a:?} {b:?}");
        }
    }
}

#[test]
fn unknown_never_matches_nor_excludes_in_a_line() {
    let l = line(None, "e4 e5", GameResult::Unknown);
    let run = |c: Condition| {
        let mut p = root(c);
        p.scope.min_move = 0;
        match_line(&p, &l, None)
    };
    // Not(Unknown) does not match (rule 2) and counts as unknown.
    let m = run(not(u()));
    assert_eq!(m.ply, None);
    assert_eq!(m.examined, 3);
    assert_eq!(m.unknown_positions, 3);
    // All[True, Unknown] does not match; the unknown decided every position.
    let m = run(all_of(vec![t(), u()]));
    assert_eq!((m.ply, m.unknown_positions), (None, 3));
    // Any[False, Unknown] likewise.
    let m = run(any_of(vec![f(), u()]));
    assert_eq!((m.ply, m.unknown_positions), (None, 3));
    // Where the unknown does not decide, it is not counted.
    let m = run(any_of(vec![t(), u()]));
    assert_eq!((m.ply, m.unknown_positions), (Some(0), 0));
    let m = run(all_of(vec![f(), u()]));
    assert_eq!((m.ply, m.unknown_positions), (None, 0));
    // Not(Not(Unknown)) is still not a match.
    assert_eq!(run(not(not(u()))).ply, None);
    // The honest "not mate": Not(False) still matches.
    assert_eq!(run(not(f())).ply, Some(0));
}

// ------------------------------------------------------------ cancelling

#[test]
fn a_cancelled_token_makes_available_mate_unknown_but_not_the_cheap_conditions() {
    let cancel = CancelToken::new();
    cancel.cancel();
    let l = fen_line("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1");
    // Probes valid at this position: White to move, not in check.
    let tt = Condition::SideToMove { side: Side::White };
    let ff = Condition::Status {
        status: Status::Check,
    };
    let easy = mate(Who::White, 1, 1, MateMode::Available, true, 20_000);
    assert_eq!(eval_position(&easy, &l, 0, None), Tri::True);
    assert_eq!(eval_position(&easy, &l, 0, Some(&cancel)), Tri::Unknown);
    // Cheap conditions do not look at the token.
    assert_eq!(eval_position(&tt, &l, 0, Some(&cancel)), Tri::True);
    // A cheap False decides before the mate is even tried.
    assert_eq!(
        eval_position(
            &all_of(vec![easy.clone(), ff.clone()]),
            &l,
            0,
            Some(&cancel)
        ),
        Tri::False
    );
    // And a cheap True short-circuits the OR.
    assert_eq!(
        eval_position(&any_of(vec![easy, tt.clone()]), &l, 0, Some(&cancel)),
        Tri::True
    );
    // match_line with a cancelled token: no match, the position is unknown.
    let mut p = root(mate(Who::White, 1, 1, MateMode::Available, true, 20_000));
    p.scope.min_move = 0;
    let m = match_line(&p, &l, Some(&cancel));
    assert_eq!(m.ply, None);
    assert_eq!(m.unknown_positions, 1);
}

#[test]
fn cancelling_in_the_middle_ends_a_long_available_mate_scan_promptly() {
    // 120 positions of a game in which each `available` search is slow
    // (2 million nodes of budget, no forcing restriction). Without the
    // cancel it would take minutes; with it the scan must end at once.
    let mut sans = String::new();
    for _ in 0..30 {
        sans.push_str("Nf3 Nf6 Ng1 Ng8 ");
    }
    let l = line(None, &sans, GameResult::Unknown);
    assert_eq!(l.frames.len(), 121);
    let mut p = root(mate(Who::Any, 8, 8, MateMode::Available, false, 2_000_000));
    p.scope.min_move = 0;
    let cancel = CancelToken::new();
    let remote = cancel.clone();
    let handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        remote.cancel();
    });
    let started = std::time::Instant::now();
    let m = match_line(&p, &l, Some(&cancel));
    handle.join().expect("canceller");
    assert_eq!(m.ply, None);
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "cancellation was not honoured: {:?}",
        started.elapsed()
    );
    assert!(m.unknown_positions >= 1);
}

// ---------------------------------------------------------- scope rules

/// `1.e4 d5 2.exd5 Nf6 3.Nc3 Nxd5`: White is a pawn up from ply 3 to 5.
const PAWN_UP: &str = "e4 d5 exd5 Nf6 Nc3 Nxd5";

fn pawn_up() -> Condition {
    Condition::Material(MaterialCond {
        diff: Some(MinMax { min: 1, max: 100 }),
        ..MaterialCond::default()
    })
}

#[test]
fn the_material_probe_holds_exactly_on_plies_3_to_5() {
    let l = line(None, PAWN_UP, GameResult::Unknown);
    let holds: Vec<usize> = (0..l.frames.len())
        .filter(|&i| ev(&pawn_up(), &l, i).is_true())
        .collect();
    assert_eq!(holds, vec![3, 4, 5]);
    // The frames carry the balance (White minus Black).
    let balances: Vec<i16> = l.frames.iter().map(|f| f.balance).collect();
    assert_eq!(balances, vec![0, 0, 0, 1, 1, 1, 0]);
}

#[rstest]
// (min_move, max_move, hold, expected ply)
#[case::default_scope(1, 999, 1, Some(3))]
#[case::hold_3_fits_exactly(1, 999, 3, Some(3))]
#[case::hold_4_never_fits(1, 999, 4, None)]
#[case::window_ends_inside_the_run(1, 2, 3, None)]
#[case::window_ends_inside_the_run_hold_2(1, 2, 2, Some(3))]
#[case::window_starts_inside_the_run(3, 999, 1, Some(5))]
#[case::window_starts_inside_the_run_hold_2(3, 999, 2, None)]
#[case::window_larger_than_hold(0, 999, 3, Some(3))]
#[case::window_before_the_run(1, 1, 1, None)]
#[case::max_below_min(5, 4, 1, None)]
#[case::min_beyond_the_game(50, 60, 1, None)]
#[case::single_ply_window(2, 2, 1, Some(3))]
fn range_and_hold_follow_rules_6_and_7(
    #[case] min_move: u16,
    #[case] max_move: u16,
    #[case] hold: u8,
    #[case] expected: Option<u16>,
) {
    // Positions are scanned from 2*min_move - 1 up to 2*max_move.
    let l = line(None, PAWN_UP, GameResult::Unknown);
    let mut p = root(pawn_up());
    p.scope = Scope {
        min_move,
        max_move,
        hold,
        end_only: false,
        ignore_colors: false,
    };
    assert_eq!(first(&p, &l), expected);
}

#[test]
fn the_window_for_min_move_one_skips_the_initial_position_but_zero_includes_it() {
    let l = line(None, "e4 e5", GameResult::Unknown);
    let mut p = root(Condition::SideToMove { side: Side::White });
    p.scope.min_move = 1;
    // White to move at plies 0 and 2; ply 0 is outside the window.
    assert_eq!(first(&p, &l), Some(2));
    p.scope.min_move = 0;
    assert_eq!(first(&p, &l), Some(0));
    assert_eq!(matching_plies(&p, &l, None), vec![0, 2]);
    // max_move = 0 with min_move = 0 only looks at the initial position.
    p.scope.max_move = 0;
    assert_eq!(matching_plies(&p, &l, None), vec![0]);
    // Black to move: odd plies only.
    let mut b = root(Condition::SideToMove { side: Side::Black });
    b.scope.min_move = 2;
    b.scope.max_move = 2;
    assert_eq!(matching_plies(&b, &l, None), Vec::<u16>::new());
    let long = line(None, "e4 e5 Nf3 Nc6 Bb5", GameResult::Unknown);
    assert_eq!(matching_plies(&b, &long, None), vec![3]);
}

#[test]
fn matching_plies_reports_each_maximal_run_once() {
    // Material: pawn up on plies 3-5, then level, then up again.
    let l = line(
        None,
        "e4 d5 exd5 Nf6 Nc3 Nxd5 Nxd5 Qxd5 d4 Qe6+ Be3 Qxa2",
        GameResult::Unknown,
    );
    let ups: Vec<usize> = (0..l.frames.len())
        .filter(|&i| ev(&pawn_up(), &l, i).is_true())
        .collect();
    assert!(ups.contains(&3) && ups.contains(&5));
    let mut p = root(pawn_up());
    let runs = matching_plies(&p, &l, None);
    assert_eq!(runs.first(), Some(&3));
    assert!(runs.windows(2).all(|w| w[0] < w[1]));
    // With hold 3 only the long run counts.
    p.scope.hold = 3;
    assert_eq!(matching_plies(&p, &l, None), vec![3]);
    assert_eq!(first(&p, &l), Some(3));
}

#[test]
fn end_only_looks_at_the_last_position_and_ignores_the_rest_of_the_scope() {
    let l = line(None, "e4 e5 Nf3", GameResult::Unknown); // 4 frames, Black to move last
    let black_last = Condition::SideToMove { side: Side::Black };
    let mut p = root(black_last);
    p.scope = Scope {
        end_only: true,
        min_move: 99,
        max_move: 100,
        hold: 40,
        ignore_colors: false,
    };
    assert_eq!(first(&p, &l), Some(3));
    // The condition holds at ply 1 as well, but only the end counts.
    let mut white = root(Condition::SideToMove { side: Side::White });
    white.scope = p.scope.clone();
    assert_eq!(first(&white, &l), None);
    // A move cut in the middle: end_only on a game with no moves examines
    // the initial position (it is the last one).
    let empty = line(None, "", GameResult::Unknown);
    let mut start = root(t());
    start.scope.end_only = true;
    assert_eq!(first(&start, &empty), Some(0));
    // The `next` move of the last position does not exist.
    let mut next = root(Condition::Move(mc(WhichMove::Next)));
    next.scope.end_only = true;
    assert_eq!(first(&next, &l), None);
    // ... while without end_only it exists for the earlier ones.
    next.scope.end_only = false;
    next.scope.min_move = 0;
    assert_eq!(first(&next, &l), Some(0));
}

#[test]
fn a_game_with_no_moves_follows_rule_5() {
    let l = line(None, "", GameResult::Unknown);
    let mut p = root(t());
    // min_move >= 1: nothing to look at ("zero length" in Scid).
    assert_eq!(first(&p, &l), None);
    p.scope.min_move = 0;
    assert_eq!(first(&p, &l), Some(0));
    // The initial position of a non-standard FEN is evaluated too.
    let odd = line(
        Some("4k3/8/8/8/8/8/8/R3K3 w - - 0 1"),
        "",
        GameResult::Unknown,
    );
    let mut q = root(piece(Who::White, &[Rook], Region::Anywhere, 1, None));
    assert_eq!(first(&q, &odd), None);
    q.scope.min_move = 0;
    assert_eq!(first(&q, &odd), Some(0));
}

#[test]
fn examined_counts_the_positions_of_the_window() {
    let l = line(None, PAWN_UP, GameResult::Unknown);
    let mut p = root(f()); // never matches, so everything in the window is looked at
    assert_eq!(match_line(&p, &l, None).examined, 6); // plies 1..=6
    p.scope.min_move = 0;
    assert_eq!(match_line(&p, &l, None).examined, 7);
    p.scope.min_move = 2;
    p.scope.max_move = 2;
    assert_eq!(match_line(&p, &l, None).examined, 2); // plies 3 and 4
    p.scope.end_only = true;
    assert_eq!(match_line(&p, &l, None).examined, 1);
}

// ------------------------------------------------------ constants folding

#[test]
fn fold_constants_table() {
    let r = |k: ResultKind| Condition::Result { results: vec![k] };
    let w = ResultKind::WhiteWins;
    let b = ResultKind::BlackWins;
    // A bare result leaf.
    assert_eq!(fold_constants(&r(w), w), Some(true));
    assert_eq!(fold_constants(&r(w), b), Some(false));
    assert_eq!(
        fold_constants(
            &Condition::Result {
                results: vec![w, ResultKind::Draw]
            },
            ResultKind::Draw
        ),
        Some(true)
    );
    // Positional leaves are not constants.
    assert_eq!(fold_constants(&t(), w), None);
    assert_eq!(fold_constants(&u(), w), None);
    // All: one false decides; trues alone give true; a positional leaf blocks.
    assert_eq!(fold_constants(&all_of(vec![r(w), t()]), b), Some(false));
    assert_eq!(fold_constants(&all_of(vec![r(w), t()]), w), None);
    assert_eq!(fold_constants(&all_of(vec![r(w), r(w)]), w), Some(true));
    // Any: one true decides.
    assert_eq!(fold_constants(&any_of(vec![r(w), t()]), w), Some(true));
    assert_eq!(fold_constants(&any_of(vec![r(w), t()]), b), None);
    assert_eq!(
        fold_constants(&any_of(vec![r(w), r(b)]), ResultKind::Draw),
        Some(false)
    );
    // Not.
    assert_eq!(fold_constants(&not(r(w)), b), Some(true));
    assert_eq!(fold_constants(&not(all_of(vec![r(w), t()])), b), Some(true));
    assert_eq!(fold_constants(&not(all_of(vec![r(w), t()])), w), None);
    assert_eq!(fold_constants(&not(t()), w), None);
    // A false conjunct wins even next to an unknown-looking mate.
    assert_eq!(fold_constants(&all_of(vec![u(), r(w)]), b), Some(false));
}

#[test]
fn a_folded_out_game_is_skipped_without_looking_at_any_position() {
    let l = line(None, PAWN_UP, GameResult::Black);
    let p = root(all_of(vec![
        Condition::Result {
            results: vec![ResultKind::WhiteWins],
        },
        pawn_up(),
    ]));
    let m = match_line(&p, &l, None);
    assert_eq!(m, LineMatch::default());
    // The same game with the right result is searched.
    let w = line(None, PAWN_UP, GameResult::White);
    assert_eq!(first(&p, &w), Some(3));
    assert!(match_line(&p, &w, None).examined > 0);
    // A result of `*` is the `Unknown` result.
    let star = line(None, PAWN_UP, GameResult::Unknown);
    let q = root(all_of(vec![
        Condition::Result {
            results: vec![ResultKind::Unknown],
        },
        pawn_up(),
    ]));
    assert_eq!(first(&q, &star), Some(3));
    assert_eq!(first(&q, &w), None);
    // `Not(Result)` folds too.
    let nr = root(all_of(vec![
        not(Condition::Result {
            results: vec![ResultKind::Draw],
        }),
        pawn_up(),
    ]));
    assert_eq!(first(&nr, &w), Some(3));
    let draw = line(None, PAWN_UP, GameResult::Draw);
    assert_eq!(first(&nr, &draw), None);
    assert_eq!(match_line(&nr, &draw, None).examined, 0);
}

#[test]
fn result_conditions_in_the_middle_of_a_tree_are_evaluated_per_game() {
    let l = line(None, PAWN_UP, GameResult::Draw);
    let c = Condition::Result {
        results: vec![ResultKind::Draw, ResultKind::WhiteWins],
    };
    assert_eq!(ev(&c, &l, 0), Tri::True);
    assert_eq!(ev(&c, &l, 4), Tri::True);
    let d = Condition::Result {
        results: vec![ResultKind::BlackWins],
    };
    assert_eq!(ev(&d, &l, 4), Tri::False);
}

// ---------------------------------------------------------- ignore_colors

fn knight_f3() -> Condition {
    piece(Who::White, &[Knight], squares(&["f3"]), 1, None)
}

#[test]
fn ignore_colors_tries_the_mirror_only_if_the_original_failed() {
    let mut p = root(knight_f3());
    // The original matches at ply 3 (2.Nf3); the mirror would match at ply 2
    // (1...Nf6 seen with the colours swapped). Rule 9: the original wins.
    let both = line(None, "e4 Nf6 Nf3", GameResult::Unknown);
    assert_eq!(first(&p, &both), Some(3));
    p.scope.ignore_colors = true;
    assert_eq!(first(&p, &both), Some(3));
    assert_eq!(matching_plies(&p, &both, None), vec![3]);
    // Only Black plays ...Nf6: found only with the mirror, at the same ply.
    let black_only = line(None, "e4 Nf6", GameResult::Unknown);
    assert_eq!(first(&p, &black_only), Some(2));
    p.scope.ignore_colors = false;
    assert_eq!(first(&p, &black_only), None);
}

#[test]
fn ignore_colors_swaps_the_result_and_the_regions() {
    // "White knight on rank 3 (own rank 3) and White wins" found in a game
    // that Black wins with the knight on f6.
    let cond = all_of(vec![
        Condition::Result {
            results: vec![ResultKind::WhiteWins],
        },
        piece(Who::White, &[Knight], Region::Rank(3), 1, None),
    ]);
    let mut p = root(cond);
    let black_wins = line(None, "e4 Nf6", GameResult::Black);
    assert_eq!(first(&p, &black_wins), None);
    p.scope.ignore_colors = true;
    assert_eq!(first(&p, &black_wins), Some(2));
    // A white win with the same knight is not a match for the mirrored game
    // but is a match for the original orientation only if White's knight is
    // on rank 3.
    let white_wins_black_knight = line(None, "e4 Nf6", GameResult::White);
    assert_eq!(first(&p, &white_wins_black_knight), None);
    // The folded-out check uses the swapped result: a draw never matches.
    let draw = line(None, "e4 Nf6", GameResult::Draw);
    assert_eq!(match_line(&p, &draw, None).examined, 0);
}

#[test]
fn ignore_colors_with_a_symmetric_pattern_changes_nothing() {
    let symmetric = [
        t(),
        Condition::Phase {
            phase: Phase::Middlegame,
        },
        piece(Who::Any, &[Queen], Region::Anywhere, 1, None),
        Condition::Material(MaterialCond {
            diff: Some(MinMax { min: 0, max: 0 }),
            ..MaterialCond::default()
        }),
        Condition::Status {
            status: Status::Check,
        },
    ];
    let games = [
        line(
            None,
            "e4 e5 Nf3 Nc6 Bb5 a6 Bxc6 dxc6 Nxe5",
            GameResult::Unknown,
        ),
        line(None, "e4 e5 Qh5 Nc6 Bc4 Nf6 Qxf7#", GameResult::White),
        line(None, PAWN_UP, GameResult::Draw),
    ];
    for cond in symmetric {
        for g in &games {
            let mut plain = root(cond.clone());
            plain.scope.min_move = 0;
            let mut mirrored = plain.clone();
            mirrored.scope.ignore_colors = true;
            assert_eq!(
                match_line(&plain, g, None).ply,
                match_line(&mirrored, g, None).ply,
                "{cond:?}"
            );
            assert_eq!(
                matching_plies(&plain, g, None),
                matching_plies(&mirrored, g, None),
                "{cond:?}"
            );
        }
    }
}

#[test]
fn the_mirrored_line_is_an_involution_and_swaps_result_and_balance() {
    let l = line(None, "e4 e5 Nf3 Nc6 Bb5 a6 Bxc6", GameResult::White);
    let m = l.mirrored().expect("mirror");
    assert_eq!(m.result, ResultKind::BlackWins);
    assert_eq!(m.frames.len(), l.frames.len());
    for (a, b) in l.frames.iter().zip(&m.frames) {
        assert_eq!(b.balance, -a.balance);
        assert_eq!(b.pos.turn(), a.pos.turn().other());
        assert_eq!(b.mv, a.mv.map(tactics::mirror_move));
    }
    let back = m.mirrored().expect("mirror again");
    assert_eq!(back.result, l.result);
    for (a, b) in l.frames.iter().zip(&back.frames) {
        assert_eq!(
            a.pos.fen().split(' ').take(4).collect::<Vec<_>>(),
            b.pos.fen().split(' ').take(4).collect::<Vec<_>>()
        );
        assert_eq!(a.mv, b.mv);
        assert_eq!(a.balance, b.balance);
    }
    assert_eq!(ResultKind::WhiteWins.swapped(), ResultKind::BlackWins);
    assert_eq!(ResultKind::BlackWins.swapped(), ResultKind::WhiteWins);
    assert_eq!(ResultKind::Draw.swapped(), ResultKind::Draw);
    assert_eq!(ResultKind::Unknown.swapped(), ResultKind::Unknown);
}

#[test]
fn mirror_keeps_castling_and_en_passant_playable_in_the_line() {
    let l = line(
        None,
        "e4 a6 e5 d5 exd6 Nf6 Nf3 e6 Be2 Be7 O-O O-O",
        GameResult::Unknown,
    );
    let m = l.mirrored().expect("mirror");
    assert_eq!(m.frames.len(), 13);
    // The en passant capture and both castlings are still capture / castle
    // moves in the mirrored line.
    let castles = m
        .frames
        .iter()
        .filter(|fr| fr.mv.is_some_and(|mv| mv.castling_side().is_some()))
        .count();
    assert_eq!(castles, 2);
    let captures = m
        .frames
        .iter()
        .filter(|fr| fr.mv.is_some_and(mychess_core::Move::is_capture))
        .count();
    assert_eq!(captures, 1);
}

// ---------------------------------------------------------- the frame limit

fn shuffle(cycles: usize, tail: &str) -> Game {
    let mut sans = "Nf3 Nf6 Ng1 Ng8 ".repeat(cycles);
    sans.push_str(tail);
    game(None, &sans, GameResult::Unknown)
}

#[test]
fn a_game_longer_than_2048_frames_is_truncated_and_flagged() {
    assert_eq!(MAX_FRAMES, 2048);
    // 2047 plies = 2048 frames: complete.
    let exact = Line::from_game(&shuffle(511, "Nf3 Nf6 Ng1"), MAX_FRAMES);
    assert_eq!(exact.frames.len(), 2048);
    assert!(!exact.truncated);
    // 2048 plies = 2049 frames: cut at 2048.
    let over = Line::from_game(&shuffle(512, ""), MAX_FRAMES);
    assert_eq!(over.frames.len(), 2048);
    assert!(over.truncated);
    assert!(!over.ends_in_mate);
    // A smaller limit.
    let small = Line::from_game(&shuffle(2, ""), 5);
    assert_eq!(small.frames.len(), 5);
    assert!(small.truncated);
    let tiny = Line::from_game(&shuffle(2, ""), 0);
    assert_eq!(tiny.frames.len(), 1);
    assert!(tiny.truncated);
    let none = Line::from_game(&Game::new(), 1);
    assert_eq!(none.frames.len(), 1);
    assert!(!none.truncated);
    assert!(none.frames[0].mv.is_none());
}

#[test]
fn a_truncated_line_never_counts_as_ending_in_mate() {
    // 2048 plies of shuffling, then the fool's mate: ply 2052.
    let g = shuffle(512, "f3 e5 g4 Qh4#");
    let full = Line::from_game(&g, 3000);
    assert_eq!(full.frames.len(), 2053);
    assert!(full.ends_in_mate && !full.truncated);
    let cut = Line::from_game(&g, MAX_FRAMES);
    assert!(cut.truncated && !cut.ends_in_mate);
    // `played` mate by Black, 1 move: found on the full line (position 2051),
    // never on the truncated one.
    let mut p = root(mate(Who::Black, 1, 1, MateMode::Played, true, 20_000));
    p.scope.max_move = 2000;
    assert_eq!(first(&p, &full), Some(2051));
    assert_eq!(first(&p, &cut), None);
    // end_only on the full line sees the mate itself.
    let mut end = root(Condition::Status {
        status: Status::Checkmate,
    });
    end.scope.end_only = true;
    assert_eq!(first(&end, &full), Some(2052));
    assert_eq!(first(&end, &cut), None);
    // The default scope (max_move 999) cannot see past ply 1998.
    let deep = root(Condition::Status {
        status: Status::Checkmate,
    });
    assert_eq!(first(&deep, &full), None);
}

#[test]
fn promotion_updates_the_balance_with_the_new_piece() {
    // 1 -> 9 (+8): the pawn disappears and the queen appears.
    let l = line(
        Some("7k/P7/8/8/8/8/8/4K3 w - - 0 1"),
        "a8=Q+",
        GameResult::Unknown,
    );
    assert_eq!(l.frames[0].balance, 1);
    assert_eq!(l.frames[1].balance, 9);
    let n = line(
        Some("7k/P7/8/8/8/8/8/4K3 w - - 0 1"),
        "a8=N",
        GameResult::Unknown,
    );
    assert_eq!(n.frames[1].balance, 3);
    // Capturing promotion: pawn x rook = queen.
    let c = line(
        Some("1r5k/P7/8/8/8/8/8/4K3 w - - 0 1"),
        "axb8=Q+",
        GameResult::Unknown,
    );
    assert_eq!(c.frames[0].balance, 1 - 5);
    assert_eq!(c.frames[1].balance, 9);
}

// ----------------------------------------------------------- conditions

#[rstest]
// side, roles, region, min, max, fen, expected
#[case::any_side_sees_black_queen(Who::Any, &[Queen], squares(&["d8"]), 1, None, "start", true)]
#[case::white_does_not_own_d8(Who::White, &[Queen], squares(&["d8"]), 1, None, "start", false)]
#[case::mover_is_white_at_start(Who::Mover, &[Queen], squares(&["d1"]), 1, None, "start", true)]
#[case::opponent_is_black_at_start(Who::Opponent, &[Queen], squares(&["d8"]), 1, None, "start", true)]
#[case::opponent_is_not_white(Who::Opponent, &[Queen], squares(&["d1"]), 1, None, "start", false)]
#[case::no_roles_means_every_piece_king_included(Who::White, &[], squares(&["e1"]), 1, None, "start", true)]
#[case::king_role_explicit(Who::White, &[King], squares(&["e1"]), 1, None, "start", true)]
#[case::pawns_on_the_second_rank(Who::White, &[Pawn], Region::Rank(2), 8, Some(8), "start", true)]
#[case::own_rank_two_is_rank_seven_for_black(Who::Black, &[Pawn], Region::OwnRank(2), 8, Some(8), "start", true)]
#[case::own_rank_seven_for_black_is_absolute_two(Who::Black, &[Pawn], Region::OwnRank(7), 1, None, "start", false)]
#[case::own_rank_seven_for_white(Who::White, &[Pawn], Region::OwnRank(7), 1, None, "start", false)]
#[case::exactly_none_of_them(Who::White, &[Queen], Region::Anywhere, 0, Some(0), "start", false)]
#[case::none_holds_without_queens(Who::White, &[Queen], Region::Anywhere, 0, Some(0), "4k3/8/8/8/8/8/8/4K3 w - - 0 1", true)]
#[case::max_cuts(Who::White, &[Knight], Region::Anywhere, 1, Some(1), "start", false)]
#[case::min_two_knights(Who::White, &[Knight], Region::Anywhere, 2, None, "start", true)]
#[case::file_region(Who::White, &[Rook], Region::File('a'), 1, None, "start", true)]
#[case::file_region_miss(Who::White, &[Rook], Region::File('b'), 1, None, "start", false)]
#[case::rect_inverted_is_normalised(Who::White, &[Knight], Region::Rect { from: sq("c3"), to: sq("a1") }, 1, None, "start", true)]
#[case::rect_corner_order_does_not_matter(Who::White, &[Knight], Region::Rect { from: sq("a1"), to: sq("c3") }, 1, None, "start", true)]
#[case::duplicate_squares_are_a_set(Who::White, &[King], squares(&["e1", "e1", "e1"]), 1, Some(1), "start", true)]
#[case::named_centre_empty_at_start(Who::Any, &[], Region::Named(NamedRegion::Center), 1, None, "start", false)]
#[case::named_black_half(Who::Black, &[King], Region::Named(NamedRegion::BlackHalf), 1, None, "start", true)]
#[case::named_light_squares(Who::White, &[Bishop], Region::Named(NamedRegion::LightSquares), 1, None, "start", true)]
#[case::named_dark_squares(Who::White, &[Bishop], Region::Named(NamedRegion::DarkSquares), 1, None, "start", true)]
#[case::named_edge(Who::White, &[Rook], Region::Named(NamedRegion::Edge), 2, Some(2), "start", true)]
#[case::named_long_diagonal(Who::White, &[Rook], Region::Named(NamedRegion::LongDiagonals), 2, Some(2), "start", true)]
#[case::named_queen_side(Who::White, &[Queen], Region::Named(NamedRegion::QueenSide), 1, None, "start", true)]
#[case::named_king_side(Who::White, &[King], Region::Named(NamedRegion::KingSide), 1, None, "start", true)]
fn piece_condition_table(
    #[case] side: Who,
    #[case] roles: &[RoleName],
    #[case] region: Region,
    #[case] min: u8,
    #[case] max: Option<u8>,
    #[case] fen: &str,
    #[case] expected: bool,
) {
    let fen = if fen == "start" {
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
    } else {
        fen
    };
    let got = at(&piece(side, roles, region, min, max), fen);
    assert_eq!(got, Tri::from_bool(expected), "{fen}");
}

#[test]
fn who_any_is_per_colour_not_a_union() {
    // 7 white pawns on the second rank and 7 black on the seventh: neither
    // colour has 8, though together they have 14.
    let c = piece(Who::Any, &[Pawn], Region::OwnRank(2), 8, None);
    assert_eq!(
        at(&c, "4k3/pppppp1p/8/8/8/8/PPPPPPP1/4K3 w - - 0 1"),
        Tri::False
    );
    assert_eq!(
        at(&c, "4k3/pppppppp/8/8/8/8/PPPPPPP1/4K3 w - - 0 1"),
        Tri::True
    );
    assert_eq!(
        at(&c, "4k3/pppppp1p/8/8/8/8/PPPPPPPP/4K3 w - - 0 1"),
        Tri::True
    );
}

#[test]
fn mover_and_opponent_follow_the_position_evaluated() {
    let l = line(None, "e4", GameResult::Unknown);
    let mover_king = piece(Who::Mover, &[King], squares(&["e8"]), 1, None);
    assert_eq!(ev(&mover_king, &l, 0), Tri::False);
    assert_eq!(ev(&mover_king, &l, 1), Tri::True);
    let opp = piece(Who::Opponent, &[Pawn], squares(&["e4"]), 1, None);
    assert_eq!(ev(&opp, &l, 0), Tri::False);
    assert_eq!(ev(&opp, &l, 1), Tri::True);
}

#[test]
fn side_status_and_result_conditions() {
    let l = line(None, "f3 e5 g4 Qh4#", GameResult::Black);
    let side = |s| Condition::SideToMove { side: s };
    assert_eq!(ev(&side(Side::White), &l, 0), Tri::True);
    assert_eq!(ev(&side(Side::Black), &l, 1), Tri::True);
    let status = |s| Condition::Status { status: s };
    assert_eq!(ev(&status(Status::Checkmate), &l, 4), Tri::True);
    assert_eq!(ev(&status(Status::Checkmate), &l, 3), Tri::False);
    // A mated side is also in check.
    assert_eq!(ev(&status(Status::Check), &l, 4), Tri::True);
    assert_eq!(ev(&status(Status::Stalemate), &l, 4), Tri::False);
    let stale = fen_line("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
    assert_eq!(ev(&status(Status::Stalemate), &stale, 0), Tri::True);
    assert_eq!(ev(&status(Status::Check), &stale, 0), Tri::False);
    assert_eq!(ev(&status(Status::Checkmate), &stale, 0), Tri::False);
    // Check on the side to move only: the attacker's own king is not asked.
    let check = fen_line("4k3/8/8/8/8/8/8/4RK2 b - - 0 1");
    assert_eq!(ev(&status(Status::Check), &check, 0), Tri::True);
    assert_eq!(ev(&status(Status::Checkmate), &check, 0), Tri::False);
}

#[rstest]
#[case::start_is_opening(
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    Phase::Opening
)]
#[case::t56_is_opening(
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/R1BQKB1R w KQkq - 0 1",
    Phase::Opening
)]
#[case::t54_is_middlegame(
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKB2 w Qkq - 0 1",
    Phase::Middlegame
)]
#[case::t25_is_middlegame("rn2k1n1/8/8/8/8/8/8/R2QK3 w - - 0 1", Phase::Middlegame)]
#[case::t24_is_endgame("r3k2r/8/8/8/8/8/8/R2QK3 w - - 0 1", Phase::Endgame)]
#[case::bare_kings_and_pawns("4k3/pppp4/8/8/8/8/PPPP4/4K3 w - - 0 1", Phase::Endgame)]
fn phase_condition_at_the_exact_thresholds(#[case] fen: &str, #[case] expected: Phase) {
    for phase in [Phase::Opening, Phase::Middlegame, Phase::Endgame] {
        let got = at(&Condition::Phase { phase }, fen);
        assert_eq!(got, Tri::from_bool(phase == expected), "{fen} {phase:?}");
    }
}

#[test]
fn material_condition_counts_diff_and_bishop_colours() {
    let mat = |white: Counts, black: Counts, diff, bishops| {
        Condition::Material(MaterialCond {
            white,
            black,
            diff,
            bishops,
        })
    };
    let one = |n: u8| Some(MinMax { min: n, max: n });
    let fen = "4k3/pp6/2b5/8/8/8/PPP5/2B1KN2 w - - 0 1";
    // White: B, N, 3 pawns; Black: B, 2 pawns. diff = 3+3+3 - 3 - 2 = 4.
    assert_eq!(
        at(
            &mat(
                Counts::default(),
                Counts::default(),
                Some(MinMax { min: 4, max: 4 }),
                BishopColors::Either
            ),
            fen
        ),
        Tri::True
    );
    assert_eq!(
        at(
            &mat(
                Counts::default(),
                Counts::default(),
                Some(MinMax { min: 5, max: 9 }),
                BishopColors::Either
            ),
            fen
        ),
        Tri::False
    );
    assert_eq!(
        at(
            &mat(
                Counts {
                    pawns: one(3),
                    ..Counts::default()
                },
                Counts {
                    pawns: one(2),
                    ..Counts::default()
                },
                None,
                BishopColors::Either
            ),
            fen
        ),
        Tri::True
    );
    assert_eq!(
        at(
            &mat(
                Counts {
                    minors: one(2),
                    ..Counts::default()
                },
                Counts {
                    minors: one(1),
                    ..Counts::default()
                },
                None,
                BishopColors::Either
            ),
            fen
        ),
        Tri::True
    );
    assert_eq!(
        at(
            &mat(
                Counts {
                    queens: one(0),
                    rooks: one(0),
                    knights: one(1),
                    bishops: one(1),
                    ..Counts::default()
                },
                Counts::default(),
                None,
                BishopColors::Either
            ),
            fen
        ),
        Tri::True
    );
    assert_eq!(
        at(
            &mat(
                Counts {
                    knights: one(2),
                    ..Counts::default()
                },
                Counts::default(),
                None,
                BishopColors::Either
            ),
            fen
        ),
        Tri::False
    );
    // c1 is dark, c6 is light: opposite-coloured bishops.
    assert_eq!(
        at(
            &mat(
                Counts::default(),
                Counts::default(),
                None,
                BishopColors::Opposite
            ),
            fen
        ),
        Tri::True
    );
    assert_eq!(
        at(
            &mat(
                Counts::default(),
                Counts::default(),
                None,
                BishopColors::Same
            ),
            fen
        ),
        Tri::False
    );
    // Same colour: c1 (dark) and f8 (dark... f8 is light), use c1 and a3.
    let same = "4k3/8/8/8/8/b7/8/2B1K3 w - - 0 1";
    assert_eq!(
        at(
            &mat(
                Counts::default(),
                Counts::default(),
                None,
                BishopColors::Same
            ),
            same
        ),
        Tri::True
    );
    assert_eq!(
        at(
            &mat(
                Counts::default(),
                Counts::default(),
                None,
                BishopColors::Opposite
            ),
            same
        ),
        Tri::False
    );
    // Negative diff range.
    let down = "4k3/pp6/8/8/8/8/8/4K3 w - - 0 1";
    assert_eq!(
        at(
            &mat(
                Counts::default(),
                Counts::default(),
                Some(MinMax { min: -2, max: -2 }),
                BishopColors::Either
            ),
            down
        ),
        Tri::True
    );
}

#[test]
fn feature_conditions_use_region_min_and_who() {
    let feat = |feature, side, region, min| {
        Condition::Feature(FeatureCond {
            feature,
            side,
            region,
            min,
        })
    };
    // White: passed d5, isolated a2 and h2, doubled c2/c3. Black king far.
    let fen = "4k3/8/8/3P4/8/2P4P/P1P5/4K3 w - - 0 1";
    assert_eq!(
        at(
            &feat(Feature::PassedPawn, Who::White, Region::Anywhere, 1),
            fen
        ),
        Tri::True
    );
    assert_eq!(
        at(
            &feat(Feature::PassedPawn, Who::White, Region::Anywhere, 5),
            fen
        ),
        Tri::True
    );
    assert_eq!(
        at(
            &feat(Feature::PassedPawn, Who::White, Region::Anywhere, 6),
            fen
        ),
        Tri::False
    );
    assert_eq!(
        at(
            &feat(Feature::PassedPawn, Who::Black, Region::Anywhere, 1),
            fen
        ),
        Tri::False
    );
    assert_eq!(
        at(
            &feat(Feature::PassedPawn, Who::Any, Region::Anywhere, 1),
            fen
        ),
        Tri::True
    );
    // The region limits the squares of the pawns.
    assert_eq!(
        at(
            &feat(Feature::PassedPawn, Who::White, squares(&["d5"]), 1),
            fen
        ),
        Tri::True
    );
    assert_eq!(
        at(
            &feat(Feature::PassedPawn, Who::White, squares(&["d5"]), 2),
            fen
        ),
        Tri::False
    );
    assert_eq!(
        at(
            &feat(Feature::PassedPawn, Who::White, Region::OwnRank(5), 1),
            fen
        ),
        Tri::True
    );
    assert_eq!(
        at(
            &feat(Feature::PassedPawn, Who::White, Region::OwnRank(6), 1),
            fen
        ),
        Tri::False
    );
    assert_eq!(
        at(
            &feat(Feature::DoubledPawns, Who::White, Region::Anywhere, 2),
            fen
        ),
        Tri::True
    );
    assert_eq!(
        at(
            &feat(Feature::DoubledPawns, Who::White, Region::Anywhere, 3),
            fen
        ),
        Tri::False
    );
    assert_eq!(
        at(
            &feat(Feature::IsolatedPawn, Who::White, squares(&["a2", "h3"]), 2),
            fen
        ),
        Tri::True
    );
    assert_eq!(
        at(
            &feat(Feature::IsolatedPawn, Who::Black, Region::Anywhere, 1),
            fen
        ),
        Tri::False
    );
    // Mover / Opponent.
    assert_eq!(
        at(
            &feat(Feature::PassedPawn, Who::Mover, Region::Anywhere, 1),
            fen
        ),
        Tri::True
    );
    assert_eq!(
        at(
            &feat(Feature::PassedPawn, Who::Opponent, Region::Anywhere, 1),
            fen
        ),
        Tri::False
    );
    // Bishop pair.
    let pair = "4k3/8/8/8/8/8/8/2B1KB2 w - - 0 1";
    assert_eq!(
        at(
            &feat(Feature::BishopPair, Who::White, Region::Anywhere, 1),
            pair
        ),
        Tri::True
    );
    assert_eq!(
        at(
            &feat(Feature::BishopPair, Who::Black, Region::Anywhere, 1),
            pair
        ),
        Tri::False
    );
    assert_eq!(
        at(
            &feat(Feature::BishopPair, Who::Any, Region::Anywhere, 1),
            pair
        ),
        Tri::True
    );
}

#[test]
fn exposed_king_feature_is_off_in_the_endgame() {
    let exposed = |side| {
        Condition::Feature(FeatureCond {
            feature: Feature::ExposedKing,
            side,
            region: Region::Anywhere,
            min: 1,
        })
    };
    // Middlegame, white king on g1 with only f2.
    let mid = "r1bq1rk1/ppp2ppp/2n5/8/8/8/5P2/RNBQ1RK1 w - - 0 1";
    assert_eq!(at(&exposed(Who::White), mid), Tri::True);
    assert_eq!(at(&exposed(Who::Black), mid), Tri::False);
    assert_eq!(at(&exposed(Who::Any), mid), Tri::True);
    let region = Condition::Feature(FeatureCond {
        feature: Feature::ExposedKing,
        side: Who::White,
        region: squares(&["h1"]),
        min: 1,
    });
    assert_eq!(at(&region, mid), Tri::False);
    // Endgame: same king, nothing exposed.
    assert_eq!(
        at(&exposed(Who::White), "4k3/8/8/8/8/8/5P2/6KR w - - 0 1"),
        Tri::False
    );
}

// -------------------------------------------------------- move conditions

#[test]
fn move_condition_flags_roles_regions_and_castling() {
    let l = line(
        None,
        "e4 a6 e5 d5 exd6 Nf6 Nf3 e6 Be2 Be7 O-O O-O",
        GameResult::Unknown,
    );
    // plies: 1 e4, 2 a6, 3 e5, 4 d5, 5 exd6 (ep), 6 Nf6, 7 Nf3, 8 e6, 9 Be2,
    // 10 Be7, 11 O-O, 12 O-O.
    let prev = |f: &dyn Fn(&mut MoveCond)| {
        let mut m = mc(WhichMove::Previous);
        f(&mut m);
        Condition::Move(m)
    };
    let cap_yes = prev(&|m| m.capture = Flag::Yes);
    let cap_no = prev(&|m| m.capture = Flag::No);
    assert_eq!(ev(&cap_yes, &l, 5), Tri::True, "en passant is a capture");
    assert_eq!(ev(&cap_yes, &l, 3), Tri::False);
    assert_eq!(ev(&cap_no, &l, 3), Tri::True);
    assert_eq!(ev(&cap_no, &l, 5), Tri::False);
    let check_yes = prev(&|m| m.check = Flag::Yes);
    let check_no = prev(&|m| m.check = Flag::No);
    let scholar = line(None, "e4 e5 Qh5 Nc6 Bc4 Nf6 Qxf7#", GameResult::White);
    assert_eq!(ev(&check_yes, &scholar, 7), Tri::True);
    assert_eq!(ev(&check_yes, &scholar, 6), Tri::False);
    assert_eq!(ev(&check_no, &scholar, 7), Tri::False);
    assert_eq!(ev(&check_no, &scholar, 6), Tri::True);
    let capture_and_check = prev(&|m| {
        m.check = Flag::Yes;
        m.capture = Flag::Yes;
        m.roles = vec![Queen];
    });
    assert_eq!(ev(&capture_and_check, &scholar, 7), Tri::True);
    // Castling kinds.
    let short = prev(&|m| m.castling = Some(CastleKind::Short));
    let long = prev(&|m| m.castling = Some(CastleKind::Long));
    let any_castle = prev(&|m| m.castling = Some(CastleKind::Any));
    assert_eq!(ev(&short, &l, 11), Tri::True);
    assert_eq!(ev(&short, &l, 12), Tri::True);
    assert_eq!(ev(&long, &l, 11), Tri::False);
    assert_eq!(ev(&any_castle, &l, 11), Tri::True);
    assert_eq!(ev(&any_castle, &l, 10), Tri::False);
    // By, roles, from, to.
    let by_black_pawn_to_d5 = prev(&|m| {
        m.by = Who::Black;
        m.roles = vec![Pawn];
        m.to = squares(&["d5"]);
        m.from = squares(&["d7"]);
    });
    assert_eq!(ev(&by_black_pawn_to_d5, &l, 4), Tri::True);
    assert_eq!(ev(&by_black_pawn_to_d5, &l, 3), Tri::False);
    let by_white_at_d5 = prev(&|m| {
        m.by = Who::White;
        m.to = squares(&["d5"]);
    });
    assert_eq!(ev(&by_white_at_d5, &l, 4), Tri::False);
    // Mover / Opponent for a move: the one who just moved is the opponent.
    let just_moved = prev(&|m| {
        m.by = Who::Opponent;
        m.roles = vec![Knight];
    });
    assert_eq!(ev(&just_moved, &l, 6), Tri::True); // 6...Nf6, White to move
    let mover_moved = prev(&|m| {
        m.by = Who::Mover;
        m.roles = vec![Knight];
    });
    assert_eq!(ev(&mover_moved, &l, 6), Tri::False);
    // Next move: the mover is who plays it.
    let next_castle = Condition::Move({
        let mut m = mc(WhichMove::Next);
        m.by = Who::Mover;
        m.castling = Some(CastleKind::Any);
        m
    });
    assert_eq!(ev(&next_castle, &l, 10), Tri::True);
    assert_eq!(ev(&next_castle, &l, 11), Tri::True);
    assert_eq!(ev(&next_castle, &l, 9), Tri::False);
    // The last position has no next move (rule 5); the first no previous one.
    let any_next = Condition::Move(mc(WhichMove::Next));
    let any_prev = Condition::Move(mc(WhichMove::Previous));
    assert_eq!(ev(&any_next, &l, l.frames.len() - 1), Tri::False);
    assert_eq!(ev(&any_next, &l, 0), Tri::True);
    assert_eq!(ev(&any_prev, &l, 0), Tri::False);
    assert_eq!(ev(&any_prev, &l, 1), Tri::True);
}

#[test]
fn promotion_moves_are_told_apart_and_subpromotion_works() {
    let fen = "1r3k2/P6P/8/8/8/8/8/4K3 w - - 0 1";
    let promo = |list: &[RoleName]| {
        Condition::Move({
            let mut m = mc(WhichMove::Previous);
            m.promotion = list.to_vec();
            m
        })
    };
    let under = [Rook, Bishop, Knight];
    for (san, queen, sub) in [
        ("a8=Q+", true, false),
        ("a8=R+", false, true),
        ("a8=B", false, true),
        ("a8=N", false, true),
        ("axb8=N", false, true),
        ("axb8=Q+", true, false),
        ("h8=Q+", true, false),
        ("h8=R", false, true),
    ] {
        let l = line(Some(fen), san, GameResult::Unknown);
        assert_eq!(ev(&promo(&[Queen]), &l, 1), Tri::from_bool(queen), "{san}");
        assert_eq!(ev(&promo(&under), &l, 1), Tri::from_bool(sub), "{san}");
        // An empty list does not restrict (any move matches).
        assert_eq!(ev(&promo(&[]), &l, 1), Tri::True, "{san}");
    }
    // A non-promotion move never matches a promotion list.
    let l = line(Some(fen), "Kd2", GameResult::Unknown);
    assert_eq!(ev(&promo(&[Queen]), &l, 1), Tri::False);
    assert_eq!(ev(&promo(&under), &l, 1), Tri::False);
}

#[test]
fn chess960_and_non_standard_start_positions_are_evaluated() {
    // Rule 10: only the pieces matter. A Chess960 start with the king on g1.
    let fen = "bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w HFhf - 0 1";
    let pos = Position::from_fen(fen).expect("960 fen");
    let g = Game::from_position(pos);
    let l = Line::from_game(&g, MAX_FRAMES);
    assert_eq!(l.frames.len(), 1);
    assert_eq!(
        ev(
            &piece(Who::White, &[King], squares(&["g1"]), 1, None),
            &l,
            0
        ),
        Tri::True
    );
    assert_eq!(
        ev(
            &piece(Who::Black, &[Queen], squares(&["c8"]), 1, None),
            &l,
            0
        ),
        Tri::True
    );
    // A castling move in a 960 game: king g1 -> g1 is a castle in Shredder
    // notation only after the squares between are free.
    let free = Position::from_fen("k7/8/8/8/8/8/8/1K5R w H - 0 1").expect("fen");
    let mut gm = Game::from_position(free.clone());
    let castle = free
        .legal_moves()
        .into_iter()
        .find(|m| m.castling_side().is_some())
        .expect("a castling move exists");
    let root_id = gm.root();
    gm.add_move(root_id, castle).expect("legal");
    let cl = Line::from_game(&gm, MAX_FRAMES);
    let c = Condition::Move({
        let mut m = mc(WhichMove::Previous);
        m.castling = Some(CastleKind::Short);
        m
    });
    assert_eq!(ev(&c, &cl, 1), Tri::True);
}

// ----------------------------------------------------------- relations

fn relation(r: RelationCond) -> Condition {
    Condition::Relation(r)
}

#[test]
fn attacks_gain_modes_and_the_king_as_a_target() {
    let attacks = |gain, target: &[RoleName]| {
        let mut r = rel(RelKind::Attacks, Who::White, &[Rook], target);
        r.gain = gain;
        relation(r)
    };
    let loose = "4k3/8/8/3p4/8/8/3R4/4K3 w - - 0 1";
    let guarded = "4k3/8/2p5/3p4/8/8/3R4/4K3 w - - 0 1";
    for gain in [Gain::Any, Gain::Undefended, Gain::WinsMaterial] {
        assert_eq!(at(&attacks(gain, &[Pawn]), loose), Tri::True, "{gain:?}");
    }
    assert_eq!(at(&attacks(Gain::Any, &[Pawn]), guarded), Tri::True);
    assert_eq!(at(&attacks(Gain::Undefended, &[Pawn]), guarded), Tri::False);
    assert_eq!(
        at(&attacks(Gain::WinsMaterial, &[Pawn]), guarded),
        Tri::False
    );
    // Wrong target role.
    assert_eq!(at(&attacks(Gain::Any, &[Queen]), loose), Tri::False);
    // Rule 13: no roles includes the king (a check)...
    let check = "4k3/8/8/8/8/8/8/4RK2 b - - 0 1";
    assert_eq!(at(&attacks(Gain::Any, &[]), check), Tri::True);
    assert_eq!(at(&attacks(Gain::Any, &[King]), check), Tri::True);
    assert_eq!(at(&attacks(Gain::Any, &[Pawn]), check), Tri::False);
    // ... but `undefended` never applies to the king (D3299 item 7).
    assert_eq!(at(&attacks(Gain::Undefended, &[King]), check), Tri::False);
    // `wins_material` uses `worth`: attacking the king always counts.
    assert_eq!(at(&attacks(Gain::WinsMaterial, &[King]), check), Tri::True);
}

#[test]
fn relation_actor_and_target_regions_and_sides() {
    let fen = "4k3/8/8/3p4/8/8/3R4/4K3 w - - 0 1";
    let mk = |actor_region: Region, target_region: Region, side: Who| {
        let mut r = rel(RelKind::Attacks, side, &[Rook], &[Pawn]);
        r.actor.region = actor_region;
        r.target.region = target_region;
        relation(r)
    };
    assert_eq!(
        at(&mk(squares(&["d2"]), squares(&["d5"]), Who::White), fen),
        Tri::True
    );
    assert_eq!(
        at(&mk(squares(&["a1"]), Region::Anywhere, Who::White), fen),
        Tri::False
    );
    assert_eq!(
        at(&mk(Region::Anywhere, squares(&["d6"]), Who::White), fen),
        Tri::False
    );
    assert_eq!(
        at(&mk(Region::Anywhere, Region::Anywhere, Who::Black), fen),
        Tri::False
    );
    assert_eq!(
        at(&mk(Region::Anywhere, Region::Anywhere, Who::Mover), fen),
        Tri::True
    );
    assert_eq!(
        at(&mk(Region::Anywhere, Region::Anywhere, Who::Opponent), fen),
        Tri::False
    );
    assert_eq!(
        at(&mk(Region::Anywhere, Region::Anywhere, Who::Any), fen),
        Tri::True
    );
    // Own-rank regions are resolved for the actor's colour.
    assert_eq!(
        at(&mk(Region::OwnRank(2), Region::Anywhere, Who::White), fen),
        Tri::True
    );
    assert_eq!(
        at(&mk(Region::OwnRank(7), Region::Anywhere, Who::White), fen),
        Tri::False
    );
}

#[test]
fn defends_is_between_pieces_of_the_same_colour() {
    let d = |actor: &[RoleName], target: &[RoleName], side| {
        relation(rel(RelKind::Defends, side, actor, target))
    };
    let fen = "4k3/8/8/8/8/8/3B4/3RK3 w - - 0 1";
    assert_eq!(at(&d(&[Rook], &[Bishop], Who::White), fen), Tri::True);
    assert_eq!(at(&d(&[Rook], &[Pawn], Who::White), fen), Tri::False);
    assert_eq!(at(&d(&[Bishop], &[Rook], Who::White), fen), Tri::False);
    // The king defends the bishop and the rook too.
    assert_eq!(at(&d(&[King], &[Rook, Bishop], Who::White), fen), Tri::True);
    // Defending is not attacking an enemy.
    assert_eq!(at(&d(&[Rook], &[Bishop], Who::Black), fen), Tri::False);
    // The opponent's pieces defend each other with side = Black.
    assert_eq!(
        at(
            &d(&[King], &[Pawn], Who::Black),
            "4k3/4p3/8/8/8/8/8/4K3 w - - 0 1"
        ),
        Tri::True
    );
}

#[test]
fn pins_absolute_relative_and_behind_filter() {
    let pins = |behind: Option<&[RoleName]>| {
        let mut r = rel(RelKind::Pins, Who::White, &[Rook, Bishop, Queen], &[]);
        r.behind = behind.map(|roles| TargetSel {
            roles: roles.to_vec(),
            region: Region::Anywhere,
        });
        relation(r)
    };
    let absolute = "4k3/8/8/4n3/8/8/8/4RK2 w - - 0 1";
    let relative = "4q1k1/8/8/8/4n3/8/8/4RK2 w - - 0 1";
    // Without `behind` either form counts.
    assert_eq!(at(&pins(None), absolute), Tri::True);
    assert_eq!(at(&pins(None), relative), Tri::True);
    // `roles = ["king"]` forces the absolute pin.
    assert_eq!(at(&pins(Some(&[King])), absolute), Tri::True);
    assert_eq!(at(&pins(Some(&[King])), relative), Tri::False);
    assert_eq!(at(&pins(Some(&[Queen])), relative), Tri::True);
    assert_eq!(at(&pins(Some(&[Queen])), absolute), Tri::False);
    // The pinner must satisfy the actor selection: a knight cannot pin.
    let mut r = rel(RelKind::Pins, Who::White, &[Knight], &[]);
    r.behind = None;
    assert_eq!(at(&relation(r), absolute), Tri::False);
    // Pinned piece roles.
    let mut r = rel(RelKind::Pins, Who::White, &[], &[Pawn]);
    r.behind = None;
    assert_eq!(at(&relation(r), absolute), Tri::False);
    // Pinner's colour: Black does not pin anything here.
    let mut r = rel(RelKind::Pins, Who::Black, &[], &[]);
    r.behind = None;
    assert_eq!(at(&relation(r), absolute), Tri::False);
}

#[test]
fn skewers_condition() {
    let mut r = rel(RelKind::Skewers, Who::White, &[Rook], &[King]);
    r.behind = Some(TargetSel {
        roles: vec![Queen],
        region: Region::Anywhere,
    });
    let c = relation(r);
    assert_eq!(at(&c, "R3k2q/8/8/8/8/8/8/4K3 b - - 0 1"), Tri::True);
    assert_eq!(at(&c, "R3k3/8/8/8/8/8/8/4K3 b - - 0 1"), Tri::False);
}

#[test]
fn forks_min_targets_when_and_new_this_move() {
    let fork = |min: u8, when: When, new: bool, side: Who| {
        let mut r = rel(RelKind::Forks, side, &[Knight], &[]);
        r.min_targets = min;
        r.when = when;
        r.new_this_move = new;
        relation(r)
    };
    // Black to move: the knight on c7 forks the king, Ra8 and Rd5.
    let direct = fen_line("r3k3/2N5/8/3r4/8/8/8/7K b - - 0 1");
    assert_eq!(
        ev(&fork(2, When::Any, false, Who::White), &direct, 0),
        Tri::True
    );
    assert_eq!(
        ev(&fork(3, When::Any, false, Who::White), &direct, 0),
        Tri::True
    );
    assert_eq!(
        ev(&fork(4, When::Any, false, Who::White), &direct, 0),
        Tri::False
    );
    // Black to move, so the white fork is "standing".
    assert_eq!(
        ev(
            &fork(2, When::ActorJustMoved, false, Who::White),
            &direct,
            0
        ),
        Tri::True
    );
    assert_eq!(
        ev(&fork(2, When::ActorToMove, false, Who::White), &direct, 0),
        Tri::False
    );
    // Black has no knights at all.
    assert_eq!(
        ev(&fork(2, When::Any, false, Who::Black), &direct, 0),
        Tri::False
    );
    // Who::Opponent is the side that just moved; Who::Mover is not.
    assert_eq!(
        ev(&fork(2, When::Any, false, Who::Opponent), &direct, 0),
        Tri::True
    );
    assert_eq!(
        ev(&fork(2, When::Any, false, Who::Mover), &direct, 0),
        Tri::False
    );
    // new_this_move is false at p = 0 (rule 5) even though the fork stands.
    assert_eq!(
        ev(&fork(2, When::Any, true, Who::White), &direct, 0),
        Tri::False
    );
    // The move that creates the fork: Nd5-c7+. It is new at ply 1 only.
    let l2 = line(
        Some("r3k3/8/8/3N4/8/8/3p4/7K w - - 0 1"),
        "Nc7+ Kd7 Nxa8",
        GameResult::Unknown,
    );
    let new = fork(2, When::ActorJustMoved, true, Who::White);
    assert_eq!(ev(&new, &l2, 0), Tri::False);
    assert_eq!(ev(&new, &l2, 1), Tri::True);
    assert_eq!(ev(&new, &l2, 2), Tri::False);
    let old = fork(2, When::Any, false, Who::White);
    assert_eq!(ev(&old, &l2, 1), Tri::True);
    // After ...Kd7 the king attacks the knight: no longer a fork.
    assert_eq!(ev(&old, &l2, 2), Tri::False);
}

#[test]
fn pin_new_this_move_is_true_only_where_the_move_created_it() {
    let l = line(
        Some("4k3/p7/8/4n3/8/8/8/R5K1 w - - 0 1"),
        "Re1 a6 Kf2",
        GameResult::Unknown,
    );
    let pin = |new: bool| {
        let mut r = rel(RelKind::Pins, Who::White, &[Rook], &[]);
        r.behind = Some(TargetSel {
            roles: vec![King],
            region: Region::Anywhere,
        });
        r.new_this_move = new;
        relation(r)
    };
    assert_eq!(ev(&pin(false), &l, 0), Tri::False);
    assert_eq!(ev(&pin(false), &l, 1), Tri::True);
    assert_eq!(ev(&pin(false), &l, 2), Tri::True);
    assert_eq!(ev(&pin(false), &l, 3), Tri::True);
    assert_eq!(ev(&pin(true), &l, 0), Tri::False);
    assert_eq!(ev(&pin(true), &l, 1), Tri::True);
    assert_eq!(ev(&pin(true), &l, 2), Tri::False);
    assert_eq!(ev(&pin(true), &l, 3), Tri::False);
    // The same pin already standing at p = 0 is a pin but not a new one.
    let standing = fen_line("4k3/8/8/4n3/8/8/8/4RK2 w - - 0 1");
    assert_eq!(ev(&pin(false), &standing, 0), Tri::True);
    assert_eq!(ev(&pin(true), &standing, 0), Tri::False);
}

#[test]
fn discovered_and_double_attack_conditions() {
    let disc = |discovery, by, roles: &[RoleName]| {
        Condition::Discovered(DiscoveredCond {
            discovery,
            by,
            roles: roles.to_vec(),
        })
    };
    let l = line(
        Some("4k3/7p/8/8/4B3/8/8/4RK2 w - - 0 1"),
        "Bxh7 Kd7",
        GameResult::Unknown,
    );
    // Rule 5: nothing "discovered" before any move.
    assert_eq!(
        ev(&disc(DiscoveredKind::Check, Who::White, &[]), &l, 0),
        Tri::False
    );
    assert_eq!(
        ev(&disc(DiscoveredKind::Check, Who::White, &[]), &l, 1),
        Tri::True
    );
    assert_eq!(
        ev(&disc(DiscoveredKind::Attack, Who::White, &[]), &l, 1),
        Tri::True
    );
    assert_eq!(
        ev(&disc(DiscoveredKind::DoubleCheck, Who::White, &[]), &l, 1),
        Tri::False
    );
    // `roles` filters the piece that moved.
    assert_eq!(
        ev(&disc(DiscoveredKind::Check, Who::White, &[Bishop]), &l, 1),
        Tri::True
    );
    assert_eq!(
        ev(&disc(DiscoveredKind::Check, Who::White, &[Rook]), &l, 1),
        Tri::False
    );
    // Who just moved is the opponent of the side to move.
    assert_eq!(
        ev(&disc(DiscoveredKind::Check, Who::Opponent, &[]), &l, 1),
        Tri::True
    );
    assert_eq!(
        ev(&disc(DiscoveredKind::Check, Who::Mover, &[]), &l, 1),
        Tri::False
    );
    assert_eq!(
        ev(&disc(DiscoveredKind::Check, Who::Black, &[]), &l, 1),
        Tri::False
    );
    assert_eq!(
        ev(&disc(DiscoveredKind::Check, Who::Any, &[]), &l, 1),
        Tri::True
    );
    // It does not last: after ...Kd7 there is no move that discovered anything.
    assert_eq!(
        ev(&disc(DiscoveredKind::Check, Who::Any, &[]), &l, 2),
        Tri::False
    );
    // Double check by Bc6+ with the rook behind.
    let d = line(
        Some("4k3/8/8/8/4B3/8/8/4RK2 w - - 0 1"),
        "Bc6+",
        GameResult::Unknown,
    );
    assert_eq!(
        ev(&disc(DiscoveredKind::DoubleCheck, Who::White, &[]), &d, 1),
        Tri::True
    );
    assert_eq!(
        ev(&disc(DiscoveredKind::DoubleCheck, Who::White, &[]), &d, 0),
        Tri::False
    );
}

#[test]
fn double_attack_condition_follows_the_last_move() {
    let da = |by| Condition::DoubleAttack(DoubleAttackCond { by });
    let l = line(
        Some("r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1"),
        "Nc7+ Kd7",
        GameResult::Unknown,
    );
    assert_eq!(ev(&da(Who::White), &l, 0), Tri::False);
    assert_eq!(ev(&da(Who::White), &l, 1), Tri::True);
    assert_eq!(ev(&da(Who::Opponent), &l, 1), Tri::True);
    assert_eq!(ev(&da(Who::Mover), &l, 1), Tri::False);
    assert_eq!(ev(&da(Who::Black), &l, 1), Tri::False);
    assert_eq!(ev(&da(Who::Any), &l, 1), Tri::True);
    // After the king move the threat on the rook is not new.
    assert_eq!(ev(&da(Who::Any), &l, 2), Tri::False);
}

// ------------------------------------------------------------ sacrifice

/// White to move. After a quiet move pair, the knight goes to d5 with
/// check, the bishop takes it and the queen takes back. (Rule 5: at `p = 0`
/// a sacrifice is false, so the interesting position is `k = 2`.)
const KNIGHT_TRADE_FEN: &str = "4k3/5b2/8/8/8/2N5/8/3QK3 w - - 0 1";
const KNIGHT_TRADE: &str = "Ke2 Ke7 Nd5+ Bxd5 Qxd5 Kf8";

fn sacrifice(by: Who, roles: &[RoleName], min_net: u8, outcome: Outcome, within: u8) -> Pattern {
    let mut p = root(Condition::Sacrifice(SacrificeCond {
        by,
        roles: roles.to_vec(),
        min_net,
        outcome,
        within,
    }));
    p.scope.min_move = 0;
    p
}

#[test]
fn sacrifice_is_false_at_the_initial_position_rule_5() {
    let l = line(
        Some(KNIGHT_TRADE_FEN),
        "Nd5+ Bxd5 Qxd5 Kf8",
        GameResult::Unknown,
    );
    assert_eq!(
        first(&sacrifice(Who::White, &[], 1, Outcome::Any, 3), &l),
        None
    );
}

#[test]
fn sacrifice_recovered_any_and_the_window() {
    let l = line(Some(KNIGHT_TRADE_FEN), KNIGHT_TRADE, GameResult::Unknown);
    let ok = |p: Pattern| first(&p, &l);
    assert_eq!(
        ok(sacrifice(Who::White, &[], 2, Outcome::Recovered, 1)),
        Some(2)
    );
    assert_eq!(
        ok(sacrifice(Who::White, &[Knight], 3, Outcome::Recovered, 1)),
        Some(2)
    );
    assert_eq!(
        ok(sacrifice(Who::White, &[], 2, Outcome::Either, 1)),
        Some(2)
    );
    assert_eq!(ok(sacrifice(Who::White, &[], 2, Outcome::Any, 1)), Some(2));
    assert_eq!(ok(sacrifice(Who::Any, &[], 2, Outcome::Any, 1)), Some(2));
    assert_eq!(ok(sacrifice(Who::Mover, &[], 2, Outcome::Any, 1)), Some(2));
    // The delivered piece must have the asked role; the loss must reach min_net.
    assert_eq!(ok(sacrifice(Who::White, &[Rook], 2, Outcome::Any, 1)), None);
    assert_eq!(
        ok(sacrifice(Who::White, &[Queen, Rook], 2, Outcome::Any, 1)),
        None
    );
    assert_eq!(ok(sacrifice(Who::White, &[], 4, Outcome::Any, 1)), None);
    // It is not a mate, and Black did not sacrifice anything.
    assert_eq!(ok(sacrifice(Who::White, &[], 2, Outcome::Mate, 3)), None);
    assert_eq!(ok(sacrifice(Who::Black, &[], 1, Outcome::Any, 3)), None);
    assert_eq!(ok(sacrifice(Who::Opponent, &[], 1, Outcome::Any, 3)), None);
}

#[test]
fn sacrifice_needs_the_capture_and_enough_line() {
    // Rule 5: the line ends before the window is complete.
    let short = line(
        Some(KNIGHT_TRADE_FEN),
        "Ke2 Ke7 Nd5+ Bxd5 Qxd5",
        GameResult::Unknown,
    );
    assert_eq!(
        first(
            &sacrifice(Who::White, &[], 2, Outcome::Recovered, 1),
            &short
        ),
        None
    );
    // ... but the plain delivery still counts when no outcome is asked.
    assert_eq!(
        first(&sacrifice(Who::White, &[], 2, Outcome::Any, 1), &short),
        Some(2)
    );
    // No capture by the opponent: nothing is delivered.
    let quiet = line(
        Some(KNIGHT_TRADE_FEN),
        "Ke2 Ke7 Nb5 Kf8 Nc3 Ke7",
        GameResult::Unknown,
    );
    assert_eq!(
        first(&sacrifice(Who::White, &[], 1, Outcome::Any, 3), &quiet),
        None
    );
    // Delivered at the very last positions: no P_{k+2}.
    let cut = line(Some(KNIGHT_TRADE_FEN), "Ke2 Ke7 Nd5+", GameResult::Unknown);
    assert_eq!(
        first(&sacrifice(Who::White, &[], 1, Outcome::Any, 1), &cut),
        None
    );
    // The material never comes back: recovered is false but any is true.
    let lost = line(
        Some(KNIGHT_TRADE_FEN),
        "Ke2 Ke7 Nd5+ Bxd5 Kf2 Kd6",
        GameResult::Unknown,
    );
    assert_eq!(
        first(&sacrifice(Who::White, &[], 2, Outcome::Recovered, 3), &lost),
        None
    );
    assert_eq!(
        first(&sacrifice(Who::White, &[], 2, Outcome::Any, 3), &lost),
        Some(2)
    );
    // A capture answered by an equal recapture is no sacrifice (loss 0).
    let equal = line(
        Some("4k3/8/4p3/3b4/8/2N5/8/4K3 w - - 0 1"),
        "Kd2 Kd7 Nxd5 exd5 Ke3 Ke7",
        GameResult::Unknown,
    );
    assert_eq!(
        first(&sacrifice(Who::White, &[], 1, Outcome::Any, 1), &equal),
        None
    );
}

#[test]
fn sacrifice_counts_the_promoted_piece_rule_14() {
    // a8=Q Rxa8: B goes -4 -> +4 -> -5, so measured at the position before
    // the promotion the net loss is 1 (the pawn); the delivered piece is the
    // new queen.
    let l = line(
        Some("1r5k/P7/8/8/8/8/8/4K3 w - - 0 1"),
        "Kd2 Kg7 a8=Q Rxa8",
        GameResult::Unknown,
    );
    let balances: Vec<i16> = l.frames.iter().map(|f| f.balance).collect();
    assert_eq!(balances, vec![-4, -4, -4, 4, -5]);
    assert_eq!(
        first(&sacrifice(Who::White, &[Queen], 1, Outcome::Any, 1), &l),
        Some(2)
    );
    assert_eq!(
        first(&sacrifice(Who::White, &[Pawn], 1, Outcome::Any, 1), &l),
        None
    );
    assert_eq!(
        first(&sacrifice(Who::White, &[], 2, Outcome::Any, 1), &l),
        None
    );
}

// ---------------------------------------------------------------- mate

#[test]
fn mate_conditions_follow_rule_15_and_by() {
    let scholar = line(None, "e4 e5 Bc4 Nc6 Qh5 Nf6 Qxf7#", GameResult::White);
    let cond = |by, mode| mate(by, 1, 1, mode, true, 20_000);
    let plies = |c: Condition| {
        let mut p = root(c);
        p.scope.min_move = 0;
        matching_plies(&p, &scholar, None)
    };
    // The position before Qxf7# is ply 6 with White to move.
    assert_eq!(plies(cond(Who::White, MateMode::Played)), vec![6]);
    assert_eq!(plies(cond(Who::White, MateMode::Available)), vec![6]);
    assert_eq!(plies(cond(Who::Mover, MateMode::Played)), vec![6]);
    assert_eq!(plies(cond(Who::Any, MateMode::Played)), vec![6]);
    assert_eq!(plies(cond(Who::Black, MateMode::Played)), Vec::<u16>::new());
    // Played mode in a game that does not end in mate is false.
    let cut = line(None, "e4 e5 Bc4 Nc6 Qh5 Nf6", GameResult::Unknown);
    let mut p = root(cond(Who::White, MateMode::Played));
    p.scope.min_move = 0;
    assert_eq!(first(&p, &cut), None);
    // The final position is already mate: not a mate "available" for anyone.
    let end = Condition::Status {
        status: Status::Checkmate,
    };
    assert_eq!(ev(&end, &scholar, 7), Tri::True);
    assert_eq!(
        ev(&cond(Who::White, MateMode::Available), &scholar, 7),
        Tri::False
    );
    assert_eq!(
        ev(&cond(Who::Black, MateMode::Available), &scholar, 7),
        Tri::False
    );
    assert_eq!(
        ev(&cond(Who::Any, MateMode::Available), &scholar, 7),
        Tri::False
    );
    // `by = black` works for the fool's mate.
    let fool = line(None, "f3 e5 g4 Qh4#", GameResult::Black);
    let mut q = root(cond(Who::Black, MateMode::Played));
    q.scope.min_move = 0;
    assert_eq!(first(&q, &fool), Some(3));
    q.root = cond(Who::White, MateMode::Played);
    assert_eq!(first(&q, &fool), None);
}

#[test]
fn played_mate_counts_the_distance_of_the_game_not_the_shortest_possible() {
    // Mate in 2 on the board (Ra7 Kg8 Rb8#), played in 3 plies.
    let l = line(
        Some("7k/8/8/8/8/8/R7/1R4K1 w - - 0 1"),
        "Ra7 Kg8 Rb8#",
        GameResult::White,
    );
    let two = |mode, min, max, forcing| mate(Who::White, min, max, mode, forcing, 100_000);
    let mut p = root(two(MateMode::Played, 2, 2, true));
    p.scope.min_move = 0;
    assert_eq!(matching_plies(&p, &l, None), vec![0]);
    // Range 1..=1 does not accept a mate in 2.
    // (Ply 2 still has the mate in 1 of the game itself.)
    p.root = two(MateMode::Played, 1, 1, true);
    assert_eq!(matching_plies(&p, &l, None), vec![2]);
    // Available with forcing_only skips the quiet first move; ply 2 has the
    // shortest mate in 1, outside 2..=2.
    p.root = two(MateMode::Available, 2, 2, true);
    assert_eq!(matching_plies(&p, &l, None), Vec::<u16>::new());
    p.root = two(MateMode::Available, 1, 3, true);
    assert_eq!(matching_plies(&p, &l, None), vec![2]);
    p.root = two(MateMode::Available, 2, 2, false);
    assert_eq!(matching_plies(&p, &l, None), vec![0]);
    // The range is about the shortest mate: 2..=3 contains 2.
    p.root = two(MateMode::Available, 2, 3, false);
    assert_eq!(matching_plies(&p, &l, None), vec![0]);
    p.root = two(MateMode::Available, 1, 3, false);
    assert_eq!(matching_plies(&p, &l, None), vec![0, 2]);
}

// ---------------------------------------------------------- highlights

fn squares_of(h: &[Highlight]) -> Vec<Square> {
    let mut out = Vec::new();
    for x in h {
        match x.kind {
            HighlightKind::Arrow(a, b) => {
                out.push(a);
                out.push(b);
            }
            HighlightKind::Square(s) => out.push(s),
        }
    }
    out.sort_by_key(|s| u32::from(*s));
    out.dedup();
    out
}

#[test]
fn highlights_of_a_pin_name_the_actor_the_pinned_piece_and_the_king() {
    let l = fen_line("4k3/8/8/4n3/8/8/8/4RK2 w - - 0 1");
    let mut r = rel(RelKind::Pins, Who::White, &[Rook], &[]);
    r.behind = Some(TargetSel {
        roles: vec![King],
        region: Region::Anywhere,
    });
    let p = root(relation(r));
    let h = highlights_at(&p, &l, 0);
    let sqs = squares_of(&h);
    for s in ["e1", "e5", "e8"] {
        assert!(sqs.contains(&square(s)), "{s} missing in {h:?}");
    }
    assert!(h.iter().any(|x| x.role == HighlightRole::Actor));
    assert!(h.iter().any(|x| x.role == HighlightRole::Behind));
}

#[test]
fn highlights_are_empty_where_the_pattern_does_not_hold() {
    let l = line(
        Some("r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1"),
        "Nc7+",
        GameResult::Unknown,
    );
    let mut r = rel(RelKind::Forks, Who::White, &[Knight], &[]);
    r.when = When::ActorJustMoved;
    r.new_this_move = true;
    let p = root(relation(r));
    assert_eq!(highlights_at(&p, &l, 0), Vec::new());
    assert_ne!(highlights_at(&p, &l, 1), Vec::new());
    let sqs = squares_of(&highlights_at(&p, &l, 1));
    for s in ["c7", "a8", "e8"] {
        assert!(sqs.contains(&square(s)), "{s} in {sqs:?}");
    }
    // Out of the line: nothing, and no panic.
    assert_eq!(highlights_at(&p, &l, 99), Vec::new());
    assert_eq!(highlights_at(&p, &l, usize::MAX), Vec::new());
}

#[test]
fn highlights_all_unions_any_takes_the_first_true_and_not_adds_nothing() {
    let l = fen_line("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
    let wk = piece(Who::White, &[King], squares(&["e1"]), 1, None);
    let bk = piece(Who::Black, &[King], squares(&["e8"]), 1, None);
    let hl = |c: Condition| squares_of(&highlights_at(&root(c), &l, 0));
    let a = hl(all_of(vec![wk.clone(), bk.clone()]));
    assert!(
        a.contains(&square("e1")) && a.contains(&square("e8")),
        "{a:?}"
    );
    let only_first = hl(any_of(vec![wk.clone(), bk.clone()]));
    assert!(only_first.contains(&square("e1")), "{only_first:?}");
    assert!(!only_first.contains(&square("e8")), "{only_first:?}");
    // The first child is false: the second one explains.
    let no_wq_on_e1 = piece(Who::White, &[Queen], squares(&["e1"]), 1, None);
    let second = hl(any_of(vec![no_wq_on_e1, bk]));
    assert!(
        second.contains(&square("e8")) && !second.contains(&square("e1")),
        "{second:?}"
    );
    // Not produces nothing, even when it holds.
    assert_eq!(hl(not(f())), Vec::new());
    assert_eq!(
        hl(not(piece(Who::White, &[Queen], squares(&["h5"]), 1, None))),
        Vec::new()
    );
    // A pattern that does not hold explains nothing.
    assert_eq!(hl(all_of(vec![wk, f()])), Vec::new());
}

#[test]
fn highlights_with_ignore_colors_come_back_in_the_original_orientation() {
    // Black knight forks on c2 (Nc2+ forking Ke1 and Ra1): the white-fork
    // pattern only holds in the mirrored line; squares are mirrored back.
    let l = line(
        Some("4k3/8/8/8/3n4/8/8/R3K3 b - - 0 1"),
        "Nc2+",
        GameResult::Unknown,
    );
    let mut r = rel(RelKind::Forks, Who::White, &[Knight], &[]);
    r.when = When::ActorJustMoved;
    r.new_this_move = true;
    let mut p = root(relation(r));
    assert_eq!(highlights_at(&p, &l, 1), Vec::new());
    p.scope.ignore_colors = true;
    let sqs = squares_of(&highlights_at(&p, &l, 1));
    for s in ["c2", "a1", "e1"] {
        assert!(sqs.contains(&square(s)), "{s} in {sqs:?}");
    }
    assert!(!sqs.contains(&square("c7")), "{sqs:?}");
}

// ------------------------------------------------------------- regions

fn bits(coords: impl Fn(u32, u32) -> bool) -> u64 {
    let mut m = 0u64;
    for file in 0..8u32 {
        for rank in 0..8u32 {
            if coords(file, rank) {
                m |= 1u64 << (rank * 8 + file);
            }
        }
    }
    m
}

#[rstest]
#[case::center(NamedRegion::Center, bits(|f, r| (f == 3 || f == 4) && (r == 3 || r == 4)))]
#[case::extended_center(NamedRegion::ExtendedCenter, bits(|f, r| (2..=5).contains(&f) && (2..=5).contains(&r)))]
#[case::queen_side(NamedRegion::QueenSide, bits(|f, _| f <= 3))]
#[case::king_side(NamedRegion::KingSide, bits(|f, _| f >= 4))]
#[case::white_half(NamedRegion::WhiteHalf, bits(|_, r| r <= 3))]
#[case::black_half(NamedRegion::BlackHalf, bits(|_, r| r >= 4))]
#[case::dark(NamedRegion::DarkSquares, bits(|f, r| (f + r) % 2 == 0))]
#[case::light(NamedRegion::LightSquares, bits(|f, r| (f + r) % 2 == 1))]
#[case::edge(NamedRegion::Edge, bits(|f, r| f == 0 || f == 7 || r == 0 || r == 7))]
#[case::long_diagonals(NamedRegion::LongDiagonals, bits(|f, r| f == r || f + r == 7))]
fn named_regions_have_the_documented_squares(#[case] named: NamedRegion, #[case] expected: u64) {
    for owner in [Color::White, Color::Black] {
        assert_eq!(
            region_mask(&Region::Named(named), owner),
            expected,
            "{named:?} {owner:?}"
        );
    }
}

#[test]
fn simple_regions_and_own_ranks() {
    assert_eq!(region_mask(&Region::Anywhere, Color::White), u64::MAX);
    assert_eq!(
        region_mask(&Region::File('d'), Color::Black),
        bits(|f, _| f == 3)
    );
    assert_eq!(
        region_mask(&Region::Rank(7), Color::White),
        bits(|_, r| r == 6)
    );
    // `own_rank k` is rank k for White and rank 9-k for Black.
    for k in 1..=8u8 {
        assert_eq!(
            region_mask(&Region::OwnRank(k), Color::White),
            bits(|_, r| r == u32::from(k) - 1)
        );
        assert_eq!(
            region_mask(&Region::OwnRank(k), Color::Black),
            bits(|_, r| r == 8 - u32::from(k))
        );
    }
    // Squares: a set, duplicates allowed.
    let e4 = 1u64 << u32::from(square("e4"));
    let d4 = 1u64 << u32::from(square("d4"));
    assert_eq!(
        region_mask(&squares(&["e4", "d4", "e4"]), Color::White),
        e4 | d4
    );
    // Rect: normalised whatever the corner order.
    let expected = bits(|f, r| (2..=5).contains(&f) && (2..=5).contains(&r));
    for (a, b) in [("c3", "f6"), ("f6", "c3"), ("c6", "f3"), ("f3", "c6")] {
        assert_eq!(
            region_mask(
                &Region::Rect {
                    from: sq(a),
                    to: sq(b)
                },
                Color::White
            ),
            expected,
            "{a}-{b}"
        );
    }
    assert_eq!(
        region_mask(
            &Region::Rect {
                from: sq("e4"),
                to: sq("e4")
            },
            Color::White
        ),
        e4
    );
    // a1 is a dark square.
    assert_eq!(
        region_mask(&Region::Named(NamedRegion::DarkSquares), Color::White) & 1,
        1
    );
}

// --------------------------------------------------------- determinism

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn evaluation_is_pure_and_never_panics_on_random_games(
        choices in prop::collection::vec(0usize..64, 0..50),
        min_move in 0u16..6,
        hold in 1u8..4,
        ignore_colors in any::<bool>(),
    ) {
        let mut g = Game::new();
        let mut node = g.root();
        for c in choices {
            let pos = g.position_at(node).expect("node").clone();
            let moves = pos.legal_moves();
            if moves.is_empty() { break; }
            node = g.add_move(node, moves[c % moves.len()]).expect("legal");
        }
        let l = Line::from_game(&g, MAX_FRAMES);
        let conds = vec![
            t(),
            piece(Who::Any, &[Queen], Region::Anywhere, 2, None),
            Condition::Move(mc(WhichMove::Previous)),
            relation(rel(RelKind::Attacks, Who::Any, &[], &[])),
            relation({ let mut r = rel(RelKind::Forks, Who::Any, &[], &[]); r.new_this_move = true; r }),
            Condition::DoubleAttack(DoubleAttackCond { by: Who::Any }),
            Condition::Discovered(DiscoveredCond { discovery: DiscoveredKind::Attack, by: Who::Any, roles: vec![] }),
            Condition::Sacrifice(SacrificeCond { by: Who::Any, roles: vec![], min_net: 2, outcome: Outcome::Either, within: 3 }),
            Condition::Feature(FeatureCond { feature: Feature::PassedPawn, side: Who::Any, region: Region::Anywhere, min: 1 }),
            mate(Who::Any, 1, 2, MateMode::Played, true, 2_000),
            mate(Who::Any, 1, 1, MateMode::Available, true, 2_000),
        ];
        for c in conds {
            let mut p = root(c);
            p.scope = Scope { min_move, max_move: 999, hold, end_only: false, ignore_colors };
            let a = match_line(&p, &l, None);
            let b = match_line(&p, &l, None);
            prop_assert_eq!(a, b);
            let all_runs = matching_plies(&p, &l, None);
            prop_assert_eq!(a.ply, all_runs.first().copied());
            prop_assert!(all_runs.windows(2).all(|w| w[0] < w[1]));
            for ply in all_runs.iter().take(2) {
                touch(highlights_at(&p, &l, usize::from(*ply)));
            }
        }
    }

    #[test]
    fn a_symmetric_or_mirrored_pattern_is_colour_blind(
        choices in prop::collection::vec(0usize..64, 0..40),
    ) {
        // For a pattern about "White's X" the ignore_colors scan equals the
        // union of the scan on the line and on its mirror (rule 9).
        let mut g = Game::new();
        let mut node = g.root();
        for c in choices {
            let pos = g.position_at(node).expect("node").clone();
            let moves = pos.legal_moves();
            if moves.is_empty() { break; }
            node = g.add_move(node, moves[c % moves.len()]).expect("legal");
        }
        let l = Line::from_game(&g, MAX_FRAMES);
        let mirror = l.mirrored().expect("mirror");
        let mut p = root(piece(Who::White, &[Queen], Region::Named(NamedRegion::BlackHalf), 1, None));
        p.scope.min_move = 0;
        let plain = matching_plies(&p, &l, None);
        let flipped = matching_plies(&p, &mirror, None);
        p.scope.ignore_colors = true;
        let both = matching_plies(&p, &l, None);
        if plain.is_empty() {
            prop_assert_eq!(both, flipped);
        } else {
            prop_assert_eq!(both, plain);
        }
    }
}

// ------------------------------------------------- robustness (rule 21)

fn zoo() -> Vec<Condition> {
    vec![
        t(),
        piece(Who::Any, &[], Region::Anywhere, 0, None),
        Condition::Move(mc(WhichMove::Next)),
        Condition::Move(mc(WhichMove::Previous)),
        Condition::Status {
            status: Status::Checkmate,
        },
        Condition::SideToMove { side: Side::Black },
        relation(rel(RelKind::Attacks, Who::Any, &[], &[])),
        relation({
            let mut r = rel(RelKind::Forks, Who::Any, &[], &[]);
            r.new_this_move = true;
            r
        }),
        relation({
            let mut r = rel(RelKind::Pins, Who::Any, &[], &[]);
            r.new_this_move = true;
            r
        }),
        Condition::DoubleAttack(DoubleAttackCond { by: Who::Any }),
        Condition::Discovered(DiscoveredCond {
            discovery: DiscoveredKind::Check,
            by: Who::Any,
            roles: vec![],
        }),
        Condition::Sacrifice(SacrificeCond {
            by: Who::Any,
            roles: vec![],
            min_net: 1,
            outcome: Outcome::Either,
            within: 8,
        }),
        mate(Who::Any, 1, 8, MateMode::Played, true, 1_000),
        mate(Who::Any, 1, 1, MateMode::Available, true, 1_000),
        Condition::Result {
            results: vec![ResultKind::Unknown],
        },
    ]
}

#[test]
fn positions_outside_the_line_never_panic_and_are_not_true() {
    let l = line(None, "e4 e5 Nf3", GameResult::Unknown);
    for c in zoo() {
        for i in [
            l.frames.len(),
            l.frames.len() + 1,
            10_000,
            usize::MAX - 1,
            usize::MAX,
        ] {
            let v = ev(&c, &l, i);
            // `not` of a condition is irrelevant: just no panic; and the
            // result constants do not depend on the position.
            if !matches!(c, Condition::Result { .. }) {
                assert_ne!(v, Tri::True, "{c:?} at {i}");
            }
            touch(highlights_at(&root(c.clone()), &l, i));
        }
    }
}

#[test]
fn every_condition_works_on_a_line_with_one_frame_and_on_the_last_frame() {
    let one = Line::from_game(&Game::new(), MAX_FRAMES);
    let l = line(None, "e4 e5 Nf3 Nc6", GameResult::Unknown);
    for c in zoo() {
        touch(ev(&c, &one, 0));
        touch(ev(&c, &l, l.frames.len() - 1));
        let mut p = root(c);
        p.scope.min_move = 0;
        touch(matching_plies(&p, &one, None));
        p.scope.end_only = true;
        touch(match_line(&p, &l, None));
        p.scope.ignore_colors = true;
        touch(match_line(&p, &l, None));
    }
}

#[test]
fn extreme_scope_values_do_not_overflow() {
    let l = line(None, "e4 e5", GameResult::Unknown);
    let mut p = root(t());
    p.scope.min_move = u16::MAX;
    p.scope.max_move = u16::MAX;
    assert_eq!(first(&p, &l), None);
    p.scope.min_move = 0;
    assert_eq!(first(&p, &l), Some(0));
    p.scope.hold = u8::MAX;
    assert_eq!(first(&p, &l), None);
    p.scope.hold = 0; // invalid, but evaluating must not misbehave
    touch(match_line(&p, &l, None));
}

#[test]
fn the_ply_of_a_late_match_is_exact_above_253() {
    // Rule 8: the filter byte saturates at slice b, but the ply is exact here.
    let g = shuffle(80, "e4");
    let l = Line::from_game(&g, MAX_FRAMES);
    assert_eq!(l.frames.len(), 322);
    let mut p = root(Condition::Move({
        let mut m = mc(WhichMove::Previous);
        m.to = squares(&["e4"]);
        m
    }));
    p.scope.max_move = 999;
    assert_eq!(first(&p, &l), Some(321));
    assert_eq!(matching_plies(&p, &l, None), vec![321]);
}

#[test]
fn only_the_main_line_is_examined() {
    let mut g = Game::new();
    let root_id = g.root();
    let pos = g.position_at(root_id).expect("root").clone();
    let e4 = pos.parse_san("e4", SanMode::Strict).expect("e4").mv;
    let d4 = pos.parse_san("d4", SanMode::Strict).expect("d4").mv;
    let main = g.add_move(root_id, e4).expect("e4");
    let var = g.add_move(root_id, d4).expect("d4 as a variation");
    assert!(g.is_variation(var).expect("node"));
    // A variation continues too.
    let after = g.position_at(var).expect("pos").clone();
    let d5 = after.parse_san("d5", SanMode::Strict).expect("d5").mv;
    g.add_move(var, d5).expect("d5");
    let l = Line::from_game(&g, MAX_FRAMES);
    assert_eq!(l.frames.len(), 2);
    assert_eq!(l.frames[1].mv, g.mv(main).ok());
    // A pattern about the variation's pawn never matches.
    let mut p = root(piece(Who::White, &[Pawn], squares(&["d4"]), 1, None));
    p.scope.min_move = 0;
    assert_eq!(first(&p, &l), None);
}

#[test]
fn result_tags_map_to_the_four_result_kinds() {
    for (r, k) in [
        (GameResult::White, ResultKind::WhiteWins),
        (GameResult::Black, ResultKind::BlackWins),
        (GameResult::Draw, ResultKind::Draw),
        (GameResult::Unknown, ResultKind::Unknown),
    ] {
        assert_eq!(line(None, "e4", r).result, k);
    }
}

#[test]
fn black_to_move_start_and_odd_fen_keep_the_ply_numbering_from_the_start_position() {
    // The plies are counted from the first position of the game, whatever
    // its side to move: ply 1 is the position after the first move.
    let l = line(
        Some("4k3/8/8/8/8/8/4P3/4K3 b - - 0 40"),
        "Kd7 e4 Ke6",
        GameResult::Unknown,
    );
    let mut p = root(Condition::SideToMove { side: Side::Black });
    p.scope.min_move = 0;
    // Black to move at plies 0 and 2.
    assert_eq!(matching_plies(&p, &l, None), vec![0, 2]);
    // The window counts plies: min_move = 1 starts at ply 1 even here.
    p.scope.min_move = 1;
    assert_eq!(matching_plies(&p, &l, None), vec![2]);
}
