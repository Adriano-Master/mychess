//! Online play for MyChess: a client for the Lichess Board API (spec
//! `docs/specs/m8-3-lichess-online.md`, ADR 0010).
//!
//! Layer: Model. This crate depends only on `mychess-core` (no iced, no app
//! types) and speaks to the network; `mychess-app` wraps its values in
//! messages and runs its calls as effects. The GUI never sees it.
//!
//! Contents (milestone M8-3a):
//! * [`models`]: tolerant decoders for the account, events, games and
//!   challenges; unknown fields and values never break a stream;
//! * [`ndjson`]: [`ndjson::LineFramer`], newline framing over arbitrary chunks;
//! * [`client`]: [`Client`] with `Bearer` auth, `https`-only, no redirects,
//!   bounded bodies and the [`RequestGate`] (one request at a time, 429
//!   block with backoff, urgent lane for moves);
//! * [`streams`]: event, game, seek and challenge streams with silence
//!   watch and reconnection;
//! * [`oauth`]: login by Authorization Code + PKCE through a loopback
//!   callback (spec D3005);
//! * [`backoff`], [`time_control`], [`token`] (token storage in the system
//!   keychain, a 0600 file or memory) and [`error`];
//! * `testing` (feature `testing`): a local fake Lichess for tests.
//!
//! Rules: the token never appears in `Debug`, logs, errors or URLs; external
//! input (JSON, network) never panics; no automatic test touches `lichess.org`.
//!
//! TLS: `rustls` with the `ring` provider (installed on first use), chosen
//! over `aws-lc-rs` because the latter carries the OpenSSL license (spec P2).

pub mod backoff;
pub mod client;
pub mod error;
pub mod fics;
pub mod gate;
pub mod models;
pub mod ndjson;
pub mod oauth;
pub mod streams;
#[cfg(feature = "testing")]
pub mod testing;
pub mod time_control;
pub mod token;

pub use client::{AccountInfo, Client, ClientConfig, GameAction, parse_game_ref};
pub use error::{Error, Result};
pub use gate::{GateConfig, Priority, RequestGate};
pub use streams::{
    ChallengeHandle, EventStream, GameStream, SeekHandle, SeekOutcome, StreamConfig, StreamHandle,
    StreamMessage, WatchStream,
};
#[cfg(feature = "keyring-store")]
pub use token::KeyringStore;
pub use token::{FileStore, MemoryStore, Token, TokenStore};
