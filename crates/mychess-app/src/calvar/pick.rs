//! The position of a game of the list that becomes the problem (spec M8-6d,
//! D2368). Pure.

use mychess_core::{Game, Position};

/// The value of the filter that means "the last move" (`FILTER_LAST_MOVE`
/// of the database crate).
const LAST_MOVE: u8 = 255;
/// Plies played when the game has no ply of its own in the filter: the end
/// of the opening, where calculating starts to pay (D2368).
pub const DEFAULT_PLY: usize = 20;

/// The position of `game` to calculate. `raw_filter` is the value of the game
/// in the filter (0 = out, otherwise ply + 1, 255 = the last move): the ply a
/// position search found is used; a game without one gives [`DEFAULT_PLY`]
/// (or the end of a shorter game). The ply is taken back while the position
/// has no legal move, so the result can always be calculated.
pub fn game_position(game: &Game, raw_filter: u8) -> Position {
    let nodes: Vec<_> = game.mainline().collect();
    let wanted = match raw_filter {
        LAST_MOVE => nodes.len(),
        raw => match usize::from(raw.saturating_sub(1)) {
            0 => DEFAULT_PLY,
            ply => ply,
        },
    };
    let mut ply = wanted.min(nodes.len());
    loop {
        let position = match ply.checked_sub(1).and_then(|i| nodes.get(i)) {
            Some(node) => game.position_at(*node).ok(),
            None => Some(game.start_position()),
        };
        match position {
            Some(p) if !p.legal_moves().is_empty() => return p.clone(),
            _ if ply == 0 => return game.start_position().clone(),
            _ => ply -= 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(moves: &[&str]) -> Game {
        let mut game = Game::new();
        let mut node = game.root();
        let mut pos = Position::new();
        for uci in moves {
            let mv = pos.parse_uci(uci).expect("uci");
            node = game.add_move(node, mv).expect("add");
            pos = pos.after(mv).expect("after");
        }
        game
    }

    const SHORT: [&str; 6] = ["e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6"];

    #[test]
    fn a_filter_ply_picks_that_position() {
        let g = game(&SHORT);
        let p = game_position(&g, 4);
        assert_eq!(p.fullmove_number(), 2, "after three plies: Black to move");
        assert_eq!(p.turn(), mychess_core::Color::Black);
    }

    #[test]
    fn without_a_ply_a_short_game_gives_its_end() {
        let g = game(&SHORT);
        let end = game_position(&g, 1);
        assert_eq!(end.fen(), game_position(&g, 255).fen());
        assert_eq!(end.turn(), mychess_core::Color::White);
    }

    #[test]
    fn without_a_ply_a_long_game_stops_at_the_default() {
        let mut moves = Vec::new();
        for _ in 0..15 {
            moves.extend(["g1f3", "g8f6", "f3g1", "f6g8"]);
        }
        let g = game(&moves);
        let p = game_position(&g, 1);
        assert_eq!(p.fullmove_number(), 11, "twenty plies played");
    }

    #[test]
    fn a_game_that_ends_in_mate_backs_off_one_ply() {
        let g = game(&["f2f3", "e7e5", "g2g4", "d8h4"]);
        let p = game_position(&g, 255);
        assert!(!p.legal_moves().is_empty());
        assert_eq!(p.turn(), mychess_core::Color::Black);
    }

    #[test]
    fn an_empty_game_gives_the_start_position() {
        let g = Game::new();
        assert_eq!(game_position(&g, 0).fen(), Position::new().fen());
    }
}
