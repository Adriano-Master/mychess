// Derived from Scid vs PC 4.27, tcl/windows/tree.tcl (`::tree::mask`,
// 1774-3013). Copyright (c) 1999-2002 Shane Hudson; Scid vs PC contributors.
// Scid is distributed under the GNU General Public License (see its
// COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! Tree masks (`docs/specs/m5-4-mascaras-de-arvore.md`, decisions D1800 to
//! D1849; file format in `docs/formatos/stm.md`): per-position colors, NAGs,
//! markers and comments for the opening tree, stored in the `.stm` file of
//! the Scid vs PC.
//!
//! - [`TreeMask`] is the data; [`MaskKey`] is the short FEN of a position.
//! - [`stm`] reads (never executing anything) and writes (atomically) the
//!   file.
//! - [`TreeMask::apply`] edits ([`MaskOp`]); [`TreeMask::overlay`] and
//!   [`TreeMask::lookup_move`] are the queries of the tree window.
//! - [`fill`] copies games into a mask, [`search`] searches it and
//!   [`outline`] shows its lines.
//! - [`MoveMatcher`] compares move texts (D1805) and [`mask_san`] writes the
//!   text of a new move (D1806).

pub mod fill;
mod matching;
mod model;
mod ops;
pub mod outline;
pub mod search;
pub mod stm;
mod tcl;

pub use fill::{FillError, FillReport, FillSide, feed_filter, feed_game, feed_line};
pub use matching::{MoveMatcher, bare_move_text, mask_san, normalize_move_text, resolve_move};
pub use model::{
    MAX_COMMENT_BYTES, MAX_FIELD_BYTES, MAX_MOVES_PER_POSITION, MAX_POSITIONS, MAX_PREVIEW_BYTES,
    Marker, MaskColor, MaskInsertError, MaskKey, MaskMove, MaskNag, MaskPosition, MaskValueError,
    NULL_MOVE, TreeMask, clean_comment,
};
pub use ops::{MarkerSlot, MaskOp, MaskOverlay, OnlyMaskMove};
pub use outline::{MAX_OUTLINE_DEPTH, Outline, OutlineBudget, OutlineNode, OutlineStep};
pub use search::{MaskHit, MaskHits, MaskQuery};
pub use stm::{StmError, StmReport, StmWarning, StmWarningKind, StmWrite};
