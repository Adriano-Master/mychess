//! Keyboard glue of the shell.
//!
//! The map between keys and actions lives in `mychess_app::Shortcuts` (spec
//! M2-2, D148-D154), customised by the user's preferences. This module only
//! translates what the GUI toolkit reports into the application's
//! [`KeyInput`] and labels bindings for the menus. `Command` is `Ctrl` on
//! Linux and Windows and `Cmd` on macOS (`Modifiers::command()`), never a
//! fixed `control()`.

use iced::keyboard::key::Named;
use iced::keyboard::{self, Key as IcedKey};
use mychess_app::{Binding, Key, KeyInput, Modifiers, NamedKey};

/// The platform whose conventions label the shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Linux and other Unix systems.
    Linux,
    /// Windows.
    Windows,
    /// macOS.
    MacOs,
}

impl Platform {
    /// The platform the program was built for.
    pub fn current() -> Platform {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }
}

/// The label shown in menus: `Ctrl+Shift+N` on Linux and Windows, `⇧⌘N` on
/// macOS.
pub fn binding_label(binding: &Binding, platform: Platform) -> String {
    let mods = binding.modifiers;
    let key = match binding.key {
        Key::Char(c) => c.to_uppercase().collect::<String>(),
        Key::Named(named) => named.to_string(),
    };
    match platform {
        Platform::MacOs => {
            let mut out = String::new();
            if mods.alt {
                out.push('⌥');
            }
            if mods.shift {
                out.push('⇧');
            }
            if mods.command {
                out.push('⌘');
            }
            out.push_str(&key);
            out
        }
        Platform::Linux | Platform::Windows => {
            let mut out = String::new();
            if mods.command {
                out.push_str("Ctrl+");
            }
            if mods.alt {
                out.push_str("Alt+");
            }
            if mods.shift {
                out.push_str("Shift+");
            }
            out.push_str(&key);
            out
        }
    }
}

/// A named key of the toolkit as the application's [`NamedKey`].
fn named_key(named: Named) -> Option<NamedKey> {
    Some(match named {
        Named::ArrowLeft => NamedKey::Left,
        Named::ArrowRight => NamedKey::Right,
        Named::ArrowUp => NamedKey::Up,
        Named::ArrowDown => NamedKey::Down,
        Named::Home => NamedKey::Home,
        Named::End => NamedKey::End,
        Named::Delete => NamedKey::Delete,
        Named::Backspace => NamedKey::Backspace,
        Named::Enter => NamedKey::Enter,
        Named::Space => NamedKey::Space,
        Named::Escape => NamedKey::Escape,
        Named::Tab => NamedKey::Tab,
        Named::PageUp => NamedKey::PageUp,
        Named::PageDown => NamedKey::PageDown,
        Named::F1 => NamedKey::F(1),
        Named::F2 => NamedKey::F(2),
        Named::F3 => NamedKey::F(3),
        Named::F4 => NamedKey::F(4),
        Named::F5 => NamedKey::F(5),
        Named::F6 => NamedKey::F(6),
        Named::F7 => NamedKey::F(7),
        Named::F8 => NamedKey::F(8),
        Named::F9 => NamedKey::F(9),
        Named::F10 => NamedKey::F(10),
        Named::F11 => NamedKey::F(11),
        Named::F12 => NamedKey::F(12),
        _ => return None,
    })
}

/// Converts a key press of the toolkit into the application's [`KeyInput`].
/// Keys the shortcut map has no name for (media keys, dead keys, text of
/// several characters) give `None`.
pub fn key_input(key: &IcedKey, modifiers: keyboard::Modifiers) -> Option<KeyInput> {
    let key = match key {
        IcedKey::Named(named) => Key::Named(named_key(*named)?),
        IcedKey::Character(text) => {
            let mut chars = text.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                return None;
            };
            if c == ' ' {
                Key::Named(NamedKey::Space)
            } else {
                // Letters in lower case; the application normalizes the rest.
                Key::Char(c.to_lowercase().next().unwrap_or(c))
            }
        }
        IcedKey::Unidentified => return None,
    };
    Some(KeyInput {
        key,
        modifiers: Modifiers {
            command: modifiers.command(),
            shift: modifiers.shift(),
            alt: modifiers.alt(),
        },
    })
}

#[cfg(test)]
mod tests {
    use mychess_app::{Action, Shortcuts};

    use super::*;

    fn character(c: &str) -> IcedKey {
        IcedKey::Character(c.into())
    }

    fn lookup(key: &IcedKey, modifiers: keyboard::Modifiers, text_focused: bool) -> Option<Action> {
        let input = key_input(key, modifiers)?;
        Shortcuts::defaults().lookup_in(&input, text_focused)
    }

    #[test]
    fn labels_follow_the_platform() {
        let flip = Binding::parse("Command+F").expect("binding");
        assert_eq!(binding_label(&flip, Platform::Linux), "Ctrl+F");
        assert_eq!(binding_label(&flip, Platform::Windows), "Ctrl+F");
        assert_eq!(binding_label(&flip, Platform::MacOs), "⌘F");
        let new = Binding::parse("Command+Shift+N").expect("binding");
        assert_eq!(binding_label(&new, Platform::Linux), "Ctrl+Shift+N");
        assert_eq!(binding_label(&new, Platform::MacOs), "⇧⌘N");
        let left = Binding::parse("Left").expect("binding");
        assert_eq!(binding_label(&left, Platform::Linux), "Left");
    }

    #[test]
    fn letters_are_lowered_and_command_comes_from_the_toolkit() {
        let input = key_input(&character("F"), keyboard::Modifiers::COMMAND).expect("input");
        assert_eq!(input.key, Key::Char('f'));
        assert_eq!(input.modifiers, Modifiers::COMMAND);
        assert_eq!(
            lookup(&character("f"), keyboard::Modifiers::COMMAND, false),
            Some(Action::FlipBoard)
        );
    }

    #[test]
    fn shifted_letters_match_shift_bindings() {
        let mods = keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT;
        assert_eq!(lookup(&character("N"), mods, false), Some(Action::NewGame));
    }

    #[test]
    fn named_keys_and_function_keys_are_converted() {
        let none = keyboard::Modifiers::empty();
        assert_eq!(
            lookup(&IcedKey::Named(Named::ArrowLeft), none, false),
            Some(Action::Back)
        );
        assert_eq!(
            lookup(&IcedKey::Named(Named::ArrowDown), none, false),
            Some(Action::ForwardFast)
        );
        assert_eq!(
            lookup(&IcedKey::Named(Named::F11), none, false),
            Some(Action::FullScreen)
        );
        assert_eq!(
            lookup(&IcedKey::Named(Named::Delete), none, false),
            Some(Action::TruncateGame)
        );
        assert_eq!(
            lookup(
                &IcedKey::Named(Named::Home),
                keyboard::Modifiers::COMMAND,
                false
            ),
            Some(Action::FirstGame)
        );
    }

    #[test]
    fn plain_keys_are_skipped_while_a_text_field_has_the_focus() {
        let none = keyboard::Modifiers::empty();
        assert_eq!(lookup(&IcedKey::Named(Named::ArrowLeft), none, true), None);
        assert_eq!(lookup(&character("v"), none, true), None);
        assert_eq!(
            lookup(&character("v"), none, false),
            Some(Action::EnterVariation)
        );
        // `Command` bindings outside the text-editing letters still work.
        assert_eq!(
            lookup(&character("s"), keyboard::Modifiers::COMMAND, true),
            Some(Action::Save)
        );
    }

    #[test]
    fn keys_without_a_name_and_multi_character_text_are_ignored() {
        let none = keyboard::Modifiers::empty();
        assert_eq!(key_input(&IcedKey::Unidentified, none), None);
        assert_eq!(key_input(&character("ab"), none), None);
        assert_eq!(key_input(&IcedKey::Named(Named::MediaPlay), none), None);
    }

    #[test]
    fn page_tab_and_backquote_keys_reach_the_application() {
        let none = keyboard::Modifiers::empty();
        let input = key_input(&IcedKey::Named(Named::PageDown), none).expect("input");
        assert_eq!(input.key, Key::Named(NamedKey::PageDown));
        let input = key_input(&IcedKey::Named(Named::PageUp), none).expect("input");
        assert_eq!(input.key, Key::Named(NamedKey::PageUp));
        assert_eq!(
            lookup(
                &IcedKey::Named(Named::PageDown),
                keyboard::Modifiers::COMMAND,
                false
            ),
            Some(Action::NextBase)
        );
        assert_eq!(
            lookup(
                &IcedKey::Named(Named::Tab),
                keyboard::Modifiers::COMMAND,
                false
            ),
            Some(Action::NextBase)
        );
        assert_eq!(
            lookup(&character("`"), keyboard::Modifiers::COMMAND, false),
            Some(Action::SelectBase(9))
        );
    }

    #[test]
    fn space_is_a_named_key() {
        let input = key_input(&character(" "), keyboard::Modifiers::empty()).expect("input");
        assert_eq!(input.key, Key::Named(NamedKey::Space));
    }
}
