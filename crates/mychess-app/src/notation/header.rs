//! The header lines above the moves (D174).
//!
//! The lines are data, not text: the front-end writes the labels in the
//! user's language.

use mychess_core::{CastlingMode, Eco, Game, GameResult};

use super::HeaderMode;

/// FEN of the standard start position.
const STANDARD_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// One line of the header.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HeaderLine {
    /// `[Name "value"]` (full header).
    Tag {
        /// The tag name.
        name: String,
        /// The value, not escaped.
        value: String,
    },
    /// `White (rating)  --  Black (rating)` (short header).
    Players {
        /// White's name.
        white: String,
        /// White's rating, if known.
        white_rating: Option<u16>,
        /// Black's name.
        black: String,
        /// Black's rating, if known.
        black_rating: Option<u16>,
    },
    /// Event, round, site, date, result and ECO (short header).
    Event {
        /// The event.
        event: String,
        /// The round, `None` for `?`.
        round: Option<String>,
        /// The site.
        site: String,
        /// The date without unknown trailing parts (`2024.05`).
        date: String,
        /// The result.
        result: GameResult,
        /// The ECO code, if any.
        eco: Option<Eco>,
    },
    /// The FEN of a start position that is not the standard one.
    Position {
        /// The FEN.
        fen: String,
    },
}

/// Whether the game starts from something other than the standard position.
fn has_custom_start(game: &Game) -> bool {
    let position = game.start_position();
    position.castling_mode() != CastlingMode::Standard || position.fen() != STANDARD_FEN
}

/// The header lines for `mode`.
pub(super) fn header_lines(game: &Game, mode: HeaderMode) -> Vec<HeaderLine> {
    let mut lines = Vec::new();
    let tags = game.tags();
    match mode {
        HeaderMode::Hidden => {}
        HeaderMode::Full => {
            for (name, value) in tags.pgn_pairs() {
                lines.push(HeaderLine::Tag { name, value });
            }
            if has_custom_start(game) {
                lines.push(HeaderLine::Tag {
                    name: "FEN".to_owned(),
                    value: game.start_position().fen(),
                });
            }
        }
        HeaderMode::Short => {
            let rating = |r: Option<mychess_core::Rating>| {
                r.map(|r| r.value.get()).filter(|&value| value > 0)
            };
            lines.push(HeaderLine::Players {
                white: tags.white.clone(),
                white_rating: rating(tags.white_rating),
                black: tags.black.clone(),
                black_rating: rating(tags.black_rating),
            });
            lines.push(HeaderLine::Event {
                event: tags.event.clone(),
                round: Some(tags.round.clone()).filter(|r| r != "?" && !r.trim().is_empty()),
                site: tags.site.clone(),
                date: tags.date.short(),
                result: tags.result,
                eco: tags.eco,
            });
            if has_custom_start(game) {
                lines.push(HeaderLine::Position {
                    fen: game.start_position().fen(),
                });
            }
        }
    }
    lines
}
