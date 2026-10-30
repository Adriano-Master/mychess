//! View-models of the player strips above and below the board (spec M2-5,
//! D231 to D236).
//!
//! The strips are indexed by **colour**; the front-end decides which one is
//! drawn on top from the board orientation. Only data lives here: no UI
//! string is built (the `?` of an unknown player, `--:--` for a missing
//! clock and `+3` are drawn by the front-end).
//!
//! Name, title and rating come from the tags of the game only (the M3 will
//! add a fallback source from the name base in the same fields). The clock
//! is the last `[%clk]` of a move of that side on the path from the root to
//! the cursor, so inside a variation it is inherited from the main line up
//! to the branching point and then comes from the variation itself.

use mychess_core::{Color, Game, NodeId, Position, RatingType, Role, Tags, clock_of};

use crate::material::{self, MaterialMode};

/// Longest player name kept, in characters (a hostile tag can be huge).
pub const MAX_STRIP_NAME_CHARS: usize = 256;

/// The longest title (`GM`, `IM`, `WFM`, `BOT`...).
const MAX_TITLE_LEN: usize = 4;

/// A rating and its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RatingView {
    /// The rating value (above zero).
    pub value: u16,
    /// The kind; the front-end shows a short label when it is not `Elo`.
    pub kind: RatingType,
}

/// What one strip shows.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlayerStripView {
    /// The side of the strip.
    pub color: Color,
    /// The player name; `None` when empty or `?` (unknown).
    pub name: Option<String>,
    /// The title from `WhiteTitle`/`BlackTitle`, only if valid (1 to 4
    /// upper-case ASCII letters or digits).
    pub title: Option<String>,
    /// The rating from the tags of the game.
    pub rating: Option<RatingView>,
    /// Pieces to draw, in the order `Q R B N P` (see
    /// [`Material`](crate::material::Material) for what they mean per mode).
    pub captured: Vec<(Role, u8)>,
    /// Points ahead; only on the side that is ahead.
    pub advantage: Option<u32>,
    /// Seconds on the clock at the cursor, if this side has a `[%clk]` on
    /// the path.
    pub clock: Option<u32>,
    /// This side is to move (false on both sides when the game is over at
    /// the cursor: checkmate or stalemate).
    pub to_move: bool,
}

impl PlayerStripView {
    /// An empty strip for `color`.
    pub fn new(color: Color) -> PlayerStripView {
        PlayerStripView {
            color,
            name: None,
            title: None,
            rating: None,
            captured: Vec::new(),
            advantage: None,
            clock: None,
            to_move: false,
        }
    }
}

/// Both strips and whether the clock slot is reserved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerStrips {
    /// White's strip.
    pub white: PlayerStripView,
    /// Black's strip.
    pub black: PlayerStripView,
    /// Some node of the game has a `[%clk]`: both strips reserve room for
    /// the clock (the side without a value yet shows `--:--`).
    pub clock_slot: bool,
}

impl Default for PlayerStrips {
    fn default() -> PlayerStrips {
        PlayerStrips {
            white: PlayerStripView::new(Color::White),
            black: PlayerStripView::new(Color::Black),
            clock_slot: false,
        }
    }
}

impl PlayerStrips {
    /// The strip of `color`.
    pub fn of(&self, color: Color) -> &PlayerStripView {
        match color {
            Color::White => &self.white,
            Color::Black => &self.black,
        }
    }

    fn of_mut(&mut self, color: Color) -> &mut PlayerStripView {
        match color {
            Color::White => &mut self.white,
            Color::Black => &mut self.black,
        }
    }

    /// The parts that depend only on the content of the game (tags and the
    /// clock slot); the cursor-dependent parts are left empty.
    pub(crate) fn identity(game: &Game) -> PlayerStrips {
        let tags = game.tags();
        let mut strips = PlayerStrips::default();
        for color in [Color::White, Color::Black] {
            let view = strips.of_mut(color);
            view.name = name_of(tags, color);
            view.title = title_of(tags, color);
            view.rating = rating_of(tags, color);
        }
        strips.clock_slot = game_has_clock(game);
        strips
    }

    /// Recomputes the parts that depend on the cursor (captured pieces,
    /// advantage, clocks, side to move), keeping the identity.
    pub(crate) fn refresh_cursor(&mut self, game: &Game, cursor: NodeId, mode: MaterialMode) {
        let shown = material::material(game, cursor, mode);
        let ahead = shown.advantage;
        self.white.captured = shown.white;
        self.black.captured = shown.black;
        for color in [Color::White, Color::Black] {
            self.of_mut(color).advantage = match ahead {
                Some((side, points)) if side == color => Some(points),
                _ => None,
            };
        }
        let (white_clock, black_clock) = clocks_at(game, cursor);
        self.white.clock = white_clock;
        self.black.clock = black_clock;
        let to_move = game
            .position_at(cursor)
            .ok()
            .and_then(|p| (!p.is_checkmate() && !p.is_stalemate()).then(|| p.turn()));
        self.white.to_move = to_move == Some(Color::White);
        self.black.to_move = to_move == Some(Color::Black);
    }
}

/// Builds both strips for the position at `cursor`.
///
/// An unknown `cursor` (a stale [`NodeId`]) gives strips with the identity
/// of the game and nothing else.
pub fn build_player_strips(game: &Game, cursor: NodeId, mode: MaterialMode) -> PlayerStrips {
    let mut strips = PlayerStrips::identity(game);
    strips.refresh_cursor(game, cursor, mode);
    strips
}

fn name_of(tags: &Tags, color: Color) -> Option<String> {
    let raw = match color {
        Color::White => &tags.white,
        Color::Black => &tags.black,
    };
    // Only the first characters matter; avoid scanning a huge tag.
    let name: String = raw.chars().take(MAX_STRIP_NAME_CHARS + 1).collect();
    let name = name.trim();
    if name.is_empty() || name == "?" {
        return None;
    }
    Some(name.chars().take(MAX_STRIP_NAME_CHARS).collect())
}

/// A title is 1 to 4 upper-case ASCII letters or digits (`GM`, `WFM`,
/// `BOT`); anything else is ignored.
fn title_of(tags: &Tags, color: Color) -> Option<String> {
    let key = match color {
        Color::White => "WhiteTitle",
        Color::Black => "BlackTitle",
    };
    let value = tags.get_extra(key)?;
    let valid = !value.is_empty()
        && value.len() <= MAX_TITLE_LEN
        && value
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    valid.then(|| value.to_owned())
}

fn rating_of(tags: &Tags, color: Color) -> Option<RatingView> {
    let rating = match color {
        Color::White => tags.white_rating,
        Color::Black => tags.black_rating,
    }?;
    let value = rating.value.get();
    (value > 0).then_some(RatingView {
        value,
        kind: rating.kind,
    })
}

/// `[%clk]` of a comment. The substring test skips the command parser for
/// the (common) comments without clocks, whatever their size.
fn clock_in(comment: &str) -> Option<u32> {
    if comment.contains("clk") {
        clock_of(comment)
    } else {
        None
    }
}

/// Whether any node of the game has a `[%clk]`.
fn game_has_clock(game: &Game) -> bool {
    game.descendants(game.root()).is_ok_and(|mut nodes| {
        nodes.any(|node| {
            game.comment(node)
                .ok()
                .flatten()
                .is_some_and(|c| clock_in(c).is_some())
        })
    })
}

/// The last clock of each side on the path from the root to `cursor`:
/// `(white, black)`. Walks from the cursor towards the root and stops as
/// soon as both sides were found.
fn clocks_at(game: &Game, cursor: NodeId) -> (Option<u32>, Option<u32>) {
    let (mut white, mut black) = (None, None);
    let mut node = cursor;
    while let Ok(Some(parent)) = game.parent(node) {
        let slot = match game.position_at(parent).map(Position::turn) {
            Ok(Color::White) => &mut white,
            Ok(Color::Black) => &mut black,
            Err(_) => break,
        };
        if slot.is_none() {
            *slot = game.comment(node).ok().flatten().and_then(clock_in);
        }
        if white.is_some() && black.is_some() {
            break;
        }
        node = parent;
    }
    (white, black)
}

#[cfg(test)]
mod tests {
    use mychess_core::{Elo, Position, Rating};

    use super::*;

    fn play(game: &mut Game, from: NodeId, ucis: &[&str]) -> Vec<NodeId> {
        let mut node = from;
        let mut out = Vec::new();
        for uci in ucis {
            let position = game.position_at(node).expect("node").clone();
            let mv = position.parse_uci(uci).expect("legal uci");
            node = game.add_move(node, mv).expect("add");
            out.push(node);
        }
        out
    }

    fn build(game: &Game, node: NodeId) -> PlayerStrips {
        build_player_strips(game, node, MaterialMode::Captured)
    }

    #[test]
    fn unknown_and_empty_names_are_none() {
        let mut game = Game::new();
        assert_eq!(build(&game, game.root()).white.name, None); // "?"
        game.tags_mut().white = "  ".to_owned();
        game.tags_mut().black = " Kasparov, G ".to_owned();
        let strips = build(&game, game.root());
        assert_eq!(strips.white.name, None);
        assert_eq!(strips.black.name.as_deref(), Some("Kasparov, G"));
    }

    #[test]
    fn huge_names_are_truncated() {
        let mut game = Game::new();
        game.tags_mut().white = "x".repeat(1 << 20);
        let name = build(&game, game.root()).white.name.expect("name");
        assert_eq!(name.chars().count(), MAX_STRIP_NAME_CHARS);
    }

    #[test]
    fn titles_are_validated() {
        let mut game = Game::new();
        for (value, expected) in [
            ("GM", Some("GM")),
            ("WFM", Some("WFM")),
            ("BOT", Some("BOT")),
            ("GM2", Some("GM2")),
            ("grandmaster", None),
            ("gm", None),
            ("", None),
            ("GMGMG", None),
            ("G M", None),
            ("G\u{c9}", None),
        ] {
            game.tags_mut()
                .set_extra("WhiteTitle", value)
                .expect("extra tag");
            assert_eq!(
                build(&game, game.root()).white.title.as_deref(),
                expected,
                "{value:?}"
            );
        }
        game.tags_mut().set_extra("BlackTitle", "IM").expect("tag");
        assert_eq!(build(&game, game.root()).black.title.as_deref(), Some("IM"));
    }

    #[test]
    fn rating_needs_a_positive_value_and_keeps_the_kind() {
        let mut game = Game::new();
        game.tags_mut().white_rating =
            Some(Rating::new(Elo::new(2750).expect("elo"), RatingType::Elo));
        game.tags_mut().black_rating =
            Some(Rating::new(Elo::new(1800).expect("elo"), RatingType::Uscf));
        let strips = build(&game, game.root());
        assert_eq!(
            strips.white.rating,
            Some(RatingView {
                value: 2750,
                kind: RatingType::Elo
            })
        );
        assert_eq!(strips.black.rating.map(|r| r.kind), Some(RatingType::Uscf));
        game.tags_mut().white_rating =
            Some(Rating::new(Elo::new(0).expect("elo"), RatingType::Elo));
        assert_eq!(build(&game, game.root()).white.rating, None);
    }

    #[test]
    fn side_to_move_follows_the_cursor() {
        let mut game = Game::new();
        let root = game.root();
        let nodes = play(&mut game, root, &["e2e4"]);
        let at_root = build(&game, root);
        assert!(at_root.white.to_move && !at_root.black.to_move);
        let after = build(&game, nodes[0]);
        assert!(!after.white.to_move && after.black.to_move);
    }

    #[test]
    fn nobody_is_to_move_after_checkmate_or_stalemate() {
        let mate = Game::from_position(
            Position::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3")
                .expect("fen"),
        );
        let s = build(&mate, mate.root());
        assert!(!s.white.to_move && !s.black.to_move);
        let stale =
            Game::from_position(Position::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").expect("fen"));
        let s = build(&stale, stale.root());
        assert!(!s.white.to_move && !s.black.to_move);
    }

    #[test]
    fn advantage_is_only_on_the_side_ahead() {
        let game =
            Game::from_position(Position::from_fen("4k3/8/8/8/8/8/8/Q3K3 w - - 0 1").expect("fen"));
        let s = build(&game, game.root());
        assert_eq!(s.white.advantage, Some(9));
        assert_eq!(s.black.advantage, None);
        let off = build_player_strips(&game, game.root(), MaterialMode::Off);
        assert_eq!(off.white.advantage, None);
    }

    #[test]
    fn captured_follow_the_path_per_colour() {
        let mut game = Game::new();
        let root = game.root();
        let nodes = play(&mut game, root, &["e2e4", "d7d5", "e4d5"]);
        let s = build(&game, nodes[2]);
        assert_eq!(s.white.captured, vec![(Role::Pawn, 1)]);
        assert!(s.black.captured.is_empty());
        assert_eq!(s.white.advantage, Some(1));
        let s = build(&game, root);
        assert!(s.white.captured.is_empty());
    }

    #[test]
    fn difference_mode_lists_the_surplus() {
        let game =
            Game::from_position(Position::from_fen("4k3/8/8/8/8/8/8/Q3K3 w - - 0 1").expect("fen"));
        let s = build_player_strips(&game, game.root(), MaterialMode::Difference);
        assert_eq!(s.white.captured, vec![(Role::Queen, 1)]);
    }

    fn with_clocks(comments: &[(usize, &str)], ucis: &[&str]) -> (Game, Vec<NodeId>) {
        let mut game = Game::new();
        let root = game.root();
        let nodes = play(&mut game, root, ucis);
        for (i, text) in comments {
            game.set_comment(nodes[*i], text).expect("comment");
        }
        (game, nodes)
    }

    #[test]
    fn clocks_come_from_the_last_move_of_each_side() {
        let (game, nodes) = with_clocks(
            &[
                (0, "[%clk 0:05:00]"),
                (1, "[%clk 0:04:58]"),
                (2, "[%clk 0:04:50]"),
            ],
            &["e2e4", "e7e5", "g1f3"],
        );
        let s = build(&game, nodes[2]);
        assert!(s.clock_slot);
        assert_eq!(s.white.clock, Some(290));
        assert_eq!(s.black.clock, Some(298));
        let s = build(&game, nodes[0]);
        assert_eq!(s.white.clock, Some(300));
        assert_eq!(s.black.clock, None);
        let s = build(&game, game.root());
        assert_eq!((s.white.clock, s.black.clock), (None, None));
        assert!(s.clock_slot, "the slot depends on the game, not the cursor");
    }

    #[test]
    fn no_clock_anywhere_means_no_slot() {
        let (game, nodes) = with_clocks(&[(0, "a plain comment")], &["e2e4"]);
        let s = build(&game, nodes[0]);
        assert!(!s.clock_slot);
        assert_eq!(s.white.clock, None);
    }

    #[test]
    fn clock_formats_are_read() {
        for (text, seconds) in [
            ("[%clk 1:02:03]", 3723),
            ("[%clk 2:03]", 123),
            ("[%clk 0:00:09.7]", 9),
            ("x [%clk 10:00] y", 600),
        ] {
            let (game, nodes) = with_clocks(&[(0, text)], &["e2e4"]);
            assert_eq!(build(&game, nodes[0]).white.clock, Some(seconds), "{text}");
        }
        let (game, nodes) = with_clocks(&[(0, "[%clk soon]")], &["e2e4"]);
        assert_eq!(build(&game, nodes[0]).white.clock, None);
    }

    #[test]
    fn variation_inherits_the_clock_of_the_main_line_up_to_the_branch() {
        let (mut game, nodes) = with_clocks(
            &[
                (0, "[%clk 0:05:00]"),
                (1, "[%clk 0:04:58]"),
                (2, "[%clk 0:04:50]"),
            ],
            &["e2e4", "e7e5", "g1f3"],
        );
        // Branch from e4: 1...c5 with its own clock.
        let var = play(&mut game, nodes[0], &["c7c5"]);
        let s = build(&game, var[0]);
        assert_eq!(s.white.clock, Some(300), "inherited");
        assert_eq!(s.black.clock, None);
        game.set_comment(var[0], "[%clk 0:04:30]").expect("comment");
        let s = build(&game, var[0]);
        assert_eq!(s.black.clock, Some(270), "own value");
    }

    #[test]
    fn hostile_comments_do_not_panic_or_stall() {
        let big = "[".repeat(1 << 20);
        let (game, nodes) = with_clocks(&[(0, big.as_str())], &["e2e4", "e7e5"]);
        let started = std::time::Instant::now();
        let s = build(&game, nodes[1]);
        assert!(!s.clock_slot);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        let long = format!("{}[%clk 0:00:01]", "x".repeat(1 << 20));
        let (game, nodes) = with_clocks(&[(0, long.as_str())], &["e2e4"]);
        assert_eq!(build(&game, nodes[0]).white.clock, Some(1));
    }

    #[test]
    fn stale_node_id_keeps_the_identity_only() {
        let mut game = Game::new();
        game.tags_mut().white = "A".to_owned();
        let mut other = Game::new();
        let root = other.root();
        let nodes = play(&mut other, root, &["e2e4"]);
        let s = build(&game, nodes[0]);
        assert_eq!(s.white.name.as_deref(), Some("A"));
        assert!(!s.white.to_move && !s.black.to_move);
        assert_eq!((s.white.clock, s.black.clock), (None, None));
        assert!(s.white.captured.is_empty());
    }

    #[test]
    fn color_indexing_is_by_colour() {
        let strips = PlayerStrips::default();
        assert_eq!(strips.of(Color::White).color, Color::White);
        assert_eq!(strips.of(Color::Black).color, Color::Black);
    }

    /// Manual measurement: `cargo test -p mychess-app --release -- --ignored
    /// --nocapture measure_cursor_walk`.
    #[test]
    #[ignore = "timing measurement, run by hand"]
    fn measure_cursor_walk() {
        let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
        let ucis: Vec<&str> = cycle.iter().copied().cycle().take(5000).collect();
        let mut game = Game::new();
        let root = game.root();
        let nodes = play(&mut game, root, &ucis);
        let end = nodes[nodes.len() - 1];
        let runs = 200;
        let mut strips = PlayerStrips::identity(&game);
        let started = std::time::Instant::now();
        for _ in 0..runs {
            strips.refresh_cursor(&game, end, MaterialMode::Captured);
        }
        println!("cursor part, 5000 plies: {:?}", started.elapsed() / runs);
        let started = std::time::Instant::now();
        for _ in 0..runs {
            std::hint::black_box(build_player_strips(&game, end, MaterialMode::Captured));
        }
        println!("full build, 5000 plies: {:?}", started.elapsed() / runs);
    }
}
