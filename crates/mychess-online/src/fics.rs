//! A client for the Free Internet Chess Server (FICS), spec
//! `docs/specs/m8-9-fics.md` (milestone M8-9a).
//!
//! FICS is a text protocol over TCP (`freechess.org:5000`), nothing like the
//! HTTP API of Lichess, so it lives apart from [`crate::client`] but in the
//! same crate: it shares the layer (Model, depends only on `mychess-core`),
//! the reconnection helpers and the fake-server style of testing.
//!
//! * [`framer`]: telnet-style line framing, bounded, never failing;
//! * [`style12`]: the `<12>` board line;
//! * [`event`]: what a line means ([`FicsEvent`]), pure and total;
//! * [`command`]: checked command lines ([`Command`]);
//! * [`client`]: the connection task ([`connect`]);
//! * `testing` (feature `testing`): a local fake FICS server.
//!
//! Rules: everything from the server is untrusted (bounded lines, cleaned
//! text, no panic); the password is never logged, echoed or stored;
//! no automatic test touches the real server.

pub mod client;
pub mod command;
pub mod event;
pub mod framer;
pub mod style12;
#[cfg(feature = "testing")]
pub mod testing;

pub use client::{
    DEFAULT_HOST, DEFAULT_PORT, Disconnect, FicsConfig, FicsError, FicsEvents, FicsMessage,
    FicsSender, Login, Password, connect,
};
pub use command::{ColorChoice, Command, CommandError, SeekSpec};
pub use event::{
    FicsEvent, FicsResult, GameSummary, LoginFailure, MatchOffer, Seek, SeekColor, parse_line,
};
pub use framer::LineFramer;
pub use style12::{Relation, Style12, VerboseMove};
