//! Scid vs PC 4.27 databases: reading the SI4 v400 format.
//!
//! A database is three files with the same base name (layout in
//! `docs/formatos/si4.md`, specification in `docs/specs/m3-1-leitura-si4.md`):
//!
//! - `.si4`: a 182-byte header and one 47-byte entry per game
//!   ([`Header`], [`IndexEntry`]);
//! - `.sn4`: the names of players, events, sites and rounds
//!   ([`NameBase`]);
//! - `.sg4`: the game records, in blocks of 131,072 bytes (moves, variations,
//!   NAGs, comments, extra tags).
//!
//! [`Si4Database`] opens them read-only and decodes games. The move codec
//! depends on Scid's piece list, so it has its own minimal position
//! ([`ListPosition`]) and does not use the move generator of the core; the
//! result is converted to a [`mychess_core::Game`] afterwards, checking that
//! every move is legal.
//!
//! Layers of the reader, from bytes to games:
//!
//! | Module | Content |
//! |---|---|
//! | `bytes` | big-endian readers over `&[u8]` with bounds checks |
//! | `header`, `entry` | the `.si4` header and entries, flags, dates, ratings |
//! | `names` | the front-coded `.sn4` |
//! | `gfile` | positioned reads of `.sg4` records |
//! | `position`, `moves` | the piece list and the move codec |
//! | `record`, `raw` | the record into a raw tree, with an explicit stack |
//! | `convert`, `database` | the core `Game` and [`Si4Database`] |
//!
//! Text is kept as raw bytes and converted at the border with
//! [`bytes_to_string`] (UTF-8 if valid, else Windows-1252).

#![cfg_attr(
    test,
    allow(clippy::cast_possible_truncation, clippy::decimal_bitwise_operands)
)]

mod bytes;
pub mod compact;
mod convert;
mod database;
pub mod derive;
mod entry;
mod entry_write;
mod error;
mod gfile;
mod header;
mod header_write;
pub mod journal;
mod move_write;
mod moves;
mod names;
pub mod names_write;
mod position;
mod prepare;
mod raw;
mod record;
mod record_write;
pub mod repair;
pub mod si3;
pub mod sort;
pub(crate) mod stored_lines;
mod text;
pub mod write;

pub use bytes::{read_u16_be, read_u24_be, read_u32_be};
pub use compact::{CompactGamesReport, CompactNamesReport, compact_games, compact_names};
pub use convert::ConversionReport;
pub(crate) use convert::build_game;
pub use database::Si4Database;
pub use derive::{MainLineData, derive_mainline};
pub use entry::{GameFlags, IndexEntry, MaterialSignature};
pub use entry_write::FLAG_LETTERS;
pub use error::{
    IndexEntryError, MoveDecodeError, NameSection, RecordError, Si4Error, Si4File,
    StartPositionError,
};
pub use gfile::BLOCK_SIZE;
pub use header::{ENTRY_LEN, HEADER_LEN, Header, MAX_GAMES, SUPPORTED_VERSION};
pub use header_write::{HeaderBytes, MAX_DESCRIPTION_LEN};
pub use journal::{FailPoint, Recovery, recover};
pub use move_write::{EncodedMove, encode_move};
pub use moves::decode_move;
pub use names::{NAME_HEADER_LEN, NameBase, NameKind};
pub use names_write::{MAX_NAME_LEN, NAME_MAX_ID, NameBuilder};
pub use position::ListPosition;
pub use prepare::{NAME_FIELDS, PreparedGame};
pub use raw::{RawGame, RawLine, RawMove, RawNode};
pub use record::{MAX_EXTRA_TAGS, MAX_VARIATION_DEPTH, mainline_of_record};
pub(crate) use record::{extra_tags_of_record, start_fen_of_record, walk_mainline};
pub use record_write::{
    EncodedRecord, MAX_RECORD_LEN, MAX_WRITE_TAGS, encode_record, raw_from_game,
};
pub use repair::{
    RepairError, RepairOptions, RepairReport, repair_database, repair_database_with_hook,
};
pub use sort::{
    MAX_SORT_KEYS, SortKey, SortKeyError, SortReport, sort_games, sort_games_controlled,
};
pub use text::bytes_to_string;
pub use write::{
    FlagOp, MAX_TAG_VALUE, NameChange, NameChangeReport, NameEdit, NameEditError, NameEditReport,
    NameEditTarget, NameScope, RewriteReport, SharedField, SharedTag, Si4Writer, SideRating,
    StripReport, StripWhat, TagOp, TagReport, life_span_allows, plan_share_tags,
};

/// Result alias of this module.
pub use error::Result;
