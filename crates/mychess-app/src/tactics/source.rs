//! Where the problems come from, and the search for the next one (spec
//! M8-4, D1901, D1903, D1905, D1914-D1917).
//!
//! Everything here is blocking: [`open_file`] reads a PGN file and
//! [`find_next`] reads games and runs the solver. Only
//! [`perform`](super::effect::perform) calls them, from `spawn_blocking`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mychess_core::mate::{MateLimits, MateResult, find_mate, principal_line};
use mychess_core::{Game, Move, NodeId, Position};
use mychess_pgn::read::ReadOptions;

use super::config::{MATE_RANGE, TacticsConfig, TacticsMode};
use super::problem::{Problem, extract_problems, make_problem, problem_id};
use super::scan::{self, SCAN_NODES, scan_plies};
use crate::bases::{FileStamp, SharedSource};
use crate::error::FileError;
use crate::files::{PgnCollection, PgnLoad, PgnSource, ScanLimits, scan_pgn};
use crate::shared::SharedGame;
use mychess_db::GameSource;

/// Nodes the solver may use to classify one problem (D1909).
pub const CLASSIFY_NODES: u64 = 300_000;
/// Nodes the solver may use to build the line of a problem.
pub const LINE_NODES: u64 = 2_000_000;
/// Nodes of the line that a search of mates in the games may build for the
/// problem it offers (D1985): a tenth of [`LINE_NODES`].
pub const SCAN_LINE_NODES: u64 = 200_000;
/// Games that one search request looks at (D1916).
pub const GAMES_PER_REQUEST: usize = 2_000;
/// Problems that one search request classifies with the solver (D1916).
pub const CLASSIFY_PER_REQUEST: usize = 30;

/// The games that problems are taken from (D1901).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SourceHandle {
    /// A PGN file with several games, not parsed yet.
    Pgn(Arc<PgnCollection>),
    /// A PGN file with one game.
    Single(SharedGame),
    /// The games of the filter of a database, by number (D1917).
    Base {
        /// The database.
        source: SharedSource,
        /// The numbers of the games (from 1), in the order of the filter.
        numbers: Arc<Vec<u32>>,
    },
}

impl SourceHandle {
    /// How many games the source has.
    pub fn len(&self) -> usize {
        match self {
            SourceHandle::Pgn(collection) => collection.len(),
            SourceHandle::Single(_) => 1,
            SourceHandle::Base { numbers, .. } => numbers.len(),
        }
    }

    /// Whether the source has no game.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether the game at `index` carries the Tactics flag `T`; `None` for
    /// a source without flags (a PGN file, D2150).
    pub fn flagged(&self, index: usize) -> Option<bool> {
        let SourceHandle::Base { source, numbers } = self else {
            return None;
        };
        let number = *numbers.get(index)?;
        let entry = source.read().entry(number).ok()?;
        Some(entry.flags.contains(mychess_db::si4::GameFlags::TACTICS))
    }

    /// The game at `index` (from 0) and its number in the source (from 1);
    /// `None` when it cannot be read (it is skipped, D1905).
    pub fn game(&self, index: usize) -> Option<(Game, u32)> {
        match self {
            SourceHandle::Pgn(collection) => {
                let parsed = collection.parse_game(index, &ReadOptions::default())?;
                Some((parsed.game, u32::try_from(index + 1).ok()?))
            }
            SourceHandle::Single(game) => (index == 0).then(|| (game.to_game(), 1)),
            SourceHandle::Base { source, numbers } => {
                let number = *numbers.get(index)?;
                let game = source.lock().game(number).ok()?;
                Some((game, number))
            }
        }
    }
}

/// A source that was opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedSource {
    /// The games.
    pub handle: SourceHandle,
    /// Identifies the source in `tactics.toml` (D1926, D1983): see
    /// [`source_key`]; empty for the Clipbase.
    pub key: String,
    /// For a Scid database read from a file: the file and its state when it
    /// was read, so a change (by the same base open in a slot, say) ends the
    /// session instead of mixing old and new games (D1987).
    pub stamp: Option<SourceStamp>,
}

/// A file and its state at a moment (D1987).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceStamp {
    /// The `.si4` file.
    pub path: PathBuf,
    /// Its length and time of change.
    pub stamp: FileStamp,
}

/// Why a source could not be opened (D1988): each reason has its own
/// remark in the front-end.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SourceError {
    /// A file operation failed.
    #[error("{0}")]
    File(FileError),
    /// An `.si3` database: it must be upgraded first.
    #[error("the database is in the old SI3 format")]
    NeedsUpgrade,
    /// The database files are damaged or of another version.
    #[error("the database files are corrupt or unsupported")]
    Corrupt,
    /// The database has an unfinished transaction.
    #[error("the database has an unfinished transaction")]
    Unfinished,
}

impl From<FileError> for SourceError {
    fn from(error: FileError) -> SourceError {
        SourceError::File(error)
    }
}

/// The key of a source in `tactics.toml` (D1926, D1983), derived without
/// touching the file system so that the machine, the reset and the opening
/// agree: the absolute path (the working directory is added to a relative
/// one), with `.si4` for any of the three files of a Scid database, and the
/// lossy UTF-8 text of it (a path that is not UTF-8 keeps only its
/// readable part: two such paths may share a key).
pub fn source_key(path: &Path) -> String {
    let mut full = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    if is_si4(&full) {
        full.set_extension("si4");
    }
    full.to_string_lossy().into_owned()
}

/// Whether `path` names a Scid database (`.si4`, or one of its two other
/// files), compared without regard to case (D1971).
fn is_si4(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["si4", "sn4", "sg4"]
            .iter()
            .any(|k| e.eq_ignore_ascii_case(k))
    })
}

/// Opens a Scid database the way the application opens a base, but only to
/// read: it takes no slot and writes nothing (D1971).
fn open_si4(path: &Path) -> Result<OpenedSource, SourceError> {
    use crate::bases::BaseError;
    let unreadable = || {
        SourceError::File(FileError::new(
            crate::error::FileErrorKind::Io(std::io::ErrorKind::InvalidData),
            Some(path.to_path_buf()),
        ))
    };
    let outcome = crate::bases::open_base(path, &[], true, &crate::bases::OpenControl::new())
        .map_err(|error| match error {
            BaseError::File(file) => SourceError::File(file),
            BaseError::Si3NeedsUpgrade => SourceError::NeedsUpgrade,
            BaseError::PendingJournal => SourceError::Unfinished,
            BaseError::Corrupt => SourceError::Corrupt,
            _ => unreadable(),
        })?;
    let crate::bases::OpenOutcome::Opened(base) = outcome else {
        return Err(unreadable());
    };
    let total = GameSource::len(&*base.source.read());
    if total == 0 {
        return Err(SourceError::File(FileError::new(
            crate::error::FileErrorKind::NoGamesFound { skipped_bytes: 0 },
            Some(path.to_path_buf()),
        )));
    }
    let numbers: Vec<u32> = (1..=total).collect();
    Ok(OpenedSource {
        key: source_key(path),
        stamp: base.stamp.map(|stamp| SourceStamp {
            path: base.path.clone(),
            stamp,
        }),
        handle: SourceHandle::Base {
            source: base.source,
            numbers: Arc::new(numbers),
        },
    })
}

/// Reads a file as a source of problems (D1901, D1971): a Scid database
/// (`.si4`) or a PGN file (also `.pgn.gz`).
///
/// # Errors
///
/// A [`SourceError`] when the file cannot be read or has no game.
pub fn open_file(path: &Path) -> Result<OpenedSource, SourceError> {
    // A Scid 3 database is not read: it must be upgraded first (D1988).
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("si3"))
    {
        return Err(SourceError::NeedsUpgrade);
    }
    if is_si4(path) {
        return open_si4(path);
    }
    let (load, _notices) = scan_pgn(
        PgnSource::File(path.to_path_buf()),
        &ScanLimits::default(),
        &ReadOptions::default(),
    )?;
    let handle = match load {
        PgnLoad::Many(collection) => SourceHandle::Pgn(collection),
        PgnLoad::Single(loaded) => SourceHandle::Single(loaded.game),
        _ => {
            return Err(SourceError::File(FileError::new(
                crate::error::FileErrorKind::NoGamesFound { skipped_bytes: 0 },
                Some(path.to_path_buf()),
            )));
        }
    };
    Ok(OpenedSource {
        handle,
        key: source_key(path),
        stamp: None,
    })
}

/// The first step of the walk that is coprime with `len`, taken from
/// `seed` (D1915). Always in `1..len` for `len > 1`, and 1 otherwise.
pub fn coprime_step(seed: u64, len: usize) -> usize {
    if len <= 2 {
        return 1;
    }
    let modulus = u64::try_from(len - 1).unwrap_or(1).max(1);
    let mut step = usize::try_from(seed % modulus).unwrap_or(0) + 1;
    for _ in 0..len {
        if gcd(step, len) == 1 {
            return step;
        }
        step = if step + 1 >= len { 1 } else { step + 1 };
    }
    1
}

fn gcd(a: usize, b: usize) -> usize {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// The index of the `k`-th game of the walk (D1915).
pub fn walk_index(start: usize, step: usize, k: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let (start, step, k, len64) = (
        u64::try_from(start).unwrap_or(0),
        u64::try_from(step).unwrap_or(1),
        u64::try_from(k % len).unwrap_or(0),
        u64::try_from(len).unwrap_or(1),
    );
    usize::try_from((start + k * step) % len64).unwrap_or(0)
}

/// What a request for the next problem asks (D1914-D1916).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindQuery {
    /// The filters and the mode.
    pub config: Box<TacticsConfig>,
    /// First game of the walk.
    pub start: usize,
    /// Step of the walk (coprime with the number of games).
    pub step: usize,
    /// The count of games walked so far.
    pub k: usize,
    /// Most games to look at in this request.
    pub max_games: usize,
    /// The ids of the problems already solved, ascending (D1904).
    pub solved: Arc<Vec<u64>>,
    /// The file of a Scid database and its state when it was read: a
    /// different state ends the search at once (D1987).
    pub stamp: Option<SourceStamp>,
    /// Raised when the session ends or the request is dropped: the search
    /// stops between two positions (D1984).
    pub cancel: Cancel,
}

/// A flag that tells a search running on another thread to stop (D1984).
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    /// A flag that is not raised.
    pub fn new() -> Cancel {
        Cancel::default()
    }

    /// Raises the flag.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether the flag is raised.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

impl PartialEq for Cancel {
    fn eq(&self, other: &Cancel) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Cancel {}

/// The problem that was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The problem.
    pub problem: Problem,
    /// The shortest mate, in moves of the side to move (Mate in N only).
    pub mate_moves: Option<u8>,
    /// The line to follow from the position: the line of the solver
    /// (Mate in N) or the solution of the game (best move).
    pub line: Vec<Move>,
}

/// The answer of [`find_next`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FindOutcome {
    /// The next problem, if one was found.
    pub found: Option<Box<Found>>,
    /// Where the walk goes on: the value of `k` for the next request.
    pub next_k: usize,
    /// How many games this request looked at.
    pub examined: usize,
    /// How many problems this request sent to the solver (D1947).
    pub classified: usize,
    /// The file of the source is not what it was when it was read (D1987).
    pub source_changed: bool,
    /// The ids of the problems of mate that this request met whose shortest
    /// mate is longer than "Longest mate" (D2155): a lower bound, since the
    /// solver may not be able to tell within its small budget. Ids, not a
    /// count, so a problem met again on another lap is counted once (D2164).
    pub beyond_ids: Vec<u64>,
}

/// Whether `problem` passes the filters that need no solver.
fn passes_filters(config: &TacticsConfig, problem: &Problem, solved: &[u64]) -> bool {
    config.accepts_difficulty(problem.difficulty)
        && config.accepts_rating(problem.rating)
        && (config.include_solved || solved.binary_search(&problem.id).is_err())
}

/// What the search of a game came to.
enum Searched {
    /// A problem that fits.
    Found(Box<Found>),
    /// Nothing in this game.
    Nothing,
    /// The game has more candidates than the request has budget left: it
    /// is looked at again in the next one (D1978).
    Deferred,
    /// The request was cancelled (D1984).
    Cancelled,
}

/// The first forced mate of `position` within `max_moves`, with the budget
/// of the classification; the line is built only for a problem that is
/// going to be offered (D1985).
fn first_mate(position: &Position, max_moves: u8, max_nodes: u64) -> Option<u8> {
    let limits = MateLimits {
        max_moves,
        max_nodes,
    };
    match find_mate(position, limits) {
        MateResult::Mate { moves, .. } => Some(moves),
        _ => None,
    }
}

/// Whether the id of a problem is one that the filters let through (D1985:
/// asked before anything is built for it).
fn id_accepted(config: &TacticsConfig, solved: &[u64], id: u64) -> bool {
    config.include_solved || solved.binary_search(&id).is_err()
}

/// The problems that the game itself declares (markers, a start position
/// that is not the standard one), judged by the mode (D1916).
fn find_declared(
    problems: Vec<Problem>,
    query: &FindQuery,
    classified: &mut usize,
    beyond: &mut Vec<u64>,
) -> Option<Found> {
    for mut problem in problems {
        if query.cancel.is_cancelled() || !passes_filters(&query.config, &problem, &query.solved) {
            continue;
        }
        // The ceiling holds inside a game too (D1947): a game with many
        // markers must not cost hundreds of solver runs in one request.
        // The problems of the game that are left wait for another lap.
        if *classified >= CLASSIFY_PER_REQUEST {
            break;
        }
        match query.config.mode {
            TacticsMode::BestMove => {
                // The game brings the solution; a game that only brings the
                // position (the "mate in N" bases of the Scid have no moves
                // at all) gets the line of the solver when there is a mate
                // (D1973).
                if problem.solution.is_empty() {
                    *classified += 1;
                    let Some(moves) = first_mate(&problem.position, MATE_RANGE.1, CLASSIFY_NODES)
                    else {
                        continue;
                    };
                    let Some(line) = principal_line(&problem.position, moves, LINE_NODES) else {
                        continue;
                    };
                    problem.solution.clone_from(&line);
                }
                let line = problem.solution.clone();
                return Some(Found {
                    problem,
                    mate_moves: None,
                    line,
                });
            }
            TacticsMode::Mate => {
                *classified += 1;
                let limits = MateLimits {
                    max_moves: query.config.mate_max,
                    max_nodes: CLASSIFY_NODES,
                };
                // One search: its answer decides both the mate and, when
                // there is none within the limit, whether to look further
                // (D2163).
                let moves = match find_mate(&problem.position, limits) {
                    MateResult::Mate { moves, .. } => moves,
                    MateResult::NoMate => {
                        // Is there a longer one? It is counted, so the dialog
                        // can say what the limit hides (D2155). Nothing to
                        // say when the limit is already the longest.
                        if query.config.mate_max < MATE_RANGE.1
                            && first_mate(&problem.position, MATE_RANGE.1, SCAN_NODES)
                                .is_some_and(|m| m > query.config.mate_max)
                        {
                            beyond.push(problem.id);
                        }
                        continue;
                    }
                    _ => continue,
                };
                if moves < query.config.mate_min {
                    continue;
                }
                let line = principal_line(&problem.position, moves, LINE_NODES).unwrap_or_default();
                return Some(Found {
                    problem,
                    mate_moves: Some(moves),
                    line,
                });
            }
        }
    }
    None
}

/// The first position of the main line of `game` (a game without markers
/// that starts from the standard position) where the side to move has a
/// forced mate that the filters accept (D1975-D1982).
fn scan_game(game: &Game, number: u32, query: &FindQuery, classified: &mut usize) -> Searched {
    let plies = scan_plies(game, &query.config);
    if plies.is_empty() {
        return Searched::Nothing;
    }
    if *classified > 0 && *classified + plies.len() > CLASSIFY_PER_REQUEST {
        return Searched::Deferred;
    }
    let nodes: Vec<NodeId> = game.mainline().collect();
    // The positions inside a line already offered are not offered again.
    let mut covered_until = 0usize;
    for ply in plies {
        if query.cancel.is_cancelled() {
            return Searched::Cancelled;
        }
        if ply <= covered_until || *classified >= CLASSIFY_PER_REQUEST {
            continue;
        }
        let Some(node) = ply.checked_sub(1).and_then(|i| nodes.get(i)).copied() else {
            continue;
        };
        let Ok(position) = game.position_at(node) else {
            continue;
        };
        // No move for the attacker: the mate was already given (D1976).
        if position.legal_moves().is_empty() {
            continue;
        }
        *classified += 1;
        let Some(moves) = first_mate(position, query.config.mate_max, SCAN_NODES) else {
            continue;
        };
        covered_until = ply + 2 * usize::from(moves) - 1;
        let ply32 = u32::try_from(ply).unwrap_or(u32::MAX);
        // Cheapest tests first (D1985): the range, the id; only then is a
        // problem (and its line) built.
        if moves < query.config.mate_min
            || !id_accepted(&query.config, &query.solved, problem_id(number, ply32))
        {
            continue;
        }
        // The line of the game when it is that very mate, else the one of
        // the solver, with a small budget (D1980, D1985).
        let line = scan::game_line_if_mate(game, node, position, moves)
            .or_else(|| principal_line(position, moves, SCAN_LINE_NODES))
            .unwrap_or_default();
        let problem = make_problem(game, number, ply32, position.clone(), line.clone(), None);
        if !passes_filters(&query.config, &problem, &query.solved) {
            continue;
        }
        return Searched::Found(Box::new(Found {
            problem,
            mate_moves: Some(moves),
            line,
        }));
    }
    Searched::Nothing
}

/// Looks for the next problem along the walk (D1916): at most
/// [`FindQuery::max_games`] games and [`CLASSIFY_PER_REQUEST`] positions
/// sent to the solver; stops at the first problem that fits. A game that
/// cannot be read is skipped.
///
/// A game that declares problems (a `****D` marker, a start position that
/// is not the standard one) offers those. Any other game is searched for
/// mates in its main line in "Mate in N" with [`TacticsConfig::scan_mates`]
/// on (D1975). The search stops early when [`FindQuery::cancel`] is raised
/// (D1984) or the file of a Scid database changed (D1987).
pub fn find_next(handle: &SourceHandle, query: &FindQuery) -> FindOutcome {
    let len = handle.len();
    let mut out = FindOutcome {
        next_k: query.k,
        ..FindOutcome::default()
    };
    if let Some(source) = &query.stamp
        && FileStamp::of(&source.path) != Some(source.stamp)
    {
        out.source_changed = true;
        return out;
    }
    let mut classified = 0usize;
    while out.examined < query.max_games.min(len)
        && classified < CLASSIFY_PER_REQUEST
        && !query.cancel.is_cancelled()
    {
        let index = walk_index(query.start, query.step, out.next_k, len);
        out.next_k = out.next_k.saturating_add(1);
        out.examined += 1;
        // Only the games with the Tactics flag, when asked (D2150).
        if query.config.flagged_only && handle.flagged(index) == Some(false) {
            continue;
        }
        let Some((game, number)) = handle.game(index) else {
            continue;
        };
        let problems = extract_problems(&game, number);
        let searched = if !problems.is_empty() {
            find_declared(problems, query, &mut classified, &mut out.beyond_ids)
                .map_or(Searched::Nothing, |f| Searched::Found(Box::new(f)))
        } else if query.config.mode == TacticsMode::Mate && query.config.scan_mates {
            scan_game(&game, number, query, &mut classified)
        } else {
            Searched::Nothing
        };
        out.classified = classified;
        match searched {
            Searched::Found(found) => {
                out.found = Some(found);
                return out;
            }
            Searched::Deferred => {
                // Not enough budget left for this game: back to it in the
                // next request (there is always an earlier game, since the
                // budget was spent on one).
                out.next_k -= 1;
                out.examined -= 1;
                break;
            }
            Searched::Cancelled => break,
            Searched::Nothing => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_core::Position;

    fn mate_game(fen: &str, solution_uci: &[&str]) -> Game {
        let start = Position::from_fen(fen).expect("fen");
        let mut game = Game::from_position(start);
        let mut cursor = game.root();
        for uci in solution_uci {
            let pos = game.position_at(cursor).expect("node").clone();
            cursor = game
                .add_move(cursor, pos.parse_uci(uci).expect("uci"))
                .expect("legal");
        }
        game
    }

    fn handle(games: &[Game]) -> (SourceHandle, usize) {
        // A PGN file is the usual source; tests use a text round trip.
        let mut text = String::new();
        for game in games {
            text.push_str(
                &mychess_pgn::write::game_to_string(
                    game,
                    &mychess_pgn::write::WriteOptions::default(),
                )
                .expect("pgn"),
            );
            text.push('\n');
        }
        let (load, _) = scan_pgn(
            PgnSource::Text(text),
            &ScanLimits::default(),
            &ReadOptions::default(),
        )
        .expect("scan");
        let len = games.len();
        match load {
            PgnLoad::Many(c) => (SourceHandle::Pgn(c), len),
            PgnLoad::Single(g) => (SourceHandle::Single(g.game), len),
            _ => panic!("no games"),
        }
    }

    fn query(mode: TacticsMode, solved: Vec<u64>) -> FindQuery {
        FindQuery {
            config: Box::new(TacticsConfig {
                mode,
                ..TacticsConfig::default()
            }),
            start: 0,
            step: 1,
            k: 0,
            max_games: GAMES_PER_REQUEST,
            solved: Arc::new(solved),
            stamp: None,
            cancel: Cancel::new(),
        }
    }

    const MATE1: &str = "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1";
    const MATE2: &str = "kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1";
    const NO_MATE: &str = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";

    #[test]
    fn the_walk_visits_every_game_once_whatever_the_seed() {
        for len in [1usize, 2, 3, 6, 10, 12, 97] {
            for seed in [0u64, 1, 5, 77, 123_456_789] {
                let step = coprime_step(seed, len);
                let mut seen = vec![false; len];
                for k in 0..len {
                    seen[walk_index(
                        usize::try_from(seed % len as u64).unwrap_or(0),
                        step,
                        k,
                        len,
                    )] = true;
                }
                assert!(seen.iter().all(|s| *s), "len {len} seed {seed} step {step}");
            }
        }
    }

    #[test]
    fn mate_mode_skips_games_without_mate_and_reports_the_distance() {
        let (h, _) = handle(&[
            mate_game(NO_MATE, &[]),
            mate_game(MATE2, &[]),
            mate_game(MATE1, &[]),
        ]);
        let out = find_next(&h, &query(TacticsMode::Mate, vec![]));
        let found = out.found.expect("a problem");
        assert_eq!(found.problem.game_number, 2);
        assert_eq!(found.mate_moves, Some(2));
        assert_eq!(found.line.len(), 3);
        assert_eq!(out.next_k, 2, "the walk goes on after the game found");
    }

    #[test]
    fn the_mate_range_filters_by_distance() {
        let (h, _) = handle(&[mate_game(MATE1, &[]), mate_game(MATE2, &[])]);
        let mut q = query(TacticsMode::Mate, vec![]);
        q.config.mate_min = 2;
        q.config.mate_max = 3;
        let found = find_next(&h, &q).found.expect("the mate in two");
        assert_eq!(found.problem.game_number, 2);
        q.config.mate_min = 1;
        q.config.mate_max = 1;
        let found = find_next(&h, &q).found.expect("the mate in one");
        assert_eq!(found.problem.game_number, 1);
    }

    #[test]
    fn solved_problems_are_skipped_unless_asked_for() {
        let (h, _) = handle(&[mate_game(MATE1, &[]), mate_game(MATE2, &[])]);
        let solved = vec![problem_id(1, 0)];
        let found = find_next(&h, &query(TacticsMode::Mate, solved.clone()))
            .found
            .expect("the second");
        assert_eq!(found.problem.game_number, 2);
        let mut q = query(TacticsMode::Mate, solved);
        q.config.include_solved = true;
        assert_eq!(
            find_next(&h, &q).found.expect("first").problem.game_number,
            1
        );
    }

    #[test]
    fn best_move_mode_needs_a_solution_or_a_mate_the_solver_can_show() {
        // No moves and no mate: nothing to judge against.
        let quiet = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";
        let (h, _) = handle(&[mate_game(quiet, &[]), mate_game(MATE1, &["a1a8"])]);
        let found = find_next(&h, &query(TacticsMode::BestMove, vec![]))
            .found
            .expect("the game with a solution");
        assert_eq!(found.problem.game_number, 2);
        assert_eq!(found.line.len(), 1);
        assert_eq!(found.mate_moves, None);
    }

    #[test]
    fn best_move_mode_takes_the_line_of_the_solver_for_a_game_without_moves() {
        let (h, _) = handle(&[mate_game(MATE1, &[])]);
        let found = find_next(&h, &query(TacticsMode::BestMove, vec![]))
            .found
            .expect("the solver gives the line");
        assert_eq!(found.problem.game_number, 1);
        assert_eq!(found.line.len(), 1);
        assert_eq!(found.problem.solution, found.line);
    }

    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    #[test]
    fn a_game_without_markers_that_ends_in_mate_offers_the_position_before_it() {
        // Scholar's mate: 4. Qxf7#.
        let scholar = mate_game(
            START,
            &["e2e4", "e7e5", "f1c4", "b8c6", "d1h5", "g8f6", "h5f7"],
        );
        let (h, _) = handle(&[scholar]);
        let found = find_next(&h, &query(TacticsMode::Mate, vec![]))
            .found
            .expect("the mate in one");
        assert_eq!(found.mate_moves, Some(1));
        assert_eq!(found.problem.ply, 6);
        assert_eq!(found.line.len(), 1, "the move of the game");
        assert_eq!(found.problem.id, problem_id(1, 6));
    }

    #[test]
    fn the_search_for_mates_does_not_run_in_best_move_mode_or_when_it_is_off() {
        let scholar = || {
            mate_game(
                START,
                &["e2e4", "e7e5", "f1c4", "b8c6", "d1h5", "g8f6", "h5f7"],
            )
        };
        let (h, _) = handle(&[scholar()]);
        assert!(
            find_next(&h, &query(TacticsMode::BestMove, vec![]))
                .found
                .is_none()
        );
        let mut off = query(TacticsMode::Mate, vec![]);
        off.config.scan_mates = false;
        assert!(find_next(&h, &off).found.is_none());
    }

    #[test]
    fn a_solved_position_of_the_search_is_skipped() {
        let scholar = mate_game(
            START,
            &["e2e4", "e7e5", "f1c4", "b8c6", "d1h5", "g8f6", "h5f7"],
        );
        let (h, _) = handle(&[scholar]);
        let solved = vec![problem_id(1, 6)];
        assert!(
            find_next(&h, &query(TacticsMode::Mate, solved))
                .found
                .is_none()
        );
    }

    #[test]
    fn a_walk_that_finds_nothing_reports_how_far_it_went() {
        let (h, _) = handle(&[mate_game(NO_MATE, &[]), mate_game(NO_MATE, &[])]);
        let out = find_next(&h, &query(TacticsMode::Mate, vec![]));
        assert!(out.found.is_none());
        assert_eq!(out.examined, 2);
        assert_eq!(out.next_k, 2);
    }

    #[test]
    fn the_request_stops_at_its_game_budget() {
        let games: Vec<Game> = (0..5).map(|_| mate_game(NO_MATE, &[])).collect();
        let (h, _) = handle(&games);
        let mut q = query(TacticsMode::Mate, vec![]);
        q.max_games = 3;
        let out = find_next(&h, &q);
        assert_eq!((out.examined, out.next_k), (3, 3));
    }

    #[test]
    fn the_walk_wraps_around_the_source() {
        let (h, _) = handle(&[mate_game(MATE1, &[]), mate_game(NO_MATE, &[])]);
        let mut q = query(TacticsMode::Mate, vec![]);
        q.k = 1;
        let found = find_next(&h, &q).found.expect("wrapped to the first");
        assert_eq!(found.problem.game_number, 1);
    }

    #[test]
    fn open_file_reports_a_missing_file() {
        let dir = tempfile::tempdir().expect("dir");
        let error = open_file(&dir.path().join("none.pgn")).expect_err("missing");
        assert!(matches!(
            error,
            SourceError::File(e) if e.kind == crate::error::FileErrorKind::NotFound
        ));
    }

    #[test]
    fn the_key_of_a_source_is_the_same_for_the_three_files_of_a_database() {
        let si4 = source_key(Path::new("some/dir/bases.si4"));
        assert_eq!(source_key(Path::new("some/dir/bases.SG4")), si4);
        assert_eq!(source_key(Path::new("some/dir/bases.sn4")), si4);
        assert!(Path::new(&si4).is_absolute(), "{si4}");
        // A PGN keeps its own name.
        assert!(source_key(Path::new("some/dir/p.pgn")).ends_with("p.pgn"));
    }

    #[test]
    fn open_file_reads_a_pgn_with_problems() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("p.pgn");
        std::fs::write(
            &path,
            format!(
                "[Event \"A\"]\n[SetUp \"1\"]\n[FEN \"{MATE1}\"]\n\n1. Ra8# 1-0\n\n\
                 [Event \"B\"]\n[SetUp \"1\"]\n[FEN \"{MATE2}\"]\n\n*\n"
            ),
        )
        .expect("write");
        let opened = open_file(&path).expect("open");
        assert_eq!(opened.handle.len(), 2);
        let q = query(TacticsMode::Mate, vec![]);
        let found = find_next(&opened.handle, &q).found.expect("first problem");
        assert_eq!(found.problem.title, "A");
    }
}
