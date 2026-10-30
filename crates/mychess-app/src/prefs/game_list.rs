//! Preferences of the game list (spec M3-4, slice 4c; D562): which columns
//! are shown, in what order, how wide and how aligned. Global, not per
//! database, like the Scid's `glistFields` setup.

use serde::Serialize;

use crate::bases::{ColumnSetup, ListColumn};

/// The columns of the game list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GameListPrefs {
    /// The columns shown, left to right. Never empty, no column twice.
    pub columns: Vec<ColumnSetup>,
}

/// The columns shown by default: the ten of the Scid's `glistFields` order
/// that fit a normal panel (D562, which settles D555).
pub const DEFAULT_COLUMNS: [ListColumn; 10] = [
    ListColumn::Number,
    ListColumn::White,
    ListColumn::Black,
    ListColumn::Result,
    ListColumn::Length,
    ListColumn::Date,
    ListColumn::Event,
    ListColumn::WhiteElo,
    ListColumn::BlackElo,
    ListColumn::Eco,
];

impl Default for GameListPrefs {
    fn default() -> GameListPrefs {
        GameListPrefs {
            columns: DEFAULT_COLUMNS.into_iter().map(ColumnSetup::new).collect(),
        }
    }
}

impl GameListPrefs {
    /// The preferences with widths limited, repeated columns dropped and at
    /// least one column left (the default ones if none is).
    #[must_use]
    pub fn sanitized(self) -> GameListPrefs {
        let mut seen = Vec::new();
        let mut columns = Vec::new();
        for setup in self.columns {
            if !seen.contains(&setup.column) {
                seen.push(setup.column);
                columns.push(setup.clamped());
            }
        }
        if columns.is_empty() {
            return GameListPrefs::default();
        }
        GameListPrefs { columns }
    }

    /// The setup of `column`, if it is shown.
    pub fn setup(&self, column: ListColumn) -> Option<&ColumnSetup> {
        self.columns.iter().find(|s| s.column == column)
    }

    /// Whether `column` is shown.
    pub fn shows(&self, column: ListColumn) -> bool {
        self.setup(column).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bases::{ColumnAlign, MAX_COLUMN_CHARS};
    use crate::prefs::{PrefChange, Preferences, PrefsIssue, load_preferences, render_preferences};

    #[test]
    fn the_default_follows_the_scid_order() {
        let prefs = GameListPrefs::default();
        let order: Vec<_> = prefs.columns.iter().map(|s| s.column).collect();
        assert_eq!(order, DEFAULT_COLUMNS);
        assert_eq!(prefs.columns[0].chars, 7);
        assert_eq!(*Preferences::default().game_list, prefs);
    }

    #[test]
    fn sanitizing_drops_repeats_clamps_and_never_empties() {
        let mut wide = ColumnSetup::new(ListColumn::Site);
        wide.chars = 999;
        let prefs = GameListPrefs {
            columns: vec![
                ColumnSetup::new(ListColumn::Site),
                wide,
                ColumnSetup::new(ListColumn::Round),
            ],
        }
        .sanitized();
        assert_eq!(prefs.columns.len(), 2);
        assert_eq!(prefs.columns[0].chars, ListColumn::Site.default_chars());
        let empty = GameListPrefs { columns: vec![] }.sanitized();
        assert_eq!(empty, GameListPrefs::default());
        let mut big = ColumnSetup::new(ListColumn::Site);
        big.chars = 999;
        let clamped = GameListPrefs { columns: vec![big] }.sanitized();
        assert_eq!(clamped.columns[0].chars, MAX_COLUMN_CHARS);
    }

    #[test]
    fn columns_survive_a_round_trip_through_the_file() {
        let mut prefs = Preferences::default();
        let mut site = ColumnSetup::new(ListColumn::Site);
        site.align = ColumnAlign::Center;
        site.chars = 22;
        PrefChange::GameList(GameListPrefs {
            columns: vec![ColumnSetup::new(ListColumn::Number), site],
        })
        .apply(&mut prefs);
        let text = render_preferences(&prefs);
        let loaded = load_preferences(&text);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
        assert_eq!(loaded.prefs.game_list, prefs.game_list);
        assert!(text.contains("site"), "{text}");
    }

    #[test]
    fn an_old_file_without_the_section_gets_the_default() {
        let loaded = load_preferences("[board]\nanimation_ms = 100\n");
        assert_eq!(*loaded.prefs.game_list, GameListPrefs::default());
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn bad_entries_are_dropped_with_a_remark_and_the_rest_is_kept() {
        let text = "[[game_list.columns]]\ncolumn = \"white\"\nalign = \"center\"\nchars = 9\n\n\
                    [[game_list.columns]]\ncolumn = \"nonsense\"\n\n\
                    [[game_list.columns]]\ncolumn = \"white\"\n\n\
                    [[game_list.columns]]\ncolumn = \"date\"\nchars = 500\n";
        let loaded = load_preferences(text);
        let columns = &loaded.prefs.game_list.columns;
        assert_eq!(columns.len(), 2, "{columns:?}");
        assert_eq!(columns[0].align, ColumnAlign::Center);
        assert_eq!(columns[0].chars, 9);
        assert_eq!(columns[1].column, ListColumn::Date);
        assert_eq!(columns[1].chars, MAX_COLUMN_CHARS);
        assert!(
            loaded.issues.iter().any(
                |i| matches!(i, PrefsIssue::InvalidValue { key } if key == "game_list.columns")
            ),
            "{:?}",
            loaded.issues
        );
    }

    #[test]
    fn a_file_with_no_valid_column_falls_back_to_the_default() {
        let loaded = load_preferences("[[game_list.columns]]\ncolumn = \"x\"\n");
        assert_eq!(*loaded.prefs.game_list, GameListPrefs::default());
    }
}
