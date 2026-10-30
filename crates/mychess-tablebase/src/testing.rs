//! Stub sources for tests (behind the `testing` feature).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use mychess_core::Position;

use crate::category::{Category, Evaluation, MoveEntry, PositionReport, Source};
use crate::convert::is_zeroing;
use crate::error::{Result, TablebaseError};
use crate::probe::{BoxFuture, OnlineProbe, PositionProbe, check_applicable, position_key};

/// A source that answers from a table of prepared answers, keyed by
/// [`position_key`]. It implements both [`PositionProbe`] and [`OnlineProbe`].
#[derive(Debug)]
pub struct StubProbe {
    max_pieces: u8,
    answers: Mutex<HashMap<String, Result<PositionReport>>>,
    calls: AtomicUsize,
}

impl StubProbe {
    /// A stub that covers up to `max_pieces` pieces and knows no position.
    #[must_use]
    pub fn new(max_pieces: u8) -> StubProbe {
        StubProbe {
            max_pieces,
            answers: Mutex::new(HashMap::new()),
            calls: AtomicUsize::new(0),
        }
    }

    /// Prepares the answer for `position`.
    pub fn answer(&self, position: &Position, answer: Result<PositionReport>) {
        self.answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(position_key(position), answer);
    }

    /// How many times the stub was asked.
    #[must_use]
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn lookup(&self, position: &Position) -> Result<PositionReport> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        check_applicable(position, self.max_pieces)?;
        self.answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&position_key(position))
            .cloned()
            .unwrap_or_else(|| {
                Err(TablebaseError::MissingTable {
                    material: crate::probe::material_name(position),
                })
            })
    }
}

impl PositionProbe for StubProbe {
    fn probe(&self, position: &Position) -> Result<PositionReport> {
        self.lookup(position)
    }

    fn max_pieces(&self) -> u8 {
        self.max_pieces
    }
}

impl OnlineProbe for StubProbe {
    fn probe<'a>(&'a self, position: &'a Position) -> BoxFuture<'a, Result<PositionReport>> {
        Box::pin(async move { self.lookup(position) })
    }
}

/// Builds a report for `position`: `root` is the result for the side to
/// move and `moves` gives, by UCI, the result for the side that plays each
/// move (distance `dtz` in plies, if any). Moves not listed are `Unknown`;
/// an entry that is not a legal move is ignored.
#[must_use]
pub fn report(
    position: &Position,
    source: Source,
    root: Evaluation,
    moves: &[(&str, Category, Option<i32>)],
) -> PositionReport {
    let entries = position
        .legal_moves()
        .into_iter()
        .map(|mv| {
            let uci = position.uci(mv);
            let listed = moves.iter().find(|(text, _, _)| *text == uci);
            let evaluation = listed.map_or(Evaluation::UNKNOWN, |(_, category, dtz)| Evaluation {
                category: *category,
                dtz: *dtz,
                dtz_rounded: false,
                dtm: None,
            });
            MoveEntry {
                mv,
                san: position.san(mv).unwrap_or_else(|_| uci.clone()),
                uci,
                evaluation,
                zeroing: is_zeroing(mv),
                checkmate: false,
                stalemate: false,
                insufficient_material: false,
            }
        })
        .collect();
    let pieces = u8::try_from(position.pieces().count()).unwrap_or(u8::MAX);
    PositionReport::new(source, pieces, root, entries)
}
