//! The keys and the wheel of the game browser (spec M3-4e, D599, D614).

use iced::mouse::ScrollDelta;
use mychess_app::{BrowseMessage, BrowseMode, GameStep, Key, KeyInput, NamedKey};

use super::{BIG_STEP, BrowserMessage};

/// What a key does in the browser, if anything.
///
/// Single game (Scid `::gbrowser::new`): Home, End, Left, Right (one ply),
/// Up, Down (ten), Ctrl/Cmd with Home, End, Up, Down for the first, last,
/// previous and next game of the filter, `f` to turn the board, Space for
/// the autoplay, Enter to load. Grid (Scid `browseGames`): Ctrl/Cmd with
/// the arrows, Home and End move every board, the plain keys move the
/// board under the pointer, `f` turns all, Enter loads the board under the
/// pointer, Ctrl/Cmd+Delete toggles its "deleted" mark.
pub fn key_message(mode: BrowseMode, focus: usize, input: &KeyInput) -> Option<BrowserMessage> {
    if input.modifiers.alt {
        return None;
    }
    let command = input.modifiers.command;
    let act = |m| Some(BrowserMessage::Act(m));
    let single = mode == BrowseMode::Single;
    // Single: the one board; grid: the board under the pointer (all, with
    // Ctrl/Cmd).
    let target = if single {
        Some(0)
    } else if command {
        None
    } else {
        Some(focus)
    };
    match input.key {
        Key::Named(NamedKey::Escape) => act(BrowseMessage::Close),
        Key::Named(NamedKey::Enter) => Some(BrowserMessage::Load {
            game: if single { 0 } else { focus },
            keep_open: Some(single && command),
        }),
        Key::Char('f') => act(BrowseMessage::Flip { game: None }),
        Key::Named(NamedKey::Space) if single => act(BrowseMessage::ToggleAutoplay),
        Key::Named(NamedKey::Home) if single && command => {
            act(BrowseMessage::GoToGame(GameStep::First))
        }
        Key::Named(NamedKey::End) if single && command => {
            act(BrowseMessage::GoToGame(GameStep::Last))
        }
        Key::Named(NamedKey::Up) if single && command => {
            act(BrowseMessage::GoToGame(GameStep::Previous))
        }
        Key::Named(NamedKey::Down) if single && command => {
            act(BrowseMessage::GoToGame(GameStep::Next))
        }
        Key::Named(NamedKey::Home) => act(BrowseMessage::GoToStart { game: target }),
        Key::Named(NamedKey::End) => act(BrowseMessage::GoToEnd { game: target }),
        Key::Named(NamedKey::Left) => act(BrowseMessage::Step {
            game: target,
            delta: -1,
        }),
        Key::Named(NamedKey::Right) => act(BrowseMessage::Step {
            game: target,
            delta: 1,
        }),
        Key::Named(NamedKey::Up) if single => act(BrowseMessage::Step {
            game: target,
            delta: -BIG_STEP,
        }),
        Key::Named(NamedKey::Down) if single => act(BrowseMessage::Step {
            game: target,
            delta: BIG_STEP,
        }),
        Key::Named(NamedKey::Delete | NamedKey::Backspace) if command && !single => {
            act(BrowseMessage::ToggleDeleted { game: focus })
        }
        Key::Char('+' | '=') if command && !single => Some(BrowserMessage::Resize(1)),
        Key::Char('-') if command && !single => Some(BrowserMessage::Resize(-1)),
        _ => None,
    }
}

/// The step of a turn of the wheel: one ply, forward when turned down
/// (the Scid's `Button-5`).
pub fn wheel_ply(delta: ScrollDelta) -> i32 {
    let (ScrollDelta::Lines { y, .. } | ScrollDelta::Pixels { y, .. }) = delta;
    if y < 0.0 {
        1
    } else if y > 0.0 {
        -1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use mychess_app::Modifiers;

    use super::*;

    fn key(key: Key, modifiers: Modifiers) -> KeyInput {
        KeyInput { key, modifiers }
    }

    fn acted(message: Option<BrowserMessage>) -> Option<BrowseMessage> {
        match message {
            Some(BrowserMessage::Act(m)) => Some(m),
            _ => None,
        }
    }

    #[test]
    fn single_keys_move_the_board_and_the_games() {
        let plain = |k| key(Key::Named(k), Modifiers::NONE);
        let ctrl = |k| key(Key::Named(k), Modifiers::COMMAND);
        let single = |input| acted(key_message(BrowseMode::Single, 0, &input));
        assert_eq!(
            single(plain(NamedKey::Right)),
            Some(BrowseMessage::Step {
                game: Some(0),
                delta: 1
            })
        );
        assert_eq!(
            single(plain(NamedKey::Up)),
            Some(BrowseMessage::Step {
                game: Some(0),
                delta: -10
            })
        );
        assert_eq!(
            single(plain(NamedKey::Home)),
            Some(BrowseMessage::GoToStart { game: Some(0) })
        );
        assert_eq!(
            single(ctrl(NamedKey::End)),
            Some(BrowseMessage::GoToGame(GameStep::Last))
        );
        assert_eq!(
            single(ctrl(NamedKey::Down)),
            Some(BrowseMessage::GoToGame(GameStep::Next))
        );
        assert_eq!(single(plain(NamedKey::Escape)), Some(BrowseMessage::Close));
    }

    #[test]
    fn grid_keys_move_all_boards_with_the_command_key_else_the_focused_one() {
        let grid = |input| acted(key_message(BrowseMode::Grid, 3, &input));
        assert_eq!(
            grid(key(Key::Named(NamedKey::Right), Modifiers::COMMAND)),
            Some(BrowseMessage::Step {
                game: None,
                delta: 1
            })
        );
        assert_eq!(
            grid(key(Key::Named(NamedKey::Left), Modifiers::NONE)),
            Some(BrowseMessage::Step {
                game: Some(3),
                delta: -1
            })
        );
        assert_eq!(
            grid(key(Key::Named(NamedKey::End), Modifiers::COMMAND)),
            Some(BrowseMessage::GoToEnd { game: None })
        );
        assert_eq!(
            grid(key(Key::Char('f'), Modifiers::NONE)),
            Some(BrowseMessage::Flip { game: None })
        );
        assert_eq!(
            grid(key(Key::Named(NamedKey::Delete), Modifiers::COMMAND)),
            Some(BrowseMessage::ToggleDeleted { game: 3 })
        );
        // Up and Down are for the single game only.
        assert_eq!(grid(key(Key::Named(NamedKey::Up), Modifiers::NONE)), None);
    }

    #[test]
    fn enter_loads_and_command_enter_keeps_the_single_browser_open() {
        let enter =
            |mode, modifiers| key_message(mode, 2, &key(Key::Named(NamedKey::Enter), modifiers));
        assert!(matches!(
            enter(BrowseMode::Single, Modifiers::NONE),
            Some(BrowserMessage::Load {
                game: 0,
                keep_open: Some(false)
            })
        ));
        assert!(matches!(
            enter(BrowseMode::Single, Modifiers::COMMAND),
            Some(BrowserMessage::Load {
                game: 0,
                keep_open: Some(true)
            })
        ));
        assert!(matches!(
            enter(BrowseMode::Grid, Modifiers::COMMAND),
            Some(BrowserMessage::Load {
                game: 2,
                keep_open: Some(false)
            })
        ));
    }
}
