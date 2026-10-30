//! Application layer of MyChess: the framework-independent controller.
//!
//! It owns the application state (open databases, the game being edited,
//! analysis sessions, preferences) and implements every use case as an
//! `update(state, message) -> effects` step. Effects such as file I/O, engine
//! processes and timers are described as data and executed by the front-end,
//! so every use case can be tested without opening a window.
//!
//! Dependency rule: may depend on the domain crates (`mychess-core`,
//! `mychess-pgn`, `mychess-db`, `mychess-engine`), never on `mychess-gui` or
//! on a GUI toolkit.
//!
//! # Layout
//!
//! This file only wires modules and re-exports the public API:
//!
//! - `message`: the top-level [`Message`] enum (one variant per slice);
//! - `effect`: the top-level [`Effect`] enum and [`perform`]. *Host effects*
//!   (file dialogs, clipboard, exit; see [`Effect::runs_on_host`]) are run by
//!   the front-end, the others by [`perform`];
//! - `state`: [`AppState`] and the pure [`update`];
//! - `session`: the game editing session ([`GameSession`]);
//! - `setup`, `clipboard`, `import`, `files`, `game_info`: set up position,
//!   copy and paste, PGN import, opening and saving PGN files, game
//!   information (M2-2);
//! - `prefs`, `shortcuts`: preferences in TOML and the keyboard map (M2-2);
//! - `bases`: the eight database slots and the Clipbase, each with its
//!   database, filter, list state and current game (M3-4, slice 4a). The
//!   game list rows come from [`list_page`].

// Tests compare lengths and emptiness freely; the lint only adds noise there.
#![cfg_attr(test, allow(clippy::assert_is_empty))]

mod advanced_search;
mod analysis;
mod annotate;
mod bases;
pub mod board_image;
mod book;
mod bookmarks;
pub mod calvar;
pub mod charts;
mod chooser;
mod clipboard;
pub mod crosstable;
pub mod eboard;
mod eco_ui;
mod effect;
pub mod engines;
pub mod epd_window;
mod error;
mod evalbar;
pub mod export_docs;
pub mod fics;
mod file_import;
mod files;
mod finder_ui;
mod flow;
mod fsutil;
mod game_info;
mod games_export;
pub mod graphs;
mod host;
mod import;
pub mod maintenance;
pub mod manual;
mod material;
mod merge_game;
mod message;
mod nag_shortcuts;
mod nav_state;
mod notation;
pub mod online;
pub mod opening_report;
pub mod opening_trainer;
mod panel_calc;
pub mod pattern_search;
pub mod piece_tracker;
mod play;
mod player_info_ui;
pub mod player_report;
mod player_strip;
mod prefs;
pub mod report;
mod session;
mod setup;
mod shared;
mod shortcuts;
pub mod sound;
mod spelling_ui;
pub mod start;
mod state;
pub mod tablebase;
pub mod tactical_game;
pub mod tactics;
pub mod tournament;
mod training;
mod tree;

pub use advanced_search::{
    AdvancedDone, AdvancedKind, AdvancedSearchMessage, AdvancedSearchNotice, AdvancedSearchPrefs,
    AdvancedSearchView, AdvancedUi, BishopsKind, BoardKind, CheckKind, DatabaseChoice,
    MAX_MATERIAL_DIFF, MAX_MIN_PLIES, MAX_MOVE_NUMBER, MAX_PIECE_COUNT, MaterialPiece,
    MaterialView, MovesView, PATTERN_ROWS, PatternView, PositionView, PresetView, RangeView,
    RunningView, SavedCommand, SavedEntry, SavedFailure, SavedOutcome, SearchFailure, SearchOp,
    SearchSummary, SideKind, SideView,
};
pub use analysis::{
    AllLinesMode, AnalysisMessage, AnalysisNotice, AnalysisView, AnalysisViews, Display,
    ExcludeError, FormatOptions, HistoryLine, LockedPosition, MAX_HISTORY, NO_MOVES_MATE_CP,
    NumberStyle, PvLine, SearchStats, Terminal, comment_score, format_comment, format_nodes,
    format_pv, format_pv_limited, format_score, format_seconds, format_wdl, nodes_per_second,
    parse_exclusion, permille_percent, search_moves_without, short_nodes, wdl_percent,
};
pub use annotate::{
    Action as AnnotateAction, AnnotateCaps, AnnotateDialog, AnnotateMessage, AnnotateNotice,
    AnnotateOptions, AnnotateReport, AnnotationError, AnnotationProgress, ApplyStats,
    BLUNDER_RANGE, BatchSave, BatchStep, BookProbe, BookVerdict, CUTOFF_RANGE, Control,
    DEFAULT_OPENING_MOVES, DEPTH_RANGE, DIFFICULTY_TIMES_MS, Decision, INFORMANT_RANGE, Informant,
    MOVETIME_RANGE_MS, NO_MOVES_PAWNS, OBVIOUS_DEPTHS, OPENING_MOVES_RANGE,
    Outcome as AnnotateOutcome, PositionEval, ScoreFormat, Side as AnnotateSide,
    Step as AnnotateStep, WithScore, WithVars, append_tag, apply_actions, batch_step,
    classify as classify_book_move, decide as decide_annotation, difficulty as exercise_difficulty,
    exercise_comment, exercise_move, format_score_text, is_obvious, last_book_move_comment,
    out_of_book_comment, score_to_nag, skips_side,
};
pub use bases::{
    BaseError, BaseKind, BaseSlot, BaseSource, Bases, BasesMessage, BasesNotice, BrowseMessage,
    BrowseMode, BrowseRead, BrowserContent, BrowserGame, BrowserHeader, BrowserState,
    CLIPBASE_DESCRIPTION, CLIPBASE_TYPE, Clipbase, ColumnAlign, ColumnSetup, CopyEnd, CopyList,
    CopyOutcome, CopyPrompt, CopyRefusal, CopyScope, CopyTarget, DEFAULT_CLIPBASE_LIMIT, FileStamp,
    FlagOutcome, GameRow, GameStep, ListColumn, ListJobKind, ListMessage, ListState,
    MAX_BROWSER_GAMES, MAX_COLUMN_CHARS, MIN_COLUMN_CHARS, OpenControl, OpenOutcome, OpenedBase,
    PickMode, ReadyGame, Reloaded, SearchKind, SearchOutcome, SearchSpec, SharedSource, SlotId,
    SortDir, TextItem, TextKind, grid_columns, list_page, rows_of,
};
pub use book::{
    BookCancel, BookCreateOptions, BookExportOptions, BookIssue, BookLeadView, BookMergeMode,
    BookMessage, BookNotice, BookPrefs, BookRowView, BookSide, BookSideView, BookTarget, BookUi,
    BookView, BranchExported, BranchStart, CreatedBook, EXPORT_LINE_LIMIT, EXPORT_PLY_LIMIT,
    MAX_LEADING_ROWS, MAX_PLY_LIMIT, MIN_GAMES_LIMIT, MergedBook, OpeningBookView, SharedBook,
    WeightScale,
};
pub use bookmarks::{
    BOOKMARKS_VERSION, BookmarkEditor, BookmarkId, BookmarkNode, BookmarkNotice, BookmarkRow,
    Bookmarks, BookmarksIssue, BookmarksMessage, EditorMessage, Folder, GameBookmark,
    LoadedBookmarks, MAX_BOOKMARK_DEPTH, MAX_BOOKMARKS, MAX_TITLE_CHARS, NEW_FOLDER_TITLE,
    NodeKind, TreeError, bookmarks_path, default_title, parse_bookmarks, render_bookmarks,
};
pub use chooser::{ChooserMessage, GameChooser};
pub use clipboard::{
    ClipboardError, ClipboardPurpose, FEN_EXCERPT_CHARS, MAX_CLIPBOARD_BYTES, PasteFenError,
    normalize_fen_text, read_pasted_fen,
};
pub use eco_ui::{
    BAR_UNITS, BrowserScope, ClassifyChoice, ClassifyOutcome, ClassifyResult, ClassifyScope,
    EcoBar, EcoBrowserView, EcoClassifyJob, EcoClassifyView, EcoEntryRow, EcoGridLine, EcoLevel,
    EcoMessage, EcoNotice, EcoNoveltyJob, EcoNoveltyView, EcoUi, EcoWindow, NoveltyOutcome,
    SharedStats, bar_parts, grid_lines,
};
pub use effect::{Effect, SaveGuard, perform};
pub use error::{FileError, FileErrorKind, RequestId};
pub use evalbar::{
    ArrowKind, ArrowMode, ArrowPrefs, EVAL_SCALE_RANGE, EngineArrow, EngineViewPrefs,
    EvalBarMessage, EvalBarPrefs, EvalBarView, EvalSource, EvalValue, Rgb, SEQUENCE_LEN_RANGE,
    SequenceSide, TerminalResult, TrafficLight, TrafficLightPrefs, bar_fill, bar_ticks,
    engine_arrows, review_arrows, traffic_light_color, traffic_lights,
};
pub use export_docs::{
    CSV_COLUMNS, ConfirmKind, ConfirmNeeded, CsvHeaders, CsvPrefs, CsvSeparator,
    DEFAULT_LIST_FORMAT, EpdPrefs, ExportAssets, ExportConfirmView, ExportDocsJob,
    ExportDocsMessage, ExportDocsUi, ExportDocsView, ExportEdit, ExportFormat, ExportImageView,
    ExportPrefs, ExportProblem, HtmlPrefs, ImageFailure, ImageFormat, ImageJob, ImageOutcome,
    ImagePrefs, ImageStepView, LatexPrefs, MAX_DIAGRAM_PX, MAX_LIST_FORMAT_CHARS, MAX_PER_FILE,
    MAX_TEMPLATE_BYTES, TemplateKind, ViewerPrefs, csv_column_key,
};
pub use file_import::{
    FileImportJob, FileImportMessage, FileImportNotice, FileImportOutcome, FileImportReport,
    FileImportResult, FileImportTarget, FileImportUi, FileImportView, ImportLine, ImportProgress,
    MAX_REPORT_LINES, ensure_base_extension, suggest_base_path,
};
pub use files::{
    GameHeader, GameOrigin, LoadedGame, MAX_FILE_STEM_CHARS, MAX_RECENT_FILES, PgnCollection,
    PgnLoad, PgnSource, RecentFiles, SaveMode, ScanLimits, ScanOutcome, ensure_pgn_extension,
    sanitize_file_name, save_pgn, scan_pgn, suggest_file_name,
};
pub use finder_ui::{
    FinderEffect, FinderMessage, FinderNotice, FinderUi, Panel as FinderPanel, PlayerField,
    PlayerFinderView, PlayerForm, PlayerRowView, StandingView, TournamentAction, TournamentField,
    TournamentFinderView, TournamentForm, TournamentRowView,
};
pub use flow::{
    AppNotice, BrowserLanding, Command, DiscardChoice, DiscardPrompt, MovetextOutcome,
    OverwritePrompt, OverwriteReason, Pending, Replacement,
};
pub use game_info::{
    COMMENT_PREVIEW_CHARS, DateDraft, DatePart, DateTarget, ExtraLineError, FieldIssue,
    FieldIssueKind, GameInfoDialog, GameInfoMessage, GameInfoOptions, GameInfoView, IssueSeverity,
    MAX_EXTRA_TAGS, MAX_NAME_MATCHES, MAX_NAMES_PER_KIND, MAX_VALUE_BYTES, MAX_VARIATION_LINES,
    NameBook, NameKind, NameMatch, NameSource, NextMove, OpeningName, PlayerLine, PlayerSide,
    PreviousMove, RatingDraft, TagField, TagsDraft, Validation, VariationLine, game_info,
    parse_extra_tags_text,
};
pub use games_export::{
    ExportCounts, ExportDraft, ExportList, ExportScope, ExportSkipCause, GamesExportJob,
    GamesExportMessage, GamesExportNotice, GamesExportOutcome, GamesExportReport,
    GamesExportResult, GamesExportStepView, GamesExportUi, GamesExportView, MAX_SKIPPED_LINES,
};
pub use host::{PickKind, PickSpec};
pub use import::{
    ImportDialog, ImportIssue, ImportMessage, ImportMode, ImportNotice, ImportReport, ScanLimit,
    pgn_message_key,
};
pub use material::{
    Material, MaterialMode, PieceCounts, advantage, captured_along, difference, material,
};
pub use merge_game::{
    BATCH_MAX, BatchReport, DEFAULT_END_PLY, END_MOVE_CHOICES, MAX_END_PLY, MergeLoadError,
    MergeMessage, MergeNotice, MergeNumbers, MergePrefs, MergeUi, MergeView, end_ply_of_moves,
};
pub use message::Message;
pub use nag_shortcuts::{NAG_SEQUENCE_TIMEOUT, NagKey, NagShortcutBuffer, NagStep};
pub use nav_state::NavState;
pub use notation::{
    Block, BlockKey, BlockKind, Cell, CommentTarget, HeaderLine, HeaderMode, HoverPreview, Inline,
    MAX_CARD_INDENT, MAX_INDENT, MovePlace, MoveTimes, NotationDoc, NotationLayout,
    NotationOptions, NotationTable, Row, RowContent, RowKey, RowKind, build_notation,
    build_notation_with_pending, nag_text, opening_along_path,
};
pub use pattern_search::{
    ConditionGroup, ConditionKind, CostView, EditRefusal, GroupKind, HitsView, NodePath, NodeView,
    ParamValue, PatternCommand, PatternDone, PatternEdit, PatternFailure, PatternIssue,
    PatternRunSummary, PatternRunningView, PatternSearchMessage, PatternSearchNotice,
    PatternSearchPrefs, PatternSearchView, PatternTab, PatternUi, PresetEntry, RegionSlot,
    SavedPattern, Sentence, SentenceArg, patterns_dir,
};
pub use play::{
    BLUNDER_CP, BONUS_RANGE, BookChoice, BookFailure, BookSelection, COACH_DEPTH_RANGE,
    COACH_SECONDS_RANGE, CoachConfig, CoachGrade, CoachOutput, CoachSearch, CoachTracker,
    CoachVerdict, CoachView, ColorChoice, DECIDED_CP, DEFAULT_COACH_DEPTH, DEFAULT_COACH_SECONDS,
    DEPTH_TOLERANCE, EVENT as PLAY_EVENT, INACCURACY_CP, MINUTES_RANGE, MISTAKE_CP, MOVETIME_RANGE,
    MoveRecord, MoveTime, NODES_RANGE, OpeningLine, OpponentInfo, PLAY_DEPTH_RANGE, PlayBook,
    PlayClock, PlayConfig, PlayController, PlayEnd, PlayError, PlayMessage, PlayNotice, PlayOutput,
    PlayPhase, PlayRemark, PlaySetup, PlayView, QUEUE_LIMIT as COACH_QUEUE_LIMIT,
    Question as PlayQuestion, SideBudget, StartFrom, Strength, TagSetup, Think, ThinkLimits,
    TimeMode, Turn as PlayTurn, load_play_book, write_result as write_play_result,
    write_start_tags as write_play_start_tags,
};
pub use player_info_ui::{
    FilterJob, InfoDone, InfoHeader, InfoJob, InfoStats, OpeningView, PhotoFormat,
    PlayerInfoEffect, PlayerInfoMessage, PlayerInfoNotice, PlayerInfoUi, PlayerInfoView,
    PlayerPhoto, SplitView, TallyView,
};
pub use player_strip::{
    MAX_STRIP_NAME_CHARS, PlayerStripView, PlayerStrips, RatingView, build_player_strips,
};
pub use prefs::{
    AUTOPLAY_DELAY_RANGE_MS, AccentPref, AnalysisPrefs, BoardPrefs, CoordinatesPref,
    DEFAULT_COLUMNS, EditingPrefs, EncodingPref, FilesPrefs, GameInfoPrefs, GameListPrefs, General,
    LanguagePref, LayoutNode, LoadedPreferences, MAX_ANIMATION_MS, MAX_INFO_INTERVAL_MS,
    MAX_LAYOUT_DEPTH, MAX_LOG_LINES, MAX_OPENING_LINES, MAX_PLAYER_NAME_CHARS, MAX_PV_PLY,
    MAX_WINDOW_SIZE, MIN_INFO_INTERVAL_MS, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, MenuStylePref,
    NagStyle, PGN_WIDTH_RANGE, PREFERENCES_VERSION, PaneKind, PgnExportPrefs, PlayBookPref,
    PlayColorPref, PlayModePref, PlayPrefs, PlayStartPref, Playback, PlayersPrefs, PrefChange,
    Preferences, PrefsIssue, PrefsMessage, SplitAxis, ThemePref, WindowGeometry, WindowPrefs,
    load_preferences, preferences_path, render_preferences,
};
pub use session::{
    Autoplay, Candidate, EditSettings, EngineLineToAdd, GameMessage, GameSession,
    MAX_AUTOPLAY_DELAY, MIN_AUTOPLAY_DELAY, MoveEntry, MoveEntrySettings, NAG_SHORTCUTS,
    NagCategory, NagFieldError, Nav, Notice, PendingMove, PlayOptions, Prompt, ReplaceChoice,
    Revision, StripKind, UNDO_LIMIT, VariationChoice, VariationChooser, VariationTarget,
    nag_category, nag_field_text, parse_nag_field, parse_nag_list,
};
pub use setup::{
    CastlingFlags, Direction, MAX_COUNTER, MAX_FEN_HISTORY, SetupAnalysis, SetupBoard,
    SetupMessage, SetupProblem, SetupWarning, analyze_setup,
};
pub use shared::{SharedEco, SharedGame};
pub use shortcuts::{
    Action, Binding, BindingParseError, Key, KeyInput, Modifiers, NamedKey, Scope, ShortcutIssue,
    Shortcuts,
};
pub use spelling_ui::{
    AddEloFlags, AddEloPrefs, AddEloView, DEFAULT_LIST_SIZE, DEFAULT_TOURNAMENT_START, EloOutcome,
    FinderPrefs, HISTORY_MAX, LoadedSpelling, MAX_LIST_SIZE, NamesOutcome, PhotoCounts, PlannedElo,
    PlayerSortKey, PlayerToolsPrefs, ScanDone, Shared, SharedPhotos, SharedSpelling, SpellFlags,
    SpellRow, SpellRowView, SpellView, SpellcheckPrefs, SpellingCounts, SpellingEffect,
    SpellingLoadError, SpellingMessage, SpellingNotice, SpellingStatusView, SpellingUi,
    TournamentPrefs, TournamentSortKey,
};
pub use state::{AppState, GameMode, update};
pub use training::{
    AUTOMOVE_RANGE, AUTOMOVE_RETRIES, AUTOMOVE_RETRY_DELAY, AutomoveState, DEFAULT_AUTOMOVE,
    EngineMode, ModeAvailability, ModeKind, ModeMessage, ModeNotice, SEARCH_STALL_GRACE,
    StopReason, TrainingHook, TrainingOutcome, run_hooks, terminal_reason,
};
pub use tree::{
    BAR_WIDTH, BarSegments, TreeAnswer, TreeBody, TreeFastMode, TreeIssue, TreeJob, TreeMessage,
    TreePrefs, TreeRowView, TreeSort, TreeTotalView, TreeUi, TreeView, bar_segments,
};
pub use tree::{
    BEST_DEFAULT_MAX, BEST_MAX_CHOICES, BestColumn, BestMessage, BestOrder, BestResults, BestRow,
    BestUi, BestView,
};
pub use tree::{
    CACHE_SIZE_RANGE, CACHE_SIZES, CacheAnswer, CacheBusy, CacheDiscard, CacheFill, CacheInfo,
    CacheIssue, CacheJob, CacheJobKind, CacheOutcome, CacheStatus, CacheView, DEFAULT_CACHE_SIZE,
};
pub use tree::{
    FillReport, FillSide, MAX_PREVIEW_BYTES, MAX_RECENT_MASKS, Marker, MarkerSlot, MaskBusy,
    MaskCell, MaskColor, MaskEdit, MaskEffect, MaskFillAnswer, MaskFillIssue, MaskFillJob, MaskHit,
    MaskHits, MaskIssue, MaskJobKind, MaskKey, MaskLoaded, MaskMessage, MaskNag, MaskNotice,
    MaskOnlyRow, MaskOutlineState, MaskPending, MaskPrefs, MaskPrompt, MaskQuery, MaskScope,
    MaskSearchState, MaskTarget, MaskView, Outline, OutlineNode, OutlineStep, SharedMask,
    StmReport, StmWarning, StmWarningKind,
};
