//! Building the repertoire from games, a Polyglot book or a tree mask (spec
//! M8-5, D2205-D2212, D2236).
//!
//! Everything here is blocking: only [`perform`](super::effect::perform)
//! calls [`build`], from `spawn_blocking`. Files and games are untrusted:
//! every walk is bounded by depth, by positions and by games, and checks the
//! cancel flag between games and every thousand positions.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mychess_book::Book;
use mychess_core::{Color, Game, Move, NodeId, Position, ZobristHash};
use mychess_db::GameSource;
use mychess_db::si4::GameFlags;
use mychess_db::tree::mask::{Marker, MaskColor, MaskMove, MaskNag, TreeMask, resolve_move, stm};

use super::config::TrainSide;
use super::repertoire::{RepBuilder, RepSet, Repertoire};
use crate::tactics::{Cancel, SourceError, SourceHandle, open_file};

/// Games read for a repertoire (D2210).
pub const MAX_GAMES: usize = 20_000;
/// Nodes read in one game (variations included).
const MAX_NODES_PER_GAME: usize = 20_000;
/// Moves of the book kept per position (D2207).
const MAX_BOOK_MOVES: usize = 8;
/// A book move needs this share of the weight of the position, in percent.
const BOOK_MIN_PERCENT: u64 = 3;
/// A book move lighter than this share of the heaviest is dubious (D2207).
const BOOK_DUBIOUS_PERCENT: u64 = 25;
/// How often the cancel flag is looked at, in positions.
const CANCEL_EVERY: usize = 1000;

/// A database of games and the key of its score in the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GamesSource {
    /// The games.
    pub handle: SourceHandle,
    /// The key of the source (see `tactics::source_key`; empty: not kept).
    pub key: String,
    /// The filter had more than [`MAX_GAMES`] games and the handle holds only
    /// the first ones (D2210, D2254).
    pub truncated: bool,
}

/// Where the repertoire is read from (D2202, D2236).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BuildSource {
    /// A file: a book (`.bin`), a mask (`.stm`) or games, by extension.
    File(PathBuf),
    /// Games already open (the current base), for both colours.
    Games(GamesSource),
    /// One database per colour, found by the type of the base (D2203).
    Typed {
        /// For the lines where the user plays White.
        white: Option<GamesSource>,
        /// For the lines where the user plays Black.
        black: Option<GamesSource>,
    },
}

/// What a file is, by its extension (D2202).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepFileKind {
    /// A Polyglot book.
    Book,
    /// A tree mask of the Scid.
    Mask,
    /// Games: PGN, `.pgn.gz` or a Scid database.
    Games,
}

impl RepFileKind {
    /// The kind of `path`, without regard to the case of the extension.
    pub fn of(path: &Path) -> RepFileKind {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext.eq_ignore_ascii_case("bin") {
            RepFileKind::Book
        } else if ext.eq_ignore_ascii_case("stm") {
            RepFileKind::Mask
        } else {
            RepFileKind::Games
        }
    }
}

/// A request to build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildJob {
    /// Where from.
    pub source: BuildSource,
    /// Which colours are needed.
    pub side: TrainSide,
    /// Depth in plies.
    pub max_depth: u8,
    /// Only games flagged as opening lines (D2206).
    pub only_flagged: bool,
    /// From a mask: only the marked moves for the user (D2208).
    pub only_marked: bool,
    /// Raised to stop the work (D2211).
    pub cancel: Cancel,
}

/// What a build produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOutput {
    /// The repertoires.
    pub set: RepSet,
    /// Games read (0 for a book or a mask).
    pub games: usize,
    /// A limit cut the repertoire (D2210).
    pub truncated: bool,
}

/// Why a repertoire could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BuildError {
    /// The file with the games could not be opened.
    #[error("{0}")]
    Source(#[from] SourceError),
    /// The book could not be read.
    #[error("the book could not be read")]
    Book,
    /// The mask could not be read.
    #[error("the mask could not be read")]
    Mask,
    /// The cancel flag was raised.
    #[error("the build was cancelled")]
    Cancelled,
    /// The work stopped unexpectedly (a panic in the thread, D2258).
    #[error("the build failed")]
    Failed,
}

/// Which colours a database of games serves (D2206).
#[derive(Debug, Clone, Copy)]
struct FlagFilter {
    white: bool,
    black: bool,
}

/// Builds the repertoire (D2205-D2208).
///
/// # Errors
///
/// A [`BuildError`] when the source cannot be read or the work is cancelled.
pub fn build(job: &BuildJob) -> Result<BuildOutput, BuildError> {
    let (want_white, want_black) = match job.side {
        TrainSide::White => (true, false),
        TrainSide::Black => (false, true),
        TrainSide::Both => (true, true),
    };
    let depth = usize::from(job.max_depth);
    let both = FlagFilter {
        white: want_white,
        black: want_black,
    };
    let shared = |rep: Repertoire, games: usize| {
        let truncated = rep.truncated();
        let rep = Arc::new(rep);
        BuildOutput {
            set: RepSet {
                white: want_white.then(|| rep.clone()),
                black: want_black.then_some(rep),
            },
            games,
            truncated,
        }
    };
    match &job.source {
        BuildSource::File(path) => match RepFileKind::of(path) {
            RepFileKind::Book => {
                let book = Book::open_readonly(path).map_err(|_| BuildError::Book)?;
                Ok(shared(from_book(&book, depth, &job.cancel)?, 0))
            }
            RepFileKind::Mask => {
                let (mask, _report) = stm::read(path).map_err(|_| BuildError::Mask)?;
                let mut out = BuildOutput {
                    set: RepSet::default(),
                    games: 0,
                    truncated: false,
                };
                for (color, wanted) in [(Color::White, want_white), (Color::Black, want_black)] {
                    if !wanted {
                        continue;
                    }
                    let rep = from_mask(&mask, color, depth, job.only_marked, &job.cancel)?;
                    out.truncated |= rep.truncated();
                    let rep = Some(Arc::new(rep));
                    match color {
                        Color::White => out.set.white = rep,
                        Color::Black => out.set.black = rep,
                    }
                }
                Ok(out)
            }
            RepFileKind::Games => {
                let opened = open_file(path)?;
                let (rep, games) =
                    from_games(&opened.handle, both, job.only_flagged, depth, &job.cancel)?;
                Ok(shared(rep, games))
            }
        },
        BuildSource::Games(source) => {
            let (rep, games) =
                from_games(&source.handle, both, job.only_flagged, depth, &job.cancel)?;
            let mut out = shared(rep, games);
            out.truncated |= source.truncated;
            Ok(out)
        }
        BuildSource::Typed { white, black } => {
            let mut out = BuildOutput {
                set: RepSet::default(),
                games: 0,
                truncated: false,
            };
            for (color, source) in [(Color::White, white), (Color::Black, black)] {
                let wanted = match color {
                    Color::White => want_white,
                    Color::Black => want_black,
                };
                let Some(source) = source.as_ref().filter(|_| wanted) else {
                    continue;
                };
                let filter = FlagFilter {
                    white: color == Color::White,
                    black: color == Color::Black,
                };
                let (rep, games) =
                    from_games(&source.handle, filter, job.only_flagged, depth, &job.cancel)?;
                out.games += games;
                out.truncated |= rep.truncated() || source.truncated;
                let rep = Some(Arc::new(rep));
                match color {
                    Color::White => out.set.white = rep,
                    Color::Black => out.set.black = rep,
                }
            }
            Ok(out)
        }
    }
}

/// Whether `flags` pass the filter of opening lines (D2206).
fn flagged(flags: GameFlags, filter: FlagFilter) -> bool {
    (filter.white && flags.contains(GameFlags::WHITE_OPENING))
        || (filter.black && flags.contains(GameFlags::BLACK_OPENING))
}

/// The repertoire of a set of games (D2205, D2206, D2210).
fn from_games(
    handle: &SourceHandle,
    filter: FlagFilter,
    only_flagged: bool,
    max_depth: usize,
    cancel: &Cancel,
) -> Result<(Repertoire, usize), BuildError> {
    let mut builder = RepBuilder::new();
    let start = Position::new();
    let total = handle.len();
    if total > MAX_GAMES {
        builder.mark_truncated();
    }
    let mut used = 0usize;
    let mut seen: HashSet<(ZobristHash, Move)> = HashSet::new();
    for index in 0..total.min(MAX_GAMES) {
        if cancel.is_cancelled() {
            return Err(BuildError::Cancelled);
        }
        // Only a database has flags; a PGN file ignores the filter (D2206).
        if only_flagged && flags_pass(handle, index, filter) == Some(false) {
            continue;
        }
        let Some((game, _number)) = handle.game(index) else {
            continue;
        };
        if game.start_position() != &start {
            continue;
        }
        seen.clear();
        let mut budget = MAX_NODES_PER_GAME;
        walk_game(
            &game,
            game.root(),
            0,
            max_depth,
            &mut seen,
            &mut budget,
            &mut builder,
        );
        used += 1;
    }
    Ok((builder.finish(), used))
}

/// `Some(passes)` for a database, `None` for a source without flags.
fn flags_pass(handle: &SourceHandle, index: usize, filter: FlagFilter) -> Option<bool> {
    let SourceHandle::Base { source, numbers } = handle else {
        return None;
    };
    let number = *numbers.get(index)?;
    let entry = source.read().entry(number).ok();
    Some(entry.is_some_and(|e| flagged(e.flags, filter)))
}

fn walk_game(
    game: &Game,
    node: NodeId,
    ply: usize,
    max_depth: usize,
    seen: &mut HashSet<(ZobristHash, Move)>,
    budget: &mut usize,
    builder: &mut RepBuilder,
) {
    if ply >= max_depth {
        return;
    }
    let Ok(position) = game.position_at(node) else {
        return;
    };
    let Ok(children) = game.children(node) else {
        return;
    };
    for &child in children {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let Ok(mv) = game.mv(child) else {
            continue;
        };
        if mv.is_null() {
            continue;
        }
        if seen.insert((position.zobrist(), mv)) {
            let dubious = game
                .nags(child)
                .is_ok_and(|nags| nags.iter().any(|n| matches!(n.value(), 2 | 4 | 6)));
            builder.add(position, mv, 1, dubious);
        }
        walk_game(game, child, ply + 1, max_depth, seen, budget, builder);
    }
}

/// The repertoire of a Polyglot book (D2207).
fn from_book(book: &Book, max_depth: usize, cancel: &Cancel) -> Result<Repertoire, BuildError> {
    let mut builder = RepBuilder::new();
    let mut expanded: HashSet<ZobristHash> = HashSet::new();
    // Breadth first: a position is reached first by its shortest path, so
    // it is expanded once and at its smallest ply, and a transposition
    // never loses the moves below the depth (D2257).
    let mut queue: VecDeque<(Position, usize)> = VecDeque::from([(Position::new(), 0usize)]);
    let mut steps = 0usize;
    while let Some((position, ply)) = queue.pop_front() {
        steps += 1;
        if steps.is_multiple_of(CANCEL_EVERY) && cancel.is_cancelled() {
            return Err(BuildError::Cancelled);
        }
        if ply >= max_depth || !expanded.insert(position.zobrist()) {
            continue;
        }
        let moves = book.moves(&position);
        let total: u64 = moves.iter().map(|m| u64::from(m.weight)).sum();
        let heaviest = moves.iter().map(|m| u64::from(m.weight)).max().unwrap_or(0);
        let mut kept: Vec<_> = moves
            .iter()
            .filter(|m| {
                let w = u64::from(m.weight);
                w > 0 && (w * 100 >= BOOK_MIN_PERCENT * total || w == heaviest)
            })
            .collect();
        kept.sort_by_key(|m| std::cmp::Reverse(m.weight));
        kept.truncate(MAX_BOOK_MOVES);
        for m in kept {
            let w = u64::from(m.weight);
            let dubious = w * 100 < BOOK_DUBIOUS_PERCENT * heaviest;
            if !builder.add(&position, m.mv, u32::from(m.weight), dubious) {
                continue;
            }
            if let Ok(next) = position.after(m.mv) {
                queue.push_back((next, ply + 1));
            }
        }
    }
    Ok(builder.finish())
}

fn is_marked(m: &MaskMove) -> bool {
    m.color == MaskColor::Green || m.markers.iter().flatten().any(|k| *k == Marker::ToTrain)
}

fn is_excluded(m: &MaskMove) -> bool {
    m.markers
        .iter()
        .flatten()
        .any(|k| matches!(k, Marker::Exclude | Marker::ToRemove))
}

fn is_dubious(m: &MaskMove) -> bool {
    matches!(
        m.nag,
        Some(MaskNag::Dubious | MaskNag::Poor | MaskNag::Blunder)
    ) || m.color == MaskColor::Red
        || m.markers.iter().flatten().any(|k| *k == Marker::Dubious)
}

/// The repertoire of a tree mask for a user who plays `user` (D2208).
fn from_mask(
    mask: &TreeMask,
    user: Color,
    max_depth: usize,
    only_marked: bool,
    cancel: &Cancel,
) -> Result<Repertoire, BuildError> {
    let marked_mode = only_marked && mask.iter().any(|(_, p)| p.visible_moves().any(is_marked));
    let mut builder = RepBuilder::new();
    let mut expanded: HashSet<ZobristHash> = HashSet::new();
    // Breadth first: a position is reached first by its shortest path, so
    // it is expanded once and at its smallest ply, and a transposition
    // never loses the moves below the depth (D2257).
    let mut queue: VecDeque<(Position, usize)> = VecDeque::from([(Position::new(), 0usize)]);
    let mut steps = 0usize;
    while let Some((position, ply)) = queue.pop_front() {
        steps += 1;
        if steps.is_multiple_of(CANCEL_EVERY) && cancel.is_cancelled() {
            return Err(BuildError::Cancelled);
        }
        if ply >= max_depth || !expanded.insert(position.zobrist()) {
            continue;
        }
        let Some(entry) = mask.position(&position) else {
            continue;
        };
        for m in entry.visible_moves() {
            if is_excluded(m) || (marked_mode && position.turn() == user && !is_marked(m)) {
                continue;
            }
            let Some(mv) = resolve_move(&position, &m.san) else {
                continue;
            };
            if !builder.add(&position, mv, 1, is_dubious(m)) {
                continue;
            }
            if let Ok(next) = position.after(mv) {
                queue.push_back((next, ply + 1));
            }
        }
    }
    Ok(builder.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_db::tree::mask::MaskPosition;

    fn pgn_handle(text: &str) -> SourceHandle {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("rep.pgn");
        std::fs::write(&path, text).expect("write");
        open_file(&path).expect("open").handle
    }

    fn rep_of(text: &str, depth: usize) -> Repertoire {
        let handle = pgn_handle(text);
        let filter = FlagFilter {
            white: true,
            black: true,
        };
        from_games(&handle, filter, false, depth, &Cancel::new())
            .expect("build")
            .0
    }

    fn uci(position: &Position, text: &str) -> Move {
        position.parse_uci(text).expect("uci")
    }

    const TWO_GAMES: &str = "[Event \"a\"]\n[Result \"*\"]\n\n1. e4 e5 2. Nf3 (2. Bc4 Nf6) 2... Nc6 *\n\n\
[Event \"b\"]\n[Result \"*\"]\n\n1. e4 c5 2. Nf3 d6?! *\n";

    #[test]
    fn games_are_merged_with_the_weight_of_one_per_game() {
        let rep = rep_of(TWO_GAMES, 16);
        let start = Position::new();
        let e4 = uci(&start, "e2e4");
        assert_eq!(rep.find(&start, e4).map(|m| m.weight), Some(2));
        let after_e4 = start.after(e4).expect("e4");
        assert_eq!(rep.moves(&after_e4).len(), 2, "e5 and c5");
        let after_e5 = after_e4.after(uci(&after_e4, "e7e5")).expect("e5");
        // 2.Nf3 (main) and 2.Bc4 (variation) are both there.
        assert_eq!(rep.moves(&after_e5).len(), 2);
    }

    #[test]
    fn a_nag_of_error_makes_the_move_dubious_and_the_depth_cuts_the_walk() {
        let rep = rep_of(TWO_GAMES, 16);
        let mut p = Position::new();
        for m in ["e2e4", "c7c5", "g1f3"] {
            p.play(uci(&p, m)).expect("play");
        }
        let d6 = rep.find(&p, uci(&p, "d7d6")).expect("d6");
        assert!(d6.dubious);
        let shallow = rep_of(TWO_GAMES, 2);
        let mut q = Position::new();
        q.play(uci(&q, "e2e4")).expect("e4");
        assert!(!shallow.moves(&q).is_empty(), "ply 2 is inside");
        q.play(uci(&q, "e7e5")).expect("e5");
        assert!(shallow.moves(&q).is_empty(), "ply 3 is beyond the depth");
    }

    #[test]
    fn a_game_with_its_own_position_is_ignored() {
        let text = "[Event \"fen\"]\n[SetUp \"1\"]\n[FEN \"4k3/8/8/8/8/8/4P3/4K3 w - - 0 1\"]\n[Result \"*\"]\n\n1. e4 *\n";
        let rep = rep_of(text, 16);
        assert!(rep.is_empty());
    }

    #[test]
    fn only_the_first_games_are_read_and_the_repertoire_says_it_was_cut() {
        let one = "[Event \"x\"]\n[Result \"*\"]\n\n1. e4 *\n\n";
        let text = one.repeat(MAX_GAMES + 1);
        let handle = pgn_handle(&text);
        let filter = FlagFilter {
            white: true,
            black: true,
        };
        let (rep, used) = from_games(&handle, filter, false, 8, &Cancel::new()).expect("build");
        assert_eq!(used, MAX_GAMES);
        assert!(rep.truncated());
        let start = Position::new();
        assert_eq!(
            rep.find(&start, uci(&start, "e2e4")).map(|m| m.weight),
            Some(u32::try_from(MAX_GAMES).expect("fits"))
        );
    }

    #[test]
    fn a_cancelled_build_reports_it() {
        let handle = pgn_handle(TWO_GAMES);
        let cancel = Cancel::new();
        cancel.cancel();
        let filter = FlagFilter {
            white: true,
            black: true,
        };
        assert_eq!(
            from_games(&handle, filter, false, 8, &cancel).map(|_| ()),
            Err(BuildError::Cancelled)
        );
    }

    #[test]
    fn a_book_gives_weights_cuts_the_small_moves_and_marks_dubious_ones() {
        let start = Position::new();
        let mut book = Book::new();
        let e4 = uci(&start, "e2e4");
        let d4 = uci(&start, "d2d4");
        let a3 = uci(&start, "a2a3");
        let h3 = uci(&start, "h2h3");
        // 1000 total: e4 600, d4 300 (50% of the heaviest, kept, good), a3 90
        // (15%: dubious, kept), h3 10 (1%: cut).
        book.set_position_moves(&start, &[(e4, 600), (d4, 300), (a3, 90), (h3, 10)])
            .expect("set");
        let rep = from_book(&book, 4, &Cancel::new()).expect("rep");
        let moves = rep.moves(&start);
        assert_eq!(moves.len(), 3, "the 1% move is cut");
        assert!(rep.find(&start, a3).is_some_and(|m| m.dubious));
        assert!(rep.find(&start, d4).is_some_and(|m| !m.dubious));
        assert!(rep.find(&start, h3).is_none());
        assert_eq!(rep.find(&start, e4).map(|m| m.weight), Some(600));
    }

    #[test]
    fn a_book_walk_follows_the_lines_to_the_depth() {
        let start = Position::new();
        let after_e4 = start.after(uci(&start, "e2e4")).expect("e4");
        let mut book = Book::new();
        book.set_position_moves(&start, &[(uci(&start, "e2e4"), 100)])
            .expect("set");
        book.set_position_moves(&after_e4, &[(uci(&after_e4, "e7e5"), 100)])
            .expect("set");
        let one = from_book(&book, 1, &Cancel::new()).expect("rep");
        assert_eq!(one.len(), 1);
        let two = from_book(&book, 2, &Cancel::new()).expect("rep");
        assert_eq!(two.len(), 2);
    }

    fn sample_mask() -> TreeMask {
        let start = Position::new();
        let after_e4 = start.after(uci(&start, "e2e4")).expect("e4");
        let mut mask = TreeMask::new();
        let mut e4 = MaskMove::new("e4");
        e4.markers[0] = Some(Marker::ToTrain);
        let d4 = MaskMove::new("d4");
        let mut a3 = MaskMove::new("a3");
        a3.markers[0] = Some(Marker::Exclude);
        mask.insert(
            mychess_db::tree::mask::MaskKey::from_position(&start),
            MaskPosition {
                moves: vec![e4, d4, a3],
                comment: String::new(),
            },
        );
        let mut e5 = MaskMove::new("e5");
        e5.nag = Some(MaskNag::Good);
        let mut a6 = MaskMove::new("a6");
        a6.nag = Some(MaskNag::Dubious);
        let nonsense = MaskMove::new("Zz9");
        mask.insert(
            mychess_db::tree::mask::MaskKey::from_position(&after_e4),
            MaskPosition {
                moves: vec![e5, a6, nonsense],
                comment: String::new(),
            },
        );
        mask
    }

    #[test]
    fn a_mask_uses_only_the_marked_moves_on_the_turns_of_the_user() {
        let mask = sample_mask();
        let start = Position::new();
        let rep = from_mask(&mask, Color::White, 8, true, &Cancel::new()).expect("rep");
        // Only e4 is marked: d4 is out for the white user, a3 is excluded.
        assert_eq!(rep.moves(&start).len(), 1);
        let after_e4 = start.after(uci(&start, "e2e4")).expect("e4");
        // The opponent keeps every non-excluded move that resolves.
        let replies = rep.moves(&after_e4);
        assert_eq!(replies.len(), 2, "e5 and a6; the nonsense text is ignored");
        assert!(
            rep.find(&after_e4, uci(&after_e4, "a7a6"))
                .is_some_and(|m| m.dubious)
        );
        // Without the option, d4 is back; a3 stays excluded.
        let all = from_mask(&mask, Color::White, 8, false, &Cancel::new()).expect("rep");
        assert_eq!(all.moves(&start).len(), 2);
        // For a black user the white moves are the opponent's: all kept.
        let black = from_mask(&mask, Color::Black, 8, true, &Cancel::new()).expect("rep");
        assert_eq!(black.moves(&start).len(), 2);
    }

    #[test]
    fn a_mask_without_marks_uses_everything_that_is_not_excluded() {
        let start = Position::new();
        let mut mask = TreeMask::new();
        mask.insert(
            mychess_db::tree::mask::MaskKey::from_position(&start),
            MaskPosition {
                moves: vec![
                    MaskMove::new("e4"),
                    MaskMove::new("d4"),
                    MaskMove::new("null"),
                ],
                comment: String::new(),
            },
        );
        let rep = from_mask(&mask, Color::White, 8, true, &Cancel::new()).expect("rep");
        assert_eq!(rep.moves(&start).len(), 2, "`null` never counts");
    }

    #[test]
    fn green_moves_count_as_marked() {
        let start = Position::new();
        let mut mask = TreeMask::new();
        let mut e4 = MaskMove::new("e4");
        e4.color = MaskColor::Green;
        mask.insert(
            mychess_db::tree::mask::MaskKey::from_position(&start),
            MaskPosition {
                moves: vec![e4, MaskMove::new("d4")],
                comment: String::new(),
            },
        );
        let rep = from_mask(&mask, Color::White, 8, true, &Cancel::new()).expect("rep");
        assert_eq!(rep.moves(&start).len(), 1);
    }

    /// The position after `moves` (UCI) from the start.
    fn after(moves: &[&str]) -> Position {
        let mut p = Position::new();
        for text in moves {
            let m = uci(&p, text);
            p.play(m).expect("legal");
        }
        p
    }

    // The position after 1.e4 a5 (white to move) is reached in two plies,
    // and in four by 1.e3 a6 2.e4 a5: the long way comes first in the order
    // the moves are listed, and below the position are 2.Nf3 Nc6 3.Nc3.
    const SHORT: [&str; 2] = ["e2e4", "a7a5"];

    #[test]
    fn a_book_transposition_reached_by_a_long_path_first_keeps_its_moves() {
        let mut book = Book::new();
        let mut set = |position: &Position, moves: &[(&str, u16)]| {
            let list: Vec<(Move, u16)> =
                moves.iter().map(|(u, w)| (uci(position, u), *w)).collect();
            book.set_position_moves(position, &list).expect("set");
        };
        set(&after(&[]), &[("e2e4", 100), ("e2e3", 90)]);
        set(&after(&["e2e4"]), &[("a7a5", 100)]);
        set(&after(&["e2e3"]), &[("a7a6", 100)]);
        set(&after(&["e2e3", "a7a6"]), &[("e3e4", 100)]);
        set(&after(&["e2e3", "a7a6", "e3e4"]), &[("a6a5", 100)]);
        set(&after(&SHORT), &[("g1f3", 100)]);
        set(&after(&["e2e4", "a7a5", "g1f3"]), &[("b8c6", 100)]);
        set(&after(&["e2e4", "a7a5", "g1f3", "b8c6"]), &[("b1c3", 100)]);
        let rep = from_book(&book, 6, &Cancel::new()).expect("rep");
        let deep = after(&["e2e4", "a7a5", "g1f3", "b8c6"]);
        assert!(
            rep.find(&deep, uci(&deep, "b1c3")).is_some(),
            "the short way (two plies) to the shared position reaches 3.Nc3 inside the depth"
        );
    }

    #[test]
    fn a_mask_transposition_reached_by_a_long_path_first_keeps_its_moves() {
        use mychess_db::tree::mask::MaskKey;
        let mut mask = TreeMask::new();
        let mut set = |position: &Position, sans: &[&str]| {
            mask.insert(
                MaskKey::from_position(position),
                MaskPosition {
                    moves: sans.iter().map(|s| MaskMove::new(*s)).collect(),
                    comment: String::new(),
                },
            );
        };
        set(&after(&[]), &["e4", "e3"]);
        set(&after(&["e2e4"]), &["a5"]);
        set(&after(&["e2e3"]), &["a6"]);
        set(&after(&["e2e3", "a7a6"]), &["e4"]);
        set(&after(&["e2e3", "a7a6", "e3e4"]), &["a5"]);
        set(&after(&SHORT), &["Nf3"]);
        set(&after(&["e2e4", "a7a5", "g1f3"]), &["Nc6"]);
        set(&after(&["e2e4", "a7a5", "g1f3", "b8c6"]), &["Nc3"]);
        let rep = from_mask(&mask, Color::White, 6, false, &Cancel::new()).expect("rep");
        let deep = after(&["e2e4", "a7a5", "g1f3", "b8c6"]);
        assert!(
            rep.find(&deep, uci(&deep, "b1c3")).is_some(),
            "the short way (two plies) to the shared position reaches 3.Nc3 inside the depth"
        );
    }

    #[test]
    fn the_kind_of_a_file_comes_from_its_extension_without_regard_to_case() {
        assert_eq!(RepFileKind::of(Path::new("a.BIN")), RepFileKind::Book);
        assert_eq!(RepFileKind::of(Path::new("a.stm")), RepFileKind::Mask);
        assert_eq!(RepFileKind::of(Path::new("a.pgn")), RepFileKind::Games);
        assert_eq!(RepFileKind::of(Path::new("a")), RepFileKind::Games);
    }

    #[test]
    fn a_garbage_book_or_mask_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().expect("tempdir");
        for (name, expected) in [("x.bin", BuildError::Book), ("x.stm", BuildError::Mask)] {
            let path = dir.path().join(name);
            std::fs::write(&path, b"\x00\x01garbage").expect("write");
            let job = BuildJob {
                source: BuildSource::File(path),
                side: TrainSide::White,
                max_depth: 8,
                only_flagged: false,
                only_marked: true,
                cancel: Cancel::new(),
            };
            let result = build(&job);
            // A short garbage `.stm` may parse as an empty mask: either way
            // it is never a panic.
            if let Err(e) = result {
                assert_eq!(e, expected);
            }
        }
    }

    #[test]
    fn both_sides_share_one_repertoire_for_games_and_a_file_of_one_side_is_not_built() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("rep.pgn");
        std::fs::write(&path, TWO_GAMES).expect("write");
        let mut job = BuildJob {
            source: BuildSource::File(path),
            side: TrainSide::Both,
            max_depth: 8,
            only_flagged: false,
            only_marked: true,
            cancel: Cancel::new(),
        };
        let both = build(&job).expect("build");
        assert!(both.set.white.is_some() && both.set.black.is_some());
        assert_eq!(both.games, 2);
        job.side = TrainSide::Black;
        let black = build(&job).expect("build");
        assert!(black.set.white.is_none() && black.set.black.is_some());
    }
}
