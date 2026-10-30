//! M8-6a (spec D2302, D2304, D2311-D2316, D2334): the pure core of the
//! calculation exercise: reading the lines, the classes of the
//! evaluations, the good moves, the comparison and the score.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use mychess_app::calvar::{
    Assessment, CRUCIAL_PENALTY, Eval, LineProbes, MATE_CP, Probe, RootLine, ScoreParts, UserLine,
    compare, good_moves, read_moves, san_line, score_of,
};
use mychess_core::{Move, Position};
use mychess_engine::Score;

fn start() -> Position {
    Position::new()
}

fn mv(p: &Position, uci: &str) -> Move {
    p.parse_uci(uci).expect("uci")
}

fn root_line(p: &Position, n: u8, uci: &str, cp: i32) -> RootLine {
    RootLine {
        multipv: n,
        depth: 14,
        eval: Eval::centipawns(cp),
        pv: vec![mv(p, uci)],
    }
}

fn probe(cp: i32) -> Probe {
    Probe {
        eval: Eval::centipawns(cp),
        depth: 12,
        pv: Vec::new(),
    }
}

fn user(p: &Position, text: &str, a: Assessment) -> UserLine {
    let read = read_moves(p, text);
    assert_eq!(read.rejected, None, "{text}");
    UserLine {
        moves: read.moves,
        assessment: a,
    }
}

// ---- D2311: the classes ----

#[test]
fn the_seven_classes_have_the_thresholds_of_the_spec() {
    let class = |cp| Eval::centipawns(cp).assessment();
    let table = [
        (400, Assessment::WhiteWinning),
        (300, Assessment::WhiteWinning),
        (299, Assessment::WhiteBetter),
        (100, Assessment::WhiteBetter),
        (99, Assessment::WhiteSlightly),
        (25, Assessment::WhiteSlightly),
        (24, Assessment::Equal),
        (0, Assessment::Equal),
        (-24, Assessment::Equal),
        (-25, Assessment::BlackSlightly),
        (-99, Assessment::BlackSlightly),
        (-100, Assessment::BlackBetter),
        (-299, Assessment::BlackBetter),
        (-300, Assessment::BlackWinning),
    ];
    for (cp, expected) in table {
        assert_eq!(class(cp), expected, "{cp} cp");
    }
    assert_eq!(
        Eval::from_score(Score::Mate(2)).assessment(),
        Assessment::WhiteWinning
    );
    assert_eq!(
        Eval::from_score(Score::Mate(-2)).assessment(),
        Assessment::BlackWinning
    );
}

#[test]
fn the_symbols_are_the_seven_of_the_scid() {
    let symbols: Vec<&str> = Assessment::ALL.iter().map(|a| a.symbol()).collect();
    assert_eq!(symbols, ["-+", "-/+", "=+", "=", "+=", "+/-", "+-"]);
}

#[test]
fn a_mate_on_the_board_is_worth_the_mate_value_for_the_winner() {
    let mated = Position::from_fen("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1").unwrap();
    let eval = Eval::terminal(&mated).expect("terminal");
    assert_eq!(eval.cp, MATE_CP);
    assert!(Eval::terminal(&start()).is_none());
}

// ---- D2302, D2334: the text of the lines ----

#[test]
fn a_whole_line_typed_with_numbers_is_read() {
    let p = start();
    let read = read_moves(&p, "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6");
    assert_eq!(read.moves.len(), 6);
    assert_eq!(san_line(&p, &read.moves), "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6");
}

#[test]
fn the_mental_board_checks_each_move_against_the_line() {
    let p = start();
    // Qh5 is legal only after e4 e5; the second Qh5 is not.
    let read = read_moves(&p, "e4 e5 Qh5 Qh5");
    assert_eq!(read.moves.len(), 3);
    assert_eq!(read.rejected.as_deref(), Some("Qh5"));
}

#[test]
fn nothing_the_user_types_can_panic() {
    let p = start();
    for text in [
        "",
        " ",
        "1.",
        "...",
        "\u{0}",
        "e9",
        "O-O-O",
        "e4".repeat(300).as_str(),
    ] {
        drop(read_moves(&p, text));
    }
}

// ---- D2312: the good moves ----

#[test]
fn a_move_is_good_within_fifty_centipawns_of_the_best() {
    let p = start();
    let lines = [
        root_line(&p, 2, "d2d4", 20),
        root_line(&p, 1, "e2e4", 40),
        root_line(&p, 3, "g1f3", -30),
    ];
    let good = good_moves(&p, &lines);
    let names: Vec<&str> = good.iter().map(|g| g.san.as_str()).collect();
    assert_eq!(names, ["e4", "d4"], "best first; Nf3 is 70 cp behind");
}

#[test]
fn in_a_won_position_every_winning_move_is_good() {
    let p = start();
    let lines = [
        root_line(&p, 1, "e2e4", 900),
        root_line(&p, 2, "d2d4", 650),
        root_line(&p, 3, "g1f3", 300),
    ];
    assert_eq!(good_moves(&p, &lines).len(), 2);
}

#[test]
fn black_reads_the_evaluations_from_its_side() {
    let p =
        Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1").unwrap();
    let lines = [root_line(&p, 1, "e7e5", -30), root_line(&p, 2, "a7a6", 60)];
    let good = good_moves(&p, &lines);
    assert_eq!(good.len(), 1);
    assert_eq!(good[0].san, "e5");
    assert_eq!(good[0].mover_cp, 30);
}

#[test]
fn two_lines_with_the_same_first_move_are_one_good_move() {
    let p = start();
    let lines = [root_line(&p, 1, "e2e4", 30), root_line(&p, 2, "e2e4", 30)];
    assert_eq!(good_moves(&p, &lines).len(), 1);
}

// ---- D2312, D2313: found, missed, crucial ----

#[test]
fn a_missed_move_better_than_the_best_candidate_is_crucial() {
    let p = start();
    let root = [root_line(&p, 1, "e2e4", 45), root_line(&p, 2, "d2d4", 30)];
    let lines = [user(&p, "d4", Assessment::WhiteSlightly)];
    let probes = [LineProbes {
        first: Some(probe(30)),
        end: None,
    }];
    let report = compare(&p, &root, &lines, &probes);
    assert_eq!((report.found, report.good_total), (1, 2));
    assert!(report.has_crucial_miss());
    assert_eq!(report.best_missed().map(|g| g.san.as_str()), Some("e4"));
}

#[test]
fn the_scid_rule_stops_at_the_first_line_the_user_considered() {
    // The user found e4, the best: d4 is missed but worse, so not crucial.
    let p = start();
    let root = [root_line(&p, 1, "e2e4", 45), root_line(&p, 2, "d2d4", 30)];
    let lines = [user(&p, "e4", Assessment::WhiteSlightly)];
    let probes = [LineProbes {
        first: Some(probe(45)),
        end: None,
    }];
    let report = compare(&p, &root, &lines, &probes);
    assert!(!report.has_crucial_miss());
    assert_eq!(report.best_missed().map(|g| g.san.as_str()), Some("d4"));
}

#[test]
fn without_any_valid_candidate_every_missed_move_is_crucial() {
    let p = start();
    let root = [root_line(&p, 1, "e2e4", 45)];
    let lines = [user(&p, "a3", Assessment::Equal)];
    let report = compare(&p, &root, &lines, &[LineProbes::default()]);
    assert_eq!(report.found, 0);
    assert!(report.has_crucial_miss());
    assert!(!report.lines[0].good_candidate);
}

// ---- D2314: the line ----

#[test]
fn accuracy_is_one_for_the_same_class_half_for_a_neighbour_zero_otherwise() {
    let p = start();
    let root = [root_line(&p, 1, "e2e4", 30)];
    let run = |a: Assessment| {
        let lines = [user(&p, "e4", a)];
        let probes = [LineProbes {
            first: Some(probe(60)),
            end: None,
        }];
        compare(&p, &root, &lines, &probes).lines[0].accuracy
    };
    assert!((run(Assessment::WhiteSlightly) - 1.0).abs() < 1e-6);
    assert!((run(Assessment::Equal) - 0.5).abs() < 1e-6);
    assert!((run(Assessment::WhiteWinning)).abs() < 1e-6);
}

#[test]
fn a_line_that_drops_more_than_eighty_centipawns_is_not_sound() {
    let p = start();
    let root = [root_line(&p, 1, "e2e4", 30)];
    let lines = [user(&p, "e4 e5", Assessment::Equal)];
    let probes = |end| {
        [LineProbes {
            first: Some(probe(30)),
            end: Some(probe(end)),
        }]
    };
    assert!(compare(&p, &root, &lines, &probes(-50)).lines[0].sound);
    assert!(!compare(&p, &root, &lines, &probes(-51)).lines[0].sound);
}

#[test]
fn black_to_move_drops_in_its_own_direction() {
    let p =
        Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1").unwrap();
    let root = [root_line(&p, 1, "e7e5", -30)];
    let lines = [user(&p, "e5 Nf3", Assessment::Equal)];
    // White's +200 is a drop of 170 for Black.
    let drop = [LineProbes {
        first: Some(probe(-30)),
        end: Some(probe(200)),
    }];
    assert!(!compare(&p, &root, &lines, &drop).lines[0].sound);
    // White's -200 is a gain for Black.
    let gain = [LineProbes {
        first: Some(probe(-30)),
        end: Some(probe(-200)),
    }];
    assert!(compare(&p, &root, &lines, &gain).lines[0].sound);
}

#[test]
fn the_correct_depth_counts_the_plies_that_follow_the_engine() {
    let p = start();
    let after = |ucis: &[&str]| {
        let mut pos = p.clone();
        let mut out = Vec::new();
        for u in ucis {
            let m = pos.parse_uci(u).unwrap();
            pos = pos.after(m).unwrap();
            out.push(m);
        }
        out
    };
    let line = after(&["e2e4", "e7e5", "g1f3", "b8c6"]);
    // The engine's play after e4: e5, Nf3, Nf6 (differs at the fourth ply).
    let engine_pv = after(&["e2e4", "e7e5", "g1f3", "g8f6"])[1..].to_vec();
    let root = [root_line(&p, 1, "e2e4", 30)];
    let lines = [UserLine {
        moves: line,
        assessment: Assessment::Equal,
    }];
    let probes = [LineProbes {
        first: Some(Probe {
            eval: Eval::centipawns(30),
            depth: 15,
            pv: engine_pv,
        }),
        end: Some(probe(30)),
    }];
    assert_eq!(compare(&p, &root, &lines, &probes).lines[0].agree_plies, 3);
}

#[test]
fn a_short_line_gets_partial_depth_credit_and_a_line_to_the_mate_gets_all() {
    let p = start();
    let root = [root_line(&p, 1, "e2e4", 30)];
    let probes = [LineProbes {
        first: Some(probe(30)),
        end: Some(probe(30)),
    }];
    let two = [user(&p, "e4 e5", Assessment::Equal)];
    let credit = compare(&p, &root, &two, &probes).lines[0].depth_credit;
    assert!((credit - 0.4).abs() < 1e-6);

    let fools = Position::new();
    let mate = [user(&fools, "f3 e5 g4 Qh4#", Assessment::BlackWinning)];
    let credit = compare(&fools, &root, &mate, &probes).lines[0].depth_credit;
    assert!((credit - 1.0).abs() < 1e-6, "the line ends the game");
}

#[test]
fn a_line_the_engine_could_not_search_counts_as_unknown() {
    let p = start();
    let root = [root_line(&p, 1, "e2e4", 30)];
    let lines = [user(&p, "e4 e5", Assessment::Equal)];
    let line = &compare(&p, &root, &lines, &[LineProbes::default()]).lines[0];
    assert!(!line.sound);
    assert!(line.accuracy.abs() < 1e-6);
    assert!(line.end.is_none());
}

// ---- D2315: the score ----

#[test]
fn a_perfect_exercise_is_worth_one_hundred() {
    let p = start();
    let root = [root_line(&p, 1, "e2e4", 30)];
    let lines = [user(&p, "e4 e5 Nf3 Nc6 Bb5", Assessment::WhiteSlightly)];
    let probes = [LineProbes {
        first: Some(probe(30)),
        end: Some(probe(40)),
    }];
    let report = compare(&p, &root, &lines, &probes);
    assert_eq!(report.score, 100);
    assert!(report.parts.penalty.abs() < 1e-6);
}

#[test]
fn the_score_follows_the_formula_with_the_penalty() {
    let parts = ScoreParts {
        candidates: 0.5,
        accuracy: 0.5,
        quality: 1.0,
        penalty: CRUCIAL_PENALTY,
    };
    // 50*0.5 + 25*0.5 + 25*1.0 - 15 = 47.5, rounded away from zero.
    assert_eq!(score_of(&parts, false), 48);
    assert_eq!(score_of(&parts, true), 0, "no lines, no score");
}

#[test]
fn the_score_never_leaves_zero_to_one_hundred() {
    let low = ScoreParts {
        penalty: 15.0,
        ..ScoreParts::default()
    };
    assert_eq!(score_of(&low, false), 0);
    let high = ScoreParts {
        candidates: 5.0,
        accuracy: 5.0,
        quality: 5.0,
        penalty: 0.0,
    };
    assert_eq!(score_of(&high, false), 100);
}

#[test]
fn no_lines_is_a_zero_with_the_good_moves_still_listed() {
    let p = start();
    let root = [root_line(&p, 1, "e2e4", 30)];
    let report = compare(&p, &root, &[], &[]);
    assert_eq!(report.score, 0);
    assert_eq!(report.good_total, 1);
    assert_eq!(report.found, 0);
}

#[test]
fn the_labels_of_the_comments_are_the_same_in_every_language() {
    assert_eq!(Eval::centipawns(31).label(), "+0.31");
    assert_eq!(Eval::centipawns(-120).label(), "-1.20");
    assert_eq!(Eval::from_score(Score::Mate(3)).label(), "#3");
    assert_eq!(Eval::from_score(Score::Mate(-4)).label(), "#-4");
}
