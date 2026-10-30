//! The evaluation of a position and the text of scores (spec M4-6, D463,
//! D466, D467, D475).

use mychess_core::{Color, Move, Nag, Position};
use mychess_engine::Score;

use super::options::{Informant, ScoreFormat, centipawns};

/// The score of a position without moves: 327 pawns for the side that gave
/// checkmate, 0 for a stalemate (`getScoreNoMoves`, `analysis.tcl`
/// 2682-2696).
pub const NO_MOVES_PAWNS: f32 = 327.0;

/// What the engine says about one position, from White's point of view
/// (D463).
#[derive(Debug, Clone, PartialEq)]
pub struct PositionEval {
    /// The score in pawns; a mate is `327.xx` (`Score::as_centipawns`).
    pub score: f32,
    /// The distance to mate in moves, positive when White mates; `None`
    /// when the score is not a mate.
    pub mate: Option<i32>,
    /// The principal variation from this position.
    pub pv: Vec<Move>,
    /// The depth of the last report.
    pub depth: u32,
    /// The score of the second line, when the engine searched with
    /// MultiPV (exercises, D470).
    pub second: Option<f32>,
}

impl PositionEval {
    /// The evaluation that an engine report gives.
    ///
    /// `Score::Mate(0)` (the side to move is already mated) carries no side
    /// and gives a plain `0.00` here; the report must never be fed with it.
    /// A position without moves is evaluated by [`PositionEval::no_moves`],
    /// which knows the side to move (D1586).
    pub fn from_score(score: Score, pv: Vec<Move>, depth: u32) -> PositionEval {
        PositionEval {
            score: score.pawns(),
            mate: match score {
                Score::Mate(n) if n != 0 => Some(n),
                _ => None,
            },
            pv,
            depth,
            second: None,
        }
    }

    /// The evaluation of a position that has no legal move: no engine is
    /// asked. The side to move is mated (or stalemated); see
    /// [`NO_MOVES_PAWNS`].
    pub fn no_moves(position: &Position) -> PositionEval {
        let score = if position.is_check() {
            match position.turn() {
                Color::Black => NO_MOVES_PAWNS,
                Color::White => -NO_MOVES_PAWNS,
            }
        } else {
            0.0
        };
        PositionEval {
            score,
            mate: None,
            pv: Vec::new(),
            depth: 0,
            second: None,
        }
    }
}

/// `+1.50`, with the sign always written and no negative zero.
pub(crate) fn signed_pawns(score: f32) -> String {
    let score = if centipawns(score) == 0 { 0.0 } else { score };
    format!("{score:+.2}")
}

/// The text of a score for a comment (D466).
///
/// `Eval` writes `[%eval +1.50]` and a mate as `[%eval #3]` (`#-3` for Black);
/// `Plain` and `Bracket` write the number of the Scid, a mate being
/// `+327.47`.
pub fn format_score_text(score: f32, mate: Option<i32>, format: ScoreFormat) -> String {
    match format {
        ScoreFormat::Plain => signed_pawns(score),
        ScoreFormat::Bracket => format!("[% {}]", signed_pawns(score)),
        ScoreFormat::Eval => match mate {
            Some(n) => format!("[%eval #{n}]"),
            None => format!("[%eval {}]", signed_pawns(score)),
        },
    }
}

/// The NAG that describes `score` (`scoreToNag`, `analysis.tcl`
/// 1682-1706): the tests run in the Scid's order, so `+0.50` is `+=` but
/// `-0.50` is `=` (D475). `None` never happens with the usual thresholds;
/// it is returned when a threshold is set so that no test matches.
pub fn score_to_nag(score: f32, informant: &Informant) -> Option<Nag> {
    let s = centipawns(score);
    let t = |pawns: f32| centipawns(pawns);
    let number = if s >= t(informant.decisive) {
        18 // +-
    } else if s >= t(informant.moderate) {
        16 // +/-
    } else if s >= t(informant.slight) {
        14 // +=
    } else if s >= -t(informant.slight) {
        10 // =
    } else if s <= -t(informant.decisive) {
        19 // -+
    } else if s <= -t(informant.moderate) {
        17 // -/+
    } else if s <= -t(informant.slight) {
        15 // -= (the Scid writes `-=`, the number is that of `=+`)
    } else {
        return None;
    };
    Nag::new(number).ok()
}

/// Adds `value` to the extra tag `existing` (`appendTag`, `misc.tcl`
/// 682-710): the value alone if the tag is new, `"<old> , <value>"` if it
/// is, and the old text unchanged if it already contains the value (D467).
pub fn append_tag(existing: Option<&str>, value: &str) -> String {
    let trimmed = value.trim();
    match existing {
        None | Some("") => trimmed.to_owned(),
        Some(old) if old.contains(value) => old.to_owned(),
        Some(old) => format!("{old} , {trimmed}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nag(score: f32) -> Option<u8> {
        score_to_nag(score, &Informant::default()).map(Nag::value)
    }

    #[test]
    fn score_text_in_the_three_formats() {
        assert_eq!(format_score_text(1.5, None, ScoreFormat::Plain), "+1.50");
        assert_eq!(
            format_score_text(-0.3, None, ScoreFormat::Bracket),
            "[% -0.30]"
        );
        assert_eq!(
            format_score_text(0.0, None, ScoreFormat::Eval),
            "[%eval +0.00]"
        );
        assert_eq!(
            format_score_text(327.47, Some(3), ScoreFormat::Eval),
            "[%eval #3]"
        );
        assert_eq!(
            format_score_text(-327.47, Some(-3), ScoreFormat::Eval),
            "[%eval #-3]"
        );
        assert_eq!(
            format_score_text(327.47, Some(3), ScoreFormat::Plain),
            "+327.47"
        );
        assert_eq!(
            format_score_text(-0.001, None, ScoreFormat::Plain),
            "+0.00",
            "no negative zero"
        );
    }

    #[test]
    fn a_written_eval_reads_back_with_the_core_parser() {
        let text = format_score_text(-1.8, None, ScoreFormat::Eval);
        let read = mychess_core::eval_of(&text).expect("eval");
        assert!((read + 1.8).abs() < 1e-6);
        let mate = format_score_text(327.0, Some(3), ScoreFormat::Eval);
        assert!(mychess_core::eval_of(&mate).expect("eval") > 100.0);
    }

    #[test]
    fn nag_boundaries_follow_the_scids_order() {
        assert_eq!(nag(3.0), Some(18));
        assert_eq!(nag(2.99), Some(16));
        assert_eq!(nag(1.5), Some(16));
        assert_eq!(nag(1.49), Some(14));
        assert_eq!(nag(0.5), Some(14));
        assert_eq!(nag(0.49), Some(10));
        assert_eq!(nag(0.0), Some(10));
        assert_eq!(nag(-0.49), Some(10));
        // The asymmetry: -0.50 is already `=` (the `>= -slight` test comes
        // first) and `-=` covers (-1.5, -0.5) only.
        assert_eq!(nag(-0.5), Some(10));
        assert_eq!(nag(-0.51), Some(15));
        assert_eq!(nag(-1.49), Some(15));
        assert_eq!(nag(-1.5), Some(17));
        assert_eq!(nag(-2.99), Some(17));
        assert_eq!(nag(-3.0), Some(19));
        assert_eq!(nag(327.0), Some(18));
        assert_eq!(nag(-327.0), Some(19));
    }

    #[test]
    fn append_tag_follows_the_scid() {
        assert_eq!(append_tag(None, " Stockfish "), "Stockfish");
        assert_eq!(append_tag(Some(""), "A"), "A");
        assert_eq!(append_tag(Some("A"), "B"), "A , B");
        assert_eq!(append_tag(Some("A , B"), "B"), "A , B");
        assert_eq!(
            append_tag(Some("Stockfish 16"), "Stockfish"),
            "Stockfish 16"
        );
    }

    #[test]
    fn a_position_without_moves_scores_by_check() {
        // Fool's mate: White is checkmated.
        let mated =
            Position::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3")
                .expect("fen");
        assert_eq!(PositionEval::no_moves(&mated).score, -NO_MOVES_PAWNS);
        let stalemate = Position::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").expect("fen");
        assert!(PositionEval::no_moves(&stalemate).score.abs() < f32::EPSILON);
    }

    #[test]
    fn mate_scores_keep_the_sign_of_white() {
        let eval = PositionEval::from_score(Score::Mate(-2), Vec::new(), 12);
        assert_eq!(eval.mate, Some(-2));
        assert!(eval.score < -327.0);
        assert_eq!(
            PositionEval::from_score(Score::Mate(0), Vec::new(), 1).mate,
            None
        );
        assert_eq!(
            PositionEval::from_score(Score::Cp(35), Vec::new(), 1).mate,
            None
        );
    }
}
