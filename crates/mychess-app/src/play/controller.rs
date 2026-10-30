//! The state machine of a game against an opponent (spec M8-1, D1220,
//! D1225-D1229, D1234-D1236, D1238-D1240, D1244).
//!
//! The controller owns the game, the clock and the book. It is **pure**: it
//! reads no clock (every call that depends on time receives an [`Instant`]),
//! does no I/O and does not know engines. Every call returns the
//! [`PlayOutput`]s the owner must carry out, in order: ask the opponent to
//! think, stop it, redraw the board, ask the player a question, show the
//! end. The glue of `AppState` (`state::play_flow`) turns them into engine
//! commands and notices; a tournament or an online game can reuse the
//! controller with another opponent (D1244).

use std::time::{Duration, Instant};

use mychess_core::{Color, Game, GameResult, Move, NodeId, PartialDate, Position};
use mychess_engine::Score;
use thiserror::Error;

use super::book::PlayBook;
use super::clock::PlayClock;
use super::config::{PlayConfig, StartFrom};
use super::opponent::{OpponentInfo, Think, limits_for};
use super::record::{TagSetup, write_result, write_start_tags, write_tactical_tags};
use super::rules::{PlayEnd, position_end, repetition_key, timeout_end};
use crate::annotate::{BookVerdict, classify, last_book_move_comment, out_of_book_comment};
use crate::tournament::clock_text;

/// How many searches of the engine (not book moves) must have an
/// evaluation at or below [`DRAW_ACCEPT_CP`] for it to accept a draw
/// (M8-1c, D1504).
pub(crate) const DRAW_WINDOW: usize = 3;

/// The evaluation, in centipawns from the engine's point of view, at or
/// below which the engine accepts a draw (D1504).
pub(crate) const DRAW_ACCEPT_CP: i32 = 10;

/// The ply the game must have reached before the engine accepts a draw
/// (20 full moves, D1504).
pub(crate) const DRAW_MIN_PLY: usize = 40;

/// Why a game could not be set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum PlayError {
    /// The specific opening has a move that is not legal.
    #[error("the opening line has an illegal move")]
    InvalidOpening,
}

/// Everything needed to start a game. The glue resolves the random choices
/// (colour, seed) and reads the date and the start position.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaySetup {
    /// The validated configuration.
    pub config: PlayConfig,
    /// The colour of the player.
    pub player_color: Color,
    /// The position to start from (ignored for a specific opening, which
    /// starts from the standard position).
    pub start: Position,
    /// The opponent.
    pub opponent: OpponentInfo,
    /// The name of the player for the tags.
    pub player_name: String,
    /// Today, for the `Date` tag.
    pub date: PartialDate,
    /// The book, when the configuration asks for one and it was read.
    pub book: Option<PlayBook>,
    /// Seed of the draws (book moves).
    pub seed: u64,
}

/// A question to the player (D1234, D1235). Answer with
/// [`PlayController::answer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Question {
    /// The move leaves the book although it still has moves: continue?
    LeaveBook {
        /// The move the player made.
        mv: Move,
    },
    /// The move leaves the specific opening line: continue?
    LeaveLine {
        /// The move the player made.
        mv: Move,
    },
}

/// Something worth telling the player that is not an end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlayRemark {
    /// The move is not legal in the position.
    IllegalMove,
    /// The player moved out of turn or the game is not running.
    NotYourTurn,
    /// The opponent is gone (D1239): the game is paused. Resume after
    /// [`PlayController::replace_opponent`], or [`PlayController::quit`].
    OpponentLost,
    /// The player declined to leave the book or the line: the move was not
    /// made.
    MoveTakenBack,
    /// There is no move of the player to take back (M8-1c, D1500).
    NothingToTakeBack,
    /// A draw cannot be offered now (M8-1c, D1503).
    DrawNotAllowed,
    /// The engine declined the draw (M8-1c, D1504).
    DrawDeclined,
}

/// A move that was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveRecord {
    /// The side that moved.
    pub mover: Color,
    /// The move.
    pub mv: Move,
    /// Its node in the game.
    pub node: NodeId,
    /// Its ply from the start of the game, from 1.
    pub ply: u32,
    /// The time it cost on the clock.
    pub spent: Duration,
    /// A book or pre-played opening move of the opponent (no time).
    pub free: bool,
}

/// What the owner of the controller must do.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PlayOutput {
    /// Ask the opponent to play (the answer is
    /// [`PlayController::opponent_moved`]).
    Think(Think),
    /// The opponent must stop thinking; whatever it answers is dropped.
    StopThinking,
    /// A move was made: the game changed.
    Moved(MoveRecord),
    /// Ask the player; the clock is stopped until the answer.
    Ask(Question),
    /// A remark.
    Notice(PlayRemark),
    /// The opponent's hash must be cleared before it thinks again (D1240,
    /// `setoption name Clear Hash`).
    ClearOpponentHash,
    /// The game is over; the result is in the game.
    Ended(PlayEnd),
    /// The last move of the player and the answer to it were taken back
    /// (M8-1c, D1501): the game in the editor must be refreshed.
    TookBack,
}

/// Whose turn it is to act.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// The player.
    Player,
    /// The opponent.
    Opponent,
}

/// Where the game is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlayPhase {
    /// The player to move.
    Player,
    /// The opponent is thinking.
    Opponent,
    /// A question waits for the player.
    Asking(Question),
    /// Paused by the user, or because the opponent is gone (`lost`).
    Paused {
        /// The opponent was lost (D1239): Resume needs a new one.
        lost: bool,
    },
    /// Over, with the reason.
    Ended(PlayEnd),
    /// Quit without a result (D1227). The state glue drops the game right
    /// after, so only direct users of the controller see this phase.
    Closed,
}

/// How a move got on the clock.
#[derive(Debug, Clone, Copy)]
enum Cost {
    /// From the instants given to the controller.
    Measured,
    /// The time was measured by someone else.
    Given(Duration),
    /// No time, no increment.
    Free,
}

/// A move of the game with what is needed to take it back (M8-1c).
#[derive(Debug, Clone, Copy)]
struct Entry {
    record: MoveRecord,
    /// Whether the book guided the game before the move.
    book_on: bool,
    /// Whether the specific line was followed before the move.
    line_alive: bool,
    /// The final evaluation of a search of the engine for this move, in
    /// centipawns from the engine's point of view.
    eval: Option<i32>,
}

/// One game against an opponent.
#[derive(Debug, Clone)]
pub struct PlayController {
    setup: PlaySetup,
    game: Game,
    tip: NodeId,
    position: Position,
    moves: Vec<Move>,
    keys: Vec<String>,
    clock: PlayClock,
    phase: PlayPhase,
    resume_to: Turn,
    /// The specific line the game follows, while every move matched it.
    line: Vec<Move>,
    line_alive: bool,
    book_on: bool,
    rng: u64,
    /// Every move made, with the state to take it back.
    history: Vec<Entry>,
    /// The evaluation of the search that produced the move being made.
    pending_eval: Option<i32>,
    /// Times the player took a move back (tag `Takebacks`).
    takebacks: u32,
    /// The ply at which a draw was last offered (one offer per player move).
    offered_at: Option<usize>,
}

impl PlayController {
    /// Sets the game up and starts it at `now`.
    ///
    /// # Errors
    ///
    /// [`PlayError::InvalidOpening`] when the specific opening has an
    /// illegal move.
    pub fn new(
        setup: PlaySetup,
        now: Instant,
    ) -> Result<(PlayController, Vec<PlayOutput>), PlayError> {
        let (start, line, opening) = match &setup.config.start {
            StartFrom::Opening(opening) => (
                Position::new(),
                opening.to_moves().ok_or(PlayError::InvalidOpening)?,
                Some(opening.name.clone()),
            ),
            _ => (setup.start.clone(), Vec::new(), None),
        };
        let mut game = Game::from_position(start.clone());
        write_start_tags(
            &mut game,
            &TagSetup {
                opponent: setup.opponent.name.clone(),
                player: setup.player_name.clone(),
                player_color: setup.player_color,
                mode: setup.config.mode,
                strength: setup.config.strength,
                opening,
                date: setup.date,
            },
        );
        if let Some(tactical) = &setup.config.tactical {
            write_tactical_tags(&mut game, tactical, !setup.player_color);
        }
        let tip = game.root();
        let mut clock = setup.config.mode.clock();
        clock.start(start.turn(), now);
        let book_on = setup.book.is_some();
        let rng = setup.seed | 1;
        let line_alive = !line.is_empty();
        let mut controller = PlayController {
            keys: vec![repetition_key(&start)],
            position: start,
            game,
            tip,
            moves: Vec::new(),
            clock,
            phase: PlayPhase::Player,
            resume_to: Turn::Player,
            line,
            line_alive,
            book_on,
            rng,
            history: Vec::new(),
            pending_eval: None,
            takebacks: 0,
            offered_at: None,
            setup,
        };
        let mut out = Vec::new();
        if let Some(end) = position_end(&controller.position, &controller.keys) {
            controller.finish(end, now, &mut out);
        } else {
            controller.begin_turn(now, &mut out);
        }
        Ok((controller, out))
    }

    // ---- reading ----

    /// The setup the game started with (for Restart, D1227).
    pub fn setup(&self) -> &PlaySetup {
        &self.setup
    }

    /// The game so far, with tags, comments and (once over) the result.
    pub fn game(&self) -> &Game {
        &self.game
    }

    /// The last node of the game.
    pub fn tip(&self) -> NodeId {
        self.tip
    }

    /// The current position.
    pub fn position(&self) -> &Position {
        &self.position
    }

    /// Where the game is.
    pub fn phase(&self) -> PlayPhase {
        self.phase
    }

    /// The colour of the player.
    pub fn player_color(&self) -> Color {
        self.setup.player_color
    }

    /// Who the opponent is.
    pub fn opponent(&self) -> &OpponentInfo {
        &self.setup.opponent
    }

    /// The clock.
    pub fn clock(&self) -> &PlayClock {
        &self.clock
    }

    /// The time shown for `side` at `now`.
    pub fn shown(&self, side: Color, now: Instant) -> Duration {
        self.clock.shown(side, now)
    }

    /// Whether the game still takes moves or answers (not over, not
    /// closed).
    pub fn is_live(&self) -> bool {
        !matches!(self.phase, PlayPhase::Ended(_) | PlayPhase::Closed)
    }

    /// The end, once the game is over.
    pub fn end(&self) -> Option<PlayEnd> {
        match self.phase {
            PlayPhase::Ended(end) => Some(end),
            _ => None,
        }
    }

    /// The result of the game (`*` while it runs).
    pub fn result(&self) -> GameResult {
        self.game.tags().result
    }

    /// Whether Take back works now: it is the player's turn and the player
    /// has made a move (D1500).
    pub fn can_take_back(&self) -> bool {
        self.take_back_from().is_some()
    }

    /// Whether the player may offer a draw now: it is the player's turn and
    /// no offer was made since the last move of the player (D1503).
    pub fn can_offer_draw(&self) -> bool {
        self.phase == PlayPhase::Player && self.offered_at != Some(self.moves.len())
    }

    /// How many times the player took a move back.
    pub fn takebacks(&self) -> u32 {
        self.takebacks
    }

    /// Whether the book still guides the game.
    pub fn book_active(&self) -> bool {
        self.book_on
    }

    /// The moves of the game so far, from its start position.
    pub fn played_moves(&self) -> &[Move] {
        &self.moves
    }

    /// The opponent's request for the position after the move it expects
    /// (`predicted`, the `ponder` move of its `bestmove`), to think on the
    /// player's time (M8-1d, D2403). Only while it is the player's turn
    /// with nothing pending; `None` when `predicted` is not legal here or
    /// leaves the opponent without a move.
    pub fn ponder_think(&self, predicted: Move, now: Instant) -> Option<Think> {
        if self.phase != PlayPhase::Player || !self.playable(predicted) {
            return None;
        }
        let after = self.position.after(predicted).ok()?;
        if after.legal_moves().is_empty() {
            return None;
        }
        let mut moves = self.moves.clone();
        moves.push(predicted);
        Some(Think {
            start: self.game.start_position().clone(),
            moves,
            side: after.turn(),
            limits: limits_for(self.setup.config.mode, self.clock.search_clock(now)),
        })
    }

    /// Writes an extra tag in the game (the coach marks it, D2426). A value
    /// the tags refuse is dropped.
    pub fn mark_game(&mut self, name: &str, value: &str) {
        self.game.tags_mut().set_extra(name, value).ok();
    }

    // ---- the player acts ----

    /// The player made `mv` at `now`.
    pub fn player_move(&mut self, mv: Move, now: Instant) -> Vec<PlayOutput> {
        let mut out = Vec::new();
        if self.phase != PlayPhase::Player {
            out.push(PlayOutput::Notice(PlayRemark::NotYourTurn));
            return out;
        }
        if !self.playable(mv) {
            out.push(PlayOutput::Notice(PlayRemark::IllegalMove));
            return out;
        }
        if let Some(question) = self.leaves_guide(mv) {
            self.clock.stop(now);
            self.phase = PlayPhase::Asking(question);
            out.push(PlayOutput::Ask(question));
            return out;
        }
        self.commit(mv, Cost::Measured, now, &mut out);
        out
    }

    /// The answer to the question of [`PlayOutput::Ask`]. "Yes" makes the
    /// move; "no" takes it back and the clock goes on (D1234).
    pub fn answer(&mut self, yes: bool, now: Instant) -> Vec<PlayOutput> {
        let mut out = Vec::new();
        let PlayPhase::Asking(question) = self.phase else {
            return out;
        };
        self.clock.resume(now);
        self.phase = PlayPhase::Player;
        if yes {
            let (Question::LeaveBook { mv } | Question::LeaveLine { mv }) = question;
            self.commit(mv, Cost::Measured, now, &mut out);
        } else {
            out.push(PlayOutput::Notice(PlayRemark::MoveTakenBack));
        }
        out
    }

    /// Takes back the last move of the player and what the opponent
    /// answered (D1500-D1502). The game is cut at the node before them (no
    /// variation is left), the clock gives back the time and takes the
    /// increments away, and the player is to move with the clock running
    /// from `now`. Outside the player's turn, or without a move of the
    /// player, only [`PlayRemark::NothingToTakeBack`] is returned.
    pub fn take_back(&mut self, now: Instant) -> Vec<PlayOutput> {
        let mut out = Vec::new();
        let Some(first) = self.take_back_from() else {
            out.push(PlayOutput::Notice(PlayRemark::NothingToTakeBack));
            return out;
        };
        // A flag that fell since the last tick counts first: taking back
        // would give the time back and let the player escape the loss.
        if let Some(loser) = self.clock.flagged(now) {
            let end = timeout_end(&self.position, loser);
            self.finish(end, now, &mut out);
            return out;
        }
        let entry = self.history[first];
        let Ok(Some(parent)) = self.game.parent(entry.record.node) else {
            out.push(PlayOutput::Notice(PlayRemark::NothingToTakeBack));
            return out;
        };
        if self.game.truncate_from(entry.record.node).is_err() {
            out.push(PlayOutput::Notice(PlayRemark::NothingToTakeBack));
            return out;
        }
        let count = self.history.len() - first;
        self.clock.undo(count, now);
        self.history.truncate(first);
        self.moves.truncate(first);
        self.keys.truncate(first + 1);
        self.tip = parent;
        let mut position = self.game.start_position().clone();
        for mv in &self.moves {
            if position.play(*mv).is_err() {
                break;
            }
        }
        self.position = position;
        self.book_on = entry.book_on;
        self.line_alive = entry.line_alive;
        self.offered_at = None;
        self.takebacks = self.takebacks.saturating_add(1);
        // A value the tags refuse is dropped: the game goes on.
        self.game
            .tags_mut()
            .set_extra("Takebacks", &self.takebacks.to_string())
            .ok();
        self.phase = PlayPhase::Player;
        out.push(PlayOutput::TookBack);
        out
    }

    /// The player offers a draw (D1503-D1505). The engine accepts only when
    /// its last `DRAW_WINDOW` searches (book moves do not count) all
    /// evaluated the game at `DRAW_ACCEPT_CP` or less for it and the game
    /// reached the ply `DRAW_MIN_PLY`. A mate against the engine counts
    /// as a very low evaluation; a mate for it, as a very high one.
    pub fn offer_draw(&mut self, now: Instant) -> Vec<PlayOutput> {
        let mut out = Vec::new();
        if !self.can_offer_draw() {
            out.push(PlayOutput::Notice(PlayRemark::DrawNotAllowed));
            return out;
        }
        self.offered_at = Some(self.moves.len());
        if self.engine_accepts_draw() {
            self.finish(PlayEnd::DrawAgreed, now, &mut out);
        } else {
            out.push(PlayOutput::Notice(PlayRemark::DrawDeclined));
        }
        out
    }

    /// The player resigns (D1227): the opponent wins and the game ends.
    pub fn resign(&mut self, now: Instant) -> Vec<PlayOutput> {
        let mut out = Vec::new();
        if !self.is_live() {
            return out;
        }
        let end = PlayEnd::Resignation {
            loser: self.setup.player_color,
        };
        self.finish(end, now, &mut out);
        out
    }

    /// Leaves the game without a result and without saving (D1227).
    pub fn quit(&mut self, now: Instant) -> Vec<PlayOutput> {
        let mut out = Vec::new();
        if !self.is_live() {
            return out;
        }
        if self.thinking() {
            out.push(PlayOutput::StopThinking);
        }
        self.clock.stop(now);
        self.phase = PlayPhase::Closed;
        out
    }

    /// Pauses the game (D1240): the clock stops and the opponent is told to
    /// stop thinking.
    pub fn pause(&mut self, now: Instant) -> Vec<PlayOutput> {
        let mut out = Vec::new();
        let turn = match self.phase {
            PlayPhase::Player => Turn::Player,
            PlayPhase::Opponent => Turn::Opponent,
            _ => return out,
        };
        if turn == Turn::Opponent {
            out.push(PlayOutput::StopThinking);
        }
        self.clock.stop(now);
        self.resume_to = turn;
        self.phase = PlayPhase::Paused { lost: false };
        out
    }

    /// Resumes a paused game: the clock of the side to move restarts and,
    /// if the opponent was to move, its hash is cleared and it thinks again
    /// (D1240). A game paused because the opponent is gone does not resume
    /// until [`PlayController::replace_opponent`].
    pub fn resume(&mut self, now: Instant) -> Vec<PlayOutput> {
        let mut out = Vec::new();
        match self.phase {
            PlayPhase::Paused { lost: false } => {}
            PlayPhase::Paused { lost: true } => {
                out.push(PlayOutput::Notice(PlayRemark::OpponentLost));
                return out;
            }
            _ => return out,
        }
        self.clock.resume(now);
        // The Scid clears the hash on every Resume (`sergame.tcl` 446), not
        // only when the engine is the one to move (D1240).
        out.push(PlayOutput::ClearOpponentHash);
        match self.resume_to {
            Turn::Player => self.phase = PlayPhase::Player,
            Turn::Opponent => self.ask_opponent(now, &mut out),
        }
        out
    }

    /// The time passed: the flag of a countdown may have fallen (D1226).
    pub fn tick(&mut self, now: Instant) -> Vec<PlayOutput> {
        let mut out = Vec::new();
        if !matches!(self.phase, PlayPhase::Player | PlayPhase::Opponent) {
            return out;
        }
        if let Some(loser) = self.clock.flagged(now) {
            let end = timeout_end(&self.position, loser);
            self.finish(end, now, &mut out);
        }
        out
    }

    // ---- the opponent acts ----

    /// The opponent answered with `mv` (`None`: no move). `spent` is the
    /// time it used when someone else measured it (D1266); `None` reads the
    /// instants. An answer that is not expected (the game is paused, over,
    /// or it is not the opponent's turn) is dropped. A move that is
    /// missing or illegal ends the game without a result (D1238).
    ///
    /// `score` is the final evaluation of the search that chose the move
    /// (`None` for an opponent that reports none). Like every [`Score`] it
    /// is **from White's point of view**; the controller turns it into the
    /// opponent's point of view for the draw offers (D1504).
    pub fn opponent_moved(
        &mut self,
        mv: Option<Move>,
        spent: Option<Duration>,
        now: Instant,
        score: Option<Score>,
    ) -> Vec<PlayOutput> {
        let mut out = Vec::new();
        if self.phase != PlayPhase::Opponent {
            return out;
        }
        match mv {
            Some(mv) if self.playable(mv) => {
                let cost = spent.map_or(Cost::Measured, Cost::Given);
                self.pending_eval = score.map(|s| {
                    let cp = s.as_centipawns();
                    if self.setup.player_color == Color::White {
                        cp.saturating_neg()
                    } else {
                        cp
                    }
                });
                self.commit(mv, cost, now, &mut out);
            }
            _ => self.finish(PlayEnd::OpponentFailure, now, &mut out),
        }
        out
    }

    /// The opponent died or stopped answering (D1239): the game pauses.
    pub fn opponent_lost(&mut self, now: Instant) -> Vec<PlayOutput> {
        let mut out = Vec::new();
        let turn = match self.phase {
            PlayPhase::Player => Turn::Player,
            PlayPhase::Opponent => Turn::Opponent,
            PlayPhase::Asking(_) => {
                // The question stays; the clock is already stopped.
                self.resume_to = Turn::Player;
                self.phase = PlayPhase::Paused { lost: true };
                out.push(PlayOutput::Notice(PlayRemark::OpponentLost));
                return out;
            }
            PlayPhase::Paused { .. } => {
                self.phase = PlayPhase::Paused { lost: true };
                out.push(PlayOutput::Notice(PlayRemark::OpponentLost));
                return out;
            }
            _ => return out,
        };
        self.clock.stop(now);
        self.resume_to = turn;
        self.phase = PlayPhase::Paused { lost: true };
        out.push(PlayOutput::Notice(PlayRemark::OpponentLost));
        out
    }

    /// Plays on against another opponent after [`PlayController::opponent_lost`]
    /// (D1239): the tag of the opponent changes and the game resumes.
    pub fn replace_opponent(&mut self, opponent: OpponentInfo, now: Instant) -> Vec<PlayOutput> {
        if !matches!(self.phase, PlayPhase::Paused { .. }) {
            return Vec::new();
        }
        let tags = self.game.tags_mut();
        match self.setup.player_color {
            Color::White => tags.black.clone_from(&opponent.name),
            Color::Black => tags.white.clone_from(&opponent.name),
        }
        self.setup.opponent = opponent;
        self.phase = PlayPhase::Paused { lost: false };
        self.resume(now)
    }

    // ---- internals ----

    /// A real, legal move (the null move of the game tree is not one).
    fn playable(&self, mv: Move) -> bool {
        !matches!(mv, Move::Null) && self.position.is_legal(mv)
    }

    /// The index in the history of the first move to take back: the last
    /// one of the player, when it is the player's turn.
    fn take_back_from(&self) -> Option<usize> {
        if self.phase != PlayPhase::Player {
            return None;
        }
        self.history
            .iter()
            .rposition(|e| e.record.mover == self.setup.player_color)
    }

    fn engine_accepts_draw(&self) -> bool {
        if self.moves.len() < DRAW_MIN_PLY {
            return false;
        }
        let evals: Vec<Option<i32>> = self
            .history
            .iter()
            .rev()
            .filter(|e| e.record.mover != self.setup.player_color && !e.record.free)
            .take(DRAW_WINDOW)
            .map(|e| e.eval)
            .collect();
        evals.len() == DRAW_WINDOW
            && evals
                .iter()
                .all(|e| e.is_some_and(|cp| cp <= DRAW_ACCEPT_CP))
    }

    fn thinking(&self) -> bool {
        self.phase == PlayPhase::Opponent
    }

    fn next_roll(&mut self) -> u64 {
        // xorshift64*: only the book draw uses it.
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// The move the specific line wants now, while the game follows it.
    fn line_next(&self) -> Option<Move> {
        if !self.line_alive {
            return None;
        }
        self.line.get(self.moves.len()).copied()
    }

    /// Whether the book guides this position (it has moves for it).
    fn book_guides(&self) -> bool {
        self.book_on
            && self
                .setup
                .book
                .as_ref()
                .is_some_and(|b| b.has_moves(&self.position))
    }

    /// The question for a move of the player that leaves the guide.
    fn leaves_guide(&self, mv: Move) -> Option<Question> {
        if let Some(expected) = self.line_next() {
            return (mv != expected).then_some(Question::LeaveLine { mv });
        }
        let book = self.setup.book.as_ref()?;
        if !self.book_guides() {
            return None;
        }
        let in_book = crate::annotate::BookProbe::moves(book, &self.position).contains(&mv);
        (!in_book).then_some(Question::LeaveBook { mv })
    }

    /// Makes `mv` for the side to move.
    fn commit(&mut self, mv: Move, cost: Cost, now: Instant, out: &mut Vec<PlayOutput>) {
        let mover = self.position.turn();
        let (book_before, line_before) = (self.book_on, self.line_alive);
        let eval = self.pending_eval.take();
        let timing = match cost {
            Cost::Measured => self.clock.press(now),
            Cost::Given(spent) => self.clock.press_with(spent, now),
            Cost::Free => self.clock.press_free(now),
        };
        if timing.flagged {
            let end = timeout_end(&self.position, mover);
            self.finish(end, now, out);
            return;
        }
        let before = self.position.clone();
        let line_expected = self.line_next();
        let book_guided = line_expected.is_none() && self.book_guides();
        let Ok(node) = self.game.add_move(self.tip, mv) else {
            // `mv` was checked legal; a failure here is a broken invariant,
            // reported as the failure of the opponent rather than a panic.
            self.finish(PlayEnd::OpponentFailure, now, out);
            return;
        };
        if self.position.play(mv).is_err() {
            self.finish(PlayEnd::OpponentFailure, now, out);
            return;
        }
        self.tip = node;
        self.moves.push(mv);
        self.keys.push(repetition_key(&self.position));
        if let Some(expected) = line_expected
            && expected != mv
        {
            self.line_alive = false;
        }
        if book_guided {
            self.book_comment(&before, mv, node);
        }
        let free = matches!(cost, Cost::Free);
        if self.setup.config.store_clock && !free {
            self.clock_comment(mover, node, timing.spent, now);
        }
        let ply = u32::try_from(self.moves.len()).unwrap_or(u32::MAX);
        let record = MoveRecord {
            mover,
            mv,
            node,
            ply,
            spent: timing.spent,
            free,
        };
        self.history.push(Entry {
            record,
            book_on: book_before,
            line_alive: line_before,
            eval: eval.filter(|_| !free),
        });
        out.push(PlayOutput::Moved(record));
        if let Some(end) = position_end(&self.position, &self.keys) {
            self.finish(end, now, out);
        } else {
            self.begin_turn(now, out);
        }
    }

    /// `Store clock` (D1506): `[%emt h:mm:ss]` with the time the move cost
    /// and, on a countdown, `[%clk h:mm:ss]` with what `mover` has left,
    /// appended to the comment of the node (the format of the tournaments
    /// and of the time graph of M4-7).
    fn clock_comment(&mut self, mover: Color, node: NodeId, spent: Duration, now: Instant) {
        let mut text = format!("[%emt {}]", clock_text(spent));
        if self.clock.is_countdown() {
            let left = clock_text(self.clock.shown(mover, now));
            text = format!("{text} [%clk {left}]");
        }
        self.game.append_comment(node, &text).ok();
    }

    /// The comments of the book on the move just made (D1234).
    fn book_comment(&mut self, before: &Position, mv: Move, node: NodeId) {
        let Some(book) = self.setup.book.as_ref() else {
            return;
        };
        let text = match classify(book, before, mv) {
            BookVerdict::InBook => return,
            BookVerdict::LastBookMove => last_book_move_comment(book.book_name()),
            BookVerdict::OutOfBook(moves) => out_of_book_comment(book.book_name(), &moves),
            // `BookVerdict` is exhaustive today; a new verdict leaves the
            // book without a comment.
            #[allow(unreachable_patterns)]
            _ => String::new(),
        };
        self.book_on = false;
        if !text.is_empty() {
            self.game.append_comment(node, &text).ok();
        }
    }

    /// Starts the turn of the side to move.
    fn begin_turn(&mut self, now: Instant, out: &mut Vec<PlayOutput>) {
        if self.position.turn() == self.setup.player_color {
            self.phase = PlayPhase::Player;
            return;
        }
        // The book silently stops guiding at a position it does not know
        // (the move before already carries the comment, if any).
        if self.line_next().is_none() && self.book_on && !self.book_guides() {
            self.book_on = false;
        }
        if let Some(mv) = self.line_next() {
            self.commit(mv, Cost::Free, now, out);
            return;
        }
        if self.book_on {
            let roll = self.next_roll();
            let pick = self
                .setup
                .book
                .as_ref()
                .and_then(|b| b.pick(&self.position, roll));
            if let Some(mv) = pick {
                self.commit(mv, Cost::Free, now, out);
                return;
            }
        }
        self.ask_opponent(now, out);
    }

    fn ask_opponent(&mut self, now: Instant, out: &mut Vec<PlayOutput>) {
        self.phase = PlayPhase::Opponent;
        let limits = limits_for(self.setup.config.mode, self.clock.search_clock(now));
        out.push(PlayOutput::Think(Think {
            start: self.game.start_position().clone(),
            moves: self.moves.clone(),
            side: self.position.turn(),
            limits,
        }));
    }

    /// Ends the game: stops the clock, writes the result.
    fn finish(&mut self, end: PlayEnd, now: Instant, out: &mut Vec<PlayOutput>) {
        if self.thinking() {
            out.push(PlayOutput::StopThinking);
        }
        self.clock.stop(now);
        self.phase = PlayPhase::Ended(end);
        write_result(&mut self.game, end, self.tip).ok();
        out.push(PlayOutput::Ended(end));
    }
}

#[cfg(test)]
mod tests {
    use super::super::book::tests::sample_book;
    use super::super::config::{BookSelection, OpeningLine};
    use super::*;
    use crate::engines::EngineId;
    use crate::play::config::TimeMode;
    use crate::play::opponent::ThinkLimits;

    const SEC: Duration = Duration::from_secs(1);

    fn setup(color: Color, mode: TimeMode) -> PlaySetup {
        let mut config = PlayConfig::new(EngineId(1));
        config.mode = mode;
        PlaySetup {
            config,
            player_color: color,
            start: Position::new(),
            opponent: OpponentInfo {
                name: "Fake 1".to_owned(),
                rating: None,
            },
            player_name: "Ana".to_owned(),
            date: PartialDate::UNKNOWN,
            book: None,
            seed: 7,
        }
    }

    fn uci(c: &PlayController, text: &str) -> Move {
        c.position().parse_uci(text).expect("uci")
    }

    fn thinks(out: &[PlayOutput]) -> Vec<Think> {
        out.iter()
            .filter_map(|o| match o {
                PlayOutput::Think(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }

    fn ended(out: &[PlayOutput]) -> Option<PlayEnd> {
        out.iter().find_map(|o| match o {
            PlayOutput::Ended(e) => Some(*e),
            _ => None,
        })
    }

    #[test]
    fn white_player_waits_and_black_player_makes_the_engine_think() {
        let t0 = Instant::now();
        let (c, out) =
            PlayController::new(setup(Color::White, TimeMode::Depth(3)), t0).expect("new");
        assert!(out.is_empty());
        assert_eq!(c.phase(), PlayPhase::Player);

        let (c, out) =
            PlayController::new(setup(Color::Black, TimeMode::Depth(3)), t0).expect("new");
        assert_eq!(c.phase(), PlayPhase::Opponent);
        let t = thinks(&out);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].side, Color::White);
        assert_eq!(t[0].limits, ThinkLimits::Depth(3));
        assert!(t[0].moves.is_empty());
    }

    #[test]
    fn a_round_trip_updates_game_clock_and_request() {
        let t0 = Instant::now();
        let mode = TimeMode::TimeBonus {
            white_minutes: 5,
            white_bonus: 3,
            black_minutes: 5,
            black_bonus: 3,
        };
        let (mut c, _) = PlayController::new(setup(Color::White, mode), t0).expect("new");
        let e4 = uci(&c, "e2e4");
        let out = c.player_move(e4, t0 + 4 * SEC);
        assert!(matches!(out[0], PlayOutput::Moved(m) if m.ply == 1 && m.spent == 4 * SEC));
        let t = thinks(&out);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].moves, [e4]);
        match t[0].limits {
            ThinkLimits::Clock(clock) => {
                // 300 - 4 + 3 for White; Black has not used anything yet.
                assert_eq!(clock.white_time, 299 * SEC);
                assert_eq!(clock.black_time, 300 * SEC);
                assert_eq!(clock.white_increment, 3 * SEC);
            }
            other => panic!("expected a clock, got {other:?}"),
        }
        let e5 = uci(&c, "e7e5");
        let out = c.opponent_moved(Some(e5), None, t0 + 9 * SEC, None);
        assert!(matches!(out[0], PlayOutput::Moved(m) if m.spent == 5 * SEC));
        assert_eq!(c.phase(), PlayPhase::Player);
        assert_eq!(c.game().mainline().count(), 2);
    }

    #[test]
    fn an_illegal_move_or_a_move_out_of_turn_is_refused() {
        let t0 = Instant::now();
        let (mut c, _) =
            PlayController::new(setup(Color::White, TimeMode::Depth(1)), t0).expect("new");
        let bad = Move::Null;
        let out = c.player_move(bad, t0);
        assert_eq!(out, [PlayOutput::Notice(PlayRemark::IllegalMove)]);
        let e4 = uci(&c, "e2e4");
        c.player_move(e4, t0);
        let d4 = Position::new().parse_uci("d2d4").expect("uci");
        let out = c.player_move(d4, t0);
        assert_eq!(out, [PlayOutput::Notice(PlayRemark::NotYourTurn)]);
    }

    #[test]
    fn checkmate_ends_the_game_with_the_result() {
        let t0 = Instant::now();
        let (mut c, _) =
            PlayController::new(setup(Color::White, TimeMode::Depth(1)), t0).expect("new");
        for (text, by_player) in [("f2f3", true), ("e7e5", false), ("g2g4", true)] {
            let m = uci(&c, text);
            if by_player {
                c.player_move(m, t0);
            } else {
                c.opponent_moved(Some(m), None, t0, None);
            }
        }
        let mate = uci(&c, "d8h4");
        let out = c.opponent_moved(Some(mate), None, t0, None);
        assert_eq!(
            ended(&out),
            Some(PlayEnd::Checkmate {
                winner: Color::Black
            })
        );
        assert_eq!(c.result(), GameResult::Black);
        assert!(!c.is_live());
        // The clock stopped and nothing else is taken.
        assert!(c.tick(t0 + 999 * SEC).is_empty());
    }

    #[test]
    fn threefold_repetition_draws_and_comments_the_last_move() {
        let t0 = Instant::now();
        let (mut c, _) =
            PlayController::new(setup(Color::White, TimeMode::Depth(1)), t0).expect("new");
        let cycle = [
            ("g1f3", true),
            ("g8f6", false),
            ("f3g1", true),
            ("f6g8", false),
        ];
        let mut last = Vec::new();
        for _ in 0..2 {
            for (text, by_player) in cycle {
                let m = uci(&c, text);
                last = if by_player {
                    c.player_move(m, t0)
                } else {
                    c.opponent_moved(Some(m), None, t0, None)
                };
            }
        }
        assert_eq!(ended(&last), Some(PlayEnd::ThreefoldRepetition));
        assert_eq!(c.result(), GameResult::Draw);
        let comment = c.game().comment(c.tip()).expect("node");
        assert_eq!(comment, Some("3 fold repetition"));
    }

    #[test]
    fn the_fifty_move_rule_draws() {
        let t0 = Instant::now();
        let mut s = setup(Color::White, TimeMode::Depth(1));
        s.start = Position::from_fen("3k4/8/8/8/8/8/4R3/4K3 w - - 98 80").expect("fen");
        let (mut c, _) = PlayController::new(s, t0).expect("new");
        let m = uci(&c, "e2e3");
        c.player_move(m, t0);
        let m = uci(&c, "d8c8");
        let out = c.opponent_moved(Some(m), None, t0, None);
        assert_eq!(ended(&out), Some(PlayEnd::FiftyMoves));
        assert_eq!(
            c.game().comment(c.tip()).expect("node"),
            Some("50 move rule")
        );
    }

    #[test]
    fn insufficient_material_draws_after_the_capture() {
        let t0 = Instant::now();
        let mut s = setup(Color::White, TimeMode::Depth(1));
        s.start = Position::from_fen("4k3/8/8/8/8/8/4r3/4K3 w - - 0 1").expect("fen");
        let (mut c, _) = PlayController::new(s, t0).expect("new");
        // Kxe2 leaves bare kings.
        let m = uci(&c, "e1e2");
        let out = c.player_move(m, t0);
        assert_eq!(ended(&out), Some(PlayEnd::InsufficientMaterial));
    }

    #[test]
    fn stalemate_draws() {
        let t0 = Instant::now();
        let mut s = setup(Color::White, TimeMode::Depth(1));
        s.start = Position::from_fen("7k/8/5KQ1/8/8/8/8/8 w - - 0 1").expect("fen");
        let (mut c, _) = PlayController::new(s, t0).expect("new");
        let m = uci(&c, "g6f7");
        let out = c.player_move(m, t0);
        assert_eq!(ended(&out), Some(PlayEnd::Stalemate));
        assert_eq!(c.result(), GameResult::Draw);
    }

    #[test]
    fn the_flag_of_the_player_loses_but_a_bare_opponent_draws() {
        let t0 = Instant::now();
        let mode = TimeMode::TimeBonus {
            white_minutes: 1,
            white_bonus: 0,
            black_minutes: 1,
            black_bonus: 0,
        };
        let (mut c, _) = PlayController::new(setup(Color::White, mode), t0).expect("new");
        assert!(c.tick(t0 + 59 * SEC).is_empty());
        let out = c.tick(t0 + 60 * SEC);
        assert_eq!(
            ended(&out),
            Some(PlayEnd::Timeout {
                loser: Color::White
            })
        );
        assert_eq!(c.result(), GameResult::Black);

        let mut s = setup(Color::White, mode);
        s.start = Position::from_fen("4k3/8/8/8/8/8/4n3/4K1Q1 w - - 0 1").expect("fen");
        let (mut c, _) = PlayController::new(s, t0).expect("new");
        // Black has a lone knight against a queen: it cannot mate, even
        // with help (the rule of shakmaty), so White's flag is a draw.
        let out = c.tick(t0 + 61 * SEC);
        assert_eq!(ended(&out), Some(PlayEnd::TimeoutDraw));
        assert_eq!(c.result(), GameResult::Draw);
    }

    #[test]
    fn a_move_that_arrives_after_the_flag_does_not_count() {
        let t0 = Instant::now();
        let mode = TimeMode::TimeBonus {
            white_minutes: 1,
            white_bonus: 0,
            black_minutes: 1,
            black_bonus: 0,
        };
        let (mut c, _) = PlayController::new(setup(Color::White, mode), t0).expect("new");
        let e4 = uci(&c, "e2e4");
        let out = c.player_move(e4, t0 + 70 * SEC);
        assert!(matches!(ended(&out), Some(PlayEnd::Timeout { .. })));
        assert_eq!(c.game().mainline().count(), 0);
    }

    #[test]
    fn the_flag_of_the_engine_stops_its_search() {
        let t0 = Instant::now();
        let mode = TimeMode::TimeBonus {
            white_minutes: 1,
            white_bonus: 0,
            black_minutes: 1,
            black_bonus: 0,
        };
        let (mut c, _) = PlayController::new(setup(Color::White, mode), t0).expect("new");
        let e4 = uci(&c, "e2e4");
        c.player_move(e4, t0 + SEC);
        let out = c.tick(t0 + 62 * SEC);
        assert!(out.contains(&PlayOutput::StopThinking));
        assert_eq!(
            ended(&out),
            Some(PlayEnd::Timeout {
                loser: Color::Black
            })
        );
        assert_eq!(c.result(), GameResult::White);
    }

    #[test]
    fn a_missing_or_illegal_engine_move_ends_without_a_result() {
        let t0 = Instant::now();
        let (mut c, _) =
            PlayController::new(setup(Color::Black, TimeMode::Depth(1)), t0).expect("new");
        let out = c.opponent_moved(None, None, t0, None);
        assert_eq!(ended(&out), Some(PlayEnd::OpponentFailure));
        assert_eq!(c.result(), GameResult::Unknown);

        let (mut c, _) =
            PlayController::new(setup(Color::Black, TimeMode::Depth(1)), t0).expect("new");
        // A black move on White's turn: illegal for the position.
        let wrong =
            Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1")
                .expect("fen")
                .parse_uci("e7e5")
                .expect("uci");
        let out = c.opponent_moved(Some(wrong), None, t0, None);
        assert_eq!(ended(&out), Some(PlayEnd::OpponentFailure));
    }

    #[test]
    fn an_answer_when_not_expected_is_dropped() {
        let t0 = Instant::now();
        let (mut c, _) =
            PlayController::new(setup(Color::White, TimeMode::Depth(1)), t0).expect("new");
        let e4 = uci(&c, "e2e4");
        assert!(c.opponent_moved(Some(e4), None, t0, None).is_empty());
    }

    #[test]
    fn resign_records_a_loss_and_quit_records_nothing() {
        let t0 = Instant::now();
        let (mut c, _) =
            PlayController::new(setup(Color::Black, TimeMode::Depth(1)), t0).expect("new");
        let out = c.resign(t0);
        assert!(out.contains(&PlayOutput::StopThinking));
        assert_eq!(
            ended(&out),
            Some(PlayEnd::Resignation {
                loser: Color::Black
            })
        );
        assert_eq!(c.result(), GameResult::White);

        let (mut c, _) =
            PlayController::new(setup(Color::Black, TimeMode::Depth(1)), t0).expect("new");
        let out = c.quit(t0);
        assert_eq!(out, [PlayOutput::StopThinking]);
        assert_eq!(c.phase(), PlayPhase::Closed);
        assert_eq!(c.result(), GameResult::Unknown);
        assert!(c.resign(t0).is_empty());
    }

    #[test]
    fn pause_stops_the_clock_and_resume_clears_the_hash_and_thinks_again() {
        let t0 = Instant::now();
        let mode = TimeMode::TimeBonus {
            white_minutes: 5,
            white_bonus: 0,
            black_minutes: 5,
            black_bonus: 0,
        };
        let (mut c, _) = PlayController::new(setup(Color::Black, mode), t0).expect("new");
        let out = c.pause(t0 + 10 * SEC);
        assert_eq!(out, [PlayOutput::StopThinking]);
        assert_eq!(c.phase(), PlayPhase::Paused { lost: false });
        assert_eq!(c.shown(Color::White, t0 + 500 * SEC), 290 * SEC);
        // A stale answer while paused is dropped.
        let e4 = uci(&c, "e2e4");
        assert!(
            c.opponent_moved(Some(e4), None, t0 + 20 * SEC, None)
                .is_empty()
        );
        let out = c.resume(t0 + 500 * SEC);
        assert_eq!(out[0], PlayOutput::ClearOpponentHash);
        assert_eq!(thinks(&out).len(), 1);
        assert_eq!(c.shown(Color::White, t0 + 503 * SEC), 287 * SEC);
        assert_eq!(c.phase(), PlayPhase::Opponent);
    }

    #[test]
    fn pausing_on_the_players_turn_resumes_on_the_players_turn() {
        let t0 = Instant::now();
        let (mut c, _) =
            PlayController::new(setup(Color::White, TimeMode::Depth(1)), t0).expect("new");
        c.pause(t0);
        let e4 = uci(&c, "e2e4");
        assert_eq!(
            c.player_move(e4, t0),
            [PlayOutput::Notice(PlayRemark::NotYourTurn)]
        );
        // The hash is cleared on every Resume (D1240), but nobody is asked yet.
        assert_eq!(c.resume(t0), [PlayOutput::ClearOpponentHash]);
        assert_eq!(c.phase(), PlayPhase::Player);
    }

    #[test]
    fn a_lost_opponent_pauses_and_a_new_one_resumes() {
        let t0 = Instant::now();
        let (mut c, _) =
            PlayController::new(setup(Color::Black, TimeMode::Depth(2)), t0).expect("new");
        let out = c.opponent_lost(t0);
        assert_eq!(out, [PlayOutput::Notice(PlayRemark::OpponentLost)]);
        assert_eq!(c.phase(), PlayPhase::Paused { lost: true });
        assert_eq!(c.resume(t0), [PlayOutput::Notice(PlayRemark::OpponentLost)]);
        let other = OpponentInfo {
            name: "Other".to_owned(),
            rating: Some(2000),
        };
        let out = c.replace_opponent(other, t0);
        assert_eq!(thinks(&out).len(), 1);
        assert_eq!(c.game().tags().white, "Other");
        assert_eq!(c.phase(), PlayPhase::Opponent);
    }

    #[test]
    fn a_lost_opponent_can_be_given_up_without_a_result() {
        let t0 = Instant::now();
        let (mut c, _) =
            PlayController::new(setup(Color::White, TimeMode::Depth(2)), t0).expect("new");
        c.opponent_lost(t0);
        assert!(c.quit(t0).is_empty());
        assert_eq!(c.phase(), PlayPhase::Closed);
        assert_eq!(c.result(), GameResult::Unknown);
    }

    // ---- the book (D1234) ----

    fn book_setup(color: Color, selection: BookSelection) -> PlaySetup {
        let mut s = setup(color, TimeMode::Depth(1));
        s.book = Some(sample_book(selection));
        s
    }

    #[test]
    fn the_engine_plays_a_book_move_for_free_and_marks_the_last_book_move() {
        let t0 = Instant::now();
        let mode = TimeMode::TimeBonus {
            white_minutes: 5,
            white_bonus: 3,
            black_minutes: 5,
            black_bonus: 3,
        };
        let mut s = book_setup(Color::Black, BookSelection::Best);
        s.config.mode = mode;
        let (c, out) = PlayController::new(s, t0 + SEC).expect("new");
        // The engine (White) played the heaviest book move, instantly.
        let moved: Vec<MoveRecord> = out
            .iter()
            .filter_map(|o| match o {
                PlayOutput::Moved(m) => Some(*m),
                _ => None,
            })
            .collect();
        assert_eq!(moved.len(), 1);
        assert!(moved[0].free);
        assert_eq!(moved[0].spent, Duration::ZERO);
        assert_eq!(c.position().turn(), Color::Black);
        // No increment was paid for a free move.
        assert_eq!(c.shown(Color::White, t0 + SEC), 300 * SEC);
        assert!(c.book_active());
        assert_eq!(c.game().comment(c.tip()).expect("node"), None);
    }

    #[test]
    fn the_last_book_move_is_commented_and_the_engine_then_thinks() {
        let t0 = Instant::now();
        let (mut c, _) =
            PlayController::new(book_setup(Color::Black, BookSelection::Best), t0).expect("new");
        // White (engine) played e4; Black's e5 is the last book move.
        let e5 = uci(&c, "e7e5");
        let out = c.player_move(e5, t0);
        assert_eq!(thinks(&out).len(), 1, "the book has nothing for White now");
        let node = out
            .iter()
            .find_map(|o| match o {
                PlayOutput::Moved(m) => Some(m.node),
                _ => None,
            })
            .expect("moved");
        assert_eq!(
            c.game().comment(node).expect("node"),
            Some("Last book move (book)")
        );
        assert!(!c.book_active());
    }

    #[test]
    fn leaving_the_book_asks_and_yes_comments_the_exit() {
        let t0 = Instant::now();
        let (mut c, _) =
            PlayController::new(book_setup(Color::Black, BookSelection::Best), t0).expect("new");
        let c5 = uci(&c, "c7c5");
        let out = c.player_move(c5, t0);
        assert_eq!(out, [PlayOutput::Ask(Question::LeaveBook { mv: c5 })]);
        assert_eq!(c.game().mainline().count(), 1, "the move was not made yet");
        let out = c.answer(true, t0);
        let node = out
            .iter()
            .find_map(|o| match o {
                PlayOutput::Moved(m) => Some(m.node),
                _ => None,
            })
            .expect("moved");
        assert_eq!(
            c.game().comment(node).expect("node"),
            Some("Move out of book (book: e5)")
        );
        assert!(!c.book_active());
        assert_eq!(c.phase(), PlayPhase::Opponent);
    }

    #[test]
    fn answering_no_takes_the_move_back_and_the_clock_keeps_going() {
        let t0 = Instant::now();
        let mut s = book_setup(Color::Black, BookSelection::Best);
        s.config.mode = TimeMode::TimeBonus {
            white_minutes: 5,
            white_bonus: 0,
            black_minutes: 5,
            black_bonus: 0,
        };
        let (mut c, _) = PlayController::new(s, t0).expect("new");
        let c5 = uci(&c, "c7c5");
        c.player_move(c5, t0 + 10 * SEC);
        // The clock stopped while the question was open.
        assert_eq!(c.shown(Color::Black, t0 + 100 * SEC), 290 * SEC);
        let out = c.answer(false, t0 + 100 * SEC);
        assert_eq!(out, [PlayOutput::Notice(PlayRemark::MoveTakenBack)]);
        assert_eq!(c.phase(), PlayPhase::Player);
        assert_eq!(c.shown(Color::Black, t0 + 105 * SEC), 285 * SEC);
        assert!(c.book_active());
        // The player can now play the book move.
        let e5 = uci(&c, "e7e5");
        assert!(matches!(
            c.player_move(e5, t0 + 106 * SEC)[0],
            PlayOutput::Moved(_)
        ));
    }

    #[test]
    fn resigning_during_a_question_ends_the_game() {
        let t0 = Instant::now();
        let (mut c, _) =
            PlayController::new(book_setup(Color::Black, BookSelection::Best), t0).expect("new");
        let c5 = uci(&c, "c7c5");
        c.player_move(c5, t0);
        let out = c.resign(t0);
        assert!(matches!(ended(&out), Some(PlayEnd::Resignation { .. })));
        assert!(c.answer(true, t0).is_empty());
    }

    // ---- the opening and the start (D1235, D1236) ----

    fn line_setup(color: Color, text: &str) -> PlaySetup {
        let mut s = setup(color, TimeMode::Depth(1));
        s.config.start = StartFrom::Opening(OpeningLine::parse(text).expect("line"));
        s
    }

    #[test]
    fn the_engine_follows_the_opening_line_and_the_tag_names_it() {
        let t0 = Instant::now();
        let (mut c, out) =
            PlayController::new(line_setup(Color::Black, "Sicilian: e4 c5 Nf3 d6"), t0)
                .expect("new");
        assert_eq!(c.game().tags().get_extra("Opening"), Some("Sicilian"));
        assert!(matches!(out[0], PlayOutput::Moved(m) if m.free && m.ply == 1));
        let c5 = uci(&c, "c7c5");
        let out = c.player_move(c5, t0);
        // The engine answered Nf3 from the line, instantly.
        assert!(matches!(out[1], PlayOutput::Moved(m) if m.free && m.ply == 3));
        let d6 = uci(&c, "d7d6");
        let out = c.player_move(d6, t0);
        // The line is over: now the engine thinks.
        assert_eq!(thinks(&out).len(), 1);
    }

    #[test]
    fn leaving_the_line_asks_and_continuing_frees_the_engine() {
        let t0 = Instant::now();
        let (mut c, _) =
            PlayController::new(line_setup(Color::Black, "Sicilian: e4 c5 Nf3"), t0).expect("new");
        let e5 = uci(&c, "e7e5");
        let out = c.player_move(e5, t0);
        assert_eq!(out, [PlayOutput::Ask(Question::LeaveLine { mv: e5 })]);
        let out = c.answer(true, t0);
        assert_eq!(thinks(&out).len(), 1, "no line move is forced any more");
        // Declining keeps the line.
        let (mut c, _) =
            PlayController::new(line_setup(Color::Black, "Sicilian: e4 c5 Nf3"), t0).expect("new");
        let e5 = uci(&c, "e7e5");
        c.player_move(e5, t0);
        c.answer(false, t0);
        let c5 = uci(&c, "c7c5");
        assert!(matches!(c.player_move(c5, t0)[0], PlayOutput::Moved(_)));
    }

    #[test]
    fn a_bad_opening_line_is_an_error() {
        let t0 = Instant::now();
        let s = line_setup(Color::White, "Bad: e4 e4");
        assert_eq!(
            PlayController::new(s, t0).err(),
            Some(PlayError::InvalidOpening)
        );
    }

    #[test]
    fn the_game_can_start_from_a_position() {
        let t0 = Instant::now();
        let mut s = setup(Color::Black, TimeMode::Depth(1));
        let fen = "3k4/8/8/8/8/8/4R3/4K3 b - - 0 1";
        s.start = Position::from_fen(fen).expect("fen");
        let (c, out) = PlayController::new(s, t0).expect("new");
        assert_eq!(c.phase(), PlayPhase::Player);
        assert!(out.is_empty());
        assert_eq!(c.game().start_position().fen(), fen);
    }

    #[test]
    fn a_start_that_is_already_over_ends_at_once() {
        let t0 = Instant::now();
        let mut s = setup(Color::White, TimeMode::Depth(1));
        s.start = Position::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").expect("fen");
        let (c, out) = PlayController::new(s, t0).expect("new");
        assert_eq!(ended(&out), Some(PlayEnd::Stalemate));
        assert!(!c.is_live());
    }

    #[test]
    fn the_time_can_be_given_from_outside_for_the_opponent() {
        let t0 = Instant::now();
        let mode = TimeMode::TimeBonus {
            white_minutes: 5,
            white_bonus: 0,
            black_minutes: 5,
            black_bonus: 0,
        };
        let (mut c, _) = PlayController::new(setup(Color::Black, mode), t0).expect("new");
        let e4 = uci(&c, "e2e4");
        let out = c.opponent_moved(Some(e4), Some(8 * SEC), t0 + SEC, None);
        assert!(matches!(out[0], PlayOutput::Moved(m) if m.spent == 8 * SEC));
        assert_eq!(c.shown(Color::White, t0 + SEC), 292 * SEC);
    }

    // ---- M8-1c slice A: take back, draw offers, Store clock ----

    fn tb_mode() -> TimeMode {
        TimeMode::TimeBonus {
            white_minutes: 5,
            white_bonus: 3,
            black_minutes: 5,
            black_bonus: 3,
        }
    }

    #[test]
    fn take_back_removes_the_pair_and_restores_clock_and_game() {
        let t0 = Instant::now();
        let (mut c, _) = PlayController::new(setup(Color::White, tb_mode()), t0).expect("new");
        assert!(
            c.take_back(t0)
                .contains(&PlayOutput::Notice(PlayRemark::NothingToTakeBack))
        );
        let e4 = uci(&c, "e2e4");
        c.player_move(e4, t0 + 4 * SEC);
        let e5 = uci(&c, "e7e5");
        c.opponent_moved(Some(e5), None, t0 + 9 * SEC, None);
        assert!(c.can_take_back());
        let out = c.take_back(t0 + 20 * SEC);
        assert_eq!(out, [PlayOutput::TookBack]);
        assert_eq!(c.phase(), PlayPhase::Player);
        assert_eq!(c.game().mainline().count(), 0);
        assert_eq!(c.position().fen(), Position::new().fen());
        assert_eq!(c.shown(Color::White, t0 + 20 * SEC), 300 * SEC);
        assert_eq!(c.shown(Color::Black, t0 + 20 * SEC), 300 * SEC);
        assert_eq!(c.game().tags().get_extra("Takebacks"), Some("1"));
        assert!(!c.can_take_back());
        // The player can move again.
        let d4 = uci(&c, "d2d4");
        assert!(matches!(
            c.player_move(d4, t0 + 21 * SEC)[0],
            PlayOutput::Moved(_)
        ));
    }

    #[test]
    fn take_back_is_refused_while_the_engine_thinks_or_with_only_the_engine_move() {
        let t0 = Instant::now();
        let (mut c, _) = PlayController::new(setup(Color::White, tb_mode()), t0).expect("new");
        let e4 = uci(&c, "e2e4");
        c.player_move(e4, t0);
        assert_eq!(c.phase(), PlayPhase::Opponent);
        assert_eq!(
            c.take_back(t0),
            [PlayOutput::Notice(PlayRemark::NothingToTakeBack)]
        );
        // Black player: only the engine has moved.
        let (mut c, _) = PlayController::new(setup(Color::Black, tb_mode()), t0).expect("new");
        let e4 = uci(&c, "e2e4");
        c.opponent_moved(Some(e4), None, t0, None);
        assert!(!c.can_take_back());
        assert_eq!(
            c.take_back(t0),
            [PlayOutput::Notice(PlayRemark::NothingToTakeBack)]
        );
    }

    #[test]
    fn take_back_as_black_keeps_the_first_engine_move() {
        let t0 = Instant::now();
        let (mut c, _) = PlayController::new(setup(Color::Black, tb_mode()), t0).expect("new");
        for (text, engine) in [("e2e4", true), ("e7e5", false), ("g1f3", true)] {
            let m = uci(&c, text);
            if engine {
                c.opponent_moved(Some(m), None, t0, None);
            } else {
                c.player_move(m, t0);
            }
        }
        assert_eq!(c.take_back(t0), [PlayOutput::TookBack]);
        assert_eq!(c.game().mainline().count(), 1);
        assert_eq!(c.position().turn(), Color::Black);
        assert_eq!(c.phase(), PlayPhase::Player);
    }

    #[test]
    fn take_back_undoes_a_book_exit() {
        let t0 = Instant::now();
        let (mut c, _) =
            PlayController::new(book_setup(Color::Black, BookSelection::Best), t0).expect("new");
        let e5 = uci(&c, "e7e5");
        c.player_move(e5, t0);
        assert!(!c.book_active());
        let nf3 = uci(&c, "g1f3");
        c.opponent_moved(Some(nf3), None, t0, None);
        assert_eq!(c.take_back(t0), [PlayOutput::TookBack]);
        assert!(c.book_active(), "the book guides again");
        assert_eq!(c.game().mainline().count(), 1);
    }

    fn feed_engine_evals(c: &mut PlayController, t0: Instant, evals: &[Score]) {
        // Shuffles the knights; each engine move carries the next score.
        let cycle = [
            ("g1f3", true),
            ("g8f6", false),
            ("f3g1", true),
            ("f6g8", false),
        ];
        let mut i = 0;
        let mut k = 0;
        while k < evals.len() {
            let (text, player) = cycle[i % 4];
            let m = uci(c, text);
            if player {
                c.player_move(m, t0);
            } else {
                c.opponent_moved(Some(m), None, t0, Some(evals[k]));
                k += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn a_draw_offer_is_allowed_once_per_player_move() {
        let t0 = Instant::now();
        let (mut c, _) = PlayController::new(setup(Color::White, tb_mode()), t0).expect("new");
        assert!(c.can_offer_draw());
        assert_eq!(
            c.offer_draw(t0),
            [PlayOutput::Notice(PlayRemark::DrawDeclined)]
        );
        assert!(!c.can_offer_draw());
        assert_eq!(
            c.offer_draw(t0),
            [PlayOutput::Notice(PlayRemark::DrawNotAllowed)]
        );
        let e4 = uci(&c, "e2e4");
        c.player_move(e4, t0);
        // Not the player's turn.
        assert_eq!(
            c.offer_draw(t0),
            [PlayOutput::Notice(PlayRemark::DrawNotAllowed)]
        );
        let e5 = uci(&c, "e7e5");
        c.opponent_moved(Some(e5), None, t0, None);
        assert!(c.can_offer_draw());
    }

    #[test]
    fn the_engine_accepts_with_three_low_evaluations_only_from_ply_40() {
        let t0 = Instant::now();
        let low = [Score::Cp(5), Score::Cp(30), Score::Cp(-10)];
        let mut c = PlayController::new(setup(Color::White, TimeMode::Depth(1)), t0)
            .expect("new")
            .0;
        // The engine is Black: its view is the negation of the White score.
        feed_engine_evals(&mut c, t0, &low);
        // Too early (ply < 40): declined, and no new offer for this move.
        assert_eq!(
            c.offer_draw(t0),
            [PlayOutput::Notice(PlayRemark::DrawDeclined)]
        );
        // The ply is faked, since a legal game that long needs a real one.
        c.offered_at = None;
        c.moves.resize(DRAW_MIN_PLY, c.moves[0]);
        assert_eq!(c.offer_draw(t0), [PlayOutput::Ended(PlayEnd::DrawAgreed)]);
        assert_eq!(c.result(), GameResult::Draw);
        assert_eq!(c.end(), Some(PlayEnd::DrawAgreed));
    }

    #[test]
    fn a_high_evaluation_or_a_mate_for_the_engine_declines_the_draw() {
        let t0 = Instant::now();
        // The engine is Black: a score of +50 for White is -50 for it
        // (accepts), -50 for White is +50 for it (declines), and a mate for
        // Black declines.
        for (scores, accepted) in [
            ([Score::Cp(50), Score::Cp(50), Score::Cp(50)], true),
            ([Score::Cp(50), Score::Cp(-50), Score::Cp(50)], false),
            ([Score::Cp(0), Score::Cp(0), Score::Mate(-3)], false),
            ([Score::Cp(0), Score::Cp(0), Score::Mate(3)], true),
        ] {
            let mut c = PlayController::new(setup(Color::White, TimeMode::Depth(1)), t0)
                .expect("new")
                .0;
            feed_engine_evals(&mut c, t0, &scores);
            c.moves.resize(DRAW_MIN_PLY, c.moves[0]);
            let out = c.offer_draw(t0);
            assert_eq!(
                out == [PlayOutput::Ended(PlayEnd::DrawAgreed)],
                accepted,
                "{scores:?}"
            );
        }
    }

    #[test]
    fn a_missing_evaluation_or_a_book_move_does_not_count_for_the_draw() {
        let t0 = Instant::now();
        let mut c = PlayController::new(setup(Color::White, TimeMode::Depth(1)), t0)
            .expect("new")
            .0;
        for (text, player) in [
            ("g1f3", true),
            ("g8f6", false),
            ("f3g1", true),
            ("f6g8", false),
            ("g1f3", true),
            ("g8f6", false),
        ] {
            let m = uci(&c, text);
            if player {
                c.player_move(m, t0);
            } else {
                c.opponent_moved(Some(m), None, t0, None);
            }
        }
        c.moves.resize(DRAW_MIN_PLY, c.moves[0]);
        assert_eq!(
            c.offer_draw(t0),
            [PlayOutput::Notice(PlayRemark::DrawDeclined)]
        );
    }

    #[test]
    fn store_clock_writes_emt_and_clk_after_the_existing_comment() {
        let t0 = Instant::now();
        let mut s = book_setup(Color::Black, BookSelection::Best);
        s.config.mode = tb_mode();
        s.config.store_clock = true;
        let (mut c, out) = PlayController::new(s, t0).expect("new");
        // The free book move of the engine carries no clock.
        let node = out
            .iter()
            .find_map(|o| match o {
                PlayOutput::Moved(m) => Some(m.node),
                _ => None,
            })
            .expect("moved");
        assert_eq!(c.game().comment(node).expect("node"), None);
        // The last book move: comment first, then the clock codes.
        let e5 = uci(&c, "e7e5");
        let out = c.player_move(e5, t0 + 12 * SEC);
        let node = out
            .iter()
            .find_map(|o| match o {
                PlayOutput::Moved(m) => Some(m.node),
                _ => None,
            })
            .expect("moved");
        assert_eq!(
            c.game().comment(node).expect("node"),
            Some("Last book move (book) [%emt 0:00:12] [%clk 0:04:51]")
        );
    }

    #[test]
    fn store_clock_is_off_by_default_and_count_up_has_no_clk() {
        let t0 = Instant::now();
        let (mut c, _) =
            PlayController::new(setup(Color::White, TimeMode::Depth(1)), t0).expect("new");
        let e4 = uci(&c, "e2e4");
        let out = c.player_move(e4, t0 + 5 * SEC);
        let PlayOutput::Moved(m) = out[0] else {
            panic!("moved");
        };
        assert_eq!(c.game().comment(m.node).expect("node"), None);

        let mut s = setup(Color::White, TimeMode::Depth(1));
        s.config.store_clock = true;
        let (mut c, _) = PlayController::new(s, t0).expect("new");
        let e4 = uci(&c, "e2e4");
        let out = c.player_move(e4, t0 + 5 * SEC);
        let PlayOutput::Moved(m) = out[0] else {
            panic!("moved");
        };
        assert_eq!(
            c.game().comment(m.node).expect("node"),
            Some("[%emt 0:00:05]")
        );
    }
}
