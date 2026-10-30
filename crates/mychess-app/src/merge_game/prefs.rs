//! The `[merge]` table of the preferences (spec M7-6, D3868).

use serde::Serialize;

/// The "up to move" the dialog starts with: move 40, ply 80 (`end.tcl`).
pub const DEFAULT_END_PLY: u32 = 80;

/// The largest ply the preferences keep (the Scid's "all" is 1000).
pub const MAX_END_PLY: u32 = 1000;

/// What the merge remembers, in `[merge]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MergePrefs {
    /// The last "up to move" chosen, as a ply; `0` is "All".
    pub end_ply: u32,
    /// Ask before merging a batch of games.
    pub confirm_batch: bool,
}

impl Default for MergePrefs {
    fn default() -> MergePrefs {
        MergePrefs {
            end_ply: DEFAULT_END_PLY,
            confirm_batch: true,
        }
    }
}

impl MergePrefs {
    /// The value with an out-of-range ply back at the default.
    #[must_use]
    pub fn sanitized(mut self) -> MergePrefs {
        if self.end_ply > MAX_END_PLY {
            self.end_ply = DEFAULT_END_PLY;
        }
        self
    }

    /// The ply as the dialog uses it: `None` is "All".
    pub fn end_ply_option(self) -> Option<u32> {
        (self.end_ply != 0).then_some(self.end_ply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::{PrefChange, Preferences, load_preferences, render_preferences};

    #[test]
    fn defaults_follow_the_spec() {
        let prefs = MergePrefs::default();
        assert_eq!(prefs.end_ply, 80);
        assert!(prefs.confirm_batch);
        assert_eq!(*Preferences::default().merge, prefs);
    }

    #[test]
    fn values_round_trip_through_the_file() {
        for (end_ply, confirm_batch) in [(0, false), (10, true), (1000, false)] {
            let prefs = Preferences {
                merge: Box::new(MergePrefs {
                    end_ply,
                    confirm_batch,
                }),
                ..Preferences::default()
            };
            let text = render_preferences(&prefs);
            assert!(text.contains("[merge]"), "{text}");
            let loaded = load_preferences(&text);
            assert_eq!(*loaded.prefs.merge, *prefs.merge);
            assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
        }
    }

    #[test]
    fn invalid_values_go_back_to_the_default() {
        for text in [
            "[merge]\nend_ply = 5000\n",
            "[merge]\nend_ply = -3\n",
            "[merge]\nend_ply = \"x\"\n",
        ] {
            let loaded = load_preferences(text);
            assert_eq!(loaded.prefs.merge.end_ply, DEFAULT_END_PLY, "{text}");
            assert!(!loaded.issues.is_empty(), "{text}");
        }
    }

    #[test]
    fn a_change_is_sanitized() {
        let mut prefs = Preferences::default();
        PrefChange::Merge(MergePrefs {
            end_ply: 99_999,
            confirm_batch: false,
        })
        .apply(&mut prefs);
        assert_eq!(prefs.merge.end_ply, DEFAULT_END_PLY);
        assert!(!prefs.merge.confirm_batch);
    }

    #[test]
    fn zero_means_all() {
        let prefs = MergePrefs {
            end_ply: 0,
            ..MergePrefs::default()
        };
        assert_eq!(prefs.end_ply_option(), None);
        assert_eq!(MergePrefs::default().end_ply_option(), Some(80));
    }
}
