//! The `[start]` table of the preferences (spec M9-4).

use serde::Serialize;

use super::tips::TIP_COUNT;

/// What the start-up remembers, in `[start]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StartPrefs {
    /// The welcome was shown and closed; it is not shown again.
    pub welcome_done: bool,
    /// Show the tip of the day when the program starts (Scid: `startup(tip)`).
    pub show_tip: bool,
    /// The tip the next start shows (an index into the tips).
    pub next_tip: u32,
}

impl Default for StartPrefs {
    fn default() -> StartPrefs {
        StartPrefs {
            welcome_done: false,
            show_tip: true,
            next_tip: 0,
        }
    }
}

impl StartPrefs {
    /// The value with a tip number out of range back at the first tip.
    #[must_use]
    pub fn sanitized(mut self) -> StartPrefs {
        if self.next_tip >= TIP_COUNT {
            self.next_tip = 0;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tip_out_of_range_goes_back_to_the_first() {
        let bad = StartPrefs {
            next_tip: TIP_COUNT,
            ..StartPrefs::default()
        };
        assert_eq!(bad.sanitized().next_tip, 0);
        let ok = StartPrefs {
            next_tip: TIP_COUNT - 1,
            ..StartPrefs::default()
        };
        assert_eq!(ok.sanitized(), ok);
    }

    #[test]
    fn the_table_round_trips_through_the_preferences_file() {
        use crate::prefs::{Preferences, load_preferences, render_preferences};
        let prefs = Preferences {
            start: StartPrefs {
                welcome_done: true,
                show_tip: false,
                next_tip: 4,
            },
            ..Preferences::default()
        };
        let loaded = load_preferences(&render_preferences(&prefs));
        assert_eq!(loaded.prefs.start, prefs.start);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn wrong_values_in_the_file_keep_the_defaults_and_are_reported() {
        use crate::prefs::load_preferences;
        let loaded =
            load_preferences("[start]\nwelcome_done = 3\nshow_tip = \"x\"\nnext_tip = 999\n");
        assert_eq!(loaded.prefs.start, StartPrefs::default());
        assert!(!loaded.issues.is_empty());
    }
}
