//! The game information panel as data (spec M2-2, D141; Scid:
//! `updateGameinfo` and `sc_game info`, `src/tkscid.cpp:7412`).

use serde::Serialize;

use mychess_core::{Eco, EcoBook, Game, GameResult, Nag, NodeId, Role, strip_mark_codes};

/// Longest comment shown in the panel, in characters (the word that crosses
/// the limit is completed).
pub const COMMENT_PREVIEW_CHARS: usize = 70;
/// Most variations listed.
pub const MAX_VARIATION_LINES: usize = 5;

/// What the panel shows and hides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct GameInfoOptions {
    /// Hide the next move, the result and the variations (study mode).
    pub hide_next_move: bool,
    /// Show the whole comment instead of a preview.
    pub full_comment: bool,
    /// Show the FEN of the position.
    pub show_fen: bool,
    /// Show the material balance.
    pub show_material: bool,
}

impl Default for GameInfoOptions {
    fn default() -> GameInfoOptions {
        GameInfoOptions {
            hide_next_move: false,
            full_comment: false,
            show_fen: false,
            show_material: true,
        }
    }
}

/// A player's line: name (`None` when unknown, so the front-end can show a
/// link to the dialog) and rating.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerLine {
    /// The name, `None` for `?` or blank.
    pub name: Option<String>,
    /// The rating, if any.
    pub rating: Option<u16>,
}

/// The name of the opening at the current position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningName {
    /// The ECO code of the book entry.
    pub eco: Eco,
    /// The opening's name.
    pub name: String,
}

/// The move that led to the position.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PreviousMove {
    /// The position is the start of the game.
    StartOfGame,
    /// Reserved: the start of a variation. In this game tree a variation
    /// starts with a real move, so this value is not produced.
    StartOfLine,
    /// A move.
    Move {
        /// Move number.
        number: u32,
        /// Whether Black played it (`12...`).
        black: bool,
        /// The move in SAN.
        san: String,
        /// The NAGs of the move.
        nags: Vec<Nag>,
    },
}

/// The move that follows the position.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NextMove {
    /// Hidden by [`GameInfoOptions::hide_next_move`].
    Hidden,
    /// The main line ends here.
    EndOfGame,
    /// A variation ends here.
    EndOfLine,
    /// A move.
    Move {
        /// Move number.
        number: u32,
        /// Whether Black plays it.
        black: bool,
        /// The move in SAN.
        san: String,
        /// The NAGs of the move.
        nags: Vec<Nag>,
    },
}

/// One alternative to the next move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariationLine {
    /// Move number.
    pub number: u32,
    /// Whether Black plays it.
    pub black: bool,
    /// The move in SAN.
    pub san: String,
    /// Its move-quality NAG (`!` to `?!`), if the first NAG is one.
    pub nag: Option<Nag>,
}

/// Everything the information panel shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInfoView {
    /// White.
    pub white: PlayerLine,
    /// Black.
    pub black: PlayerLine,
    /// The result; `None` when hidden or unknown.
    pub result: Option<GameResult>,
    /// `Event`, `None` when unknown.
    pub event: Option<String>,
    /// `Site`, `None` when unknown.
    pub site: Option<String>,
    /// The date without trailing unknown parts (`2024.05`).
    pub date: Option<String>,
    /// `Round`, `None` when unknown.
    pub round: Option<String>,
    /// The `ECO` tag (extended form).
    pub eco_tag: Option<Eco>,
    /// The opening of the current position, from the ECO book.
    pub opening: Option<OpeningName>,
    /// The move before the position.
    pub previous: PreviousMove,
    /// The move after the position.
    pub next: NextMove,
    /// Whether the position is inside a variation.
    pub in_variation: bool,
    /// Full moves of the main line.
    pub length_moves: u32,
    /// White's material minus Black's, in pawns; `None` when not shown.
    pub material: Option<i32>,
    /// The comment (marks removed, one line, shortened unless asked).
    pub comment: Option<String>,
    /// Up to five alternatives to the next move.
    pub variations: Vec<VariationLine>,
    /// The FEN, when asked for.
    pub fen: Option<String>,
}

fn known(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty() && text != "?").then(|| text.to_owned())
}

fn piece_points(role: Role) -> i32 {
    match role {
        Role::Queen => 9,
        Role::Rook => 5,
        Role::Bishop | Role::Knight => 3,
        Role::Pawn => 1,
        Role::King => 0,
    }
}

/// One line of text, cut after [`COMMENT_PREVIEW_CHARS`] at the end of the
/// word, with `...` when something was left out.
fn comment_preview(text: &str, full: bool) -> Option<String> {
    let stripped = strip_mark_codes(text);
    let oneline = stripped.replace(['\r', '\n'], " ");
    let oneline = oneline.trim();
    if oneline.is_empty() {
        return None;
    }
    if full || oneline.chars().count() <= COMMENT_PREVIEW_CHARS {
        return Some(oneline.to_owned());
    }
    let mut cut = oneline.len();
    for (count, (at, ch)) in oneline.char_indices().enumerate() {
        if count >= COMMENT_PREVIEW_CHARS && ch.is_whitespace() {
            cut = at;
            break;
        }
    }
    let shown = oneline[..cut].trim_end();
    if cut < oneline.len() {
        Some(format!("{shown}..."))
    } else {
        Some(shown.to_owned())
    }
}

/// A node's move as number, colour, SAN and NAGs.
fn move_parts(game: &Game, node: NodeId) -> Option<(u32, bool, String, Vec<Nag>)> {
    let parent = game.parent(node).ok().flatten()?;
    let before = game.position_at(parent).ok()?;
    let mv = game.mv(node).ok()?;
    let san = before.san(mv).unwrap_or_else(|_| "?".to_owned());
    let nags = game.nags(node).map(<[Nag]>::to_vec).unwrap_or_default();
    Some((
        before.fullmove_number(),
        before.turn() == mychess_core::Color::Black,
        san,
        nags,
    ))
}

pub(crate) fn in_variation(game: &Game, cursor: NodeId) -> bool {
    let mut node = cursor;
    loop {
        if game.is_variation(node).unwrap_or(false) {
            return true;
        }
        match game.parent(node) {
            Ok(Some(parent)) => node = parent,
            _ => return false,
        }
    }
}

/// White's material minus Black's, in pawns.
fn material_balance(position: &mychess_core::Position) -> i32 {
    position
        .pieces()
        .map(|(_, piece)| {
            let points = piece_points(piece.role);
            if piece.color == mychess_core::Color::White {
                points
            } else {
                -points
            }
        })
        .sum()
}

/// The alternatives to the main continuation, at most five.
fn variation_lines(game: &Game, children: &[NodeId]) -> Vec<VariationLine> {
    children
        .iter()
        .skip(1)
        .take(MAX_VARIATION_LINES)
        .filter_map(|&child| {
            let (number, black, san, nags) = move_parts(game, child)?;
            let nag = nags
                .first()
                .copied()
                .filter(|n| (1..=6).contains(&n.value()));
            Some(VariationLine {
                number,
                black,
                san,
                nag,
            })
        })
        .collect()
}

/// Builds the panel for the position at `cursor`.
///
/// A stale `cursor` is treated as the start of the game. `eco` is the
/// optional opening book, used for the opening name.
#[allow(clippy::trivially_copy_pass_by_ref)] // the API takes the options by reference
pub fn game_info(
    game: &Game,
    cursor: NodeId,
    opts: &GameInfoOptions,
    eco: Option<&EcoBook>,
) -> GameInfoView {
    let cursor = if game.contains(cursor) {
        cursor
    } else {
        game.root()
    };
    let tags = game.tags();
    let position = game.position_at(cursor).unwrap_or(game.start_position());
    let is_root = cursor == game.root();

    let previous = if is_root {
        PreviousMove::StartOfGame
    } else {
        move_parts(game, cursor).map_or(PreviousMove::StartOfGame, |(number, black, san, nags)| {
            PreviousMove::Move {
                number,
                black,
                san,
                nags,
            }
        })
    };
    let children = game.children(cursor).unwrap_or(&[]);
    let in_var = in_variation(game, cursor);
    let next = match children.first() {
        None if in_var => NextMove::EndOfLine,
        None => NextMove::EndOfGame,
        Some(_) if opts.hide_next_move => NextMove::Hidden,
        Some(&child) => {
            move_parts(game, child).map_or(NextMove::EndOfGame, |(number, black, san, nags)| {
                NextMove::Move {
                    number,
                    black,
                    san,
                    nags,
                }
            })
        }
    };
    let variations = if opts.hide_next_move {
        Vec::new()
    } else {
        variation_lines(game, children)
    };
    let material = opts.show_material.then(|| material_balance(position));
    let plies = game.mainline().count();
    let opening = if is_root {
        None
    } else {
        eco.and_then(|book| book.lookup(position))
            .map(|entry| OpeningName {
                eco: entry.eco(),
                name: entry.name().to_owned(),
            })
    };
    GameInfoView {
        white: PlayerLine {
            name: known(&tags.white),
            rating: tags.white_rating.map(|r| r.value.get()).filter(|v| *v > 0),
        },
        black: PlayerLine {
            name: known(&tags.black),
            rating: tags.black_rating.map(|r| r.value.get()).filter(|v| *v > 0),
        },
        result: (!opts.hide_next_move && tags.result != GameResult::Unknown).then_some(tags.result),
        event: known(&tags.event),
        site: known(&tags.site),
        date: (!tags.date.is_unknown()).then(|| tags.date.short()),
        round: known(&tags.round),
        eco_tag: tags.eco,
        opening,
        previous,
        next,
        in_variation: in_var,
        length_moves: u32::try_from(plies.div_ceil(2)).unwrap_or(u32::MAX),
        material,
        comment: game
            .comment(cursor)
            .ok()
            .flatten()
            .and_then(|c| comment_preview(c, opts.full_comment)),
        variations,
        fen: opts.show_fen.then(|| position.fen()),
    }
}

#[cfg(test)]
mod tests {
    use mychess_core::{Position, SanMode};

    use super::*;

    fn play(game: &mut Game, parent: NodeId, san: &str) -> NodeId {
        let mv = game
            .position_at(parent)
            .expect("position")
            .parse_san(san, SanMode::Strict)
            .expect("san")
            .mv;
        game.add_move(parent, mv).expect("move")
    }

    fn sample() -> (Game, NodeId, NodeId) {
        let mut game = Game::new();
        let root = game.root();
        let e4 = play(&mut game, root, "e4");
        let e5 = play(&mut game, e4, "e5");
        play(&mut game, e5, "Nf3");
        (game, e4, e5)
    }

    #[test]
    fn previous_and_next_moves() {
        let (game, e4, e5) = sample();
        let opts = GameInfoOptions::default();
        let at_start = game_info(&game, game.root(), &opts, None);
        assert_eq!(at_start.previous, PreviousMove::StartOfGame);
        assert!(matches!(
            at_start.next,
            NextMove::Move {
                number: 1,
                black: false,
                ..
            }
        ));
        let after_e4 = game_info(&game, e4, &opts, None);
        assert!(
            matches!(after_e4.previous, PreviousMove::Move { number: 1, black: false, ref san, .. } if san == "e4")
        );
        assert!(matches!(
            after_e4.next,
            NextMove::Move {
                number: 1,
                black: true,
                ..
            }
        ));
        let after_e5 = game_info(&game, e5, &opts, None);
        assert!(matches!(
            after_e5.previous,
            PreviousMove::Move {
                number: 1,
                black: true,
                ..
            }
        ));
        let end = game.mainline().last().expect("leaf");
        assert_eq!(game_info(&game, end, &opts, None).next, NextMove::EndOfGame);
        assert_eq!(at_start.length_moves, 2);
    }

    #[test]
    fn hiding_the_next_move() {
        let (mut game, e4, _) = sample();
        play(&mut game, e4, "c5");
        game.tags_mut().result = GameResult::White;
        let opts = GameInfoOptions {
            hide_next_move: true,
            ..GameInfoOptions::default()
        };
        let view = game_info(&game, e4, &opts, None);
        assert_eq!(view.next, NextMove::Hidden);
        assert_eq!(view.result, None);
        assert!(view.variations.is_empty());
        let shown = game_info(&game, e4, &GameInfoOptions::default(), None);
        assert_eq!(shown.result, Some(GameResult::White));
        assert_eq!(shown.variations.len(), 1);
    }

    #[test]
    fn variations_are_flagged_and_limited() {
        let (mut game, e4, _) = sample();
        let c5 = play(&mut game, e4, "c5");
        game.add_nag(c5, Nag::new(6).expect("nag"))
            .expect("add nag");
        for san in ["e6", "c6", "d6", "Nc6", "Nf6", "g6"] {
            play(&mut game, e4, san);
        }
        let view = game_info(&game, e4, &GameInfoOptions::default(), None);
        assert_eq!(view.variations.len(), MAX_VARIATION_LINES);
        assert_eq!(view.variations[0].san, "c5");
        assert_eq!(view.variations[0].nag.map(Nag::value), Some(6));
        let in_line = game_info(&game, c5, &GameInfoOptions::default(), None);
        assert!(in_line.in_variation);
        assert_eq!(in_line.next, NextMove::EndOfLine);
    }

    #[test]
    fn header_fields_are_options() {
        let mut game = Game::new();
        let tags = game.tags_mut();
        tags.event = "E".to_owned();
        tags.date = "2024.05.??".parse().expect("date");
        let view = game_info(&game, game.root(), &GameInfoOptions::default(), None);
        assert_eq!(view.white.name, None);
        assert_eq!(view.event.as_deref(), Some("E"));
        assert_eq!(view.site, None);
        assert_eq!(view.round, None);
        assert_eq!(view.date.as_deref(), Some("2024.05"));
    }

    #[test]
    fn material_and_fen() {
        let game = Game::from_position(
            Position::from_fen("4k3/8/8/8/8/8/4P3/3QK3 w - - 0 1").expect("fen"),
        );
        let opts = GameInfoOptions {
            show_fen: true,
            ..GameInfoOptions::default()
        };
        let view = game_info(&game, game.root(), &opts, None);
        assert_eq!(view.material, Some(10));
        assert_eq!(
            view.fen.as_deref(),
            Some("4k3/8/8/8/8/8/4P3/3QK3 w - - 0 1")
        );
        let off = GameInfoOptions {
            show_material: false,
            ..GameInfoOptions::default()
        };
        assert_eq!(game_info(&game, game.root(), &off, None).material, None);
    }

    #[test]
    fn comments_are_cleaned_and_shortened() {
        assert_eq!(
            comment_preview("a [%clk 0:01:00] b\nc", false).as_deref(),
            Some("a b c")
        );
        let words = "word ".repeat(30);
        let short = comment_preview(&words, false).expect("some");
        assert!(short.ends_with("word..."), "{short}");
        assert!(short.chars().count() >= COMMENT_PREVIEW_CHARS);
        assert_eq!(
            comment_preview(&words, true).map(|c| c.len()),
            Some(words.trim().len())
        );
        assert_eq!(comment_preview("   ", false), None);
        assert_eq!(comment_preview("[%clk 1:00]", false), None);
        assert_eq!(
            comment_preview(&"x".repeat(200), false).map(|c| c.len()),
            Some(200)
        );
    }
}
