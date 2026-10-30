//! The XBoard / WinBoard (CECP) protocol: `feature` negotiation, the engine
//! quirk table, thinking-line parsing, command formatting and the session
//! state machine (spec m4-2).

pub(crate) mod features;
pub(crate) mod format;
pub(crate) mod handshake;
pub(crate) mod quirks;
pub(crate) mod session;
pub(crate) mod thinking;
