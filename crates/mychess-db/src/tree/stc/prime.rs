// Derived from Scid vs PC 4.27, tcl/windows/tree.tcl (`::tree::prime`,
// `::tree::primeWithBase`, `::tree::primeWithGame`, `::tree::parseGame`,
// `::tree::parseVar` and the `tree(standardLines)` list). Copyright (c)
// 1999-2003 Shane Hudson; Scid vs PC contributors. Scid is distributed under
// the GNU General Public License (see its COPYING); this file is part of
// MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Priming the cache: running full tree searches over a list of positions so
//! the results are cached (decision D1051).
//!
//! The functions are pure over the database: they do not touch the game the
//! user is looking at. They do use the [`TreeState`] they are given (every
//! search leaves its tree filter there), so pass a scratch one when the
//! state of the open tree must stay as it is. Searches always scan the whole
//! database (no fast mode). A position the tree cannot hold (more than 60
//! moves) is skipped and counted.

use mychess_core::{Game, Position, SanMode};
use thiserror::Error;

use crate::filter::Filter;
use crate::progress::{CancelToken, NoProgress, ProgressSink};
use crate::source::GameSource;
use crate::tree::{
    CachedTree, FastMode, TreeCache, TreeError, TreeOptions, TreeQuery, TreeState, search,
};

/// The opening lines the Scid fills the cache with (`tree(standardLines)`):
/// the start position and the most common continuations, in SAN with move
/// numbers.
pub const STANDARD_LINES: &[&str] = &[
    "",
    "1.c4",
    "1.c4 c5",
    "1.c4 c5 2.Nf3",
    "1.c4 e5",
    "1.c4 Nf6",
    "1.c4 Nf6 2.Nc3",
    "1.d4",
    "1.d4 d5",
    "1.d4 d5 2.c4",
    "1.d4 d5 2.c4 c6",
    "1.d4 d5 2.c4 c6 3.Nf3",
    "1.d4 d5 2.c4 c6 3.Nf3 Nf6",
    "1.d4 d5 2.c4 c6 3.Nf3 Nf6 4.Nc3",
    "1.d4 d5 2.c4 c6 3.Nf3 Nf6 4.Nc3 dxc4",
    "1.d4 d5 2.c4 c6 3.Nf3 Nf6 4.Nc3 e6",
    "1.d4 d5 2.c4 c6 3.Nf3 Nf6 4.Nc3 e6 5.e3",
    "1.d4 d5 2.c4 e6",
    "1.d4 d5 2.c4 e6 3.Nc3",
    "1.d4 d5 2.c4 e6 3.Nc3 Nf6",
    "1.d4 d5 2.c4 e6 3.Nf3",
    "1.d4 d5 2.c4 dxc4",
    "1.d4 d5 2.c4 dxc4 3.Nf3",
    "1.d4 d5 2.c4 dxc4 3.Nf3 Nf6",
    "1.d4 d5 2.Nf3",
    "1.d4 d5 2.Nf3 Nf6",
    "1.d4 d5 2.Nf3 Nf6 3.c4",
    "1.d4 d6",
    "1.d4 d6 2.c4",
    "1.d4 Nf6",
    "1.d4 Nf6 2.c4",
    "1.d4 Nf6 2.c4 c5",
    "1.d4 Nf6 2.c4 d6",
    "1.d4 Nf6 2.c4 e6",
    "1.d4 Nf6 2.c4 e6 3.Nc3",
    "1.d4 Nf6 2.c4 e6 3.Nc3 Bb4",
    "1.d4 Nf6 2.c4 e6 3.Nf3",
    "1.d4 Nf6 2.c4 g6",
    "1.d4 Nf6 2.c4 g6 3.Nc3",
    "1.d4 Nf6 2.c4 g6 3.Nc3 Bg7",
    "1.d4 Nf6 2.c4 g6 3.Nc3 Bg7 4.e4",
    "1.d4 Nf6 2.c4 g6 3.Nc3 Bg7 4.e4 d6",
    "1.d4 Nf6 2.c4 g6 3.Nc3 Bg7 4.e4 d6 5.Nf3",
    "1.d4 Nf6 2.c4 g6 3.Nc3 Bg7 4.e4 d6 5.Nf3 O-O",
    "1.d4 Nf6 2.c4 g6 3.Nc3 Bg7 4.e4 d6 5.Nf3 O-O 6.Be2",
    "1.d4 Nf6 2.c4 g6 3.Nf3",
    "1.d4 Nf6 2.Bg5",
    "1.d4 Nf6 2.Bg5 Ne4",
    "1.d4 Nf6 2.Nf3",
    "1.d4 Nf6 2.Nf3 e6",
    "1.d4 Nf6 2.Nf3 g6",
    "1.e4",
    "1.e4 c5",
    "1.e4 c5 2.c3",
    "1.e4 c5 2.c3 d5",
    "1.e4 c5 2.c3 Nf6",
    "1.e4 c5 2.Nc3",
    "1.e4 c5 2.Nc3 Nc6",
    "1.e4 c5 2.Nf3",
    "1.e4 c5 2.Nf3 d6",
    "1.e4 c5 2.Nf3 d6 3.d4",
    "1.e4 c5 2.Nf3 d6 3.d4 cxd4",
    "1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4",
    "1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4 Nf6",
    "1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4 Nf6 5.Nc3",
    "1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4 Nf6 5.Nc3 a6",
    "1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4 Nf6 5.Nc3 e6",
    "1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4 Nf6 5.Nc3 g6",
    "1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4 Nf6 5.Nc3 Nc6",
    "1.e4 c5 2.Nf3 d6 3.Bb5+",
    "1.e4 c5 2.Nf3 e6",
    "1.e4 c5 2.Nf3 Nc6",
    "1.e4 c5 2.Nf3 Nc6 3.d4",
    "1.e4 c5 2.Nf3 Nc6 3.Bb5",
    "1.e4 c6",
    "1.e4 c6 2.d4",
    "1.e4 c6 2.d4 d5",
    "1.e4 c6 2.d4 d5 3.e5",
    "1.e4 c6 2.d4 d5 3.Nc3",
    "1.e4 c6 2.d4 d5 3.Nd2",
    "1.e4 d5",
    "1.e4 d6",
    "1.e4 d6 2.d4",
    "1.e4 d6 2.d4 Nf6",
    "1.e4 d6 2.d4 Nf6 3.Nc3",
    "1.e4 e5",
    "1.e4 e5 2.Nf3",
    "1.e4 e5 2.Nf3 Nc6",
    "1.e4 e5 2.Nf3 Nc6 3.d4",
    "1.e4 e5 2.Nf3 Nc6 3.Bb5",
    "1.e4 e5 2.Nf3 Nc6 3.Bb5 a6",
    "1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 4.Ba4",
    "1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 4.Ba4 Nf6",
    "1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 4.Ba4 Nf6 5.O-O",
    "1.e4 e5 2.Nf3 Nc6 3.Bc4",
    "1.e4 e5 2.Nf3 Nf6",
    "1.e4 e6",
    "1.e4 e6 2.d4",
    "1.e4 e6 2.d4 d5",
    "1.e4 e6 2.d4 d5 3.Nc3",
    "1.e4 e6 2.d4 d5 3.Nc3 Bb4",
    "1.e4 e6 2.d4 d5 3.Nc3 Nf6",
    "1.e4 e6 2.d4 d5 3.Nd2",
    "1.e4 e6 2.d4 d5 3.Nd2 c5",
    "1.e4 e6 2.d4 d5 3.Nd2 Nf6",
    "1.e4 Nf6",
    "1.e4 Nf6 2.e5",
    "1.e4 Nf6 2.e5 Nd5",
    "1.Nf3",
    "1.Nf3 Nf6",
];

/// Errors of the priming functions.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PrimeError {
    /// The token was cancelled; the searches already done stay in the cache.
    /// The partial [`PrimeReport`] is not returned (known limitation: the
    /// variant carries no data so the match sites stay compatible).
    #[error("priming cancelled")]
    Cancelled,
    /// The database has no SI4 files, so it cannot have a cache file.
    #[error("only Scid-format databases can have a tree cache file")]
    NotScidBase,
    /// A line has a move that is not legal. Nothing was searched.
    #[error("illegal move {token:?} in line {line:?}")]
    IllegalMove {
        /// The line.
        line: String,
        /// The offending token.
        token: String,
    },
    /// The filter does not cover the games of the database.
    #[error("filter has {filter} games but the database has {games}")]
    FilterSize {
        /// Length of the filter.
        filter: u32,
        /// Number of games of the database.
        games: u32,
    },
    /// A search failed.
    #[error(transparent)]
    Tree(#[from] TreeError),
}

/// What a priming run did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrimeReport {
    /// Searches that finished.
    pub searches: u32,
    /// Searches whose position was already in the cache (cheap).
    pub cache_hits: u32,
    /// Calls to the `store` of the cache (a search that was not a hit). The
    /// cache may keep fewer trees: it rejects one with a total below its
    /// lowest when full, and a tree already there is not duplicated. The name
    /// is historical; this is not the number of trees inserted.
    pub stored: u32,
    /// Positions skipped because the tree cannot hold them, and games that
    /// could not be read ([`prime_with_filter`]).
    pub skipped: u32,
}

/// Counts what goes through the cache.
struct Counting<'a> {
    inner: &'a mut dyn TreeCache,
    hits: u32,
    stores: u32,
}

impl TreeCache for Counting<'_> {
    fn lookup(&mut self, pos: &Position, len: u32) -> Option<CachedTree> {
        let found = self.inner.lookup(pos, len);
        if found.is_some() {
            self.hits = self.hits.saturating_add(1);
        }
        found
    }

    fn store(&mut self, pos: &Position, tree: CachedTree) {
        self.stores = self.stores.saturating_add(1);
        self.inner.store(pos, tree);
    }
}

struct Primer<'a, D: GameSource + ?Sized> {
    db: &'a mut D,
    state: &'a mut TreeState,
    cache: Counting<'a>,
    cancel: &'a CancelToken,
    main_filter: Filter,
    report: PrimeReport,
}

impl<'a, D: GameSource + ?Sized> Primer<'a, D> {
    fn new(
        db: &'a mut D,
        state: &'a mut TreeState,
        cache: &'a mut dyn TreeCache,
        cancel: &'a CancelToken,
    ) -> Result<Primer<'a, D>, PrimeError> {
        if !db.is_scid_database() {
            return Err(PrimeError::NotScidBase);
        }
        let main_filter = Filter::new(db.len());
        Ok(Primer {
            db,
            state,
            cache: Counting {
                inner: cache,
                hits: 0,
                stores: 0,
            },
            cancel,
            main_filter,
            report: PrimeReport::default(),
        })
    }

    /// One full search of `pos`, stored in the cache by the engine.
    fn step(&mut self, pos: &Position) -> Result<(), PrimeError> {
        if self.cancel.is_cancelled() {
            return Err(PrimeError::Cancelled);
        }
        let query = TreeQuery {
            position: pos,
            ply: 0,
            main_filter: &self.main_filter,
        };
        let opts = TreeOptions {
            fast: FastMode::Slow,
            ..TreeOptions::default()
        };
        match search(
            &mut *self.db,
            &query,
            &opts,
            &mut *self.state,
            &mut self.cache,
            self.cancel,
            &mut NoProgress,
        ) {
            Ok(_) => self.report.searches = self.report.searches.saturating_add(1),
            Err(TreeError::Cancelled) => return Err(PrimeError::Cancelled),
            Err(TreeError::TooManyMoves | TreeError::UnsupportedPosition) => {
                self.report.skipped = self.report.skipped.saturating_add(1);
            }
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }

    /// Searches the start position and the position after every move of the
    /// main line and of all the variations, at any depth (`parseGame` and
    /// `parseVar`). Iterative, so no depth limit applies.
    fn game(&mut self, game: &Game) -> Result<(), PrimeError> {
        self.step(game.start_position())?;
        if let Ok(nodes) = game.descendants(game.root()) {
            for id in nodes {
                if let Ok(pos) = game.position_at(id) {
                    self.step(pos)?;
                }
            }
        }
        Ok(())
    }

    fn finish(self) -> PrimeReport {
        PrimeReport {
            cache_hits: self.cache.hits,
            stored: self.cache.stores,
            ..self.report
        }
    }
}

/// The position after the moves of `line` (SAN with or without move
/// numbers); an empty line is the start position.
fn position_of_line(line: &str) -> Result<Position, PrimeError> {
    let mut pos = Position::new();
    for token in line.split_whitespace() {
        // Moves never start with a digit: this drops "12." and "12...".
        let san = token
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .trim_start_matches('.');
        if san.is_empty() {
            continue;
        }
        let illegal = || PrimeError::IllegalMove {
            line: line.to_owned(),
            token: token.to_owned(),
        };
        let parsed = pos
            .parse_san(san, SanMode::Tolerant)
            .map_err(|_| illegal())?;
        pos.play(parsed.mv).map_err(|_| illegal())?;
    }
    Ok(pos)
}

/// Fills the cache with a full search of the position at the end of each line
/// (`::tree::prime`; pass [`STANDARD_LINES`] for the Scid's list). Progress is
/// reported per line.
///
/// # Errors
///
/// [`PrimeError::NotScidBase`], [`PrimeError::IllegalMove`] (before any
/// search), [`PrimeError::Cancelled`] and [`PrimeError::Tree`].
pub fn prime_with_lines<D: GameSource + ?Sized>(
    db: &mut D,
    state: &mut TreeState,
    cache: &mut dyn TreeCache,
    lines: &[&str],
    cancel: &CancelToken,
    progress: &mut dyn ProgressSink,
) -> Result<PrimeReport, PrimeError> {
    let mut primer = Primer::new(db, state, cache, cancel)?;
    let positions = lines
        .iter()
        .map(|line| position_of_line(line))
        .collect::<Result<Vec<_>, _>>()?;
    let total = u64::try_from(positions.len()).unwrap_or(u64::MAX);
    for (i, pos) in positions.iter().enumerate() {
        primer.step(pos)?;
        progress.progress(u64::try_from(i + 1).unwrap_or(u64::MAX), total);
    }
    Ok(primer.finish())
}

/// Fills the cache with a full search of every position of `game`: the start,
/// and after each move of the main line and of the variations, recursively
/// (`::tree::primeWithGame`). Progress counts the nodes of the game.
///
/// # Errors
///
/// [`PrimeError::NotScidBase`], [`PrimeError::Cancelled`] and
/// [`PrimeError::Tree`].
pub fn prime_with_game<D: GameSource + ?Sized>(
    db: &mut D,
    state: &mut TreeState,
    cache: &mut dyn TreeCache,
    game: &Game,
    cancel: &CancelToken,
    progress: &mut dyn ProgressSink,
) -> Result<PrimeReport, PrimeError> {
    let mut primer = Primer::new(db, state, cache, cancel)?;
    primer.game(game)?;
    let total = u64::try_from(game.node_count()).unwrap_or(u64::MAX);
    progress.progress(total, total);
    Ok(primer.finish())
}

/// Fills the cache with every position of every game in `filter`
/// (`::tree::primeWithBase`). Progress is reported per game. A game that
/// cannot be read is skipped and counted.
///
/// # Errors
///
/// [`PrimeError::NotScidBase`], [`PrimeError::FilterSize`],
/// [`PrimeError::Cancelled`] and [`PrimeError::Tree`].
pub fn prime_with_filter<D: GameSource + ?Sized>(
    db: &mut D,
    state: &mut TreeState,
    cache: &mut dyn TreeCache,
    filter: &Filter,
    cancel: &CancelToken,
    progress: &mut dyn ProgressSink,
) -> Result<PrimeReport, PrimeError> {
    let mut primer = Primer::new(db, state, cache, cancel)?;
    if filter.len() != primer.db.len() {
        return Err(PrimeError::FilterSize {
            filter: filter.len(),
            games: primer.db.len(),
        });
    }
    let total = u64::from(filter.count());
    let mut done = 0u64;
    for n in filter.iter_games() {
        if primer.cancel.is_cancelled() {
            return Err(PrimeError::Cancelled);
        }
        match primer.db.game(n) {
            Ok(game) => primer.game(&game)?,
            Err(_) => primer.report.skipped = primer.report.skipped.saturating_add(1),
        }
        done += 1;
        progress.progress(done, total);
    }
    Ok(primer.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_lines_are_all_legal_and_distinct() {
        assert_eq!(STANDARD_LINES.len(), 110);
        let mut seen = std::collections::HashSet::new();
        for line in STANDARD_LINES {
            let pos = position_of_line(line).expect("legal line");
            assert!(seen.insert((
                pos.turn(),
                pos.pieces().collect::<Vec<_>>().len(),
                pos.fen()
            )));
        }
        assert_eq!(
            position_of_line("").map(|p| p.fen()).ok(),
            Some(Position::new().fen())
        );
    }

    #[test]
    fn castling_and_numbers_are_read() {
        let pos = position_of_line("1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 4.Ba4 Nf6 5.O-O").expect("legal");
        assert_eq!(pos.turn(), mychess_core::Color::Black);
        assert!(matches!(
            position_of_line("1.e4 e4"),
            Err(PrimeError::IllegalMove { .. })
        ));
    }

    #[test]
    fn pgn_databases_are_refused_and_bad_lines_search_nothing() {
        use crate::pgn_db::{OpenOptions, PgnDatabase};
        let pgn = "[Event \"?\"]\n[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n";
        let (mut db, _) = PgnDatabase::from_reader(
            pgn.as_bytes(),
            std::path::Path::new("t.pgn"),
            &OpenOptions::default(),
            &mut NoProgress,
            &CancelToken::new(),
        )
        .expect("pgn opens");
        let mut cache = crate::tree::NoCache;
        let result = prime_with_lines(
            &mut db,
            &mut TreeState::default(),
            &mut cache,
            &["1.e4"],
            &CancelToken::new(),
            &mut NoProgress,
        );
        assert!(matches!(result, Err(PrimeError::NotScidBase)));
    }
}
