//! How a game against an opponent ends (spec M8-1, D1226-D1229, D1233).
//!
//! The automatic ends come from the position and the history; the others
//! (time, resignation, a broken engine) come from the controller. Scid:
//! `sergame.tcl` `checkEndOfGame` (544-573) and `checkRepetition`
//! (`main.tcl` 1936-1971).

use mychess_core::{Color, GameResult, Position};

/// Why a game against an opponent ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlayEnd {
    /// The side to move was mated.
    Checkmate {
        /// The side that gave mate.
        winner: Color,
    },
    /// The side to move has no move and is not in check.
    Stalemate,
    /// Neither side can mate (D1233).
    InsufficientMaterial,
    /// More than 99 half-moves without a capture or a pawn move.
    FiftyMoves,
    /// The same position (board, side to move and castling rights; no en
    /// passant) occurred three times.
    ThreefoldRepetition,
    /// The flag of `loser` fell and the opponent can still mate (D1226).
    Timeout {
        /// The side that ran out of time.
        loser: Color,
    },
    /// The flag of a side fell but the other cannot mate: a draw.
    TimeoutDraw,
    /// The player resigned (D1227).
    Resignation {
        /// The side that resigned.
        loser: Color,
    },
    /// The opponent played an illegal move or none at all (D1238): the game
    /// stops without a result.
    OpponentFailure,
    /// The engine accepted the draw the player offered (M8-1c, D1505).
    DrawAgreed,
}

impl PlayEnd {
    /// The result to record.
    pub fn result(self) -> GameResult {
        match self {
            PlayEnd::Checkmate { winner } => win(winner),
            PlayEnd::Timeout { loser } | PlayEnd::Resignation { loser } => win(!loser),
            PlayEnd::Stalemate
            | PlayEnd::InsufficientMaterial
            | PlayEnd::FiftyMoves
            | PlayEnd::ThreefoldRepetition
            | PlayEnd::TimeoutDraw
            | PlayEnd::DrawAgreed => GameResult::Draw,
            PlayEnd::OpponentFailure => GameResult::Unknown,
        }
    }

    /// The comment the Scid writes on the last move for the automatic
    /// draws (D1228), in English like every comment of the game.
    pub fn comment(self) -> Option<&'static str> {
        match self {
            PlayEnd::ThreefoldRepetition => Some("3 fold repetition"),
            PlayEnd::FiftyMoves => Some("50 move rule"),
            PlayEnd::DrawAgreed => Some("Draw agreed"),
            _ => None,
        }
    }
}

fn win(color: Color) -> GameResult {
    match color {
        Color::White => GameResult::White,
        Color::Black => GameResult::Black,
    }
}

/// The part of a position that makes two positions "the same" for the
/// repetition rule: board, side to move and castling rights (D1228).
pub(crate) fn repetition_key(position: &Position) -> String {
    let fen = position.fen();
    let mut fields = fen.split(' ');
    let mut key = String::with_capacity(fen.len());
    for (i, field) in fields.by_ref().take(3).enumerate() {
        if i > 0 {
            key.push(' ');
        }
        key.push_str(field);
    }
    key
}

/// The end of the game caused by the position just reached, if any.
///
/// `keys` are the repetition keys of **every** position of the game so far,
/// the current one included (last).
pub(crate) fn position_end(position: &Position, keys: &[String]) -> Option<PlayEnd> {
    if position.is_checkmate() {
        return Some(PlayEnd::Checkmate {
            winner: !position.turn(),
        });
    }
    if position.is_stalemate() {
        return Some(PlayEnd::Stalemate);
    }
    if position.is_insufficient_material() {
        return Some(PlayEnd::InsufficientMaterial);
    }
    // `halfmove_clock() > 99`, as the Scid checks it.
    if position.halfmove_clock() > 99 {
        return Some(PlayEnd::FiftyMoves);
    }
    if let Some(current) = keys.last()
        && keys.iter().filter(|k| *k == current).count() >= 3
    {
        return Some(PlayEnd::ThreefoldRepetition);
    }
    None
}

/// Whether `color` can still give mate when the other side helps (FIDE: a
/// flag that falls is a draw when the opponent cannot). The rule is the one
/// of `shakmaty` (D1233), the same used for [`PlayEnd::InsufficientMaterial`]:
/// K+B+B of the same colour against a bare king cannot mate, K+N+N can.
pub(crate) fn can_mate(position: &Position, color: Color) -> bool {
    !position.has_insufficient_material(color)
}

/// The end for the fall of the flag of `loser` in `position` (D1226).
pub(crate) fn timeout_end(position: &Position, loser: Color) -> PlayEnd {
    if can_mate(position, !loser) {
        PlayEnd::Timeout { loser }
    } else {
        PlayEnd::TimeoutDraw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(fen: &str) -> Position {
        Position::from_fen(fen).expect("fen")
    }

    fn keys_of(ucis: &[&str]) -> (Position, Vec<String>) {
        let mut p = Position::new();
        let mut keys = vec![repetition_key(&p)];
        for u in ucis {
            let m = p.parse_uci(u).expect("uci");
            p.play(m).expect("play");
            keys.push(repetition_key(&p));
        }
        (p, keys)
    }

    #[test]
    fn mate_gives_the_win_to_the_mating_side() {
        let p = pos("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3");
        let end = position_end(&p, &[]).expect("end");
        assert_eq!(
            end,
            PlayEnd::Checkmate {
                winner: Color::Black
            }
        );
        assert_eq!(end.result(), GameResult::Black);
    }

    #[test]
    fn stalemate_is_a_draw() {
        let p = pos("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
        let end = position_end(&p, &[]).expect("end");
        assert_eq!(end, PlayEnd::Stalemate);
        assert_eq!(end.result(), GameResult::Draw);
    }

    #[test]
    fn bare_kings_and_a_lone_minor_are_a_draw() {
        for fen in [
            "8/8/4k3/8/8/3K4/8/8 w - - 0 1",
            "8/8/4k3/8/8/3KB3/8/8 w - - 0 1",
            "8/8/4k3/8/8/3KN3/8/8 b - - 0 1",
        ] {
            assert_eq!(
                position_end(&pos(fen), &[]),
                Some(PlayEnd::InsufficientMaterial),
                "{fen}"
            );
        }
        assert_eq!(
            position_end(&pos("8/8/4k3/8/8/3KP3/8/8 w - - 0 1"), &[]),
            None
        );
    }

    #[test]
    fn fifty_move_rule_triggers_above_99_halfmoves() {
        let at_99 = pos("3k4/8/8/8/8/8/4R3/4K3 w - - 99 80");
        let at_100 = pos("3k4/8/8/8/8/8/4R3/4K3 w - - 100 80");
        assert_eq!(position_end(&at_99, &[]), None);
        let end = position_end(&at_100, &[]).expect("end");
        assert_eq!(end, PlayEnd::FiftyMoves);
        assert_eq!(end.comment(), Some("50 move rule"));
    }

    #[test]
    fn mate_wins_over_the_fifty_move_rule() {
        let p = pos("7k/5Q2/6K1/8/8/8/8/8 b - - 120 90");
        assert_eq!(position_end(&p, &[]), Some(PlayEnd::Stalemate));
    }

    #[test]
    fn the_third_occurrence_is_a_threefold_repetition() {
        let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
        let (p, keys) = keys_of(&cycle);
        assert_eq!(position_end(&p, &keys), None, "second occurrence");
        let twice: Vec<&str> = cycle.iter().chain(cycle.iter()).copied().collect();
        let (p, keys) = keys_of(&twice);
        let end = position_end(&p, &keys).expect("end");
        assert_eq!(end, PlayEnd::ThreefoldRepetition);
        assert_eq!(end.result(), GameResult::Draw);
        assert_eq!(end.comment(), Some("3 fold repetition"));
    }

    #[test]
    fn repetition_needs_the_same_side_to_move() {
        let a = pos("3k4/8/8/8/8/8/4R3/4K3 w - - 0 1");
        let b = pos("3k4/8/8/8/8/8/4R3/4K3 b - - 0 1");
        assert_ne!(repetition_key(&a), repetition_key(&b));
    }

    #[test]
    fn repetition_needs_the_same_castling_rights() {
        let with = pos("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        let without = pos("r3k2r/8/8/8/8/8/8/R3K2R w - - 0 1");
        assert_ne!(repetition_key(&with), repetition_key(&without));
    }

    #[test]
    fn repetition_ignores_the_en_passant_square() {
        let with = pos("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2");
        let without = pos("4k3/8/8/3pP3/8/8/8/4K3 w - - 0 2");
        assert_eq!(repetition_key(&with), repetition_key(&without));
    }

    #[test]
    fn a_timeout_loses_unless_the_opponent_cannot_mate() {
        let rook = pos("3k4/8/8/8/8/8/4R3/4K3 w - - 0 1");
        let end = timeout_end(&rook, Color::Black);
        assert_eq!(
            end,
            PlayEnd::Timeout {
                loser: Color::Black
            }
        );
        assert_eq!(end.result(), GameResult::White);
        // White has only a knight: Black's flag falls, the game is drawn.
        let knight = pos("4k3/8/8/8/8/8/4N3/4K3 w - - 0 1");
        assert_eq!(timeout_end(&knight, Color::Black), PlayEnd::TimeoutDraw);
        assert_eq!(PlayEnd::TimeoutDraw.result(), GameResult::Draw);
    }

    #[test]
    fn two_minor_pieces_and_pawns_can_mate() {
        assert!(can_mate(
            &pos("4k3/8/8/8/8/8/4NN2/4K3 w - - 0 1"),
            Color::White
        ));
        assert!(can_mate(
            &pos("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1"),
            Color::White
        ));
        assert!(!can_mate(
            &pos("4k3/8/8/8/8/8/4B3/4K3 w - - 0 1"),
            Color::White
        ));
    }

    #[test]
    fn mating_material_follows_the_rule_of_the_position_end() {
        // K+B+B on the same colour (c1 and e3 are both dark) cannot mate.
        assert!(!can_mate(
            &pos("4k3/8/8/8/8/4B3/8/2B1K3 w - - 0 1"),
            Color::White
        ));
        // Opposite colours (c1 dark, d3 light) can.
        assert!(can_mate(
            &pos("4k3/8/8/8/8/3B4/8/2B1K3 w - - 0 1"),
            Color::White
        ));
        // K+N+N against a bare king can (with help).
        assert!(can_mate(
            &pos("4k3/8/8/8/8/8/4NN2/4K3 w - - 0 1"),
            Color::White
        ));
        // K+B against K+B of the same colour: nobody can mate.
        let same = pos("4kb2/8/8/8/8/8/8/2B1K3 w - - 0 1");
        assert!(!can_mate(&same, Color::White) && !can_mate(&same, Color::Black));
        assert_eq!(timeout_end(&same, Color::Black), PlayEnd::TimeoutDraw);
        // Opposite colours: a flag is a loss.
        let opposite = pos("4k3/5b2/8/8/8/8/8/2B1K3 w - - 0 1");
        assert!(can_mate(&opposite, Color::White));
        assert_eq!(
            timeout_end(&opposite, Color::Black),
            PlayEnd::Timeout {
                loser: Color::Black
            }
        );
    }

    #[test]
    fn resignation_loses_and_failure_has_no_result() {
        let resign = PlayEnd::Resignation {
            loser: Color::White,
        };
        assert_eq!(resign.result(), GameResult::Black);
        assert_eq!(PlayEnd::OpponentFailure.result(), GameResult::Unknown);
    }
}
