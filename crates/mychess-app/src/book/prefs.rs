//! The `[book]` preferences (spec M5-6, D1662).

use std::path::PathBuf;

use serde::Serialize;

/// The book of the window and of the annotation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BookPrefs {
    /// The opening book used by the annotation and the report. A file that
    /// is missing or not a book leaves the book unloaded but is kept here.
    #[serde(skip_serializing_if = "path_is_unsavable")]
    pub opening_book: Option<PathBuf>,
}

#[allow(clippy::ref_option)] // the signature serde's `skip_serializing_if` wants
fn path_is_unsavable(path: &Option<PathBuf>) -> bool {
    path.as_ref().is_none_or(|p| p.to_str().is_none())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::{PrefChange, Preferences, PrefsIssue, load_preferences, render_preferences};

    #[test]
    fn the_default_is_no_book_and_an_old_file_is_valid() {
        assert_eq!(Preferences::default().book, Box::<BookPrefs>::default());
        let loaded = load_preferences("[board]\nanimation_ms = 100\n");
        assert_eq!(loaded.prefs.book, Box::<BookPrefs>::default());
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn the_path_round_trips_through_the_file() {
        let prefs = Preferences {
            book: Box::new(BookPrefs {
                opening_book: Some(PathBuf::from("books").join("main.bin")),
            }),
            ..Preferences::default()
        };
        let text = render_preferences(&prefs);
        assert!(text.contains("[book]"), "{text}");
        let loaded = load_preferences(&text);
        assert_eq!(loaded.prefs.book, prefs.book);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn a_wrong_value_is_dropped_with_a_remark_and_an_empty_one_is_no_book() {
        let loaded = load_preferences("[book]\nopening_book = 42\n");
        assert_eq!(loaded.prefs.book.opening_book, None);
        assert!(
            loaded.issues.contains(&PrefsIssue::InvalidValue {
                key: "book.opening_book".to_owned()
            }),
            "{:?}",
            loaded.issues
        );
        let loaded = load_preferences("[book]\nopening_book = \"\"\n");
        assert_eq!(loaded.prefs.book.opening_book, None);
        let loaded = load_preferences("book = 3\n");
        assert_eq!(loaded.prefs.book, Box::<BookPrefs>::default());
        assert!(!loaded.issues.is_empty());
    }

    #[test]
    fn the_change_replaces_the_preference() {
        let mut prefs = Preferences::default();
        PrefChange::Book(BookPrefs {
            opening_book: Some(PathBuf::from("a.bin")),
        })
        .apply(&mut prefs);
        assert_eq!(prefs.book.opening_book, Some(PathBuf::from("a.bin")));
    }
}
