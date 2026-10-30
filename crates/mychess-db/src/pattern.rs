//! Tactics and pattern search (milestone M6, slice 2; `docs/specs/m6-2-taticas-e-padroes.md`).
//!
//! A [`Pattern`] is a tree of [`Condition`]s (AND / OR / NOT around leaves
//! such as "a white rook pins a black knight against the king"), evaluated on
//! the positions of the main line of a game. It is data, not a language: the
//! tree is edited visually, saved as TOML and built by presets (ADR 0009).
//!
//! Slice a is the model and the evaluation; slice b adds the scan of a
//! database, the presets and the storage of the user's patterns.
//!
//! - [`model`]: the types, their validation and the TOML file format;
//! - [`eval`]: the three-state evaluator on one position and on a game line,
//!   the match rules (range, `hold`, `end_only`, mirror) and the highlights;
//! - [`scan`]: [`search_pattern`] over a database on the shared block-parallel
//!   scan, and [`hits_in_game`] for the game on the board;
//! - [`presets`]: the catalogue of ready-made patterns;
//! - [`store`]: the patterns of the user as TOML files in a directory.

pub mod eval;
pub mod model;
pub mod presets;
pub mod scan;
pub mod store;

pub use eval::{
    Frame, Highlight, HighlightKind, HighlightRole, Line, LineMatch, MAX_FRAMES, Tri,
    eval_position, fold_constants, highlights_at, match_line, matching_plies, region_mask,
};
pub use model::{
    BishopColors, CastleKind, Condition, Counts, DiscoveredCond, DiscoveredKind, DoubleAttackCond,
    Feature, FeatureCond, Flag, Gain, MAX_DEPTH, MAX_NODES, MAX_PATTERN_FILE_BYTES, MateCond,
    MateMode, MaterialCond, MinMax, MoveCond, NamedRegion, Outcome, PATTERN_FORMAT,
    PATTERN_VERSION, Pattern, PatternError, PatternFile, Phase, PieceCond, PieceSel, Region,
    RelKind, RelationCond, ResultKind, RoleName, SacrificeCond, Scope, Side, Sq, Status, TargetSel,
    When, WhichMove, Who,
};
pub use scan::{
    CONFIRM_POSITIONS, Hit, POSITIONS_PER_GAME, PatternReport, ScanCost, ScanError, ScanLimits,
    estimate_cost, hits_in_game, hits_in_game_with, hits_in_line, search_pattern,
    search_pattern_with,
};
pub use store::{
    MAX_PATTERNS, PatternSummary, delete_pattern, delete_pattern_file, export_pattern,
    import_pattern, list_patterns, load_pattern, load_pattern_named, name_exists, save_pattern,
};
