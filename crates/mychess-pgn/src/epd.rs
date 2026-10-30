//! EPD (Extended Position Description) books.
//!
//! An EPD file holds one position per line, followed by *operations*:
//!
//! ```text
//! r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - bm Bb5; id "ruy";
//! ```
//!
//! [`EpdBook`] models what the Scid's `PBook` does for `.epd` files (see
//! `docs/specs/m1-4-epd-eco.md`, part A): records keyed by [`Position`]
//! (clocks ignored), kept in file order, where a repeated position replaces
//! the earlier data but keeps its place (decisions D40 to D47).
//!
//! - [`EpdBook::read`] / [`EpdBook::read_file`] read in streaming fashion and
//!   report problems per line as [`EpdDiagnostic`]s instead of failing;
//! - [`EpdBook::write`] / [`EpdBook::write_file`] write one line per record;
//!   reading what was written gives back the same book;
//! - [`EpdOptions::scid_escapes`] switches to the Scid dialect (`\s` for a
//!   semicolon, `\\` for a backslash, no quote tracking).
//!
//! [`Position`]: mychess_core::Position

mod book;
mod error;
mod find;
mod reader;
mod record;
mod text;
mod writer;

pub use book::{EpdBook, EpdIndex, InsertResult};
pub use error::{EpdDiagnostic, EpdDiagnosticKind, EpdError};
pub use reader::{EpdLoad, EpdOptions};
pub use record::{EpdOperation, EpdRecord};
pub use text::TextWarning;
