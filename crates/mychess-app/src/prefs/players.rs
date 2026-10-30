//! Preferences of the player strips (spec M2-5, D239).

use serde::Serialize;

use crate::material::MaterialMode;

/// How the player strips above and below the board look.
///
/// The strips have three sizes (full, compact, hidden); the front-end picks
/// one from the available space (D237). `show_strips = false` hides them and
/// `compact_strips` asks for the compact size even when there is room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PlayersPrefs {
    /// Draw the strips at all.
    pub show_strips: bool,
    /// Draw the generic avatar (full strips only).
    pub show_avatars: bool,
    /// Draw the rating of each player.
    pub show_ratings: bool,
    /// Draw the clocks (when the game has `[%clk]`).
    pub show_clocks: bool,
    /// Prefer the compact (one line) strip.
    pub compact_strips: bool,
    /// What the strips show about the material.
    pub material: MaterialMode,
}

impl Default for PlayersPrefs {
    fn default() -> PlayersPrefs {
        PlayersPrefs {
            show_strips: true,
            show_avatars: true,
            show_ratings: true,
            show_clocks: true,
            compact_strips: false,
            material: MaterialMode::Captured,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::{PrefChange, Preferences, PrefsIssue, load_preferences, render_preferences};

    #[test]
    fn defaults_follow_the_spec() {
        let p = PlayersPrefs::default();
        assert!(p.show_strips && p.show_avatars && p.show_ratings && p.show_clocks);
        assert!(!p.compact_strips);
        assert_eq!(p.material, MaterialMode::Captured);
        assert_eq!(Preferences::default().players, p);
    }

    #[test]
    fn old_file_without_the_section_opens_with_defaults_and_no_issue() {
        let loaded = load_preferences("[board]\nanimation_ms = 100\n");
        assert_eq!(loaded.prefs.players, PlayersPrefs::default());
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn every_value_round_trips() {
        for material in [
            MaterialMode::Off,
            MaterialMode::Difference,
            MaterialMode::Captured,
        ] {
            let prefs = Preferences {
                players: PlayersPrefs {
                    show_strips: false,
                    show_avatars: false,
                    show_ratings: false,
                    show_clocks: false,
                    compact_strips: true,
                    material,
                },
                ..Preferences::default()
            };
            let text = render_preferences(&prefs);
            assert!(text.contains("[players]"), "{text}");
            let loaded = load_preferences(&text);
            assert_eq!(loaded.prefs.players, prefs.players, "{text}");
            assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
        }
    }

    #[test]
    fn bad_values_fall_back_one_by_one() {
        let loaded = load_preferences(
            "[players]\nshow_clocks = false\nmaterial = \"everything\"\nshow_avatars = 3\n",
        );
        let p = loaded.prefs.players;
        assert!(!p.show_clocks);
        assert_eq!(p.material, MaterialMode::Captured);
        assert!(p.show_avatars);
        let keys: Vec<_> = loaded
            .issues
            .iter()
            .map(|i| match i {
                PrefsIssue::InvalidValue { key } => key.as_str(),
                _ => "other",
            })
            .collect();
        assert_eq!(keys, ["players.show_avatars", "players.material"]);
    }

    #[test]
    fn section_of_the_wrong_type_is_reported() {
        let loaded = load_preferences("players = 5\n");
        assert_eq!(loaded.prefs.players, PlayersPrefs::default());
        assert_eq!(loaded.issues.len(), 1);
    }

    #[test]
    fn change_applies_and_is_not_debounced() {
        let mut prefs = Preferences::default();
        let new = PlayersPrefs {
            material: MaterialMode::Difference,
            ..PlayersPrefs::default()
        };
        PrefChange::Players(new).apply(&mut prefs);
        assert_eq!(prefs.players, new);
        assert!(!PrefChange::Players(new).is_debounced());
    }
}
