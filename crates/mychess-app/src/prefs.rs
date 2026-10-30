//! User preferences in TOML (spec M2-2, D142-D147).
//!
//! [`Preferences`] is plain data. [`load_preferences`] reads it tolerantly
//! (a wrong value only resets its own field, D144) and
//! [`render_preferences`] writes it. File access lives in `storage` and is
//! only called from [`perform`](crate::perform).

mod analysis;
mod change;
mod game_list;
mod layout;
mod play;
mod players;
mod read;
mod storage;

use std::collections::BTreeMap;
use std::path::PathBuf;

use mychess_pgn::write::{Encoding, LineWidth, NagOutput, WriteOptions};
use serde::Serialize;

use crate::error::FileErrorKind;
use crate::files::RecentFiles;
use crate::game_info::GameInfoOptions;
use crate::notation::NotationOptions;

pub use analysis::{
    AnalysisPrefs, MAX_INFO_INTERVAL_MS, MAX_LOG_LINES, MAX_PV_PLY, MIN_INFO_INTERVAL_MS,
};
pub use change::{PrefChange, PrefsMessage};
pub use game_list::{DEFAULT_COLUMNS, GameListPrefs};
pub use layout::{LayoutNode, MAX_LAYOUT_DEPTH, PaneKind, SplitAxis};
pub use play::{
    MAX_OPENING_LINES, MAX_PLAYER_NAME_CHARS, PlayBookPref, PlayColorPref, PlayModePref, PlayPrefs,
    PlayStartPref,
};
pub use players::PlayersPrefs;
pub(crate) use storage::{load_file, save_file};

/// Version of the file format written by this program.
pub const PREFERENCES_VERSION: u32 = 1;

/// Smallest window size accepted.
pub const MIN_WINDOW_WIDTH: u32 = 320;
/// See [`MIN_WINDOW_WIDTH`].
pub const MIN_WINDOW_HEIGHT: u32 = 240;
/// Largest window width or height accepted, in logical pixels.
pub const MAX_WINDOW_SIZE: u32 = 20_000;
/// Autoplay delay limits, in milliseconds.
pub const AUTOPLAY_DELAY_RANGE_MS: (u32, u32) = (100, 300_000);
/// Longest board animation, in milliseconds (0 turns it off).
pub const MAX_ANIMATION_MS: u32 = 2000;
/// Narrowest and widest PGN export line, in columns.
pub const PGN_WIDTH_RANGE: (u16, u16) = (20, 500);

/// Declares a string-valued enum with its TOML spelling.
macro_rules! text_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
        #[non_exhaustive]
        pub enum $name {
            $($(#[$vmeta])* #[serde(rename = $text)] $variant),+
        }

        impl $name {
            /// The spelling used in the TOML file.
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text),+ }
            }

            /// Reads the TOML spelling.
            pub fn from_name(text: &str) -> Option<$name> {
                match text { $($text => Some($name::$variant),)+ _ => None }
            }
        }
    };
}

text_enum! {
    /// Interface language.
    LanguagePref {
        /// Follow the system.
        System => "system",
        /// English (United States).
        EnUs => "en-US",
        /// Portuguese (Brazil).
        PtBr => "pt-BR",
    }
}

text_enum! {
    /// Colour theme of the interface.
    ThemePref {
        /// Follow the system.
        System => "system",
        /// Light.
        Light => "light",
        /// Dark.
        Dark => "dark",
    }
}

text_enum! {
    /// Accent colour of the interface (spec M9-1, D1600).
    AccentPref {
        /// The MyChess green (default).
        Mychess => "mychess",
        /// Blue.
        Blue => "blue",
        /// Purple.
        Purple => "purple",
        /// Pink.
        Pink => "pink",
        /// Red.
        Red => "red",
        /// Orange.
        Orange => "orange",
        /// Yellow.
        Yellow => "yellow",
        /// Green.
        Green => "green",
        /// Gray.
        Gray => "gray",
    }
}

text_enum! {
    /// How the menus are drawn (spec M9-2, D1620).
    MenuStylePref {
        /// Drop-down lists under a menu bar (default).
        Dropdown => "dropdown",
        /// A ribbon with tabs, groups and large icon buttons.
        Ribbon => "ribbon",
    }
}

text_enum! {
    /// Where the board coordinates are drawn.
    CoordinatesPref {
        /// Not drawn.
        None => "none",
        /// Outside the board.
        Outside => "outside",
        /// Inside the corner squares.
        Inside => "inside",
    }
}

text_enum! {
    /// How NAGs are written in exported PGN.
    NagStyle {
        /// `$1`.
        Numeric => "numeric",
        /// `!`, `?!`, ...
        Symbolic => "symbolic",
        /// Left out.
        None => "none",
    }
}

text_enum! {
    /// Text encoding of saved PGN files.
    EncodingPref {
        /// UTF-8 without a byte order mark.
        Utf8 => "utf8",
        /// UTF-8 with a byte order mark.
        Utf8Bom => "utf8-bom",
        /// ISO 8859-1.
        Latin1 => "latin1",
    }
}

/// General settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct General {
    /// Interface language.
    pub language: LanguagePref,
    /// Colour theme.
    pub theme: ThemePref,
    /// Accent colour (`general.accent`; absent means the default).
    pub accent: AccentPref,
    /// Menu style (`general.menu_style`; spec M9-2).
    pub menu_style: MenuStylePref,
    /// Whether the ribbon shows only its tabs (`general.ribbon_collapsed`).
    pub ribbon_collapsed: bool,
}

impl Default for General {
    fn default() -> General {
        General {
            language: LanguagePref::System,
            theme: ThemePref::Dark,
            accent: AccentPref::Mychess,
            menu_style: MenuStylePref::Dropdown,
            ribbon_collapsed: false,
        }
    }
}

/// Board appearance. `theme` and `piece_set` are ids defined by the board
/// slice; an unknown id is kept in the file (D146).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BoardPrefs {
    /// Board theme id.
    pub theme: String,
    /// Piece set id.
    pub piece_set: String,
    /// Where coordinates are drawn.
    pub coordinates: CoordinatesPref,
    /// Mark the side to move.
    pub show_side_to_move: bool,
    /// Highlight the last move.
    pub highlight_last_move: bool,
    /// Move animation time in milliseconds, `0..=2000` (0 turns it off).
    pub animation_ms: u32,
}

impl Default for BoardPrefs {
    fn default() -> BoardPrefs {
        BoardPrefs {
            theme: "mychess-green".to_owned(),
            piece_set: "cburnett".to_owned(),
            coordinates: CoordinatesPref::Inside,
            show_side_to_move: true,
            highlight_last_move: true,
            animation_ms: 200,
        }
    }
}

/// Autoplay settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Playback {
    /// Delay between moves in milliseconds, `100..=300000`.
    pub autoplay_delay_ms: u32,
}

impl Default for Playback {
    fn default() -> Playback {
        Playback {
            autoplay_delay_ms: 5000,
        }
    }
}

/// Editing questions (spec M2-4, D188 and D190).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EditingPrefs {
    /// Ask before replacing a move that already has continuations (Scid:
    /// `askToReplaceMoves`). Off: the move replaces the old line without a
    /// question.
    pub ask_to_replace_moves: bool,
    /// Ask before removing comments, variations or moves.
    pub confirm_destructive: bool,
    /// Ask before copying the games of one database to another (Scid:
    /// `confirmCopy`, on by default; slice 4d, D580). Copying to the
    /// Clipbase never asks.
    pub confirm_copy: bool,
}

impl Default for EditingPrefs {
    fn default() -> EditingPrefs {
        EditingPrefs {
            ask_to_replace_moves: true,
            confirm_destructive: true,
            confirm_copy: true,
        }
    }
}

/// Window geometry. The front-end checks that it fits the screen.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct WindowPrefs {
    /// Width in logical pixels, at least 320.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// Height in logical pixels, at least 240.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// Left edge, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,
    /// Top edge, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,
    /// Whether the window was maximised.
    pub maximized: bool,
}

/// A window size and position, as the front-end reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowGeometry {
    /// Width in logical pixels.
    pub width: u32,
    /// Height in logical pixels.
    pub height: u32,
    /// Left edge, if known.
    pub x: Option<i32>,
    /// Top edge, if known.
    pub y: Option<i32>,
    /// Whether the window is maximised.
    pub maximized: bool,
}

/// Files: recent list, last directories and the ECO file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct FilesPrefs {
    /// Recently opened or saved files, newest first (at most 10).
    pub recent: RecentFiles,
    /// Directory of the last Open.
    #[serde(skip_serializing_if = "path_is_unsavable")]
    pub last_open_dir: Option<PathBuf>,
    /// Directory of the last Save.
    #[serde(skip_serializing_if = "path_is_unsavable")]
    pub last_save_dir: Option<PathBuf>,
    /// The `.eco` opening file used to classify games.
    #[serde(skip_serializing_if = "path_is_unsavable")]
    pub eco_file: Option<PathBuf>,
}

/// TOML can only hold UTF-8 text: other paths are left out (D131).
#[allow(clippy::ref_option)] // the signature serde's `skip_serializing_if` wants
fn path_is_unsavable(path: &Option<PathBuf>) -> bool {
    path.as_ref().is_none_or(|p| p.to_str().is_none())
}

/// Options of the PGN writer that the user can change (D132).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PgnExportPrefs {
    /// NAG style.
    pub nags: NagStyle,
    /// Start variations on a new, indented line.
    pub indent_variations: bool,
    /// Put comments on their own indented lines.
    pub indent_comments: bool,
    /// Remove `[%...]` commands from comments.
    pub strip_marks: bool,
    /// Text encoding.
    pub encoding: EncodingPref,
    /// Line width in columns, `20..=500`.
    pub width: u16,
}

impl Default for PgnExportPrefs {
    /// Strict PGN, like `WriteOptions::default()`.
    fn default() -> PgnExportPrefs {
        PgnExportPrefs {
            nags: NagStyle::Numeric,
            indent_variations: false,
            indent_comments: false,
            strip_marks: false,
            encoding: EncodingPref::Utf8,
            width: 80,
        }
    }
}

impl PgnExportPrefs {
    /// The writer options for saving to a file (line feed endings).
    pub fn write_options(&self) -> WriteOptions {
        let nags = match self.nags {
            NagStyle::Symbolic => NagOutput::Symbolic,
            NagStyle::None => NagOutput::None,
            _ => NagOutput::Numeric,
        };
        let encoding = match self.encoding {
            EncodingPref::Utf8Bom => Encoding::Utf8Bom,
            EncodingPref::Latin1 => Encoding::Latin1,
            _ => Encoding::Utf8,
        };
        WriteOptions::default()
            .with_nags(nags)
            .with_indent_variations(self.indent_variations)
            .with_indent_comments(self.indent_comments)
            .with_strip_marks(self.strip_marks)
            .with_encoding(encoding)
            .with_width(LineWidth::Columns(self.width))
    }

    /// The writer options for the clipboard (D118): same style, unlimited
    /// width.
    pub fn clipboard_options(&self) -> WriteOptions {
        self.write_options()
            .with_width(LineWidth::Unlimited)
            .with_encoding(Encoding::Utf8)
    }
}

/// The options of the game information panel are also preferences.
pub type GameInfoPrefs = GameInfoOptions;

/// Everything the user can configure.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Preferences {
    /// Format version of the file (always [`PREFERENCES_VERSION`] once read).
    pub version: u32,
    /// Language and theme.
    pub general: General,
    /// Board appearance.
    pub board: BoardPrefs,
    /// Autoplay.
    pub playback: Playback,
    /// Questions asked while editing.
    pub editing: EditingPrefs,
    /// Window geometry.
    pub window: WindowPrefs,
    /// Recent files and directories.
    /// (Boxed so that `Message::PreferencesLoaded` stays small.)
    pub files: Box<FilesPrefs>,
    /// PGN writing.
    pub pgn_export: PgnExportPrefs,
    /// Game information panel.
    pub game_info: GameInfoPrefs,
    /// Notation panel (spec M2-4, D162).
    pub notation: NotationOptions,
    /// Player strips (spec M2-5, D239).
    pub players: PlayersPrefs,
    /// Analysis engines (spec M4-3, D431).
    /// (Boxed so that `Message::PreferencesLoaded` stays small: the enum
    /// really trips `clippy::large_enum_variant` without it, checked in D732.)
    pub analysis: Box<AnalysisPrefs>,
    /// The game against an engine (spec M8-1, D1242).
    /// (Boxed so that `Message::PreferencesLoaded` stays small.)
    pub play: Box<PlayPrefs>,
    /// The opening book of the annotation and the report (spec M5-6,
    /// D1662), in `[book]`.
    pub book: Box<crate::book::BookPrefs>,
    /// The last tournament between engines that was configured (spec M8-2,
    /// D1285), in the `[tournament]` table.
    /// (Boxed so that `Message::PreferencesLoaded` stays small.)
    pub tournament: Box<crate::tournament::TournamentConfig>,
    /// Playing on Lichess (spec M8-3, D1338), in the `[online]` table. The
    /// token is not here.
    /// (Boxed so that `Message::PreferencesLoaded` stays small.)
    pub online: Box<crate::online::OnlinePrefs>,
    /// Playing on FICS (spec M8-9, D2628), in the `[fics]` table. The
    /// password is not here.
    /// (Boxed so that `Message::PreferencesLoaded` stays small.)
    pub fics: Box<crate::fics::FicsPrefs>,
    /// The tablebases (spec M8-8, D2562), in the `[tablebase]` table.
    /// (Boxed so that `Message::PreferencesLoaded` stays small.)
    pub tablebase: Box<crate::tablebase::TablebasePrefs>,
    /// The electronic board (spec M8-10, D2740), in the `[eboard]` table.
    /// (Boxed so that `Message::PreferencesLoaded` stays small.)
    pub eboard: Box<crate::eboard::EboardPrefs>,
    /// The sounds (spec M8-11, D2804), in the `[sound]` table.
    /// (Boxed so that `Message::PreferencesLoaded` stays small.)
    pub sound: Box<crate::sound::SoundPrefs>,
    /// Evaluation bar, engine arrows and traffic lights (spec M4-5, D455).
    /// (Kept boxed: the `Box` is part of the public type that the contract
    /// tests of M4-5 read as `*prefs.engine_view`; the payload is tiny, so
    /// unboxing is a cosmetic change for a later pass.)
    pub engine_view: Box<crate::evalbar::EngineViewPrefs>,
    /// The game report and its review (spec M4-11, D1667), in `[report]`.
    pub report: crate::report::ReportPrefs,
    /// How the report is made and classified (spec M4-10, D2100), in
    /// `[report_config]`. (Boxed so that `Message::PreferencesLoaded` stays
    /// small.)
    pub report_config: Box<crate::report::ReportConfig>,
    /// The score and time graph (spec M4-7, D871).
    pub graph: crate::graphs::GraphPrefs,
    /// The opening tree panel (spec M5-2, D970).
    pub tree: crate::tree::TreePrefs,
    /// The advanced searches (spec M6-1, D3249 item 5), in
    /// `[advanced_search]`.
    pub advanced_search: crate::advanced_search::AdvancedSearchPrefs,
    /// The spelling file, the player tools and their last values (spec
    /// M6-3), in `[player_tools]`. (Boxed so that `Message::PreferencesLoaded`
    /// stays small.)
    pub player_tools: Box<crate::spelling_ui::PlayerToolsPrefs>,
    /// The crosstable (spec M6-4, D3391), in `[crosstable]`.
    pub crosstable: Box<crate::crosstable::CrosstablePrefs>,
    /// The lines of the statistics table (spec M6-4), in `[stats]`.
    pub stats: Box<crate::charts::StatsPrefs>,
    /// The graphs of the filter (spec M6-4), in `[filter_graph]`.
    pub filter_graph: Box<crate::charts::FilterGraphPrefs>,
    /// The rating graph (spec M6-4), in `[rating_graph]`.
    pub rating_graph: crate::charts::RatingPrefs,
    /// The piece tracker (spec M6-4), in `[ptracker]`.
    pub ptracker: Box<crate::piece_tracker::PieceTrackerPrefs>,
    /// The search for tactics and patterns (spec M6-2, slice c), in
    /// `[pattern_search]`. (Boxed so that `Message::PreferencesLoaded` stays
    /// small.)
    pub pattern_search: Box<crate::pattern_search::PatternSearchPrefs>,
    /// Merging a game (spec M7-6, D3868), in `[merge]`.
    /// (Boxed so that `Message::PreferencesLoaded` stays small.)
    pub merge: Box<crate::merge_game::MergePrefs>,
    /// The EPD window (spec M7-5, D3823), in `[epd]`. (Boxed so that
    /// `Message::PreferencesLoaded` stays small.)
    pub epd: Box<crate::epd_window::EpdPrefs>,
    /// The export of documents and the board image (spec M7-4, D3729), in
    /// `[export]`. (Boxed so that `Message::PreferencesLoaded` stays small.)
    #[serde(rename = "export")]
    pub export_docs: Box<crate::export_docs::ExportPrefs>,
    /// The opening report (spec M7-2, D3624), in `[opening_report]`.
    /// (Boxed so that `Message::PreferencesLoaded` stays small.)
    pub opening_report: Box<crate::opening_report::OpeningReportPrefs>,
    /// The built-in manual (spec M9-5, D4060), in `[manual]`. (Boxed so that
    /// `Message::PreferencesLoaded` stays small.)
    pub manual: Box<crate::manual::ManualPrefs>,
    /// The start-up: welcome done, tip of the day (spec M9-4), in `[start]`.
    pub start: crate::start::StartPrefs,
    /// The player report (spec M7-3, D3663), in `[player_report]`.
    /// (Boxed so that `Message::PreferencesLoaded` stays small.)
    pub player_report: Box<crate::player_report::PlayerReportPrefs>,
    /// The maintenance tools (spec M7-1, D3530), in `[maintenance]`.
    /// (Boxed so that `Message::PreferencesLoaded` stays small.)
    pub maintenance: Box<crate::maintenance::MaintenancePrefs>,
    /// The tree masks (spec M5-4, D1819), in `[tree_mask]`.
    /// (Boxed so that `Message::PreferencesLoaded` stays small.)
    #[serde(rename = "tree_mask")]
    pub mask: Box<crate::tree::MaskPrefs>,
    /// Columns of the game list (spec M3-4, D562).
    /// (Boxed so that `Message::PreferencesLoaded` stays small.)
    pub game_list: Box<GameListPrefs>,
    /// Customised shortcuts: action name to binding text (`""` turns off).
    pub shortcuts: BTreeMap<String, String>,
    /// Arrangement of the panels, if the user changed it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<LayoutNode>,
}

impl Default for Preferences {
    fn default() -> Preferences {
        Preferences {
            version: PREFERENCES_VERSION,
            general: General::default(),
            board: BoardPrefs::default(),
            playback: Playback::default(),
            editing: EditingPrefs::default(),
            window: WindowPrefs::default(),
            files: Box::default(),
            pgn_export: PgnExportPrefs::default(),
            game_info: GameInfoPrefs::default(),
            notation: NotationOptions::default(),
            players: PlayersPrefs::default(),
            analysis: Box::default(),
            play: Box::default(),
            book: Box::default(),
            tournament: Box::default(),
            online: Box::default(),
            fics: Box::default(),
            tablebase: Box::default(),
            eboard: Box::default(),
            sound: Box::default(),
            engine_view: Box::default(),
            report: crate::report::ReportPrefs::default(),
            report_config: Box::default(),
            graph: crate::graphs::GraphPrefs::default(),
            tree: crate::tree::TreePrefs::default(),
            advanced_search: crate::advanced_search::AdvancedSearchPrefs::default(),
            player_tools: Box::default(),
            crosstable: Box::default(),
            stats: Box::default(),
            filter_graph: Box::default(),
            rating_graph: crate::charts::RatingPrefs::default(),
            ptracker: Box::default(),
            pattern_search: Box::default(),
            merge: Box::default(),
            epd: Box::default(),
            export_docs: Box::default(),
            opening_report: Box::default(),
            manual: Box::default(),
            start: crate::start::StartPrefs::default(),
            player_report: Box::default(),
            maintenance: Box::default(),
            mask: Box::default(),
            game_list: Box::default(),
            shortcuts: BTreeMap::new(),
            layout: None,
        }
    }
}

/// A remark about the preferences file (D144).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PrefsIssue {
    /// A value has the wrong type or an unknown spelling; the default was
    /// used for that field.
    InvalidValue {
        /// Dotted key, `board.animation_ms`.
        key: String,
    },
    /// A number was outside its range and was moved to the nearest limit.
    Clamped {
        /// Dotted key.
        key: String,
    },
    /// The file was written by a newer version; what is understood was
    /// read, and a backup is kept before the file is rewritten.
    NewerVersion {
        /// The version found.
        found: u32,
    },
    /// The file could not be read as TOML; it was kept as
    /// `preferences.toml.corrupt` and the defaults are used.
    Unreadable,
    /// The preferences could not be saved.
    SaveFailed(FileErrorKind),
    /// There is no configuration directory: nothing is saved.
    NoConfigDir,
    /// A customised shortcut was not applied.
    Shortcut(crate::shortcuts::ShortcutIssue),
}

impl PrefsIssue {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            PrefsIssue::InvalidValue { .. } => "prefs-issue-invalid-value",
            PrefsIssue::Clamped { .. } => "prefs-issue-clamped",
            PrefsIssue::NewerVersion { .. } => "prefs-issue-newer-version",
            PrefsIssue::Unreadable => "prefs-issue-unreadable",
            PrefsIssue::SaveFailed(_) => "prefs-issue-save-failed",
            PrefsIssue::NoConfigDir => "prefs-issue-no-config-dir",
            PrefsIssue::Shortcut(issue) => issue.message_key(),
        }
    }
}

/// The preferences read from a file and the remarks about them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LoadedPreferences {
    /// The preferences (defaults for whatever could not be read).
    pub prefs: Preferences,
    /// What went wrong, if anything.
    pub issues: Vec<PrefsIssue>,
}

/// Where the preferences file lives: `preferences.toml` in the user's
/// configuration directory, or `None` when there is none (D142).
pub fn preferences_path() -> Option<PathBuf> {
    crate::start::config_dir().map(|dir| dir.join("preferences.toml"))
}

/// Reads preferences from TOML text, tolerantly (D144). Never fails.
///
/// ```
/// use mychess_app::load_preferences;
///
/// let loaded = load_preferences("[board]\nanimation_ms = \"fast\"\n");
/// assert_eq!(loaded.prefs.board.animation_ms, 200);
/// assert_eq!(loaded.issues.len(), 1);
/// ```
pub fn load_preferences(text: &str) -> LoadedPreferences {
    read::load(text)
}

/// Why the preferences cannot be rendered.
#[derive(Debug, thiserror::Error)]
pub(crate) enum RenderError {
    /// The serializer refused a value.
    #[error("the serializer refused a value: {0}")]
    Serialize(#[from] toml::ser::Error),
    /// The text does not read back (the serializer writes an integer above
    /// `i64::MAX` as it is, which no TOML reader accepts).
    #[error("the rendered text is not valid TOML: {0}")]
    Unreadable(#[from] toml::de::Error),
}

/// Renders preferences as TOML text, or says why that cannot be done. The
/// text is read back before it is returned, so a file written from it always
/// loads. Saving uses this one: a failed rendering never replaces the file
/// with an empty or unreadable text (D1310).
pub(crate) fn try_render_preferences(prefs: &Preferences) -> Result<String, RenderError> {
    let text = toml::to_string(prefs)?;
    text.parse::<toml::Table>()?;
    Ok(text)
}

/// Renders preferences as TOML text. A value the serializer refuses gives
/// an empty text (which loads as the defaults) and a log line; code that
/// writes a file uses `try_render_preferences` instead.
pub fn render_preferences(prefs: &Preferences) -> String {
    try_render_preferences(prefs).unwrap_or_else(|error| {
        tracing::error!(%error, "could not render the preferences");
        String::new()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_gives_defaults_without_issues() {
        let loaded = load_preferences("");
        assert_eq!(loaded.prefs, Preferences::default());
        assert!(loaded.issues.is_empty());
    }

    #[test]
    fn defaults_round_trip() {
        let prefs = Preferences::default();
        let loaded = load_preferences(&render_preferences(&prefs));
        assert_eq!(loaded.prefs, prefs);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn a_full_set_round_trips() {
        let mut prefs = Preferences::default();
        prefs.general.language = LanguagePref::PtBr;
        prefs.general.theme = ThemePref::Dark;
        prefs.board.theme = "custom theme".to_owned();
        prefs.board.coordinates = CoordinatesPref::Inside;
        prefs.board.animation_ms = 0;
        prefs.playback.autoplay_delay_ms = 123_456;
        prefs.editing = EditingPrefs {
            ask_to_replace_moves: false,
            confirm_destructive: false,
            confirm_copy: false,
        };
        prefs.window = WindowPrefs {
            width: Some(1200),
            height: Some(800),
            x: Some(-20),
            y: Some(30),
            maximized: true,
        };
        prefs
            .files
            .recent
            .push(PathBuf::from("dir with space/\u{e9}.pgn"));
        prefs.files.recent.push(PathBuf::from("b.pgn"));
        prefs.files.last_open_dir = Some(PathBuf::from("open dir"));
        prefs.files.eco_file = Some(PathBuf::from("scid.eco"));
        prefs.pgn_export = PgnExportPrefs {
            nags: NagStyle::Symbolic,
            indent_variations: true,
            indent_comments: true,
            strip_marks: true,
            encoding: EncodingPref::Latin1,
            width: 75,
        };
        prefs.game_info = GameInfoPrefs {
            hide_next_move: true,
            full_comment: true,
            show_fen: true,
            show_material: false,
        };
        prefs.notation = NotationOptions {
            layout: crate::notation::NotationLayout::Inline,
            indent_comments: false,
            symbolic_nags: false,
            move_number_space: true,
            header: crate::notation::HeaderMode::Short,
            strip_marks: true,
            bold_main_line: true,
            show_comments: false,
            show_variations: false,
            show_result: false,
            hover_preview: crate::notation::HoverPreview::Off,
            figurines: false,
            show_time_column: false,
        };
        prefs
            .shortcuts
            .insert("FlipBoard".to_owned(), "Command+Shift+F".to_owned());
        prefs.layout = Some(LayoutNode::default_layout());
        let text = render_preferences(&prefs);
        let loaded = load_preferences(&text);
        assert_eq!(loaded.prefs, prefs, "{text}");
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn notation_section_is_tolerant_and_limited() {
        let loaded = load_preferences(
            "[notation]\nlayout = \"zigzag\"\nhover_preview_ms = 99999\nshow_result = false\nheader = 3\n",
        );
        let notation = loaded.prefs.notation;
        assert_eq!(notation.layout, NotationOptions::default().layout);
        assert_eq!(notation.header, NotationOptions::default().header);
        assert!(!notation.show_result);
        assert_eq!(notation.hover_preview.as_millis(), 5000);
        assert_eq!(loaded.issues.len(), 3, "{:?}", loaded.issues);
        let mut prefs = Preferences::default();
        let mut wild = NotationOptions::default();
        wild.hover_preview = crate::notation::HoverPreview::Delay(u32::MAX);
        PrefChange::Notation(wild).apply(&mut prefs);
        assert_eq!(prefs.notation.hover_preview.as_millis(), 5000);
    }

    #[test]
    fn editing_questions_default_to_asking_and_read_tolerantly() {
        let defaults = load_preferences("[editing]\nconfirm_destructive = 3\n");
        assert!(defaults.prefs.editing.confirm_destructive);
        assert!(defaults.prefs.editing.ask_to_replace_moves);
        assert_eq!(defaults.issues.len(), 1, "{:?}", defaults.issues);
        let off = load_preferences("[editing]\nask_to_replace_moves = false\n");
        assert!(!off.prefs.editing.ask_to_replace_moves);
        assert!(off.prefs.editing.confirm_destructive);
        assert!(off.issues.is_empty());
    }

    #[test]
    fn export_options_follow_the_prefs() {
        let prefs = PgnExportPrefs::default();
        assert_eq!(prefs.write_options(), WriteOptions::default());
        let clip = prefs.clipboard_options();
        assert_eq!(clip.width, LineWidth::Unlimited);
    }
}
