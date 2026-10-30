//! What the shell does for the game list panel (spec M3-4, slice 4b): runs
//! its messages, handles its keys, keeps its position inside the range and
//! opens the panel when a database with several games arrives.

use iced::Task;
use mychess_app::KeyInput;

use super::{Message, Shell};
use crate::windows::game_list::{GameListMessage, ListKey, SEARCH_ID};

impl Shell {
    /// Handles a message of the game list.
    pub(super) fn update_games(&mut self, message: GameListMessage) -> Task<Message> {
        if matches!(
            message,
            GameListMessage::FocusSearch | GameListMessage::Key(ListKey::FocusSearch)
        ) {
            return self.focus_search();
        }
        let messages = self.ui.games.update(message, &self.app);
        let tasks: Vec<Task<Message>> = messages
            .into_iter()
            .map(|message| self.dispatch(message))
            .collect();
        Task::batch(tasks)
    }

    /// A key press for the list: Page Up and Down while the panel is open,
    /// the arrows, Home, End and Enter while the pointer is over it (they
    /// have other meanings over the board). `None`: not a key of the list.
    pub(super) fn game_list_key(
        &mut self,
        input: &KeyInput,
        text_focused: bool,
    ) -> Option<Task<Message>> {
        // The search field has the focus: its keys are text, not list
        // commands (D565). `text_focused` alone misses Tab, Page Up and
        // Page Down, which a `text_input` does not capture.
        if !self
            .ui
            .is_visible(crate::windows::layout::PaneKind::GameList)
            || text_focused
            || self.ui.games.search_focused
            || self.modal_layer_open()
        {
            return None;
        }
        let key = ListKey::of(input)?;
        if key.needs_pointer() && !self.ui.games.hovered {
            return None;
        }
        Some(self.update_games(GameListMessage::Key(key)))
    }

    /// Whether the keys that edit the filter or write flags may act: the
    /// panel is open and the search field is not being typed in.
    pub(super) fn list_panel_ready(&self) -> bool {
        self.ui
            .is_visible(crate::windows::layout::PaneKind::GameList)
            && !self.ui.games.search_focused
    }

    /// Moves the focus to the search field of the list (Ctrl/Cmd+F over the
    /// list, or the shortcut of the map). Does nothing with the panel
    /// closed.
    pub(super) fn focus_search(&mut self) -> Task<Message> {
        if !self
            .ui
            .is_visible(crate::windows::layout::PaneKind::GameList)
        {
            return Task::none();
        }
        self.ui.games.search_focused = true;
        iced::widget::operation::focus(SEARCH_ID)
    }

    /// Asks the widget whether the search field has the focus. Run after
    /// every click: the focus changes by clicking, and `text_input` has no
    /// "focus lost" event to subscribe to (D565).
    pub(super) fn probe_search_focus(&self) -> Task<Message> {
        if !self
            .ui
            .is_visible(crate::windows::layout::PaneKind::GameList)
        {
            return Task::none();
        }
        iced::widget::operation::is_focused(SEARCH_ID)
            .map(|focused| Message::Games(GameListMessage::SearchFocus(focused)))
    }

    /// Brings the position of the list of the current database back inside
    /// its range (the window grew, another tab was selected). Moving a
    /// list has no effects, so the messages go straight to the
    /// application.
    pub(super) fn clamp_game_list(&mut self) {
        if !self.ui.is_open(crate::windows::layout::PaneKind::GameList) {
            return;
        }
        if let Some(message) = self.ui.games.clamp(&self.app) {
            drop(mychess_app::update(&mut self.app, message));
        }
    }

    /// Opens the game list panel when the database just opened has several
    /// games (D537); a file with one game is loaded at once instead.
    pub(super) fn reveal_game_list(&mut self) -> Task<Message> {
        let current = self.app.bases.current();
        let several = !current.is_clipbase()
            && self
                .app
                .bases
                .slot(current)
                .is_some_and(|slot| slot.total() > 1);
        if several {
            // Not saved by itself (D552): only what the user does is.
            self.ui.show_game_list_automatically();
        }
        Task::none()
    }
}

#[cfg(test)]
#[allow(clippy::format_push_string)]
mod tests {
    use std::path::Path;

    use mychess_app::{
        BasesMessage, Effect, Key, KeyInput, Message as AppMessage, Modifiers, NamedKey, SlotId,
        update,
    };

    use super::*;

    fn pgn(dir: &Path, name: &str, count: u32) -> std::path::PathBuf {
        let mut text = String::new();
        for n in 1..=count {
            text.push_str(&format!("[White \"W {n}\"]\n\n1. e4 e5 *\n\n"));
        }
        let path = dir.join(name);
        std::fs::write(&path, text).expect("write");
        path
    }

    /// A shell with two databases of 40 games open and the list shown;
    /// the second one is current.
    fn shell(dir: &Path) -> Shell {
        let mut shell = Shell::new(None);
        shell.app.open_pgn_in_slots = true;
        for name in ["a.pgn", "b.pgn"] {
            let mut queue = vec![AppMessage::Bases(BasesMessage::Open(pgn(dir, name, 40)))];
            while let Some(message) = queue.pop() {
                for effect in update(&mut shell.app, message) {
                    if matches!(
                        effect,
                        Effect::OpenBase { .. } | Effect::LoadBaseGame { .. }
                    ) {
                        queue.push(crate::test_support::run_effect(effect));
                    }
                }
            }
        }
        shell
            .ui
            .show_pane(crate::windows::layout::PaneKind::GameList);
        shell.ui.games.hovered = true;
        shell
    }

    fn plain(key: NamedKey) -> KeyInput {
        KeyInput {
            key: Key::Named(key),
            modifiers: Modifiers::NONE,
        }
    }

    fn first(shell: &Shell) -> u32 {
        let id = shell.app.bases.current();
        shell.app.bases.slot(id).expect("slot").list.first
    }

    #[test]
    fn keys_belong_to_the_list_while_the_pointer_is_over_it() {
        let dir = tempfile::tempdir().expect("dir");
        let mut shell = shell(dir.path());
        assert!(
            shell
                .game_list_key(&plain(NamedKey::PageDown), false)
                .is_some()
        );
        assert!(
            shell
                .game_list_key(&plain(NamedKey::Enter), false)
                .is_some()
        );
    }

    #[test]
    fn a_focused_search_field_keeps_page_keys_and_enter_for_itself() {
        let dir = tempfile::tempdir().expect("dir");
        let mut shell = shell(dir.path());
        // The widget did not capture the key (a text_input leaves Page Up
        // and Page Down alone), but the field has the focus: D565.
        shell.ui.games.search_focused = true;
        for key in [
            NamedKey::PageDown,
            NamedKey::PageUp,
            NamedKey::Enter,
            NamedKey::Home,
        ] {
            assert!(
                shell.game_list_key(&plain(key), false).is_none(),
                "{key:?} must not reach the list"
            );
        }
        drop(shell.on_key(plain(NamedKey::PageDown), false, None));
        assert_eq!(first(&shell), 1, "the list did not scroll");
        // And the same with the key captured by the widget.
        shell.ui.games.search_focused = false;
        assert!(
            shell
                .game_list_key(&plain(NamedKey::PageDown), true)
                .is_none()
        );
        assert!(shell.game_list_key(&plain(NamedKey::Enter), true).is_none());
    }

    #[test]
    fn shift_tab_does_not_change_the_database_while_typing_in_the_search_field() {
        let dir = tempfile::tempdir().expect("dir");
        let mut shell = shell(dir.path());
        let current = shell.app.bases.current();
        let shift_tab = KeyInput {
            key: Key::Named(NamedKey::Tab),
            modifiers: Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
        };
        shell.ui.games.search_focused = true;
        drop(shell.on_key(shift_tab, false, None));
        assert_eq!(shell.app.bases.current(), current);
        drop(shell.on_key(shift_tab, true, None));
        assert_eq!(shell.app.bases.current(), current, "captured: same");
        shell.ui.games.search_focused = false;
        drop(shell.on_key(shift_tab, false, None));
        assert_ne!(
            shell.app.bases.current(),
            current,
            "free: Shift+Tab goes back"
        );
        assert_eq!(
            shell.app.bases.current(),
            SlotId::from_number(1).expect("slot")
        );
    }

    #[test]
    fn page_keys_scroll_when_the_search_field_is_not_focused() {
        let dir = tempfile::tempdir().expect("dir");
        let mut shell = shell(dir.path());
        drop(shell.on_key(plain(NamedKey::PageDown), false, None));
        assert!(first(&shell) > 1);
    }

    #[test]
    fn escape_leaves_the_search_field_and_the_focus_flag_follows_the_probe() {
        let dir = tempfile::tempdir().expect("dir");
        let mut shell = shell(dir.path());
        shell.ui.games.search_focused = true;
        drop(shell.on_key(plain(NamedKey::Escape), true, None));
        assert!(!shell.ui.games.search_focused);
        // The answer of the focus probe sets the flag.
        drop(shell.update(Message::Games(GameListMessage::SearchFocus(true))));
        assert!(shell.ui.games.search_focused);
        drop(shell.update(Message::Games(GameListMessage::SearchFocus(false))));
        assert!(!shell.ui.games.search_focused);
    }

    #[test]
    fn modifier_events_reach_the_list_and_change_how_a_click_selects() {
        let dir = tempfile::tempdir().expect("dir");
        let mut shell = shell(dir.path());
        let id = shell.app.bases.current();
        let click = |shell: &mut Shell, number| {
            drop(shell.update(Message::Games(GameListMessage::Click { slot: id, number })));
        };
        let selection = |shell: &Shell| -> Vec<u32> {
            shell
                .app
                .bases
                .slot(id)
                .expect("slot")
                .list
                .selection
                .iter()
                .copied()
                .collect()
        };
        click(&mut shell, 2);
        let mods = |command, shift| Message::Games(GameListMessage::Modifiers { command, shift });
        drop(shell.update(mods(true, false)));
        click(&mut shell, 5);
        assert_eq!(selection(&shell), [2, 5], "Ctrl+click adds");
        drop(shell.update(mods(false, true)));
        click(&mut shell, 4);
        assert_eq!(
            selection(&shell),
            [4, 5],
            "Shift+click: from the anchor (5) to 4"
        );
        drop(shell.update(mods(false, false)));
        click(&mut shell, 7);
        assert_eq!(selection(&shell), [7]);
    }

    /// The shell moves a remark of the application to its own state.
    fn noticed(shell: &Shell) -> bool {
        shell.app.notice.is_some() || shell.ui.status_notice.is_some()
    }

    fn chord(key: Key, command: bool, shift: bool) -> KeyInput {
        KeyInput {
            key,
            modifiers: Modifiers {
                command,
                shift,
                ..Modifiers::NONE
            },
        }
    }

    #[test]
    fn list_chords_do_nothing_in_a_text_field_or_with_the_panel_closed() {
        let dir = tempfile::tempdir().expect("dir");
        let mut shell = shell(dir.path());
        let id = shell.app.bases.current();
        drop(shell.update(Message::Games(GameListMessage::Click {
            slot: id,
            number: 5,
        })));
        let flag = chord(Key::Named(NamedKey::Delete), true, false);
        let remove = chord(Key::Named(NamedKey::Delete), false, true);
        let select_all = chord(Key::Char('a'), true, true);
        let count = |shell: &Shell| shell.app.bases.slot(id).expect("slot").filtered();
        let selected = |shell: &Shell| shell.app.bases.slot(id).expect("slot").list.selection.len();
        shell.app.notice = None;
        shell.ui.status_notice = None;
        // A text field of another panel has the key.
        for input in [flag, remove, select_all] {
            drop(shell.on_key(input, true, None));
        }
        assert_eq!((count(&shell), selected(&shell)), (40, 1));
        assert!(!noticed(&shell), "no flag code was reached");
        // The panel is closed.
        shell
            .ui
            .close_pane(crate::windows::layout::PaneKind::GameList);
        for input in [flag, remove, select_all] {
            drop(shell.on_key(input, false, None));
        }
        assert_eq!((count(&shell), selected(&shell)), (40, 1));
        assert!(!noticed(&shell));
        // Open and free: they act (a PGN cannot be flagged: the notice says
        // the flag code was reached).
        shell
            .ui
            .show_pane(crate::windows::layout::PaneKind::GameList);
        drop(shell.on_key(flag, false, None));
        assert!(noticed(&shell));
        drop(shell.on_key(remove, false, None));
        assert_eq!(count(&shell), 39);
        drop(shell.on_key(select_all, false, None));
        assert!(selected(&shell) > 0);
        // And with the search field focused they are text.
        shell.ui.games.search_focused = true;
        shell.app.notice = None;
        shell.ui.status_notice = None;
        drop(shell.on_key(flag, false, None));
        assert!(!noticed(&shell));
    }

    #[test]
    fn a_list_behind_another_tab_gets_no_keys_and_no_focus() {
        use crate::windows::layout::PaneKind;
        let dir = tempfile::tempdir().expect("dir");
        let mut shell = shell(dir.path());
        let id = shell.app.bases.current();
        drop(shell.update(Message::Games(GameListMessage::Click {
            slot: id,
            number: 5,
        })));
        shell.ui.games.search_focused = true;
        let list = shell
            .ui
            .panes
            .iter()
            .find(|(_, g)| g.contains(PaneKind::GameList))
            .map(|(p, _)| *p)
            .expect("list pane");
        let notation = shell
            .ui
            .panes
            .iter()
            .find(|(_, g)| g.contains(PaneKind::Notation))
            .map(|(p, _)| *p)
            .expect("notation pane");
        shell.ui.merge_panes(list, notation);
        shell.ui.select_tab(PaneKind::Notation);
        assert!(shell.ui.is_open(PaneKind::GameList));
        assert!(!shell.ui.is_visible(PaneKind::GameList));
        assert!(!shell.ui.games.search_focused, "hiding lets go of focus");
        // Ctrl+F style focus request: nothing is focused.
        drop(shell.focus_search());
        assert!(!shell.ui.games.search_focused);
        assert!(!shell.list_panel_ready());
        // Ctrl+Del does not flag.
        shell.ui.games.hovered = true;
        shell.app.notice = None;
        shell.ui.status_notice = None;
        let flag = chord(Key::Named(NamedKey::Delete), true, false);
        drop(shell.on_key(flag, false, None));
        assert!(!noticed(&shell), "the hidden list did not act");
        assert!(shell.game_list_key(&flag, false).is_none());
    }

    #[test]
    fn closing_the_panel_lets_go_of_the_search_focus() {
        let dir = tempfile::tempdir().expect("dir");
        let mut shell = shell(dir.path());
        shell.ui.games.search_focused = true;
        shell.ui.games.menu = Some(crate::windows::game_list::ListMenu {
            target: crate::windows::game_list::MenuTarget::Columns,
            position: iced::Point::ORIGIN,
        });
        shell
            .ui
            .toggle_pane(crate::windows::layout::PaneKind::GameList);
        assert!(!shell.ui.is_open(crate::windows::layout::PaneKind::GameList));
        assert!(!shell.ui.games.search_focused);
        assert!(shell.ui.games.menu.is_none());
        // The arrow keys belong to the game again, not to a dead field.
        shell.ui.games.search_focused = true;
        shell.ui.reset_layout();
        assert!(!shell.ui.games.search_focused);
    }

    #[test]
    fn the_keys_of_the_list_do_not_act_behind_a_layer() {
        let mut shell = Shell::new(None);
        shell
            .ui
            .toggle_pane(crate::windows::layout::PaneKind::GameList);
        shell.ui.games.hovered = true;
        let down = KeyInput {
            key: Key::Named(NamedKey::Down),
            modifiers: Modifiers::NONE,
        };
        assert!(
            shell.game_list_key(&down, false).is_some(),
            "no layer: acts"
        );
        shell.ui.show_about = true;
        assert!(shell.game_list_key(&down, false).is_none(), "About open");
    }
}
