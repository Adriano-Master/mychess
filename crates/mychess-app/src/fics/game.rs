//! A game seen through the `<12>` lines of FICS (spec m8-9, D2613).
//!
//! The server sends the whole board with every move. [`FicsGame`] keeps the
//! moves it could follow (the SAN or the verbose move of the line, checked by
//! the core), so the editor gets a real game; when the line does not follow
//! from the position it holds (a takeback, an observation that began in the
//! middle) it takes the new board as the start and marks the history
//! `incomplete`.

use std::time::{Duration, Instant};

use mychess_core::{
    Color, Elo, Game, GameResult, Move, PartialDate, Position, Rating, RatingType, SanMode,
};
use mychess_online::fics::{FicsResult, GameSummary, Relation, Style12, VerboseMove};

/// Most plies kept (a bound against a hostile or endless game).
pub const MAX_PLIES: usize = 2000;

/// What the user does in the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Role {
    /// He plays.
    Playing,
    /// He watches a game being played.
    Observing,
    /// He examines (or watches an examined game).
    Examining,
}

/// What a `<12>` line did to the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Applied {
    /// Same position: only the clocks changed (or the server confirmed the
    /// move of the user).
    Same,
    /// A move was added.
    Moved,
    /// Moves were taken back.
    TookBack,
    /// The line did not follow: the board is the new start.
    Resynced,
    /// The board of the line is not a valid position.
    Invalid,
}

/// A game of FICS.
#[derive(Debug, Clone)]
pub struct FicsGame {
    /// The number the server gave.
    pub number: u32,
    /// White.
    pub white: String,
    /// Black.
    pub black: String,
    /// White's rating, when known.
    pub white_rating: Option<u32>,
    /// Black's rating, when known.
    pub black_rating: Option<u32>,
    /// What the user does here.
    pub role: Role,
    /// The side of the user when he plays.
    pub my_color: Option<Color>,
    /// Rated game.
    pub rated: bool,
    /// `blitz`, `lightning`, `standard`, `untimed`...
    pub kind: String,
    /// Minutes of each side.
    pub minutes: u32,
    /// Increment in seconds.
    pub increment: u32,
    /// The board is shown with Black at the bottom.
    pub flipped: bool,
    /// The result and the reason of the server, once it said it.
    pub result: Option<(FicsResult, String)>,
    /// The history before the first known board is missing.
    pub incomplete: bool,
    start: Position,
    start_is_standard: bool,
    moves: Vec<Move>,
    /// Milliseconds the mover spent on each ply.
    emt: Vec<Option<u32>>,
    /// Milliseconds left to the mover after each ply.
    clk: Vec<Option<u64>>,
    current: Position,
    /// A move of the user that the server did not confirm yet.
    pending: bool,
    white_ms: u64,
    black_ms: u64,
    synced_at: Instant,
    date: Option<(i64, u8, u8)>,
}

/// The board, the side to move and the castling rights: what must agree
/// between the server and the core (the en passant square differs in how it
/// is reported and the counters do not matter).
fn key(position: &Position) -> String {
    position
        .fen()
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
}

fn seconds_to_ms(secs: i64) -> u64 {
    u64::try_from(secs.max(0)).unwrap_or(0).saturating_mul(1000)
}

fn role_of(relation: Relation) -> Option<Role> {
    match relation {
        Relation::PlayingMyMove | Relation::PlayingOpponentToMove => Some(Role::Playing),
        Relation::Observing => Some(Role::Observing),
        Relation::Examining | Relation::ObservingExamined => Some(Role::Examining),
        _ => None,
    }
}

impl FicsGame {
    /// The game a `<12>` line starts (or joins in the middle). `None` when
    /// the board is not a valid position or the relation is an isolated
    /// position.
    pub fn from_style12(line: &Style12, meta: Option<&GameSummary>, at: Instant) -> Option<Self> {
        let role = role_of(line.relation)?;
        let start = Position::from_fen(&line.to_fen()).ok()?;
        // A standard game seen from its first move starts from the
        // initial position; anything else is a custom start.
        let initial = Position::new();
        let start_is_standard = key(&start) == key(&initial) && line.verbose.is_none();
        let mut game = FicsGame {
            number: line.game,
            white: line.white.clone(),
            black: line.black.clone(),
            white_rating: meta.and_then(|m| m.white_rating),
            black_rating: meta.and_then(|m| m.black_rating),
            role,
            my_color: None,
            rated: meta.is_some_and(|m| m.rated),
            kind: meta.map_or_else(String::new, |m| m.kind.clone()),
            minutes: line.initial_minutes,
            increment: line.increment,
            flipped: line.flipped,
            result: None,
            incomplete: !start_is_standard && line.verbose.is_some(),
            start: start.clone(),
            start_is_standard,
            moves: Vec::new(),
            emt: Vec::new(),
            clk: Vec::new(),
            current: start,
            pending: false,
            white_ms: 0,
            black_ms: 0,
            synced_at: at,
            date: today(),
        };
        game.sync(line, at);
        Some(game)
    }

    fn sync(&mut self, line: &Style12, at: Instant) {
        self.white_ms = seconds_to_ms(line.white_time);
        self.black_ms = seconds_to_ms(line.black_time);
        self.synced_at = at;
        if let Some(role) = role_of(line.relation) {
            self.role = role;
        }
        self.my_color = match line.relation {
            Relation::PlayingMyMove => Some(line.turn),
            Relation::PlayingOpponentToMove => Some(opposite(line.turn)),
            _ => self.my_color.filter(|_| self.role == Role::Playing),
        };
        self.flipped = line.flipped;
        self.minutes = line.initial_minutes;
        self.increment = line.increment;
        self.white.clone_from(&line.white);
        self.black.clone_from(&line.black);
    }

    /// Takes a `<12>` line of this game.
    pub fn apply(&mut self, line: &Style12, at: Instant) -> Applied {
        let Ok(target) = Position::from_fen(&line.to_fen()) else {
            return Applied::Invalid;
        };
        self.sync(line, at);
        // A move of the user the server does not have: take it out and look
        // at the line as if it had not been made.
        if self.pending && key(&self.current) != key(&target) {
            self.undo_local();
        }
        self.pending = false;
        if key(&self.current) == key(&target) {
            return Applied::Same;
        }
        if let Some(mv) = self.resolve(line)
            && let Ok(next) = self.current.after(mv)
            && key(&next) == key(&target)
            && self.moves.len() < MAX_PLIES
        {
            self.current = next;
            self.moves.push(mv);
            self.emt.push(line.move_time_ms);
            let mover_ms = match opposite(line.turn) {
                Color::White => self.white_ms,
                Color::Black => self.black_ms,
            };
            self.clk.push(Some(mover_ms));
            return Applied::Moved;
        }
        if let Some(ply) = self.ply_of(&target) {
            self.moves.truncate(ply);
            self.emt.truncate(ply);
            self.clk.truncate(ply);
            self.current = target;
            return Applied::TookBack;
        }
        self.start = target.clone();
        self.start_is_standard = false;
        self.current = target;
        self.moves.clear();
        self.emt.clear();
        self.clk.clear();
        self.incomplete = true;
        Applied::Resynced
    }

    /// The move the line reports, as a move of the current position.
    fn resolve(&self, line: &Style12) -> Option<Move> {
        let from_verbose = match line.verbose {
            Some(VerboseMove::CastleShort) => self
                .current
                .parse_san("O-O", SanMode::Strict)
                .ok()
                .map(|p| p.mv),
            Some(VerboseMove::CastleLong) => self
                .current
                .parse_san("O-O-O", SanMode::Strict)
                .ok()
                .map(|p| p.mv),
            Some(step @ VerboseMove::Step { .. }) => {
                step.uci().and_then(|uci| self.current.parse_uci(&uci).ok())
            }
            None => None,
        };
        from_verbose.or_else(|| {
            let san = line.san.as_deref()?;
            self.current
                .parse_san(san, SanMode::Tolerant)
                .ok()
                .map(|p| p.mv)
        })
    }

    /// The ply (0 = the start) at which the position is `target`, for a
    /// takeback.
    fn ply_of(&self, target: &Position) -> Option<usize> {
        let wanted = key(target);
        let mut position = self.start.clone();
        if key(&position) == wanted {
            return Some(0);
        }
        for (i, mv) in self.moves.iter().enumerate() {
            position.play(*mv).ok()?;
            if key(&position) == wanted {
                return Some(i + 1);
            }
        }
        None
    }

    /// The position now.
    #[must_use]
    pub fn position(&self) -> &Position {
        &self.current
    }

    /// Plies in the history.
    #[must_use]
    pub fn plies(&self) -> usize {
        self.moves.len()
    }

    /// The side to move.
    #[must_use]
    pub fn turn(&self) -> Color {
        self.current.turn()
    }

    /// Whether `line` can be the next board of this very game (D2690): the
    /// game is not over, the user has the same role and, when he plays, the
    /// players are the same. Anything else under the same number is a new
    /// game (the server reuses numbers).
    #[must_use]
    pub fn continues(&self, line: &Style12) -> bool {
        self.number == line.game
            && self.result.is_none()
            && role_of(line.relation) == Some(self.role)
            && (self.role != Role::Playing
                || (self.white.eq_ignore_ascii_case(&line.white)
                    && self.black.eq_ignore_ascii_case(&line.black)))
    }

    /// Whether the game is standard chess (D2600, D2693): a variant the
    /// server told about is not one.
    #[must_use]
    pub fn is_standard(&self) -> bool {
        self.kind.is_empty() || super::seeks::PLAYABLE_KINDS.contains(&self.kind.as_str())
    }

    /// The user plays and the game is on.
    #[must_use]
    pub fn is_live_player(&self) -> bool {
        self.role == Role::Playing && self.result.is_none()
    }

    /// The user plays, the game is on and it is his turn.
    #[must_use]
    pub fn my_turn(&self) -> bool {
        self.is_live_player() && self.my_color == Some(self.turn()) && !self.pending
    }

    /// A move of the user waits for the server.
    #[must_use]
    pub fn has_pending_move(&self) -> bool {
        self.pending
    }

    /// Plays the move of the user at once, before the server confirms it.
    ///
    /// # Errors
    /// `Err(())` when the core refuses the move or the history is full.
    #[allow(clippy::result_unit_err)] // nothing to say but "refused"
    pub fn push_local(&mut self, mv: Move) -> Result<(), ()> {
        if self.moves.len() >= MAX_PLIES {
            return Err(());
        }
        let next = self.current.after(mv).map_err(|_| ())?;
        self.current = next;
        self.moves.push(mv);
        self.emt.push(None);
        self.clk.push(None);
        self.pending = true;
        Ok(())
    }

    /// Takes the unconfirmed move out (the server refused it).
    pub fn undo_local(&mut self) -> bool {
        if !self.pending {
            return false;
        }
        self.pending = false;
        self.moves.pop();
        self.emt.pop();
        self.clk.pop();
        let mut position = self.start.clone();
        for mv in &self.moves {
            if position.play(*mv).is_err() {
                break;
            }
        }
        self.current = position;
        true
    }

    /// The time left to `side` at `now` (D2613: the clock of the server,
    /// interpolated for the side to move).
    #[must_use]
    pub fn shown(&self, side: Color, now: Instant) -> Duration {
        let base = match side {
            Color::White => self.white_ms,
            Color::Black => self.black_ms,
        };
        let running = self.result.is_none() && self.role != Role::Examining && self.turn() == side;
        let spent = if running {
            now.saturating_duration_since(self.synced_at)
        } else {
            Duration::ZERO
        };
        Duration::from_millis(base).saturating_sub(spent)
    }

    /// Records the result of the server.
    pub fn finish(&mut self, result: FicsResult, reason: &str) {
        let reason: String = reason
            .chars()
            .filter(|c| !c.is_control())
            .take(256)
            .collect();
        self.result = Some((result, reason));
    }

    /// The game for the editor, with `[%emt]` and `[%clk]` comments (D2623)
    /// when asked.
    #[must_use]
    pub fn to_game(&self, emt: bool, clk: bool) -> Game {
        let mut game = if self.start_is_standard {
            Game::new()
        } else {
            Game::from_position(self.start.clone())
        };
        self.write_tags(&mut game);
        let mut parent = game.root();
        for (i, mv) in self.moves.iter().enumerate() {
            let Ok(node) = game.add_move(parent, *mv) else {
                break;
            };
            let mut parts: Vec<String> = Vec::new();
            if emt && let Some(ms) = self.emt.get(i).copied().flatten() {
                parts.push(comment("emt", u64::from(ms)));
            }
            if clk && let Some(ms) = self.clk.get(i).copied().flatten() {
                parts.push(comment("clk", ms));
            }
            if !parts.is_empty() {
                // A comment the game refuses is dropped: the move stays.
                game.set_comment(node, &parts.join(" ")).ok();
            }
            parent = node;
        }
        game
    }

    fn write_tags(&self, game: &mut Game) {
        let tags = game.tags_mut();
        let rated = if self.rated { "rated" } else { "unrated" };
        tags.event = if self.kind.is_empty() {
            format!("FICS {rated} game")
        } else {
            format!("FICS {rated} {} game", self.kind)
        };
        "freechess.org".clone_into(&mut tags.site);
        "-".clone_into(&mut tags.round);
        if let Some((y, m, d)) = self.date {
            tags.date = PartialDate::new(u16::try_from(y).ok(), Some(m), Some(d))
                .unwrap_or(PartialDate::UNKNOWN);
        }
        tags.white.clone_from(&self.white);
        tags.black.clone_from(&self.black);
        tags.result = match self.result {
            Some((FicsResult::WhiteWins, _)) => GameResult::White,
            Some((FicsResult::BlackWins, _)) => GameResult::Black,
            Some((FicsResult::Draw, _)) => GameResult::Draw,
            _ => GameResult::Unknown,
        };
        tags.white_rating = rating(self.white_rating);
        tags.black_rating = rating(self.black_rating);
        tags.set_extra(
            "TimeControl",
            &format!("{}+{}", self.minutes.saturating_mul(60), self.increment),
        )
        .ok();
        if let Some((_, reason)) = &self.result {
            tags.set_extra("Termination", reason).ok();
        }
    }

    /// A key that tells two games of the same encounter apart, for the
    /// idempotent import (D2624).
    #[must_use]
    pub fn import_key(&self) -> String {
        let date = self
            .date
            .map_or_else(String::new, |(y, m, d)| format!("{y}.{m}.{d}"));
        let result = self
            .result
            .as_ref()
            .map_or_else(String::new, |(r, _)| format!("{r:?}"));
        format!(
            "{}|{}|{}|{date}|{}|{result}",
            self.number,
            self.white,
            self.black,
            self.moves.len()
        )
    }
}

fn opposite(color: Color) -> Color {
    match color {
        Color::White => Color::Black,
        Color::Black => Color::White,
    }
}

fn rating(value: Option<u32>) -> Option<Rating> {
    let value = u16::try_from(value?).ok()?;
    Elo::new(value)
        .ok()
        .filter(|e| e.get() > 0)
        .map(|e| Rating::new(e, RatingType::Elo))
}

/// `[%emt h:mm:ss]` or `[%clk h:mm:ss]` for `ms` milliseconds.
fn comment(name: &str, ms: u64) -> String {
    let s = ms / 1000;
    format!("[%{name} {}:{:02}:{:02}]", s / 3600, s / 60 % 60, s % 60)
}

/// Today (UTC) as year, month, day, from the clock of the system.
fn today() -> Option<(i64, u8, u8)> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(civil_from_days(i64::try_from(secs / 86_400).ok()?))
}

/// Days since 1970-01-01 to a civil date (Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (
        year,
        u8::try_from(m).unwrap_or(1),
        u8::try_from(d).unwrap_or(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> Style12 {
        Style12::parse(text).expect("a valid line")
    }

    const START: &str = "<12> rnbqkbnr pppppppp -------- -------- -------- -------- PPPPPPPP RNBQKBNR W -1 1 1 1 1 0 7 alice bob 1 5 0 39 39 300 300 1 none (0:00) none 0";
    const E4: &str = "<12> rnbqkbnr pppppppp -------- -------- ----P--- -------- PPPP-PPP RNBQKBNR B 4 1 1 1 1 0 7 alice bob -1 5 0 39 39 297 300 1 P/e2-e4 (0:03) e4 0";
    const E5: &str = "<12> rnbqkbnr pppp-ppp -------- ----p--- ----P--- -------- PPPP-PPP RNBQKBNR W 4 1 1 1 1 0 7 alice bob 1 5 0 39 39 297 296 2 P/e7-e5 (0:04) e5 0";

    fn game() -> (FicsGame, Instant) {
        let now = Instant::now();
        (
            FicsGame::from_style12(&line(START), None, now).expect("starts"),
            now,
        )
    }

    #[test]
    fn starts_from_the_initial_position_and_follows_moves() {
        let (mut g, now) = game();
        assert_eq!(g.role, Role::Playing);
        assert_eq!(g.my_color, Some(Color::White));
        assert!(g.my_turn() && !g.incomplete);
        assert_eq!(g.apply(&line(E4), now), Applied::Moved);
        assert_eq!(g.apply(&line(E5), now), Applied::Moved);
        assert_eq!(g.plies(), 2);
        // The same line again only syncs the clocks.
        assert_eq!(g.apply(&line(E5), now), Applied::Same);
        assert_eq!(g.plies(), 2);
        let game = g.to_game(true, true);
        assert_eq!(game.tags().white, "alice");
        let nodes: Vec<_> = game.mainline().collect();
        assert_eq!(nodes.len(), 2);
        assert_eq!(
            game.comment(nodes[0]),
            Ok(Some("[%emt 0:00:03] [%clk 0:04:57]"))
        );
    }

    #[test]
    fn comments_follow_the_switches() {
        let (mut g, now) = game();
        g.apply(&line(E4), now);
        let off = g.to_game(false, false);
        let node = off.mainline().next().expect("one move");
        assert!(matches!(off.comment(node), Ok(None | Some(""))));
        let only_clk = g.to_game(false, true);
        let node = only_clk.mainline().next().expect("one move");
        assert_eq!(only_clk.comment(node), Ok(Some("[%clk 0:04:57]")));
    }

    #[test]
    fn a_takeback_truncates_and_an_unknown_board_resyncs() {
        let (mut g, now) = game();
        g.apply(&line(E4), now);
        g.apply(&line(E5), now);
        assert_eq!(g.apply(&line(START), now), Applied::TookBack);
        assert_eq!(g.plies(), 0);
        g.apply(&line(E4), now);
        let odd = "<12> rnbqkbnr pppppppp -------- -------- -------- ----N--- PPPPPPPP RNBQKB-R B -1 1 1 1 1 1 7 alice bob 1 5 0 39 39 300 300 1 N/g1-e2 (0:01) Ne2 0";
        assert_eq!(g.apply(&line(odd), now), Applied::Resynced);
        assert!(g.incomplete);
        assert_eq!(g.plies(), 0);
    }

    #[test]
    fn the_optimistic_move_is_confirmed_or_undone() {
        let (mut g, now) = game();
        let mv = g.position().parse_uci("e2e4").expect("legal");
        g.push_local(mv).expect("played");
        assert!(g.has_pending_move() && !g.my_turn());
        assert_eq!(g.plies(), 1);
        // Confirmed by the line that has it.
        assert_eq!(g.apply(&line(E4), now), Applied::Same);
        assert!(!g.has_pending_move());

        let (mut g, now) = game();
        g.push_local(g.position().parse_uci("d2d4").expect("legal"))
            .expect("played");
        // The server answers with the old board: the move is out.
        assert_eq!(g.apply(&line(START), now), Applied::Same);
        assert_eq!(g.plies(), 0);
        assert!(g.my_turn());
    }

    #[test]
    fn undo_local_restores_the_position() {
        let (mut g, _) = game();
        let before = g.position().fen();
        g.push_local(g.position().parse_uci("e2e4").expect("legal"))
            .expect("played");
        assert!(g.undo_local());
        assert_eq!(g.position().fen(), before);
        assert!(!g.undo_local());
    }

    #[test]
    fn castling_comes_as_o_o() {
        let setup = "<12> r---k--r pppppppp -------- -------- -------- -------- PPPPPPPP R---K--R W -1 1 1 1 1 0 3 a b 1 5 0 39 39 300 300 1 none (0:00) none 0";
        let castled = "<12> r---k--r pppppppp -------- -------- -------- -------- PPPPPPPP R----RK- B -1 0 0 1 1 1 3 a b -1 5 0 39 39 299 300 1 o-o (0:01) O-O 0";
        let now = Instant::now();
        let mut g = FicsGame::from_style12(&line(setup), None, now).expect("starts");
        assert_eq!(g.apply(&line(castled), now), Applied::Moved);
        assert_eq!(g.plies(), 1);
    }

    #[test]
    fn the_clock_is_the_servers_and_only_the_mover_runs() {
        let (mut g, now) = game();
        let later = now + Duration::from_secs(10);
        assert_eq!(g.shown(Color::White, later), Duration::from_secs(290));
        assert_eq!(g.shown(Color::Black, later), Duration::from_secs(300));
        assert_eq!(
            g.shown(Color::White, now + Duration::from_secs(9999)),
            Duration::ZERO
        );
        g.finish(FicsResult::WhiteWins, "bob resigns");
        assert_eq!(g.shown(Color::White, later), Duration::from_secs(300));
        assert!(!g.is_live_player());
    }

    #[test]
    fn results_and_ratings_reach_the_tags() {
        let (mut g, _) = game();
        g.white_rating = Some(1500);
        g.rated = true;
        g.kind = "blitz".into();
        g.finish(FicsResult::Draw, "Game drawn by mutual agreement");
        let game = g.to_game(true, true);
        assert_eq!(game.tags().result, GameResult::Draw);
        assert_eq!(game.tags().event, "FICS rated blitz game");
        assert!(game.tags().white_rating.is_some());
        assert_eq!(game.tags().get_extra("TimeControl"), Some("300+0"));
    }

    #[test]
    fn isolated_positions_and_bad_boards_are_not_games() {
        let isolated = START.replace(" 1 5 0 39 39", " -3 5 0 39 39");
        assert!(FicsGame::from_style12(&line(&isolated), None, Instant::now()).is_none());
        // Two white kings: valid syntax, invalid position.
        let bad = "<12> rnbqkbnr pppppppp -------- -------- -------- -------- PPPPPPPP RNBQKBKR W -1 0 0 0 0 0 7 a b 0 5 0 39 39 300 300 1 none (0:00) none 0";
        assert!(FicsGame::from_style12(&line(bad), None, Instant::now()).is_none());
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }
}
