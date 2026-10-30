//! Edit preferences (defaults of the Scid vs PC).

use std::time::Duration;

/// Preferences that affect game editing. Persistence (TOML keys) belongs to
/// the preferences item of M2, not to this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditSettings {
    /// Ask before replacing an existing move (Scid: `askToReplaceMoves`,
    /// default on).
    pub ask_to_replace_moves: bool,
    /// Moving forward over a branching point opens the variation chooser
    /// (Scid: `showVarPopup`, default off).
    pub show_var_popup: bool,
    /// Keyboard move entry.
    pub move_entry: MoveEntrySettings,
    /// Autoplay delay (Scid: 5 s).
    pub autoplay_delay: Duration,
    /// A NAG replaces the others of its category (move quality `$1`-`$6`,
    /// evaluation `$10`-`$21`). The Scid does not do this; it is a
    /// deliberate deviation, on by default.
    pub exclusive_nags: bool,
}

impl Default for EditSettings {
    fn default() -> EditSettings {
        EditSettings {
            ask_to_replace_moves: true,
            show_var_popup: false,
            move_entry: MoveEntrySettings::default(),
            autoplay_delay: Duration::from_secs(5),
            exclusive_nags: true,
        }
    }
}

/// Keyboard move entry preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveEntrySettings {
    /// Keyboard entry is active.
    pub enabled: bool,
    /// Play a move as soon as it is the only candidate.
    pub auto_expand: bool,
    /// Also accept coordinate moves (`e2e4`).
    pub coordinate: bool,
}

impl Default for MoveEntrySettings {
    fn default() -> MoveEntrySettings {
        MoveEntrySettings {
            enabled: true,
            auto_expand: false,
            coordinate: true,
        }
    }
}
