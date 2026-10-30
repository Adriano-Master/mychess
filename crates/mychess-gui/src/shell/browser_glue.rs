//! What the shell does for the game browser (spec M3-4e): runs its
//! messages, handles its keys and keeps the move text in view.

use iced::Task;
use iced::widget::scrollable::RelativeOffset;
use mychess_app::{BasesMessage, BrowseMessage, BrowseMode, Key, KeyInput, NamedKey};

use super::{Message, Shell};
use crate::widgets::modal;
use crate::windows::browser::{BROWSER_BOX_ID, BROWSER_MOVES_ID, BrowserMessage, key_message};

fn browse(message: BrowseMessage) -> mychess_app::Message {
    mychess_app::Message::Bases(BasesMessage::Browse(message))
}

impl Shell {
    /// Handles a message of the browser's widgets.
    pub(super) fn update_browser(&mut self, message: BrowserMessage) -> Task<Message> {
        match message {
            BrowserMessage::Act(message) => {
                let moves = matches!(
                    message,
                    BrowseMessage::Step { .. }
                        | BrowseMessage::GoToStart { .. }
                        | BrowseMessage::GoToEnd { .. }
                        | BrowseMessage::GoToPly { .. }
                );
                let task = self.dispatch(browse(message));
                if moves {
                    Task::batch([task, self.reveal_current_move()])
                } else {
                    task
                }
            }
            BrowserMessage::Load { game, keep_open } => {
                let keep_open = keep_open.unwrap_or(self.ui.games.command);
                // The main board is turned like the browser's only when the
                // game is in the editor (the application sets
                // `board_flipped` then): a cancelled question leaves it alone.
                self.dispatch(browse(BrowseMessage::LoadIntoBoard { game, keep_open }))
            }
            BrowserMessage::Wheel { game, delta } => {
                let steps = self.ui.browser.wheel_steps(delta);
                let single = self.app.browser.as_ref().map(|b| b.mode) == Some(BrowseMode::Single);
                if steps == 0 {
                    return Task::none();
                }
                if self.ui.games.command {
                    // Ctrl/Cmd+wheel: the size of the boards of the grid.
                    if !single {
                        self.ui.browser.resize(-steps);
                    }
                    return Task::none();
                }
                let game = if single { 0 } else { game };
                self.update_browser(BrowserMessage::Act(BrowseMessage::Step {
                    game: Some(game),
                    delta: steps,
                }))
            }
            BrowserMessage::Resize(steps) => {
                self.ui.browser.resize(steps);
                Task::none()
            }
            BrowserMessage::Noop => Task::none(),
        }
    }

    /// A key while the browser is open. `None`: not a key of the browser
    /// (shortcuts with Ctrl/Cmd still reach the shortcut map; plain keys
    /// are spent here so nothing under the layer reacts).
    pub(super) fn browser_key(
        &mut self,
        input: &KeyInput,
        text_focused: bool,
    ) -> Option<Task<Message>> {
        // The search field of the list is not part of the layer: it was
        // let go when the browser opened (D613); a stale flag must not eat
        // the keys either.
        self.ui.games.search_focused = false;
        if text_focused {
            return None;
        }
        let browser = self.app.browser.as_ref()?;
        if input.key == Key::Named(NamedKey::Tab) {
            // In the grid Tab walks the boards (Alt is not allowed for
            // shortcuts, D614); in the single game it cycles the buttons.
            if browser.mode == BrowseMode::Grid {
                let count = browser.games.len();
                let next = if input.modifiers.shift {
                    (browser.focus + count - 1) % count.max(1)
                } else {
                    (browser.focus + 1) % count.max(1)
                };
                return Some(self.update_browser(BrowserMessage::Act(BrowseMessage::Focus(next))));
            }
            return Some(modal::cycle_focus(BROWSER_BOX_ID, input.modifiers.shift));
        }
        match key_message(browser.mode, browser.focus, input) {
            Some(message) => Some(self.update_browser(message)),
            None if input.modifiers.command => None,
            None => Some(Task::none()),
        }
    }

    /// Scrolls the move text to where the move shown is, estimated by its
    /// share of the plies (the text is one wrapping paragraph, so the
    /// share of the height is close to it).
    pub(super) fn reveal_current_move(&self) -> Task<Message> {
        let Some(ready) = self
            .app
            .browser
            .as_ref()
            .filter(|b| b.mode == BrowseMode::Single)
            .and_then(|b| b.games.first())
            .and_then(|g| g.ready())
        else {
            return Task::none();
        };
        let share = ratio(ready.ply(), ready.max_ply());
        iced::widget::operation::snap_to(
            BROWSER_MOVES_ID,
            RelativeOffset {
                x: None,
                y: Some(share),
            },
        )
    }
}

fn ratio(ply: usize, max: usize) -> f32 {
    if max == 0 {
        return 0.0;
    }
    // Two small counts: the precision of an f32 is plenty.
    #[allow(clippy::cast_precision_loss)]
    let ratio = (ply as f32 / max as f32).clamp(0.0, 1.0);
    ratio
}

#[cfg(test)]
#[allow(clippy::format_push_string)]
mod tests {
    use std::path::Path;

    use iced::{Settings, Size};
    use iced_test::Simulator;
    use mychess_app::{Effect, Modifiers, update};

    use super::*;
    use crate::i18n::{I18n, Language};

    fn pgn(dir: &Path, count: u32) -> std::path::PathBuf {
        let mut text = String::new();
        for n in 1..=count {
            text.push_str(&format!(
                "[White \"W{n}\"]\n[Black \"B{n}\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 Nc6 1-0\n\n"
            ));
        }
        let path = dir.join("g.pgn");
        std::fs::write(&path, text).expect("write");
        path
    }

    /// Runs the effects an application message leads to, as the shell
    /// would (the timers are left out).
    fn settle(shell: &mut Shell, message: mychess_app::Message) {
        let mut queue = vec![message];
        while let Some(message) = queue.pop() {
            for effect in update(&mut shell.app, message) {
                if !effect.runs_on_host() && !matches!(effect, Effect::BrowserTick { .. }) {
                    queue.push(crate::test_support::run_effect(effect));
                }
            }
        }
        shell.sync();
    }

    fn shell(dir: &Path, numbers: Vec<u32>) -> Shell {
        let mut shell = Shell::new(None);
        shell.i18n = I18n::new(Language::EnUs);
        shell.app.open_pgn_in_slots = true;
        settle(
            &mut shell,
            mychess_app::Message::Bases(BasesMessage::Open(pgn(dir, 8))),
        );
        let slot = shell.app.bases.current();
        settle(&mut shell, browse(BrowseMessage::Open { slot, numbers }));
        shell
    }

    fn key(key: NamedKey, modifiers: Modifiers) -> KeyInput {
        KeyInput {
            key: Key::Named(key),
            modifiers,
        }
    }

    fn ply(shell: &Shell, index: usize) -> usize {
        shell.app.browser.as_ref().expect("open").games[index]
            .ready()
            .expect("read")
            .ply()
    }

    #[test]
    fn the_arrow_keys_move_the_single_board_and_escape_closes() {
        let dir = tempfile::tempdir().expect("dir");
        let mut shell = shell(dir.path(), vec![3]);
        assert_eq!(ply(&shell, 0), 0);
        drop(shell.on_key(key(NamedKey::Right, Modifiers::NONE), false, None));
        drop(shell.on_key(key(NamedKey::Right, Modifiers::NONE), false, None));
        assert_eq!(ply(&shell, 0), 2);
        drop(shell.on_key(key(NamedKey::End, Modifiers::NONE), false, None));
        assert_eq!(ply(&shell, 0), 4);
        drop(shell.on_key(key(NamedKey::Home, Modifiers::NONE), false, None));
        assert_eq!(ply(&shell, 0), 0);
        assert!(shell.app.modal_open());
        drop(shell.on_key(key(NamedKey::Escape, Modifiers::NONE), false, None));
        assert!(shell.app.browser.is_none());
    }

    #[test]
    fn command_arrows_move_every_board_of_the_grid() {
        let dir = tempfile::tempdir().expect("dir");
        let mut shell = shell(dir.path(), vec![1, 2, 3, 4]);
        assert_eq!(
            shell
                .app
                .browser
                .as_ref()
                .map(mychess_app::BrowserState::columns),
            Some(4)
        );
        drop(shell.on_key(key(NamedKey::Left, Modifiers::COMMAND), false, None));
        assert_eq!(
            (0..4).map(|i| ply(&shell, i)).collect::<Vec<_>>(),
            vec![3; 4]
        );
        drop(shell.on_key(key(NamedKey::Home, Modifiers::COMMAND), false, None));
        assert_eq!(
            (0..4).map(|i| ply(&shell, i)).collect::<Vec<_>>(),
            vec![0; 4]
        );
        drop(shell.on_key(key(NamedKey::Right, Modifiers::NONE), false, None));
        // Without the command key only the board under the pointer moves.
        assert_eq!(
            (0..4).map(|i| ply(&shell, i)).collect::<Vec<_>>(),
            vec![1, 0, 0, 0]
        );
    }

    #[test]
    fn the_views_show_the_title_and_the_buttons_in_both_modes() {
        let dir = tempfile::tempdir().expect("dir");
        for (numbers, title) in [
            (vec![3], "Game 3 of g.pgn"),
            (vec![1, 2, 3], "3 games of g.pgn"),
        ] {
            let shell = shell(dir.path(), numbers);
            let view = crate::windows::browser::view(&shell).expect("open");
            let mut sim = Simulator::with_size(Settings::default(), Size::new(1280.0, 800.0), view);
            assert!(sim.find(title).is_ok(), "{title}");
            assert!(sim.find("Close").is_ok());
        }
    }

    #[test]
    fn a_focused_search_field_does_not_eat_the_keys_of_the_browser() {
        let dir = tempfile::tempdir().expect("dir");
        let mut shell = shell(dir.path(), vec![3]);
        shell.ui.games.search_focused = true;
        drop(shell.on_key(key(NamedKey::Right, Modifiers::NONE), false, None));
        assert_eq!(ply(&shell, 0), 1);
        assert!(!shell.ui.games.search_focused);
        // Any message lets go of the field while the browser is open.
        shell.ui.games.search_focused = true;
        drop(shell.update(Message::Browser(BrowserMessage::Noop)));
        assert!(!shell.ui.games.search_focused);
    }

    #[test]
    fn tab_walks_the_boards_of_the_grid() {
        let dir = tempfile::tempdir().expect("dir");
        let mut shell = shell(dir.path(), vec![1, 2, 3]);
        let focus = |s: &Shell| s.app.browser.as_ref().map(|b| b.focus);
        drop(shell.on_key(key(NamedKey::Tab, Modifiers::NONE), false, None));
        assert_eq!(focus(&shell), Some(1));
        drop(shell.on_key(key(NamedKey::Tab, Modifiers::NONE), false, None));
        drop(shell.on_key(key(NamedKey::Tab, Modifiers::NONE), false, None));
        assert_eq!(focus(&shell), Some(0), "wraps around");
        let shift = Modifiers {
            shift: true,
            ..Modifiers::NONE
        };
        drop(shell.on_key(key(NamedKey::Tab, shift), false, None));
        assert_eq!(focus(&shell), Some(2));
    }

    #[test]
    fn a_scroll_on_the_single_game_steps_a_ply() {
        let dir = tempfile::tempdir().expect("dir");
        let mut shell = shell(dir.path(), vec![3]);
        let down = iced::mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 };
        drop(shell.update(Message::Browser(BrowserMessage::Wheel {
            game: 0,
            delta: down,
        })));
        assert_eq!(ply(&shell, 0), 1);
        // Smooth scrolling needs 40 pixels per ply.
        let small = iced::mouse::ScrollDelta::Pixels { x: 0.0, y: -25.0 };
        for _ in 0..2 {
            drop(shell.update(Message::Browser(BrowserMessage::Wheel {
                game: 0,
                delta: small,
            })));
        }
        assert_eq!(ply(&shell, 0), 2);
    }

    #[test]
    fn a_load_that_lands_turns_the_main_board_like_the_browser() {
        let dir = tempfile::tempdir().expect("dir");
        let mut shell = shell(dir.path(), vec![2]);
        drop(shell.on_key(
            KeyInput {
                key: Key::Char('f'),
                modifiers: Modifiers::NONE,
            },
            false,
            None,
        ));
        let before = shell.ui.orientation;
        settle(
            &mut shell,
            browse(BrowseMessage::LoadIntoBoard {
                game: 0,
                keep_open: false,
            }),
        );
        assert_ne!(shell.ui.orientation, before);
    }

    #[test]
    fn b_opens_the_row_under_the_pointer_before_the_selected_one_or_the_current_game() {
        use crate::windows::game_list::{GameListMessage, ListKey};
        let dir = tempfile::tempdir().expect("dir");
        let mut shell = shell(dir.path(), vec![1]);
        drop(shell.on_key(key(NamedKey::Escape, Modifiers::NONE), false, None));
        let slot = shell.app.bases.current();
        settle(
            &mut shell,
            mychess_app::Message::Bases(BasesMessage::LoadGame { slot, number: 2 }),
        );
        shell.ui.games.hover_row = Some(5);
        let sent = shell
            .ui
            .games
            .update(GameListMessage::Key(ListKey::Browse), &shell.app);
        assert!(matches!(
            sent.as_slice(),
            [mychess_app::Message::Bases(BasesMessage::Browse(BrowseMessage::Open { numbers, .. }))]
                if numbers == &[5]
        ));
        shell.ui.games.hover_row = None;
        let sent = shell
            .ui
            .games
            .update(GameListMessage::Key(ListKey::Browse), &shell.app);
        assert!(matches!(
            sent.as_slice(),
            [mychess_app::Message::Bases(BasesMessage::Browse(BrowseMessage::Open { numbers, .. }))]
                if numbers == &[2]
        ));
    }

    #[test]
    fn no_browser_means_no_layer() {
        let dir = tempfile::tempdir().expect("dir");
        let mut shell = shell(dir.path(), vec![1]);
        drop(shell.on_key(key(NamedKey::Escape, Modifiers::NONE), false, None));
        assert!(crate::windows::browser::view(&shell).is_none());
    }
}
