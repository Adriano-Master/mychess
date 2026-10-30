//! Searches that set the [filter](crate::Filter) (milestone M3, slice 3a).
//!
//! - [`search_header`]: by header, `sc_search header` of Scid;
//! - name matching ([`HeaderSearch::white`] and friends): substring without
//!   case or spaces, or a quoted glob.

mod board;
mod header;
mod material;
mod moves;
mod names;
mod presets;
mod scan;
mod sig;

pub(crate) use names::fold_case;

pub use header::{
    EcoRange, FlagFilter, GameEndKind, HeaderSearch, MinMax, ResultSet, SearchError, SearchReport,
    Title, TitleProvider, TitleSet, Tri, complete_max_date, complete_min_date, search_header,
};

pub use board::{BoardMatch, BoardSearch, search_board};
pub use material::{
    BishopColors, MAX_PATTERNS, MaterialSearch, Pattern, PieceRange, SideRanges, search_material,
};
pub use moves::{
    CheckTest, DEFAULT_LAST_START_PLY, MAX_MOVE_TOKENS, MoveToken, MovesSearch, SideFilter,
    search_moves,
};
pub use presets::{CountAdjust, EndingPreset, PatternPreset, material_presets, pattern_presets};
pub use scan::{BLOCK_BYTES, BLOCK_GAMES, GameMatcher, Shortcut, scan_games};

#[cfg(test)]
mod tests;
