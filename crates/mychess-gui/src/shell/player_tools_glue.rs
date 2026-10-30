//! The part of the shell that serves the windows of the players, the
//! tournaments and the spelling file (spec M6-3c): the scroll of the lists,
//! the row the keyboard stands on and the keys of the windows.

use iced::Task;
use iced::widget::scrollable::AbsoluteOffset;
use mychess_app::{
    FinderMessage, Key, KeyInput, Message as AppMessage, Modifiers, NamedKey, PlayerInfoMessage,
    SpellingMessage, TournamentAction,
};

use super::{Message, Shell};
use crate::windows::dialog_ui::ActiveDialog;
use crate::windows::player_tools::table::{self, ROW_HEIGHT};
use crate::windows::player_tools::{
    ListId, PlayerToolsMessage, ToolWindow, finder_view, list_height, spell_view,
};

impl Shell {
    /// An event of the windows that is not just a message for the
    /// application.
    pub(super) fn update_player_tools(&mut self, message: PlayerToolsMessage) -> Task<Message> {
        let tools = &mut self.ui.dialogs.player_tools;
        match message {
            PlayerToolsMessage::Scrolled(list, offset) => {
                tools.list_mut(list).scroll = offset;
                Task::none()
            }
            PlayerToolsMessage::ClickPlayer(index) => {
                tools.players.cursor = Some(index);
                self.dispatch(AppMessage::Finder(FinderMessage::ClickPlayer(index)))
            }
            PlayerToolsMessage::SelectTournament(index) => {
                tools.tournaments.cursor = Some(index);
                Task::none()
            }
            PlayerToolsMessage::Tick => Task::none(),
        }
    }

    /// A key pressed while a window of these is on top. `None` leaves the
    /// key to the generic handling of the dialogs (Tab cycles the focus).
    pub(super) fn player_tools_key(
        &mut self,
        active: ActiveDialog,
        input: &KeyInput,
        text_focused: bool,
    ) -> Option<Task<Message>> {
        let window = active.tool_window()?;
        if input.modifiers != Modifiers::NONE {
            return None;
        }
        let Key::Named(key) = input.key else {
            return None;
        };
        match key {
            NamedKey::Escape => Some(self.tools_escape(window)),
            NamedKey::Enter if !text_focused => Some(self.tools_enter(window)),
            NamedKey::Up
            | NamedKey::Down
            | NamedKey::PageUp
            | NamedKey::PageDown
            | NamedKey::Home
            | NamedKey::End
                if !text_focused =>
            {
                self.tools_move(window, key)
            }
            _ => None,
        }
    }

    fn tools_escape(&mut self, window: ToolWindow) -> Task<Message> {
        let message = match window {
            ToolWindow::PlayerFinder => AppMessage::Finder(FinderMessage::ClosePlayers),
            ToolWindow::TournamentFinder => AppMessage::Finder(FinderMessage::CloseTournaments),
            ToolWindow::PlayerInfo => AppMessage::PlayerInfo(PlayerInfoMessage::Close),
            ToolWindow::SpellCheck => match self.app.spell_flags() {
                Some(f) if f.scanning => AppMessage::Spelling(SpellingMessage::CancelScan),
                Some(f) if f.confirm => AppMessage::Spelling(SpellingMessage::ConfirmApply(false)),
                // A write in flight cannot be taken back: Esc waits for it.
                Some(f) if f.writing => return Task::none(),
                _ => AppMessage::Spelling(SpellingMessage::CloseCheck),
            },
            ToolWindow::AddElo => match self.app.add_elo_flags() {
                Some(f) if f.planning => AppMessage::Spelling(SpellingMessage::CancelJob),
                Some(f) if f.writing => return Task::none(),
                _ => AppMessage::Spelling(SpellingMessage::CloseAddElo),
            },
        };
        self.dispatch(message)
    }

    fn tools_enter(&mut self, window: ToolWindow) -> Task<Message> {
        let tools = &self.ui.dialogs.player_tools;
        let message = match window {
            ToolWindow::PlayerFinder => match tools.players.cursor {
                Some(index) => {
                    return self.update_player_tools(PlayerToolsMessage::ClickPlayer(index));
                }
                None => AppMessage::Finder(FinderMessage::UpdatePlayers),
            },
            ToolWindow::TournamentFinder => match tools.tournaments.cursor {
                Some(index) => AppMessage::Finder(FinderMessage::ClickTournament {
                    index,
                    action: TournamentAction::OpenCrosstable,
                }),
                None => AppMessage::Finder(FinderMessage::UpdateTournaments),
            },
            ToolWindow::PlayerInfo => AppMessage::PlayerInfo(PlayerInfoMessage::Refresh),
            ToolWindow::SpellCheck => match self.app.spell_flags() {
                Some(f) if f.confirm => AppMessage::Spelling(SpellingMessage::ConfirmApply(true)),
                _ => return Task::none(),
            },
            ToolWindow::AddElo => match self.app.add_elo_flags() {
                Some(f) if f.has_result => AppMessage::Spelling(SpellingMessage::CloseAddElo),
                Some(f) if f.planned && !f.writing => {
                    AppMessage::Spelling(SpellingMessage::ConfirmElo)
                }
                Some(f) if !f.planning && !f.writing => {
                    AppMessage::Spelling(SpellingMessage::PlanElo)
                }
                _ => return Task::none(),
            },
        };
        self.dispatch(message)
    }

    /// The arrows, the pages and Home/End move the row of the cursor of the
    /// list of the window and bring it into view.
    fn tools_move(&mut self, window: ToolWindow, key: NamedKey) -> Option<Task<Message>> {
        let size = self
            .ui
            .window_size
            .unwrap_or(iced::Size::new(1280.0, 800.0));
        let (list, total, height, row_height) = match window {
            ToolWindow::PlayerFinder => (
                ListId::Players,
                self.app.player_finder_view()?.rows.len(),
                list_height(size, finder_view::PLAYER_CHROME),
                ROW_HEIGHT,
            ),
            ToolWindow::TournamentFinder => (
                ListId::Tournaments,
                self.app
                    .tournament_finder_view()?
                    .rows
                    .map_or(0, |r| r.len()),
                list_height(size, finder_view::TOURNAMENT_CHROME),
                ROW_HEIGHT,
            ),
            ToolWindow::SpellCheck => (
                ListId::Spell,
                self.app.spell_flags()?.rows,
                list_height(size, spell_view::CHROME),
                spell_view::row_height(),
            ),
            _ => return None,
        };
        if total == 0 {
            return Some(Task::none());
        }
        let page = table::page_rows(height, row_height);
        let state = self.ui.dialogs.player_tools.list_mut(list);
        let last = total - 1;
        let target = match (key, state.cursor) {
            (NamedKey::End, _) => last,
            (NamedKey::Up, Some(at)) => at.min(last).saturating_sub(1),
            (NamedKey::PageUp, Some(at)) => at.min(last).saturating_sub(page),
            (NamedKey::Down, Some(at)) => (at.min(last) + 1).min(last),
            (NamedKey::PageDown, Some(at)) => (at.min(last) + page).min(last),
            _ => 0,
        };
        let target = target.min(last);
        state.cursor = Some(target);
        let Some(offset) = table::reveal(state.scroll, target, height, row_height) else {
            return Some(Task::none());
        };
        state.scroll = offset;
        Some(iced::widget::operation::scroll_to(
            table::scroll_id(list),
            AbsoluteOffset {
                x: Some(0.0),
                y: Some(offset),
            },
        ))
    }
}
