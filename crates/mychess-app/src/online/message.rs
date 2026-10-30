//! The messages, notices and errors of the online play (spec M8-3, D1310,
//! D1328).
//!
//! The front-end never sees the types of `mychess-online`: what it needs is
//! here. The stream updates are opaque ([`StreamUpdate`]); the hub of
//! [`crate::online::OnlineHub`] produces them and `update` consumes them.

use std::time::Instant;

use mychess_core::Role;
use mychess_core::Square;
use mychess_online::models::{ChallengeUpdate, Event, GameEvent, WatchEvent};
use mychess_online::{Error, SeekOutcome, StreamMessage};

/// Why the online play failed, for the front-end to translate (D1328). Never
/// carries the token nor a raw server body.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OnlineError {
    /// The token was refused (expired or revoked).
    Unauthorized,
    /// The token lacks a scope.
    ScopeMissing,
    /// Too many requests: new ones wait `secs` seconds.
    RateLimited {
        /// Seconds to wait.
        secs: u64,
    },
    /// No connection to the server.
    Network,
    /// The server did not answer in time.
    Timeout,
    /// The game, user or challenge does not exist.
    NotFound,
    /// The server refused the request.
    Rejected {
        /// The reason the server gave (short, may be empty).
        reason: String,
    },
    /// The server said something that cannot be understood.
    Protocol,
    /// The account is a Bot account (D1301).
    BotAccount,
    /// The configured server address is not safe for a token.
    InsecureUrl,
    /// There is no token to connect with.
    NoToken,
    /// The text is not a valid token.
    BadToken,
    /// The keychain or the token file failed.
    TokenStore,
    /// The time control is not accepted by the Board API.
    TimeControl,
    /// The login in the browser did not complete (denied, timed out or
    /// the callback was refused, D3005).
    LoginFailed,
    /// The server has a problem.
    Server,
    /// Anything else (a closed stream, an invalid argument).
    Other,
}

impl From<&Error> for OnlineError {
    fn from(error: &Error) -> OnlineError {
        match error {
            Error::Unauthorized => OnlineError::Unauthorized,
            Error::ScopeMissing { .. } => OnlineError::ScopeMissing,
            Error::RateLimited { retry_after } => OnlineError::RateLimited {
                secs: retry_after.as_secs(),
            },
            Error::Network(_) => OnlineError::Network,
            Error::Timeout => OnlineError::Timeout,
            Error::NotFound => OnlineError::NotFound,
            Error::Rejected { reason, .. } | Error::AmbiguousAfterRetry { reason, .. } => {
                OnlineError::Rejected {
                    reason: super::game_builder::clean_text(reason, 120),
                }
            }
            Error::Protocol(_) | Error::LineTooLong | Error::Redirect(_) => OnlineError::Protocol,
            Error::BotAccount => OnlineError::BotAccount,
            Error::InsecureUrl => OnlineError::InsecureUrl,
            Error::TimeControl(_) => OnlineError::TimeControl,
            Error::OAuth(_) => OnlineError::LoginFailed,
            Error::TokenStore(_) => OnlineError::TokenStore,
            Error::Server(_) => OnlineError::Server,
            Error::InvalidArgument { what } if *what == "token" => OnlineError::BadToken,
            _ => OnlineError::Other,
        }
    }
}

impl OnlineError {
    /// The Fluent key of the error (`online-error-*`).
    pub fn message_key(&self) -> &'static str {
        match self {
            OnlineError::Unauthorized => "online-error-unauthorized",
            OnlineError::ScopeMissing => "online-error-scope-missing",
            OnlineError::RateLimited { .. } => "online-error-rate-limited",
            OnlineError::Network => "online-error-network",
            OnlineError::Timeout => "online-error-timeout",
            OnlineError::NotFound => "online-error-not-found",
            OnlineError::Rejected { .. } => "online-error-rejected",
            OnlineError::Protocol => "online-error-protocol",
            OnlineError::BotAccount => "online-error-bot-account",
            OnlineError::InsecureUrl => "online-error-insecure-url",
            OnlineError::NoToken => "online-error-no-token",
            OnlineError::BadToken => "online-error-bad-token",
            OnlineError::TokenStore => "online-error-token-store",
            OnlineError::TimeControl => "online-error-time-control",
            OnlineError::LoginFailed => "online-error-login-failed",
            OnlineError::Server => "online-error-server",
            OnlineError::Other => "online-error-other",
        }
    }
}

/// Why a command was refused without touching the network (D1311).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RejectReason {
    /// The session is not in a state that allows it.
    WrongState,
    /// Not connected.
    NotConnected,
    /// A game against an engine, a tournament, an annotation or a training
    /// mode is on (D1312).
    Busy,
    /// A game is on: leave it first.
    GameInProgress,
    /// The event stream is not connected yet (D1315).
    EventsNotReady,
    /// The time control is not accepted.
    TimeControl,
    /// It is not the turn of the user.
    NotYourTurn,
    /// There is no game.
    NoGame,
    /// The move cannot be made.
    IllegalMove,
    /// The challenge is unknown or cannot be played.
    ChallengeNotPlayable,
    /// The action does not apply now (resign before two moves, claim
    /// without an absent opponent...).
    NotAvailable,
    /// The name of the user is not valid.
    BadUser,
    /// A move is already waiting for the server.
    MovePending,
    /// The text is not a game id or a Lichess game link (D3000).
    BadGameRef,
    /// The game is the one the user plays (D3001).
    OwnGame,
    /// The variant or the fixed starting position is not acceptable (M9-6).
    BadVariant,
}

impl RejectReason {
    fn key(self) -> &'static str {
        match self {
            RejectReason::WrongState => "online-notice-rejected-wrong-state",
            RejectReason::NotConnected => "online-notice-rejected-not-connected",
            RejectReason::Busy => "online-notice-rejected-busy",
            RejectReason::GameInProgress => "online-notice-rejected-game-in-progress",
            RejectReason::EventsNotReady => "online-notice-rejected-events-not-ready",
            RejectReason::TimeControl => "online-notice-rejected-time-control",
            RejectReason::NotYourTurn => "online-notice-rejected-not-your-turn",
            RejectReason::NoGame => "online-notice-rejected-no-game",
            RejectReason::IllegalMove => "online-notice-rejected-illegal-move",
            RejectReason::ChallengeNotPlayable => "online-notice-rejected-challenge",
            RejectReason::NotAvailable => "online-notice-rejected-not-available",
            RejectReason::BadUser => "online-notice-rejected-bad-user",
            RejectReason::MovePending => "online-notice-rejected-move-pending",
            RejectReason::BadGameRef => "online-notice-rejected-bad-game-ref",
            RejectReason::OwnGame => "online-notice-rejected-own-game",
            RejectReason::BadVariant => "online-notice-rejected-bad-variant",
        }
    }
}

/// A remark of the online play for the status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OnlineNotice {
    /// A command was refused.
    Rejected(RejectReason),
    /// A failure.
    Error(OnlineError),
    /// A message of the families of D1330 was dropped.
    AssistanceDisabled,
    /// A seek or challenge starts: engines and queries will be disabled
    /// (D1331).
    AssistanceWillBeDisabled,
    /// Connected as `login`.
    Connected,
    /// Signed out; the token was revoked in the keychain only, so the user
    /// should revoke it at `lichess.org/account/oauth/token` (D1307).
    SignedOut,
    /// The connection to the server is not coming back.
    Disconnected,
    /// Another game is on elsewhere (D1312).
    AnotherGame,
    /// A game in progress was resumed.
    Resumed,
    /// The variant of the game is not played here (D1313).
    UnsupportedVariant,
    /// The server refused the move: it was taken back.
    MoveRejected {
        /// What the server said.
        reason: String,
    },
    /// A retried move was refused; the server state decides (D1347).
    MoveUncertain,
    /// The view of the game was rebuilt once from the server (D1318).
    Resynchronised,
    /// The view of the game cannot follow the server any more (D1318).
    Desynced,
    /// The stream of the game could not be recovered and the game was closed
    /// here (D1363); it may still be on at the server.
    GameLost,
    /// The search for a game ended without a game starting (D1366).
    SearchTimedOut,
    /// The challenge was declined.
    ChallengeDeclined {
        /// The reason key of the server (`generic`, `later`...), if any.
        reason: Option<String>,
    },
    /// The game was stored in the Clipbase.
    Imported,
    /// The game had been stored already (D1335).
    AlreadyImported,
    /// The game ended without moves and is not stored (D1327).
    NotImported,
    /// The game could not be stored.
    ImportFailed,
    /// The lock of the assistance was released by the user (D1332).
    LockReleased,
    /// The marker of a game in progress was stale and is gone (D1332).
    MarkerCleared,
    /// The game that was on the editor before is back.
    PreviousRestored,
    /// The game is over.
    GameOver,
    /// A game of other players is being watched (D3000).
    Watching,
    /// The watched game ended.
    WatchEnded,
    /// The watched game could not be completed from the start; the moves
    /// shown begin at a later position (D3003).
    WatchIncomplete,
    /// Moves of the watched game arrived faster than the history could be
    /// loaded and some were dropped (D3115).
    WatchStepsDropped,
    /// The browser could not be opened by the program: the user must open
    /// the link by hand (D3009).
    BrowserNotOpened,
    /// The rematch was proposed (D3012).
    RematchSent,
}

impl OnlineNotice {
    /// The Fluent key (`online-notice-*` or `online-error-*`).
    pub fn message_key(&self) -> &'static str {
        match self {
            OnlineNotice::Rejected(reason) => reason.key(),
            OnlineNotice::Error(error) => error.message_key(),
            OnlineNotice::AssistanceDisabled => "online-notice-assistance-disabled",
            OnlineNotice::AssistanceWillBeDisabled => "online-notice-assistance-will-be-disabled",
            OnlineNotice::Connected => "online-notice-connected",
            OnlineNotice::SignedOut => "online-notice-signed-out",
            OnlineNotice::Disconnected => "online-notice-disconnected",
            OnlineNotice::AnotherGame => "online-notice-another-game",
            OnlineNotice::Resumed => "online-notice-resumed",
            OnlineNotice::UnsupportedVariant => "online-notice-unsupported-variant",
            OnlineNotice::MoveRejected { .. } => "online-notice-move-rejected",
            OnlineNotice::MoveUncertain => "online-notice-move-uncertain",
            OnlineNotice::Resynchronised => "online-notice-resynchronised",
            OnlineNotice::Desynced => "online-notice-desynced",
            OnlineNotice::GameLost => "online-notice-game-lost",
            OnlineNotice::SearchTimedOut => "online-notice-search-timed-out",
            OnlineNotice::ChallengeDeclined { .. } => "online-notice-challenge-declined",
            OnlineNotice::Imported => "online-notice-imported",
            OnlineNotice::AlreadyImported => "online-notice-already-imported",
            OnlineNotice::NotImported => "online-notice-not-imported",
            OnlineNotice::ImportFailed => "online-notice-import-failed",
            OnlineNotice::LockReleased => "online-notice-lock-released",
            OnlineNotice::MarkerCleared => "online-notice-marker-cleared",
            OnlineNotice::PreviousRestored => "online-notice-previous-restored",
            OnlineNotice::GameOver => "online-notice-game-over",
            OnlineNotice::Watching => "online-notice-watching",
            OnlineNotice::WatchEnded => "online-notice-watch-ended",
            OnlineNotice::WatchIncomplete => "online-notice-watch-incomplete",
            OnlineNotice::WatchStepsDropped => "online-notice-watch-steps-dropped",
            OnlineNotice::BrowserNotOpened => "online-notice-browser-not-opened",
            OnlineNotice::RematchSent => "online-notice-rematch-sent",
        }
    }
}

/// Where the token of a connection comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TokenSource {
    /// The one staged with
    /// [`OnlineHub::stage_token`](crate::online::OnlineHub::stage_token) (the
    /// text never goes through a message).
    Staged,
    /// The one kept in the store for this account.
    Stored {
        /// The account name.
        login: String,
    },
}

/// What to do with a staged token once it is known to work (D1307).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StoreChoice {
    /// Keep it in the keychain of the system.
    Keyring,
    /// Keep it in the 0600 file (only after the user consented).
    File,
    /// Keep it for this session only.
    Session,
}

/// A request to connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectRequest {
    /// Where the token comes from.
    pub source: TokenSource,
    /// Where to keep a staged token.
    pub store: StoreChoice,
}

/// The colour asked for in a seek or a challenge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OnlineColor {
    /// A draw decides.
    #[default]
    Random,
    /// White.
    White,
    /// Black.
    Black,
}

/// The variant asked for in a seek or a challenge (M9-6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OnlineVariant {
    /// Standard chess.
    #[default]
    Standard,
    /// Chess960. `start` is the Scharnagl number (0 to 959) of a fixed
    /// starting position, only for a challenge (a seek cannot fix it); `None`
    /// lets the server draw one.
    Chess960 {
        /// The fixed starting position, when there is one.
        start: Option<u16>,
    },
}

impl OnlineVariant {
    /// Whether it is Chess960.
    #[must_use]
    pub fn is_chess960(self) -> bool {
        matches!(self, OnlineVariant::Chess960 { .. })
    }
}

/// A seek for an opponent (D1315).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeekRequest {
    /// Minutes of each side.
    pub minutes: u32,
    /// Increment in seconds.
    pub increment: u32,
    /// Rated game.
    pub rated: bool,
    /// The colour.
    pub color: OnlineColor,
    /// The accepted rating range of the opponent.
    pub rating_range: Option<(u32, u32)>,
    /// The variant (a seek never fixes the position).
    pub variant: OnlineVariant,
}

/// A challenge to a user (D1316).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChallengeRequest {
    /// The name of the user.
    pub username: String,
    /// The clock limit in seconds.
    pub limit_secs: u32,
    /// Increment in seconds.
    pub increment: u32,
    /// Rated game.
    pub rated: bool,
    /// The colour.
    pub color: OnlineColor,
    /// The variant; a fixed Chess960 position needs a casual game.
    pub variant: OnlineVariant,
}

/// What an action sent to the server was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ActionKind {
    /// A move.
    Move,
    /// Resign.
    Resign,
    /// Abort.
    Abort,
    /// Offer or accept a draw.
    DrawYes,
    /// Decline a draw.
    DrawNo,
    /// Accept a takeback.
    TakebackYes,
    /// Propose a takeback of the last move of the user (the same endpoint
    /// as accepting one, D3011).
    TakebackPropose,
    /// Claim a draw over an absent opponent.
    ClaimDraw,
    /// Decline a takeback.
    TakebackNo,
    /// Claim the victory over an absent opponent.
    ClaimVictory,
    /// Accept a challenge.
    AcceptChallenge,
    /// Decline a challenge.
    DeclineChallenge,
    /// Cancel a challenge.
    CancelChallenge,
    /// Sign out.
    Disconnect,
}

/// The answer of the server to an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDone {
    /// What was asked.
    pub action: ActionKind,
    /// The game or challenge it was about.
    pub id: Option<String>,
    /// The ply the move would reach (for [`ActionKind::Move`]).
    pub ply: Option<usize>,
    /// Whether it worked.
    pub result: Result<(), ActionFailure>,
    /// When the answer arrived.
    pub at: Instant,
}

/// Why an action failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionFailure {
    /// The error.
    pub error: OnlineError,
    /// The request may have reached the server (a retried 400, D1347).
    pub ambiguous: bool,
}

impl ActionFailure {
    pub(crate) fn from_error(error: &Error) -> ActionFailure {
        ActionFailure {
            error: OnlineError::from(error),
            ambiguous: matches!(error, Error::AmbiguousAfterRetry { .. }),
        }
    }
}

/// What a stream said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StreamKind {
    Account(StreamMessage<Event>),
    Game {
        id: String,
        message: StreamMessage<GameEvent>,
    },
    Seek(SeekOutcome),
    Challenge(StreamMessage<ChallengeUpdate>),
    Watch {
        id: String,
        message: StreamMessage<WatchEvent>,
    },
}

/// An update from a stream task, stamped with the epoch of the stream (an
/// update of an earlier stream is dropped) and the instant it arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamUpdate {
    pub(crate) epoch: u64,
    pub(crate) at: Instant,
    pub(crate) kind: StreamKind,
}

/// The account, for the menu and the strips (D1329).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountView {
    /// The user name.
    pub login: String,
    /// The id (lower case) used to tell which side is the user.
    pub id: String,
    /// The title (`GM`...).
    pub title: Option<String>,
    /// Ratings by modality: `(name, rating, provisional)`, sorted by name.
    pub ratings: Vec<(String, u32, bool)>,
    /// Whether challenges can be sent and answered.
    pub can_challenge: bool,
}

/// The marker of a game in progress (D1332).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerInfo {
    /// The game.
    pub game_id: String,
    /// The date it was written.
    pub date: String,
}

/// An event for the online play.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum OnlineMessage {
    // ---- commands of the user ----
    /// Connect to the server.
    Connect(ConnectRequest),
    /// Sign out (the token is deleted from the stores).
    Disconnect,
    /// Look for an opponent.
    Seek(SeekRequest),
    /// Stop looking.
    CancelSeek,
    /// Challenge a user.
    Challenge(ChallengeRequest),
    /// Withdraw the challenge sent.
    CancelChallenge,
    /// Accept a challenge received.
    AcceptChallenge(String),
    /// Decline a challenge received.
    DeclineChallenge(String),
    /// A move of the user (or a premove when it is not his turn).
    PlayerMove {
        /// Origin.
        from: Square,
        /// Destination.
        to: Square,
        /// Promotion piece.
        promotion: Option<Role>,
        /// Offer a draw with the move (D1322).
        offer_draw: bool,
        /// When the user did it.
        at: Instant,
    },
    /// Drop the premove.
    CancelPremove,
    /// Resign.
    Resign,
    /// Abort the game.
    Abort,
    /// Offer a draw.
    OfferDraw,
    /// Accept (`true`) or decline the draw offered by the opponent.
    AnswerDraw(bool),
    /// Accept (`true`) or decline the takeback proposed by the opponent.
    AnswerTakeback(bool),
    /// Claim the victory over an absent opponent, at the instant `at`:
    /// only valid once the wait the server announced is over (D3116).
    ClaimVictory {
        /// When the user asked.
        at: Instant,
    },
    /// Claim a draw over an absent opponent (D3011), checked as the victory.
    ClaimDraw {
        /// When the user asked.
        at: Instant,
    },
    /// Propose a takeback of the last move of the user (D3011).
    ProposeTakeback,
    /// Challenge the opponent of the finished game again, with the same
    /// time control and the colours swapped (D3012).
    Rematch,
    /// Watch a game of other players: a game id or a link (D3000).
    Watch(String),
    /// Stop watching and put the previous game back.
    StopWatching,
    /// Log in through the browser (OAuth + PKCE, D3005); the token goes to
    /// `store` once it works.
    StartOAuth {
        /// Where to keep the token.
        store: StoreChoice,
    },
    /// Give up the login in the browser.
    CancelOAuth,
    /// Leave the result of the finished game and go back to idle.
    DismissResult,
    /// Put back the game that was in the editor before the online game.
    RestorePrevious,
    /// Release the lock without network (D1332); `confirmed` is the answer
    /// to "I understand...".
    RemoveLock {
        /// The user confirmed.
        confirmed: bool,
    },
    /// Read the marker of a game in progress (the front-end sends it right
    /// after `Startup`).
    LoadMarker,

    // ---- answers of the tasks ----
    /// A command that has nothing to report (what the streams say comes as
    /// [`OnlineMessage::Stream`]).
    Ack,
    /// The connection attempt finished.
    Connected {
        /// The epoch of the attempt.
        epoch: u64,
        /// The account or why not.
        result: Result<AccountView, OnlineError>,
    },
    /// A stream said something.
    Stream(StreamUpdate),
    /// An action finished.
    Done(ActionDone),
    /// The marker was read.
    MarkerRead(Option<MarkerInfo>),
    /// The grace period for the marker ended (D1332).
    MarkerGrace(u64),
    /// The authorization page is ready (D3009).
    OAuthUrl {
        /// The epoch of the attempt.
        epoch: u64,
        /// The page, for the user to open by hand if needed.
        url: String,
        /// The program opened it in the browser.
        opened: bool,
    },
    /// The login in the browser ended; the token, if any, is staged in
    /// the hub.
    OAuthDone {
        /// The epoch of the attempt.
        epoch: u64,
        /// Whether a token is staged, or why not.
        result: Result<(), OnlineError>,
        /// Where to keep it.
        store: StoreChoice,
    },
    /// The exported PGN of the watched game (D3003).
    WatchPgn {
        /// The epoch of the watch.
        epoch: u64,
        /// The game read from it, or why not (D3117).
        result: Result<super::watch::ParsedExport, OnlineError>,
    },
    /// A retry timer ended (D1363, D1366).
    Retry {
        /// What waited.
        kind: RetryKind,
        /// The number of the timer.
        generation: u64,
    },
}

/// What a retry timer waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RetryKind {
    /// Open the stream of the game again (D1363).
    Game,
    /// The search ended and no game started (D1366).
    Search,
}
