//! Playing on Lichess (spec M8-3, phase b): the state machine of the
//! session, the hub that talks to `mychess-online`, the lock of the
//! assistance during a game, the conversion of the game to the editor and
//! the premove.
//!
//! The front-end never sees the types of `mychess-online`: it sends
//! [`OnlineMessage`]s, reads [`OnlineState`] and runs the
//! [`Effect::Online`](crate::Effect::Online) effects through
//! [`perform`](crate::perform).

mod assist;
mod browser;
mod commands;
mod game_builder;
mod message;
mod prefs;
mod premove;
mod session;
mod state;
#[cfg(test)]
mod state_tests;
mod watch;

pub use assist::{Family, MARKER_FILE, family_of, marker_path, read_marker};
pub(crate) use browser::{open_path, open_url};
pub(crate) use commands::perform_online;
pub use commands::{OnlineCommand, OnlineEvents, OnlineHub};
pub(crate) use game_builder::last_node;
pub use game_builder::{
    EndKind, OnlineGame, Outcome as OnlineOutcome, PlayerInfo, SITE_PREFIX, StartError, SyncError,
    clk_comment,
};
pub use message::{
    AccountView, ActionKind, ChallengeRequest, ConnectRequest, MarkerInfo, OnlineColor,
    OnlineError, OnlineMessage, OnlineNotice, OnlineVariant, RejectReason, RetryKind, SeekRequest,
    StoreChoice, TokenSource,
};
pub use prefs::{
    DEFAULT_BASE_URL, OnlinePrefs, SaveTo, TokenStorePref, base_url_is_acceptable,
    challenge_time_is_valid, login_is_valid, seek_time_is_valid,
};
pub use premove::{Premove, PremoveSlot, is_pseudo_legal};
pub use session::{Phase as OnlinePhase, PlayingState};
pub(crate) use state::{Ctx, Out};
pub use state::{
    GAME_RETRY_DELAY, IncomingChallenge, Link, MARKER_GRACE, OAuthWait, OnlineState, OpponentGone,
    SEARCH_WATCHDOG, Watch,
};
pub use watch::{ParsedExport, WatchPhase, WatchPlayerView, WatchedGame};
