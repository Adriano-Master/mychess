//! The game that "import this line" makes from the moves of a book entry.

use mychess_core::{Game, SanMode};

/// A game from the standard position with the moves of `text` (`1.e4 c5
/// 2.Nf3`, as written in an `.eco` file). Move numbers and the result are
/// skipped. `None` if a word is not a legal move (the entries of a loaded
/// book were played when it was read, so this only guards against a
/// different reading).
pub(crate) fn game_from_line(text: &str) -> Option<Game> {
    let mut game = Game::new();
    let mut node = game.root();
    for word in text.split_whitespace() {
        // "12.Nf3" and "12...Nf6" carry their number; a lone "12." is a
        // number only.
        let san = word.trim_start_matches(|c: char| c.is_ascii_digit());
        let san = san.trim_start_matches('.');
        if san.is_empty() || matches!(san, "*" | "1-0" | "0-1" | "1/2-1/2") {
            continue;
        }
        let position = game.position_at(node).ok()?.clone();
        let parsed = position.parse_san(san, SanMode::Tolerant).ok()?;
        node = game.add_move(node, parsed.mv).ok()?;
    }
    Some(game)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_the_numbered_moves() {
        let game = game_from_line("1.e4 c5 2.Nf3 d6").expect("a line");
        assert_eq!(game.mainline().count(), 4);
    }

    #[test]
    fn plays_black_numbers_and_skips_the_result() {
        let game = game_from_line("1.e4 1...e5 *").expect("a line");
        assert_eq!(game.mainline().count(), 2);
    }

    #[test]
    fn an_illegal_move_gives_nothing() {
        assert!(game_from_line("1.e4 e4").is_none());
    }

    #[test]
    fn an_empty_line_is_the_empty_game() {
        assert_eq!(game_from_line("").expect("empty").mainline().count(), 0);
    }
}
