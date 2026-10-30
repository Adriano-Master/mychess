//! A tactical problem and how it is found inside a game (spec M8-4, D1902,
//! D1903, D1904, D1905).
//!
//! Scid: `tcl/tools/tactics.tcl`, `gotoNextTacticMarker` (the `****D`
//! comment marks the position before the move to find) and `findBestMove`
//! (a game with a non-standard start is an exercise by itself).

use mychess_core::{Game, Move, NodeId, Position};

/// The text that the automatic annotation writes before a tactical move
/// (`annotate::exercise_comment`, M4-6).
pub const MARKER: &str = "****D";

/// Longest solution kept, in plies (D1902).
pub const MAX_SOLUTION_PLIES: usize = 40;

/// Most problems taken from one game (D1905).
pub const MAX_PROBLEMS_PER_GAME: usize = 200;

/// Tags that may carry the Elo of a problem (D1914), in order of
/// preference.
const RATING_TAGS: [&str; 3] = ["PuzzleRating", "Rating", "Elo"];

/// One exercise: a position and, perhaps, the line that solves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The identity in its source (D1904): see [`problem_id`].
    pub id: u64,
    /// The number of the game in the source (from 1).
    pub game_number: u32,
    /// The ply of the position in its game (0 for the start).
    pub ply: u16,
    /// The position to solve.
    pub position: Position,
    /// The main line that follows the position, at most
    /// [`MAX_SOLUTION_PLIES`] plies. May be empty.
    pub solution: Vec<Move>,
    /// The difficulty of a `****D<n>` marker (1 to 9).
    pub difficulty: Option<u8>,
    /// The Elo of the problem, from the tags of its game.
    pub rating: Option<u16>,
    /// A short name for the panel: the event, or the players.
    pub title: String,
}

/// The identity of a problem: the number of the game in the high bits and
/// the ply in the low 16.
pub fn problem_id(game_number: u32, ply: u32) -> u64 {
    (u64::from(game_number) << 16) | u64::from(ply.min(u32::from(u16::MAX)))
}

/// What a comment says about the marker.
struct Marked {
    /// The digit written after `****D`, if any.
    difficulty: Option<u8>,
}

/// The marker of `comment`, if it has one.
fn marker_of(comment: &str) -> Option<Marked> {
    let at = comment.find(MARKER)?;
    let digit = comment[at + MARKER.len()..]
        .chars()
        .next()
        .and_then(|c| c.to_digit(10))
        .and_then(|d| u8::try_from(d).ok())
        .filter(|d| (1..=9).contains(d));
    Some(Marked { difficulty: digit })
}

fn rating_of(game: &Game) -> Option<u16> {
    RATING_TAGS.iter().find_map(|name| {
        game.tags()
            .get_extra(name)
            .and_then(|text| text.trim().parse::<u16>().ok())
            .filter(|elo| (1..=4000).contains(elo))
    })
}

fn title_of(game: &Game) -> String {
    let tags = game.tags();
    let known = |text: &str| !text.is_empty() && text != "?";
    if known(&tags.event) {
        tags.event.clone()
    } else if known(&tags.white) || known(&tags.black) {
        format!("{} - {}", tags.white, tags.black)
    } else {
        String::new()
    }
}

/// The line that follows `node` in the main line, as moves.
fn line_after(game: &Game, node: NodeId) -> Vec<Move> {
    let Ok(rest) = game.mainline_from(node) else {
        return Vec::new();
    };
    rest.take(MAX_SOLUTION_PLIES)
        .filter_map(|id| game.mv(id).ok())
        .collect()
}

/// A problem of `game` at `ply` (the position after that many plies).
pub(crate) fn make_problem(
    game: &Game,
    game_number: u32,
    ply: u32,
    position: Position,
    solution: Vec<Move>,
    difficulty: Option<u8>,
) -> Problem {
    Problem {
        id: problem_id(game_number, ply),
        game_number,
        ply: u16::try_from(ply).unwrap_or(u16::MAX),
        position,
        solution,
        difficulty,
        rating: rating_of(game),
        title: title_of(game),
    }
}

/// The problems of `game`, number `game_number` of its source.
///
/// 1. Every node of the main line whose comment has the `****D` marker is a
///    problem: its position, and the main line after it as the solution.
/// 2. Without markers, a game that starts from a position other than the
///    standard one is one problem: its start, and its main line.
/// 3. Anything else is not a problem.
pub fn extract_problems(game: &Game, game_number: u32) -> Vec<Problem> {
    let make = |ply: u32, position: Position, solution: Vec<Move>, difficulty: Option<u8>| {
        make_problem(game, game_number, ply, position, solution, difficulty)
    };
    let mut out = Vec::new();
    for (index, node) in game.mainline().enumerate() {
        if out.len() >= MAX_PROBLEMS_PER_GAME {
            break;
        }
        let Ok(Some(comment)) = game.comment(node) else {
            continue;
        };
        let Some(Marked { difficulty }) = marker_of(comment) else {
            continue;
        };
        let Ok(position) = game.position_at(node) else {
            continue;
        };
        let ply = u32::try_from(index + 1).unwrap_or(u32::MAX);
        out.push(make(
            ply,
            position.clone(),
            line_after(game, node),
            difficulty,
        ));
    }
    if out.is_empty() && *game.start_position() != Position::new() {
        let solution = line_after(game, game.root());
        out.push(make(0, game.start_position().clone(), solution, None));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game_with_markers() -> Game {
        let mut game = Game::new();
        let mut cursor = game.root();
        for uci in ["e2e4", "e7e5", "g1f3", "b8c6", "f1c4"] {
            let pos = game.position_at(cursor).expect("node").clone();
            let m = pos.parse_uci(uci).expect("uci");
            cursor = game.add_move(cursor, m).expect("legal");
        }
        game
    }

    #[test]
    fn a_marked_node_is_a_problem_with_the_next_moves_as_solution() {
        let mut game = game_with_markers();
        let node = game.mainline().nth(1).expect("second move");
        game.set_comment(node, "****D4 0.3->3.5 a note")
            .expect("comment");
        let problems = extract_problems(&game, 7);
        assert_eq!(problems.len(), 1);
        let p = &problems[0];
        assert_eq!(p.difficulty, Some(4));
        assert_eq!(p.ply, 2);
        assert_eq!(p.id, problem_id(7, 2));
        assert_eq!(p.solution.len(), 3);
        assert_eq!(p.position, *game.position_at(node).expect("node"));
    }

    #[test]
    fn a_bare_marker_has_no_difficulty() {
        let mut game = game_with_markers();
        let node = game.mainline().next().expect("first move");
        game.set_comment(node, "****D").expect("comment");
        let problems = extract_problems(&game, 1);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].difficulty, None);
    }

    #[test]
    fn a_standard_game_without_markers_is_not_a_problem() {
        assert!(extract_problems(&game_with_markers(), 1).is_empty());
    }

    #[test]
    fn a_fen_game_without_markers_is_one_problem_with_its_main_line() {
        let start = Position::from_fen("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1").expect("fen");
        let mut game = Game::from_position(start.clone());
        let m = start.parse_uci("a1a8").expect("uci");
        game.add_move(game.root(), m).expect("legal");
        game.tags_mut().event = "Back rank".to_owned();
        let problems = extract_problems(&game, 3);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].ply, 0);
        assert_eq!(problems[0].solution, vec![m]);
        assert_eq!(problems[0].title, "Back rank");
    }

    #[test]
    fn markers_win_over_the_non_standard_start() {
        let start = Position::from_fen("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1").expect("fen");
        let mut game = Game::from_position(start.clone());
        let m = start.parse_uci("a1a8").expect("uci");
        let node = game.add_move(game.root(), m).expect("legal");
        game.set_comment(node, "****D2").expect("comment");
        let problems = extract_problems(&game, 3);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].ply, 1);
    }

    #[test]
    fn the_solution_is_cut_at_the_limit() {
        let mut game =
            Game::from_position(Position::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 0 1").expect("fen"));
        let mut cursor = game.root();
        for i in 0..60 {
            let pos = game.position_at(cursor).expect("node").clone();
            let uci = match i % 4 {
                0 => "a1a2",
                1 => "e8e7",
                2 => "a2a1",
                _ => "e7e8",
            };
            let m = pos.parse_uci(uci).expect("uci");
            cursor = game.add_move(cursor, m).expect("legal");
        }
        let problems = extract_problems(&game, 1);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].solution.len(), MAX_SOLUTION_PLIES);
    }

    #[test]
    fn the_rating_comes_from_the_tags() {
        let start = Position::from_fen("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1").expect("fen");
        let mut game = Game::from_position(start);
        game.tags_mut().set_extra("Rating", "1750").expect("tag");
        assert_eq!(extract_problems(&game, 1)[0].rating, Some(1750));
        game.tags_mut()
            .set_extra("PuzzleRating", "abc")
            .expect("tag");
        assert_eq!(extract_problems(&game, 1)[0].rating, Some(1750));
    }

    #[test]
    fn ids_of_different_games_and_plies_differ() {
        assert_ne!(problem_id(1, 0), problem_id(0, 1));
        assert_ne!(problem_id(1, 2), problem_id(2, 1));
        assert_eq!(problem_id(1, 70_000), problem_id(1, 65_535));
    }
}
