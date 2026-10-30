//! Types shared by every engine protocol: scores, search statistics,
//! capabilities and identity.
//!
//! Scores are always from White's point of view (spec m4-1, D405), whatever
//! the protocol or the side to move: the interface, the evaluation bar and the
//! annotation code can then use one convention.

use std::ops::RangeInclusive;

use mychess_core::Move;

use crate::option::EngineOption;

/// The engine protocol spoken by an engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Protocol {
    /// Universal Chess Interface.
    #[default]
    Uci,
    /// XBoard / WinBoard (CECP). Implemented by milestone M4, slice 2.
    XBoard,
}

/// Identifies one search (one `go`) of an engine session.
///
/// Ids grow with each search and every search-related event carries one, so a
/// consumer can discard events of a search it already replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct SearchId(pub u64);

/// The value the interface uses for a mate score when ordering lines: mate in
/// `n` is `32_750 - n` centipawns (the constant of the Scid, `uci.tcl:225`).
pub const MATE_CENTIPAWNS: i32 = 32_750;

/// An engine evaluation from White's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Score {
    /// Centipawns; positive favours White.
    Cp(i32),
    /// Mate in `n` moves (full moves); positive: White mates, negative: Black
    /// mates.
    Mate(i32),
}

impl Score {
    /// A single number for ordering and bars: a mate in `n` is
    /// `±(32_750 - |n|)`, so a mate in 1 beats a mate in 3, which beats any
    /// centipawn value.
    ///
    /// ```
    /// use mychess_engine::Score;
    ///
    /// assert_eq!(Score::Cp(-34).as_centipawns(), -34);
    /// assert_eq!(Score::Mate(3).as_centipawns(), 32_747);
    /// assert!(Score::Mate(1).as_centipawns() > Score::Mate(3).as_centipawns());
    /// assert_eq!(Score::Mate(-2).as_centipawns(), -32_748);
    /// ```
    pub fn as_centipawns(self) -> i32 {
        match self {
            Score::Cp(cp) => cp,
            Score::Mate(n) => {
                // Keeps the sign even for absurd distances (|n| > 32750).
                let magnitude = MATE_CENTIPAWNS.saturating_sub(n.saturating_abs()).max(1);
                if n < 0 { -magnitude } else { magnitude }
            }
        }
    }

    /// The score in pawns (`cp / 100`; mate scores use [`Score::as_centipawns`]).
    #[allow(clippy::cast_precision_loss)]
    pub fn pawns(self) -> f32 {
        self.as_centipawns() as f32 / 100.0
    }

    /// The same score seen from the other side.
    pub(crate) fn negated(self) -> Score {
        match self {
            Score::Cp(cp) => Score::Cp(cp.saturating_neg()),
            Score::Mate(n) => Score::Mate(n.saturating_neg()),
        }
    }
}

/// Whether a score is exact or only a bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Bound {
    /// The exact value.
    #[default]
    Exact,
    /// The true value is at least this good for White.
    Lower,
    /// The true value is at most this good for White.
    Upper,
}

impl Bound {
    /// The bound seen from the other side.
    pub(crate) fn flipped(self) -> Bound {
        match self {
            Bound::Exact => Bound::Exact,
            Bound::Lower => Bound::Upper,
            Bound::Upper => Bound::Lower,
        }
    }
}

/// Win/draw/loss expectation in per mille, from White's point of view. The
/// three values add up to 1000.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Wdl {
    /// White wins.
    pub win: u16,
    /// Draw.
    pub draw: u16,
    /// White loses.
    pub loss: u16,
}

/// One analysis report of an engine (a UCI `info` line), already converted:
/// score from White's point of view, moves validated against the searched
/// position.
///
/// Every statistic is optional: a line without `score` or without `pv` still
/// carries depth, node counts and so on. A consumer replacing a MultiPV line
/// should do so only when both `score` and a non-empty `pv` are present.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchInfo {
    /// The search this report belongs to.
    pub search: SearchId,
    /// The MultiPV line number, starting at 1.
    pub multipv: u8,
    /// Search depth in plies.
    pub depth: Option<u32>,
    /// Selective search depth.
    pub seldepth: Option<u32>,
    /// Time spent, in milliseconds.
    pub time_ms: Option<u64>,
    /// Nodes searched.
    pub nodes: Option<u64>,
    /// Nodes per second.
    pub nps: Option<u64>,
    /// Hash table fill, per mille.
    pub hashfull_permille: Option<u16>,
    /// Engine CPU load, per mille.
    pub cpuload_permille: Option<u16>,
    /// Tablebase hits.
    pub tbhits: Option<u64>,
    /// The evaluation and whether it is exact or a bound.
    pub score: Option<(Score, Bound)>,
    /// Win/draw/loss expectation (`UCI_ShowWDL`).
    pub wdl: Option<Wdl>,
    /// The principal variation, validated move by move from the searched
    /// position.
    pub pv: Vec<Move>,
    /// `true` when the engine's PV had a move that is illegal in the searched
    /// position; `pv` holds the moves before it.
    pub pv_truncated: bool,
    /// The move being examined.
    pub currmove: Option<Move>,
    /// The number of that move in the root move list.
    pub currmovenumber: Option<u32>,
    /// Free text (`info string`), meant for the log.
    pub string: Option<String>,
}

/// What an engine can do, derived from the options it announces.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Capabilities {
    /// The `MultiPV` range, `None` when the engine has no such option.
    pub multipv: Option<RangeInclusive<u8>>,
    /// Whether `searchmoves` can be sent (always true for UCI; an engine that
    /// ignores it cannot be detected).
    pub search_moves: bool,
    /// The engine announces `UCI_Chess960`.
    pub chess960: bool,
    /// The engine announces `UCI_AnalyseMode`.
    pub analyse_mode: bool,
    /// The engine announces `UCI_ShowWDL`.
    pub wdl: bool,
    /// The engine announces `Ponder`.
    pub ponder: bool,
}

/// Who the engine says it is, and what it offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineIdentity {
    /// `id name`.
    pub name: String,
    /// `id author`.
    pub author: Option<String>,
    /// The announced options, in the engine's order (duplicates removed, the
    /// last announcement wins).
    pub options: Vec<EngineOption>,
    /// What the options say the engine can do.
    pub capabilities: Capabilities,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mate_scores_order_above_centipawns() {
        assert!(Score::Mate(30).as_centipawns() > Score::Cp(5000).as_centipawns());
        assert!(Score::Mate(-30).as_centipawns() < Score::Cp(-5000).as_centipawns());
    }

    #[test]
    fn absurd_mate_distances_keep_their_sign() {
        assert_eq!(Score::Mate(40_000).as_centipawns(), 1);
        assert_eq!(Score::Mate(-40_000).as_centipawns(), -1);
        assert_eq!(Score::Mate(i32::MAX).as_centipawns(), 1);
    }

    #[test]
    fn negation_flips_scores_and_bounds() {
        assert_eq!(Score::Cp(12).negated(), Score::Cp(-12));
        assert_eq!(Score::Mate(-3).negated(), Score::Mate(3));
        assert_eq!(Bound::Lower.flipped(), Bound::Upper);
        assert_eq!(Bound::Exact.flipped(), Bound::Exact);
    }

    #[test]
    fn extreme_values_do_not_overflow() {
        assert_eq!(Score::Cp(i32::MIN).negated(), Score::Cp(i32::MAX));
        let _ = Score::Mate(i32::MIN).as_centipawns();
    }
}
