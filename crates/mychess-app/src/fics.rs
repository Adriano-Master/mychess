//! Playing on FICS (spec `docs/specs/m8-9-fics.md`, milestone M8-9b): the
//! state machine of the session, the hub that talks to the server, the
//! console, the seek graph, the games (played, observed, examined), the
//! premove and the lock of the assistance during a game.
//!
//! The front-end never sees the types of `mychess-online`: it sends
//! [`FicsMessage`]s, reads [`FicsState`] and runs the
//! [`Effect::Fics`](crate::Effect::Fics) effects through
//! [`perform`](crate::perform). The protocol is in
//! `mychess_online::fics` (M8-9a).

mod console;
mod game;
mod hub;
mod message;
mod seeks;
mod state;
#[cfg(test)]
mod state_tests;

pub use console::{Console, ConsoleLine, LineKind, MAX_LINES};
pub use game::{Applied, FicsGame, MAX_PLIES, Role};
pub(crate) use hub::perform_fics;
pub use hub::{FicsEvents, FicsHub};
pub use message::{
    AutoPromote, ConnectRequest, FicsCommand, FicsMessage, FicsNotice, FicsPrefs, NetUpdate, Offer,
};
pub use mychess_online::fics::{
    ColorChoice, Disconnect, FicsResult, LoginFailure, Seek, SeekColor, SeekSpec,
};
pub use seeks::{MAX_SEEKS, PLAYABLE_KINDS, SeekGraph, estimated_minutes, is_playable};
pub use state::{FicsState, LECTURE_CHANNEL, MAX_OBSERVED, Out, Phase};
