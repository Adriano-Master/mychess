//! Tolerant reading of the preferences (D144): field by field, so one bad
//! value never costs the others.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use super::{
    AUTOPLAY_DELAY_RANGE_MS, AccentPref, AnalysisPrefs, BoardPrefs, CoordinatesPref, EditingPrefs,
    EncodingPref, FilesPrefs, GameInfoPrefs, GameListPrefs, General, LanguagePref, LayoutNode,
    LoadedPreferences, MAX_ANIMATION_MS, MAX_INFO_INTERVAL_MS, MAX_LOG_LINES, MAX_PV_PLY,
    MAX_WINDOW_SIZE, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, MenuStylePref, NagStyle, PGN_WIDTH_RANGE,
    PREFERENCES_VERSION, PgnExportPrefs, PlayBookPref, PlayColorPref, PlayModePref, PlayPrefs,
    PlayStartPref, Playback, PlayersPrefs, Preferences, PrefsIssue, ThemePref, WindowPrefs,
};
use crate::advanced_search::{
    AdvancedSearchPrefs, BishopsKind, BoardKind, CheckKind, MAX_MIN_PLIES, MAX_MOVE_NUMBER,
    SearchOp, SideKind,
};
use crate::bases::{ColumnAlign, ColumnSetup, ListColumn, MAX_COLUMN_CHARS, MIN_COLUMN_CHARS};
use crate::evalbar::{
    ArrowMode, ArrowPrefs, EVAL_SCALE_RANGE, EngineViewPrefs, EvalBarPrefs, SEQUENCE_LEN_RANGE,
    SequenceSide, TrafficLightPrefs,
};
use crate::files::RecentFiles;
use crate::graphs::{GraphKind, GraphPrefs, GraphStyle, Invert};
use crate::material::MaterialMode;
use crate::notation::{HeaderMode, HoverPreview, NotationLayout, NotationOptions};
use crate::pattern_search::{MAX_HOLD, MAX_SCOPE_MOVE, PatternSearchPrefs};
use crate::tree::{CACHE_SIZE_RANGE, TreeFastMode, TreePrefs, TreeSort};

/// One `[section]` of the file, with helpers that report problems.
struct Section<'a> {
    name: &'static str,
    table: Option<&'a Table>,
}

impl<'a> Section<'a> {
    fn new(root: &'a Table, name: &'static str, issues: &mut Vec<PrefsIssue>) -> Section<'a> {
        let table = match root.get(name) {
            None => None,
            Some(Value::Table(table)) => Some(table),
            Some(_) => {
                issues.push(PrefsIssue::InvalidValue {
                    key: name.to_owned(),
                });
                None
            }
        };
        Section { name, table }
    }

    fn value(&self, key: &str) -> Option<&'a Value> {
        self.table.and_then(|t| t.get(key))
    }

    fn invalid(&self, key: &str, issues: &mut Vec<PrefsIssue>) {
        issues.push(PrefsIssue::InvalidValue {
            key: format!("{}.{key}", self.name),
        });
    }

    fn bool(&self, key: &str, default: bool, issues: &mut Vec<PrefsIssue>) -> bool {
        match self.value(key) {
            None => default,
            Some(Value::Boolean(b)) => *b,
            Some(_) => {
                self.invalid(key, issues);
                default
            }
        }
    }

    fn text(&self, key: &str, default: &str, issues: &mut Vec<PrefsIssue>) -> String {
        match self.value(key) {
            None => default.to_owned(),
            Some(Value::String(s)) => s.clone(),
            Some(_) => {
                self.invalid(key, issues);
                default.to_owned()
            }
        }
    }

    fn choice<T: Copy>(
        &self,
        key: &str,
        default: T,
        parse: fn(&str) -> Option<T>,
        issues: &mut Vec<PrefsIssue>,
    ) -> T {
        match self.value(key) {
            None => default,
            Some(Value::String(s)) => parse(s).unwrap_or_else(|| {
                self.invalid(key, issues);
                default
            }),
            Some(_) => {
                self.invalid(key, issues);
                default
            }
        }
    }

    /// An integer limited to `min..=max`; `None` when absent or not a number.
    fn int(&self, key: &str, min: i64, max: i64, issues: &mut Vec<PrefsIssue>) -> Option<i64> {
        match self.value(key) {
            None => None,
            Some(Value::Integer(n)) => {
                let limited = (*n).clamp(min, max);
                if limited != *n {
                    issues.push(PrefsIssue::Clamped {
                        key: format!("{}.{key}", self.name),
                    });
                }
                Some(limited)
            }
            Some(_) => {
                self.invalid(key, issues);
                None
            }
        }
    }

    fn path(&self, key: &str, issues: &mut Vec<PrefsIssue>) -> Option<PathBuf> {
        match self.value(key) {
            None => None,
            Some(Value::String(s)) if !s.is_empty() => Some(PathBuf::from(s)),
            Some(_) => {
                self.invalid(key, issues);
                None
            }
        }
    }
}

fn to_u32(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Brings the table of an older format up to [`PREFERENCES_VERSION`], one
/// step at a time. Version 0 (no `version` key) has the same layout as 1.
fn migrate(table: Table, from: u32) -> Table {
    let mut version = from;
    while version < PREFERENCES_VERSION {
        // 0 -> 1: nothing to rename; every known key is read as is.
        version += 1;
    }
    table
}

#[allow(clippy::too_many_lines)] // one line per table of the file
pub(super) fn load(text: &str) -> LoadedPreferences {
    let Ok(root) = text.parse::<Table>() else {
        return LoadedPreferences {
            prefs: Preferences::default(),
            issues: vec![PrefsIssue::Unreadable],
        };
    };
    let mut issues = Vec::new();
    let found = match root.get("version") {
        None => 0,
        Some(Value::Integer(n)) if *n >= 0 => to_u32(*n),
        Some(_) => {
            issues.push(PrefsIssue::InvalidValue {
                key: "version".to_owned(),
            });
            0
        }
    };
    if found > PREFERENCES_VERSION {
        issues.push(PrefsIssue::NewerVersion { found });
    }
    let root = migrate(root, found);
    let defaults = Preferences::default();

    let sections = Sections::new(&root, &mut issues);
    let prefs = Preferences {
        version: PREFERENCES_VERSION,
        general: read_general(&sections.general, &defaults.general, &mut issues),
        board: read_board(&sections.board, &defaults.board, &mut issues),
        playback: read_playback(&sections.playback, &defaults.playback, &mut issues),
        editing: read_editing(&sections.editing, defaults.editing, &mut issues),
        window: read_window(&sections.window, &mut issues),
        files: Box::new(FilesPrefs {
            recent: read_recent(&sections.files, &mut issues),
            last_open_dir: sections.files.path("last_open_dir", &mut issues),
            last_save_dir: sections.files.path("last_save_dir", &mut issues),
            eco_file: sections.files.path("eco_file", &mut issues),
        }),
        pgn_export: read_export(&sections.export, &defaults.pgn_export, &mut issues),
        game_info: read_game_info(&sections.info, &defaults.game_info, &mut issues),
        notation: read_notation(&sections.notation, &defaults.notation, &mut issues),
        players: read_players(&sections.players, &defaults.players, &mut issues),
        analysis: Box::new(read_analysis(
            &sections.analysis,
            &defaults.analysis,
            &mut issues,
        )),
        play: Box::new(read_play(&sections.play, &defaults.play, &mut issues)),
        tournament: Box::new(read_tournament(&root, &mut issues)),
        online: Box::new(read_online(&root, &defaults.online, &mut issues)),
        fics: Box::new(read_fics(&root, &defaults.fics, &mut issues)),
        tablebase: Box::new(read_tablebase(&root, &defaults.tablebase, &mut issues)),
        eboard: Box::new(read_eboard(&root, &defaults.eboard, &mut issues)),
        sound: Box::new(read_sound(&root, &defaults.sound, &mut issues)),
        engine_view: Box::new(read_engine_view(&root, &defaults.engine_view, &mut issues)),
        report: read_report(&root, defaults.report, &mut issues),
        report_config: Box::new(read_report_config(
            &root,
            *defaults.report_config,
            &mut issues,
        )),
        book: Box::new(read_book(&root, &defaults.book, &mut issues)),
        graph: read_graph(&root, defaults.graph, &mut issues),
        tree: read_tree(&root, defaults.tree, &mut issues),
        advanced_search: read_advanced_search(&root, defaults.advanced_search, &mut issues),
        player_tools: Box::new(crate::spelling_ui::read_player_tools(
            &root,
            &defaults.player_tools,
            &mut issues,
        )),
        crosstable: Box::new(read_crosstable(&root, &defaults.crosstable, &mut issues)),
        stats: Box::new(read_stats(&root, &mut issues)),
        filter_graph: Box::new(read_filter_graph(
            &root,
            &defaults.filter_graph,
            &mut issues,
        )),
        rating_graph: read_rating_graph(&root, defaults.rating_graph, &mut issues),
        ptracker: Box::new(read_ptracker(&root, &defaults.ptracker, &mut issues)),
        pattern_search: Box::new(read_pattern_search(
            &root,
            &defaults.pattern_search,
            &mut issues,
        )),
        merge: Box::new(read_merge(&root, *defaults.merge, &mut issues)),
        manual: Box::new(read_manual(&root, *defaults.manual, &mut issues)),
        start: read_start(&root, defaults.start, &mut issues),
        epd: Box::new(read_epd(&root, &defaults.epd, &mut issues)),
        export_docs: Box::new(read_export_docs(&root, &defaults.export_docs, &mut issues)),
        opening_report: Box::new(read_opening_report(
            &root,
            &defaults.opening_report,
            &mut issues,
        )),
        maintenance: Box::new(read_maintenance(&root, &defaults.maintenance, &mut issues)),
        player_report: Box::new(read_player_report(
            &root,
            &defaults.player_report,
            &mut issues,
        )),
        mask: Box::new(read_tree_mask(&root, &defaults.mask, &mut issues)),
        game_list: Box::new(read_game_list(&root, &mut issues)),
        shortcuts: read_shortcuts(&root, &mut issues),
        layout: read_layout(&root, &mut issues),
    };
    LoadedPreferences { prefs, issues }
}

/// The sections of the file.
struct Sections<'a> {
    general: Section<'a>,
    board: Section<'a>,
    playback: Section<'a>,
    editing: Section<'a>,
    window: Section<'a>,
    files: Section<'a>,
    export: Section<'a>,
    info: Section<'a>,
    notation: Section<'a>,
    players: Section<'a>,
    analysis: Section<'a>,
    play: Section<'a>,
}

impl<'a> Sections<'a> {
    fn new(root: &'a Table, issues: &mut Vec<PrefsIssue>) -> Sections<'a> {
        Sections {
            general: Section::new(root, "general", issues),
            board: Section::new(root, "board", issues),
            playback: Section::new(root, "playback", issues),
            editing: Section::new(root, "editing", issues),
            window: Section::new(root, "window", issues),
            files: Section::new(root, "files", issues),
            export: Section::new(root, "pgn_export", issues),
            info: Section::new(root, "game_info", issues),
            notation: Section::new(root, "notation", issues),
            players: Section::new(root, "players", issues),
            analysis: Section::new(root, "analysis", issues),
            play: Section::new(root, "play", issues),
        }
    }
}

fn read_general(sec: &Section<'_>, defaults: &General, issues: &mut Vec<PrefsIssue>) -> General {
    General {
        language: sec.choice(
            "language",
            defaults.language,
            LanguagePref::from_name,
            issues,
        ),
        theme: sec.choice("theme", defaults.theme, ThemePref::from_name, issues),
        accent: sec.choice("accent", defaults.accent, AccentPref::from_name, issues),
        menu_style: sec.choice(
            "menu_style",
            defaults.menu_style,
            MenuStylePref::from_name,
            issues,
        ),
        ribbon_collapsed: sec.bool("ribbon_collapsed", defaults.ribbon_collapsed, issues),
    }
}

fn read_board(
    sec: &Section<'_>,
    defaults: &BoardPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> BoardPrefs {
    BoardPrefs {
        theme: sec.text("theme", &defaults.theme, issues),
        piece_set: sec.text("piece_set", &defaults.piece_set, issues),
        coordinates: sec.choice(
            "coordinates",
            defaults.coordinates,
            CoordinatesPref::from_name,
            issues,
        ),
        show_side_to_move: sec.bool("show_side_to_move", defaults.show_side_to_move, issues),
        highlight_last_move: sec.bool("highlight_last_move", defaults.highlight_last_move, issues),
        animation_ms: sec
            .int("animation_ms", 0, i64::from(MAX_ANIMATION_MS), issues)
            .map_or(defaults.animation_ms, to_u32),
    }
}

fn read_playback(sec: &Section<'_>, defaults: &Playback, issues: &mut Vec<PrefsIssue>) -> Playback {
    Playback {
        autoplay_delay_ms: sec
            .int(
                "autoplay_delay_ms",
                i64::from(AUTOPLAY_DELAY_RANGE_MS.0),
                i64::from(AUTOPLAY_DELAY_RANGE_MS.1),
                issues,
            )
            .map_or(defaults.autoplay_delay_ms, to_u32),
    }
}

fn read_editing(
    sec: &Section<'_>,
    defaults: EditingPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> EditingPrefs {
    EditingPrefs {
        ask_to_replace_moves: sec.bool(
            "ask_to_replace_moves",
            defaults.ask_to_replace_moves,
            issues,
        ),
        confirm_destructive: sec.bool("confirm_destructive", defaults.confirm_destructive, issues),
        confirm_copy: sec.bool("confirm_copy", defaults.confirm_copy, issues),
    }
}

fn read_window(sec: &Section<'_>, issues: &mut Vec<PrefsIssue>) -> WindowPrefs {
    let coordinate = |key: &str, issues: &mut Vec<PrefsIssue>| {
        sec.int(key, i64::from(i32::MIN), i64::from(i32::MAX), issues)
            .and_then(|n| i32::try_from(n).ok())
    };
    WindowPrefs {
        width: sec
            .int(
                "width",
                i64::from(MIN_WINDOW_WIDTH),
                i64::from(MAX_WINDOW_SIZE),
                issues,
            )
            .map(to_u32),
        height: sec
            .int(
                "height",
                i64::from(MIN_WINDOW_HEIGHT),
                i64::from(MAX_WINDOW_SIZE),
                issues,
            )
            .map(to_u32),
        x: coordinate("x", issues),
        y: coordinate("y", issues),
        maximized: sec.bool("maximized", false, issues),
    }
}

fn read_export(
    sec: &Section<'_>,
    defaults: &PgnExportPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> PgnExportPrefs {
    PgnExportPrefs {
        nags: sec.choice("nags", defaults.nags, NagStyle::from_name, issues),
        indent_variations: sec.bool("indent_variations", defaults.indent_variations, issues),
        indent_comments: sec.bool("indent_comments", defaults.indent_comments, issues),
        strip_marks: sec.bool("strip_marks", defaults.strip_marks, issues),
        encoding: sec.choice(
            "encoding",
            defaults.encoding,
            EncodingPref::from_name,
            issues,
        ),
        width: sec
            .int(
                "width",
                i64::from(PGN_WIDTH_RANGE.0),
                i64::from(PGN_WIDTH_RANGE.1),
                issues,
            )
            .and_then(|n| u16::try_from(n).ok())
            .unwrap_or(defaults.width),
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)] // same shape as the other sections
fn read_game_info(
    sec: &Section<'_>,
    defaults: &GameInfoPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> GameInfoPrefs {
    GameInfoPrefs {
        hide_next_move: sec.bool("hide_next_move", defaults.hide_next_move, issues),
        full_comment: sec.bool("full_comment", defaults.full_comment, issues),
        show_fen: sec.bool("show_fen", defaults.show_fen, issues),
        show_material: sec.bool("show_material", defaults.show_material, issues),
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)] // same shape as the other sections
fn read_players(
    sec: &Section<'_>,
    defaults: &PlayersPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> PlayersPrefs {
    PlayersPrefs {
        show_strips: sec.bool("show_strips", defaults.show_strips, issues),
        show_avatars: sec.bool("show_avatars", defaults.show_avatars, issues),
        show_ratings: sec.bool("show_ratings", defaults.show_ratings, issues),
        show_clocks: sec.bool("show_clocks", defaults.show_clocks, issues),
        compact_strips: sec.bool("compact_strips", defaults.compact_strips, issues),
        material: sec.choice(
            "material",
            defaults.material,
            MaterialMode::from_name,
            issues,
        ),
    }
}

fn read_play(sec: &Section<'_>, defaults: &PlayPrefs, issues: &mut Vec<PrefsIssue>) -> PlayPrefs {
    let number = |key: &str, max: u32, default: u32, issues: &mut Vec<PrefsIssue>| {
        sec.int(key, 0, i64::from(max), issues)
            .map_or(default, to_u32)
    };
    let d = defaults;
    let openings = match sec.value("openings") {
        None => d.openings.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        Some(_) => {
            sec.invalid("openings", issues);
            d.openings.clone()
        }
    };
    PlayPrefs {
        engine: sec
            .int("engine", 0, i64::MAX, issues)
            .and_then(|n| u64::try_from(n).ok())
            .or(d.engine),
        color: sec.choice("color", d.color, PlayColorPref::from_name, issues),
        mode: sec.choice("mode", d.mode, PlayModePref::from_name, issues),
        white_minutes: number("white_minutes", 100_000, d.white_minutes, issues),
        white_bonus: number("white_bonus", 100_000, d.white_bonus, issues),
        black_minutes: number("black_minutes", 100_000, d.black_minutes, issues),
        black_bonus: number("black_bonus", 100_000, d.black_bonus, issues),
        depth: number("depth", 1000, d.depth, issues),
        nodes: number("nodes", 1_000_000, d.nodes, issues),
        movetime: number("movetime", 100_000, d.movetime, issues),
        start: sec.choice("start", d.start, PlayStartPref::from_name, issues),
        opening: sec.text("opening", &d.opening, issues),
        openings,
        use_book: sec.bool("use_book", d.use_book, issues),
        book_path: sec.path("book_path", issues),
        book_selection: sec.choice(
            "book_selection",
            d.book_selection,
            PlayBookPref::from_name,
            issues,
        ),
        limit_strength: sec.bool("limit_strength", d.limit_strength, issues),
        elo: number("elo", 100_000, d.elo, issues),
        use_skill: sec.bool("use_skill", d.use_skill, issues),
        skill: number("skill", 100_000, d.skill, issues),
        player_name: sec.text("player_name", &d.player_name, issues),
        ponder: sec.bool("ponder", d.ponder, issues),
        store_clock: sec.bool("store_clock", d.store_clock, issues),
        coach: sec.bool("coach", d.coach, issues),
        coach_engine: sec
            .int("coach_engine", 0, i64::MAX, issues)
            .and_then(|n| u64::try_from(n).ok())
            .or(d.coach_engine),
        coach_depth: number("coach_depth", 1000, d.coach_depth, issues),
        coach_seconds: number("coach_seconds", 100_000, d.coach_seconds, issues),
        coach_hint: sec.bool("coach_hint", d.coach_hint, issues),
        coach_numbers: sec.bool("coach_numbers", d.coach_numbers, issues),
        tactical: crate::tactical_game::TacticalPrefs::read(sec.value("tactical"), issues),
    }
    .sanitized()
}

/// The `[tournament]` table: the last configuration of a tournament. A table
/// that does not fit is dropped as a whole (a remark is made) and the
/// defaults are used.
fn read_tournament(
    root: &Table,
    issues: &mut Vec<PrefsIssue>,
) -> crate::tournament::TournamentConfig {
    let Some(value) = root.get("tournament") else {
        return crate::tournament::TournamentConfig::default();
    };
    if let Ok(config) = value
        .clone()
        .try_into::<crate::tournament::TournamentConfig>()
    {
        config.clamped()
    } else {
        issues.push(PrefsIssue::InvalidValue {
            key: "tournament".to_owned(),
        });
        crate::tournament::TournamentConfig::default()
    }
}

/// The `[online]` table (spec M8-3, D1338). The token is never read from
/// here; a server address that could carry it in clear is replaced.
fn read_online(
    root: &Table,
    defaults: &crate::online::OnlinePrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::online::OnlinePrefs {
    use crate::online::{OnlinePrefs, SaveTo, TokenStorePref};
    let sec = Section::new(root, "online", issues);
    let number = |key: &str, max: i64, default: u32, issues: &mut Vec<PrefsIssue>| {
        sec.int(key, 0, max, issues).map_or(default, to_u32)
    };
    let d = defaults;
    OnlinePrefs {
        login: sec.text("login", &d.login, issues),
        rated: sec.bool("rated", d.rated, issues),
        time: number("time", 180, d.time, issues),
        increment: number("increment", 180, d.increment, issues),
        premove: sec.bool("premove", d.premove, issues),
        confirm_resign: sec.bool("confirm_resign", d.confirm_resign, issues),
        show_ping: sec.bool("show_ping", d.show_ping, issues),
        save_to: sec.choice("save_to", d.save_to, SaveTo::from_name, issues),
        token_store: sec.choice(
            "token_store",
            d.token_store,
            TokenStorePref::from_name,
            issues,
        ),
        base_url: sec.text("base_url", &d.base_url, issues),
    }
    .sanitized()
}

/// The `[fics]` table (spec M8-9, D2628). The password is never read.
fn read_fics(
    root: &Table,
    defaults: &crate::fics::FicsPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::fics::FicsPrefs {
    use crate::fics::{AutoPromote, FicsPrefs};
    let sec = Section::new(root, "fics", issues);
    let d = defaults;
    let init_commands = match sec.value("init_commands") {
        None => d.init_commands.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        Some(_) => {
            sec.invalid("init_commands", issues);
            d.init_commands.clone()
        }
    };
    FicsPrefs {
        host: sec.text("host", &d.host, issues),
        port: sec
            .int("port", 1, 65535, issues)
            .and_then(|v| u16::try_from(v).ok())
            .unwrap_or(d.port),
        login: sec.text("login", &d.login, issues),
        rated: sec.bool("rated", d.rated, issues),
        minutes: sec.int("minutes", 0, 600, issues).map_or(d.minutes, to_u32),
        increment: sec
            .int("increment", 0, 600, issues)
            .map_or(d.increment, to_u32),
        auto_promote: sec.choice(
            "auto_promote",
            d.auto_promote,
            AutoPromote::from_name,
            issues,
        ),
        premove: sec.bool("premove", d.premove, issues),
        store_emt: sec.bool("store_emt", d.store_emt, issues),
        store_clk: sec.bool("store_clk", d.store_clk, issues),
        shouts: sec.bool("shouts", d.shouts, issues),
        init_commands,
    }
    .sanitized()
}

/// The `[sound]` table (spec M8-11, D2804); the volume is limited.
fn read_sound(
    root: &Table,
    defaults: &crate::sound::SoundPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::sound::SoundPrefs {
    let sec = Section::new(root, "sound", issues);
    let max = i64::from(*crate::sound::VOLUME_RANGE.end());
    crate::sound::SoundPrefs {
        enabled: sec.bool("enabled", defaults.enabled, issues),
        volume: sec
            .int("volume", 0, max, issues)
            .and_then(|v| u8::try_from(v).ok())
            .unwrap_or(defaults.volume),
        on_new_move: sec.bool("on_new_move", defaults.on_new_move, issues),
        on_forward: sec.bool("on_forward", defaults.on_forward, issues),
        on_back: sec.bool("on_back", defaults.on_back, issues),
        tick_only: sec.bool("tick_only", defaults.tick_only, issues),
        announce: sec.bool("announce", defaults.announce, issues),
        folder: sec.text("folder", &defaults.folder, issues),
        pack: match sec.value("pack") {
            None => defaults.pack.clone(),
            Some(Value::String(s)) => crate::sound::PackId::parse(s).unwrap_or_else(|| {
                sec.invalid("pack", issues);
                defaults.pack.clone()
            }),
            Some(_) => {
                sec.invalid("pack", issues);
                defaults.pack.clone()
            }
        },
    }
    .sanitized()
}

/// The `[eboard]` table (spec M8-10, D2740).
fn read_eboard(
    root: &Table,
    defaults: &crate::eboard::EboardPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::eboard::EboardPrefs {
    use crate::eboard::{BoardChoice, EboardPrefs, FilterChoice};
    let sec = Section::new(root, "eboard", issues);
    EboardPrefs {
        kind: sec.choice("kind", defaults.kind, BoardChoice::from_name, issues),
        port: sec.text("port", &defaults.port, issues),
        engine_command: sec.text("engine_command", &defaults.engine_command, issues),
        engine_params: sec.text("engine_params", &defaults.engine_params, issues),
        send_filter: sec.choice(
            "send_filter",
            defaults.send_filter,
            FilterChoice::from_name,
            issues,
        ),
    }
    .sanitized()
}

/// The `[tablebase]` table (spec M8-8, D2562). Folders that are not text are
/// dropped with a remark.
fn read_tablebase(
    root: &Table,
    defaults: &crate::tablebase::TablebasePrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::tablebase::TablebasePrefs {
    let sec = Section::new(root, "tablebase", issues);
    let directories = match sec.value("directories") {
        None => defaults.directories.clone(),
        Some(Value::Array(items)) => {
            // A folder that is not text is dropped, with a remark.
            if items.iter().any(|v| !v.is_str()) {
                sec.invalid("directories", issues);
            }
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        }
        Some(_) => {
            sec.invalid("directories", issues);
            defaults.directories.clone()
        }
    };
    crate::tablebase::TablebasePrefs {
        local: sec.bool("local", defaults.local, issues),
        directories,
        online: sec.bool("online", defaults.online, issues),
    }
    .sanitized()
}

fn read_analysis(
    sec: &Section<'_>,
    defaults: &AnalysisPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> AnalysisPrefs {
    let number = |key: &str, max: u32, default: u32, issues: &mut Vec<PrefsIssue>| {
        sec.int(key, 0, i64::from(max), issues)
            .map_or(default, to_u32)
    };
    // `sanitized` raises an interval below the minimum.
    AnalysisPrefs {
        log_lines: number("log_lines", MAX_LOG_LINES, defaults.log_lines, issues),
        name_in_comments: sec.bool("name_in_comments", defaults.name_in_comments, issues),
        max_ply: number("max_ply", MAX_PV_PLY, defaults.max_ply, issues),
        low_priority: sec.bool("low_priority", defaults.low_priority, issues),
        wrap_pv: sec.bool("wrap_pv", defaults.wrap_pv, issues),
        show_info: sec.bool("show_info", defaults.show_info, issues),
        info_interval_ms: number(
            "info_interval_ms",
            MAX_INFO_INTERVAL_MS,
            defaults.info_interval_ms,
            issues,
        ),
    }
    .sanitized()
}

/// The `[engine_view]` table with its `eval_bar` and `arrows` sub-tables
/// (spec M4-5, D455).
fn read_engine_view(
    root: &Table,
    defaults: &EngineViewPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> EngineViewPrefs {
    let top = Section::new(root, "engine_view", issues);
    let sub = |name: &'static str, issues: &mut Vec<PrefsIssue>| -> Section<'_> {
        let table = match top.value(name) {
            Some(Value::Table(table)) => Some(table),
            Some(_) => {
                top.invalid(name, issues);
                None
            }
            None => None,
        };
        Section {
            name: match name {
                "eval_bar" => "engine_view.eval_bar",
                "arrows" => "engine_view.arrows",
                _ => "engine_view.traffic_lights",
            },
            table,
        }
    };
    let bar = sub("eval_bar", issues);
    let arrows = sub("arrows", issues);
    let lights = sub("traffic_lights", issues);
    let d = defaults;
    EngineViewPrefs {
        eval_bar: EvalBarPrefs {
            show: bar.bool("show", d.eval_bar.show, issues),
            scale: bar
                .int(
                    "scale",
                    i64::from(EVAL_SCALE_RANGE.0),
                    i64::from(EVAL_SCALE_RANGE.1),
                    issues,
                )
                .map_or(d.eval_bar.scale, |n| {
                    u8::try_from(n).unwrap_or(d.eval_bar.scale)
                }),
            log: bar.bool("log", d.eval_bar.log, issues),
            ticks: bar.bool("ticks", d.eval_bar.ticks, issues),
            label: bar.bool("label", d.eval_bar.label, issues),
        },
        arrows: ArrowPrefs {
            enabled: arrows.bool("enabled", d.arrows.enabled, issues),
            mode: arrows.choice("mode", d.arrows.mode, ArrowMode::from_name, issues),
            number_lines: arrows.bool("number_lines", d.arrows.number_lines, issues),
            sequence_len: arrows
                .int(
                    "sequence_len",
                    i64::from(SEQUENCE_LEN_RANGE.0),
                    i64::from(SEQUENCE_LEN_RANGE.1),
                    issues,
                )
                .map_or(d.arrows.sequence_len, |n| {
                    u8::try_from(n).unwrap_or(d.arrows.sequence_len)
                }),
            sequence_side: arrows.choice(
                "sequence_side",
                d.arrows.sequence_side,
                SequenceSide::from_name,
                issues,
            ),
            click_to_play: arrows.bool("click_to_play", d.arrows.click_to_play, issues),
        },
        traffic_lights: TrafficLightPrefs {
            show: lights.bool("show", d.traffic_lights.show, issues),
        },
    }
}

/// The `[graph]` table (spec M4-7, D871).
/// The `[book]` table (spec M5-6, D1662).
fn read_book(
    root: &Table,
    defaults: &crate::book::BookPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::book::BookPrefs {
    let sec = Section::new(root, "book", issues);
    crate::book::BookPrefs {
        opening_book: sec
            .path("opening_book", issues)
            .or_else(|| defaults.opening_book.clone()),
    }
}

/// The `[report]` table (spec M4-11, D1667).
fn read_report(
    root: &Table,
    defaults: crate::report::ReportPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::report::ReportPrefs {
    let sec = Section::new(root, "report", issues);
    crate::report::ReportPrefs {
        review_arrow: sec.bool("review_arrow", defaults.review_arrow, issues),
    }
}

/// The `[report_config]` table (spec M4-10, D2100).
fn read_report_config(
    root: &Table,
    d: crate::report::ReportConfig,
    issues: &mut Vec<PrefsIssue>,
) -> crate::report::ReportConfig {
    let sec = Section::new(root, "report_config", issues);
    let number = |key: &str, default: u32, max: u32, issues: &mut Vec<PrefsIssue>| {
        sec.int(key, 0, i64::from(max), issues)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(default)
    };
    let tenths = |key: &str, default: u16, issues: &mut Vec<PrefsIssue>| {
        let max = u32::from(crate::report::LIMIT_MAX_TENTHS);
        u16::try_from(number(key, u32::from(default), max, issues)).unwrap_or(default)
    };
    crate::report::ReportConfig {
        by_time: sec.bool("by_time", d.by_time, issues),
        depth: number("depth", d.depth, 1000, issues),
        movetime_ms: number("movetime_ms", d.movetime_ms, 3_600_000, issues),
        best_tenths: tenths("best_tenths", d.best_tenths, issues),
        excellent_tenths: tenths("excellent_tenths", d.excellent_tenths, issues),
        good_tenths: tenths("good_tenths", d.good_tenths, issues),
        inaccuracy_tenths: tenths("inaccuracy_tenths", d.inaccuracy_tenths, issues),
        mistake_tenths: tenths("mistake_tenths", d.mistake_tenths, issues),
        board_marks: sec.bool("board_marks", d.board_marks, issues),
    }
    .sanitized()
}

fn read_graph(root: &Table, defaults: GraphPrefs, issues: &mut Vec<PrefsIssue>) -> GraphPrefs {
    let sec = Section::new(root, "graph", issues);
    GraphPrefs {
        kind: sec.choice("kind", defaults.kind, GraphKind::from_name, issues),
        invert: Invert {
            white: sec.bool("invert_white", defaults.invert.white, issues),
            black: sec.bool("invert_black", defaults.invert.black, issues),
        },
        highlight_current: sec.bool("highlight_current", defaults.highlight_current, issues),
        style: sec.choice("style", defaults.style, GraphStyle::from_name, issues),
    }
}

/// The `[merge]` table (spec M7-6, D3868): a ply out of range is invalid and
/// goes back to the default.
fn read_merge(
    root: &Table,
    defaults: crate::merge_game::MergePrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::merge_game::MergePrefs {
    let sec = Section::new(root, "merge", issues);
    let end_ply = match sec.int("end_ply", i64::MIN, i64::MAX, issues) {
        None => defaults.end_ply,
        Some(n) => match u32::try_from(n) {
            Ok(n) if n <= crate::merge_game::MAX_END_PLY => n,
            _ => {
                sec.invalid("end_ply", issues);
                defaults.end_ply
            }
        },
    };
    crate::merge_game::MergePrefs {
        end_ply,
        confirm_batch: sec.bool("confirm_batch", defaults.confirm_batch, issues),
    }
}

/// The `[opening_report]` table (spec M7-2, D3624): a value of the wrong
/// type or out of its range goes back to the default of its own field.
fn read_opening_report(
    root: &Table,
    defaults: &crate::opening_report::OpeningReportPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::opening_report::OpeningReportPrefs {
    use crate::opening_report::{BOARD_SIZE_RANGE, OpeningReportPrefs, ReportFormat, ReportPart};
    use mychess_db::report::{MAX_COUNT_OPTION, MAX_GAMES_LIMIT, MAX_TABLE_GAMES_LIMIT};
    let sec = Section::new(root, "opening_report", issues);
    // An integer inside `0..=max`; anything else is invalid.
    let mut number = |key: &str, max: u32, default: u32| -> u32 {
        match sec.int(key, i64::MIN, i64::MAX, issues) {
            None => default,
            Some(n) => match u32::try_from(n) {
                Ok(n) if n <= max => n,
                _ => {
                    sec.invalid(key, issues);
                    default
                }
            },
        }
    };
    let count_max = u32::from(MAX_COUNT_OPTION);
    let mut count = |key: &str, default: u8| -> u8 {
        u8::try_from(number(key, count_max, u32::from(default))).unwrap_or(default)
    };
    let oldest = count("oldest", defaults.oldest);
    let newest = count("newest", defaults.newest);
    let most_frequent = count("most_frequent", defaults.most_frequent);
    let high_rating = count("high_rating", defaults.high_rating);
    let shortest = count("shortest", defaults.shortest);
    let move_orders = count("move_orders", defaults.move_orders);
    let extra_moves = count("extra_moves", defaults.extra_moves);
    let max_table_games = number(
        "max_table_games",
        MAX_TABLE_GAMES_LIMIT,
        defaults.max_table_games,
    );
    let max_games = number("max_games", MAX_GAMES_LIMIT, defaults.max_games);
    let board_size = match sec.int("board_size", i64::MIN, i64::MAX, issues) {
        None => defaults.board_size,
        Some(n) => match u8::try_from(n) {
            Ok(n) if BOARD_SIZE_RANGE.contains(&n) => n,
            _ => {
                sec.invalid("board_size", issues);
                defaults.board_size
            }
        },
    };
    OpeningReportPrefs {
        stats: sec.bool("stats", defaults.stats, issues),
        popular: sec.bool("popular", defaults.popular, issues),
        avg_perf: sec.bool("avg_perf", defaults.avg_perf, issues),
        results: sec.bool("results", defaults.results, issues),
        moves_from: sec.bool("moves_from", defaults.moves_from, issues),
        themes: sec.bool("themes", defaults.themes, issues),
        endgames: sec.bool("endgames", defaults.endgames, issues),
        oldest,
        newest,
        most_frequent,
        most_frequent_white: sec.bool("most_frequent_white", defaults.most_frequent_white, issues),
        most_frequent_black: sec.bool("most_frequent_black", defaults.most_frequent_black, issues),
        high_rating,
        shortest,
        shortest_white: sec.bool("shortest_white", defaults.shortest_white, issues),
        shortest_black: sec.bool("shortest_black", defaults.shortest_black, issues),
        move_orders,
        max_table_games,
        max_games,
        extra_moves,
        last_part: sec.choice(
            "last_part",
            defaults.last_part,
            ReportPart::from_name,
            issues,
        ),
        last_format: sec.choice(
            "last_format",
            defaults.last_format,
            ReportFormat::from_name,
            issues,
        ),
        last_dir: sec
            .path("last_dir", issues)
            .or_else(|| defaults.last_dir.clone()),
        board_size,
    }
    .sanitized()
}

/// An integer of `sec.key` inside `range`; anything else is invalid and
/// gives `default`.
fn ranged_u32(
    sec: &Section<'_>,
    key: &str,
    range: std::ops::RangeInclusive<u32>,
    default: u32,
    issues: &mut Vec<PrefsIssue>,
) -> u32 {
    match sec.int(key, i64::MIN, i64::MAX, issues) {
        None => default,
        Some(n) => match u32::try_from(n) {
            Ok(n) if range.contains(&n) => n,
            _ => {
                sec.invalid(key, issues);
                default
            }
        },
    }
}

/// The `[export]` table (spec M7-4, D3729) with its sub-tables: a wrong
/// value only resets its own field.
#[allow(clippy::too_many_lines)] // one line per option
fn read_export_docs(
    root: &Table,
    defaults: &crate::export_docs::ExportPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::export_docs::ExportPrefs {
    use crate::board_image::{MAX_MARGIN_PX, SIZE_RANGE};
    use crate::export_docs::{
        CsvPrefs, CsvSeparator, EpdPrefs, ExportFormat, ExportPrefs, HtmlPrefs, ImageFormat,
        ImagePrefs, LatexPrefs, MAX_DIAGRAM_PX, MAX_LIST_FORMAT_CHARS, MAX_PER_FILE,
        MAX_TEMPLATE_BYTES, ViewerPrefs,
    };
    let top = Section::new(root, "export", issues);
    let empty = Table::new();
    let table = top.table.unwrap_or(&empty);
    let html = Section::new(table, "html", issues);
    let viewer = Section::new(table, "viewer", issues);
    let latex = Section::new(table, "latex", issues);
    let epd = Section::new(table, "epd", issues);
    let csv = Section::new(table, "csv", issues);
    let image = Section::new(table, "image", issues);
    let d = defaults;
    // Text with a size limit: a longer one is invalid.
    let template = |key: &str, default: &str, issues: &mut Vec<PrefsIssue>| -> String {
        let text = top.text(key, default, issues);
        if text.len() > MAX_TEMPLATE_BYTES {
            top.invalid(key, issues);
            default.to_owned()
        } else {
            text
        }
    };
    let list_format = {
        let text = top.text("list_format", &d.list_format, issues);
        if text.chars().count() > MAX_LIST_FORMAT_CHARS || text.chars().any(char::is_control) {
            top.invalid("list_format", issues);
            d.list_format.clone()
        } else {
            text
        }
    };
    ExportPrefs {
        format: top.choice("format", d.format, ExportFormat::from_name, issues),
        html: HtmlPrefs {
            comments: html.bool("comments", d.html.comments, issues),
            variations: html.bool("variations", d.html.variations, issues),
            symbols: html.bool("symbols", d.html.symbols, issues),
            diagram_px: u16::try_from(ranged_u32(
                &html,
                "diagram_px",
                0..=u32::from(MAX_DIAGRAM_PX),
                u32::from(d.html.diagram_px),
                issues,
            ))
            .unwrap_or(d.html.diagram_px),
        },
        viewer: ViewerPrefs {
            per_file: ranged_u32(
                &viewer,
                "per_file",
                1..=MAX_PER_FILE,
                d.viewer.per_file,
                issues,
            ),
            include_pgn: viewer.bool("include_pgn", d.viewer.include_pgn, issues),
            open_after: viewer.bool("open_after", d.viewer.open_after, issues),
        },
        latex: LatexPrefs {
            comments: latex.bool("comments", d.latex.comments, issues),
            variations: latex.bool("variations", d.latex.variations, issues),
            flip: latex.bool("flip", d.latex.flip, issues),
            convert_null_moves: latex.bool(
                "convert_null_moves",
                d.latex.convert_null_moves,
                issues,
            ),
        },
        epd: EpdPrefs {
            dedupe: epd.bool("dedupe", d.epd.dedupe, issues),
            final_position: epd.bool("final_position", d.epd.final_position, issues),
            hmvc_fmvn: epd.bool("hmvc_fmvn", d.epd.hmvc_fmvn, issues),
            id: epd.bool("id", d.epd.id, issues),
            game_number: epd.bool("game_number", d.epd.game_number, issues),
        },
        list_format,
        csv: CsvPrefs {
            header: csv.bool("header", d.csv.header, issues),
            separator: csv.choice(
                "separator",
                d.csv.separator,
                CsvSeparator::from_name,
                issues,
            ),
            bom: csv.bool("bom", d.csv.bom, issues),
        },
        image: ImagePrefs {
            format: image.choice("format", d.image.format, ImageFormat::from_name, issues),
            size_px: ranged_u32(&image, "size_px", SIZE_RANGE, d.image.size_px, issues),
            coordinates: image.bool("coordinates", d.image.coordinates, issues),
            last_move: image.bool("last_move", d.image.last_move, issues),
            marks: image.bool("marks", d.image.marks, issues),
            transparent: image.bool("transparent", d.image.transparent, issues),
            margin_px: ranged_u32(
                &image,
                "margin_px",
                0..=MAX_MARGIN_PX,
                d.image.margin_px,
                issues,
            ),
        },
        html_start: template("html_start", &d.html_start, issues),
        html_end: template("html_end", &d.html_end, issues),
        latex_start: template("latex_start", &d.latex_start, issues),
        latex_end: template("latex_end", &d.latex_end, issues),
    }
}

/// The `[manual]` table (spec M9-5, D4060): a step out of range is invalid
/// and goes back to the default.
fn read_manual(
    root: &Table,
    defaults: crate::manual::ManualPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::manual::ManualPrefs {
    let sec = Section::new(root, "manual", issues);
    let text_scale = match sec.int("text_scale", i64::MIN, i64::MAX, issues) {
        None => defaults.text_scale,
        Some(n) => match u8::try_from(n) {
            Ok(n) if n <= crate::manual::MAX_TEXT_SCALE => n,
            _ => {
                sec.invalid("text_scale", issues);
                defaults.text_scale
            }
        },
    };
    crate::manual::ManualPrefs { text_scale }
}

/// The `[start]` table (spec M9-4): a tip number out of range goes back to
/// the first tip.
fn read_start(
    root: &Table,
    defaults: crate::start::StartPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::start::StartPrefs {
    let sec = Section::new(root, "start", issues);
    let next_tip = sec
        .int("next_tip", 0, i64::from(u32::MAX), issues)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(defaults.next_tip);
    crate::start::StartPrefs {
        welcome_done: sec.bool("welcome_done", defaults.welcome_done, issues),
        show_tip: sec.bool("show_tip", defaults.show_tip, issues),
        next_tip,
    }
    .sanitized()
}

/// The `[epd]` table (spec M7-5, D3823): a wrong value only resets its own
/// field; the seconds are limited and the recent list is cut.
fn read_epd(
    root: &Table,
    defaults: &crate::epd_window::EpdPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::epd_window::EpdPrefs {
    use crate::epd_window::{AnnotateMode, Dialect, SECONDS_RANGE};
    let sec = Section::new(root, "epd", issues);
    let recent = match sec.value("recent") {
        None => defaults.recent.clone(),
        Some(Value::Array(items)) => {
            // A path that is not a string is dropped; one remark however
            // many there are.
            let mut bad = false;
            let kept = items
                .iter()
                .filter_map(|item| match item {
                    Value::String(s) if !s.is_empty() => Some(PathBuf::from(s)),
                    _ => {
                        bad = true;
                        None
                    }
                })
                .collect();
            if bad {
                sec.invalid("recent", issues);
            }
            kept
        }
        Some(_) => {
            sec.invalid("recent", issues);
            defaults.recent.clone()
        }
    };
    let annotate_engine = match sec.int("annotate_engine", 0, i64::MAX, issues) {
        None => defaults.annotate_engine,
        Some(n) => u64::try_from(n).ok(),
    };
    crate::epd_window::EpdPrefs {
        flip_to_side_to_move: sec.bool(
            "flip_to_side_to_move",
            defaults.flip_to_side_to_move,
            issues,
        ),
        annotate_seconds: sec
            .int(
                "annotate_seconds",
                i64::from(*SECONDS_RANGE.start()),
                i64::from(*SECONDS_RANGE.end()),
                issues,
            )
            .map_or(defaults.annotate_seconds, to_u32),
        annotate_mode: sec.choice(
            "annotate_mode",
            defaults.annotate_mode,
            AnnotateMode::from_name,
            issues,
        ),
        annotate_engine,
        dialect: sec.choice("dialect", defaults.dialect, Dialect::from_name, issues),
        last_dir: sec
            .path("last_dir", issues)
            .or_else(|| defaults.last_dir.clone()),
        recent,
        confirm_run_undo: sec.bool("confirm_run_undo", defaults.confirm_run_undo, issues),
    }
    .sanitized()
}

/// The `[tree]` table (spec M5-2, D970).
fn read_tree(root: &Table, defaults: TreePrefs, issues: &mut Vec<PrefsIssue>) -> TreePrefs {
    let sec = Section::new(root, "tree", issues);
    TreePrefs {
        short: sec.bool("short", defaults.short, issues),
        sort: sec.choice("sort", defaults.sort, TreeSort::from_name, issues),
        fast_mode: sec.choice(
            "fast_mode",
            defaults.fast_mode,
            TreeFastMode::from_name,
            issues,
        ),
        auto_adjust: sec.bool("auto_adjust", defaults.auto_adjust, issues),
        cache_size: sec
            .int(
                "cache_size",
                to_i64(*CACHE_SIZE_RANGE.start()),
                to_i64(*CACHE_SIZE_RANGE.end()),
                issues,
            )
            .map_or(defaults.cache_size, |n| {
                usize::try_from(n).unwrap_or(defaults.cache_size)
            }),
        cache_autosave: sec.bool("cache_autosave", defaults.cache_autosave, issues),
    }
}

/// The `[advanced_search]` table (spec M6-1, D3249 item 5).
fn read_advanced_search(
    root: &Table,
    defaults: AdvancedSearchPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> AdvancedSearchPrefs {
    let sec = Section::new(root, "advanced_search", issues);
    let number = |key: &str, max: u16, default: u16, issues: &mut Vec<PrefsIssue>| -> u16 {
        sec.int(key, 0, i64::from(max), issues)
            .and_then(|n| u16::try_from(n).ok())
            .unwrap_or(default)
    };
    AdvancedSearchPrefs {
        board_op: sec.choice("board_op", defaults.board_op, SearchOp::from_name, issues),
        board_kind: sec.choice(
            "board_kind",
            defaults.board_kind,
            BoardKind::from_name,
            issues,
        ),
        board_variations: sec.bool("board_variations", defaults.board_variations, issues),
        board_ignore_colors: sec.bool("board_ignore_colors", defaults.board_ignore_colors, issues),
        material_op: sec.choice(
            "material_op",
            defaults.material_op,
            SearchOp::from_name,
            issues,
        ),
        material_ignore_colors: sec.bool(
            "material_ignore_colors",
            defaults.material_ignore_colors,
            issues,
        ),
        material_end_only: sec.bool("material_end_only", defaults.material_end_only, issues),
        material_bishops: sec.choice(
            "material_bishops",
            defaults.material_bishops,
            BishopsKind::from_name,
            issues,
        ),
        material_move_min: number(
            "material_move_min",
            MAX_MOVE_NUMBER,
            defaults.material_move_min,
            issues,
        ),
        material_move_max: number(
            "material_move_max",
            MAX_MOVE_NUMBER,
            defaults.material_move_max,
            issues,
        ),
        material_min_plies: number(
            "material_min_plies",
            MAX_MIN_PLIES,
            defaults.material_min_plies,
            issues,
        ),
        moves_op: sec.choice("moves_op", defaults.moves_op, SearchOp::from_name, issues),
        moves_side: sec.choice(
            "moves_side",
            defaults.moves_side,
            SideKind::from_name,
            issues,
        ),
        moves_check: sec.choice(
            "moves_check",
            defaults.moves_check,
            CheckKind::from_name,
            issues,
        ),
        moves_variations: sec.bool("moves_variations", defaults.moves_variations, issues),
    }
}

/// The `[pattern_search]` table (spec M6-2, slice c): a wrong value only
/// resets its own field.
fn read_pattern_search(
    root: &Table,
    defaults: &PatternSearchPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> PatternSearchPrefs {
    let sec = Section::new(root, "pattern_search", issues);
    let number = |key: &str, max: u16, default: u16, issues: &mut Vec<PrefsIssue>| -> u16 {
        sec.int(key, 0, i64::from(max), issues)
            .and_then(|n| u16::try_from(n).ok())
            .unwrap_or(default)
    };
    let hold = sec
        .int("hold", 1, i64::from(MAX_HOLD), issues)
        .and_then(|n| u8::try_from(n).ok())
        .unwrap_or(defaults.hold);
    let last = sec.text("last_preset", &defaults.last_preset, issues);
    let sane = PatternSearchPrefs {
        last_preset: last.clone(),
        ..PatternSearchPrefs::default()
    }
    .sanitized();
    if sane.last_preset != last {
        sec.invalid("last_preset", issues);
    }
    PatternSearchPrefs {
        op: sec.choice("op", defaults.op, SearchOp::from_name, issues),
        min_move: number("min_move", MAX_SCOPE_MOVE, defaults.min_move, issues),
        max_move: number("max_move", MAX_SCOPE_MOVE, defaults.max_move, issues),
        hold,
        end_only: sec.bool("end_only", defaults.end_only, issues),
        ignore_colors: sec.bool("ignore_colors", defaults.ignore_colors, issues),
        last_preset: sane.last_preset,
    }
}

/// The `[tree_mask]` table (spec M5-4b, D1861): unknown keys are ignored and
/// a wrong value only resets its own field.
fn read_tree_mask(
    root: &Table,
    defaults: &crate::tree::MaskPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::tree::MaskPrefs {
    let sec = Section::new(root, "tree_mask", issues);
    let recent = match sec.value("recent") {
        None => defaults.recent.clone(),
        Some(Value::Array(items)) => {
            // A path that is not a string, or not absolute, is dropped; one
            // remark however many there are (D1879).
            let mut bad = false;
            let kept = items
                .iter()
                .filter_map(|item| match item {
                    Value::String(s) if Path::new(s).is_absolute() => Some(PathBuf::from(s)),
                    _ => {
                        bad = true;
                        None
                    }
                })
                .collect();
            if bad {
                sec.invalid("recent", issues);
            }
            kept
        }
        Some(_) => {
            sec.invalid("recent", issues);
            defaults.recent.clone()
        }
    };
    crate::tree::MaskPrefs {
        recent,
        auto_load: sec.bool("auto_load", defaults.auto_load, issues),
        show_markers: sec.bool("show_markers", defaults.show_markers, issues),
        tooltips: sec.bool("tooltips", defaults.tooltips, issues),
        last_dir: sec
            .path("last_dir", issues)
            .or_else(|| defaults.last_dir.clone()),
    }
    .sanitized()
}

fn to_i64(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn read_notation(
    sec: &Section<'_>,
    defaults: &NotationOptions,
    issues: &mut Vec<PrefsIssue>,
) -> NotationOptions {
    NotationOptions {
        layout: sec.choice("layout", defaults.layout, NotationLayout::from_name, issues),
        indent_comments: sec.bool("indent_comments", defaults.indent_comments, issues),
        symbolic_nags: sec.bool("symbolic_nags", defaults.symbolic_nags, issues),
        move_number_space: sec.bool("move_number_space", defaults.move_number_space, issues),
        header: sec.choice("header", defaults.header, HeaderMode::from_name, issues),
        strip_marks: sec.bool("strip_marks", defaults.strip_marks, issues),
        bold_main_line: sec.bool("bold_main_line", defaults.bold_main_line, issues),
        show_comments: sec.bool("show_comments", defaults.show_comments, issues),
        show_variations: sec.bool("show_variations", defaults.show_variations, issues),
        show_result: sec.bool("show_result", defaults.show_result, issues),
        hover_preview: sec
            .int(
                "hover_preview_ms",
                0,
                i64::from(HoverPreview::MAX_DELAY_MS),
                issues,
            )
            .map_or(defaults.hover_preview, |ms| {
                HoverPreview::from_millis(to_u32(ms))
            }),
        figurines: sec.bool("figurines", defaults.figurines, issues),
        show_time_column: sec.bool("show_time_column", defaults.show_time_column, issues),
    }
}

fn read_recent(files: &Section<'_>, issues: &mut Vec<PrefsIssue>) -> RecentFiles {
    let mut recent = RecentFiles::default();
    match files.value("recent") {
        None => {}
        Some(Value::Array(items)) => {
            // The file lists the newest first; `push` puts the last pushed
            // first, so push in reverse.
            for item in items.iter().rev() {
                match item {
                    Value::String(s) if !s.is_empty() => recent.push(PathBuf::from(s)),
                    _ => files.invalid("recent", issues),
                }
            }
        }
        Some(_) => files.invalid("recent", issues),
    }
    recent
}

fn read_shortcuts(root: &Table, issues: &mut Vec<PrefsIssue>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    match root.get("shortcuts") {
        None => {}
        Some(Value::Table(table)) => {
            for (action, value) in table {
                match value {
                    Value::String(text) => {
                        out.insert(action.clone(), text.clone());
                    }
                    _ => issues.push(PrefsIssue::InvalidValue {
                        key: format!("shortcuts.{action}"),
                    }),
                }
            }
        }
        Some(_) => issues.push(PrefsIssue::InvalidValue {
            key: "shortcuts".to_owned(),
        }),
    }
    out
}

/// The columns of the game list. A bad entry is dropped (with a remark);
/// a missing or empty list gives the default columns.
fn read_game_list(root: &Table, issues: &mut Vec<PrefsIssue>) -> GameListPrefs {
    let invalid = |issues: &mut Vec<PrefsIssue>| {
        issues.push(PrefsIssue::InvalidValue {
            key: "game_list.columns".to_owned(),
        });
    };
    let Some(Value::Table(section)) = root.get("game_list") else {
        if root.contains_key("game_list") {
            invalid(issues);
        }
        return GameListPrefs::default();
    };
    let items = match section.get("columns") {
        None => return GameListPrefs::default(),
        Some(Value::Array(items)) => items,
        Some(_) => {
            invalid(issues);
            return GameListPrefs::default();
        }
    };
    let mut columns = Vec::new();
    let mut bad = false;
    for item in items {
        let parsed = (|| {
            let table = item.as_table()?;
            let column = ListColumn::from_name(table.get("column")?.as_str()?)?;
            let mut setup = ColumnSetup::new(column);
            if let Some(value) = table.get("align") {
                setup.align = ColumnAlign::from_name(value.as_str()?)?;
            }
            if let Some(value) = table.get("chars") {
                let n = value.as_integer()?;
                setup.chars = u16::try_from(n.clamp(0, i64::from(u16::MAX))).ok()?;
            }
            Some(setup)
        })();
        match parsed {
            Some(setup)
                if columns
                    .iter()
                    .all(|c: &ColumnSetup| c.column != setup.column) =>
            {
                columns.push(setup);
            }
            _ => bad = true,
        }
    }
    if bad {
        invalid(issues);
    }
    if columns
        .iter()
        .any(|c| !(MIN_COLUMN_CHARS..=MAX_COLUMN_CHARS).contains(&c.chars))
    {
        issues.push(PrefsIssue::Clamped {
            key: "game_list.columns".to_owned(),
        });
    }
    GameListPrefs { columns }.sanitized()
}

fn read_layout(root: &Table, issues: &mut Vec<PrefsIssue>) -> Option<LayoutNode> {
    let value = root.get("layout")?;
    let layout = LayoutNode::from_value(value).filter(LayoutNode::is_valid);
    // The old default becomes the new one (M2-5 D213); a customised layout
    // is kept as it is.
    let layout = layout.map(|l| {
        if l.is_legacy_default() {
            LayoutNode::default_layout()
        } else {
            l
        }
    });
    if layout.is_none() {
        issues.push(PrefsIssue::InvalidValue {
            key: "layout".to_owned(),
        });
    }
    layout
}

/// The `[crosstable]` table (spec M6-4, D3391): a wrong value only resets
/// its own field.
fn read_crosstable(
    root: &Table,
    defaults: &crate::crosstable::CrosstablePrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::crosstable::CrosstablePrefs {
    use crate::crosstable::{CrossFormat, CrossSort, CrosstablePrefs};
    let sec = Section::new(root, "crosstable", issues);
    let row_color = sec.text("row_color", &defaults.row_color, issues);
    CrosstablePrefs {
        format: sec.choice("format", defaults.format, CrossFormat::from_name, issues),
        sort: sec.choice("sort", defaults.sort, CrossSort::from_name, issues),
        show_ages: sec.bool("show_ages", defaults.show_ages, issues),
        show_countries: sec.bool("show_countries", defaults.show_countries, issues),
        show_tallies: sec.bool("show_tallies", defaults.show_tallies, issues),
        show_ratings: sec.bool("show_ratings", defaults.show_ratings, issues),
        show_titles: sec.bool("show_titles", defaults.show_titles, issues),
        show_tiebreaks: sec.bool("show_tiebreaks", defaults.show_tiebreaks, issues),
        include_deleted: sec.bool("include_deleted", defaults.include_deleted, issues),
        swiss_colors: sec.bool("swiss_colors", defaults.swiss_colors, issues),
        numbered_columns: sec.bool("numbered_columns", defaults.numbered_columns, issues),
        head_to_head: sec.bool("head_to_head", defaults.head_to_head, issues),
        tiebreak_wins: sec.bool("tiebreak_wins", defaults.tiebreak_wins, issues),
        three_points: sec.bool("three_points", defaults.three_points, issues),
        separate_groups: sec.bool("separate_groups", defaults.separate_groups, issues),
        color_rows: sec.bool("color_rows", defaults.color_rows, issues),
        row_color,
    }
    .sanitized()
}

/// A list of small numbers; `None` when the key is absent or not a list.
fn read_number_list(
    sec: &Section<'_>,
    key: &str,
    issues: &mut Vec<PrefsIssue>,
) -> Option<Vec<u16>> {
    match sec.value(key) {
        None => None,
        Some(Value::Array(items)) => Some(
            items
                .iter()
                .filter_map(|v| v.as_integer().and_then(|n| u16::try_from(n).ok()))
                .collect(),
        ),
        Some(_) => {
            sec.invalid(key, issues);
            None
        }
    }
}

/// The `[stats]` table (spec M6-4, D3371).
fn read_stats(root: &Table, issues: &mut Vec<PrefsIssue>) -> crate::charts::StatsPrefs {
    let sec = Section::new(root, "stats", issues);
    crate::charts::StatsPrefs {
        elo_checked: read_number_list(&sec, "elo_checked", issues),
        years_checked: read_number_list(&sec, "years_checked", issues),
        years_known: sec
            .int("years_known", 0, 3000, issues)
            .and_then(|y| u16::try_from(y).ok()),
    }
    .sanitized()
}

/// The `[filter_graph]` table (spec M6-4, D3391).
fn read_filter_graph(
    root: &Table,
    defaults: &crate::charts::FilterGraphPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::charts::FilterGraphPrefs {
    use crate::charts::{FilterGraphKind, FilterGraphPrefs, StatsTab};
    let sec = Section::new(root, "filter_graph", issues);
    let mut number = |key: &str, default: i32| -> i32 {
        sec.int(key, 0, i64::from(mychess_db::MAX_GRAPH_FIELD), issues)
            .and_then(|n| i32::try_from(n).ok())
            .unwrap_or(default)
    };
    let (year_min, year_max, year_step) = (
        number("year_min", defaults.year_min),
        number("year_max", defaults.year_max),
        number("year_step", defaults.year_step),
    );
    let (elo_min, elo_max, elo_step) = (
        number("elo_min", defaults.elo_min),
        number("elo_max", defaults.elo_max),
        number("elo_step", defaults.elo_step),
    );
    let (moves_min, moves_max, moves_step) = (
        number("moves_min", defaults.moves_min),
        number("moves_max", defaults.moves_max),
        number("moves_step", defaults.moves_step),
    );
    FilterGraphPrefs {
        tab: sec.choice("tab", defaults.tab, StatsTab::from_name, issues),
        kind: sec.choice("kind", defaults.kind, FilterGraphKind::from_name, issues),
        year_min,
        year_max,
        year_step,
        elo_min,
        elo_max,
        elo_step,
        moves_min,
        moves_max,
        moves_step,
        estimate: sec.bool("estimate", defaults.estimate, issues),
        show_dots: sec.bool("show_dots", defaults.show_dots, issues),
    }
    .sanitized()
}

/// The `[rating_graph]` table (spec M6-4, D3391).
fn read_rating_graph(
    root: &Table,
    defaults: crate::charts::RatingPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::charts::RatingPrefs {
    let sec = Section::new(root, "rating_graph", issues);
    let mut number = |key: &str, default: u16| -> u16 {
        sec.int(key, 0, i64::from(u16::MAX), issues)
            .and_then(|n| u16::try_from(n).ok())
            .unwrap_or(default)
    };
    let (start_year, start_elo) = (
        number("start_year", defaults.start_year),
        number("start_elo", defaults.start_elo),
    );
    crate::charts::RatingPrefs {
        start_year,
        start_elo,
        only_filter: sec.bool("only_filter", defaults.only_filter, issues),
        show_dots: sec.bool("show_dots", defaults.show_dots, issues),
    }
    .sanitized()
}

/// The `[ptracker]` table (spec M6-4, D3391).
fn read_ptracker(
    root: &Table,
    defaults: &crate::piece_tracker::PieceTrackerPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::piece_tracker::PieceTrackerPrefs {
    use crate::piece_tracker::{PieceTrackerPrefs, TrackerMode, TrackerScheme};
    let sec = Section::new(root, "ptracker", issues);
    let squares = match sec.value("squares") {
        None => defaults.squares.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| {
                v.as_str()
                    .and_then(|s| s.parse::<mychess_core::Square>().ok())
            })
            .collect(),
        Some(_) => {
            sec.invalid("squares", issues);
            defaults.squares.clone()
        }
    };
    let mut number = |key: &str, default: u16| -> u16 {
        sec.int(
            key,
            0,
            i64::from(crate::piece_tracker::MAX_TRACK_MOVE),
            issues,
        )
        .and_then(|n| u16::try_from(n).ok())
        .unwrap_or(default)
    };
    let (start_move, end_move) = (
        number("start_move", defaults.start_move),
        number("end_move", defaults.end_move),
    );
    PieceTrackerPrefs {
        squares,
        start_move,
        end_move,
        mode: sec.choice("mode", defaults.mode, TrackerMode::from_name, issues),
        scheme: sec.choice("scheme", defaults.scheme, TrackerScheme::from_name, issues),
    }
    .sanitized()
}

/// A sub-table of a section (`[maintenance.twins]` or the inline table
/// `twins = { ... }`, which TOML reads the same way).
fn subsection<'a>(
    parent: &Section<'a>,
    key: &str,
    name: &'static str,
    issues: &mut Vec<PrefsIssue>,
) -> Section<'a> {
    let table = match parent.value(key) {
        None => None,
        Some(Value::Table(table)) => Some(table),
        Some(_) => {
            parent.invalid(key, issues);
            None
        }
    };
    Section { name, table }
}

/// The `[maintenance]` table (spec M7-1, D3530): a value of the wrong type
/// or out of its range goes back to the default of its own field.
fn read_maintenance(
    root: &Table,
    defaults: &crate::maintenance::MaintenancePrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::maintenance::MaintenancePrefs {
    use crate::maintenance::{
        CleanerPrefs, DeleteChoice, MaintenancePrefs, NameScopeChoice, ScopeChoice, SortCriterion,
        TwinPrefs,
    };
    let sec = Section::new(root, "maintenance", issues);
    let t = subsection(&sec, "twins", "maintenance.twins", issues);
    let d = &defaults.twins;
    let twins = TwinPrefs {
        exact_names: t.bool("exact_names", d.exact_names, issues),
        same_colors: t.bool("same_colors", d.same_colors, issues),
        same_event: t.bool("same_event", d.same_event, issues),
        same_site: t.bool("same_site", d.same_site, issues),
        same_round: t.bool("same_round", d.same_round, issues),
        same_year: t.bool("same_year", d.same_year, issues),
        same_month: t.bool("same_month", d.same_month, issues),
        same_day: t.bool("same_day", d.same_day, issues),
        same_result: t.bool("same_result", d.same_result, issues),
        same_eco: t.bool("same_eco", d.same_eco, issues),
        same_moves: t.bool("same_moves", d.same_moves, issues),
        skip_short: t.bool("skip_short", d.skip_short, issues),
        undelete_first: t.bool("undelete_first", d.undelete_first, issues),
        set_filter: t.bool("set_filter", d.set_filter, issues),
        keep_commented: t.bool("keep_commented", d.keep_commented, issues),
        keep_with_variations: t.bool("keep_with_variations", d.keep_with_variations, issues),
        copy_ratings: t.bool("copy_ratings", d.copy_ratings, issues),
        only_filter: t.bool("only_filter", d.only_filter, issues),
        delete: t.choice("delete", d.delete, DeleteChoice::from_name, issues),
    };
    let c = subsection(&sec, "cleaner", "maintenance.cleaner", issues);
    let d = &defaults.cleaner;
    let cleaner = CleanerPrefs {
        players: c.bool("players", d.players, issues),
        events: c.bool("events", d.events, issues),
        sites: c.bool("sites", d.sites, issues),
        rounds: c.bool("rounds", d.rounds, issues),
        eco: c.bool("eco", d.eco, issues),
        elo: c.bool("elo", d.elo, issues),
        twins: c.bool("twins", d.twins, issues),
        compact_names: c.bool("compact_names", d.compact_names, issues),
        compact_games: c.bool("compact_games", d.compact_games, issues),
        tree: c.bool("tree", d.tree, issues),
    };
    // A list with one wrong name goes back to the default list as a whole.
    let sort_keys = match sec.value("sort_keys") {
        None => defaults.sort_keys.clone(),
        Some(Value::Array(items)) => {
            let keys: Option<Vec<SortCriterion>> = items
                .iter()
                .map(|v| v.as_str().and_then(SortCriterion::from_name))
                .collect();
            keys.unwrap_or_else(|| {
                sec.invalid("sort_keys", issues);
                defaults.sort_keys.clone()
            })
        }
        Some(_) => {
            sec.invalid("sort_keys", issues);
            defaults.sort_keys.clone()
        }
    };
    MaintenancePrefs {
        twins,
        cleaner,
        sort_keys,
        sort_reverse: sec.bool("sort_reverse", defaults.sort_reverse, issues),
        strip_scope: sec.choice(
            "strip_scope",
            defaults.strip_scope,
            ScopeChoice::from_name,
            issues,
        ),
        tag_scope: sec.choice(
            "tag_scope",
            defaults.tag_scope,
            ScopeChoice::from_name,
            issues,
        ),
        name_editor_scope: sec.choice(
            "name_editor_scope",
            defaults.name_editor_scope,
            NameScopeChoice::from_name,
            issues,
        ),
        compact_after_rewrite: sec.bool(
            "compact_after_rewrite",
            defaults.compact_after_rewrite,
            issues,
        ),
    }
    .sanitized()
}

/// The `[player_report]` table (spec M7-3, D3663): a value of the wrong type
/// or out of its range goes back to the default of its own field.
fn read_player_report(
    root: &Table,
    defaults: &crate::player_report::PlayerReportPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> crate::player_report::PlayerReportPrefs {
    use crate::player_report::{
        NameMatching, PlayerColor, PlayerReportPrefs, PlayerStart, ReportFormat, ReportPart,
    };
    use mychess_db::report::{MAX_COUNT_OPTION, MAX_TABLE_GAMES_LIMIT};
    let sec = Section::new(root, "player_report", issues);
    // An integer inside `0..=max`; anything else is invalid.
    let mut number = |key: &str, max: u32, default: u32| -> u32 {
        match sec.int(key, i64::MIN, i64::MAX, issues) {
            None => default,
            Some(n) => match u32::try_from(n) {
                Ok(n) if n <= max => n,
                _ => {
                    sec.invalid(key, issues);
                    default
                }
            },
        }
    };
    let count_max = u32::from(MAX_COUNT_OPTION);
    let mut count = |key: &str, default: u8| -> u8 {
        u8::try_from(number(key, count_max, u32::from(default))).unwrap_or(default)
    };
    let oldest = count("oldest", defaults.oldest);
    let newest = count("newest", defaults.newest);
    let opponents = count("opponents", defaults.opponents);
    let high_rating = count("high_rating", defaults.high_rating);
    let eco_codes = count("eco_codes", defaults.eco_codes);
    let extra_moves = count("extra_moves", defaults.extra_moves);
    let max_table_games = number(
        "max_table_games",
        MAX_TABLE_GAMES_LIMIT,
        defaults.max_table_games,
    );
    PlayerReportPrefs {
        stats: sec.bool("stats", defaults.stats, issues),
        oldest,
        newest,
        opponents,
        results: sec.bool("results", defaults.results, issues),
        avg_perf: sec.bool("avg_perf", defaults.avg_perf, issues),
        high_rating,
        eco_codes,
        themes: sec.bool("themes", defaults.themes, issues),
        endgames: sec.bool("endgames", defaults.endgames, issues),
        max_table_games,
        extra_moves,
        last_color: sec.choice(
            "last_color",
            defaults.last_color,
            PlayerColor::from_name,
            issues,
        ),
        last_start: sec.choice(
            "last_start",
            defaults.last_start,
            PlayerStart::from_name,
            issues,
        ),
        matching: sec.choice(
            "matching",
            defaults.matching,
            NameMatching::from_name,
            issues,
        ),
        copy_to_clipbase: sec.bool("copy_to_clipbase", defaults.copy_to_clipbase, issues),
        last_part: sec.choice(
            "last_part",
            defaults.last_part,
            ReportPart::from_name,
            issues,
        ),
        last_format: sec.choice(
            "last_format",
            defaults.last_format,
            ReportFormat::from_name,
            issues,
        ),
        last_dir: sec
            .path("last_dir", issues)
            .or_else(|| defaults.last_dir.clone()),
    }
    .sanitized()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::{load_preferences, render_preferences};

    #[test]
    fn missing_fields_and_unknown_keys_are_ignored() {
        let loaded = load_preferences(
            "[board]\ntheme = \"wood\"\nunknown = 1\n[mystery]\nx = 1\nextra = true\n",
        );
        assert_eq!(loaded.prefs.board.theme, "wood");
        assert_eq!(loaded.prefs.board.piece_set, "cburnett");
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn absurd_window_sizes_are_clamped() {
        let loaded = load_preferences("[window]\nwidth = 4000000000\nheight = 600\n");
        assert_eq!(loaded.prefs.window.width, Some(MAX_WINDOW_SIZE));
        assert_eq!(loaded.prefs.window.height, Some(600));
        assert_eq!(
            loaded.issues,
            vec![PrefsIssue::Clamped {
                key: "window.width".to_owned()
            }]
        );
        let edge = format!("[window]\nwidth = {MAX_WINDOW_SIZE}\nheight = {MAX_WINDOW_SIZE}\n");
        let loaded = load_preferences(&edge);
        assert_eq!(loaded.prefs.window.width, Some(MAX_WINDOW_SIZE));
        assert!(loaded.issues.is_empty());
    }

    #[test]
    fn every_accent_round_trips_through_toml() {
        for accent in [
            AccentPref::Mychess,
            AccentPref::Blue,
            AccentPref::Purple,
            AccentPref::Pink,
            AccentPref::Red,
            AccentPref::Orange,
            AccentPref::Yellow,
            AccentPref::Green,
            AccentPref::Gray,
        ] {
            let mut prefs = Preferences::default();
            prefs.general.accent = accent;
            let loaded = load_preferences(&render_preferences(&prefs));
            assert_eq!(loaded.prefs.general.accent, accent);
            assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
        }
    }

    #[test]
    fn the_menu_style_and_collapsed_ribbon_round_trip_through_toml() {
        for style in [MenuStylePref::Dropdown, MenuStylePref::Ribbon] {
            for collapsed in [false, true] {
                let mut prefs = Preferences::default();
                prefs.general.menu_style = style;
                prefs.general.ribbon_collapsed = collapsed;
                let text = render_preferences(&prefs);
                assert!(text.contains(&format!("menu_style = \"{}\"", style.as_str())));
                let loaded = load_preferences(&text);
                assert_eq!(loaded.prefs.general.menu_style, style);
                assert_eq!(loaded.prefs.general.ribbon_collapsed, collapsed);
                assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
            }
        }
    }

    #[test]
    fn missing_menu_keys_are_the_defaults_and_bad_ones_warn() {
        let old = load_preferences("[general]\ntheme = \"dark\"\n");
        assert_eq!(old.prefs.general.menu_style, MenuStylePref::Dropdown);
        assert!(!old.prefs.general.ribbon_collapsed);
        assert!(old.issues.is_empty());
        let bad = load_preferences(
            "[general]\nmenu_style = \"tabs\"\nribbon_collapsed = \"yes\"\ntheme = \"light\"\n",
        );
        assert_eq!(bad.prefs.general.menu_style, MenuStylePref::Dropdown);
        assert!(!bad.prefs.general.ribbon_collapsed);
        assert_eq!(bad.prefs.general.theme, ThemePref::Light);
        assert_eq!(
            bad.issues,
            vec![
                PrefsIssue::InvalidValue {
                    key: "general.menu_style".to_owned()
                },
                PrefsIssue::InvalidValue {
                    key: "general.ribbon_collapsed".to_owned()
                }
            ]
        );
    }

    #[test]
    fn a_missing_accent_is_the_default_and_a_bad_one_warns() {
        let old = load_preferences("[general]\ntheme = \"dark\"\n");
        assert_eq!(old.prefs.general.accent, AccentPref::Mychess);
        assert!(old.issues.is_empty());
        let bad = load_preferences("[general]\naccent = \"magenta\"\ntheme = \"light\"\n");
        assert_eq!(bad.prefs.general.accent, AccentPref::Mychess);
        assert_eq!(bad.prefs.general.theme, ThemePref::Light);
        assert_eq!(
            bad.issues,
            vec![PrefsIssue::InvalidValue {
                key: "general.accent".to_owned()
            }]
        );
    }

    #[test]
    fn a_bad_value_only_resets_its_own_field() {
        let loaded =
            load_preferences("[board]\nanimation_ms = \"fast\"\nhighlight_last_move = false\n");
        assert_eq!(loaded.prefs.board.animation_ms, 200);
        assert!(!loaded.prefs.board.highlight_last_move);
        assert_eq!(
            loaded.issues,
            vec![PrefsIssue::InvalidValue {
                key: "board.animation_ms".to_owned()
            }]
        );
    }

    #[test]
    fn numbers_are_clamped() {
        let loaded = load_preferences(
            "[playback]\nautoplay_delay_ms = 5\n[window]\nwidth = 10\nheight = 5000\n",
        );
        assert_eq!(loaded.prefs.playback.autoplay_delay_ms, 100);
        assert_eq!(loaded.prefs.window.width, Some(320));
        assert_eq!(loaded.prefs.window.height, Some(5000));
        let keys: Vec<_> = loaded
            .issues
            .iter()
            .filter_map(|i| match i {
                PrefsIssue::Clamped { key } => Some(key.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(keys, ["playback.autoplay_delay_ms", "window.width"]);
    }

    #[test]
    fn versions_are_handled() {
        let old = load_preferences("[general]\ntheme = \"dark\"\n");
        assert_eq!(old.prefs.version, PREFERENCES_VERSION);
        assert_eq!(old.prefs.general.theme, ThemePref::Dark);
        let newer = load_preferences("version = 99\n[general]\ntheme = \"light\"\n");
        assert_eq!(newer.prefs.general.theme, ThemePref::Light);
        assert_eq!(newer.issues, vec![PrefsIssue::NewerVersion { found: 99 }]);
    }

    #[test]
    fn broken_toml_is_unreadable() {
        let loaded = load_preferences("this is = = not toml [");
        assert_eq!(loaded.prefs, Preferences::default());
        assert_eq!(loaded.issues, vec![PrefsIssue::Unreadable]);
    }

    #[test]
    fn wrong_section_types_and_bad_layouts_are_reported() {
        let loaded = load_preferences("board = 3\nshortcuts = []\nlayout = \"x\"\n");
        assert_eq!(loaded.prefs, Preferences::default());
        assert_eq!(loaded.issues.len(), 3);
    }

    #[test]
    fn old_default_layout_becomes_the_new_one_and_custom_is_kept() {
        let old = render_preferences(&Preferences {
            layout: Some(LayoutNode::legacy_default_layout()),
            ..Preferences::default()
        });
        let loaded = load_preferences(&old);
        assert_eq!(loaded.prefs.layout, Some(LayoutNode::default_layout()));
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
        let custom = LayoutNode::Split {
            axis: crate::prefs::SplitAxis::Horizontal,
            ratio: 0.7,
            a: Box::new(LayoutNode::pane(crate::prefs::PaneKind::Board)),
            b: Box::new(LayoutNode::pane(crate::prefs::PaneKind::Info)),
        };
        let text = render_preferences(&Preferences {
            layout: Some(custom.clone()),
            ..Preferences::default()
        });
        assert_eq!(load_preferences(&text).prefs.layout, Some(custom));
    }

    #[test]
    fn file_without_the_new_notation_keys_opens_silently_with_defaults() {
        let loaded = load_preferences("[notation]\nlayout = \"indented\"\nshow_result = false\n");
        assert_eq!(loaded.prefs.notation.layout, NotationLayout::Indented);
        assert!(loaded.prefs.notation.figurines);
        assert!(loaded.prefs.notation.show_time_column);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
        let bad = load_preferences("[notation]\nfigurines = 3\nlayout = \"table\"\n");
        assert_eq!(bad.prefs.notation.layout, NotationLayout::Table);
        assert_eq!(bad.issues.len(), 1);
    }

    #[test]
    fn recent_files_keep_their_order() {
        let loaded = load_preferences("[files]\nrecent = [\"a.pgn\", \"b.pgn\", 3]\n");
        let list: Vec<_> = loaded.prefs.files.recent.iter().collect();
        assert_eq!(list, [&PathBuf::from("a.pgn"), &PathBuf::from("b.pgn")]);
        assert_eq!(loaded.issues.len(), 1);
    }
}
