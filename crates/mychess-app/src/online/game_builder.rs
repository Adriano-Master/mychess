//! The online game as the server describes it, and its conversion to a
//! [`Game`] with tags and `[%clk]` comments (spec M8-3, D1318, D1335).
//!
//! The list of moves of the server is **replayed from the start position**
//! every time it arrives, so applying the same state twice, or a state that
//! is shorter (a takeback), always gives what the server says. The moves are
//! decoded by `Position::parse_uci`, which takes castling as "king takes
//! rook" and as the king's two-square step (D1318, D1346).

use mychess_core::{
    Color, Elo, FenMode, Game, GameResult, Move, NodeId, PartialDate, Position, Rating, RatingType,
};
use mychess_online::models::{Color as OColor, GameFull, GameState, Player, Status};

use crate::play::PlayEnd;

/// The `Site` tag prefix; the id of the game follows.
pub const SITE_PREFIX: &str = "https://lichess.org/";

/// A player as the `gameFull` describes him.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerInfo {
    /// The lower-case id, absent for the AI.
    pub id: Option<String>,
    /// The name shown (`AI level N` for the AI).
    pub name: String,
    /// The title (`GM`...).
    pub title: Option<String>,
    /// The rating.
    pub rating: Option<u32>,
    /// The rating is provisional.
    pub provisional: bool,
}

/// Longest name, in characters, taken from the server (D1363).
pub(crate) const MAX_NAME_CHARS: usize = 64;
/// Longest title, time control or similar short label (D1363).
pub(crate) const MAX_LABEL_CHARS: usize = 32;

/// Text from the server made safe to keep and show: control characters
/// (line breaks, escapes) are dropped and at most `max` characters stay
/// (D1363). Applied before the text reaches any state.
pub(crate) fn clean_text(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control()).take(max).collect()
}

impl PlayerInfo {
    fn from_player(player: &Player) -> PlayerInfo {
        let name = match (&player.name, player.ai_level) {
            (Some(name), _) if !name.is_empty() => clean_text(name, MAX_NAME_CHARS),
            (_, Some(level)) => format!("AI level {level}"),
            _ => "?".to_owned(),
        };
        let name = if name.is_empty() {
            "?".to_owned()
        } else {
            name
        };
        PlayerInfo {
            id: player
                .id
                .as_ref()
                .map(|id| clean_text(&id.to_lowercase(), MAX_NAME_CHARS)),
            name,
            title: player
                .title
                .as_deref()
                .map(|t| clean_text(t, MAX_LABEL_CHARS))
                .filter(|t| !t.is_empty()),
            rating: player.rating.filter(|r| *r > 0),
            provisional: player.provisional,
        }
    }
}

/// Why a game could not be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StartError {
    /// A variant the core does not play (D1313).
    UnsupportedVariant,
    /// Neither player is the user.
    NotMine,
    /// The initial position cannot be read.
    BadPosition,
}

/// A list of moves that does not fit the position (D1318).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncError {
    /// The ply (1-based) of the move that failed.
    pub ply: usize,
}

/// What a state changed in the list of moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Applied {
    /// Plies before.
    pub before: usize,
    /// Plies after.
    pub after: usize,
    /// Some earlier ply was removed or replaced (a takeback or a move of
    /// the user the server did not take).
    pub rewritten: bool,
}

/// How a finished game ended, for the strip and the status (D1327).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EndKind {
    /// Checkmate.
    Mate,
    /// Resignation.
    Resign,
    /// Stalemate.
    Stalemate,
    /// The flag fell (or the opponent left).
    Timeout,
    /// A draw (agreement, repetition, 50 moves...).
    Draw,
    /// Insufficient material claimed.
    InsufficientMaterial,
    /// A rule infraction.
    Cheat,
    /// A variant ending.
    VariantEnd,
    /// Aborted or never started: nothing is stored.
    Aborted,
    /// The server does not say.
    Unknown,
}

/// The end of a game. The server decides; the local rules are not applied
/// (D1327).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// The result.
    pub result: GameResult,
    /// How it ended.
    pub kind: EndKind,
    /// The icon of the end, reusing the rules of the game against an
    /// engine for the picture only.
    pub icon: Option<PlayEnd>,
}

fn core_color(c: OColor) -> Color {
    match c {
        OColor::White => Color::White,
        OColor::Black => Color::Black,
    }
}

/// The online game: description, moves and what the server last said.
#[derive(Debug, Clone, PartialEq)]
pub struct OnlineGame {
    /// The id (also the id of the page).
    pub id: String,
    /// The colour of the user.
    pub my_color: Color,
    /// White.
    pub white: PlayerInfo,
    /// Black.
    pub black: PlayerInfo,
    /// Rated game.
    pub rated: bool,
    /// The speed (`rapid`...), lower case.
    pub speed: String,
    /// The variant key.
    pub variant: String,
    /// The FEN of the start, `None` for the standard position.
    pub initial_fen: Option<String>,
    /// Initial time and increment in milliseconds.
    pub clock: Option<(u64, u64)>,
    /// When the server created it (ms since the epoch).
    pub created_at: Option<u64>,
    pub(crate) start: Position,
    pub(crate) moves: Vec<Move>,
    pub(crate) current: Position,
    /// Moves the server confirmed; more than this are the user's own moves
    /// waiting for it.
    pub(crate) confirmed: usize,
    /// Milliseconds on the clock of the mover after each ply, when known.
    pub(crate) clk: Vec<Option<u64>>,
    /// The last status.
    pub status: Status,
    /// The winner the server named.
    pub winner: Option<Color>,
    /// Remaining time of White (ms), as last reported.
    pub wtime: u64,
    /// Remaining time of Black (ms).
    pub btime: u64,
    /// White offers a draw.
    pub wdraw: bool,
    /// Black offers a draw.
    pub bdraw: bool,
    /// White proposes a takeback.
    pub wtakeback: bool,
    /// Black proposes a takeback.
    pub btakeback: bool,
}

/// Plays `tokens` from `start`.
fn replay(start: &Position, tokens: &[&str]) -> Result<(Vec<Move>, Position), SyncError> {
    let mut position = start.clone();
    let mut moves = Vec::with_capacity(tokens.len());
    for (i, token) in tokens.iter().enumerate() {
        let error = || SyncError { ply: i + 1 };
        let mv = position.parse_uci(token).map_err(|_| error())?;
        position.play(mv).map_err(|_| error())?;
        moves.push(mv);
    }
    Ok((moves, position))
}

impl OnlineGame {
    /// Opens the game from a `gameFull`; `me` is the lower-case id of the
    /// account.
    ///
    /// # Errors
    ///
    /// [`StartError`] for a variant that is not played, a game the user is
    /// not in, or a start position that cannot be read.
    pub fn from_full(full: &GameFull, me: &str) -> Result<OnlineGame, StartError> {
        let chess960 = full.variant.key == "chess960";
        if !full.variant.is_standard() && !chess960 {
            return Err(StartError::UnsupportedVariant);
        }
        let white = PlayerInfo::from_player(&full.white);
        let black = PlayerInfo::from_player(&full.black);
        let me = me.to_lowercase();
        let my_color = if white.id.as_deref() == Some(me.as_str()) {
            Color::White
        } else if black.id.as_deref() == Some(me.as_str()) {
            Color::Black
        } else {
            return Err(StartError::NotMine);
        };
        let initial_fen = Some(full.initial_fen.trim())
            .filter(|f| !f.is_empty() && *f != "startpos")
            .map(str::to_owned);
        let start = match &initial_fen {
            None if chess960 => Position::from_fen_with(
                "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
                FenMode::Chess960,
            )
            .map_err(|_| StartError::BadPosition)?,
            None => Position::new(),
            Some(fen) => {
                let mode = if chess960 {
                    FenMode::Chess960
                } else {
                    FenMode::Auto
                };
                Position::from_fen_with(fen, mode).map_err(|_| StartError::BadPosition)?
            }
        };
        let speed = full.speed.map_or("unknown", speed_name).to_owned();
        let mut game = OnlineGame {
            id: full.id.clone(),
            my_color,
            white,
            black,
            rated: full.rated,
            speed,
            variant: full.variant.key.clone(),
            initial_fen,
            clock: full.clock.map(|c| (c.initial, c.increment)),
            created_at: full.created_at.filter(|t| *t > 0),
            current: start.clone(),
            start,
            moves: Vec::new(),
            confirmed: 0,
            clk: Vec::new(),
            status: Status::Created,
            winner: None,
            wtime: 0,
            btime: 0,
            wdraw: false,
            bdraw: false,
            wtakeback: false,
            btakeback: false,
        };
        game.apply_state(&full.state)
            .map_err(|_| StartError::BadPosition)?;
        Ok(game)
    }

    /// Replays the moves of `state` from the start and copies its status,
    /// clocks and offers (D1318).
    ///
    /// # Errors
    ///
    /// [`SyncError`] when a move does not fit; nothing changes then.
    pub fn apply_state(&mut self, state: &GameState) -> Result<Applied, SyncError> {
        let tokens = state.move_list();
        let (moves, current) = replay(&self.start, &tokens)?;
        let before = self.moves.len();
        let common = self
            .moves
            .iter()
            .zip(&moves)
            .take_while(|(a, b)| a == b)
            .count();
        let rewritten = common < before.min(moves.len()) || moves.len() < before;
        // Local moves the server did not (yet) take are dropped with the
        // rewrite; the clocks of the common prefix are kept.
        self.clk.truncate(common);
        self.clk.resize(moves.len(), None);
        self.moves = moves;
        self.current = current;
        self.confirmed = self.moves.len();
        self.status = state.status.unwrap_or(Status::Started);
        self.winner = state.winner.map(core_color);
        self.wtime = state.wtime;
        self.btime = state.btime;
        self.wdraw = state.wdraw;
        self.bdraw = state.bdraw;
        self.wtakeback = state.wtakeback;
        self.btakeback = state.btakeback;
        self.record_clocks();
        Ok(Applied {
            before,
            after: self.moves.len(),
            rewritten,
        })
    }

    /// Keeps the times of the last state for `[%clk]`: the mover of the last
    /// ply has just moved, and the other side's time is what it was after
    /// its own move.
    fn record_clocks(&mut self) {
        let n = self.moves.len();
        if n == 0 {
            return;
        }
        let (mover, other) = if n % 2 == 1 {
            (self.wtime, self.btime)
        } else {
            (self.btime, self.wtime)
        };
        self.clk[n - 1] = Some(mover);
        if n >= 2 && self.clk[n - 2].is_none() {
            self.clk[n - 2] = Some(other);
        }
    }

    /// Plies in the list (the user's own pending moves included).
    pub fn plies(&self) -> usize {
        self.moves.len()
    }

    /// The position after the last move.
    pub fn position(&self) -> &Position {
        &self.current
    }

    /// The side to move.
    pub fn turn(&self) -> Color {
        self.current.turn()
    }

    /// The side to move in the position the server confirmed, ignoring the
    /// move of the user that waits for it (D1364).
    pub fn confirmed_turn(&self) -> Color {
        if self.confirmed.is_multiple_of(2) {
            self.start.turn()
        } else {
            !self.start.turn()
        }
    }

    /// Plies the server confirmed.
    pub fn confirmed_plies(&self) -> usize {
        self.confirmed
    }

    /// Whether it is the turn of the user.
    pub fn my_turn(&self) -> bool {
        self.turn() == self.my_color
    }

    /// Whether a move of the user waits for the server.
    pub fn has_pending_move(&self) -> bool {
        self.moves.len() > self.confirmed
    }

    /// Whether the server said the game is over.
    pub fn is_over(&self) -> bool {
        self.status.is_final()
    }

    /// The last move as UCI (for the log of a refusal), if any.
    pub(crate) fn last_uci(&self) -> Option<String> {
        self.moves.last().map(|m| self.current_before_last_uci(*m))
    }

    fn current_before_last_uci(&self, mv: Move) -> String {
        mv.to_uci(self.start.castling_mode())
    }

    /// Plays the user's move on the board before the server confirms it.
    ///
    /// # Errors
    ///
    /// `Err(())` when the move is illegal here.
    pub(crate) fn push_local(&mut self, mv: Move) -> Result<(), ()> {
        let next = self.current.after(mv).map_err(|_| ())?;
        self.moves.push(mv);
        self.clk.push(None);
        self.current = next;
        Ok(())
    }

    /// Takes back the moves the server did not confirm.
    pub(crate) fn undo_local(&mut self) {
        if !self.has_pending_move() {
            return;
        }
        self.moves.truncate(self.confirmed);
        self.clk.truncate(self.confirmed);
        let mut position = self.start.clone();
        for mv in &self.moves {
            // The moves were legal when they were stored.
            if position.play(*mv).is_err() {
                break;
            }
        }
        self.current = position;
    }

    /// The end of the game, once the server said it is over (D1327).
    pub fn outcome(&self) -> Option<Outcome> {
        if !self.status.is_final() {
            return None;
        }
        let win = |c: Color| match c {
            Color::White => GameResult::White,
            Color::Black => GameResult::Black,
        };
        let winner = self.winner;
        let result_of_winner = winner.map_or(GameResult::Unknown, win);
        let (kind, result, icon) = match self.status {
            Status::Mate => (
                EndKind::Mate,
                result_of_winner,
                winner.map(|winner| PlayEnd::Checkmate { winner }),
            ),
            Status::Resign => (
                EndKind::Resign,
                result_of_winner,
                winner.map(|w| PlayEnd::Resignation { loser: !w }),
            ),
            Status::Timeout | Status::Outoftime => (
                EndKind::Timeout,
                result_of_winner,
                winner.map(|w| PlayEnd::Timeout { loser: !w }),
            ),
            Status::Cheat => (EndKind::Cheat, result_of_winner, None),
            Status::VariantEnd => (EndKind::VariantEnd, result_of_winner, None),
            Status::Stalemate => (
                EndKind::Stalemate,
                GameResult::Draw,
                Some(PlayEnd::Stalemate),
            ),
            Status::Draw => (EndKind::Draw, GameResult::Draw, None),
            Status::InsufficientMaterialClaim => (
                EndKind::InsufficientMaterial,
                GameResult::Draw,
                Some(PlayEnd::InsufficientMaterial),
            ),
            Status::Aborted | Status::NoStart => (EndKind::Aborted, GameResult::Unknown, None),
            _ => (EndKind::Unknown, GameResult::Unknown, None),
        };
        Some(Outcome { result, kind, icon })
    }

    /// The `Termination` tag (D1335).
    fn termination(&self) -> &'static str {
        match self.status {
            Status::Timeout | Status::Outoftime => "Time forfeit",
            Status::Aborted | Status::NoStart => "Abandoned",
            Status::Cheat => "Rules infraction",
            Status::Created | Status::Started | Status::UnknownFinish | Status::Unknown => {
                "Unterminated"
            }
            _ => "Normal",
        }
    }

    /// The time control as `600+5` (seconds).
    pub fn time_control(&self) -> Option<String> {
        self.clock
            .map(|(initial, inc)| format!("{}+{}", initial / 1000, inc / 1000))
    }

    /// Tags and moves as a [`Game`] (D1335).
    pub fn to_game(&self) -> Game {
        let mut game = if self.initial_fen.is_some() || self.variant == "chess960" {
            Game::from_position(self.start.clone())
        } else {
            Game::new()
        };
        self.write_tags(&mut game);
        let mut parent = game.root();
        for (i, mv) in self.moves.iter().enumerate() {
            let Ok(node) = game.add_move(parent, *mv) else {
                break;
            };
            if let Some(ms) = self.clk.get(i).copied().flatten() {
                // A comment the game refuses is dropped: the move stays.
                game.set_comment(node, &clk_comment(ms)).ok();
            }
            parent = node;
        }
        game
    }

    fn write_tags(&self, game: &mut Game) {
        let (date, time) = self.created_at.map_or((None, None), |ms| {
            let (y, m, d, hh, mm, ss) = utc_parts(ms / 1000);
            (Some((y, m, d)), Some(format!("{hh:02}:{mm:02}:{ss:02}")))
        });
        let tags = game.tags_mut();
        tags.event = format!(
            "{} {} game",
            if self.rated { "Rated" } else { "Casual" },
            self.speed
        );
        tags.site = format!("{SITE_PREFIX}{}", self.id);
        "-".clone_into(&mut tags.round);
        if let Some((y, m, d)) = date {
            tags.date = PartialDate::new(u16::try_from(y).ok(), Some(m), Some(d))
                .unwrap_or(PartialDate::UNKNOWN);
        }
        tags.white.clone_from(&self.white.name);
        tags.black.clone_from(&self.black.name);
        tags.result = self.outcome().map_or(GameResult::Unknown, |o| o.result);
        tags.white_rating = rating(self.white.rating);
        tags.black_rating = rating(self.black.rating);
        // Values the tags refuse (a line break) are dropped: the game stays.
        if let Some((y, m, d)) = date {
            tags.set_extra("UTCDate", &format!("{y:04}.{m:02}.{d:02}"))
                .ok();
        }
        if let Some(time) = time {
            tags.set_extra("UTCTime", &time).ok();
        }
        if let Some(title) = &self.white.title {
            tags.set_extra("WhiteTitle", title).ok();
        }
        if let Some(title) = &self.black.title {
            tags.set_extra("BlackTitle", title).ok();
        }
        if self.variant != "standard" {
            let name = if self.variant == "chess960" {
                "Chess960"
            } else {
                &self.variant
            };
            tags.set_extra("Variant", name).ok();
        }
        if let Some(control) = self.time_control() {
            tags.set_extra("TimeControl", &control).ok();
        }
        tags.set_extra("Termination", self.termination()).ok();
    }

    /// The game is already stored when a game with this `Site` was.
    pub fn site(&self) -> String {
        format!("{SITE_PREFIX}{}", self.id)
    }
}

pub(crate) fn rating(value: Option<u32>) -> Option<Rating> {
    let value = u16::try_from(value?).ok()?;
    Elo::new(value)
        .ok()
        .filter(|e| e.get() > 0)
        .map(|e| Rating::new(e, RatingType::Elo))
}

pub(crate) fn speed_name(speed: mychess_online::models::Speed) -> &'static str {
    use mychess_online::models::Speed;
    match speed {
        Speed::UltraBullet => "ultraBullet",
        Speed::Bullet => "bullet",
        Speed::Blitz => "blitz",
        Speed::Rapid => "rapid",
        Speed::Classical => "classical",
        Speed::Correspondence => "correspondence",
        _ => "unknown",
    }
}

/// `[%clk h:mm:ss]` for `ms` milliseconds (D1335).
pub fn clk_comment(ms: u64) -> String {
    let s = ms / 1000;
    format!("[%clk {}:{:02}:{:02}]", s / 3600, s / 60 % 60, s % 60)
}

/// Calendar parts `(year, month, day, hour, minute, second)` of `secs`
/// seconds after the Unix epoch, UTC (days-to-civil algorithm).
pub(crate) fn utc_parts(secs: u64) -> (i64, u8, u8, u8, u8, u8) {
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let small = |v: i64| u8::try_from(v).unwrap_or(0);
    (
        year,
        small(month),
        small(day),
        u8::try_from(rem / 3600).unwrap_or(0),
        u8::try_from(rem / 60 % 60).unwrap_or(0),
        u8::try_from(rem % 60).unwrap_or(0),
    )
}

/// The node of the last move of the main line of `game`.
pub(crate) fn last_node(game: &Game) -> NodeId {
    game.mainline().last().unwrap_or_else(|| game.root())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_online::models::GameEvent;

    fn full(json: &str) -> GameFull {
        match GameEvent::parse_line(json) {
            GameEvent::Full(f) => *f,
            other => panic!("not a gameFull: {other:?}"),
        }
    }

    fn full_json(moves: &str, extra: &str) -> String {
        format!(
            r#"{{"type":"gameFull","id":"abcd1234","variant":{{"key":"standard","name":"Standard"}},
"clock":{{"initial":600000,"increment":5000}},"speed":"rapid","perf":{{"name":"Rapid"}},"rated":false,
"createdAt":1700000000000,"white":{{"id":"ana","name":"Ana","title":"FM","rating":1800}},
"black":{{"id":"bob","name":"Bob","rating":1700,"provisional":true}},"initialFen":"startpos",
"state":{{"type":"gameState","moves":"{moves}","wtime":590000,"btime":585000,"winc":5000,"binc":5000,"status":"started"{extra}}}}}"#
        )
    }

    fn state(moves: &str, wtime: u64, btime: u64, status: &str) -> GameState {
        let line = format!(
            r#"{{"type":"gameState","moves":"{moves}","wtime":{wtime},"btime":{btime},"winc":0,"binc":0,"status":"{status}"}}"#
        );
        match GameEvent::parse_line(&line) {
            GameEvent::State(s) => s,
            other => panic!("not a state: {other:?}"),
        }
    }

    #[test]
    fn opens_the_game_and_finds_my_colour() {
        let g = OnlineGame::from_full(&full(&full_json("e2e4 e7e5", "")), "Bob").expect("open");
        assert_eq!(g.my_color, Color::Black);
        assert_eq!(g.plies(), 2);
        assert!(!g.my_turn() || g.turn() == Color::White);
        assert_eq!(g.white.title.as_deref(), Some("FM"));
        assert!(g.black.provisional);
        assert_eq!(g.time_control().as_deref(), Some("600+5"));
        let other = OnlineGame::from_full(&full(&full_json("", "")), "carl");
        assert_eq!(other.unwrap_err(), StartError::NotMine);
    }

    #[test]
    fn an_unknown_variant_does_not_open() {
        let json = full_json("", "").replace(r#""key":"standard""#, r#""key":"atomic""#);
        let r = OnlineGame::from_full(&full(&json), "ana");
        assert_eq!(r.unwrap_err(), StartError::UnsupportedVariant);
    }

    #[test]
    fn replaying_is_idempotent_and_follows_takebacks() {
        let mut g = OnlineGame::from_full(&full(&full_json("e2e4", "")), "ana").expect("open");
        let s = state("e2e4 e7e5 g1f3", 1000, 2000, "started");
        let a = g.apply_state(&s).expect("apply");
        assert_eq!((a.before, a.after, a.rewritten), (1, 3, false));
        let again = g.apply_state(&s).expect("apply");
        assert_eq!((again.before, again.after, again.rewritten), (3, 3, false));
        let back = g
            .apply_state(&state("e2e4 e7e5", 1, 2, "started"))
            .expect("apply");
        assert_eq!((back.after, back.rewritten), (2, true));
        assert_eq!(g.plies(), 2);
    }

    #[test]
    fn castling_is_taken_in_both_forms() {
        let base = "e2e4 e7e5 g1f3 b8c6 f1c4 g8f6";
        for castle in ["e1g1", "e1h1"] {
            let mut g = OnlineGame::from_full(&full(&full_json("", "")), "ana").expect("open");
            let moves = format!("{base} {castle}");
            g.apply_state(&state(&moves, 1, 1, "started"))
                .expect("castle");
            assert_eq!(g.plies(), 7);
            assert!(
                g.position()
                    .fen()
                    .starts_with("r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQ1RK1 b")
            );
        }
    }

    #[test]
    fn an_illegal_move_changes_nothing() {
        let mut g = OnlineGame::from_full(&full(&full_json("e2e4", "")), "ana").expect("open");
        let err = g
            .apply_state(&state("e2e4 e7e5 e1e3", 1, 1, "started"))
            .unwrap_err();
        assert_eq!(err, SyncError { ply: 3 });
        assert_eq!(g.plies(), 1);
    }

    #[test]
    fn local_moves_are_taken_back_or_replaced_by_the_server() {
        let mut g = OnlineGame::from_full(&full(&full_json("", "")), "ana").expect("open");
        let e4 = g.position().parse_uci("e2e4").expect("uci");
        g.push_local(e4).expect("push");
        assert!(g.has_pending_move());
        g.undo_local();
        assert_eq!((g.plies(), g.has_pending_move()), (0, false));
        g.push_local(e4).expect("push");
        // The server confirms the same move: nothing is rewritten.
        let a = g
            .apply_state(&state("e2e4", 5, 5, "started"))
            .expect("apply");
        assert!(!a.rewritten && !g.has_pending_move());
    }

    #[test]
    fn the_game_has_the_tags_and_a_clock_per_move() {
        let mut g = OnlineGame::from_full(&full(&full_json("", "")), "ana").expect("open");
        g.apply_state(&state("e2e4", 598_000, 600_000, "started"))
            .expect("1");
        g.apply_state(&state("e2e4 e7e5", 598_000, 597_500, "started"))
            .expect("2");
        g.apply_state(&state("e2e4 e7e5 g1f3", 3_725_000, 597_500, "mate"))
            .ok();
        let game = g.to_game();
        let tags = game.tags();
        assert_eq!(tags.event, "Casual rapid game");
        assert_eq!(tags.site, "https://lichess.org/abcd1234");
        assert_eq!((tags.white.as_str(), tags.black.as_str()), ("Ana", "Bob"));
        assert_eq!(tags.get_extra("TimeControl"), Some("600+5"));
        assert_eq!(tags.get_extra("UTCDate"), Some("2023.11.14"));
        assert_eq!(tags.get_extra("UTCTime"), Some("22:13:20"));
        assert_eq!(tags.get_extra("WhiteTitle"), Some("FM"));
        assert_eq!(tags.get_extra("Termination"), Some("Normal"));
        let nodes: Vec<_> = game.mainline().collect();
        assert_eq!(nodes.len(), 3);
        assert_eq!(game.comment(nodes[0]).expect("c"), Some("[%clk 0:09:58]"));
        assert_eq!(game.comment(nodes[1]).expect("c"), Some("[%clk 0:09:57]"));
        assert_eq!(game.comment(nodes[2]).expect("c"), Some("[%clk 1:02:05]"));
    }

    #[test]
    fn the_outcome_comes_from_the_status_only() {
        let mut g = OnlineGame::from_full(&full(&full_json("", "")), "ana").expect("open");
        assert!(g.outcome().is_none());
        let line = r#"{"type":"gameState","moves":"","wtime":0,"btime":9,"status":"outoftime","winner":"black"}"#;
        let GameEvent::State(s) = GameEvent::parse_line(line) else {
            panic!("state")
        };
        g.apply_state(&s).expect("apply");
        let o = g.outcome().expect("over");
        assert_eq!(o.result, GameResult::Black);
        assert_eq!(o.kind, EndKind::Timeout);
        assert_eq!(
            o.icon,
            Some(PlayEnd::Timeout {
                loser: Color::White
            })
        );
        assert_eq!(g.termination(), "Time forfeit");
        g.status = Status::Aborted;
        assert_eq!(g.outcome().map(|o| o.kind), Some(EndKind::Aborted));
        assert_eq!(g.termination(), "Abandoned");
    }

    #[test]
    fn the_calendar_conversion_is_right() {
        assert_eq!(utc_parts(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(utc_parts(951_782_400), (2000, 2, 29, 0, 0, 0));
        assert_eq!(utc_parts(1_700_000_000), (2023, 11, 14, 22, 13, 20));
    }

    #[test]
    fn a_chess960_start_position_is_read() {
        let json = full_json("", "")
            .replace(r#""key":"standard""#, r#""key":"chess960""#)
            .replace(
                "startpos",
                "bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w HFhf - 0 1",
            );
        let g = OnlineGame::from_full(&full(&json), "ana").expect("open");
        assert_eq!(g.position().chess960_number(), Some(0));
        assert_eq!(g.to_game().tags().get_extra("Variant"), Some("Chess960"));
    }

    proptest::proptest! {
        #[test]
        fn any_chunking_of_the_same_list_gives_the_same_game(cut in 0usize..8) {
            let all = "e2e4 e7e5 g1f3 b8c6 f1b5 a7a6 b5a4 g8f6";
            let tokens: Vec<&str> = all.split(' ').collect();
            let mut g = OnlineGame::from_full(&full(&full_json("", "")), "ana").unwrap();
            let part = tokens[..cut].join(" ");
            g.apply_state(&state(&part, 1, 1, "started")).unwrap();
            g.apply_state(&state(all, 1, 1, "started")).unwrap();
            g.apply_state(&state(all, 1, 1, "started")).unwrap();
            let mut fresh = OnlineGame::from_full(&full(&full_json(all, "")), "ana").unwrap();
            fresh.apply_state(&state(all, 1, 1, "started")).unwrap();
            proptest::prop_assert_eq!(&g.moves, &fresh.moves);
            proptest::prop_assert_eq!(g.position().fen(), fresh.position().fen());
        }
    }
}
