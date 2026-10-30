//! From a game and the engine's evaluations to the inputs of the report
//! (spec M4-10b, D1530): book detection from the ECO list, the sacrifice
//! heuristic of D1428 and the SAN of every move.

use mychess_core::{Color, EcoBook, Game, Move, Position};

use super::MoveInput;
use crate::annotate::PositionEval;
use crate::material::advantage;

/// How many moves of the engine's line the sacrifice test follows after the
/// move played: the opponent's reply and the mover's answer (D1428).
const SACRIFICE_PLIES: usize = 2;

/// The least material (in pawns) the mover must be behind at the end of the
/// line for the move to count as a sacrifice.
const SACRIFICE_POINTS: i32 = 2;

/// The material balance (pawns) for `color`: positive when it is ahead.
fn balance(position: &Position, color: Color) -> i32 {
    match advantage(position) {
        Some((ahead, points)) => {
            let points = i32::try_from(points).unwrap_or(i32::MAX);
            if ahead == color { points } else { -points }
        }
        None => 0,
    }
}

/// Whether `played` gives up material that the engine's line (`pv`, the
/// continuation of the position after the move) does not win back within
/// [`SACRIFICE_PLIES`] moves. Conservative (D1428): without a line long
/// enough, or on a line that cannot be played, the answer is no.
pub(crate) fn is_sacrifice(before: &Position, played: Move, pv: &[Move]) -> bool {
    if pv.len() < SACRIFICE_PLIES {
        return false;
    }
    let mover = before.turn();
    let start = balance(before, mover);
    let mut position = before.clone();
    if position.play(played).is_err() {
        return false;
    }
    for reply in pv.iter().take(SACRIFICE_PLIES) {
        if position.play(*reply).is_err() {
            return false;
        }
    }
    balance(&position, mover) - start <= -SACRIFICE_POINTS
}

/// What [`inputs_from_game`] produces: the inputs of the report and the SAN
/// of every move analysed.
#[derive(Debug, Clone, Default)]
pub struct ReportInputs {
    /// One input per move whose two positions were evaluated.
    pub moves: Vec<MoveInput>,
    /// The SAN of each of those moves.
    pub sans: Vec<String>,
}

/// The inputs of the report for the main line of `game`. `evals[i]` is the
/// evaluation of the position after `i` moves; a move is included when both
/// of its positions have one, so a partial run gives a shorter report.
pub fn inputs_from_game(
    game: &Game,
    evals: &[PositionEval],
    eco: Option<&EcoBook>,
) -> ReportInputs {
    inputs_from_game_with_book(game, evals, eco, None)
}

/// Like [`inputs_from_game`], and a move is also a book move when the
/// Polyglot `book` has it in the position before (spec M5-6, D1666).
pub fn inputs_from_game_with_book(
    game: &Game,
    evals: &[PositionEval],
    eco: Option<&EcoBook>,
    book: Option<&dyn crate::annotate::BookProbe>,
) -> ReportInputs {
    let mut out = ReportInputs::default();
    for (i, node) in game.mainline().enumerate() {
        let (Some(before_eval), Some(after_eval)) = (evals.get(i), evals.get(i + 1)) else {
            break;
        };
        let Some(parent) = game.parent(node).ok().flatten() else {
            break;
        };
        let (Ok(before), Ok(after), Ok(played)) = (
            game.position_at(parent),
            game.position_at(node),
            game.mv(node),
        ) else {
            break;
        };
        let Ok(san) = before.san(played) else {
            break;
        };
        out.sans.push(san);
        out.moves.push(MoveInput {
            mover: before.turn(),
            before: before_eval.clone(),
            after: after_eval.clone(),
            played,
            book: eco.is_some_and(|eco| eco.lookup(after).is_some())
                || book.is_some_and(|book| book.moves(before).contains(&played)),
            sacrifice: is_sacrifice(before, played, &after_eval.pv),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use mychess_core::SanMode;

    use super::*;

    fn game_of(moves: &[&str]) -> Game {
        let mut game = Game::new();
        let mut position = Position::new();
        let mut node = game.root();
        for san in moves {
            let m = san_move(&position, san);
            position.play(m).expect("play");
            node = game.add_move(node, m).expect("add");
        }
        game
    }

    fn san_move(position: &Position, text: &str) -> Move {
        position
            .parse_san(text, SanMode::Strict)
            .expect("legal san")
            .mv
    }

    fn eval(score: f32) -> PositionEval {
        PositionEval {
            score,
            mate: None,
            pv: Vec::new(),
            depth: 10,
            second: None,
        }
    }

    #[test]
    fn a_partial_run_gives_the_moves_that_have_both_evaluations() {
        let game = game_of(&["e4", "e5", "Nf3"]);
        let evals = [eval(0.2), eval(0.3), eval(0.2)];
        let inputs = inputs_from_game(&game, &evals, None);
        assert_eq!(inputs.moves.len(), 2);
        assert_eq!(inputs.sans, ["e4", "e5"]);
        assert_eq!(inputs.moves[1].mover, Color::Black);
        assert!(inputs.moves.iter().all(|m| !m.book && !m.sacrifice));
    }

    #[test]
    fn no_evaluations_give_no_inputs() {
        let game = game_of(&["e4"]);
        assert!(inputs_from_game(&game, &[], None).moves.is_empty());
        assert!(inputs_from_game(&game, &[eval(0.0)], None).moves.is_empty());
    }

    #[test]
    fn giving_a_piece_for_nothing_is_a_sacrifice_and_a_trade_is_not() {
        // 1.e4 d5 2.Bb5+ c6 3.Bxc6+ bxc6: White gives a bishop for a pawn
        // and Black takes back: net -2 for White after the reply.
        let mut position = Position::new();
        for san in ["e4", "d5", "Bb5+", "c6"] {
            let m = san_move(&position, san);
            position.play(m).expect("play");
        }
        let bxc6 = san_move(&position, "Bxc6+");
        let mut after = position.clone();
        after.play(bxc6).expect("play");
        let reply = san_move(&after, "bxc6");
        let mut tail = after.clone();
        tail.play(reply).expect("play");
        let next = san_move(&tail, "Nf3");
        assert!(is_sacrifice(&position, bxc6, &[reply, next]));
        // A short line proves nothing.
        assert!(!is_sacrifice(&position, bxc6, &[reply]));
    }

    #[test]
    fn an_even_exchange_is_not_a_sacrifice() {
        let mut position = Position::new();
        for san in ["e4", "d5"] {
            let m = san_move(&position, san);
            position.play(m).expect("play");
        }
        let exd5 = san_move(&position, "exd5");
        let mut after = position.clone();
        after.play(exd5).expect("play");
        let reply = san_move(&after, "Qxd5");
        let mut tail = after.clone();
        tail.play(reply).expect("play");
        let next = san_move(&tail, "Nc3");
        assert!(!is_sacrifice(&position, exd5, &[reply, next]));
    }

    /// The evaluations of a game that ends at its last position: the engine
    /// is not asked there, `no_moves` stands in (D1586).
    fn evals_with_end(game: &Game, searched: &[f32]) -> Vec<PositionEval> {
        let mut evals: Vec<PositionEval> = searched.iter().copied().map(eval).collect();
        let last = game
            .mainline()
            .last()
            .and_then(|n| game.position_at(n).ok())
            .expect("last position");
        evals.push(PositionEval::no_moves(last));
        evals
    }

    #[test]
    fn the_mating_move_of_either_side_is_not_a_fault() {
        use crate::report::{ReportThresholds, build_report};
        // Black mates with 2...Qh4#; White mates with 3.Qh5#.
        let black = game_of(&["f3", "e5", "g4", "Qh4#"]);
        let white = game_of(&["e4", "f6", "d4", "g5", "Qh5#"]);
        for (game, searched) in [
            (&black, vec![0.0, 0.0, 0.0, -9.0]),
            (&white, vec![0.0, 0.0, 0.0, 0.0, 9.0]),
        ] {
            let evals = evals_with_end(game, &searched);
            let inputs = inputs_from_game(game, &evals, None);
            let report = build_report(&inputs.moves, &ReportThresholds::default());
            let last = report.moves.last().expect("a verdict");
            assert!(!last.class.is_fault(), "{:?}", last.class);
            assert_eq!(last.loss, 0.0);
        }
    }

    #[test]
    fn stalemating_a_won_position_is_a_fault_of_the_winner() {
        use crate::report::{ReportThresholds, build_report};
        let stalemate = Position::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").expect("fen");
        let end = PositionEval::no_moves(&stalemate);
        assert_eq!(end.score, 0.0);
        assert_eq!(end.mate, None);
        let input = MoveInput {
            mover: Color::White,
            before: eval(9.0),
            after: end,
            played: some_move(),
            book: false,
            sacrifice: false,
        };
        let report = build_report(&[input], &ReportThresholds::default());
        assert!(report.moves[0].class.is_fault());
    }

    fn some_move() -> Move {
        Position::new().parse_uci("e2e4").expect("legal")
    }
}
