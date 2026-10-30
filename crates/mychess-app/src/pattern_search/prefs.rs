//! The `[pattern_search]` table of `preferences.toml` (spec M6-2, D3290 as
//! extended by slice c): the operation on the filter, the last scope and
//! the last preset. The pattern being edited is **not** saved here: the
//! user's patterns are for that.

use mychess_db::pattern::model::Scope;
use serde::Serialize;

use crate::advanced_search::SearchOp;

/// The largest move number of the scope.
pub const MAX_SCOPE_MOVE: u16 = 999;
/// The most plies `hold` may ask for.
pub const MAX_HOLD: u8 = 40;
/// The longest id of a preset kept in the preferences.
pub const MAX_PRESET_ID: usize = 40;

/// The options that outlive the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternSearchPrefs {
    /// Operation on the filter.
    pub op: SearchOp,
    /// First move number (0 includes the initial position).
    pub min_move: u16,
    /// Last move number.
    pub max_move: u16,
    /// Consecutive plies the pattern must hold (1 to 40).
    pub hold: u8,
    /// Only the final position.
    pub end_only: bool,
    /// Also try with the colours swapped.
    pub ignore_colors: bool,
    /// The id of the last preset picked (empty: none).
    pub last_preset: String,
}

impl Default for PatternSearchPrefs {
    fn default() -> PatternSearchPrefs {
        let scope = Scope::default();
        PatternSearchPrefs {
            op: SearchOp::Reset,
            min_move: scope.min_move,
            max_move: scope.max_move,
            hold: scope.hold,
            end_only: scope.end_only,
            ignore_colors: scope.ignore_colors,
            last_preset: String::new(),
        }
    }
}

/// Whether `id` can be an id of a preset (lowercase letters, digits and
/// `_`).
fn is_preset_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_PRESET_ID
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

impl PatternSearchPrefs {
    /// The preferences with every number inside its range.
    #[must_use]
    pub fn sanitized(mut self) -> PatternSearchPrefs {
        self.min_move = self.min_move.min(MAX_SCOPE_MOVE);
        self.max_move = self.max_move.min(MAX_SCOPE_MOVE);
        self.hold = self.hold.clamp(1, MAX_HOLD);
        if !is_preset_id(&self.last_preset) {
            self.last_preset.clear();
        }
        self
    }

    /// The scope the preferences describe.
    pub fn scope(&self) -> Scope {
        Scope {
            min_move: self.min_move,
            max_move: self.max_move,
            hold: self.hold,
            end_only: self.end_only,
            ignore_colors: self.ignore_colors,
        }
    }

    /// The preferences with this scope (clamped).
    #[must_use]
    pub fn with_scope(mut self, scope: &Scope) -> PatternSearchPrefs {
        self.min_move = scope.min_move;
        self.max_move = scope.max_move;
        self.hold = scope.hold;
        self.end_only = scope.end_only;
        self.ignore_colors = scope.ignore_colors;
        self.sanitized()
    }
}

#[derive(Serialize)]
struct Table<'a> {
    op: &'static str,
    min_move: u16,
    max_move: u16,
    hold: u8,
    end_only: bool,
    ignore_colors: bool,
    last_preset: &'a str,
}

impl Serialize for PatternSearchPrefs {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Table {
            op: self.op.name(),
            min_move: self.min_move,
            max_move: self.max_move,
            hold: self.hold,
            end_only: self.end_only,
            ignore_colors: self.ignore_colors,
            last_preset: &self.last_preset,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Preferences, load_preferences, render_preferences};

    #[test]
    fn the_file_round_trips() {
        let prefs = Preferences {
            pattern_search: Box::new(PatternSearchPrefs {
                op: SearchOp::Or,
                min_move: 0,
                max_move: 40,
                hold: 3,
                end_only: true,
                ignore_colors: true,
                last_preset: "mate_in".to_owned(),
            }),
            ..Preferences::default()
        };
        let text = render_preferences(&prefs);
        assert!(text.contains("[pattern_search]"), "{text}");
        let loaded = load_preferences(&text);
        assert_eq!(loaded.prefs.pattern_search, prefs.pattern_search);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn a_file_without_the_table_has_the_defaults() {
        let loaded = load_preferences("[board]\nanimation_ms = 100\n");
        assert_eq!(*loaded.prefs.pattern_search, PatternSearchPrefs::default());
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn bad_values_fall_back_one_by_one() {
        let loaded = load_preferences(
            "[pattern_search]\nop = \"and\"\nhold = 99\nmax_move = 5000\nend_only = 3\n\
             last_preset = \"../x\"\n",
        );
        let want = PatternSearchPrefs {
            op: SearchOp::And,
            hold: 40,
            ..PatternSearchPrefs::default()
        };
        assert_eq!(*loaded.prefs.pattern_search, want);
        assert_eq!(loaded.issues.len(), 4, "{:?}", loaded.issues);
        let loaded = load_preferences("pattern_search = 1\n");
        assert_eq!(*loaded.prefs.pattern_search, PatternSearchPrefs::default());
        assert_eq!(loaded.issues.len(), 1);
    }

    #[test]
    fn sanitizing_limits_the_numbers_and_the_id() {
        let p = PatternSearchPrefs {
            min_move: 5000,
            hold: 0,
            last_preset: "Bad Id".to_owned(),
            ..PatternSearchPrefs::default()
        }
        .sanitized();
        assert_eq!((p.min_move, p.hold), (999, 1));
        assert!(p.last_preset.is_empty());
        let scope = Scope {
            hold: 200,
            ..Scope::default()
        };
        assert_eq!(PatternSearchPrefs::default().with_scope(&scope).hold, 40);
    }
}
