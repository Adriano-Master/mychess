//! The configuration of a tactics session: what the dialog edits (spec
//! M8-4, D1900-D1927).

use std::path::PathBuf;

/// The two ways to train.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum TacticsMode {
    /// "Mate in N": the solver judges every move (D1906-D1912).
    #[default]
    Mate,
    /// "Find the best move": the move of the game (D1913).
    BestMove,
}

impl TacticsMode {
    /// The spelling used in `tactics.toml`.
    pub fn as_str(self) -> &'static str {
        match self {
            TacticsMode::Mate => "mate",
            TacticsMode::BestMove => "best",
        }
    }

    /// Reads the spelling of [`TacticsMode::as_str`].
    pub fn from_name(text: &str) -> Option<TacticsMode> {
        match text {
            "mate" => Some(TacticsMode::Mate),
            "best" => Some(TacticsMode::BestMove),
            _ => None,
        }
    }
}

/// The order of the problems (D1915).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum TacticsOrder {
    /// From the first game on.
    #[default]
    Sequential,
    /// A walk with a step that is coprime with the number of games, from a
    /// start that the seed chooses.
    Random,
}

impl TacticsOrder {
    /// The spelling used in `tactics.toml`.
    pub fn as_str(self) -> &'static str {
        match self {
            TacticsOrder::Sequential => "sequential",
            TacticsOrder::Random => "random",
        }
    }

    /// Reads the spelling of [`TacticsOrder::as_str`].
    pub fn from_name(text: &str) -> Option<TacticsOrder> {
        match text {
            "sequential" => Some(TacticsOrder::Sequential),
            "random" => Some(TacticsOrder::Random),
            _ => None,
        }
    }
}

/// Where the problems come from (D1901).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum SourceKind {
    /// A PGN file chosen by the user.
    #[default]
    File,
    /// The games of the filter of the current database.
    CurrentBase,
}

impl SourceKind {
    /// The spelling used in `tactics.toml`.
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::File => "file",
            SourceKind::CurrentBase => "base",
        }
    }

    /// Reads the spelling of [`SourceKind::as_str`].
    pub fn from_name(text: &str) -> Option<SourceKind> {
        match text {
            "file" => Some(SourceKind::File),
            "base" => Some(SourceKind::CurrentBase),
            _ => None,
        }
    }
}

/// Longest mate that a session asks for (D1909).
pub const MATE_RANGE: (u8, u8) = (1, 5);
/// Attempts per problem (D1918).
pub const ATTEMPTS_RANGE: (u8, u8) = (1, 9);
/// Time limit of a problem in seconds, 0 meaning none (D1925).
pub const TIME_LIMIT_RANGE: (u16, u16) = (5, 600);
/// The difficulty of a `****D` marker (D1902).
pub const DIFFICULTY_RANGE: (u8, u8) = (1, 9);
/// Largest Elo that a filter accepts.
pub const MAX_FILTER_ELO: u16 = 4000;

/// What the dialog edits (D1900-D1927). [`TacticsConfig::sanitized`] brings
/// every field into its range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TacticsConfig {
    /// Mate in N, or the best move.
    pub mode: TacticsMode,
    /// Sequential or random.
    pub order: TacticsOrder,
    /// A file or the current base.
    pub source: SourceKind,
    /// The PGN file, when the source is a file.
    pub file: Option<PathBuf>,
    /// Shortest mate asked for (Mate in N only).
    pub mate_min: u8,
    /// Longest mate asked for (Mate in N only).
    pub mate_max: u8,
    /// Easiest `****D` difficulty accepted, 0 for no filter.
    pub difficulty_min: u8,
    /// Hardest `****D` difficulty accepted, 0 for no filter.
    pub difficulty_max: u8,
    /// Lowest Elo of the problem accepted, 0 for no filter.
    pub elo_min: u16,
    /// Highest Elo of the problem accepted, 0 for no filter.
    pub elo_max: u16,
    /// Show the problems that were solved before.
    pub include_solved: bool,
    /// Wrong moves allowed before the problem is lost.
    pub max_attempts: u8,
    /// Whether the hint button works.
    pub hints: bool,
    /// Accept a mate that is up to two moves longer than the shortest
    /// (D1911).
    pub accept_longer: bool,
    /// Seconds to solve a problem, 0 for no limit.
    pub time_limit_s: u16,
    /// Mate in N: also look in the main line of the games without markers
    /// for positions where the side to move has a forced mate (D1975).
    pub scan_mates: bool,
    /// "Best move": when the move is not the one of the game, ask the
    /// engine whether it is as good (D2130). Off by default.
    pub second_opinion: bool,
    /// Take problems only from the games of a database that carry the
    /// Tactics flag `T` (D2150). Off by default; PGN files have no flags and
    /// ignore it.
    pub flagged_only: bool,
}

impl Default for TacticsConfig {
    fn default() -> TacticsConfig {
        TacticsConfig {
            mode: TacticsMode::Mate,
            order: TacticsOrder::Sequential,
            source: SourceKind::File,
            file: None,
            mate_min: 1,
            mate_max: 3,
            difficulty_min: 0,
            difficulty_max: 0,
            elo_min: 0,
            elo_max: 0,
            include_solved: false,
            max_attempts: 3,
            hints: true,
            accept_longer: false,
            time_limit_s: 0,
            scan_mates: true,
            second_opinion: false,
            flagged_only: false,
        }
    }
}

impl TacticsConfig {
    /// The same configuration with every number brought into its range
    /// (swapped bounds are put in order).
    #[must_use]
    pub fn sanitized(mut self) -> TacticsConfig {
        self.mate_min = self.mate_min.clamp(MATE_RANGE.0, MATE_RANGE.1);
        self.mate_max = self.mate_max.clamp(MATE_RANGE.0, MATE_RANGE.1);
        if self.mate_min > self.mate_max {
            std::mem::swap(&mut self.mate_min, &mut self.mate_max);
        }
        self.difficulty_min = self.difficulty_min.min(DIFFICULTY_RANGE.1);
        self.difficulty_max = self.difficulty_max.min(DIFFICULTY_RANGE.1);
        if self.difficulty_min != 0
            && self.difficulty_max != 0
            && self.difficulty_min > self.difficulty_max
        {
            std::mem::swap(&mut self.difficulty_min, &mut self.difficulty_max);
        }
        self.elo_min = self.elo_min.min(MAX_FILTER_ELO);
        self.elo_max = self.elo_max.min(MAX_FILTER_ELO);
        if self.elo_min != 0 && self.elo_max != 0 && self.elo_min > self.elo_max {
            std::mem::swap(&mut self.elo_min, &mut self.elo_max);
        }
        self.max_attempts = self.max_attempts.clamp(ATTEMPTS_RANGE.0, ATTEMPTS_RANGE.1);
        if self.time_limit_s != 0 {
            self.time_limit_s = self
                .time_limit_s
                .clamp(TIME_LIMIT_RANGE.0, TIME_LIMIT_RANGE.1);
        }
        self
    }

    /// Whether a problem with this `difficulty` passes the filter; a
    /// problem without the value always passes (D1914).
    pub fn accepts_difficulty(&self, difficulty: Option<u8>) -> bool {
        let Some(d) = difficulty else {
            return true;
        };
        (self.difficulty_min == 0 || d >= self.difficulty_min)
            && (self.difficulty_max == 0 || d <= self.difficulty_max)
    }

    /// Whether a problem with this `rating` passes the filter; a problem
    /// without the value always passes (D1914).
    pub fn accepts_rating(&self, rating: Option<u16>) -> bool {
        let Some(r) = rating else {
            return true;
        };
        (self.elo_min == 0 || r >= self.elo_min) && (self.elo_max == 0 || r <= self.elo_max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizing_puts_every_number_in_range_and_orders_the_bounds() {
        let config = TacticsConfig {
            mate_min: 9,
            mate_max: 0,
            difficulty_min: 7,
            difficulty_max: 3,
            elo_min: 60_000,
            elo_max: 1500,
            max_attempts: 0,
            time_limit_s: 2,
            ..TacticsConfig::default()
        }
        .sanitized();
        assert_eq!((config.mate_min, config.mate_max), (1, 5));
        assert_eq!((config.difficulty_min, config.difficulty_max), (3, 7));
        assert_eq!((config.elo_min, config.elo_max), (1500, 4000));
        assert_eq!(config.max_attempts, 1);
        assert_eq!(config.time_limit_s, 5);
    }

    #[test]
    fn a_zero_time_limit_stays_off() {
        let config = TacticsConfig::default().sanitized();
        assert_eq!(config.time_limit_s, 0);
    }

    #[test]
    fn filters_let_unmarked_problems_through() {
        let config = TacticsConfig {
            difficulty_min: 3,
            elo_min: 1000,
            ..TacticsConfig::default()
        };
        assert!(config.accepts_difficulty(None));
        assert!(config.accepts_rating(None));
        assert!(!config.accepts_difficulty(Some(2)));
        assert!(config.accepts_difficulty(Some(3)));
        assert!(!config.accepts_rating(Some(999)));
    }

    #[test]
    fn the_spellings_round_trip() {
        for mode in [TacticsMode::Mate, TacticsMode::BestMove] {
            assert_eq!(TacticsMode::from_name(mode.as_str()), Some(mode));
        }
        for order in [TacticsOrder::Sequential, TacticsOrder::Random] {
            assert_eq!(TacticsOrder::from_name(order.as_str()), Some(order));
        }
        for kind in [SourceKind::File, SourceKind::CurrentBase] {
            assert_eq!(SourceKind::from_name(kind.as_str()), Some(kind));
        }
        assert_eq!(TacticsMode::from_name("x"), None);
    }
}
