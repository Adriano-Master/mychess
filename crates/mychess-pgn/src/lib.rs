//! PGN (Portable Game Notation) and EPD support for MyChess.
//!
//! Contents (milestone M1, slice 3 in `docs/roteiro.md`):
//! - [`read`]: a tolerant, streaming PGN reader modelled on Scid's
//!   `pgnparse.cpp` (see ADR 0006 and `docs/specs/m1-3-pgn.md`);
//! - [`mod@write`]: a PGN writer with Scid's export options.
//!
//! - [`eco_file`]: reading `.eco` opening files (slice 4); EPD follows in the same slice.
//!
//! Dependency rule: may depend only on `mychess-core`.

pub mod eco_file;
pub mod epd;
pub mod export;
pub mod read;
pub mod write;
