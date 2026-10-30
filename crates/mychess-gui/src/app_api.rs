//! The only module of the GUI that touches `mychess_app` (anti-corruption
//! layer, spec D133).
//!
//! It exposes what the front-end reads (view-models) and builds the messages
//! it sends. When the application layer renames something, only this file
//! changes.
//!
//! Preferences, layout and shortcuts come from `mychess_app::Preferences`;
//! view-only options that the application layer does not store yet live in
//! [`ViewOptions`].

use std::io::Read;
use std::path::{Path, PathBuf};

#[cfg(test)]
use mychess_app::DiscardPrompt;
use mychess_app::engines::EngineMessage;
use mychess_app::{
    AccentPref, AnalysisMessage, AnnotateMessage, AppState, BasesMessage, CoordinatesPref,
    DiscardChoice, GameMessage, LanguagePref as AppLanguage, LoadedPreferences, MenuStylePref,
    Message as AppMessage, ModeMessage, Nav, Notice, OverwritePrompt, PlayOptions, PrefChange,
    PrefsIssue, PrefsMessage, ThemePref, WindowGeometry,
};
pub(crate) use mychess_app::{
    MaterialMode, PlayerStripView, PlayerStrips, PlayersPrefs, RatingView,
};
use mychess_core::{
    BoardMark, EmbeddedCommand, Game, MarkKind, Move, NodeId, Position, embedded_commands,
    toggle_mark,
};

use crate::i18n::LanguagePref;
use crate::theme::board_theme::BoardThemeId;
use crate::theme::{AccentId, ThemeMode};
use crate::widgets::board::BoardOptions;
use crate::widgets::board::geometry::CoordsMode;
use crate::widgets::board::pieces::PieceSetId;
use crate::windows::layout::LayoutNode;

/// The preferences are tiny; a larger file is not read at boot (the
/// asynchronous load after the window opens reports it).
const PEEK_LIMIT_BYTES: u64 = 1 << 20;

/// Visual options that the application's preferences do not hold (yet).
///
/// They last for the session and are *not persisted*: legal-move hints, marks, the status bar and the "both sides" variant of
/// the coordinates (the app has `none`, `outside` and `inside` only).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewOptions {
    /// Whether legal destinations get dots and rings.
    pub show_legal_hints: bool,
    /// Whether the marks of the comment are drawn.
    pub show_marks: bool,
    /// Whether the status bar is shown.
    pub show_status_bar: bool,
    /// Whether the outside coordinates are drawn on both sides.
    pub coords_both_sides: bool,
}

impl Default for ViewOptions {
    fn default() -> ViewOptions {
        ViewOptions {
            show_legal_hints: true,
            show_marks: true,
            show_status_bar: true,
            coords_both_sides: false,
        }
    }
}

/// A change of a session-only visual option (Visualizar menu).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewChange {
    /// Legal move hints.
    LegalHints(bool),
    /// Colored squares and arrows.
    ShowMarks(bool),
    /// Status bar.
    StatusBar(bool),
}

impl ViewOptions {
    /// Applies one change.
    pub fn apply(&mut self, change: ViewChange) {
        match change {
            ViewChange::LegalHints(v) => self.show_legal_hints = v,
            ViewChange::ShowMarks(v) => self.show_marks = v,
            ViewChange::StatusBar(v) => self.show_status_bar = v,
        }
    }
}

/// What the board and the animation need from the preferences.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Appearance {
    /// Board display options.
    pub board: BoardOptions,
    /// Whether the player strips mark the side to move (`[board]` key).
    pub show_side_to_move: bool,
    /// The player strips preferences (`[players]`).
    pub players: PlayersPrefs,
    /// Move animation time in milliseconds (`0` turns it off).
    pub animation_ms: u32,
    /// The piece set; an id the GUI does not know shows the default set.
    pub piece_set: PieceSetId,
}

/// The board options for `app`'s preferences and the session options.
///
/// The board theme id of the preferences is the kebab name of
/// [`BoardThemeId`]; an id the GUI does not know (the app's default is
/// `classic`) shows the default theme and stays in the file untouched (D146).
pub(crate) fn appearance(app: &AppState, options: ViewOptions) -> Appearance {
    let board = &app.prefs.board;
    let coords = match board.coordinates {
        CoordinatesPref::None => CoordsMode::None,
        CoordinatesPref::Inside => CoordsMode::Inside,
        _ if options.coords_both_sides => CoordsMode::OutsideBothSides,
        _ => CoordsMode::Outside,
    };
    Appearance {
        board: BoardOptions {
            theme: BoardThemeId::from_kebab_name(&board.theme).unwrap_or_default(),
            coords,
            highlight_last_move: board.highlight_last_move,
            show_legal_hints: options.show_legal_hints,
            show_marks: options.show_marks,
        },
        show_side_to_move: board.show_side_to_move,
        players: app.prefs.players,
        // A slide of the walk of a review lasts at most the interval of its
        // steps (M4-11, D1662).
        animation_ms: app
            .review_walk_slide_cap_ms()
            .map_or(board.animation_ms, |cap| board.animation_ms.min(cap)),
        piece_set: PieceSetId::from_kebab_name(&board.piece_set).unwrap_or_default(),
    }
}

/// The interface theme preference.
pub(crate) fn theme_mode(app: &AppState) -> ThemeMode {
    match app.prefs.general.theme {
        ThemePref::Light => ThemeMode::Light,
        ThemePref::Dark => ThemeMode::Dark,
        _ => ThemeMode::System,
    }
}

/// The accent color preference (spec M9-1).
pub(crate) fn accent(app: &AppState) -> AccentId {
    match app.prefs.general.accent {
        AccentPref::Blue => AccentId::Blue,
        AccentPref::Purple => AccentId::Purple,
        AccentPref::Pink => AccentId::Pink,
        AccentPref::Red => AccentId::Red,
        AccentPref::Orange => AccentId::Orange,
        AccentPref::Yellow => AccentId::Yellow,
        AccentPref::Green => AccentId::Green,
        AccentPref::Gray => AccentId::Gray,
        _ => AccentId::Mychess,
    }
}

/// The menu style preference (spec M9-2).
pub(crate) fn menu_style(app: &AppState) -> MenuStylePref {
    app.prefs.general.menu_style
}

/// Whether the ribbon shows only its tabs (spec M9-2).
pub(crate) fn ribbon_collapsed(app: &AppState) -> bool {
    app.prefs.general.ribbon_collapsed
}

/// The language preference.
pub(crate) fn language_pref(app: &AppState) -> LanguagePref {
    match app.prefs.general.language {
        AppLanguage::EnUs => LanguagePref::EnUs,
        AppLanguage::PtBr => LanguagePref::PtBr,
        _ => LanguagePref::System,
    }
}

/// The saved window size (logical pixels), if both sides are known.
pub(crate) fn saved_window_size(prefs: &mychess_app::Preferences) -> Option<(f32, f32)> {
    let window = &prefs.window;
    let (w, h) = (window.width?, window.height?);
    // Window sizes are small integers; `f32` holds them exactly.
    #[allow(clippy::cast_precision_loss)]
    Some((w as f32, h as f32))
}

/// Where the preferences file lives, `None` without a configuration
/// directory.
pub fn default_preferences_path() -> Option<PathBuf> {
    mychess_app::preferences_path()
}

/// Reads the preferences file *synchronously*, only to size the window and
/// to draw the first frame in the right theme and language (D134). It runs
/// once, before the window exists; the asynchronous `Startup` load that
/// follows is the authoritative one (backups of unreadable files, remarks).
/// Missing, unreadable or oversized files give `None`.
pub fn peek_preferences(path: &Path) -> Option<LoadedPreferences> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(PEEK_LIMIT_BYTES).read_to_end(&mut bytes).ok()?;
    if u64::try_from(bytes.len()).ok()? >= PEEK_LIMIT_BYTES {
        return None;
    }
    Some(mychess_app::load_preferences(&String::from_utf8_lossy(
        &bytes,
    )))
}

// ---- messages that change preferences ----

fn set(change: PrefChange) -> AppMessage {
    AppMessage::Prefs(PrefsMessage::Set(change))
}

pub(crate) fn set_theme(mode: ThemeMode) -> AppMessage {
    set(PrefChange::Theme(match mode {
        ThemeMode::System => ThemePref::System,
        ThemeMode::Light => ThemePref::Light,
        ThemeMode::Dark => ThemePref::Dark,
    }))
}

pub(crate) fn set_accent(accent: AccentId) -> AppMessage {
    set(PrefChange::Accent(match accent {
        AccentId::Mychess => AccentPref::Mychess,
        AccentId::Blue => AccentPref::Blue,
        AccentId::Purple => AccentPref::Purple,
        AccentId::Pink => AccentPref::Pink,
        AccentId::Red => AccentPref::Red,
        AccentId::Orange => AccentPref::Orange,
        AccentId::Yellow => AccentPref::Yellow,
        AccentId::Green => AccentPref::Green,
        AccentId::Gray => AccentPref::Gray,
    }))
}

pub(crate) fn set_menu_style(style: MenuStylePref) -> AppMessage {
    set(PrefChange::MenuStyle(style))
}

pub(crate) fn set_ribbon_collapsed(collapsed: bool) -> AppMessage {
    set(PrefChange::RibbonCollapsed(collapsed))
}

pub(crate) fn set_language(pref: LanguagePref) -> AppMessage {
    set(PrefChange::Language(match pref {
        LanguagePref::System => AppLanguage::System,
        LanguagePref::EnUs => AppLanguage::EnUs,
        LanguagePref::PtBr => AppLanguage::PtBr,
    }))
}

/// The preference change that selects the piece set.
pub(crate) fn set_piece_set(id: PieceSetId) -> AppMessage {
    set(PrefChange::PieceSet(id.kebab_name().to_owned()))
}

/// The preference change that selects the board theme.
pub(crate) fn set_board_theme(id: BoardThemeId) -> AppMessage {
    set(PrefChange::BoardTheme(id.kebab_name().to_owned()))
}

/// The coordinates preference for a GUI mode (`OutsideBothSides` is
/// `outside` plus a session option).
pub(crate) fn set_coordinates(mode: CoordsMode) -> AppMessage {
    set(PrefChange::Coordinates(match mode {
        CoordsMode::None => CoordinatesPref::None,
        CoordsMode::Inside => CoordinatesPref::Inside,
        CoordsMode::Outside | CoordsMode::OutsideBothSides => CoordinatesPref::Outside,
    }))
}

// ---- evaluation bar, engine arrows and traffic lights (spec M4-5) ----

pub(crate) use mychess_app::{
    ArrowMode, EngineArrow, EngineViewPrefs, EvalBarMessage, EvalBarView, SequenceSide,
    TrafficLight,
};
use mychess_app::{ArrowPrefs, EvalBarPrefs, TrafficLightPrefs};

/// The preferences of the bar, the arrows and the traffic lights.
pub(crate) fn engine_view_prefs(app: &AppState) -> EngineViewPrefs {
    *app.prefs.engine_view
}

/// What the evaluation bar shows.
pub(crate) fn eval_bar_view(app: &AppState) -> &EvalBarView {
    app.eval_bar_view()
}

/// The strips of the traffic lights, line 1 first.
pub(crate) fn traffic_lights(app: &AppState) -> &[TrafficLight] {
    app.traffic_lights()
}

/// The arrows of the engine for the position under the cursor.
///
/// During a guided review these are the arrows of the review (the live ones
/// are suppressed by the application, M4-11).
pub(crate) fn engine_arrows(app: &AppState) -> &[EngineArrow] {
    app.board_arrows()
}

/// The message of a click on the tip of an arrow; `None` when no engine
/// draws arrows now.
pub(crate) fn play_arrow(
    app: &AppState,
    from: mychess_core::Square,
    to: mychess_core::Square,
    rank: u8,
) -> Option<AppMessage> {
    let run = app.arrows_engine()?;
    Some(AppMessage::EvalBar(EvalBarMessage::PlayArrow {
        run,
        line: rank,
        from,
        to,
    }))
}

/// A command of the overlays of the engine.
pub(crate) fn eval_bar_command(message: EvalBarMessage) -> AppMessage {
    AppMessage::EvalBar(message)
}

/// Replaces the evaluation bar preferences.
pub(crate) fn set_eval_bar(prefs: EvalBarPrefs) -> AppMessage {
    set(PrefChange::EvalBar(prefs))
}

/// Replaces the arrow preferences.
pub(crate) fn set_arrows(prefs: ArrowPrefs) -> AppMessage {
    set(PrefChange::Arrows(prefs))
}

/// Replaces the `[report]` preferences (the arrow of the review, M4-11).
pub(crate) fn set_report_prefs(prefs: mychess_app::report::ReportPrefs) -> AppMessage {
    set(PrefChange::Report(prefs))
}

/// Replaces the traffic light preferences.
#[allow(dead_code)]
pub(crate) fn set_traffic_lights(prefs: TrafficLightPrefs) -> AppMessage {
    set(PrefChange::TrafficLights(prefs))
}

/// Whether the engine of `run` draws the arrows (the button is pressed).
pub(crate) fn arrows_engine_is(app: &AppState, run: mychess_app::engines::RunId) -> bool {
    app.arrows_engine() == Some(run)
}

/// The preference change that replaces the player strip preferences.
pub(crate) fn set_players(players: PlayersPrefs) -> AppMessage {
    set(PrefChange::Players(players))
}

/// The strips (name, rating, captured pieces, clock, side to move) of the
/// position under the cursor.
pub(crate) fn player_strips(app: &AppState) -> &PlayerStrips {
    app.player_strips()
}

/// The last configuration of the tournament, for `[tournament]` (spec
/// M8-2b, D1299).
pub(crate) fn set_tournament_prefs(
    config: mychess_app::tournament::TournamentConfig,
) -> AppMessage {
    set(PrefChange::Tournament(Box::new(config)))
}

/// The preferences of the game against an engine (spec M8-1b, D1242).
pub(crate) fn set_play_prefs(prefs: mychess_app::PlayPrefs) -> AppMessage {
    set(PrefChange::Play(Box::new(prefs)))
}

/// What the game against an engine shows, at `now` (spec M8-1).
pub(crate) fn play_view(app: &AppState, now: std::time::Instant) -> Option<mychess_app::PlayView> {
    app.play_view(now)
}

/// The phase of the game against an engine: cheap and without a time
/// (the views and the ticks use it; spec M8-1b, D1483).
pub(crate) fn play_phase(app: &AppState) -> Option<mychess_app::PlayPhase> {
    app.play_phase()
}

/// The clocks of the game at `now`: `(white, black, countdown)`.
pub(crate) fn play_clocks(
    app: &AppState,
    now: std::time::Instant,
) -> Option<(std::time::Duration, std::time::Duration, bool)> {
    app.play_clocks(now)
}

/// Whether the board may take a move from the user: not while the engine
/// plays or a question of the game is open (spec M8-1b, D1241). It reads
/// the phase, never the clock.
fn play_allows_moves(app: &AppState) -> bool {
    !app.play_live() || play_phase(app) == Some(mychess_app::PlayPhase::Player)
}

pub(crate) fn set_side_to_move(on: bool) -> AppMessage {
    set(PrefChange::ShowSideToMove(on))
}

pub(crate) fn set_highlight_last_move(on: bool) -> AppMessage {
    set(PrefChange::HighlightLastMove(on))
}

pub(crate) fn set_animation_ms(ms: u32) -> AppMessage {
    set(PrefChange::AnimationMs(ms))
}

/// The arrangement of the panels (saved with the next flush).
pub(crate) fn set_layout(layout: Option<LayoutNode>) -> AppMessage {
    set(PrefChange::Layout(layout))
}

pub(crate) fn window_geometry(width: u32, height: u32) -> AppMessage {
    AppMessage::WindowGeometryChanged(WindowGeometry {
        width,
        height,
        x: None,
        y: None,
        maximized: false,
    })
}

pub(crate) fn flush_preferences() -> AppMessage {
    AppMessage::FlushPreferences
}

pub(crate) fn quit() -> AppMessage {
    AppMessage::Quit
}

pub(crate) fn startup() -> AppMessage {
    AppMessage::Startup
}

pub(crate) fn preferences_loaded(loaded: LoadedPreferences) -> AppMessage {
    AppMessage::PreferencesLoaded(loaded)
}

pub(crate) fn command(command: mychess_app::Command) -> AppMessage {
    AppMessage::Command(command)
}

pub(crate) fn action(action: mychess_app::Action) -> AppMessage {
    AppMessage::Action(action)
}

pub(crate) fn answer_discard(choice: DiscardChoice) -> AppMessage {
    AppMessage::AnswerDiscard(choice)
}

pub(crate) fn answer_overwrite(yes: bool) -> AppMessage {
    AppMessage::AnswerOverwrite(yes)
}

/// The answer to "copy N games from X to Y?" (spec M3-4d).
pub(crate) fn answer_copy(yes: bool) -> AppMessage {
    AppMessage::Bases(BasesMessage::List(mychess_app::ListMessage::AnswerCopy(
        yes,
    )))
}

/// The "copy N games from X to Y?" question, if open.
pub(crate) fn copy_prompt(app: &AppState) -> Option<&mychess_app::CopyPrompt> {
    app.copy_prompt.as_ref()
}

pub(crate) fn state_with_preferences_path(path: Option<PathBuf>) -> AppState {
    let mut app = AppState::with_preferences_path(path);
    // A PGN opened from the menu, the recent list or a shortcut takes a
    // slot and shows its list (spec M3-4, D503, D517).
    app.open_pgn_in_slots = true;
    app
}

// ---- reading ----

/// What the board needs from the application.
pub(crate) struct BoardView<'a> {
    /// The position under the cursor.
    pub position: &'a Position,
    /// The move that led to it (`None` at the start and for the null move).
    pub last_move: Option<Move>,
    /// Whether the board accepts moves (no modal question is open).
    pub accepts_moves: bool,
}

pub(crate) fn board_view(app: &AppState) -> BoardView<'_> {
    let session = &app.session;
    let last_move = session
        .game()
        .mv(session.cursor())
        .ok()
        .filter(|m| !m.is_null());
    BoardView {
        position: session.position(),
        last_move,
        accepts_moves: !app.modal_open()
            && app.overwrite_prompt.is_none()
            && play_allows_moves(app),
    }
}

/// The marks written in the comment of the current node.
pub(crate) fn board_marks(app: &AppState) -> Vec<BoardMark> {
    let comment = current_comment(app.session.game(), app.session.cursor());
    let mut marks: Vec<BoardMark> = embedded_commands(&comment)
        .into_iter()
        .filter_map(|c| match c {
            EmbeddedCommand::Mark(mark) => Some(mark),
            _ => None,
        })
        .collect();
    marks.extend(pattern_marks(app));
    marks
}

/// The arrows and squares of the occurrence of the search for patterns at
/// the cursor (spec M6-2, D3284). They belong to the application: they are
/// not in the game. Reads the highlights without building the view of the
/// window (D3299 item 43).
fn pattern_marks(app: &AppState) -> Vec<BoardMark> {
    highlight_marks(
        app.pattern_search_highlights(),
        theme_mode(app) == ThemeMode::Dark,
    )
}

/// The board marks of some highlights of the search for patterns: an arrow
/// or a circle, colored by role (tokens in `theme::mark_colors`).
pub(crate) fn highlight_marks(
    highlights: &[mychess_app::pattern_search::model::Highlight],
    dark: bool,
) -> Vec<BoardMark> {
    use crate::theme::mark_colors::highlight_color;
    use mychess_app::pattern_search::model::HighlightKind;
    highlights
        .iter()
        .map(|h| {
            let color = highlight_color(h.role, dark);
            match h.kind {
                HighlightKind::Arrow(from, to) => BoardMark::arrow(from, to, color),
                HighlightKind::Square(sq) => BoardMark::square(MarkKind::Circle, sq, color),
            }
        })
        .collect()
}

fn current_comment(game: &Game, node: NodeId) -> String {
    game.comment(node)
        .ok()
        .flatten()
        .unwrap_or_default()
        .to_owned()
}

pub(crate) fn is_modified(app: &AppState) -> bool {
    app.session.is_dirty()
}

/// The file name of the game, when it comes from or went to a file.
pub(crate) fn file_name(app: &AppState) -> Option<String> {
    app.origin
        .path()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
}

pub(crate) fn notice(app: &AppState) -> Option<&Notice> {
    app.session.notice()
}

pub(crate) fn can_undo(app: &AppState) -> bool {
    app.session.can_undo()
}

pub(crate) fn can_redo(app: &AppState) -> bool {
    app.session.can_redo()
}

/// The open question of the editing session (replace the move, choose a
/// promotion, confirm a removal), if any.
pub(crate) fn session_prompt(app: &AppState) -> Option<&mychess_app::Prompt> {
    app.session.prompt()
}

/// The "save changes?" question, if open.
#[cfg(test)]
pub(crate) fn discard_prompt(app: &AppState) -> Option<&DiscardPrompt> {
    app.discard_prompt.as_ref()
}

/// The "overwrite a file with several games?" question, if open.
pub(crate) fn overwrite_prompt(app: &AppState) -> Option<&OverwritePrompt> {
    app.overwrite_prompt.as_ref()
}

/// Takes the application's last remark for the status bar (the front-end
/// decides how long it stays on screen).
pub(crate) fn take_app_notice(app: &mut AppState) -> Option<mychess_app::AppNotice> {
    app.notice.take()
}

/// The newest remark about the preferences worth showing. "No
/// configuration directory" is only logged: it is not actionable.
pub(crate) fn prefs_remark(app: &AppState) -> Option<&PrefsIssue> {
    app.prefs_issues
        .iter()
        .rev()
        .find(|issue| !matches!(issue, PrefsIssue::NoConfigDir))
}

/// The recently opened files, newest first.
pub(crate) fn recent_files(app: &AppState) -> Vec<PathBuf> {
    app.prefs.files.recent.iter().cloned().collect()
}

/// Whether the game belongs to a file with several games (next/previous).
pub(crate) fn has_collection(app: &AppState) -> bool {
    app.collection.is_some()
}

/// Whether `message` brings the preferences read from the file.
pub(crate) fn is_preferences_loaded(message: &AppMessage) -> bool {
    matches!(message, AppMessage::PreferencesLoaded(_))
}

/// Whether `message` brings a database that was opened as a new slot (not
/// a failure, not a file that was already open, not a full set of slots).
/// Whether the message is the answer that brings the games to the browser.
pub(crate) fn is_browser_loaded(message: &AppMessage) -> bool {
    matches!(
        message,
        AppMessage::Bases(BasesMessage::Browse(
            mychess_app::BrowseMessage::Loaded { .. }
        ))
    )
}

pub(crate) fn is_base_opened(message: &AppMessage) -> bool {
    matches!(
        message,
        AppMessage::Bases(BasesMessage::Opened {
            result: Ok(mychess_app::OpenOutcome::Opened(_)),
            ..
        })
    )
}

/// Whether `message` comes from the user and replaces older remarks.
pub(crate) fn is_user_action(message: &AppMessage) -> bool {
    matches!(
        message,
        AppMessage::Game(_)
            | AppMessage::Command(_)
            | AppMessage::Action(_)
            | AppMessage::AnswerDiscard(_)
            | AppMessage::AnswerOverwrite(_)
            | AppMessage::Mode(
                ModeMessage::ToggleThreat(_)
                    | ModeMessage::ToggleFinishGame(_)
                    | ModeMessage::ToggleAutomove(_),
            )
            | AppMessage::Epd(
                mychess_app::epd_window::EpdMessage::Open
                    | mychess_app::epd_window::EpdMessage::New
                    | mychess_app::epd_window::EpdMessage::OpenPath(_)
                    | mychess_app::epd_window::EpdMessage::Save
                    | mychess_app::epd_window::EpdMessage::SaveAs
                    | mychess_app::epd_window::EpdMessage::Commit
                    | mychess_app::epd_window::EpdMessage::AddPosition
                    | mychess_app::epd_window::EpdMessage::SortRecord
                    | mychess_app::epd_window::EpdMessage::SortAll
                    | mychess_app::epd_window::EpdMessage::StripOpcode(_)
                    | mychess_app::epd_window::EpdMessage::PasteAnalysis(_)
                    | mychess_app::epd_window::EpdMessage::Find(..)
                    | mychess_app::epd_window::EpdMessage::Annotate(_)
                    | mychess_app::epd_window::EpdMessage::UndoRun,
            )
            | AppMessage::Annotate(AnnotateMessage::Open | AnnotateMessage::Start)
            | AppMessage::Eco(mychess_app::EcoMessage::Identify)
            | AppMessage::Analysis(
                AnalysisMessage::AddMove(_)
                    | AnalysisMessage::AddScore(_)
                    | AnalysisMessage::AddLine(_)
                    | AnalysisMessage::AddLineNumber(..)
                    | AnalysisMessage::AddAllLines(..)
                    | AnalysisMessage::SetExclusion(..)
                    | AnalysisMessage::SetMultiPv(..),
            )
            | AppMessage::Engine(
                EngineMessage::Start(..)
                    | EngineMessage::Stop(_)
                    | EngineMessage::Restart(_)
                    | EngineMessage::ToggleHotkey(_)
                    | EngineMessage::ToggleMainAnalysis,
            )
            | AppMessage::Bases(
                BasesMessage::Open(_)
                    | BasesMessage::Select(_)
                    | BasesMessage::Next
                    | BasesMessage::Previous
                    | BasesMessage::Close(_)
                    | BasesMessage::LoadGame { .. }
                    | BasesMessage::ClipbaseCopy
                    | BasesMessage::ClipbasePaste
                    | BasesMessage::ClipbaseClear
            )
    )
}

/// The file an `Opened` answer with an error is about (still known to the
/// application until the answer is handled).
pub(crate) fn failed_open_path(app: &AppState, message: &AppMessage) -> Option<PathBuf> {
    match message {
        AppMessage::Bases(BasesMessage::Opened { result: Err(_), .. }) => {
            app.base_opening_path().map(Path::to_path_buf)
        }
        _ => None,
    }
}

/// A failure to open a database that does not name the file (empty file,
/// not a PGN) with the file added, so the status bar can name it (D553).
pub(crate) fn name_failed_file(
    notice: mychess_app::AppNotice,
    path: Option<PathBuf>,
) -> mychess_app::AppNotice {
    use mychess_app::{AppNotice, BaseError, BasesNotice};
    match (notice, path) {
        (AppNotice::Bases(BasesNotice::Failed(error)), Some(path))
            if !matches!(error, BaseError::File(_) | BaseError::Cancelled) =>
        {
            AppNotice::BaseFile { path, error }
        }
        (notice, _) => notice,
    }
}

/// The progress of the database being opened: the file name and the
/// fraction read (`None` until the total is known), if one is being opened.
pub(crate) fn base_opening(app: &AppState) -> Option<(String, Option<f32>)> {
    let control = app.base_opening()?;
    let name = app
        .base_opening_path()
        .and_then(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let (done, total) = control.progress();
    // Only a ratio to draw: the precision of an `f32` is plenty.
    #[allow(clippy::cast_precision_loss)]
    let fraction = (total > 0).then(|| (done as f32 / total as f32).clamp(0.0, 1.0));
    Some((name, fraction))
}

/// The message that stops the database being opened.
pub(crate) fn cancel_open() -> AppMessage {
    AppMessage::Bases(BasesMessage::CancelOpen)
}

// ---- messages the GUI sends ----

pub(crate) fn play_move(mv: Move) -> AppMessage {
    AppMessage::Game(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    })
}

pub(crate) fn play_null_move() -> AppMessage {
    AppMessage::Game(GameMessage::PlayNullMove)
}

pub(crate) fn answer_replace(choice: mychess_app::ReplaceChoice) -> AppMessage {
    AppMessage::Game(GameMessage::AnswerReplace(choice))
}

pub(crate) fn answer_promotion(role: Option<mychess_core::Role>) -> AppMessage {
    AppMessage::Game(GameMessage::AnswerPromotion(role))
}

pub(crate) fn answer_strip(yes: bool) -> AppMessage {
    AppMessage::Game(GameMessage::AnswerStrip(yes))
}

/// Whether removals ask first (the `confirm_destructive` preference).
pub(crate) fn confirm_destructive(app: &AppState) -> bool {
    app.prefs.editing.confirm_destructive
}

/// Asks to remove comments, variations or moves; the application counts
/// what goes and opens the confirmation (or acts at once when the user
/// turned the question off).
pub(crate) fn request_strip(app: &AppState, kind: mychess_app::StripKind) -> AppMessage {
    AppMessage::Game(GameMessage::RequestStrip {
        kind,
        confirm: confirm_destructive(app),
    })
}

/// Whether `message` is an answer that goes ahead (not a cancellation):
/// the only ones for which "don't ask again" is remembered.
pub(crate) fn is_confirming_answer(message: &AppMessage) -> bool {
    matches!(
        message,
        AppMessage::Game(
            GameMessage::AnswerReplace(
                mychess_app::ReplaceChoice::Replace
                    | mychess_app::ReplaceChoice::NewMainLine
                    | mychess_app::ReplaceChoice::AddVariation
                    | mychess_app::ReplaceChoice::Trial
            ) | GameMessage::AnswerStrip(true)
        )
    )
}

/// The message that remembers "don't ask again" for the question `prompt`
/// answered.
pub(crate) fn dont_ask_again(prompt: &mychess_app::Prompt) -> Option<AppMessage> {
    let change = match prompt {
        mychess_app::Prompt::ReplaceMove(_) => PrefChange::AskToReplaceMoves(false),
        mychess_app::Prompt::ConfirmStrip { .. } => PrefChange::ConfirmDestructive(false),
        mychess_app::Prompt::ChoosePromotion { .. } => return None,
    };
    Some(AppMessage::Prefs(PrefsMessage::Set(change)))
}

/// The message that toggles `mark` in the comment of the current node.
pub(crate) fn toggle_mark_message(app: &AppState, mark: &BoardMark) -> AppMessage {
    let node = app.session.cursor();
    let text = toggle_mark(&current_comment(app.session.game(), node), mark);
    AppMessage::Game(GameMessage::SetComment { node, text })
}

pub(crate) fn navigate(nav: Nav) -> AppMessage {
    AppMessage::Game(GameMessage::Navigate(nav))
}

pub(crate) fn undo() -> AppMessage {
    AppMessage::Game(GameMessage::Undo)
}

pub(crate) fn redo() -> AppMessage {
    AppMessage::Game(GameMessage::Redo)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)]
    use mychess_app::update;

    use super::*;

    fn sq(name: &str) -> mychess_core::Square {
        name.parse().expect("square")
    }

    #[test]
    fn board_view_of_a_new_game_is_the_start_position_without_last_move() {
        let app = AppState::default();
        let view = board_view(&app);
        assert_eq!(*view.position, Position::new());
        assert_eq!(view.last_move, None);
    }

    #[test]
    fn playing_a_move_updates_the_view_and_last_move() {
        let mut app = AppState::default();
        let mv = app
            .session
            .position()
            .find_move(sq("e2"), sq("e4"), None)
            .expect("legal");
        update(&mut app, play_move(mv));
        let view = board_view(&app);
        assert_eq!(view.last_move, Some(mv));
        assert!(is_modified(&app));
    }

    #[test]
    fn the_highlights_of_a_pattern_become_arrows_and_circles_by_role() {
        use crate::theme::mark_colors::highlight_color;
        use mychess_app::pattern_search::model::{Highlight, HighlightKind, HighlightRole};
        let highlights = [
            Highlight {
                kind: HighlightKind::Arrow(sq("c7"), sq("e8")),
                role: HighlightRole::Actor,
            },
            Highlight {
                kind: HighlightKind::Square(sq("e8")),
                role: HighlightRole::Target,
            },
            Highlight {
                kind: HighlightKind::Square(sq("a8")),
                role: HighlightRole::Behind,
            },
            Highlight {
                kind: HighlightKind::Arrow(sq("a1"), sq("a2")),
                role: HighlightRole::Info,
            },
        ];
        for dark in [false, true] {
            let color = |role| highlight_color(role, dark);
            assert_eq!(
                highlight_marks(&highlights, dark),
                vec![
                    BoardMark::arrow(sq("c7"), sq("e8"), color(HighlightRole::Actor)),
                    BoardMark::square(MarkKind::Circle, sq("e8"), color(HighlightRole::Target)),
                    BoardMark::square(MarkKind::Circle, sq("a8"), color(HighlightRole::Behind)),
                    BoardMark::arrow(sq("a1"), sq("a2"), color(HighlightRole::Info)),
                ]
            );
        }
        assert!(highlight_marks(&[], false).is_empty());
    }

    #[test]
    fn toggling_a_mark_twice_restores_the_comment_and_shows_the_mark() {
        let mut app = AppState::default();
        let mark = BoardMark::arrow(sq("d4"), sq("f6"), "green");
        let message = toggle_mark_message(&app, &mark);
        update(&mut app, message);
        assert_eq!(board_marks(&app), vec![mark.clone()]);
        let message = toggle_mark_message(&app, &mark);
        update(&mut app, message);
        assert!(board_marks(&app).is_empty());
        assert_eq!(
            app.session
                .game()
                .comment(app.session.cursor())
                .ok()
                .flatten(),
            None
        );
    }

    #[test]
    fn the_board_theme_ids_of_the_preferences_map_to_the_presets() {
        let mut app = AppState::default();
        // The application's default id is the Green preset (D219).
        assert_eq!(app.prefs.board.theme, "mychess-green");
        let look = appearance(&app, ViewOptions::default());
        assert_eq!(look.board.theme, BoardThemeId::default());
        for id in BoardThemeId::ALL {
            app.prefs.board.theme = id.kebab_name().to_owned();
            assert_eq!(appearance(&app, ViewOptions::default()).board.theme, id);
        }
        app.prefs.board.theme = "unknown".to_owned();
        assert_eq!(
            appearance(&app, ViewOptions::default()).board.theme,
            BoardThemeId::default()
        );
    }

    #[test]
    fn the_piece_set_ids_of_the_preferences_map_to_the_sets_and_unknown_ones_fall_back() {
        let mut app = AppState::default();
        assert_eq!(app.prefs.board.piece_set, "cburnett");
        let look = |app: &AppState| appearance(app, ViewOptions::default()).piece_set;
        assert_eq!(look(&app), PieceSetId::Cburnett);
        for id in PieceSetId::ALL {
            update(&mut app, set_piece_set(id));
            assert_eq!(app.prefs.board.piece_set, id.kebab_name());
            assert_eq!(look(&app), id);
        }
        // An id from a newer version: shown as the default, kept in the file.
        app.prefs.board.piece_set = "staunty".to_owned();
        assert_eq!(look(&app), PieceSetId::Cburnett);
        assert_eq!(app.prefs.board.piece_set, "staunty");
    }

    #[test]
    fn coordinates_map_both_ways_and_both_sides_only_refines_outside() {
        let mut app = AppState::default();
        let both = ViewOptions {
            coords_both_sides: true,
            ..ViewOptions::default()
        };
        for (pref, plain, with_both) in [
            (CoordinatesPref::None, CoordsMode::None, CoordsMode::None),
            (
                CoordinatesPref::Outside,
                CoordsMode::Outside,
                CoordsMode::OutsideBothSides,
            ),
            (
                CoordinatesPref::Inside,
                CoordsMode::Inside,
                CoordsMode::Inside,
            ),
        ] {
            app.prefs.board.coordinates = pref;
            assert_eq!(appearance(&app, ViewOptions::default()).board.coords, plain);
            assert_eq!(appearance(&app, both).board.coords, with_both);
        }
        for mode in [
            CoordsMode::None,
            CoordsMode::Outside,
            CoordsMode::OutsideBothSides,
            CoordsMode::Inside,
        ] {
            update(&mut app, set_coordinates(mode));
            let look = appearance(&app, ViewOptions::default());
            let expected = match mode {
                CoordsMode::OutsideBothSides => CoordsMode::Outside,
                other => other,
            };
            assert_eq!(look.board.coords, expected);
        }
    }

    #[test]
    fn language_and_theme_round_trip_through_the_preferences() {
        let mut app = AppState::default();
        for pref in [LanguagePref::System, LanguagePref::EnUs, LanguagePref::PtBr] {
            update(&mut app, set_language(pref));
            assert_eq!(language_pref(&app), pref);
        }
        for mode in [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark] {
            update(&mut app, set_theme(mode));
            assert_eq!(theme_mode(&app), mode);
        }
    }

    #[test]
    fn every_accent_round_trips_through_the_preferences() {
        let mut app = AppState::default();
        assert_eq!(accent(&app), AccentId::Mychess);
        for id in AccentId::ALL {
            update(&mut app, set_accent(id));
            assert_eq!(accent(&app), id);
        }
    }

    #[test]
    fn the_title_file_name_comes_from_the_origin() {
        let mut app = AppState::default();
        assert_eq!(file_name(&app), None);
        app.origin = mychess_app::GameOrigin::SavedFile {
            path: PathBuf::from("games").join("kasparov.pgn"),
        };
        assert_eq!(file_name(&app).as_deref(), Some("kasparov.pgn"));
    }

    #[test]
    fn the_saved_window_size_needs_both_sides() {
        let mut prefs = mychess_app::Preferences::default();
        assert_eq!(saved_window_size(&prefs), None);
        prefs.window.width = Some(1000);
        assert_eq!(saved_window_size(&prefs), None);
        prefs.window.height = Some(700);
        assert_eq!(saved_window_size(&prefs), Some((1000.0, 700.0)));
    }

    #[test]
    fn peeking_reads_a_valid_file_and_tolerates_the_rest() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("preferences.toml");
        assert!(peek_preferences(&path).is_none(), "missing file");
        std::fs::write(
            &path,
            "[general]\nlanguage = \"pt-BR\"\ntheme = \"dark\"\n[window]\nwidth = 900\nheight = 600\n",
        )
        .expect("write");
        let loaded = peek_preferences(&path).expect("loaded");
        assert_eq!(loaded.prefs.general.language, AppLanguage::PtBr);
        assert_eq!(loaded.prefs.general.theme, ThemePref::Dark);
        assert_eq!(saved_window_size(&loaded.prefs), Some((900.0, 600.0)));
        std::fs::write(&path, b"\xff\xfe not toml [[[").expect("write");
        let loaded = peek_preferences(&path).expect("defaults for garbage");
        assert_eq!(loaded.prefs, mychess_app::Preferences::default());
        assert!(!loaded.issues.is_empty());
    }

    #[test]
    fn peeking_skips_a_file_larger_than_the_limit() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("preferences.toml");
        let big = vec![b'#'; usize::try_from(PEEK_LIMIT_BYTES).expect("fits") + 1];
        std::fs::write(&path, big).expect("write");
        assert!(peek_preferences(&path).is_none());
    }

    #[test]
    fn remarks_about_a_missing_config_dir_are_not_shown() {
        let mut app = AppState::default();
        app.prefs_issues.push(PrefsIssue::NoConfigDir);
        assert_eq!(prefs_remark(&app), None);
        app.prefs_issues.push(PrefsIssue::Unreadable);
        assert_eq!(prefs_remark(&app), Some(&PrefsIssue::Unreadable));
    }
}
