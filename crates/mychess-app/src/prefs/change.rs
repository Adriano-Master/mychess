//! Edits of the preferences made through messages (spec M2-2, D145).

use std::path::PathBuf;

use crate::notation::{HoverPreview, NotationOptions};

use super::{
    AUTOPLAY_DELAY_RANGE_MS, AnalysisPrefs, CoordinatesPref, GameInfoPrefs, GameListPrefs,
    LanguagePref, LayoutNode, MAX_ANIMATION_MS, PGN_WIDTH_RANGE, PgnExportPrefs, PlayersPrefs,
    Preferences, ThemePref,
};
use super::{AccentPref, MenuStylePref};

/// One change of the preferences.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PrefChange {
    /// Interface language.
    Language(LanguagePref),
    /// Colour theme.
    Theme(ThemePref),
    /// Accent colour (spec M9-1).
    Accent(AccentPref),
    /// Menu style: drop-down lists or ribbon (spec M9-2).
    MenuStyle(MenuStylePref),
    /// Whether the ribbon shows only its tabs (spec M9-2).
    RibbonCollapsed(bool),
    /// Board theme id.
    BoardTheme(String),
    /// Piece set id.
    PieceSet(String),
    /// Where coordinates are drawn.
    Coordinates(CoordinatesPref),
    /// Mark the side to move.
    ShowSideToMove(bool),
    /// Highlight the last move.
    HighlightLastMove(bool),
    /// Animation time in milliseconds (0 turns it off; limited to 2000).
    AnimationMs(u32),
    /// Autoplay delay in milliseconds (limited to `100..=300000`).
    AutoplayDelayMs(u32),
    /// Ask before replacing a move that already has continuations.
    AskToReplaceMoves(bool),
    /// Ask before removing comments, variations or moves.
    ConfirmDestructive(bool),
    /// Ask before copying games from one database to another (spec M3-4,
    /// slice 4d).
    ConfirmCopy(bool),
    /// PGN writing options (the width is limited to `20..=500`).
    PgnExport(PgnExportPrefs),
    /// Game information panel options.
    GameInfo(GameInfoPrefs),
    /// Notation panel options (the hover delay is limited to 5000 ms).
    Notation(NotationOptions),
    /// Player strip options (spec M2-5).
    Players(PlayersPrefs),
    /// Analysis engine options (spec M4-3, D431); numbers are limited.
    Analysis(AnalysisPrefs),
    /// The evaluation bar (spec M4-5); the scale is limited to `1..=10`.
    EvalBar(crate::evalbar::EvalBarPrefs),
    /// The arrows of the engine (spec M4-5); the length is limited.
    Arrows(crate::evalbar::ArrowPrefs),
    /// The MultiPV traffic lights (spec M4-5).
    TrafficLights(crate::evalbar::TrafficLightPrefs),
    /// The `[report]` table (spec M4-11).
    Report(crate::report::ReportPrefs),
    /// The `[report_config]` table (spec M4-10, D2100); numbers are limited.
    ReportConfig(Box<crate::report::ReportConfig>),
    /// The opening book (spec M5-6, D1662).
    Book(crate::book::BookPrefs),
    /// The score and time graph (spec M4-7).
    Graph(crate::graphs::GraphPrefs),
    /// The opening tree panel (spec M5-2).
    Tree(crate::tree::TreePrefs),
    /// The advanced searches (spec M6-1): the operations and the last
    /// options; numbers are limited.
    AdvancedSearch(crate::advanced_search::AdvancedSearchPrefs),
    /// The spelling file, the finders and the last values of the player
    /// tools (spec M6-3); numbers are limited and the history trimmed.
    PlayerTools(Box<crate::spelling_ui::PlayerToolsPrefs>),
    /// The crosstable (spec M6-4): the options and the row colour.
    Crosstable(crate::crosstable::CrosstablePrefs),
    /// The lines of the statistics table (spec M6-4).
    Stats(crate::charts::StatsPrefs),
    /// The graphs of the filter (spec M6-4); numbers are limited.
    FilterGraph(crate::charts::FilterGraphPrefs),
    /// The rating graph (spec M6-4); numbers are limited.
    RatingGraph(crate::charts::RatingPrefs),
    /// The piece tracker (spec M6-4): pieces, window, mode, colour.
    PieceTracker(crate::piece_tracker::PieceTrackerPrefs),
    /// The search for patterns (spec M6-2): the operation, the scope and
    /// the last preset; numbers are limited.
    PatternSearch(Box<crate::pattern_search::PatternSearchPrefs>),
    /// The merge of games (spec M7-6): the last ply and the confirmation.
    Merge(crate::merge_game::MergePrefs),
    /// The EPD window (spec M7-5): the dialect, the analysis settings and
    /// the recent files; numbers are limited, repeated files dropped.
    Epd(Box<crate::epd_window::EpdPrefs>),
    /// The export of documents and the board image (spec M7-4): the format,
    /// the options and the document templates; out-of-range values go back
    /// to the default.
    ExportDocs(Box<crate::export_docs::ExportPrefs>),
    /// The opening report (spec M7-2): options, last save choices and the
    /// size of the board; out-of-range values go back to the default.
    OpeningReport(Box<crate::opening_report::OpeningReportPrefs>),
    /// The built-in manual (spec M9-5): the size of the text; a step out of
    /// range goes back to the default.
    Manual(crate::manual::ManualPrefs),
    /// The start-up (spec M9-4): welcome done, tip of the day.
    Start(crate::start::StartPrefs),
    /// The player report (spec M7-3): options and last choices; out-of-range
    /// values go back to the default.
    PlayerReport(Box<crate::player_report::PlayerReportPrefs>),
    /// The maintenance tools (spec M7-1): the criteria of the duplicates,
    /// the steps of the Cleaner and the last sort; out-of-range values go
    /// back to the default.
    Maintenance(Box<crate::maintenance::MaintenancePrefs>),
    /// The tree masks (spec M5-4b): the recent files and the options.
    TreeMask(Box<crate::tree::MaskPrefs>),
    /// The columns of the game list (spec M3-4): widths are limited,
    /// repeated columns dropped, and the default ones come back if none is
    /// left.
    GameList(GameListPrefs),
    /// The `.eco` file (`None` forgets it).
    EcoFile(Option<PathBuf>),
    /// Customises one shortcut; `None` as the binding restores the default.
    /// Use `Some("")` to turn the shortcut off.
    Shortcut {
        /// The action name (`Action::name`).
        action: String,
        /// The binding text (`Command+Shift+B`).
        binding: Option<String>,
    },
    /// The game against an engine (spec M8-1b); numbers are limited.
    Play(Box<super::PlayPrefs>),
    /// A tactical game that ended (spec M8-7, D2463, D2473): a delta, so it
    /// counts on top of the score of the file even when the file is read
    /// after the game.
    RecordTactical {
        /// How the game went for the user.
        outcome: crate::tactical_game::Outcome,
        /// The Elo the engine played at.
        faced: u32,
        /// The Elo for the next game, when the adjustment is on.
        new_elo: Option<u32>,
    },
    /// The configuration of the last tournament between engines (spec
    /// M8-2); saved when the dialog starts one.
    Tournament(Box<crate::tournament::TournamentConfig>),
    /// The `[online]` table (spec M8-3, D1338).
    Online(Box<crate::online::OnlinePrefs>),
    /// The `[fics]` table (spec M8-9, D2628).
    Fics(Box<crate::fics::FicsPrefs>),
    /// The `[tablebase]` table (spec M8-8, D2562); the folders are limited.
    Tablebase(Box<crate::tablebase::TablebasePrefs>),
    /// The `[sound]` table (spec M8-11, D2804); the volume is limited.
    Sound(crate::sound::SoundPrefs),
    /// The arrangement of the panels (`None` restores the default).
    /// Saved with the next flush, not at once.
    Layout(Option<LayoutNode>),
}

impl PrefChange {
    /// Whether the change waits for `FlushPreferences` instead of being
    /// saved at once.
    pub fn is_debounced(&self) -> bool {
        matches!(self, PrefChange::Layout(_))
    }

    /// Applies the change to `prefs`, limiting numbers to their ranges.
    pub(crate) fn apply(self, prefs: &mut Preferences) {
        match self {
            PrefChange::Language(v) => prefs.general.language = v,
            PrefChange::Theme(v) => prefs.general.theme = v,
            PrefChange::Accent(v) => prefs.general.accent = v,
            PrefChange::MenuStyle(v) => prefs.general.menu_style = v,
            PrefChange::RibbonCollapsed(v) => prefs.general.ribbon_collapsed = v,
            PrefChange::BoardTheme(v) => prefs.board.theme = v,
            PrefChange::PieceSet(v) => prefs.board.piece_set = v,
            PrefChange::Coordinates(v) => prefs.board.coordinates = v,
            PrefChange::ShowSideToMove(v) => prefs.board.show_side_to_move = v,
            PrefChange::HighlightLastMove(v) => prefs.board.highlight_last_move = v,
            PrefChange::AnimationMs(v) => prefs.board.animation_ms = v.min(MAX_ANIMATION_MS),
            PrefChange::AutoplayDelayMs(v) => {
                prefs.playback.autoplay_delay_ms =
                    v.clamp(AUTOPLAY_DELAY_RANGE_MS.0, AUTOPLAY_DELAY_RANGE_MS.1);
            }
            PrefChange::AskToReplaceMoves(v) => prefs.editing.ask_to_replace_moves = v,
            PrefChange::ConfirmDestructive(v) => prefs.editing.confirm_destructive = v,
            PrefChange::ConfirmCopy(v) => prefs.editing.confirm_copy = v,
            PrefChange::PgnExport(mut v) => {
                v.width = v.width.clamp(PGN_WIDTH_RANGE.0, PGN_WIDTH_RANGE.1);
                prefs.pgn_export = v;
            }
            PrefChange::GameInfo(v) => prefs.game_info = v,
            PrefChange::Notation(mut v) => {
                v.hover_preview = HoverPreview::from_millis(v.hover_preview.as_millis());
                prefs.notation = v;
            }
            PrefChange::Players(v) => prefs.players = v,
            PrefChange::Analysis(v) => *prefs.analysis = v.sanitized(),
            PrefChange::EvalBar(v) => prefs.engine_view.eval_bar = v.sanitized(),
            PrefChange::Arrows(v) => prefs.engine_view.arrows = v.sanitized(),
            PrefChange::TrafficLights(v) => prefs.engine_view.traffic_lights = v,
            PrefChange::Report(v) => prefs.report = v,
            PrefChange::ReportConfig(v) => *prefs.report_config = v.sanitized(),
            PrefChange::Book(v) => *prefs.book = v,
            PrefChange::Graph(v) => prefs.graph = v,
            PrefChange::Tree(v) => prefs.tree = v,
            PrefChange::AdvancedSearch(v) => prefs.advanced_search = v.sanitized(),
            PrefChange::PlayerTools(v) => *prefs.player_tools = v.sanitized(),
            PrefChange::Crosstable(v) => *prefs.crosstable = v.sanitized(),
            PrefChange::Stats(v) => *prefs.stats = v.sanitized(),
            PrefChange::FilterGraph(v) => *prefs.filter_graph = v.sanitized(),
            PrefChange::RatingGraph(v) => prefs.rating_graph = v.sanitized(),
            PrefChange::PieceTracker(v) => *prefs.ptracker = v.sanitized(),
            PrefChange::PatternSearch(v) => *prefs.pattern_search = v.sanitized(),
            PrefChange::Merge(v) => *prefs.merge = v.sanitized(),
            PrefChange::Epd(v) => *prefs.epd = v.sanitized(),
            PrefChange::ExportDocs(v) => *prefs.export_docs = v.sanitized(),
            PrefChange::OpeningReport(v) => *prefs.opening_report = v.sanitized(),
            PrefChange::Manual(v) => *prefs.manual = v.sanitized(),
            PrefChange::Start(v) => prefs.start = v.sanitized(),
            PrefChange::PlayerReport(v) => *prefs.player_report = v.sanitized(),
            PrefChange::Maintenance(v) => *prefs.maintenance = v.sanitized(),
            PrefChange::TreeMask(v) => *prefs.mask = v.sanitized(),
            PrefChange::GameList(v) => *prefs.game_list = v.sanitized(),
            PrefChange::EcoFile(v) => prefs.files.eco_file = v,
            PrefChange::Shortcut { action, binding } => match binding {
                Some(text) => {
                    prefs.shortcuts.insert(action, text);
                }
                None => {
                    prefs.shortcuts.remove(&action);
                }
            },
            PrefChange::Play(v) => *prefs.play = v.sanitized(),
            PrefChange::RecordTactical {
                outcome,
                faced,
                new_elo,
            } => {
                prefs.play.tactical.stats.record(outcome, faced);
                if let Some(elo) = new_elo {
                    prefs.play.tactical.elo = elo;
                    prefs.play.tactical = prefs.play.tactical.clone().sanitized();
                }
            }
            PrefChange::Tournament(v) => *prefs.tournament = v.clamped(),
            PrefChange::Online(v) => *prefs.online = v.sanitized(),
            PrefChange::Fics(v) => *prefs.fics = v.sanitized(),
            PrefChange::Tablebase(v) => *prefs.tablebase = v.sanitized(),
            PrefChange::Sound(v) => *prefs.sound = v.sanitized(),
            PrefChange::Layout(v) => prefs.layout = v.filter(LayoutNode::is_valid),
        }
    }
}

/// Events of the preferences.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PrefsMessage {
    /// Changes one preference.
    Set(PrefChange),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_play_preferences_are_limited_and_saved_at_once() {
        let mut prefs = Preferences::default();
        let play = super::super::PlayPrefs {
            movetime: 9999,
            ..super::super::PlayPrefs::default()
        };
        let change = PrefChange::Play(Box::new(play));
        assert!(!change.is_debounced());
        change.apply(&mut prefs);
        assert_eq!(prefs.play.movetime, crate::play::MOVETIME_RANGE.1);
    }

    #[test]
    fn a_wild_tournament_configuration_still_renders_the_preferences() {
        // Regression (D1310): `u64::MAX` made the TOML serializer fail and
        // the file was written empty.
        use crate::tournament::{Participant, TimeControl, TournamentConfig};
        let mut prefs = Preferences::default();
        prefs.general.theme = ThemePref::Dark;
        let config = TournamentConfig {
            participants: vec![Participant::new(u64::MAX, "A")],
            time: TimeControl::Nodes(u64::MAX),
            ..TournamentConfig::default()
        };
        PrefChange::Tournament(Box::new(config)).apply(&mut prefs);
        let text = super::super::try_render_preferences(&prefs).expect("renders");
        let loaded = super::super::load_preferences(&text);
        assert_eq!(loaded.prefs.general.theme, ThemePref::Dark);
        assert_eq!(
            loaded.prefs.tournament.time,
            TimeControl::Nodes(u64::MAX >> 1)
        );
    }

    #[test]
    fn numbers_are_limited() {
        let mut prefs = Preferences::default();
        PrefChange::AnimationMs(99_999).apply(&mut prefs);
        PrefChange::AutoplayDelayMs(1).apply(&mut prefs);
        assert_eq!(prefs.board.animation_ms, MAX_ANIMATION_MS);
        assert_eq!(prefs.playback.autoplay_delay_ms, AUTOPLAY_DELAY_RANGE_MS.0);
        let export = PgnExportPrefs {
            width: 5,
            ..PgnExportPrefs::default()
        };
        PrefChange::PgnExport(export).apply(&mut prefs);
        assert_eq!(prefs.pgn_export.width, PGN_WIDTH_RANGE.0);
    }

    #[test]
    fn the_accent_changes_the_preference_and_is_saved_at_once() {
        let mut prefs = Preferences::default();
        assert_eq!(prefs.general.accent, AccentPref::Mychess);
        PrefChange::Accent(AccentPref::Purple).apply(&mut prefs);
        assert_eq!(prefs.general.accent, AccentPref::Purple);
        assert!(!PrefChange::Accent(AccentPref::Blue).is_debounced());
    }

    #[test]
    fn the_menu_style_and_the_collapsed_ribbon_change_and_save_at_once() {
        let mut prefs = Preferences::default();
        assert_eq!(prefs.general.menu_style, MenuStylePref::Dropdown);
        assert!(!prefs.general.ribbon_collapsed);
        PrefChange::MenuStyle(MenuStylePref::Ribbon).apply(&mut prefs);
        PrefChange::RibbonCollapsed(true).apply(&mut prefs);
        assert_eq!(prefs.general.menu_style, MenuStylePref::Ribbon);
        assert!(prefs.general.ribbon_collapsed);
        assert!(!PrefChange::MenuStyle(MenuStylePref::Ribbon).is_debounced());
        assert!(!PrefChange::RibbonCollapsed(true).is_debounced());
    }

    #[test]
    fn shortcuts_and_layouts() {
        let mut prefs = Preferences::default();
        PrefChange::Shortcut {
            action: "Save".to_owned(),
            binding: Some(String::new()),
        }
        .apply(&mut prefs);
        assert_eq!(prefs.shortcuts.get("Save").map(String::as_str), Some(""));
        PrefChange::Shortcut {
            action: "Save".to_owned(),
            binding: None,
        }
        .apply(&mut prefs);
        assert!(prefs.shortcuts.is_empty());
        PrefChange::Layout(Some(LayoutNode::pane(super::super::PaneKind::Info))).apply(&mut prefs);
        assert_eq!(prefs.layout, None);
        PrefChange::Layout(Some(LayoutNode::default_layout())).apply(&mut prefs);
        assert!(prefs.layout.is_some());
        assert!(PrefChange::Layout(None).is_debounced());
        assert!(!PrefChange::Theme(ThemePref::Dark).is_debounced());
    }
}
