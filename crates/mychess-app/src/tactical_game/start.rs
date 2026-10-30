//! Where a tactical game starts from (spec M8-7, D2457-D2460): a typed
//! FEN, a position drawn from a game of a file or of the current database.
//!
//! The drawing reads files: [`run_pick`] blocks and runs only from
//! [`perform`](crate::perform) in `spawn_blocking`.

use std::path::PathBuf;

use mychess_core::{Game, Position};

use crate::play::rules::{position_end, repetition_key};
use crate::tactics::{Cancel, SourceHandle, coprime_step, extract_problems, open_file, walk_index};

/// The longest FEN text read (a real FEN has under 100 characters).
pub const MAX_FEN_CHARS: usize = 200;
/// Games a drawing may look at before it gives up (D2460).
pub const PICK_TRIES: usize = 200;
/// The first half-move of an ordinary game that may be drawn (D2460).
pub const MIN_PICK_PLY: usize = 6;
/// The last half-move of an ordinary game that may be drawn (D2460).
pub const MAX_PICK_PLY: usize = 20;

/// Why a start position cannot be used (D2458, D2459).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StartProblem {
    /// The text is not a FEN the game can play from.
    BadFen,
    /// The position has no legal move or the game is over in it.
    Ended,
    /// "Problem of the tactics training" with no problem on the board.
    NoProblem,
    /// "Position from a file" with no file chosen.
    NoFile,
    /// "Specific opening" with no opening chosen.
    NoOpening,
}

/// A FEN typed by the user, as a position to play from (D2458).
///
/// # Errors
///
/// [`StartProblem::BadFen`] for text that is not a FEN, too long or empty;
/// [`StartProblem::Ended`] when the position is already mate, stalemate or
/// a draw by insufficient material, so there is nothing to play.
pub fn parse_start_fen(text: &str) -> Result<Position, StartProblem> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_FEN_CHARS {
        return Err(StartProblem::BadFen);
    }
    let position = Position::from_fen(text).map_err(|_| StartProblem::BadFen)?;
    if !playable(&position) {
        return Err(StartProblem::Ended);
    }
    Ok(position)
}

/// Whether a game can be played from `position`: it has a legal move and
/// is not over by the position alone.
#[must_use]
pub fn playable(position: &Position) -> bool {
    !position.legal_moves().is_empty()
        && position_end(position, &[repetition_key(position)]).is_none()
}

/// Where a position is drawn from (D2460).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickSource {
    /// A PGN file (also `.pgn.gz`) or a Scid database (`.si4`).
    File(PathBuf),
    /// The games of the filter of the current database.
    CurrentBase,
}

/// A position drawn from a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picked {
    /// The position to play from.
    pub position: Position,
    /// A short name for the notice: the event or the players.
    pub label: String,
}

/// Why nothing was drawn (D2460).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PickFailure {
    /// The source has no game.
    Empty,
    /// The file could not be read.
    Unreadable,
    /// No game of the source gave a position to play from.
    NoPosition,
    /// The drawing was cancelled.
    Cancelled,
}

/// What the drawing reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickInput {
    /// A file to open.
    File(PathBuf),
    /// Games that were already opened (the current database).
    Handle(SourceHandle),
}

/// A request to draw a position (D2460).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickJob {
    /// What to read.
    pub input: PickInput,
    /// Decides where the walk starts and which position of a game.
    pub seed: u64,
    /// Raised to stop it early.
    pub cancel: Cancel,
}

/// The mix of the seed with the step of the walk (SplitMix64): every game
/// gets its own draw.
fn mix(seed: u64, step: u64) -> u64 {
    let mut z = seed.wrapping_add(step.wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn label_of(game: &Game) -> String {
    let tags = game.tags();
    if tags.event.trim().is_empty() || tags.event == "?" {
        format!("{} - {}", tags.white, tags.black)
    } else {
        tags.event.clone()
    }
}

/// The position drawn from one game, or `None` when the game has none to
/// offer (D2460): a game with a problem (the marker `****D` or a start
/// other than the standard one) offers the position of one of its
/// problems; an ordinary game offers the position after `MIN_PICK_PLY` to
/// `MAX_PICK_PLY` half-moves of its main line. `roll` decides.
#[must_use]
pub fn pick_in_game(game: &Game, game_number: u32, roll: u64) -> Option<Picked> {
    let problems = extract_problems(game, game_number);
    if !problems.is_empty() {
        let at = usize::try_from(roll % u64::try_from(problems.len()).ok()?).ok()?;
        let problem = problems.get(at)?;
        return playable(&problem.position).then(|| Picked {
            position: problem.position.clone(),
            label: problem.title.clone(),
        });
    }
    // The main line does not include the root: `nodes[k - 1]` is the node
    // after `k` half-moves.
    let nodes: Vec<_> = game.mainline().collect();
    let plies = nodes.len();
    if plies <= MIN_PICK_PLY {
        return None;
    }
    let high = MAX_PICK_PLY.min(plies - 1);
    let span = u64::try_from(high - MIN_PICK_PLY + 1).ok()?;
    let ply = MIN_PICK_PLY + usize::try_from((roll >> 8) % span).ok()?;
    let node = *nodes.get(ply - 1)?;
    let position = game.position_at(node).ok()?;
    playable(position).then(|| Picked {
        position: position.clone(),
        label: label_of(game),
    })
}

/// Draws a position (D2460). Blocking.
///
/// # Errors
///
/// A [`PickFailure`]: nothing to read, nothing usable, or cancelled.
pub fn run_pick(job: &PickJob) -> Result<Picked, PickFailure> {
    let handle = match &job.input {
        PickInput::Handle(handle) => handle.clone(),
        PickInput::File(path) => open_file(path).map_err(|_| PickFailure::Unreadable)?.handle,
    };
    let len = handle.len();
    if len == 0 {
        return Err(PickFailure::Empty);
    }
    let start = usize::try_from(job.seed % u64::try_from(len).unwrap_or(1)).unwrap_or(0);
    let step = coprime_step(job.seed >> 16, len);
    for k in 0..len.min(PICK_TRIES) {
        if job.cancel.is_cancelled() {
            return Err(PickFailure::Cancelled);
        }
        let index = walk_index(start, step, k, len);
        let Some((game, number)) = handle.game(index) else {
            continue;
        };
        let roll = mix(job.seed, u64::try_from(k).unwrap_or(0));
        if let Some(picked) = pick_in_game(&game, number, roll) {
            return Ok(picked);
        }
    }
    Err(PickFailure::NoPosition)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ordinary(plies: usize) -> Game {
        // A shuffle of knights keeps the game legal for any length.
        let mut game = Game::new();
        let mut cursor = game.root();
        let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
        for i in 0..plies {
            let position = game.position_at(cursor).expect("node").clone();
            let mv = position.parse_uci(cycle[i % 4]).expect("uci");
            cursor = game.add_move(cursor, mv).expect("move");
        }
        game
    }

    #[test]
    fn a_fen_is_read_trimmed_and_checked() {
        let start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        assert_eq!(
            parse_start_fen(&format!("  {start}  ")),
            Ok(Position::new())
        );
        assert_eq!(parse_start_fen(""), Err(StartProblem::BadFen));
        assert_eq!(parse_start_fen("nonsense"), Err(StartProblem::BadFen));
        assert_eq!(
            parse_start_fen(&"8/".repeat(500)),
            Err(StartProblem::BadFen)
        );
        // Mate, stalemate and a bare-kings draw leave nothing to play.
        assert_eq!(
            parse_start_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1"),
            Err(StartProblem::Ended)
        );
        assert_eq!(
            parse_start_fen("k7/8/1QK5/8/8/8/8/8 b - - 0 1"),
            Err(StartProblem::Ended)
        );
        assert_eq!(
            parse_start_fen("8/8/8/4k3/8/3K4/8/8 w - - 0 1"),
            Err(StartProblem::Ended)
        );
    }

    #[test]
    fn a_fen_with_the_black_to_move_is_a_position_like_any_other() {
        let fen = "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 2 2";
        assert!(parse_start_fen(fen).is_ok());
    }

    #[test]
    fn an_ordinary_game_gives_a_position_between_six_and_twenty_plies() {
        let game = ordinary(40);
        for roll in [0_u64, 1, 255, 256, 9999, u64::MAX] {
            let picked = pick_in_game(&game, 1, roll).expect("a position");
            let found = (MIN_PICK_PLY..=MAX_PICK_PLY).any(|ply| {
                let node = game.mainline().nth(ply - 1).expect("node");
                game.position_at(node).expect("position") == &picked.position
            });
            assert!(found, "roll {roll}");
        }
    }

    #[test]
    fn the_same_roll_gives_the_same_position() {
        let game = ordinary(40);
        assert_eq!(pick_in_game(&game, 1, 77), pick_in_game(&game, 1, 77));
    }

    #[test]
    fn a_short_game_gives_nothing() {
        assert_eq!(pick_in_game(&ordinary(6), 1, 0), None);
        assert_eq!(pick_in_game(&Game::new(), 1, 0), None);
        assert!(pick_in_game(&ordinary(7), 1, 0).is_some());
    }

    #[test]
    fn a_game_with_its_own_start_gives_that_start() {
        let fen = "6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1";
        let game = Game::from_position(Position::from_fen(fen).expect("fen"));
        let picked = pick_in_game(&game, 3, 5).expect("the problem");
        assert_eq!(picked.position.fen(), fen);
    }

    #[test]
    fn a_problem_that_is_already_over_is_not_offered() {
        let game =
            Game::from_position(Position::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").expect("fen"));
        assert_eq!(pick_in_game(&game, 1, 0), None);
    }

    #[test]
    fn a_marker_offers_the_position_at_the_marker() {
        let mut game = ordinary(10);
        let node = game.mainline().nth(4).expect("node");
        game.set_comment(node, "****D3 a->b").expect("comment");
        let picked = pick_in_game(&game, 1, 0).expect("marked");
        assert_eq!(&picked.position, game.position_at(node).expect("position"));
    }
}
