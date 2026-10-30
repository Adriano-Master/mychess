//! Polyglot opening books for MyChess (`.bin`, 16-byte big-endian records).
//!
//! This is the core of milestone M5-5: reading and querying books, editing
//! weights, moves and lines, building a book from games and merging two
//! books, all natively (Scid relies on the external `polyglot` program for
//! the last two). There is no user interface here; the book window and the
//! use of books while playing come later (M5-6).
//!
//! Layering: this crate depends only on `mychess-core`. It has no PGN
//! dependency, so [`BookBuilder`] receives games already as moves plus a
//! result. Books are read whole into memory (no `mmap`, no `unsafe`) and saved
//! atomically. Specification: `docs/specs/m5-5-livros-polyglot.md`.
//!
//! ```
//! use mychess_book::Book;
//! use mychess_core::Position;
//!
//! let pos = Position::new();
//! let mut book = Book::new();
//! let e4 = pos.parse_uci("e2e4")?;
//! book.set_position_moves(&pos, &[(e4, 100)])?;
//! assert_eq!(book.moves(&pos)[0].percent, 100);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod book;
mod build;
mod codec;
mod edit;
mod error;
mod file;
mod format;
mod merge;
mod random;
mod zobrist;

pub use book::{Book, BookMove, ValidationReport};
pub use build::{BookBuilder, BuildOptions, BuildReport};
pub use codec::{code_to_text, decode_move, encode_move};
pub use edit::{MAX_MOVES_PER_POSITION, weights_from_percent};
pub use error::BookError;
pub use file::MAX_FILE_SIZE;
#[doc(hidden)]
pub use file::SaveFault;
pub use format::{Entry, MAX_WEIGHT, RECORD_SIZE};
pub use merge::{MergeMode, MergeReport, merge};
pub use zobrist::polyglot_key;
