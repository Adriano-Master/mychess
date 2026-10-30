//! The tablebase of the tournament (spec M8-2, D2520 and D2538-D2540): the
//! adapter from the local Syzygy tables of `mychess-tablebase` to the
//! adjudication hook [`TablebaseProbe`] (D1277).
//!
//! The tables are opened by the load of the tournament (`perform`) and held
//! by the tournament until it is dropped, apart from the set the tablebase
//! panel holds: the panel lets go of its files when it closes (D2584), and
//! the tournament must not lose its probe in the middle of a game. The probe
//! itself asks for the position only (no list of moves): one short read per
//! table, and at most once per game in practice, since a decided position
//! ends the game.

use std::fmt;
use std::sync::Arc;

use mychess_core::Position;
use mychess_tablebase::{Category, LocalTablebase, PositionProbe};

use super::adjudicate::{TablebaseProbe, TbOutcome};

/// The local tables opened for a tournament (D2538). Cheap to clone and
/// compared by identity.
#[derive(Clone)]
pub struct TournamentTablebase(Arc<LocalTablebase>);

impl TournamentTablebase {
    /// Wraps an opened set of tables.
    #[must_use]
    pub fn new(tables: LocalTablebase) -> TournamentTablebase {
        TournamentTablebase(Arc::new(tables))
    }

    /// Whether the set has no table at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for TournamentTablebase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TournamentTablebase")
            .field("max_pieces", &self.0.max_pieces())
            .finish()
    }
}

impl PartialEq for TournamentTablebase {
    fn eq(&self, other: &TournamentTablebase) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl TablebaseProbe for TournamentTablebase {
    fn max_pieces(&self) -> u32 {
        u32::from(self.0.max_pieces())
    }

    fn probe(&self, position: &Position) -> Option<TbOutcome> {
        // A position the tables do not cover (missing file, castling rights,
        // too many pieces) is simply not decided.
        let evaluation = self.0.evaluate(position).ok()?;
        outcome_of(evaluation.category)
    }
}

/// The adjudication outcome of a tablebase category (D2539). The category
/// already counts the 50-move clock of the position; the two uncertain
/// categories (a DTZ rounded at the edge of the rule) decide nothing, and the
/// game goes on until a clear position.
#[must_use]
pub fn outcome_of(category: Category) -> Option<TbOutcome> {
    match category {
        Category::Win => Some(TbOutcome::Win),
        Category::CursedWin => Some(TbOutcome::CursedWin),
        Category::Draw => Some(TbOutcome::Draw),
        Category::BlessedLoss => Some(TbOutcome::BlessedLoss),
        Category::Loss => Some(TbOutcome::Loss),
        // `Unknown` and whatever a later crate adds decide nothing either.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_map_to_outcomes_and_the_uncertain_ones_decide_nothing() {
        assert_eq!(outcome_of(Category::Win), Some(TbOutcome::Win));
        assert_eq!(outcome_of(Category::CursedWin), Some(TbOutcome::CursedWin));
        assert_eq!(outcome_of(Category::Draw), Some(TbOutcome::Draw));
        assert_eq!(
            outcome_of(Category::BlessedLoss),
            Some(TbOutcome::BlessedLoss)
        );
        assert_eq!(outcome_of(Category::Loss), Some(TbOutcome::Loss));
        assert_eq!(outcome_of(Category::MaybeWin), None);
        assert_eq!(outcome_of(Category::MaybeLoss), None);
        assert_eq!(outcome_of(Category::Unknown), None);
    }

    #[test]
    fn an_empty_set_knows_no_pieces_and_decides_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (tables, _report) =
            LocalTablebase::open(&[dir.path().to_path_buf()]).expect("an empty folder opens");
        let tb = TournamentTablebase::new(tables);
        assert!(tb.is_empty());
        assert_eq!(TablebaseProbe::max_pieces(&tb), 0);
        let krk = Position::from_fen("8/8/8/8/8/4k3/8/R3K3 w - - 0 1").expect("fen");
        assert_eq!(tb.probe(&krk), None);
        assert_eq!(tb.probe(&Position::new()), None);
        assert_eq!(tb.clone(), tb, "compared by identity");
    }
}
