//! Messages, notices and the view of the game against an engine (spec
//! M8-1, D1220-D1244). The front-end sends [`PlayMessage`]s, shows
//! [`PlayNotice`]s and draws the clocks and buttons from a [`PlayView`].

use std::time::{Duration, Instant};

use mychess_core::{Color, GameResult, Role, Square};

use super::book::PlayBook;
use super::coach::CoachView;
use super::config::PlayConfig;
use super::controller::{PlayPhase, Question};
use super::opponent::OpponentInfo;
use super::rules::PlayEnd;
use crate::engines::EngineId;
use crate::tactical_game::{PickFailure, Picked};

/// Why a book could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BookFailure {
    /// The file does not exist.
    NotFound,
    /// The file could not be read or is not a Polyglot book.
    Unreadable,
}

/// An event of the game against an engine.
///
/// The messages that depend on time carry the `Instant` of the front-end's
/// clock (the model never reads it, D1225).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PlayMessage {
    /// Starts a game with this configuration (the "Play" button).
    Start(Box<PlayConfig>),
    /// The book asked for by `Start` was read.
    BookLoaded {
        /// The request of the effect.
        request: u64,
        /// The book, or why not.
        result: Result<PlayBook, BookFailure>,
    },
    /// The player moved a piece on the board.
    PlayerMove {
        /// Origin square.
        from: Square,
        /// Destination square.
        to: Square,
        /// The piece chosen for a promotion.
        promotion: Option<Role>,
        /// When.
        at: Instant,
    },
    /// The answer to a [`Question`].
    Answer {
        /// Yes: go on and leave the book or the line.
        yes: bool,
        /// When.
        at: Instant,
    },
    /// The player resigns; the game is saved with the loss (D1227).
    Resign(Instant),
    /// Leaves without result and without saving (D1227).
    Quit(Instant),
    /// Starts again with the same configuration (D1227).
    Restart(Instant),
    /// Starts again with the same configuration and the colours swapped,
    /// once the game is over; ignored otherwise (M8-12, D3101).
    Rematch(Instant),
    /// Pauses the game (D1240).
    Pause(Instant),
    /// Resumes a paused game (D1240).
    Resume(Instant),
    /// After the engine was lost (D1239): goes on with another engine.
    SwitchEngine(EngineId, Instant),
    /// Takes back the last move of the player and the answer to it (M8-1c,
    /// D1500).
    TakeBack(Instant),
    /// The player offers a draw (M8-1c, D1503).
    OfferDraw(Instant),
    /// A position was drawn for a tactical game (M8-7, D2460).
    Picked {
        /// The request of the effect.
        request: u64,
        /// The position, or why not.
        result: Result<Picked, PickFailure>,
    },
    /// The clock ticks (every 100 ms while
    /// [`AppState::play_wants_ticks`](crate::AppState::play_wants_ticks)).
    Tick(Instant),
    /// The player enters a premove while the engine thinks (D4452). Ignored
    /// when it is not the engine's turn, the premove is off or the piece
    /// could never make the move.
    SetPremove {
        /// Origin square.
        from: Square,
        /// Destination square.
        to: Square,
        /// The piece chosen for a promotion (a queen when `None`).
        promotion: Option<Role>,
    },
    /// Drops the premove (D4452).
    CancelPremove,
    /// Switches the premove against the engine on or off for the session
    /// (D4452); off also drops the one kept.
    EnablePremove(bool),
}

/// A remark about the game against an engine; the front-end translates it
/// from [`PlayNotice::message_key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlayNotice {
    /// A game is already on.
    AlreadyRunning,
    /// The engine does not exist or is not ready (D1223).
    NoEngine,
    /// Only UCI engines play (D1223).
    NotUci,
    /// An annotation, a shoot-out, a dialog or the trial mode holds the
    /// game.
    Busy,
    /// The game cannot be edited while the engine plays.
    Locked,
    /// The game in the editor has unsaved changes and would be replaced:
    /// save or discard it first.
    UnsavedGame,
    /// The book could not be read; the game goes on without it.
    BookUnreadable,
    /// The specific opening has an illegal move.
    BadOpening,
    /// The move is illegal.
    IllegalMove,
    /// It is not the player's turn.
    NotYourTurn,
    /// The player declined to leave the book or the line.
    MoveTakenBack,
    /// The engine is gone; the game is paused (D1239).
    EngineLost,
    /// The game ended.
    Ended(PlayEnd),
    /// The game was saved or is in the editor, altered; the database does
    /// not take it automatically (see D1245).
    Unsaved,
    /// There is no move of the player to take back (M8-1c, D1500).
    NothingToTakeBack,
    /// A draw cannot be offered now (M8-1c, D1503).
    DrawNotAllowed,
    /// The engine declined the draw (M8-1c, D1504).
    DrawDeclined,
    /// *Ponder* was asked for but the engine has no `Ponder` option (M8-1d,
    /// D2401).
    PonderUnsupported,
    /// The coach cannot start: its engine is missing, is not UCI or is the
    /// opponent's (M8-1d, D2416).
    CoachUnavailable,
    /// The coach engine is gone; the game goes on without it (D2423).
    CoachLost,
    /// Taking back is off while the coach watches (D2427).
    CoachNoTakeBack,
    /// The Elo asked for is outside what the engine declares and was moved
    /// into its range (M8-7, D2455).
    TacticalEloClamped,
    /// The engine has no option of strength: its searches are cut at a
    /// depth (M8-7, D2454).
    TacticalDepthLimited,
    /// No position could be drawn (M8-7, D2460).
    TacticalPickFailed,
    /// The source of the drawing has no game (M8-7, D2460).
    TacticalPickEmpty,
    /// The start position has no move to play (M8-7, D2458).
    TacticalBadPosition,
    /// The game is Chess960 but the engine has no `UCI_Chess960` option
    /// (M9-6, D4505).
    Chess960Unsupported,
}

impl PlayNotice {
    /// The Fluent key of the message.
    pub fn message_key(self) -> &'static str {
        match self {
            PlayNotice::AlreadyRunning => "play-notice-already-running",
            PlayNotice::NoEngine => "play-notice-no-engine",
            PlayNotice::NotUci => "play-notice-not-uci",
            PlayNotice::Chess960Unsupported => "play-notice-chess960-unsupported",
            PlayNotice::Busy => "play-notice-busy",
            PlayNotice::Locked => "play-notice-locked",
            PlayNotice::UnsavedGame => "play-notice-unsaved-game",
            PlayNotice::BookUnreadable => "play-notice-book-unreadable",
            PlayNotice::BadOpening => "play-notice-bad-opening",
            PlayNotice::IllegalMove => "play-notice-illegal-move",
            PlayNotice::NotYourTurn => "play-notice-not-your-turn",
            PlayNotice::MoveTakenBack => "play-notice-move-taken-back",
            PlayNotice::EngineLost => "play-notice-engine-lost",
            PlayNotice::Unsaved => "play-notice-unsaved",
            PlayNotice::NothingToTakeBack => "play-notice-nothing-to-take-back",
            PlayNotice::DrawNotAllowed => "play-notice-draw-not-allowed",
            PlayNotice::DrawDeclined => "play-notice-draw-declined",
            PlayNotice::PonderUnsupported => "play-notice-ponder-unsupported",
            PlayNotice::CoachUnavailable => "play-notice-coach-unavailable",
            PlayNotice::CoachLost => "play-notice-coach-lost",
            PlayNotice::CoachNoTakeBack => "play-notice-coach-no-take-back",
            PlayNotice::TacticalEloClamped => "play-notice-tactical-elo-clamped",
            PlayNotice::TacticalDepthLimited => "play-notice-tactical-depth-limited",
            PlayNotice::TacticalPickFailed => "play-notice-tactical-pick-failed",
            PlayNotice::TacticalPickEmpty => "play-notice-tactical-pick-empty",
            PlayNotice::TacticalBadPosition => "play-notice-tactical-bad-position",
            PlayNotice::Ended(end) => match end {
                PlayEnd::Checkmate { .. } => "play-notice-end-checkmate",
                PlayEnd::Stalemate => "play-notice-end-stalemate",
                PlayEnd::InsufficientMaterial => "play-notice-end-material",
                PlayEnd::FiftyMoves => "play-notice-end-fifty",
                PlayEnd::ThreefoldRepetition => "play-notice-end-repetition",
                PlayEnd::Timeout { .. } => "play-notice-end-timeout",
                PlayEnd::TimeoutDraw => "play-notice-end-timeout-draw",
                PlayEnd::Resignation { .. } => "play-notice-end-resignation",
                PlayEnd::OpponentFailure => "play-notice-end-engine-failure",
                PlayEnd::DrawAgreed => "play-notice-end-draw-agreed",
            },
        }
    }
}

/// What the front-end draws: the clocks, the buttons and the question.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayView {
    /// Where the game is.
    pub phase: PlayPhase,
    /// The colour of the player (the board turns to it, D1222).
    pub player_color: Color,
    /// Who the opponent is.
    pub opponent: OpponentInfo,
    /// The time of White (left in a countdown, used otherwise).
    pub white_time: Duration,
    /// The time of Black.
    pub black_time: Duration,
    /// Whether the clocks count down (the flag can fall).
    pub countdown: bool,
    /// The question waiting for the player, if any.
    pub question: Option<Question>,
    /// The result so far (`*` while the game runs).
    pub result: GameResult,
    /// Whether Resume is possible (paused and the engine is there).
    pub can_resume: bool,
    /// The engine was lost: the front-end offers another one or Quit.
    pub engine_lost: bool,
    /// Whether Take back is possible now (M8-1c, D1500).
    pub can_take_back: bool,
    /// Whether a draw can be offered now (M8-1c, D1503).
    pub can_offer_draw: bool,
    /// The opponent is thinking on the player's time (M8-1d, D2411).
    pub pondering: bool,
    /// The coach, when the game has one (M8-1d, D2425).
    pub coach: Option<CoachView>,
}
