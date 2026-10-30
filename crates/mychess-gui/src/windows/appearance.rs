//! The appearance window: its model (spec M9-3, D1700 to D1706).
//!
//! **Ver > Tema...** opens one modal window with four tabs (Theme, Board,
//! Pieces, Players). The choices live in an [`AppearanceDraft`]; nothing
//! changes in the application until **Apply**, which sends only what
//! differs, with the messages the submenus of the View menu used to send
//! (no new preference, D1704). **Cancel** and `Esc` drop the draft.
//!
//! This module is the pure part: the draft, the diff, the messages and the
//! visual state of the previews (the real board widget needs its caches
//! brought up to date outside `view`). The window itself is in
//! [`appearance_view`](super::appearance_view) and the previews in
//! [`appearance_preview`](super::appearance_preview).

use mychess_app::AppState;
use mychess_core::Position;

use crate::app_api::{self, MaterialMode, PlayersPrefs, ViewOptions};
use crate::shell::{Message, UiMessage};
use crate::theme::board_theme::BoardThemeId;
use crate::theme::{AccentId, ThemeMode};
use crate::widgets::board::geometry::{CoordsMode, Orientation};
use crate::widgets::board::marks::MarkBrush;
use crate::widgets::board::pieces::{PieceSet, PieceSetId};
use crate::widgets::board::{BoardCaches, BoardData, BoardOptions, BoardWidget};
use crate::widgets::icon::Icon;

/// A tab of the window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum AppearanceTab {
    /// Mode (system, light, dark) and accent color.
    #[default]
    Theme,
    /// Board theme and coordinates.
    Board,
    /// Piece set and captured material.
    Pieces,
    /// The player strips.
    Players,
}

impl AppearanceTab {
    /// The tabs, top to bottom.
    pub const ALL: [AppearanceTab; 4] = [
        AppearanceTab::Theme,
        AppearanceTab::Board,
        AppearanceTab::Pieces,
        AppearanceTab::Players,
    ];

    /// The Fluent key of the name.
    pub fn name_key(self) -> &'static str {
        match self {
            AppearanceTab::Theme => "appearance-tab-theme",
            AppearanceTab::Board => "appearance-tab-board",
            AppearanceTab::Pieces => "appearance-tab-pieces",
            AppearanceTab::Players => "appearance-tab-players",
        }
    }

    /// The id of the button of the tab (the handle for tests).
    pub fn button_id(self) -> &'static str {
        match self {
            AppearanceTab::Theme => "appearance-tab-theme",
            AppearanceTab::Board => "appearance-tab-board",
            AppearanceTab::Pieces => "appearance-tab-pieces",
            AppearanceTab::Players => "appearance-tab-players",
        }
    }

    /// The icon of the tab.
    pub fn icon(self) -> Icon {
        match self {
            AppearanceTab::Theme => Icon::Palette,
            AppearanceTab::Board => Icon::BoardGrid,
            AppearanceTab::Pieces => Icon::Pieces,
            AppearanceTab::Players => Icon::Users,
        }
    }

    /// The tab `delta` places below this one (clamped at both ends).
    #[must_use]
    pub fn step(self, delta: isize) -> AppearanceTab {
        let at = AppearanceTab::ALL
            .iter()
            .position(|tab| *tab == self)
            .unwrap_or(0);
        let last = AppearanceTab::ALL.len() - 1;
        let next = at.saturating_add_signed(delta).min(last);
        AppearanceTab::ALL.get(next).copied().unwrap_or(self)
    }
}

/// One of the five switches of the Players tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerSwitch {
    /// Show the strips at all.
    Strips,
    /// Prefer the compact strip.
    Compact,
    /// Draw the avatars.
    Avatars,
    /// Draw the ratings.
    Ratings,
    /// Draw the clocks.
    Clocks,
}

impl PlayerSwitch {
    /// The switches, in the order of the old menu.
    pub const ALL: [PlayerSwitch; 5] = [
        PlayerSwitch::Strips,
        PlayerSwitch::Compact,
        PlayerSwitch::Avatars,
        PlayerSwitch::Ratings,
        PlayerSwitch::Clocks,
    ];

    /// The Fluent key of the label (the keys of the old View menu).
    pub fn label_key(self) -> &'static str {
        match self {
            PlayerSwitch::Strips => "menu-view-player-strips-show",
            PlayerSwitch::Compact => "menu-view-player-strips-compact",
            PlayerSwitch::Avatars => "menu-view-player-avatars",
            PlayerSwitch::Ratings => "menu-view-player-ratings",
            PlayerSwitch::Clocks => "menu-view-player-clocks",
        }
    }

    /// Whether the switch is on in `prefs`.
    pub fn is_on(self, prefs: PlayersPrefs) -> bool {
        match self {
            PlayerSwitch::Strips => prefs.show_strips,
            PlayerSwitch::Compact => prefs.compact_strips,
            PlayerSwitch::Avatars => prefs.show_avatars,
            PlayerSwitch::Ratings => prefs.show_ratings,
            PlayerSwitch::Clocks => prefs.show_clocks,
        }
    }

    fn set(self, prefs: &mut PlayersPrefs, on: bool) {
        match self {
            PlayerSwitch::Strips => prefs.show_strips = on,
            PlayerSwitch::Compact => prefs.compact_strips = on,
            PlayerSwitch::Avatars => prefs.show_avatars = on,
            PlayerSwitch::Ratings => prefs.show_ratings = on,
            PlayerSwitch::Clocks => prefs.show_clocks = on,
        }
    }
}

/// Events of the window.
#[derive(Debug, Clone)]
pub enum AppearanceMessage {
    /// Opens the window with the current preferences as the draft.
    Open,
    /// Closes it and drops the draft (Cancel, `Esc`).
    Close,
    /// Sends what changed and closes (Apply, `Enter`).
    Apply,
    /// Shows a tab.
    Tab(AppearanceTab),
    /// Draft: the interface mode.
    SetMode(ThemeMode),
    /// Draft: the accent color.
    SetAccent(AccentId),
    /// Draft: the board theme.
    SetBoardTheme(BoardThemeId),
    /// Draft: the coordinates.
    SetCoords(CoordsMode),
    /// Draft: the piece set.
    SetPieceSet(PieceSetId),
    /// Draft: what the strips show about the material.
    SetMaterial(MaterialMode),
    /// Draft: one switch of the player strips.
    SetSwitch(PlayerSwitch, bool),
    /// Events of the previews that do nothing (the board is not
    /// interactive).
    Noop,
}

/// Every choice of the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppearanceDraft {
    /// The interface mode.
    pub mode: ThemeMode,
    /// The accent color.
    pub accent: AccentId,
    /// The board theme.
    pub board_theme: BoardThemeId,
    /// The coordinates.
    pub coords: CoordsMode,
    /// The piece set.
    pub piece_set: PieceSetId,
    /// The player strips (and their material).
    pub players: PlayersPrefs,
}

impl AppearanceDraft {
    /// What the application shows now.
    pub(crate) fn current(app: &AppState, options: ViewOptions) -> AppearanceDraft {
        let look = app_api::appearance(app, options);
        AppearanceDraft {
            mode: app_api::theme_mode(app),
            accent: app_api::accent(app),
            board_theme: look.board.theme,
            coords: look.board.coords,
            piece_set: look.piece_set,
            players: look.players,
        }
    }

    /// Whether the choices of `tab` differ from `other`'s (the dot on the
    /// tab, spec 2.2).
    pub fn differs_in(&self, other: &AppearanceDraft, tab: AppearanceTab) -> bool {
        let same_players = |a: &PlayersPrefs, b: &PlayersPrefs| {
            PlayerSwitch::ALL.iter().all(|s| s.is_on(*a) == s.is_on(*b))
        };
        match tab {
            AppearanceTab::Theme => self.mode != other.mode || self.accent != other.accent,
            AppearanceTab::Board => {
                self.board_theme != other.board_theme || self.coords != other.coords
            }
            AppearanceTab::Pieces => {
                self.piece_set != other.piece_set || self.players.material != other.players.material
            }
            AppearanceTab::Players => !same_players(&self.players, &other.players),
        }
    }

    /// Whether anything differs from `other`.
    pub fn differs(&self, other: &AppearanceDraft) -> bool {
        AppearanceTab::ALL
            .iter()
            .any(|tab| self.differs_in(other, *tab))
    }

    /// The messages that apply this draft: only the choices the user
    /// changed (`!= baseline`, what the window showed when it opened) that
    /// also differ from what the application shows now (`current`), so a
    /// preference changed elsewhere while the window was open is not
    /// reverted (three-way diff, D1713). In the order of the old menu
    /// (D1704).
    pub(crate) fn changes_from(
        &self,
        baseline: &AppearanceDraft,
        current: &AppearanceDraft,
    ) -> Vec<Message> {
        fn touched<T: PartialEq>(mine: &T, base: &T, now: &T) -> bool {
            mine != base && mine != now
        }
        let mut out = Vec::new();
        if touched(&self.mode, &baseline.mode, &current.mode) {
            out.push(Message::App(app_api::set_theme(self.mode)));
        }
        if touched(&self.accent, &baseline.accent, &current.accent) {
            out.push(Message::App(app_api::set_accent(self.accent)));
        }
        if touched(
            &self.board_theme,
            &baseline.board_theme,
            &current.board_theme,
        ) {
            out.push(Message::App(app_api::set_board_theme(self.board_theme)));
        }
        if touched(&self.piece_set, &baseline.piece_set, &current.piece_set) {
            out.push(Message::App(app_api::set_piece_set(self.piece_set)));
        }
        if touched(&self.coords, &baseline.coords, &current.coords) {
            out.push(Message::Ui(UiMessage::Coords(self.coords)));
        }
        // The players travel in one message, merged field by field over
        // what the application shows now.
        let mut players = current.players;
        if self.players.material != baseline.players.material {
            players.material = self.players.material;
        }
        for which in PlayerSwitch::ALL {
            if which.is_on(self.players) != which.is_on(baseline.players) {
                which.set(&mut players, which.is_on(self.players));
            }
        }
        if players != current.players {
            out.push(Message::App(app_api::set_players(players)));
        }
        out
    }

    /// Whether the interface is dark with this draft, given what the
    /// system reports.
    pub fn is_dark(&self, system: iced::theme::Mode) -> bool {
        match self.mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => matches!(system, iced::theme::Mode::Dark),
        }
    }
}

/// The visual state of the open window.
#[derive(Debug)]
pub struct AppearanceUi {
    /// The tab on show.
    pub tab: AppearanceTab,
    /// The choices made so far.
    pub draft: AppearanceDraft,
    /// What the application showed when the window opened (for the dots of
    /// the tabs).
    pub baseline: AppearanceDraft,
    position: Position,
    brush: MarkBrush,
    /// Every set, loaded once when the window opens (the cards).
    sets: Vec<PieceSet>,
    board_caches: BoardCaches,
    piece_caches: BoardCaches,
}

impl AppearanceUi {
    /// The window for `baseline`, the draft starting equal to it.
    pub fn open(baseline: AppearanceDraft) -> AppearanceUi {
        let mut ui = AppearanceUi {
            tab: AppearanceTab::default(),
            draft: baseline,
            baseline,
            position: Position::new(),
            brush: MarkBrush::default(),
            sets: PieceSetId::ALL.map(PieceSet::load).into(),
            board_caches: BoardCaches::default(),
            piece_caches: BoardCaches::default(),
        };
        ui.sync();
        ui
    }

    /// Applies an event that only changes the draft or the tab.
    #[allow(clippy::needless_pass_by_value)] // the other messages are small
    pub fn update(&mut self, message: AppearanceMessage) {
        // The caches follow the draft only when it changed.
        let changes_draft = !matches!(
            message,
            AppearanceMessage::Tab(_)
                | AppearanceMessage::Open
                | AppearanceMessage::Close
                | AppearanceMessage::Apply
                | AppearanceMessage::Noop
        );
        match message {
            AppearanceMessage::Tab(tab) => self.tab = tab,
            AppearanceMessage::SetMode(mode) => self.draft.mode = mode,
            AppearanceMessage::SetAccent(accent) => self.draft.accent = accent,
            AppearanceMessage::SetBoardTheme(id) => self.draft.board_theme = id,
            AppearanceMessage::SetCoords(coords) => self.draft.coords = coords,
            AppearanceMessage::SetPieceSet(id) => self.draft.piece_set = id,
            AppearanceMessage::SetMaterial(mode) => self.draft.players.material = mode,
            AppearanceMessage::SetSwitch(which, on) => which.set(&mut self.draft.players, on),
            AppearanceMessage::Open
            | AppearanceMessage::Close
            | AppearanceMessage::Apply
            | AppearanceMessage::Noop => {}
        }
        if changes_draft {
            self.sync();
        }
    }

    /// The loaded set `id` (the default one if the id is somehow absent).
    pub fn set(&self, id: PieceSetId) -> &PieceSet {
        let at = PieceSetId::ALL.iter().position(|s| *s == id).unwrap_or(0);
        // `sets` has one entry per id, built in `open`.
        match self.sets.get(at).or_else(|| self.sets.first()) {
            Some(set) => set,
            None => fallback_set(),
        }
    }

    /// The board options of the Board tab preview: the draft theme and
    /// coordinates.
    fn board_options(&self) -> BoardOptions {
        BoardOptions {
            theme: self.draft.board_theme,
            coords: self.draft.coords,
            highlight_last_move: false,
            show_legal_hints: false,
            show_marks: false,
        }
    }

    /// The board options of the Pieces tab preview: the board theme in use,
    /// no coordinates.
    fn pieces_options(&self) -> BoardOptions {
        BoardOptions {
            theme: self.baseline.board_theme,
            coords: CoordsMode::None,
            highlight_last_move: false,
            show_legal_hints: false,
            show_marks: false,
        }
    }

    fn data<'a>(&'a self, options: BoardOptions, pieces: &'a PieceSet) -> BoardData<'a> {
        BoardData {
            position: &self.position,
            last_move: None,
            marks: &[],
            orientation: Orientation::White,
            options,
            accepts_moves: false,
            brush: &self.brush,
            pieces,
            animation: None,
        }
    }

    /// Brings the caches of the two boards up to date with the draft.
    fn sync(&mut self) {
        let board_pieces = self.set(self.baseline.piece_set).clone();
        let draft_pieces = self.set(self.draft.piece_set).clone();
        let board = BoardData {
            position: &self.position,
            last_move: None,
            marks: &[],
            orientation: Orientation::White,
            options: BoardOptions {
                theme: self.draft.board_theme,
                coords: self.draft.coords,
                highlight_last_move: false,
                show_legal_hints: false,
                show_marks: false,
            },
            accepts_moves: false,
            brush: &self.brush,
            pieces: &board_pieces,
            animation: None,
        };
        self.board_caches.sync_data(&board);
        let pieces = BoardData {
            options: BoardOptions {
                theme: self.baseline.board_theme,
                coords: CoordsMode::None,
                ..board.options
            },
            pieces: &draft_pieces,
            ..board
        };
        self.piece_caches.sync_data(&pieces);
    }

    /// The real board of the Board tab: the draft theme and coordinates
    /// with the piece set in use (D1703). `pieces` must be
    /// [`board_pieces`](Self::board_pieces).
    pub fn board_widget<'a>(&'a self, pieces: &'a PieceSet) -> BoardWidget<'a> {
        BoardWidget::new(self.data(self.board_options(), pieces), &self.board_caches)
    }

    /// The set the Board tab preview draws (the one in use).
    pub fn board_pieces(&self) -> &PieceSet {
        self.set(self.baseline.piece_set)
    }

    /// The real board of the Pieces tab: the draft set over the board theme
    /// in use. `pieces` must be [`draft_pieces`](Self::draft_pieces).
    pub fn pieces_widget<'a>(&'a self, pieces: &'a PieceSet) -> BoardWidget<'a> {
        BoardWidget::new(self.data(self.pieces_options(), pieces), &self.piece_caches)
    }

    /// The set of the draft.
    pub fn draft_pieces(&self) -> &PieceSet {
        self.set(self.draft.piece_set)
    }
}

/// `sets` is never empty (one entry per `PieceSetId::ALL`); this keeps the
/// accessor total without a panic path in the normal flow.
fn fallback_set() -> &'static PieceSet {
    use std::sync::LazyLock;
    static FALLBACK: LazyLock<PieceSet> = LazyLock::new(PieceSet::default);
    &FALLBACK
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> AppearanceDraft {
        AppearanceDraft::current(&AppState::default(), ViewOptions::default())
    }

    #[test]
    fn the_draft_starts_equal_to_the_current_preferences() {
        let mut app = AppState::default();
        app.prefs.general.accent = mychess_app::AccentPref::Purple;
        app.prefs.general.theme = mychess_app::ThemePref::Dark;
        app.prefs.board.piece_set = "merida".to_owned();
        let now = AppearanceDraft::current(&app, ViewOptions::default());
        assert_eq!(now.accent, AccentId::Purple);
        assert_eq!(now.mode, ThemeMode::Dark);
        assert_eq!(now.piece_set, PieceSetId::Merida);
        let ui = AppearanceUi::open(now);
        assert_eq!(ui.draft, now);
        assert!(!ui.draft.differs(&ui.baseline));
        assert_eq!(ui.tab, AppearanceTab::Theme);
    }

    #[test]
    fn nothing_is_sent_when_nothing_changed() {
        let now = base();
        assert!(now.changes_from(&now, &now).is_empty());
    }

    #[test]
    fn apply_sends_only_what_differs_in_the_order_of_the_old_menu() {
        let now = base();
        let mut draft = now;
        draft.accent = AccentId::Blue;
        draft.coords = CoordsMode::Outside;
        let sent = draft.changes_from(&now, &now);
        assert_eq!(sent.len(), 2, "{sent:?}");
        assert!(matches!(
            &sent[0],
            Message::App(mychess_app::Message::Prefs(mychess_app::PrefsMessage::Set(
                mychess_app::PrefChange::Accent(mychess_app::AccentPref::Blue)
            )))
        ));
        assert!(matches!(
            &sent[1],
            Message::Ui(UiMessage::Coords(CoordsMode::Outside))
        ));
    }

    #[test]
    fn every_kind_of_change_has_its_own_message() {
        let now = base();
        let mut draft = now;
        draft.mode = if now.mode == ThemeMode::Dark {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };
        draft.board_theme = BoardThemeId::ScidBrown;
        draft.piece_set = PieceSetId::Celtic;
        draft.players.show_clocks = !now.players.show_clocks;
        draft.players.material = MaterialMode::Off;
        // The players (switches and material) travel in one message.
        assert_eq!(draft.changes_from(&now, &now).len(), 4);
    }

    #[test]
    fn choices_change_the_draft_but_not_the_baseline() {
        let mut ui = AppearanceUi::open(base());
        let before = ui.baseline;
        ui.update(AppearanceMessage::SetMode(ThemeMode::Dark));
        ui.update(AppearanceMessage::SetAccent(AccentId::Red));
        ui.update(AppearanceMessage::SetBoardTheme(BoardThemeId::ScidTan));
        ui.update(AppearanceMessage::SetCoords(CoordsMode::None));
        ui.update(AppearanceMessage::SetPieceSet(PieceSetId::Fantasy));
        ui.update(AppearanceMessage::SetMaterial(MaterialMode::Difference));
        ui.update(AppearanceMessage::SetSwitch(PlayerSwitch::Ratings, false));
        assert_eq!(ui.baseline, before);
        assert_eq!(ui.draft.mode, ThemeMode::Dark);
        assert_eq!(ui.draft.accent, AccentId::Red);
        assert_eq!(ui.draft.board_theme, BoardThemeId::ScidTan);
        assert_eq!(ui.draft.coords, CoordsMode::None);
        assert_eq!(ui.draft.piece_set, PieceSetId::Fantasy);
        assert_eq!(ui.draft.players.material, MaterialMode::Difference);
        assert!(!ui.draft.players.show_ratings);
        assert_eq!(ui.draft_pieces().id(), PieceSetId::Fantasy);
        assert_eq!(ui.board_pieces().id(), before.piece_set);
    }

    #[test]
    fn the_dot_of_a_tab_follows_the_differences_of_that_tab() {
        let mut ui = AppearanceUi::open(base());
        let dots = |ui: &AppearanceUi| {
            AppearanceTab::ALL.map(|tab| ui.draft.differs_in(&ui.baseline, tab))
        };
        assert_eq!(dots(&ui), [false; 4]);
        ui.update(AppearanceMessage::SetAccent(AccentId::Pink));
        assert_eq!(dots(&ui), [true, false, false, false]);
        ui.update(AppearanceMessage::SetCoords(CoordsMode::Outside));
        assert_eq!(dots(&ui), [true, true, false, false]);
        ui.update(AppearanceMessage::SetMaterial(MaterialMode::Off));
        assert_eq!(dots(&ui), [true, true, true, false]);
        ui.update(AppearanceMessage::SetSwitch(PlayerSwitch::Avatars, false));
        assert_eq!(dots(&ui), [true, true, true, true]);
        // Going back to the original value clears the dot.
        ui.update(AppearanceMessage::SetAccent(ui.baseline.accent));
        assert!(!dots(&ui)[0]);
    }

    #[test]
    fn tabs_step_and_stop_at_the_ends() {
        assert_eq!(AppearanceTab::Theme.step(1), AppearanceTab::Board);
        assert_eq!(AppearanceTab::Players.step(1), AppearanceTab::Players);
        assert_eq!(AppearanceTab::Theme.step(-1), AppearanceTab::Theme);
        assert_eq!(AppearanceTab::Pieces.step(-1), AppearanceTab::Board);
    }

    #[test]
    fn system_mode_follows_what_the_system_reports() {
        let mut draft = base();
        draft.mode = ThemeMode::System;
        assert!(draft.is_dark(iced::theme::Mode::Dark));
        assert!(!draft.is_dark(iced::theme::Mode::Light));
        assert!(!draft.is_dark(iced::theme::Mode::None));
        draft.mode = ThemeMode::Dark;
        assert!(draft.is_dark(iced::theme::Mode::Light));
        draft.mode = ThemeMode::Light;
        assert!(!draft.is_dark(iced::theme::Mode::Dark));
    }

    #[test]
    fn all_the_piece_sets_are_loaded_for_the_cards() {
        let ui = AppearanceUi::open(base());
        for id in PieceSetId::ALL {
            assert_eq!(ui.set(id).id(), id);
        }
    }
}
