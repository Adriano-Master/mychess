//! Visual state of the shell: orientation, brush, animation, caches, panes.
//!
//! Anything that must be saved or tested as a rule lives in the application
//! layer; this is only what the screen needs (spec rule 3).

use std::time::{Duration, Instant};

use iced::Size;
use iced::widget::pane_grid;
use mychess_core::{BoardMark, Position, ZobristHash};

use crate::app_api::{Appearance, BoardView};
use crate::widgets::board::animation::{
    MoveAnimation, animation_between, ease_out_cubic, progress,
};
use crate::widgets::board::geometry::Orientation;
use crate::widgets::board::marks::MarkBrush;
use crate::widgets::board::pieces::PieceSet;
use crate::widgets::board::{BoardCaches, BoardData, BoardWidget};
use crate::windows::layout::{self, Group, LayoutNode, PaneKind};

/// An animation in progress. `started` is set by the first frame, because
/// the clock is injected (never read here).
#[derive(Debug, Clone)]
pub struct ActiveAnimation {
    /// What slides.
    pub animation: MoveAnimation,
    started: Option<Instant>,
    eased: f32,
}

/// The context menu of a tab: which tab and where the pointer was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabMenu {
    /// The tab the menu is about.
    pub kind: PaneKind,
    /// The pointer, in window coordinates.
    pub position: iced::Point,
}

/// The visual state.
#[derive(Debug)]
pub struct UiState {
    /// Who is at the bottom of the board.
    pub orientation: Orientation,
    /// The brush of `Shift`/`Ctrl` click marks (session only).
    pub brush: MarkBrush,
    /// The panels: one pane per group of tabs (spec M2-6, D1009). Which
    /// panels are open is derived from it ([`UiState::is_open`]).
    pub panes: pane_grid::State<Group>,
    /// The context menu of a tab (spec M2-6, D1006), if open.
    pub tab_menu: Option<TabMenu>,
    /// The visual state of the graph panel.
    pub graph: crate::windows::graph::GraphUi,
    /// The visual state of the curve of the report panel (M4-10b).
    pub report_graph: crate::windows::graph::GraphUi,
    /// The visual state of the analysis panel.
    pub analysis: crate::windows::analysis::AnalysisUi,
    /// The game list was opened by the application (a database arrived),
    /// not by the user: the arrangement is saved without it (D552).
    pub games_auto: bool,
    /// The visual state of the game list.
    pub games: crate::windows::game_list::GameListUi,
    /// The visual state of the game browser (spec M3-4e).
    pub browser: crate::windows::browser::BrowserUi,
    /// The last theme the system reported.
    pub system_mode: iced::theme::Mode,
    /// Whether the About box is open.
    pub show_about: bool,
    /// The tab and the temporary state of the ribbon (spec M9-2).
    pub ribbon: crate::windows::ribbon::RibbonUi,
    /// The last known window size.
    pub window_size: Option<Size>,
    /// Whether the window is full screen (toggled by the shortcut).
    pub fullscreen: bool,
    /// The last remark of the application (file, clipboard, preferences),
    /// shown in the status bar until the user does something else.
    pub status_notice: Option<mychess_app::AppNotice>,
    /// The visual state of the dialogs, the move field and the status clock.
    pub dialogs: crate::windows::dialog_ui::DialogUi,
    /// The notation panel and the comment editor.
    pub notation: crate::windows::notation::NotationPanel,
    /// The running animation.
    pub animation: Option<ActiveAnimation>,
    caches: BoardCaches,
    pieces: PieceSet,
    marks: Vec<BoardMark>,
    shown: Option<(ZobristHash, u32)>,
    shown_position: Option<Position>,
    suppress_next: bool,
    /// The game generation last seen; a change means "another game".
    seen_generation: u64,
    /// Whether the comment editor was on screen when the shell last
    /// synchronized it (see [`UiState::take_comment_change`]).
    comment_synced: bool,
}

impl UiState {
    /// A fresh state with the given panel arrangement.
    pub fn new(layout: &LayoutNode) -> UiState {
        UiState {
            orientation: Orientation::White,
            brush: MarkBrush::default(),
            panes: pane_grid::State::with_configuration(layout::to_configuration(layout)),
            tab_menu: None,
            graph: crate::windows::graph::GraphUi::default(),
            report_graph: crate::windows::graph::GraphUi::default(),
            analysis: crate::windows::analysis::AnalysisUi::default(),
            games_auto: false,
            games: crate::windows::game_list::GameListUi::default(),
            browser: crate::windows::browser::BrowserUi::default(),
            system_mode: iced::theme::Mode::None,
            show_about: false,
            ribbon: crate::windows::ribbon::RibbonUi::default(),
            window_size: None,
            fullscreen: false,
            status_notice: None,
            dialogs: crate::windows::dialog_ui::DialogUi::default(),
            notation: crate::windows::notation::NotationPanel::default(),
            animation: None,
            caches: BoardCaches::default(),
            pieces: PieceSet::default(),
            marks: Vec::new(),
            shown: None,
            shown_position: None,
            suppress_next: false,
            seen_generation: 0,
            comment_synced: false,
        }
    }

    /// Whether the visibility of the comment editor changed since the last
    /// call: `Some(true)` when it came into sight, `Some(false)` when it
    /// went out. Whatever path changed the arrangement, the shell asks here
    /// and saves the draft or opens the editor (D1027).
    pub fn take_comment_change(&mut self) -> Option<bool> {
        let now = self.comment_visible();
        (now != std::mem::replace(&mut self.comment_synced, now)).then_some(now)
    }

    /// Turns the board; this ends any animation.
    pub fn flip(&mut self) {
        self.orientation = self.orientation.flipped();
        self.animation = None;
    }

    /// Records the game generation of the session; a different game (new,
    /// opened, pasted) must not animate from the previous one.
    pub(crate) fn note_generation(&mut self, generation: u64) {
        if self.seen_generation != generation {
            self.seen_generation = generation;
            self.suppress_next = true;
        }
    }

    /// The next position change must not animate (the user dragged the piece).
    pub fn suppress_next_animation(&mut self) {
        self.suppress_next = true;
    }

    /// The marks of the current comment, as last synchronized.
    pub fn marks(&self) -> &[BoardMark] {
        &self.marks
    }

    /// Brings the visual state up to date with the application state:
    /// starts or ends animations and clears the board
    /// caches whose content changed (spec rules 30, 56).
    pub(crate) fn sync(
        &mut self,
        view: &BoardView<'_>,
        marks: Vec<BoardMark>,
        appearance: &Appearance,
    ) {
        let id = (view.position.zobrist(), view.position.ply());
        if self.shown != Some(id) {
            let animation = if appearance.animation_ms > 0 && !self.suppress_next {
                self.shown_position
                    .as_ref()
                    .and_then(|old| animation_between(old, view.position))
            } else {
                None
            };
            self.animation = animation.map(|mut animation| {
                animation.duration = Duration::from_millis(u64::from(appearance.animation_ms));
                ActiveAnimation {
                    animation,
                    started: None,
                    eased: 0.0,
                }
            });
            self.shown = Some(id);
            self.shown_position = Some(view.position.clone());
        }
        self.suppress_next = false;
        // Handles are built once per set; the board caches notice the new
        // set id in their signatures and redraw only what shows pieces.
        if self.pieces.id() != appearance.piece_set {
            self.pieces = PieceSet::load(appearance.piece_set);
        }
        if self.dialogs.pieces.id() != appearance.piece_set {
            self.dialogs.pieces = PieceSet::load(appearance.piece_set);
        }
        // Compare before replacing: the common message changes nothing here.
        if self.marks != marks {
            self.marks = marks;
        }
        // Built field by field so `caches` can be borrowed mutably.
        let data = BoardData {
            position: view.position,
            last_move: view.last_move,
            marks: &self.marks,
            orientation: self.orientation,
            options: appearance.board,
            accepts_moves: view.accepts_moves,
            brush: &self.brush,
            pieces: &self.pieces,
            animation: self.animation.as_ref().map(|a| (&a.animation, a.eased)),
        };
        self.caches.sync_data(&data);
    }

    /// Brings the layer of the engine arrows up to date (spec M4-5).
    pub(crate) fn sync_arrows(&mut self, arrows: &[mychess_app::EngineArrow]) {
        self.caches.sync_arrows(arrows, self.orientation);
    }

    /// Brings the layer of the class badge up to date (spec M4-10, D1434).
    pub(crate) fn sync_badge(&mut self, badge: Option<crate::widgets::board::ClassBadge>) {
        self.caches.sync_badge(badge, self.orientation);
    }

    /// Advances the animation clock to `now`; ends the animation when done.
    pub fn on_frame(&mut self, now: Instant) {
        let Some(active) = &mut self.animation else {
            return;
        };
        let started = *active.started.get_or_insert(now);
        let elapsed = now.saturating_duration_since(started);
        let p = progress(elapsed, active.animation.duration);
        if p >= 1.0 {
            self.animation = None;
        } else {
            active.eased = ease_out_cubic(p);
        }
    }

    /// Whether frames must be delivered (an animation is running).
    pub fn is_animating(&self) -> bool {
        self.animation.is_some()
    }

    fn data<'a>(&'a self, view: &BoardView<'a>, appearance: &Appearance) -> BoardData<'a> {
        BoardData {
            position: view.position,
            last_move: view.last_move,
            marks: &self.marks,
            orientation: self.orientation,
            options: appearance.board,
            accepts_moves: view.accepts_moves,
            brush: &self.brush,
            pieces: &self.pieces,
            animation: self.animation.as_ref().map(|a| (&a.animation, a.eased)),
        }
    }

    /// The selected piece set (the player strips draw captured pieces).
    pub(crate) fn pieces(&self) -> &PieceSet {
        &self.pieces
    }

    /// The board widget for the current state.
    pub(crate) fn board_widget<'a>(
        &'a self,
        view: &BoardView<'a>,
        appearance: &Appearance,
    ) -> BoardWidget<'a> {
        BoardWidget::new(self.data(view, appearance), &self.caches)
    }

    // ---- panels ----

    /// The arrangement as data, for saving.
    pub fn layout(&self) -> LayoutNode {
        layout::from_state(&self.panes).unwrap_or_else(LayoutNode::default_layout)
    }

    /// Shows `layout` in the grid (the groups, tabs and ratios it has).
    fn adopt(&mut self, layout: &LayoutNode) {
        let before = self.watched_visibility();
        self.panes = pane_grid::State::with_configuration(layout::to_configuration(layout));
        self.tab_menu = None;
        self.release_hidden(before);
    }

    /// Whether the analysis panel and the game list are on screen.
    fn watched_visibility(&self) -> (bool, bool) {
        (
            self.is_visible(PaneKind::Analysis),
            self.is_visible(PaneKind::GameList),
        )
    }

    /// A panel that went out of sight (closed or behind another tab) keeps
    /// no focus, hover, menu, drag or box: the keyboard must not reach it
    /// (D1008, D1026). Every change of the arrangement passes through here.
    fn release_hidden(&mut self, before: (bool, bool)) {
        let after = self.watched_visibility();
        if before.0 && !after.0 {
            self.analysis.pane_closed();
        }
        if before.1 && !after.1 {
            self.games.close_with_panel(false);
        }
    }

    /// Whether the panel is open: a tab of some group, selected or not
    /// (spec M2-6, D1008).
    pub fn is_open(&self, kind: PaneKind) -> bool {
        self.panes.iter().any(|(_, group)| group.contains(kind))
    }

    /// Whether the panel is the selected tab of its group.
    pub fn is_visible(&self, kind: PaneKind) -> bool {
        self.panes
            .iter()
            .any(|(_, group)| group.active_kind() == kind)
    }

    /// Whether the comment editor is on screen.
    pub fn comment_visible(&self) -> bool {
        self.is_visible(PaneKind::Comment)
    }

    /// The panel whose group draws the command bar and the move field: the
    /// first group (in layout order) with the move list (spec M2-6, D1002).
    pub fn command_bar_owner(&self) -> Option<pane_grid::Pane> {
        layout::groups_in_order(&self.panes)
            .into_iter()
            .find(|(_, group)| group.contains(PaneKind::Notation))
            .map(|(pane, _)| pane)
    }

    /// Opens or closes the panel (the board cannot be closed). Reopening
    /// puts it in its default place (spec M2-6, D1004).
    pub fn toggle_pane(&mut self, kind: PaneKind) {
        if self.is_open(kind) {
            self.close_pane(kind);
        } else {
            self.open_pane(kind);
        }
    }

    /// Opens the panel and selects its tab; an open one is only selected.
    /// The one routine behind the menu, the shortcuts and the buttons.
    pub fn open_pane(&mut self, kind: PaneKind) {
        if self.is_open(kind) {
            self.select_tab(kind);
            return;
        }
        let current = self.layout();
        let next = current.open(kind);
        if next == current {
            return;
        }
        if kind == PaneKind::GameList {
            self.games_auto = false;
        }
        self.adopt(&next);
        if kind == PaneKind::GameList {
            self.games.close_with_panel(true);
        }
    }

    /// Closes the panel with the side effects its closing always had: the
    /// analysis stops being drawn, the game list drops its drag and menus.
    /// The opening tree and the graph follow [`UiState::is_open`] in the
    /// shell's synchronization.
    pub fn close_pane(&mut self, kind: PaneKind) {
        if !self.is_open(kind) {
            return;
        }
        let current = self.layout();
        let next = current.close(kind);
        if next == current {
            return;
        }
        self.adopt(&next);
        match kind {
            PaneKind::Analysis => self.analysis.pane_closed(),
            PaneKind::GameList => {
                self.games_auto = false;
                self.games.close_with_panel(false);
            }
            _ => {}
        }
    }

    /// Opens the panel if it is closed and selects it.
    pub fn show_pane(&mut self, kind: PaneKind) {
        self.open_pane(kind);
    }

    /// Makes the panel the selected tab of its group (spec M2-6, D1007).
    pub fn select_tab(&mut self, kind: PaneKind) {
        self.tab_menu = None;
        let before = self.watched_visibility();
        for (_, group) in self.panes.iter_mut() {
            if let Some(index) = group.tabs.iter().position(|k| *k == kind) {
                group.active = index;
            }
        }
        self.release_hidden(before);
    }

    /// Puts the panel out of sight without closing it: a sibling tab is
    /// selected (the move list first). Alone in its group, it is closed.
    pub fn hide_tab(&mut self, kind: PaneKind) {
        let sibling = self.panes.iter().find_map(|(_, group)| {
            if !group.contains(kind) {
                return None;
            }
            let others = group.tabs.iter().copied().filter(|k| *k != kind);
            let mut others: Vec<PaneKind> = others.collect();
            others.sort_by_key(|k| *k != PaneKind::Notation);
            others.first().copied()
        });
        match sibling {
            Some(other) => self.select_tab(other),
            None => self.close_pane(kind),
        }
    }

    /// Takes the tab out of its group into a split of its own (D1006).
    pub fn detach_tab(&mut self, kind: PaneKind) {
        let current = self.layout();
        let next = current.detach(kind);
        if next != current {
            self.adopt(&next);
        }
        self.tab_menu = None;
    }

    /// Drops the group of `source` on the center of the group of `target`:
    /// the tabs merge (D1005). The board takes no part: nothing happens.
    pub fn merge_panes(&mut self, source: pane_grid::Pane, target: pane_grid::Pane) {
        let (Some(from), Some(to)) = (self.panes.get(source), self.panes.get(target)) else {
            return;
        };
        let (Some(from), Some(to)) = (from.tabs.first().copied(), to.tabs.first().copied()) else {
            return;
        };
        let current = self.layout();
        let next = current.merge(from, to);
        if next != current {
            self.adopt(&next);
        }
    }

    /// Drops `pane` on `target` (an edge: the panel splits the space). A drop
    /// that would nest deeper than the layout allows is refused, and the
    /// previous arrangement stays (D1028).
    pub fn drop_pane(&mut self, pane: pane_grid::Pane, target: pane_grid::Target) {
        // A pane dropped on itself changes nothing (D1029): the grid would
        // split it from itself and destroy the layout.
        if matches!(target, pane_grid::Target::Pane(over, _) if over == pane) {
            return;
        }
        let Some(previous) = layout::from_state(&self.panes) else {
            return;
        };
        self.panes.drop(pane, target);
        let valid = layout::from_state(&self.panes).is_some_and(|node| node.is_valid());
        if !valid {
            self.adopt(&previous);
        }
    }

    /// Opens the game list because a database arrived (D552): it is not
    /// saved with the layout until the user takes part in it.
    pub fn show_game_list_automatically(&mut self) {
        if !self.is_open(PaneKind::GameList) {
            self.open_pane(PaneKind::GameList);
            self.games_auto = true;
        }
    }

    /// The arrangement to save: what is on screen, except a game list the
    /// application opened by itself.
    pub fn layout_to_save(&self) -> LayoutNode {
        let layout = self.layout();
        if self.games_auto {
            layout.close(PaneKind::GameList)
        } else {
            layout
        }
    }

    /// Shows `layout` (a saved arrangement) in place of the current one.
    pub fn set_layout(&mut self, layout: &LayoutNode) {
        self.adopt(layout);
        self.games_auto = false;
    }

    /// Back to the default arrangement: the board and the group of Moves,
    /// Information and Comment (spec M2-6, D1001).
    pub fn reset_layout(&mut self) {
        self.analysis.pane_closed();
        self.games_auto = false;
        self.games.close_with_panel(false);
        self.adopt(&LayoutNode::default_layout());
    }
}

#[cfg(test)]
mod tests {
    use mychess_app::AppState;

    use super::*;
    use crate::app_api;

    fn sq(name: &str) -> mychess_core::Square {
        name.parse().expect("square")
    }

    fn play(app: &mut AppState, from: &str, to: &str) {
        let mv = app
            .session
            .position()
            .find_move(sq(from), sq(to), None)
            .expect("legal");
        mychess_app::update(app, app_api::play_move(mv));
    }

    fn default_appearance() -> Appearance {
        app_api::appearance(&AppState::default(), app_api::ViewOptions::default())
    }

    fn sync(ui: &mut UiState, app: &AppState, appearance: &Appearance) {
        ui.sync(
            &app_api::board_view(app),
            app_api::board_marks(app),
            appearance,
        );
    }

    #[test]
    fn dropping_a_pane_on_its_own_edge_keeps_the_layout() {
        let mut ui = UiState::new(&LayoutNode::default_layout());
        let before = ui.layout();
        let pane = ui.panes.iter().map(|(pane, _)| *pane).next().expect("pane");
        for region in [
            pane_grid::Region::Center,
            pane_grid::Region::Edge(pane_grid::Edge::Right),
            pane_grid::Region::Edge(pane_grid::Edge::Top),
        ] {
            ui.drop_pane(pane, pane_grid::Target::Pane(pane, region));
            assert_eq!(ui.layout(), before, "{region:?}");
        }
    }

    #[test]
    fn a_single_move_starts_an_animation_and_the_clock_ends_it() {
        let appearance = default_appearance();
        let mut app = AppState::default();
        let mut ui = UiState::new(&LayoutNode::default_layout());
        sync(&mut ui, &app, &appearance);
        assert!(!ui.is_animating());
        play(&mut app, "e2", "e4");
        sync(&mut ui, &app, &appearance);
        assert!(ui.is_animating());
        let t0 = Instant::now();
        ui.on_frame(t0);
        assert!(ui.is_animating());
        ui.on_frame(t0 + Duration::from_millis(100));
        assert!(ui.is_animating());
        ui.on_frame(t0 + Duration::from_millis(250));
        assert!(!ui.is_animating());
    }

    #[test]
    fn dragged_moves_and_zero_duration_do_not_animate() {
        let mut appearance = default_appearance();
        let mut app = AppState::default();
        let mut ui = UiState::new(&LayoutNode::default_layout());
        sync(&mut ui, &app, &appearance);
        ui.suppress_next_animation();
        play(&mut app, "e2", "e4");
        sync(&mut ui, &app, &appearance);
        assert!(!ui.is_animating());
        appearance.animation_ms = 0;
        play(&mut app, "e7", "e5");
        sync(&mut ui, &app, &appearance);
        assert!(!ui.is_animating());
    }

    #[test]
    fn going_back_animates_and_a_jump_of_two_moves_does_not() {
        let appearance = default_appearance();
        let mut app = AppState::default();
        let mut ui = UiState::new(&LayoutNode::default_layout());
        sync(&mut ui, &app, &appearance);
        play(&mut app, "e2", "e4");
        play(&mut app, "e7", "e5");
        sync(&mut ui, &app, &appearance);
        assert!(!ui.is_animating(), "two moves in one step do not animate");
        mychess_app::update(&mut app, app_api::navigate(mychess_app::Nav::Back(1)));
        sync(&mut ui, &app, &appearance);
        assert!(ui.is_animating());
        mychess_app::update(&mut app, app_api::navigate(mychess_app::Nav::Start));
        sync(&mut ui, &app, &appearance);
        assert!(ui.is_animating(), "start is one move back");
    }

    #[test]
    fn flipping_ends_the_animation() {
        let appearance = default_appearance();
        let mut app = AppState::default();
        let mut ui = UiState::new(&LayoutNode::default_layout());
        sync(&mut ui, &app, &appearance);
        play(&mut app, "d2", "d4");
        sync(&mut ui, &app, &appearance);
        ui.flip();
        assert!(!ui.is_animating());
        assert_eq!(ui.orientation, Orientation::Black);
    }

    #[test]
    fn panes_can_be_closed_reopened_and_reset() {
        let mut ui = UiState::new(&LayoutNode::default_layout());
        assert_eq!(
            ui.layout().visible_panes(),
            [PaneKind::Board, PaneKind::Notation]
        );
        ui.toggle_pane(PaneKind::Notation);
        assert!(!ui.is_open(PaneKind::Notation) && ui.is_open(PaneKind::Info));
        ui.toggle_pane(PaneKind::Info);
        ui.toggle_pane(PaneKind::Comment);
        assert_eq!(ui.layout(), LayoutNode::pane(PaneKind::Board));
        ui.toggle_pane(PaneKind::Board);
        assert_eq!(
            ui.layout().visible_panes().len(),
            1,
            "the board cannot be closed"
        );
        // With nothing of the family open, a panel enters beside the board.
        ui.toggle_pane(PaneKind::Info);
        assert_eq!(
            ui.layout().visible_panes(),
            [PaneKind::Board, PaneKind::Info]
        );
        // The others join its group, as tabs (D1004).
        ui.toggle_pane(PaneKind::Notation);
        assert_eq!(
            ui.layout()
                .group_of(PaneKind::Info)
                .map(|(tabs, _)| tabs.to_vec()),
            Some(vec![PaneKind::Info, PaneKind::Notation])
        );
        ui.reset_layout();
        assert_eq!(ui.layout(), LayoutNode::default_layout());
        assert!(ui.is_open(PaneKind::Info) && ui.is_open(PaneKind::Notation));
    }

    #[test]
    fn the_game_list_opens_around_the_panels_and_reset_closes_it() {
        let mut ui = UiState::new(&LayoutNode::default_layout());
        assert!(!ui.is_open(PaneKind::GameList));
        ui.show_pane(PaneKind::GameList);
        assert!(ui.is_open(PaneKind::GameList));
        assert_eq!(
            ui.layout().visible_panes(),
            [PaneKind::Board, PaneKind::Notation, PaneKind::GameList]
        );
        ui.show_pane(PaneKind::GameList);
        assert!(
            ui.is_open(PaneKind::GameList),
            "showing an open panel changes nothing"
        );
        ui.toggle_pane(PaneKind::GameList);
        assert_eq!(ui.layout(), LayoutNode::default_layout());
        ui.toggle_pane(PaneKind::GameList);
        ui.reset_layout();
        assert!(!ui.is_open(PaneKind::GameList));
        assert_eq!(ui.layout(), LayoutNode::default_layout());
        let saved = LayoutNode::default_layout().open(PaneKind::GameList);
        ui.set_layout(&saved);
        assert!(ui.is_open(PaneKind::GameList));
        assert_eq!(ui.layout(), saved, "the saved arrangement comes back whole");
    }

    #[test]
    fn closing_keeps_the_groups_and_the_ratios_the_user_chose() {
        let mut ui = UiState::new(&LayoutNode::default_layout());
        ui.show_pane(PaneKind::GameList);
        ui.show_pane(PaneKind::Analysis);
        let split = match ui.panes.layout() {
            pane_grid::Node::Split { id, .. } => *id,
            pane_grid::Node::Pane(_) => panic!("splits expected"),
        };
        ui.panes.resize(split, 0.5);
        let before = ui.layout();
        ui.close_pane(PaneKind::Info);
        let after = ui.layout();
        assert_eq!(
            after
                .group_of(PaneKind::Notation)
                .map(|(tabs, _)| tabs.len()),
            Some(2)
        );
        assert_eq!(after.panes().len(), before.panes().len() - 1);
        ui.close_pane(PaneKind::Analysis);
        ui.close_pane(PaneKind::GameList);
        assert_eq!(
            ui.layout(),
            before
                .close(PaneKind::Info)
                .close(PaneKind::Analysis)
                .close(PaneKind::GameList)
        );
    }

    #[test]
    fn selecting_a_tab_and_hiding_one() {
        let mut ui = UiState::new(&LayoutNode::default_layout());
        assert!(ui.is_visible(PaneKind::Notation) && !ui.comment_visible());
        ui.select_tab(PaneKind::Comment);
        assert!(ui.comment_visible());
        ui.hide_tab(PaneKind::Comment);
        assert!(
            ui.is_visible(PaneKind::Notation),
            "the move list comes first"
        );
        assert!(ui.is_open(PaneKind::Comment), "hidden, not closed");
        ui.close_pane(PaneKind::Notation);
        ui.close_pane(PaneKind::Info);
        ui.hide_tab(PaneKind::Comment);
        assert!(!ui.is_open(PaneKind::Comment), "alone, it is closed");
    }

    #[test]
    fn a_game_list_opened_by_the_application_is_not_saved() {
        let mut ui = UiState::new(&LayoutNode::default_layout());
        ui.show_game_list_automatically();
        assert!(ui.is_open(PaneKind::GameList));
        assert_eq!(ui.layout_to_save(), LayoutNode::default_layout());
        ui.toggle_pane(PaneKind::GameList);
        ui.toggle_pane(PaneKind::GameList);
        assert!(ui.layout_to_save().contains(PaneKind::GameList));
    }

    #[test]
    fn sync_extracts_marks_from_the_comment() {
        let appearance = default_appearance();
        let mut app = AppState::default();
        let mut ui = UiState::new(&LayoutNode::default_layout());
        let mark = BoardMark::arrow(sq("d4"), sq("f6"), "green");
        let message = app_api::toggle_mark_message(&app, &mark);
        mychess_app::update(&mut app, message);
        sync(&mut ui, &app, &appearance);
        assert_eq!(ui.marks(), [mark]);
    }
}
