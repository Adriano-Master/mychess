//! The openings of the games: start position, suite and draws (spec M8-2,
//! D1272, D1273).
//!
//! Scid: `comp.tcl` 513-515 (current position) and the book (`help.tcl`
//! 3058). The suite of openings is an extension: one opening per pair of
//! games, in order or shuffled with the seed of the tournament, so the
//! tournament can be reproduced.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use mychess_core::{Move, Position};
use mychess_pgn::epd::{EpdBook, EpdOptions};
use mychess_pgn::read::{PgnReader, ReadOptions};
use thiserror::Error;

use super::config::{OpeningSource, SuiteOrder, TournamentConfig};
use super::schedule::Pairing;

/// The most openings read from a suite.
pub const MAX_SUITE_OPENINGS: usize = 100_000;

/// Where a game starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opening {
    /// The name, written to the `Opening` tag.
    pub name: Option<String>,
    /// The position the game starts from (`FEN` + `SetUp` when not the
    /// standard one).
    pub start: Position,
    /// Moves played at once, without time (D1267).
    pub moves: Vec<Move>,
}

impl Opening {
    /// The standard position, no moves.
    pub fn initial() -> Opening {
        Opening {
            name: None,
            start: Position::new(),
            moves: Vec::new(),
        }
    }
}

/// Why a suite could not be read.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SuiteError {
    /// The file could not be opened or read.
    #[error("cannot read the suite {path}")]
    Io {
        /// The file.
        path: std::path::PathBuf,
        /// The cause.
        #[source]
        source: std::io::Error,
    },
    /// The EPD file is malformed beyond use.
    #[error("invalid EPD suite {0}")]
    Epd(std::path::PathBuf),
    /// The file has no opening.
    #[error("the suite {0} has no opening")]
    Empty(std::path::PathBuf),
}

/// The openings read from a file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OpeningSuite {
    openings: Vec<Opening>,
}

impl OpeningSuite {
    /// A suite made of `openings`.
    pub fn new(openings: Vec<Opening>) -> OpeningSuite {
        OpeningSuite { openings }
    }

    /// The number of openings.
    pub fn len(&self) -> usize {
        self.openings.len()
    }

    /// Whether the suite is empty.
    pub fn is_empty(&self) -> bool {
        self.openings.is_empty()
    }

    /// Opening `index`.
    pub fn get(&self, index: usize) -> Option<&Opening> {
        self.openings.get(index)
    }
}

/// Reads a suite: `.epd` files are positions (the `id` operation names
/// them); anything else is PGN, taking the first `plies` half-moves of each
/// game. Blocking: run it from `perform`.
///
/// # Errors
///
/// [`SuiteError`] when the file cannot be read or has no opening.
pub fn load_suite(path: &Path, plies: u32) -> Result<OpeningSuite, SuiteError> {
    let is_epd = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("epd"));
    let openings = if is_epd {
        read_epd(path)?
    } else {
        read_pgn(path, plies)?
    };
    if openings.is_empty() {
        return Err(SuiteError::Empty(path.to_path_buf()));
    }
    Ok(OpeningSuite { openings })
}

fn read_epd(path: &Path) -> Result<Vec<Opening>, SuiteError> {
    let load = EpdBook::read_file(path, EpdOptions::standard()).map_err(|e| match e {
        mychess_pgn::epd::EpdError::Io { source, .. } => SuiteError::Io {
            path: path.to_path_buf(),
            source,
        },
        _ => SuiteError::Epd(path.to_path_buf()),
    })?;
    Ok(load
        .book
        .iter()
        .take(MAX_SUITE_OPENINGS)
        .map(|record| Opening {
            name: record
                .operations()
                .iter()
                .find(|op| op.opcode == "id")
                .map(|op| op.unquoted().to_owned())
                .filter(|name| !name.is_empty()),
            start: record.position().clone(),
            moves: Vec::new(),
        })
        .collect())
}

fn read_pgn(path: &Path, plies: u32) -> Result<Vec<Opening>, SuiteError> {
    let file = File::open(path).map_err(|source| SuiteError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let reader = PgnReader::new(BufReader::new(file), ReadOptions::default());
    let take = usize::try_from(plies).unwrap_or(usize::MAX);
    let mut out = Vec::new();
    for parsed in reader.filter_map(Result::ok).take(MAX_SUITE_OPENINGS) {
        let game = parsed.game;
        let moves: Vec<Move> = game
            .mainline()
            .filter_map(|id| game.mv(id).ok())
            .take(take)
            .collect();
        out.push(Opening {
            name: game
                .tags()
                .get_extra("Opening")
                .map(str::to_owned)
                .filter(|n| !n.is_empty()),
            start: game.start_position().clone(),
            moves,
        });
    }
    Ok(out)
}

/// A reproducible pseudo-random number from three keys (`SplitMix64`). The
/// tournament has no random state: every draw is a function of the seed, the
/// game and the ply, so the same seed gives the same tournament (D1273).
pub fn mix(seed: u64, a: u64, b: u64) -> u64 {
    let mut z = seed
        .wrapping_add(a.wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add(b.wrapping_mul(0xD1B5_4A32_D192_ED03))
        .wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The order in which the openings of a suite are used.
fn suite_order(len: usize, order: SuiteOrder, seed: u64) -> Vec<usize> {
    let mut indexes: Vec<usize> = (0..len).collect();
    if order == SuiteOrder::Random {
        // Fisher-Yates.
        for i in (1..len).rev() {
            let j = usize::try_from(mix(seed, i as u64, 0) % (i as u64 + 1)).unwrap_or(0);
            indexes.swap(i, j);
        }
    }
    indexes
}

/// The openings of a tournament: what each game starts from (D1272,
/// D1273).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningPlan {
    source: OpeningSource,
    start: Option<Position>,
    suite: OpeningSuite,
    order: Vec<usize>,
    pair_openings: bool,
}

/// An opening and whether the suite had to start again to supply it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    /// The opening.
    pub opening: Opening,
    /// The suite ran out and started again from its first opening.
    pub wrapped: bool,
}

impl OpeningPlan {
    /// The plan for `config`; `suite` is what [`load_suite`] read (ignored
    /// unless the source is a suite).
    pub fn new(config: &TournamentConfig, suite: Option<OpeningSuite>) -> OpeningPlan {
        let suite = suite.unwrap_or_default();
        let order_kind = config.suite.as_ref().map(|s| s.order).unwrap_or_default();
        let order = suite_order(suite.len(), order_kind, config.seed);
        OpeningPlan {
            source: config.opening,
            start: config
                .start_fen
                .as_deref()
                .and_then(|fen| Position::from_fen(fen).ok()),
            suite,
            order,
            pair_openings: config.pair_openings,
        }
    }

    /// Whether the moves come from a book (the runner asks the book).
    pub fn uses_book(&self) -> bool {
        self.source == OpeningSource::Book
    }

    /// What `pairing` starts from. A suite is used one opening per group of
    /// games (a pair) when `pair_openings` is on, else one per game.
    pub fn opening_for(&self, pairing: &Pairing) -> Chosen {
        let plain = |opening: Opening| Chosen {
            opening,
            wrapped: false,
        };
        match self.source {
            OpeningSource::Initial | OpeningSource::Book => plain(Opening::initial()),
            OpeningSource::CurrentPosition => plain(self.current()),
            OpeningSource::CurrentPositionFirstGame => plain(if pairing.number == 1 {
                self.current()
            } else {
                Opening::initial()
            }),
            OpeningSource::Suite => {
                let slot = if self.pair_openings {
                    pairing.group
                } else {
                    pairing.number.saturating_sub(1)
                };
                let slot = usize::try_from(slot).unwrap_or(usize::MAX);
                let len = self.order.len();
                if len == 0 {
                    return plain(Opening::initial());
                }
                let opening = self
                    .order
                    .get(slot % len)
                    .and_then(|&i| self.suite.get(i))
                    .cloned()
                    .unwrap_or_else(Opening::initial);
                Chosen {
                    opening,
                    wrapped: slot >= len && slot % len == 0,
                }
            }
        }
    }

    fn current(&self) -> Opening {
        Opening {
            name: None,
            start: self.start.clone().unwrap_or_default(),
            moves: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::tournament::config::{Participant, Scheduling, SuiteConfig};
    use crate::tournament::schedule::build_schedule;

    fn plan_config(source: OpeningSource, order: SuiteOrder, seed: u64) -> TournamentConfig {
        TournamentConfig {
            participants: vec![Participant::new(1, "A"), Participant::new(1, "B")],
            opening: source,
            suite: Some(SuiteConfig {
                path: "x.epd".into(),
                order,
                plies: 4,
            }),
            seed,
            ..TournamentConfig::default()
        }
    }

    fn suite_of(n: usize) -> OpeningSuite {
        // Distinct positions: the number of the move differs.
        OpeningSuite::new(
            (0..n)
                .map(|i| Opening {
                    name: Some(format!("op{i}")),
                    start: Position::from_fen(&format!("8/8/4k3/8/8/3K4/8/8 w - - 0 {}", i + 1))
                        .expect("fen"),
                    moves: Vec::new(),
                })
                .collect(),
        )
    }

    fn names(plan: &OpeningPlan, pairings: &[Pairing]) -> Vec<String> {
        pairings
            .iter()
            .map(|p| plan.opening_for(p).opening.name.unwrap_or_default())
            .collect()
    }

    #[test]
    fn mix_is_deterministic_and_spreads() {
        assert_eq!(mix(1, 2, 3), mix(1, 2, 3));
        assert_ne!(mix(1, 2, 3), mix(1, 2, 4));
        assert_ne!(mix(1, 2, 3), mix(2, 2, 3));
    }

    #[test]
    fn a_sequential_suite_gives_one_opening_per_pair_and_wraps() {
        let cfg = plan_config(OpeningSource::Suite, SuiteOrder::Sequential, 0);
        let plan = OpeningPlan::new(&cfg, Some(suite_of(2)));
        // 3 participants, 2 cycles: pairs of games share a group.
        let s = build_schedule(3, Scheduling::RoundRobin, 2, 0);
        assert_eq!(
            names(&plan, &s.pairings),
            ["op0", "op0", "op1", "op1", "op0", "op0"]
        );
        assert!(plan.opening_for(&s.pairings[4]).wrapped);
        assert!(!plan.opening_for(&s.pairings[2]).wrapped);
    }

    #[test]
    fn without_pairing_each_game_takes_the_next_opening() {
        let mut cfg = plan_config(OpeningSource::Suite, SuiteOrder::Sequential, 0);
        cfg.pair_openings = false;
        let plan = OpeningPlan::new(&cfg, Some(suite_of(4)));
        let s = build_schedule(2, Scheduling::RoundRobin, 4, 0);
        assert_eq!(names(&plan, &s.pairings), ["op0", "op1", "op2", "op3"]);
    }

    #[test]
    fn the_same_seed_gives_the_same_shuffle_and_another_seed_another() {
        let s = build_schedule(2, Scheduling::RoundRobin, 2, 0);
        let run = |seed| {
            let cfg = plan_config(OpeningSource::Suite, SuiteOrder::Random, seed);
            let plan = OpeningPlan::new(&cfg, Some(suite_of(20)));
            let mut all = Vec::new();
            for pair in 0..20 {
                let p = Pairing {
                    group: pair,
                    ..s.pairings[0]
                };
                all.push(plan.opening_for(&p).opening.name.unwrap_or_default());
            }
            all
        };
        assert_eq!(run(7), run(7));
        assert_ne!(run(7), run(8));
        let mut sorted = run(7);
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 20, "a shuffle repeats nothing");
    }

    #[test]
    fn current_position_applies_to_all_or_to_the_first_game() {
        let mut cfg = plan_config(OpeningSource::CurrentPosition, SuiteOrder::Sequential, 0);
        cfg.start_fen = Some("8/8/4k3/8/8/3K4/R7/8 w - - 0 1".to_owned());
        let s = build_schedule(2, Scheduling::RoundRobin, 2, 0);
        let plan = OpeningPlan::new(&cfg, None);
        assert!(s.pairings.iter().all(|p| {
            plan.opening_for(p)
                .opening
                .start
                .fen()
                .starts_with("8/8/4k3")
        }));
        cfg.opening = OpeningSource::CurrentPositionFirstGame;
        let plan = OpeningPlan::new(&cfg, None);
        assert_ne!(
            plan.opening_for(&s.pairings[0]).opening.start,
            Position::new()
        );
        assert_eq!(
            plan.opening_for(&s.pairings[1]).opening.start,
            Position::new()
        );
    }

    #[test]
    fn initial_and_book_start_from_the_standard_position() {
        let s = build_schedule(2, Scheduling::RoundRobin, 1, 0);
        for source in [OpeningSource::Initial, OpeningSource::Book] {
            let plan = OpeningPlan::new(&plan_config(source, SuiteOrder::Sequential, 0), None);
            assert_eq!(plan.opening_for(&s.pairings[0]).opening, Opening::initial());
            assert_eq!(plan.uses_book(), source == OpeningSource::Book);
        }
    }

    #[test]
    fn reads_an_epd_suite_with_names() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("openings.EPD");
        let mut f = File::create(&path).expect("file");
        writeln!(
            f,
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - id \"King pawn\";"
        )
        .expect("write");
        writeln!(
            f,
            "rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b KQkq - bm e5;"
        )
        .expect("write");
        drop(f);
        let suite = load_suite(&path, 4).expect("suite");
        assert_eq!(suite.len(), 2);
        assert_eq!(
            suite.get(0).and_then(|o| o.name.clone()),
            Some("King pawn".to_owned())
        );
        assert_eq!(suite.get(1).and_then(|o| o.name.clone()), None);
    }

    #[test]
    fn reads_a_pgn_suite_taking_the_first_plies() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("openings.pgn");
        std::fs::write(
            &path,
            "[Event \"a\"]\n[Opening \"Ruy\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 *\n\n[Event \"b\"]\n\n1. d4 d5 *\n",
        )
        .expect("write");
        let suite = load_suite(&path, 3).expect("suite");
        assert_eq!(suite.len(), 2);
        assert_eq!(suite.get(0).map(|o| o.moves.len()), Some(3));
        assert_eq!(
            suite.get(0).and_then(|o| o.name.clone()),
            Some("Ruy".to_owned())
        );
        assert_eq!(suite.get(1).map(|o| o.moves.len()), Some(2));
    }

    #[test]
    fn a_missing_or_empty_suite_is_an_error() {
        let dir = tempfile::tempdir().expect("dir");
        assert!(matches!(
            load_suite(&dir.path().join("nope.pgn"), 4),
            Err(SuiteError::Io { .. })
        ));
        let empty = dir.path().join("empty.pgn");
        std::fs::write(&empty, "nothing here").expect("write");
        assert!(matches!(load_suite(&empty, 4), Err(SuiteError::Empty(_))));
    }
}
