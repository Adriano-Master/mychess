//! Chess game databases for MyChess.
//!
//! Contents:
//! - [`si4`]: the Scid SI4 v400 format (`.si4` index, `.sn4` name base, `.sg4`
//!   game file): reading, writing byte-compatibly with Scid vs PC 4.27,
//!   conversion to [`mychess_core::Game`], journal and compaction.
//! - [`Filter`] (milestone M3, slice 3a): one byte per game, with the count
//!   kept, rank/select, negate, "to last move" and the quick text filter.
//! - [`search_header`]: the search by header (`sc_search header`), with
//!   [`HeaderSearch`] options, [`PlayerStats`] for the fallback Elo and a
//!   [`TitleProvider`] hook for the spelling file.
//! - [`ProgressSink`] and [`CancelToken`]: progress and cancellation shared by
//!   searches, imports and exports.
//! - [`import_pgn`] (milestone M3, slice 3b): PGN into an SI4 database, with an
//!   [`ImportReport`] in the text format of the Scid.
//! - [`export_pgn`]: games to a PGN file, atomically or appending, with an
//!   [`ExportReport`].
//! - [`export`] (milestone M7, slice 4a): HTML ([`export::document`]), LaTeX,
//!   EPD of the filter ([`export::epd`]), game list text and CSV
//!   ([`export::game_list`], [`export::csv`]) and the self-contained HTML
//!   viewer in volumes ([`export::viewer`]).
//! - [`PgnDatabase`] (milestone M3, slice 3c): a PGN or `.pgn.gz` file opened
//!   as a read-only database in memory (type 3), with the same ECO, signatures
//!   and name order as an imported database.
//! - [`GameSource`]: the read interface shared by [`si4::Si4Database`] and
//!   [`PgnDatabase`], used by the searches, the filter and the export.
//!
//! - [`saved_search`] (milestone M3, slice 7): named header searches saved as
//!   TOML files, and the reading of Scid `.sso` files without running Tcl.
//! - [`si4::sort_games`]: permanent sort of the index (`scidt -S`), used by
//!   `mychess-cli`.
//!
//! - [`tree`] (milestone M5, slice 1): the opening tree engine, with the
//!   statistics per move, sorting, "adjust filter", best games and the
//!   random move of the training mode.
//!
//! - [`eco`] (milestone M5, slice 7a): statistics per ECO prefix, the
//!   classification of a database ([`classify_database`], with
//!   `Si4Writer::set_eco`), [`identify_opening`] and [`find_novelty`].
//!
//! - [`search_board`], [`search_material`] and [`search_moves`] (milestone M6,
//!   slice 1a): the advanced searches by position, by material and patterns
//!   and by a sequence of moves, on the shared block-parallel scan
//!   [`scan_games`].
//!
//! - [`pattern`] (milestone M6, slice 2): tactical and structural patterns as a
//!   tree of conditions; [`search_pattern`] scans a database with one,
//!   [`hits_in_game`] finds its occurrences in the game on the board, and the
//!   presets and the storage of the user's patterns live in
//!   [`pattern::presets`] and [`pattern::store`]. The model types are not
//!   re-exported at the root (use [`pattern::model`]): the root already has
//!   the `Pattern` and `MinMax` of the material search.
//!
//! - [`crosstable`] (milestone M6, slice 4a): the tournament table, with
//!   the selection of the games of a tournament, the Auto, all-play-all,
//!   Swiss and knockout formats, the tie-breaks, the performance and the text,
//!   HTML and LaTeX output.
//!
//! - [`mod@filter_stats`], [`rating_graph`] and [`piece_tracker`] (milestone M6,
//!   slices 4b and 4c): the statistics of the filter and the histograms of
//!   the filter graphs, the rating history of a player and the piece tracker,
//!   as plain data ready to draw.
//!
//! - [`report`] (milestone M7, slice 2a): the opening report, with its
//!   statistics, theory table and the text, HTML and LaTeX documents.
//!
//! - [`maintenance`] (milestone M7, slice 1a): database statistics, check
//!   games, twin games with the side-by-side comparison, and the flags in
//!   bulk and tag sharing of the writer.
//!
//! Dependency rule: may depend only on `mychess-core` and `mychess-pgn`.

pub mod crosstable;
pub mod eco;
pub mod export;
pub mod filter;
pub mod filter_stats;
pub mod finder;
pub mod import;
pub mod maintenance;
pub mod pattern;
pub mod pgn_db;
pub mod piece_tracker;
mod platform;
pub mod player_info;
pub mod player_stats;
pub mod progress;
pub mod rating_graph;
pub mod report;
pub mod saved_search;
pub mod search;
pub mod si4;
pub mod source;
pub mod spelling;
pub mod tree;

pub use crosstable::{
    ALL_PLAY_ALL_LARGE_OUTPUT, ALL_PLAY_ALL_LIMIT, Clash as CrosstableClash,
    Columns as CrosstableColumns, Crosstable, CrosstableError, CrosstableFormat, CrosstableLabels,
    CrosstableOptions, CrosstableRequest, CrosstableSort, KnockoutMatch, KnockoutRound,
    MAX_PLAYERS as CROSSTABLE_MAX_PLAYERS, MAX_TOURNAMENT_GAMES as CROSSTABLE_MAX_GAMES,
    Notice as CrosstableNotice, Outcome as CrosstableOutcome, Performance as CrosstablePerformance,
    PlayerBio, PlayerBioProvider, PlayerRow, ResultCounts, TableMode, TournamentGame,
    TournamentGames, TournamentReference, TournamentSelection, build_crosstable, fide_category,
    load_games, load_games_cancellable, opponent_elo, parse_round, performance_rating,
    rating_change, render_html, render_latex, render_text, select_tournament,
};
pub use eco::{
    ClassifyError, ClassifyReport, ClassifySelection, EcoChild, EcoScope, EcoStats, EcoStatsTable,
    InvalidPrefix, Novelty, NoveltyError, NoveltyOptions, OpeningMatch, bar_scale,
    classify_database, classify_preview, find_novelty, identify_opening,
};
pub use export::{
    ExportError, ExportItem, ExportOptions, ExportReport, ExportSkipReason, csv::CsvColumn,
    csv::CsvOptions, csv::export_game_csv, document::HtmlExportOptions,
    document::LatexExportOptions, document::export_html, document::export_latex,
    epd::EpdExportOptions, epd::EpdExportReport, epd::EpdItem, epd::export_epd, export_pgn,
    game_list::GameListOptions, game_list::export_game_list, game_list::format_game_line,
    viewer::ViewerAssets, viewer::ViewerOptions, viewer::ViewerReport, viewer::viewer_volumes,
    with_pgn_extension,
};
pub use filter::{
    FILTER_LAST_MOVE, Filter, FilterOp, MAX_UNREADABLE, TextFilterReport, TextFindReport,
    TextMatcher,
};
pub use filter_stats::{
    FilterGraphConfig, FilterStatsRow, GraphKind, Histogram, HistogramBin, HistogramView,
    MAX_GRAPH_BINS, MAX_GRAPH_FIELD, OPEN_YEAR_LIMIT, StatsConfig, StatsError, StatsLine,
    StatsQuery, ViewPoint, filter_histogram, filter_stats, filter_stats_rows, filter_stats_table,
};
pub use import::{
    ImportError, ImportNotice, ImportNoticeKind, ImportOptions, ImportReport, ImportStop,
    PlyCountTag, ScidFlagsTag, SkipReason, import_pgn, import_pgn_file,
};
pub use maintenance::{
    CheckCause, CheckIssue, CheckReport, CleanerCounts, CleanerReport, CleanerStep,
    CompactionStats, DatabaseStats, DeleteWhich, DiffToken, DuplicateCriteria, DuplicateReport,
    DuplicateScan, GameDiff, MaintError, NameFrequencies, NameUsage, RepairError, RepairOptions,
    RepairReport, Scope as MaintScope, SharedField, SharedTag, StepRecord, StepStatus, TagCount,
    TagFilter, TagListing, TwinPair, check_games, compaction_stats, database_stats, diff_games,
    filter_by_tag, find_duplicates, find_duplicates_with_limit, games_in_scope, list_extra_tags,
    name_frequencies, name_usage, repair_database, share_tags_preview,
};
pub use pattern::{
    Hit as PatternHit, PatternError, PatternFile, PatternReport, PatternSummary,
    ScanCost as PatternScanCost, ScanError as PatternScanError, ScanLimits as PatternScanLimits,
    delete_pattern, delete_pattern_file, estimate_cost as estimate_pattern_cost, export_pattern,
    hits_in_game, hits_in_game_with, hits_in_line as pattern_hits_in_line, import_pattern,
    list_patterns, load_pattern, load_pattern_named, name_exists as pattern_name_exists,
    save_pattern, search_pattern, search_pattern_with,
};
pub use pgn_db::{
    DEFAULT_MAX_BYTES, DEFAULT_MAX_GAMES, OpenOptions, OpenReport, PgnDatabase, PgnOpenError,
    READ_ONLY_DESCRIPTION, UnsupportedKind,
};
pub use piece_tracker::{
    ColorScheme, PieceTrackQuery, PieceTrackResult, RankedSquare, TRACK_BLOCK_GAMES, TrackMode,
    TrackerGrid, piece_track, piece_track_plies,
};
pub use player_stats::PlayerStats;
pub use progress::{CancelToken, NoProgress, ProgressSink};
pub use rating_graph::{
    MAX_RATING_PLAYERS, RatingOptions, RatingPlayer, RatingSeries, rating_histories,
    rating_history, rating_x,
};
pub use saved_search::{
    AdvancedQuery, AnySsoReport, MAX_FILE_BYTES, MAX_NAME_CHARS, MaterialSsoReport, SavedAdvanced,
    SavedSearch, SavedSearchEntry, SavedSearchError, SavedSearchList, SkippedFile, SsoReport,
    SsoWarning, SsoWarningKind, delete_advanced, file_stem_for, import_any_sso,
    import_any_sso_bytes, import_material_sso, import_material_sso_bytes, import_sso,
    import_sso_bytes, list_advanced, load_advanced, save_advanced,
};
pub use search::{
    BLOCK_BYTES, BLOCK_GAMES, BishopColors, BoardMatch, BoardSearch, CheckTest, CountAdjust,
    EcoRange, EndingPreset, FlagFilter, GameEndKind, GameMatcher, HeaderSearch, MAX_MOVE_TOKENS,
    MAX_PATTERNS, MaterialSearch, MinMax, MoveToken, MovesSearch, Pattern, PatternPreset,
    PieceRange, ResultSet, SearchError, SearchReport, Shortcut, SideFilter, SideRanges, Title,
    TitleProvider, TitleSet, Tri, complete_max_date, complete_min_date, material_presets,
    pattern_presets, scan_games, search_board, search_header, search_material, search_moves,
};
pub use source::GameSource;
