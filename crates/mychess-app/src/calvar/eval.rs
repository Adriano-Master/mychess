//! Evaluations from the engine, as the exercise needs them (spec M8-6,
//! D2308, D2311).

use mychess_core::{Color, Move, Position};
use mychess_engine::Score;

use super::line::Assessment;

/// Centipawns of a mate that is already on the board (D2311).
pub const MATE_CP: i32 = 32_750;

/// `+-` from this many centipawns on (D2311).
const WINNING_CP: i32 = 300;
/// `+/-` from this many centipawns on.
const BETTER_CP: i32 = 100;
/// `+=` from this many centipawns on; `=` inside.
const SLIGHT_CP: i32 = 25;

/// The evaluation of a position, from the point of view of White.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Eval {
    /// Centipawns; a mate is `±(32750 - distance)` (D2311).
    pub cp: i32,
    /// The mate distance in moves, positive when White mates, if it is a
    /// mate.
    pub mate_in: Option<i32>,
}

impl Eval {
    /// An evaluation in centipawns.
    pub fn centipawns(cp: i32) -> Eval {
        Eval { cp, mate_in: None }
    }

    /// The evaluation that an engine reported.
    pub fn from_score(score: Score) -> Eval {
        Eval {
            cp: score.as_centipawns(),
            mate_in: match score {
                Score::Mate(n) => Some(n),
                Score::Cp(_) => None,
            },
        }
    }

    /// The evaluation of a position that ends the game: `Some` for mate and
    /// stalemate, `None` while there are moves.
    pub fn terminal(position: &Position) -> Option<Eval> {
        if position.is_checkmate() {
            // The side to move is mated.
            let cp = match position.turn() {
                Color::White => -MATE_CP,
                Color::Black => MATE_CP,
            };
            return Some(Eval {
                cp,
                mate_in: Some(0),
            });
        }
        position
            .legal_moves()
            .is_empty()
            .then(|| Eval::centipawns(0))
    }

    /// The same evaluation seen from `color`.
    pub fn for_mover(self, color: Color) -> i32 {
        match color {
            Color::White => self.cp,
            Color::Black => self.cp.saturating_neg(),
        }
    }

    /// Whether it is a mate score (one that no centipawn tolerance applies
    /// to).
    pub fn is_mate(self) -> bool {
        self.mate_in.is_some() || self.cp.unsigned_abs() >= (MATE_CP - 1_000).unsigned_abs()
    }

    /// The evaluation as text for a comment or a list: `+0.31`, `-1.20`,
    /// `#3` (White mates in 3), `#-2`; the same in every language.
    pub fn label(self) -> String {
        match self.mate_in {
            Some(n) if self.cp >= 0 => format!("#{}", n.unsigned_abs()),
            Some(n) => format!("#-{}", n.unsigned_abs()),
            None => {
                let sign = if self.cp < 0 { '-' } else { '+' };
                let abs = self.cp.unsigned_abs();
                format!("{sign}{}.{:02}", abs / 100, abs % 100)
            }
        }
    }

    /// The evaluation as the symbols of the exercise (D2311).
    pub fn assessment(self) -> Assessment {
        let cp = self.cp;
        if cp >= WINNING_CP {
            Assessment::WhiteWinning
        } else if cp >= BETTER_CP {
            Assessment::WhiteBetter
        } else if cp >= SLIGHT_CP {
            Assessment::WhiteSlightly
        } else if cp > -SLIGHT_CP {
            Assessment::Equal
        } else if cp > -BETTER_CP {
            Assessment::BlackSlightly
        } else if cp > -WINNING_CP {
            Assessment::BlackBetter
        } else {
            Assessment::BlackWinning
        }
    }
}

/// One line of the first stage: the MultiPV report of the engine for the
/// start position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootLine {
    /// The MultiPV number, from 1.
    pub multipv: u8,
    /// The depth reached.
    pub depth: u32,
    /// The evaluation.
    pub eval: Eval,
    /// The variation, starting with the move of this line.
    pub pv: Vec<Move>,
}

/// What a search of the second stage found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// The evaluation of the position that was searched.
    pub eval: Eval,
    /// The depth reached (0 for a position that ends the game).
    pub depth: u32,
    /// The best play from that position.
    pub pv: Vec<Move>,
}

/// The two searches of one line (D2308).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LineProbes {
    /// After the first move of the line.
    pub first: Option<Probe>,
    /// At the end of the line (`None` for a line of one move: it is `first`).
    pub end: Option<Probe>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_classes_follow_the_table_of_the_spec() {
        let class = |cp| Eval::centipawns(cp).assessment();
        assert_eq!(class(0), Assessment::Equal);
        assert_eq!(class(24), Assessment::Equal);
        assert_eq!(class(25), Assessment::WhiteSlightly);
        assert_eq!(class(99), Assessment::WhiteSlightly);
        assert_eq!(class(100), Assessment::WhiteBetter);
        assert_eq!(class(299), Assessment::WhiteBetter);
        assert_eq!(class(300), Assessment::WhiteWinning);
        assert_eq!(class(-24), Assessment::Equal);
        assert_eq!(class(-25), Assessment::BlackSlightly);
        assert_eq!(class(-100), Assessment::BlackBetter);
        assert_eq!(class(-300), Assessment::BlackWinning);
        assert_eq!(
            Eval::from_score(Score::Mate(-3)).assessment(),
            Assessment::BlackWinning
        );
    }

    #[test]
    fn labels_are_the_same_in_every_language() {
        assert_eq!(Eval::centipawns(31).label(), "+0.31");
        assert_eq!(Eval::centipawns(-5).label(), "-0.05");
        assert_eq!(Eval::centipawns(-120).label(), "-1.20");
        assert_eq!(Eval::from_score(Score::Mate(3)).label(), "#3");
        assert_eq!(Eval::from_score(Score::Mate(-2)).label(), "#-2");
        assert!(Eval::centipawns(i32::MIN).label().starts_with('-'));
    }

    #[test]
    fn a_mate_on_the_board_is_a_mate_for_the_other_side() {
        let mated = Position::from_fen("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1").expect("fen");
        let eval = Eval::terminal(&mated).expect("mate");
        assert_eq!(eval.cp, MATE_CP);
        assert!(eval.is_mate());
        assert_eq!(eval.for_mover(Color::Black), -MATE_CP);
        let stale = Position::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").expect("fen");
        assert_eq!(Eval::terminal(&stale), Some(Eval::centipawns(0)));
        assert_eq!(Eval::terminal(&Position::new()), None);
    }

    #[test]
    fn the_extreme_score_of_a_file_does_not_overflow() {
        let eval = Eval {
            cp: i32::MIN,
            mate_in: None,
        };
        assert!(eval.is_mate());
        assert!(!Eval::centipawns(0).is_mate());
    }
}
