//! Writing the report into the game and reading it back (spec M4-10d,
//! D1439, D1447): `[%eval]` comments, NAGs and a variation with the best
//! move for the faults; and a report rebuilt from the `[%eval]` comments of
//! a game, without an engine.

use mychess_core::{Game, Move, Nag, NodeId, Position, eval_of};

use super::{GameReport, MoveClass};
use crate::annotate::{Action, PositionEval, ScoreFormat, format_score_text};

/// The longest engine line written as a variation.
const VARIATION_PLIES: usize = 8;

/// A score at or above this many pawns is a mate (`MATE_SCORE`).
const MATE_PAWNS: f32 = 300.0;

/// The NAG a class writes; `Best` and the quiet classes write none, to keep
/// the game readable (deviation from D1439, which lists `!` for the best).
fn class_nag(class: MoveClass) -> Option<Nag> {
    let number = match class {
        MoveClass::Brilliant => 3,
        MoveClass::Great => 1,
        MoveClass::Inaccuracy => 6,
        MoveClass::Mistake | MoveClass::Miss => 2,
        MoveClass::Blunder => 4,
        MoveClass::Best | MoveClass::Excellent | MoveClass::Good | MoveClass::Book => return None,
    };
    Nag::new(number).ok()
}

/// Whether a NAG says what the user thinks of the move (`!`, `?`, `!!`,
/// `??`, `!?`, `?!`): the report never adds a second opinion to it (D1582).
fn is_judgement(nag: Nag) -> bool {
    (1..=6).contains(&nag.value())
}

/// Whether the engine line `pv` is in the game already, from `parent` on.
fn line_exists(game: &Game, parent: NodeId, pv: &[Move]) -> bool {
    let mut current = parent;
    for mv in pv {
        let next = game
            .children(current)
            .ok()
            .and_then(|c| c.iter().copied().find(|n| game.mv(*n).ok() == Some(*mv)));
        match next {
            Some(node) => current = node,
            None => return false,
        }
    }
    true
}

/// The edits that write `report` into `game` (D1447) and how many moves they
/// touch: the evaluation after every move (unless the move already has one,
/// in any format of the Scid), the NAG of its class (unless the move already
/// has a judgement NAG of the user) and, for an inaccuracy or worse, the
/// engine's line as a variation (unless it is in the game). `evals[i]` is
/// the evaluation after `i` moves. A game that does not match the report
/// gives no edit. Writing twice gives nothing the second time.
pub fn write_plan(game: &Game, report: &GameReport, evals: &[PositionEval]) -> (Vec<Action>, u32) {
    let mut actions = Vec::new();
    let mut touched = 0u32;
    for (i, node) in game.mainline().enumerate() {
        let (Some(verdict), Some(after), Some(before)) =
            (report.moves.get(i), evals.get(i + 1), evals.get(i))
        else {
            break;
        };
        let Some(parent) = game.parent(node).ok().flatten() else {
            break;
        };
        if game.mv(node).ok() != Some(verdict.played) {
            return (Vec::new(), 0);
        }
        let first = actions.len();
        let has_eval = game
            .comment(node)
            .ok()
            .flatten()
            .is_some_and(|c| eval_of(c).is_some());
        if !has_eval {
            actions.push(Action::Score {
                node,
                text: format_score_text(after.score, after.mate, ScoreFormat::Eval),
            });
        }
        let nags = game.nags(node).unwrap_or_default();
        if let Some(nag) = class_nag(verdict.class)
            && !nags.contains(&nag)
            && !nags.iter().any(|n| is_judgement(*n))
        {
            actions.push(Action::Nag { node, nag });
        }
        if verdict.class.is_fault()
            && let Some(best) = before.pv.first()
            && *best != verdict.played
        {
            let pv: Vec<Move> = before.pv.iter().take(VARIATION_PLIES).copied().collect();
            if !line_exists(game, parent, &pv) {
                actions.push(Action::Variation {
                    parent,
                    pv,
                    comment: None,
                    end_nag: None,
                });
            }
        }
        if actions.len() > first {
            touched += 1;
        }
    }
    (actions, touched)
}

/// The edits of [`write_plan`], without the count.
pub fn write_actions(game: &Game, report: &GameReport, evals: &[PositionEval]) -> Vec<Action> {
    write_plan(game, report, evals).0
}

fn eval_of_pawns(pawns: f32) -> PositionEval {
    PositionEval {
        score: pawns,
        mate: (pawns.abs() >= MATE_PAWNS).then_some(if pawns > 0.0 { 1 } else { -1 }),
        pv: Vec::new(),
        depth: 0,
        second: None,
    }
}

/// The evaluations kept in the `[%eval]` comments of the main line, as far
/// as they are continuous from the start (D1439). The start position counts
/// as level (0.00) when it has no evaluation of its own and the game starts
/// from the standard position. There is no line and no second line: the
/// report built from them cannot tell the best move or a great move.
pub fn evals_from_game(game: &Game) -> Vec<PositionEval> {
    let root = game.comment(game.root()).ok().flatten().and_then(eval_of);
    let start = match root {
        Some(value) => value,
        None if game.start_position() == &Position::new() => 0.0,
        None => return Vec::new(),
    };
    let mut evals = vec![eval_of_pawns(start)];
    for node in game.mainline() {
        match game.comment(node).ok().flatten().and_then(eval_of) {
            Some(value) => evals.push(eval_of_pawns(value)),
            None => break,
        }
    }
    evals
}

#[cfg(test)]
mod tests {
    use mychess_core::SanMode;

    use super::*;
    use crate::report::{ReportThresholds, build_report, inputs_from_game};

    fn game_of(sans: &[&str]) -> Game {
        let mut game = Game::new();
        let mut position = Position::new();
        let mut node = game.root();
        for san in sans {
            let m = position.parse_san(san, SanMode::Strict).expect("san").mv;
            position.play(m).expect("play");
            node = game.add_move(node, m).expect("add");
        }
        game
    }

    fn eval(score: f32, pv: Vec<Move>) -> PositionEval {
        PositionEval {
            score,
            mate: None,
            pv,
            depth: 12,
            second: None,
        }
    }

    /// 1.e4 e5 2.Qh5 (best was Nf3) Nc6: White's second move loses ground.
    fn blunder_game() -> (Game, Vec<PositionEval>) {
        let game = game_of(&["e4", "e5", "Qh5", "Nc6"]);
        let p = Position::new();
        let nf3 = {
            let mut q = p.clone();
            for san in ["e4", "e5"] {
                let m = q.parse_san(san, SanMode::Strict).expect("san").mv;
                q.play(m).expect("play");
            }
            q.parse_san("Nf3", SanMode::Strict).expect("san").mv
        };
        let evals = vec![
            eval(0.2, Vec::new()),
            eval(0.3, Vec::new()),
            eval(0.3, vec![nf3]),
            eval(-2.5, Vec::new()),
            eval(-2.4, Vec::new()),
        ];
        (game, evals)
    }

    #[test]
    fn writing_adds_evals_nags_and_the_best_line_for_faults_only() {
        let (game, evals) = blunder_game();
        let inputs = inputs_from_game(&game, &evals, None);
        let report = build_report(&inputs.moves, &ReportThresholds::default());
        let actions = write_actions(&game, &report, &evals);
        let scores = actions
            .iter()
            .filter(|a| matches!(a, Action::Score { .. }))
            .count();
        assert_eq!(scores, 4);
        assert!(actions.iter().any(|a| matches!(a, Action::Nag { .. })));
        let variations: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, Action::Variation { .. }))
            .collect();
        assert_eq!(variations.len(), 1, "only the fault gets a line");
    }

    #[test]
    fn a_move_that_already_has_an_eval_is_not_scored_twice() {
        let (mut game, evals) = blunder_game();
        let first = game.mainline().next().expect("move");
        game.set_comment(first, "[%eval +0.30]").expect("comment");
        let inputs = inputs_from_game(&game, &evals, None);
        let report = build_report(&inputs.moves, &ReportThresholds::default());
        let actions = write_actions(&game, &report, &evals);
        let scored: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                Action::Score { node, .. } => Some(*node),
                _ => None,
            })
            .collect();
        assert_eq!(scored.len(), 3);
        assert!(!scored.contains(&first));
    }

    #[test]
    fn a_report_of_another_game_writes_nothing() {
        let (game, evals) = blunder_game();
        let inputs = inputs_from_game(&game, &evals, None);
        let report = build_report(&inputs.moves, &ReportThresholds::default());
        let other = game_of(&["d4", "d5", "c4", "e6"]);
        assert!(write_actions(&other, &report, &evals).is_empty());
    }

    #[test]
    fn evaluations_are_read_back_up_to_the_first_gap() {
        let mut game = game_of(&["e4", "e5", "Nf3"]);
        let nodes: Vec<_> = game.mainline().collect();
        game.set_comment(nodes[0], "[%eval +0.30]").expect("c");
        game.set_comment(nodes[1], "[%eval #-3]").expect("c");
        let evals = evals_from_game(&game);
        assert_eq!(evals.len(), 3, "start (assumed level), 1.e4, 1...e5");
        assert_eq!(evals[0].score, 0.0);
        assert_eq!(evals[2].mate, Some(-1));
    }

    #[test]
    fn a_game_without_any_eval_gives_only_the_assumed_start() {
        let game = game_of(&["e4", "e5"]);
        assert_eq!(evals_from_game(&game).len(), 1);
    }

    #[test]
    fn the_classification_survives_the_trip_through_the_comments() {
        let (mut game, evals) = blunder_game();
        let inputs = inputs_from_game(&game, &evals, None);
        let report = build_report(&inputs.moves, &ReportThresholds::default());
        let actions = write_actions(&game, &report, &evals);
        crate::annotate::apply_actions(&mut game, &actions).expect("apply");
        let read = evals_from_game(&game);
        assert_eq!(read.len(), 5);
        let again = inputs_from_game(&game, &read, None);
        let rebuilt = build_report(&again.moves, &ReportThresholds::default());
        let blunder = |r: &GameReport| r.moves[2].class;
        assert_eq!(blunder(&rebuilt), blunder(&report));
    }

    #[test]
    fn a_judgement_nag_of_the_user_is_never_stacked_with_the_reports() {
        let (mut game, evals) = blunder_game();
        let qh5 = game.mainline().nth(2).expect("3.Qh5");
        game.add_nag(qh5, Nag::new(1).expect("nag")).expect("nag");
        let inputs = inputs_from_game(&game, &evals, None);
        let report = build_report(&inputs.moves, &ReportThresholds::default());
        assert!(report.moves[2].class.is_fault());
        let actions = write_actions(&game, &report, &evals);
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, Action::Nag { node, .. } if *node == qh5)),
            "the user's $1 stays alone"
        );
    }

    #[test]
    fn the_plain_format_of_the_scid_counts_as_an_eval() {
        let (mut game, evals) = blunder_game();
        let first = game.mainline().next().expect("move");
        game.set_comment(first, "+0.30").expect("comment");
        let inputs = inputs_from_game(&game, &evals, None);
        let report = build_report(&inputs.moves, &ReportThresholds::default());
        let scored = write_actions(&game, &report, &evals)
            .iter()
            .filter(|a| matches!(a, Action::Score { node, .. } if *node == first))
            .count();
        assert_eq!(scored, 0);
    }

    #[test]
    fn writing_again_touches_no_move() {
        let (mut game, evals) = blunder_game();
        let inputs = inputs_from_game(&game, &evals, None);
        let report = build_report(&inputs.moves, &ReportThresholds::default());
        let (actions, moves) = write_plan(&game, &report, &evals);
        assert_eq!(moves, 4);
        crate::annotate::apply_actions(&mut game, &actions).expect("apply");
        let (again, moves) = write_plan(&game, &report, &evals);
        assert!(again.is_empty(), "{again:?}");
        assert_eq!(moves, 0);
    }
}
