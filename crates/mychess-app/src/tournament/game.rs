//! One game between two engines, as a pure state machine (spec M8-2, D1267,
//! D1269, D1270, D1271, D1274-D1277).
//!
//! It reuses what M8-1 built: [`PlayClock`] (with `press_with` for the time
//! measured by the task that talks to the engine, and `press_free` for book
//! and opening moves) and the end-of-game rules of `play::rules`. It never
//! reads the system clock: every call receives an [`Instant`] (and the
//! front-end's tasks measure the thinking time, D1267).
//!
//! Protocol of use: call [`TournamentGame::advance`]; it plays book moves by
//! itself and answers either [`Step::Think`] (an engine must move: send the
//! request, then call [`TournamentGame::reply`]) or [`Step::Over`].

use std::sync::Arc;
use std::time::{Duration, Instant};

use mychess_core::{Color, GameResult, Move, Position};
use mychess_engine::Score as EngineScore;

use super::adjudicate::{Adjudicator, TablebaseProbe};
use super::config::{Adjudication, TimeControl};
use super::openings::{Opening, mix};
use crate::play::rules::{PlayEnd, position_end, repetition_key, timeout_end};
use crate::play::{PlayBook, PlayClock, SideBudget, Think, ThinkLimits};

/// How a game ended, for the `Termination` tag (D1280).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Termination {
    /// Mate, stalemate, draw by the rules.
    Normal,
    /// The flag fell.
    TimeForfeit,
    /// An illegal move or a protocol error.
    RulesInfraction,
    /// The engine died or hung and the game was not repeated.
    Abandoned,
    /// Decided by an adjudication (manual, automatic or tablebase).
    Adjudication,
    /// Stopped without a result.
    Unterminated,
}

impl Termination {
    /// The value of the PGN tag.
    pub fn tag(self) -> &'static str {
        match self {
            Termination::Normal => "normal",
            Termination::TimeForfeit => "time forfeit",
            Termination::RulesInfraction => "rules infraction",
            Termination::Abandoned => "abandoned",
            Termination::Adjudication => "adjudication",
            Termination::Unterminated => "unterminated",
        }
    }

    /// Reads a tag value.
    pub fn from_tag(text: &str) -> Option<Termination> {
        [
            Termination::Normal,
            Termination::TimeForfeit,
            Termination::RulesInfraction,
            Termination::Abandoned,
            Termination::Adjudication,
            Termination::Unterminated,
        ]
        .into_iter()
        .find(|t| t.tag() == text)
    }
}

/// The end of a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameEnd {
    /// The result.
    pub result: GameResult,
    /// How it ended.
    pub termination: Termination,
    /// The text of the comment on the last move (`3 fold repetition`).
    pub comment: Option<String>,
}

/// The note of an engine for a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveScore {
    /// The score from the point of view of the side that moved (positive:
    /// good for the mover). [`TournamentGame::reply`] converts it once, from
    /// the White point of view the engine layer delivers (D1302).
    pub score: EngineScore,
    /// The depth that reached it.
    pub depth: Option<u32>,
}

/// A move of the game with what was measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayedMove {
    /// The move.
    pub mv: Move,
    /// The side that played it.
    pub side: Color,
    /// A book or opening move: no time, no note (D1267).
    pub free: bool,
    /// The time the engine used.
    pub spent: Option<Duration>,
    /// The clock of the side after the move (countdown only).
    pub left: Option<Duration>,
    /// The note of the engine.
    pub score: Option<MoveScore>,
    /// Extra comment (`Last book move`).
    pub note: Option<String>,
}

/// What an engine answered to a [`Think`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// The move; `None` for `bestmove (none)` or an illegal move.
    pub mv: Option<Move>,
    /// The time between `go` (or `ponderhit`) and `bestmove`, measured by
    /// the task of the engine.
    pub spent: Duration,
    /// The last note the engine sent, as `mychess-engine` delivers it: from
    /// the point of view of WHITE, whoever moved (D1302).
    pub score: Option<EngineScore>,
    /// The depth of that note.
    pub depth: Option<u32>,
    /// The reply the engine expects, to ponder on.
    pub ponder: Option<Move>,
}

/// The engine that waits for the opponent thinking about the expected move
/// (D1270).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PonderPlan {
    /// The engine that ponders.
    pub side: Color,
    /// The search: the position after the expected move.
    pub think: Think,
    /// The move the engine expects.
    pub expected: Move,
}

/// What an engine has to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    /// The search.
    pub think: Think,
    /// The longest to wait for the `bestmove` (D1268).
    pub deadline: Duration,
    /// The opponent ponders meanwhile.
    pub ponder: Option<PonderPlan>,
}

/// The answer of [`TournamentGame::advance`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// An engine must move.
    Think(Box<Turn>),
    /// The game is over.
    Over(GameEnd),
}

/// What a game needs to be played.
#[derive(Debug, Clone)]
pub struct GameSetup {
    /// Where it starts, with the moves played at once.
    pub opening: Opening,
    /// Those moves come from a book: the last one is commented.
    pub opening_is_book: bool,
    /// The limits.
    pub time: TimeControl,
    /// Per cent of the time of `MoveTime` tolerated (D1267).
    pub leeway_percent: u32,
    /// The margin of the watchdog (D1268).
    pub margin: Duration,
    /// The ceiling per move of `Depth`/`Nodes`.
    pub depth_limit: Duration,
    /// Repetition and 50 moves end the game (D1274).
    pub claim_draws: bool,
    /// The adjudication rules.
    pub adjudication: Adjudication,
    /// The book that plays the first moves.
    pub book: Option<PlayBook>,
    /// The most plies taken from the book.
    pub book_ply_limit: Option<u32>,
    /// Each side thinks on the time of the opponent: `[white, black]`.
    pub ponder: [bool; 2],
    /// Seed of the draws.
    pub seed: u64,
    /// The number of the game, a key of the draws.
    pub number: u32,
    /// The tablebase, if any.
    pub tablebase: Option<Arc<dyn TablebaseProbe>>,
}

fn index(color: Color) -> usize {
    match color {
        Color::White => 0,
        Color::Black => 1,
    }
}

/// The score as seen by `mover`, given the White point of view.
fn mover_pov(score: EngineScore, mover: Color) -> EngineScore {
    match (score, mover) {
        (s, Color::White) => s,
        (EngineScore::Cp(cp), Color::Black) => EngineScore::Cp(cp.saturating_neg()),
        (EngineScore::Mate(n), Color::Black) => EngineScore::Mate(n.saturating_neg()),
    }
}

fn side_name(color: Color) -> &'static str {
    match color {
        Color::White => "White",
        Color::Black => "Black",
    }
}

fn win(color: Color) -> GameResult {
    match color {
        Color::White => GameResult::White,
        Color::Black => GameResult::Black,
    }
}

/// A game between two engines.
#[derive(Debug)]
pub struct TournamentGame {
    setup: GameSetup,
    start: Position,
    position: Position,
    moves: Vec<Move>,
    played: Vec<PlayedMove>,
    keys: Vec<String>,
    clock: PlayClock,
    adjudicator: Adjudicator,
    in_book: bool,
    book_plies: u32,
    ended: Option<GameEnd>,
    pending_ponder: Option<(Color, Move)>,
}

impl TournamentGame {
    /// Starts the game: the pre-played moves of the opening are made at
    /// once, without time (D1267).
    pub fn new(setup: GameSetup, now: Instant) -> TournamentGame {
        let start = setup.opening.start.clone();
        let clock = match setup.time {
            TimeControl::Clock {
                base_ms,
                increment_ms,
            } => {
                // A zero base would flag the first move whatever it costs:
                // the first move gets the increment as its time.
                let budget = SideBudget {
                    // Never zero: a flag that falls before the first move
                    // would end every game at once (D1309).
                    time: Duration::from_millis(base_ms.max(increment_ms).max(1)),
                    increment: Duration::from_millis(increment_ms),
                };
                PlayClock::countdown(budget, budget)
            }
            _ => PlayClock::count_up(),
        };
        let mut game = TournamentGame {
            keys: vec![repetition_key(&start)],
            position: start.clone(),
            start,
            moves: Vec::new(),
            played: Vec::new(),
            clock,
            adjudicator: Adjudicator::new(setup.adjudication),
            in_book: setup.book.is_some(),
            book_plies: 0,
            ended: None,
            pending_ponder: None,
            setup,
        };
        // The clock is stopped: the model never counts the time of a move
        // itself (the task of the engine does, D1267).
        game.clock.start(game.position.turn(), now);
        game.clock.stop(now);
        let opening_moves = game.setup.opening.moves.clone();
        for mv in opening_moves {
            if !game.push_free(mv, now) {
                break;
            }
        }
        if game.setup.opening_is_book {
            game.note_last_book_move();
        }
        game.check_end();
        game
    }

    // ---- reading ----

    /// The position now.
    pub fn position(&self) -> &Position {
        &self.position
    }

    /// The position the game started from.
    pub fn start(&self) -> &Position {
        &self.start
    }

    /// The moves played, from the start.
    pub fn moves(&self) -> &[Move] {
        &self.moves
    }

    /// The moves with what was measured.
    pub fn played(&self) -> &[PlayedMove] {
        &self.played
    }

    /// The clock (for the live display: `shown` at the tick instant).
    pub fn clock(&self) -> &PlayClock {
        &self.clock
    }

    /// The end, once the game is over.
    pub fn end(&self) -> Option<&GameEnd> {
        self.ended.as_ref()
    }

    /// The side to move.
    pub fn side_to_move(&self) -> Color {
        self.position.turn()
    }

    /// The leading moves taken from the book, for the other game of the
    /// pair (D1273).
    pub fn book_line(&self) -> Vec<Move> {
        self.played
            .iter()
            .take_while(|m| m.free)
            .take(usize::try_from(self.book_plies).unwrap_or(usize::MAX))
            .map(|m| m.mv)
            .collect()
    }

    // ---- moving ----

    fn push_free(&mut self, mv: Move, now: Instant) -> bool {
        let side = self.position.turn();
        if self.position.play(mv).is_err() {
            return false;
        }
        self.clock.press_free(now);
        self.moves.push(mv);
        self.keys.push(repetition_key(&self.position));
        self.played.push(PlayedMove {
            mv,
            side,
            free: true,
            spent: None,
            left: None,
            score: None,
            note: None,
        });
        true
    }

    fn note_last_book_move(&mut self) {
        let name = self
            .setup
            .book
            .as_ref()
            .map_or("book", PlayBook::book_name)
            .to_owned();
        if let Some(last) = self.played.last_mut().filter(|m| m.free) {
            last.note = Some(format!("Last book move ({name})"));
        }
    }

    fn plies(&self) -> u32 {
        u32::try_from(self.moves.len()).unwrap_or(u32::MAX)
    }

    fn check_end(&mut self) {
        if self.ended.is_some() {
            return;
        }
        if let Some(end) = position_end(&self.position, &self.keys) {
            let draws_by_claim = matches!(end, PlayEnd::FiftyMoves | PlayEnd::ThreefoldRepetition);
            if !draws_by_claim || self.setup.claim_draws {
                let comment = match end {
                    PlayEnd::Stalemate => Some("Stalemate".to_owned()),
                    PlayEnd::InsufficientMaterial => Some("Insufficient material".to_owned()),
                    other => other.comment().map(str::to_owned),
                };
                self.ended = Some(GameEnd {
                    result: end.result(),
                    termination: Termination::Normal,
                    comment,
                });
                return;
            }
        }
        let verdict = self.adjudicator.verdict(self.plies()).or_else(|| {
            self.adjudicator
                .tablebase_verdict(self.setup.tablebase.as_deref(), &self.position)
        });
        if let Some(verdict) = verdict {
            self.ended = Some(GameEnd {
                result: verdict.result,
                termination: Termination::Adjudication,
                comment: Some(verdict.reason),
            });
        }
    }

    /// Plays the book moves and says what is next (see the module docs).
    pub fn advance(&mut self, now: Instant) -> Step {
        self.play_book(now);
        if let Some(end) = &self.ended {
            return Step::Over(end.clone());
        }
        let side = self.position.turn();
        let limits = match self.setup.time {
            TimeControl::Clock { .. } => ThinkLimits::Clock(self.clock.search_clock(now)),
            TimeControl::MoveTime { ms } => ThinkLimits::MoveTime(Duration::from_millis(ms)),
            TimeControl::Depth(depth) => ThinkLimits::Depth(depth),
            TimeControl::Nodes(nodes) => ThinkLimits::Nodes(nodes),
        };
        let deadline = match self.setup.time {
            TimeControl::Clock { .. } => self
                .clock
                .shown(side, now)
                .saturating_add(self.setup.margin),
            TimeControl::MoveTime { ms } => Duration::from_millis(ms)
                .saturating_mul(self.setup.leeway_percent)
                .checked_div(100)
                .unwrap_or_default()
                .saturating_add(self.setup.margin),
            TimeControl::Depth(_) | TimeControl::Nodes(_) => self.setup.depth_limit,
        };
        let think = Think {
            start: self.start.clone(),
            moves: self.moves.clone(),
            side,
            limits,
        };
        let ponder = self.ponder_plan(&think);
        // The clock of the display runs while the engine thinks.
        self.clock.resume(now);
        Step::Think(Box::new(Turn {
            think,
            deadline,
            ponder,
        }))
    }

    fn play_book(&mut self, now: Instant) {
        while self.in_book && self.ended.is_none() {
            let limit_hit = self
                .setup
                .book_ply_limit
                .is_some_and(|l| self.book_plies >= l);
            let pick = if limit_hit {
                None
            } else {
                let roll = mix(
                    self.setup.seed,
                    u64::from(self.setup.number),
                    u64::from(self.plies()),
                );
                self.setup
                    .book
                    .as_ref()
                    .and_then(|book| book.pick(&self.position, roll))
            };
            match pick {
                Some(mv) if self.push_free(mv, now) => {
                    self.book_plies += 1;
                    self.check_end();
                }
                _ => {
                    self.in_book = false;
                    if self.book_plies > 0 {
                        self.note_last_book_move();
                    }
                }
            }
        }
    }

    /// The engine that just moved ponders while the other thinks (D1270).
    fn ponder_plan(&self, think: &Think) -> Option<PonderPlan> {
        let (side, expected) = self.pending_ponder?;
        if side == think.side || !self.setup.ponder[index(side)] {
            return None;
        }
        // The expected move must be legal for the engine that is thinking.
        if !self.position.is_legal(expected) {
            return None;
        }
        let mut moves = self.moves.clone();
        moves.push(expected);
        Some(PonderPlan {
            side,
            think: Think {
                start: self.start.clone(),
                moves,
                side,
                limits: think.limits,
            },
            expected,
        })
    }

    /// Applies the answer of the engine that was asked to move.
    pub fn reply(&mut self, reply: &Reply, now: Instant) {
        if self.ended.is_some() {
            return;
        }
        let side = self.position.turn();
        let Some(mv) = reply.mv.filter(|m| self.position.is_legal(*m)) else {
            self.forfeit(side, Termination::RulesInfraction);
            return;
        };
        let timed = self.clock.press_with(reply.spent, now);
        self.clock.stop(now);
        let over_leeway = match self.setup.time {
            TimeControl::MoveTime { ms } => {
                let allowed = Duration::from_millis(ms)
                    .saturating_mul(self.setup.leeway_percent)
                    .checked_div(100)
                    .unwrap_or_default();
                reply.spent > allowed
            }
            _ => false,
        };
        if timed.flagged || over_leeway {
            self.flag_fell(side);
            return;
        }
        let left = self
            .clock
            .is_countdown()
            .then(|| self.clock.shown(side, now));
        if self.position.play(mv).is_err() {
            self.forfeit(side, Termination::RulesInfraction);
            return;
        }
        let note = reply.score.map(|score| mover_pov(score, side));
        self.moves.push(mv);
        self.keys.push(repetition_key(&self.position));
        self.played.push(PlayedMove {
            mv,
            side,
            free: false,
            spent: Some(reply.spent),
            left,
            score: note.map(|score| MoveScore {
                score,
                depth: reply.depth,
            }),
            note: None,
        });
        self.adjudicator
            .record(side, note.map(EngineScore::as_centipawns));
        self.pending_ponder = reply.ponder.map(|p| (side, p));
        self.check_end();
    }

    // ---- other ways to end ----

    /// Forgets the move the engine that just moved expects (its pondering
    /// stopped); the next search is a normal one.
    pub fn forget_ponder(&mut self) {
        self.pending_ponder = None;
    }

    /// The flag of `loser` fell: a loss, or a draw when the opponent cannot
    /// mate (D1226).
    pub fn flag_fell(&mut self, loser: Color) {
        if self.ended.is_some() {
            return;
        }
        let end = timeout_end(&self.position, loser);
        self.ended = Some(GameEnd {
            result: end.result(),
            termination: Termination::TimeForfeit,
            comment: Some(format!("{} loses on time", side_name(loser))),
        });
    }

    /// `loser` loses for a reason of its own (D1269).
    pub fn forfeit(&mut self, loser: Color, termination: Termination) {
        if self.ended.is_some() {
            return;
        }
        let comment = match termination {
            Termination::RulesInfraction => format!("{} made an illegal move", side_name(loser)),
            Termination::Abandoned => format!("{} engine failed", side_name(loser)),
            Termination::TimeForfeit => format!("{} loses on time", side_name(loser)),
            _ => format!("{} loses", side_name(loser)),
        };
        self.ended = Some(GameEnd {
            result: win(!loser),
            termination,
            comment: Some(comment),
        });
    }

    /// The user decided the game (D1275).
    pub fn adjudicate(&mut self, result: GameResult) {
        if self.ended.is_some() {
            return;
        }
        self.ended = Some(GameEnd {
            result,
            termination: Termination::Adjudication,
            comment: Some("Manual adjudication".to_owned()),
        });
    }

    /// The game stops without a result.
    pub fn abort(&mut self) {
        if self.ended.is_none() {
            self.ended = Some(GameEnd {
                result: GameResult::Unknown,
                termination: Termination::Unterminated,
                comment: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use mychess_book::Book;

    use super::*;
    use crate::play::BookSelection;

    fn setup(time: TimeControl) -> GameSetup {
        GameSetup {
            opening: Opening::initial(),
            opening_is_book: false,
            time,
            leeway_percent: 175,
            margin: Duration::from_secs(2),
            depth_limit: Duration::from_secs(600),
            claim_draws: true,
            adjudication: Adjudication::default(),
            book: None,
            book_ply_limit: None,
            ponder: [false, false],
            seed: 1,
            number: 1,
            tablebase: None,
        }
    }

    fn clock(base: u64, inc: u64) -> TimeControl {
        TimeControl::Clock {
            base_ms: base,
            increment_ms: inc,
        }
    }

    fn mv(position: &Position, uci: &str) -> Move {
        position.parse_uci(uci).expect("uci")
    }

    fn reply(game: &TournamentGame, uci: &str, spent_ms: u64) -> Reply {
        Reply {
            mv: Some(mv(game.position(), uci)),
            spent: Duration::from_millis(spent_ms),
            score: Some(EngineScore::Cp(20)),
            depth: Some(12),
            ponder: None,
        }
    }

    fn play_all(game: &mut TournamentGame, ucis: &[&str], now: Instant) {
        for uci in ucis {
            let Step::Think(_) = game.advance(now) else {
                panic!("game ended early");
            };
            let r = reply(game, uci, 100);
            game.reply(&r, now);
        }
    }

    fn book_with_two_moves() -> PlayBook {
        let start = Position::new();
        let e4 = start.parse_uci("e2e4").expect("uci");
        let after = start.after(e4).expect("after");
        let e5 = after.parse_uci("e7e5").expect("uci");
        let mut book = Book::new();
        book.set_position_moves(&start, &[(e4, 10)]).expect("set");
        book.set_position_moves(&after, &[(e5, 10)]).expect("set");
        PlayBook::new(book, BookSelection::Best)
    }

    #[test]
    fn the_first_think_has_the_clock_and_the_deadline() {
        let now = Instant::now();
        let mut game = TournamentGame::new(setup(clock(60_000, 1_000)), now);
        let Step::Think(turn) = game.advance(now) else {
            panic!("expected a search");
        };
        assert_eq!(turn.think.side, Color::White);
        assert!(turn.think.moves.is_empty());
        let ThinkLimits::Clock(c) = turn.think.limits else {
            panic!("expected a clock");
        };
        assert_eq!(c.white_time, Duration::from_secs(60));
        assert_eq!(c.black_increment, Duration::from_secs(1));
        assert_eq!(turn.deadline, Duration::from_secs(62), "time left + 2 s");
    }

    #[test]
    fn each_mode_builds_its_limits_and_deadline() {
        let now = Instant::now();
        let cases = [
            (
                TimeControl::MoveTime { ms: 1_000 },
                ThinkLimits::MoveTime(Duration::from_secs(1)),
                Duration::from_millis(1_750 + 2_000),
            ),
            (
                TimeControl::Depth(9),
                ThinkLimits::Depth(9),
                Duration::from_secs(600),
            ),
            (
                TimeControl::Nodes(5_000),
                ThinkLimits::Nodes(5_000),
                Duration::from_secs(600),
            ),
        ];
        for (time, limits, deadline) in cases {
            let mut game = TournamentGame::new(setup(time), now);
            let Step::Think(turn) = game.advance(now) else {
                panic!("expected a search");
            };
            assert_eq!(turn.think.limits, limits);
            assert_eq!(turn.deadline, deadline);
        }
    }

    #[test]
    fn the_increment_is_added_after_the_first_move_of_each_side() {
        let now = Instant::now();
        let mut game = TournamentGame::new(setup(clock(10_000, 2_000)), now);
        play_all(&mut game, &["e2e4", "e7e5"], now);
        let times = game.clock().shown(Color::White, now);
        assert_eq!(times, Duration::from_millis(10_000 - 100 + 2_000));
        assert_eq!(
            game.clock().shown(Color::Black, now),
            Duration::from_millis(11_900)
        );
        let played = game.played();
        assert_eq!(played[0].left, Some(Duration::from_millis(11_900)));
        assert_eq!(played[1].spent, Some(Duration::from_millis(100)));
        assert_eq!(played[0].score.map(|s| s.depth), Some(Some(12)));
    }

    #[test]
    fn a_late_reply_loses_on_time() {
        let now = Instant::now();
        let mut game = TournamentGame::new(setup(clock(1_000, 0)), now);
        let Step::Think(_) = game.advance(now) else {
            panic!("search")
        };
        let late = reply(&game, "e2e4", 1_500);
        game.reply(&late, now);
        let end = game.end().expect("ended");
        assert_eq!(end.result, GameResult::Black);
        assert_eq!(end.termination, Termination::TimeForfeit);
        assert_eq!(end.comment.as_deref(), Some("White loses on time"));
        assert!(game.moves().is_empty(), "the late move does not count");
    }

    #[test]
    fn a_flag_against_a_side_that_cannot_mate_is_a_draw() {
        let now = Instant::now();
        let mut s = setup(clock(1_000, 0));
        // Black: king + pawn. White: king + bishop + pawn... White flags;
        // Black has a pawn and can promote and mate => White loses. Instead
        // use K+N+N? Use: Black has K only plus a pawn blocked? Keep simple:
        // White K+P, Black K+B+... Black bare king cannot mate White.
        s.opening = Opening {
            name: None,
            start: Position::from_fen("8/8/4k3/8/8/3K4/4P3/8 w - - 0 1").expect("fen"),
            moves: Vec::new(),
        };
        let mut game = TournamentGame::new(s, now);
        let Step::Think(_) = game.advance(now) else {
            panic!("search")
        };
        // Black has a bare king: if Black's flag falls White can mate => win.
        // If White's flag falls, Black (bare king) cannot mate => draw.
        let late = reply(&game, "d3d4", 5_000);
        game.reply(&late, now);
        let end = game.end().expect("ended");
        assert_eq!(end.result, GameResult::Draw);
        assert_eq!(end.termination, Termination::TimeForfeit);
    }

    #[test]
    fn move_time_tolerates_the_leeway_and_punishes_more() {
        let now = Instant::now();
        let mut game = TournamentGame::new(setup(TimeControl::MoveTime { ms: 1_000 }), now);
        play_all(&mut game, &["e2e4"], now);
        let ok = reply(&game, "e7e5", 1_700);
        game.reply(&ok, now);
        assert!(game.end().is_none(), "170% is inside the 175% leeway");
        let slow = reply(&game, "g1f3", 1_800);
        game.reply(&slow, now);
        let end = game.end().expect("ended");
        assert_eq!(end.result, GameResult::Black);
        assert_eq!(end.termination, Termination::TimeForfeit);
    }

    #[test]
    fn an_illegal_or_missing_move_is_a_loss_for_that_engine() {
        let now = Instant::now();
        let mut game = TournamentGame::new(setup(TimeControl::Depth(1)), now);
        let Step::Think(_) = game.advance(now) else {
            panic!("search")
        };
        let none = Reply {
            mv: None,
            spent: Duration::ZERO,
            score: None,
            depth: None,
            ponder: None,
        };
        game.reply(&none, now);
        let end = game.end().expect("ended");
        assert_eq!(end.result, GameResult::Black);
        assert_eq!(end.termination, Termination::RulesInfraction);
        assert_eq!(end.comment.as_deref(), Some("White made an illegal move"));

        let mut game = TournamentGame::new(setup(TimeControl::Depth(1)), now);
        let other = Position::from_fen("8/8/3k4/8/8/3K4/4R3/8 w - - 0 1").expect("fen");
        let illegal = other.parse_uci("e2e5").expect("uci");
        game.reply(
            &Reply {
                mv: Some(illegal),
                ..none
            },
            now,
        );
        assert_eq!(
            game.end().map(|e| e.termination),
            Some(Termination::RulesInfraction)
        );
    }

    #[test]
    fn mate_ends_the_game_with_the_win() {
        let now = Instant::now();
        let mut game = TournamentGame::new(setup(TimeControl::Depth(1)), now);
        play_all(&mut game, &["f2f3", "e7e5", "g2g4", "d8h4"], now);
        let end = game.end().expect("ended");
        assert_eq!(end.result, GameResult::Black);
        assert_eq!(end.termination, Termination::Normal);
        assert!(matches!(game.advance(now), Step::Over(_)));
    }

    #[test]
    fn stalemate_and_insufficient_material_have_their_comments() {
        let now = Instant::now();
        let mut s = setup(TimeControl::Depth(1));
        s.opening = Opening {
            name: None,
            start: Position::from_fen("7k/5K2/8/6Q1/8/8/8/8 w - - 0 1").expect("fen"),
            moves: Vec::new(),
        };
        let mut game = TournamentGame::new(s, now);
        play_all(&mut game, &["g5g6"], now);
        let end = game.end().expect("ended");
        assert_eq!(end.result, GameResult::Draw);
        assert_eq!(end.comment.as_deref(), Some("Stalemate"));

        let mut s = setup(TimeControl::Depth(1));
        s.opening = Opening {
            name: None,
            start: Position::from_fen("4k3/8/8/8/8/1n6/8/R3K3 b - - 0 1").expect("fen"),
            moves: Vec::new(),
        };
        let mut game = TournamentGame::new(s, now);
        play_all(&mut game, &["b3a1"], now);
        let end = game.end().expect("ended");
        assert_eq!(end.comment.as_deref(), Some("Insufficient material"));
    }

    #[test]
    fn threefold_repetition_and_fifty_moves_obey_claim_draws() {
        let now = Instant::now();
        let shuffle = [
            "g1f3", "g8f6", "f3g1", "f6g8", "g1f3", "g8f6", "f3g1", "f6g8",
        ];
        let mut game = TournamentGame::new(setup(TimeControl::Depth(1)), now);
        play_all(&mut game, &shuffle, now);
        let end = game.end().expect("ended");
        assert_eq!(end.result, GameResult::Draw);
        assert_eq!(end.comment.as_deref(), Some("3 fold repetition"));

        let mut s = setup(TimeControl::Depth(1));
        s.claim_draws = false;
        let mut game = TournamentGame::new(s, now);
        play_all(&mut game, &shuffle, now);
        assert!(game.end().is_none(), "repetition is not claimed");

        let mut s = setup(TimeControl::Depth(1));
        s.opening = Opening {
            name: None,
            start: Position::from_fen("3k4/8/8/8/8/8/4R3/4K3 w - - 99 80").expect("fen"),
            moves: Vec::new(),
        };
        let mut game = TournamentGame::new(s, now);
        play_all(&mut game, &["e2e3"], now);
        let end = game.end().expect("ended");
        assert_eq!(end.comment.as_deref(), Some("50 move rule"));
    }

    #[test]
    fn book_moves_are_free_and_the_last_one_is_commented() {
        let now = Instant::now();
        let mut s = setup(clock(10_000, 1_000));
        s.book = Some(book_with_two_moves());
        let mut game = TournamentGame::new(s, now);
        let Step::Think(turn) = game.advance(now) else {
            panic!("search")
        };
        assert_eq!(turn.think.moves.len(), 2, "e4 e5 from the book");
        assert_eq!(turn.think.side, Color::White);
        let p = game.played();
        assert!(p[0].free && p[1].free);
        assert_eq!(p[1].note.as_deref(), Some("Last book move (book)"));
        assert_eq!(p[0].note, None);
        // No time and no increment were charged.
        let ThinkLimits::Clock(c) = turn.think.limits else {
            panic!("clock")
        };
        assert_eq!(c.white_time, Duration::from_secs(10));
        assert_eq!(c.black_time, Duration::from_secs(10));
        assert_eq!(game.book_line().len(), 2);
    }

    #[test]
    fn the_book_limit_stops_the_book() {
        let now = Instant::now();
        let mut s = setup(TimeControl::Depth(1));
        s.book = Some(book_with_two_moves());
        s.book_ply_limit = Some(1);
        let mut game = TournamentGame::new(s, now);
        let Step::Think(turn) = game.advance(now) else {
            panic!("search")
        };
        assert_eq!(turn.think.moves.len(), 1);
        assert_eq!(
            game.played()[0].note.as_deref(),
            Some("Last book move (book)")
        );
    }

    #[test]
    fn a_replayed_book_line_keeps_its_comment() {
        let now = Instant::now();
        let start = Position::new();
        let e4 = start.parse_uci("e2e4").expect("uci");
        let mut s = setup(TimeControl::Depth(1));
        s.opening = Opening {
            name: None,
            start,
            moves: vec![e4],
        };
        s.opening_is_book = true;
        let game = TournamentGame::new(s, now);
        assert!(game.played()[0].free);
        assert!(game.played()[0].note.is_some());
    }

    #[test]
    fn pre_played_opening_moves_cost_nothing() {
        let now = Instant::now();
        let start = Position::new();
        let e4 = start.parse_uci("e2e4").expect("uci");
        let mut s = setup(clock(5_000, 1_000));
        s.opening = Opening {
            name: Some("King pawn".to_owned()),
            start,
            moves: vec![e4],
        };
        let mut game = TournamentGame::new(s, now);
        let Step::Think(turn) = game.advance(now) else {
            panic!("search")
        };
        assert_eq!(turn.think.side, Color::Black);
        let ThinkLimits::Clock(c) = turn.think.limits else {
            panic!("clock")
        };
        assert_eq!(c.white_time, Duration::from_secs(5));
    }

    #[test]
    fn automatic_adjudication_ends_the_game_with_its_reason() {
        let now = Instant::now();
        let mut s = setup(TimeControl::Depth(1));
        s.adjudication.max_plies = Some(4);
        let mut game = TournamentGame::new(s, now);
        play_all(&mut game, &["g1f3", "g8f6", "f3g1", "f6g8"], now);
        let end = game.end().expect("ended");
        assert_eq!(end.termination, Termination::Adjudication);
        assert_eq!(end.result, GameResult::Draw);
        assert_eq!(end.comment.as_deref(), Some("Adjudicated: move limit"));
    }

    #[test]
    fn manual_adjudication_and_forfeits() {
        let now = Instant::now();
        let mut game = TournamentGame::new(setup(TimeControl::Depth(1)), now);
        game.adjudicate(GameResult::White);
        let end = game.end().cloned().expect("ended");
        assert_eq!(end.termination, Termination::Adjudication);
        assert_eq!(end.comment.as_deref(), Some("Manual adjudication"));
        // The first end stands.
        game.forfeit(Color::White, Termination::Abandoned);
        assert_eq!(game.end(), Some(&end));

        let mut game = TournamentGame::new(setup(TimeControl::Depth(1)), now);
        game.forfeit(Color::Black, Termination::Abandoned);
        let end = game.end().expect("ended");
        assert_eq!(end.result, GameResult::White);
        assert_eq!(end.comment.as_deref(), Some("Black engine failed"));

        let mut game = TournamentGame::new(setup(TimeControl::Depth(1)), now);
        game.abort();
        assert_eq!(game.end().map(|e| e.result), Some(GameResult::Unknown));
    }

    #[test]
    fn the_ponder_plan_is_for_the_engine_that_just_moved() {
        let now = Instant::now();
        let mut s = setup(TimeControl::Depth(1));
        s.ponder = [true, true];
        let mut game = TournamentGame::new(s, now);
        let Step::Think(first) = game.advance(now) else {
            panic!("search")
        };
        assert!(first.ponder.is_none(), "nothing to ponder on yet");
        let e4 = mv(game.position(), "e2e4");
        let after = game.position().after(e4).expect("after");
        let e5 = after.parse_uci("e7e5").expect("uci");
        let mut r = reply(&game, "e2e4", 10);
        r.ponder = Some(e5);
        game.reply(&r, now);
        let Step::Think(turn) = game.advance(now) else {
            panic!("search")
        };
        assert_eq!(turn.think.side, Color::Black);
        let plan = turn.ponder.expect("white ponders");
        assert_eq!(plan.side, Color::White);
        assert_eq!(plan.expected, e5);
        assert_eq!(plan.think.moves.len(), 2, "e4 and the expected e5");
    }

    #[test]
    fn ponder_needs_the_option_of_the_side() {
        let now = Instant::now();
        let mut s = setup(TimeControl::Depth(1));
        s.ponder = [false, true];
        let mut game = TournamentGame::new(s, now);
        let Step::Think(_) = game.advance(now) else {
            panic!("search")
        };
        let e4 = mv(game.position(), "e2e4");
        let e5 = game
            .position()
            .after(e4)
            .expect("after")
            .parse_uci("e7e5")
            .expect("uci");
        let mut r = reply(&game, "e2e4", 10);
        r.ponder = Some(e5);
        game.reply(&r, now);
        let Step::Think(turn) = game.advance(now) else {
            panic!("search")
        };
        assert!(turn.ponder.is_none(), "white may not ponder");
    }

    #[test]
    fn termination_tags_round_trip() {
        for t in [
            Termination::Normal,
            Termination::TimeForfeit,
            Termination::RulesInfraction,
            Termination::Abandoned,
            Termination::Adjudication,
            Termination::Unterminated,
        ] {
            assert_eq!(Termination::from_tag(t.tag()), Some(t));
        }
        assert_eq!(Termination::from_tag("x"), None);
    }

    #[test]
    fn white_point_of_view_scores_are_converted_to_the_mover_once() {
        // Regression (D1302): the engine layer reports from White's point of
        // view, so `Cp(-700)` on a Black move is Black winning.
        let now = Instant::now();
        let mut s = setup(TimeControl::Depth(5));
        s.adjudication.resign = Some(crate::tournament::config::ResignRule {
            centipawns: 600,
            moves: 2,
            require_both: false,
        });
        let mut game = TournamentGame::new(s, now);
        let line = [
            "e2e4", "e7e5", "g1f3", "b8c6", "f1c4", "g8f6", "d2d3", "f8c5",
        ];
        for uci in line {
            let Step::Think(_) = game.advance(now) else {
                break;
            };
            let mut r = reply(&game, uci, 10);
            // White is losing by 7 pawns on every move, from its own view.
            r.score = Some(EngineScore::Cp(-700));
            game.reply(&r, now);
        }
        let end = game.end().expect("adjudicated");
        assert_eq!(end.result, GameResult::Black, "White is the one resigning");
        assert_eq!(
            game.played()[1].score.map(|n| n.score),
            Some(EngineScore::Cp(700)),
            "Black's own view"
        );
        assert_eq!(
            game.played()[0].score.map(|n| n.score),
            Some(EngineScore::Cp(-700))
        );
    }
}
