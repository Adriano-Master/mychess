//! The windows of the players, the tournaments and the spelling file (spec
//! M6-3, slice c): the player finder, the tournament finder, the player
//! information, the name checker and "Add Elo ratings".
//!
//! The application owns everything that matters ([`mychess_app::FinderMessage`],
//! `PlayerInfoMessage`, `SpellingMessage` and their views); this module draws
//! the views and keeps only what is visual: which window is on top, the
//! scroll of the lists and the row the keyboard stands on.

pub mod finder_view;
pub mod info_view;
pub mod spell_view;
pub mod table;
pub mod text;

use iced::Size;
use mychess_app::{AppState, PlayerRowView, Shared, TournamentRowView};

use crate::shell::Message;

/// Id of the box of the player finder.
pub const PLAYER_FINDER_BOX_ID: &str = "player-finder-box";
/// Id of the box of the tournament finder.
pub const TOURNAMENT_FINDER_BOX_ID: &str = "tournament-finder-box";
/// Id of the box of the player information.
pub const PLAYER_INFO_BOX_ID: &str = "player-info-box";
/// Id of the box of the name checker.
pub const SPELL_BOX_ID: &str = "spell-check-box";
/// Id of the box of "Add Elo ratings".
pub const ADD_ELO_BOX_ID: &str = "add-elo-box";
/// Id of the field "Player" of the player finder.
pub const PLAYER_PREFIX_ID: &str = "player-finder-prefix";

/// One of the five windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolWindow {
    /// The player finder.
    PlayerFinder,
    /// The tournament finder.
    TournamentFinder,
    /// The player information.
    PlayerInfo,
    /// The name checker.
    SpellCheck,
    /// "Add Elo ratings".
    AddElo,
}

impl ToolWindow {
    /// Every window, in the order the stack looks for the open ones.
    pub const ALL: [ToolWindow; 5] = [
        ToolWindow::PlayerFinder,
        ToolWindow::TournamentFinder,
        ToolWindow::PlayerInfo,
        ToolWindow::SpellCheck,
        ToolWindow::AddElo,
    ];

    /// Whether the application has this window open (cheap: no view is
    /// built).
    pub fn is_open(self, app: &AppState) -> bool {
        match self {
            ToolWindow::PlayerFinder => app.finder().players_open(),
            ToolWindow::TournamentFinder => app.finder().tournaments_open(),
            ToolWindow::PlayerInfo => app.player_info().is_open(),
            ToolWindow::SpellCheck => app.spelling().is_check_open(),
            ToolWindow::AddElo => app.spelling().is_add_elo_open(),
        }
    }

    /// The id of the container of the window (the focus scope).
    pub fn box_id(self) -> &'static str {
        match self {
            ToolWindow::PlayerFinder => PLAYER_FINDER_BOX_ID,
            ToolWindow::TournamentFinder => TOURNAMENT_FINDER_BOX_ID,
            ToolWindow::PlayerInfo => PLAYER_INFO_BOX_ID,
            ToolWindow::SpellCheck => SPELL_BOX_ID,
            ToolWindow::AddElo => ADD_ELO_BOX_ID,
        }
    }
}

/// A list that scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListId {
    /// The rows of the player finder.
    Players,
    /// The rows of the tournament finder.
    Tournaments,
    /// The corrections of the name checker.
    Spell,
}

/// An event of the windows that is not just a message for the application.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayerToolsMessage {
    /// A list scrolled to this offset (pixels).
    Scrolled(ListId, f32),
    /// The click on a row of the player finder: the keyboard stands there
    /// and the information of the player opens.
    ClickPlayer(usize),
    /// The click on a row of the tournament finder: it is selected.
    SelectTournament(usize),
    /// A time went by while something runs (the progress is redrawn).
    Tick,
}

/// The scroll of a list and the row the keyboard stands on.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ListState {
    /// Pixels scrolled from the top.
    pub scroll: f32,
    /// The row of the cursor.
    pub cursor: Option<usize>,
}

/// The visual state of the windows.
#[derive(Debug, Default)]
pub struct PlayerToolsUi {
    /// `LoadAtStart` was sent (once, after the preferences were read).
    pub started: bool,
    stack: Vec<ToolWindow>,
    /// The player finder list.
    pub players: ListState,
    /// The tournament finder list.
    pub tournaments: ListState,
    /// The list of the name checker.
    pub spell: ListState,
    /// The rows the player cursor was set for (a new list drops it).
    players_seen: Option<Shared<Vec<PlayerRowView>>>,
    /// The rows the tournament cursor was set for.
    tournaments_seen: Option<Shared<Vec<TournamentRowView>>>,
    /// The number of rows the checker cursor was set for.
    spell_seen: usize,
}

impl PlayerToolsUi {
    /// Follows the application: a window that opened goes on top, one that
    /// closed leaves the stack and forgets its scroll.
    pub fn sync(&mut self, app: &AppState) {
        for window in ToolWindow::ALL {
            let open = window.is_open(app);
            let there = self.stack.contains(&window);
            if open && !there {
                self.stack.push(window);
            } else if !open && there {
                self.stack.retain(|w| *w != window);
            }
            if !open {
                match window {
                    ToolWindow::PlayerFinder => {
                        self.players = ListState::default();
                        self.players_seen = None;
                    }
                    ToolWindow::TournamentFinder => {
                        self.tournaments = ListState::default();
                        self.tournaments_seen = None;
                    }
                    ToolWindow::SpellCheck => {
                        self.spell = ListState::default();
                        self.spell_seen = 0;
                    }
                    _ => {}
                }
            }
        }
        self.drop_stale_cursors(app);
    }

    /// A list that changed its rows leaves the cursor on no row (it could
    /// point past the end or at another player).
    fn drop_stale_cursors(&mut self, app: &AppState) {
        if let Some(v) = app.player_finder_view()
            && self.players_seen.as_ref() != Some(&v.rows)
        {
            self.players.cursor = None;
            self.players_seen = Some(v.rows);
        }
        if let Some(rows) = app.tournament_finder_view().and_then(|v| v.rows)
            && self.tournaments_seen.as_ref() != Some(&rows)
        {
            self.tournaments.cursor = None;
            self.tournaments_seen = Some(rows);
        }
        if let Some(flags) = app.spell_flags()
            && self.spell_seen != flags.rows
        {
            self.spell.cursor = None;
            self.spell_seen = flags.rows;
        }
    }

    /// The window on top.
    pub fn top(&self) -> Option<ToolWindow> {
        self.stack.last().copied()
    }

    /// The state of a list.
    pub fn list(&self, id: ListId) -> &ListState {
        match id {
            ListId::Players => &self.players,
            ListId::Tournaments => &self.tournaments,
            ListId::Spell => &self.spell,
        }
    }

    /// The state of a list, to change.
    pub fn list_mut(&mut self, id: ListId) -> &mut ListState {
        match id {
            ListId::Players => &mut self.players,
            ListId::Tournaments => &mut self.tournaments,
            ListId::Spell => &mut self.spell,
        }
    }
}

/// The width of a card for a window `window` wide: `wanted`, or less in a
/// narrow window.
pub fn card_width(wanted: f32, window: Size) -> f32 {
    wanted.min((window.width - 32.0).max(320.0))
}

/// The height of a list in a card, for a window `window` high: `chrome` is
/// what the rest of the card spends.
pub fn list_height(window: Size, chrome: f32) -> f32 {
    (window.height - chrome).clamp(100.0, 460.0)
}

/// Whether anything of these windows runs and shows a progress (the shell
/// then redraws now and then).
pub fn wants_ticks(app: &AppState) -> bool {
    let busy = |p: Option<(u64, u64)>| p.is_some();
    app.spelling().is_loading()
        || app.player_finder_view().is_some_and(|v| busy(v.building))
        || app
            .tournament_finder_view()
            .is_some_and(|v| busy(v.running))
        || busy(app.player_info_progress())
        || app.spell_flags().is_some_and(mychess_app::SpellFlags::busy)
        || app
            .add_elo_flags()
            .is_some_and(mychess_app::AddEloFlags::busy)
}

/// A message of the application as a message of the shell.
pub(crate) fn app(message: impl Into<mychess_app::Message>) -> Message {
    Message::App(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stack_keeps_the_newest_window_on_top() {
        let mut ui = PlayerToolsUi::default();
        let app = AppState::default();
        ui.sync(&app);
        assert_eq!(ui.top(), None);
    }

    #[test]
    fn a_card_never_exceeds_a_narrow_window() {
        assert!((card_width(1100.0, Size::new(2000.0, 900.0)) - 1100.0).abs() < f32::EPSILON);
        assert!((card_width(1100.0, Size::new(600.0, 900.0)) - 568.0).abs() < f32::EPSILON);
        assert!((card_width(1100.0, Size::new(200.0, 900.0)) - 320.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_list_keeps_a_readable_height() {
        assert!((list_height(Size::new(900.0, 100.0), 300.0) - 100.0).abs() < f32::EPSILON);
        assert!((list_height(Size::new(900.0, 2000.0), 300.0) - 460.0).abs() < f32::EPSILON);
    }
}
