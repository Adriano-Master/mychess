//! A game of other players, watched through the public stream (spec M8-3,
//! phase d, D3000-D3004).
//!
//! The stream gives the description of the game and then, for each move,
//! the new position (`fen`) and the move (`lm`), delayed by three moves for
//! a game in progress. The list of moves comes from the exported PGN
//! (`/game/export/{id}`), read with `mychess-pgn`. Every move of the stream is
//! checked against the game before it is added: a move that does not follow
//! from the last position, and a position that differs from the one the move
//! leads to, make the game restart from the position of the stream and mark
//! the history as incomplete, never panic.

use std::sync::Arc;

use mychess_core::{Color, FenMode, Game, GameResult, PartialDate, Position};
use mychess_online::models::{Color as OColor, Status, WatchGame, WatchMove, WatchPlayer};
use mychess_pgn::read::{ReadOptions, parse_pgn_text};

use super::game_builder::{
    MAX_LABEL_CHARS, MAX_NAME_CHARS, SITE_PREFIX, StartError, clean_text, last_node, rating,
    speed_name, utc_parts,
};

/// The exported PGN of a watched game, already read into a game (D3117).
///
/// The reading of a PGN of up to 4 MiB is CPU work: it is done in `perform`
/// (on a blocking thread), so that `update` only applies the result. Two
/// values are equal when they are the same reading.
#[derive(Debug, Clone)]
pub struct ParsedExport(Arc<Game>);

impl PartialEq for ParsedExport {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl ParsedExport {
    /// Reads the first game of `text`; `None` when there is none.
    #[must_use]
    pub fn parse(text: &str) -> Option<ParsedExport> {
        let parsed = parse_pgn_text(text, &ReadOptions::default())
            .into_iter()
            .next()?;
        Some(ParsedExport(Arc::new(parsed.game)))
    }
}

/// How far the watch got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WatchPhase {
    /// The stream is being opened.
    Opening,
    /// Moves arrive.
    Live,
    /// The game is over.
    Finished,
}

/// How a move of the stream met the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Followed {
    /// The game already had that position.
    Same,
    /// The move was added.
    Added,
    /// The position did not follow: the game restarts from it.
    Restarted,
}

/// The game being watched.
#[derive(Debug, Clone)]
pub struct WatchedGame {
    /// The id of the game.
    pub id: String,
    /// White: name, title and rating.
    pub white: WatchPlayerView,
    /// Black.
    pub black: WatchPlayerView,
    /// Rated game.
    pub rated: bool,
    /// The speed, lower case (`blitz`...), or empty.
    pub speed: String,
    /// The last status the server gave.
    pub status: Option<Status>,
    /// The winner, if any.
    pub winner: Option<Color>,
    /// The history begins at the start of the game (the PGN was read, or
    /// the stream began at the start).
    pub complete: bool,
    /// The game for the editor.
    pub game: Game,
    current: Position,
    chess960: bool,
}

/// A player of the watched game.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchPlayerView {
    /// The name (`?` when unknown).
    pub name: String,
    /// The title.
    pub title: Option<String>,
    /// The rating.
    pub rating: Option<u32>,
}

impl WatchPlayerView {
    fn from(player: &WatchPlayer) -> WatchPlayerView {
        let name = clean_text(&player.name, MAX_NAME_CHARS);
        WatchPlayerView {
            name: if name.is_empty() {
                "?".to_owned()
            } else {
                name
            },
            title: player
                .title
                .as_deref()
                .map(|t| clean_text(t, MAX_LABEL_CHARS))
                .filter(|t| !t.is_empty()),
            rating: player.rating.filter(|r| *r > 0),
        }
    }
}

fn core_color(color: OColor) -> Color {
    match color {
        OColor::White => Color::White,
        OColor::Black => Color::Black,
    }
}

impl WatchedGame {
    /// The watched game described by the first line of the stream.
    ///
    /// # Errors
    ///
    /// [`StartError::UnsupportedVariant`] for a variant that is not played
    /// and [`StartError::BadPosition`] for a start that cannot be read.
    pub fn from_description(desc: &WatchGame) -> Result<WatchedGame, StartError> {
        let chess960 = desc.variant.key == "chess960";
        if !desc.variant.is_standard() && !chess960 {
            return Err(StartError::UnsupportedVariant);
        }
        let mode = if chess960 {
            FenMode::Chess960
        } else {
            FenMode::Auto
        };
        let start = match desc.initial_fen.as_deref() {
            Some(fen) => Position::from_fen_with(fen, mode).map_err(|_| StartError::BadPosition)?,
            None if chess960 => return Err(StartError::BadPosition),
            None => Position::new(),
        };
        let mut watched = WatchedGame {
            id: clean_text(&desc.id, MAX_NAME_CHARS),
            white: WatchPlayerView::from(&desc.white),
            black: WatchPlayerView::from(&desc.black),
            rated: desc.rated,
            speed: desc
                .speed
                .map(|s| speed_name(s).to_lowercase())
                .unwrap_or_default(),
            status: desc.status,
            winner: desc.winner.map(core_color),
            complete: true,
            game: if desc.initial_fen.is_some() || chess960 {
                Game::from_position(start.clone())
            } else {
                Game::new()
            },
            current: start,
            chess960,
        };
        watched.write_tags(desc.created_at);
        Ok(watched)
    }

    fn result(&self) -> GameResult {
        let win = |c: Color| match c {
            Color::White => GameResult::White,
            Color::Black => GameResult::Black,
        };
        match self.status {
            Some(Status::Draw | Status::Stalemate | Status::InsufficientMaterialClaim) => {
                GameResult::Draw
            }
            Some(
                Status::Mate
                | Status::Resign
                | Status::Timeout
                | Status::Outoftime
                | Status::Cheat
                | Status::VariantEnd,
            ) => self.winner.map_or(GameResult::Unknown, win),
            _ => GameResult::Unknown,
        }
    }

    fn write_tags(&mut self, created_at: Option<u64>) {
        let result = self.result();
        let tags = self.game.tags_mut();
        tags.event = if self.speed.is_empty() {
            "Lichess game".to_owned()
        } else {
            format!(
                "{} {} game",
                if self.rated { "Rated" } else { "Casual" },
                self.speed
            )
        };
        tags.site = format!("{SITE_PREFIX}{}", self.id);
        "-".clone_into(&mut tags.round);
        tags.white.clone_from(&self.white.name);
        tags.black.clone_from(&self.black.name);
        tags.result = result;
        tags.white_rating = rating(self.white.rating);
        tags.black_rating = rating(self.black.rating);
        if let Some(ms) = created_at.filter(|t| *t > 0) {
            let (y, m, d, ..) = utc_parts(ms / 1000);
            tags.date = PartialDate::new(u16::try_from(y).ok(), Some(m), Some(d))
                .unwrap_or(PartialDate::UNKNOWN);
        }
        if let Some(title) = &self.white.title {
            tags.set_extra("WhiteTitle", title).ok();
        }
        if let Some(title) = &self.black.title {
            tags.set_extra("BlackTitle", title).ok();
        }
        if self.chess960 {
            tags.set_extra("Variant", "Chess960").ok();
        }
    }

    /// The position after the last move.
    pub fn position(&self) -> &Position {
        &self.current
    }

    /// Number of half-moves in the main line.
    pub fn plies(&self) -> usize {
        self.game.mainline().count()
    }

    /// Whether the status says the game is over.
    pub fn is_over(&self) -> bool {
        self.status.is_some_and(Status::is_final)
    }

    /// Takes the status and winner of a description (the last line of the
    /// stream) and updates the result tag.
    pub(crate) fn finish_with(&mut self, desc: &WatchGame) {
        self.status = desc.status.or(self.status);
        self.winner = desc.winner.map(core_color).or(self.winner);
        let result = self.result();
        self.game.tags_mut().result = result;
    }

    /// Replaces the game by the one of the exported PGN, when it is the same
    /// game; the tags of Lichess (opening, ratings, clocks as comments) come
    /// with it. Returns whether the PGN was used.
    pub(crate) fn apply_export(&mut self, export: ParsedExport) -> bool {
        let mut game = Arc::try_unwrap(export.0).unwrap_or_else(|shared| (*shared).clone());
        if game.start_position().zobrist() != self.start_zobrist() {
            return false;
        }
        let last = last_node(&game);
        let Ok(position) = game.position_at(last).cloned() else {
            return false;
        };
        // Keep what the stream knows that the PGN may not say.
        let result = self.result();
        if game.tags().result == GameResult::Unknown {
            game.tags_mut().result = result;
        }
        self.game = game;
        self.current = position;
        self.complete = true;
        true
    }

    fn start_zobrist(&self) -> mychess_core::ZobristHash {
        self.game.start_position().zobrist()
    }

    /// Follows a move of the stream: adds it, or restarts from its position
    /// when it does not follow from the game (D3003).
    pub(crate) fn follow(&mut self, step: &WatchMove) -> Followed {
        let mode = if self.chess960 {
            FenMode::Chess960
        } else {
            FenMode::Auto
        };
        let Ok(target) = Position::from_fen_with(&step.fen, mode) else {
            // A position this client cannot read says nothing: ignore it.
            return Followed::Same;
        };
        if target.zobrist() == self.current.zobrist() {
            return Followed::Same;
        }
        if let Some(uci) = step.last_move.as_deref()
            && let Ok(mv) = self.current.parse_uci(uci)
            && let Ok(next) = self.current.after(mv)
            && next.zobrist() == target.zobrist()
        {
            let parent = last_node(&self.game);
            if self.game.add_move(parent, mv).is_ok() {
                self.current = next;
                return Followed::Added;
            }
        }
        self.restart_from(target);
        Followed::Restarted
    }

    fn restart_from(&mut self, target: Position) {
        let tags = self.game.tags().clone();
        let mut game = Game::from_position(target.clone());
        *game.tags_mut() = tags;
        game.tags_mut().set_extra("SetUp", "1").ok();
        self.game = game;
        self.current = target;
        self.complete = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_online::models::WatchEvent;

    fn desc(json: &str) -> WatchGame {
        match WatchEvent::parse_line(json) {
            WatchEvent::Description(d) => *d,
            other => panic!("not a description: {other:?}"),
        }
    }

    const START: &str = r#"{"id":"abcd1234","rated":true,"variant":"standard","speed":"blitz","status":"started","players":{"white":{"user":{"name":"Alice","title":"GM"},"rating":2500},"black":{"user":{"name":"Bob"},"rating":1500}}}"#;

    fn step(fen: &str, lm: Option<&str>) -> WatchMove {
        WatchMove {
            fen: fen.to_owned(),
            last_move: lm.map(str::to_owned),
            white_clock: 0,
            black_clock: 0,
        }
    }

    const AFTER_E4: &str = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1";
    const AFTER_E5: &str = "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq e6 0 2";

    #[test]
    fn description_builds_tags_and_players() {
        let w = WatchedGame::from_description(&desc(START)).expect("game");
        assert_eq!(w.white.name, "Alice");
        assert_eq!(w.white.title.as_deref(), Some("GM"));
        assert_eq!(w.black.rating, Some(1500));
        let tags = w.game.tags();
        assert_eq!(tags.event, "Rated blitz game");
        assert_eq!(tags.site, "https://lichess.org/abcd1234");
        assert_eq!(tags.white, "Alice");
        assert!(w.complete && !w.is_over());
        assert_eq!(w.plies(), 0);
    }

    #[test]
    fn unsupported_variant_is_refused() {
        let d = desc(r#"{"id":"abcd1234","variant":"crazyhouse"}"#);
        assert_eq!(
            WatchedGame::from_description(&d).err(),
            Some(StartError::UnsupportedVariant)
        );
    }

    #[test]
    fn moves_follow_one_after_the_other_and_repeats_are_ignored() {
        let mut w = WatchedGame::from_description(&desc(START)).expect("game");
        assert_eq!(w.follow(&step(AFTER_E4, Some("e2e4"))), Followed::Added);
        assert_eq!(w.follow(&step(AFTER_E4, Some("e2e4"))), Followed::Same);
        assert_eq!(w.follow(&step(AFTER_E5, Some("e7e5"))), Followed::Added);
        assert_eq!(w.plies(), 2);
        assert!(w.complete);
    }

    #[test]
    fn a_position_that_does_not_follow_restarts_the_game_incomplete() {
        let mut w = WatchedGame::from_description(&desc(START)).expect("game");
        assert_eq!(w.follow(&step(AFTER_E4, Some("e2e4"))), Followed::Added);
        // The stream jumps: the move given is not the one that leads there.
        let jump = "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2";
        assert_eq!(w.follow(&step(jump, Some("g1f3"))), Followed::Restarted);
        assert!(!w.complete);
        assert_eq!(w.plies(), 0);
        assert_eq!(w.position().turn(), Color::Black);
        // Garbage moves and positions never panic.
        assert_eq!(w.follow(&step("not a fen", Some("e2e4"))), Followed::Same);
        let _ = w.follow(&step(AFTER_E4, Some("zzzz")));
        let _ = w.follow(&step(AFTER_E5, None));
    }

    #[test]
    fn exported_pgn_replaces_the_history() {
        let mut w = WatchedGame::from_description(&desc(START)).expect("game");
        let pgn = "[Event \"Rated Blitz game\"]\n[Site \"https://lichess.org/abcd1234\"]\n[White \"Alice\"]\n[Black \"Bob\"]\n[Result \"*\"]\n\n1. e4 { [%clk 0:05:00] } e5 2. Nf3 *\n";
        assert!(w.apply_export(ParsedExport::parse(pgn).expect("pgn")));
        assert_eq!(w.plies(), 3);
        // The stream then repeats the last position: nothing to add.
        let after_nf3 = "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2";
        assert_eq!(w.follow(&step(after_nf3, Some("g1f3"))), Followed::Same);
        assert_eq!(w.plies(), 3);
        // Text that is not a PGN, or another start, is not used.
        assert!(ParsedExport::parse("nothing here").is_none());
        let other =
            "[Event \"x\"]\n[SetUp \"1\"]\n[FEN \"8/8/8/8/8/8/8/K6k w - - 0 1\"]\n\n1. Ka2 *\n";
        assert!(!w.apply_export(ParsedExport::parse(other).expect("pgn")));
        assert_eq!(w.plies(), 3);
    }

    #[test]
    fn final_description_sets_the_result() {
        let mut w = WatchedGame::from_description(&desc(START)).expect("game");
        w.finish_with(&desc(
            r#"{"id":"abcd1234","status":"resign","winner":"black"}"#,
        ));
        assert!(w.is_over());
        assert_eq!(w.game.tags().result, GameResult::Black);
        w.finish_with(&desc(r#"{"id":"abcd1234","status":"draw"}"#));
        assert_eq!(w.game.tags().result, GameResult::Draw);
    }
}
