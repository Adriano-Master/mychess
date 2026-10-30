//! The graph preferences in the `[graph]` table of `preferences.toml`
//! (spec M4-7, D871).
//!
//! The file keeps the spelling of the kinds (`auto`, `score`, `time`,
//! `score_combo`, `time_combo`) and flat booleans; an unknown spelling falls
//! back to the default in the reader (`prefs/read.rs`).

use serde::{Serialize, Serializer};

use super::series::Invert;
use super::view::{GraphKind, GraphPrefs};

impl GraphKind {
    /// Every kind, in menu order.
    pub const ALL: [GraphKind; 5] = [
        GraphKind::Auto,
        GraphKind::Score,
        GraphKind::Time,
        GraphKind::ScoreCombo,
        GraphKind::TimeCombo,
    ];

    /// The spelling used in the preferences file.
    pub fn name(self) -> &'static str {
        match self {
            GraphKind::Auto => "auto",
            GraphKind::Score => "score",
            GraphKind::Time => "time",
            GraphKind::ScoreCombo => "score_combo",
            GraphKind::TimeCombo => "time_combo",
        }
    }

    /// Reads the spelling used in the preferences file.
    pub fn from_name(text: &str) -> Option<GraphKind> {
        GraphKind::ALL.into_iter().find(|k| k.name() == text)
    }
}

/// The shape of the `[graph]` table.
#[derive(Serialize)]
struct Table {
    kind: &'static str,
    invert_white: bool,
    invert_black: bool,
    highlight_current: bool,
    style: &'static str,
}

impl Serialize for GraphPrefs {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Invert { white, black } = self.invert;
        Table {
            kind: self.kind.name(),
            invert_white: white,
            invert_black: black,
            highlight_current: self.highlight_current,
            style: self.style.name(),
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::super::line::GraphStyle;
    use super::*;

    #[test]
    fn kind_names_round_trip() {
        for kind in GraphKind::ALL {
            assert_eq!(GraphKind::from_name(kind.name()), Some(kind));
        }
        assert_eq!(GraphKind::from_name("Auto"), None);
        assert_eq!(GraphKind::from_name(""), None);
    }

    #[test]
    fn serializes_as_a_flat_table() {
        let prefs = GraphPrefs {
            kind: GraphKind::TimeCombo,
            invert: Invert {
                white: true,
                black: false,
            },
            highlight_current: false,
            ..GraphPrefs::default()
        };
        let text = toml::to_string(&prefs).expect("serialize");
        assert!(text.contains("kind = \"time_combo\""));
        assert!(text.contains("invert_white = true"));
        assert!(text.contains("invert_black = false"));
        assert!(text.contains("highlight_current = false"));
    }

    fn custom() -> GraphPrefs {
        GraphPrefs {
            kind: GraphKind::ScoreCombo,
            invert: Invert {
                white: false,
                black: true,
            },
            highlight_current: false,
            style: GraphStyle::Bars,
        }
    }

    #[test]
    fn the_file_round_trips() {
        let prefs = crate::Preferences {
            graph: custom(),
            ..crate::Preferences::default()
        };
        let text = crate::render_preferences(&prefs);
        assert!(text.contains("[graph]"), "{text}");
        let loaded = crate::load_preferences(&text);
        assert_eq!(loaded.prefs.graph, custom());
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn an_old_file_without_the_table_gets_the_defaults() {
        let loaded = crate::load_preferences("version = 1\n[general]\ntheme = \"dark\"\n");
        assert_eq!(loaded.prefs.graph, GraphPrefs::default());
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn hostile_values_fall_back_one_field_at_a_time() {
        let text = "[graph]\nkind = \"pie\"\ninvert_white = 3\ninvert_black = true\n\
                    highlight_current = \"yes\"\n";
        let loaded = crate::load_preferences(text);
        let want = GraphPrefs {
            invert: Invert {
                white: false,
                black: true,
            },
            ..GraphPrefs::default()
        };
        assert_eq!(loaded.prefs.graph, want);
        assert_eq!(loaded.issues.len(), 3, "{:?}", loaded.issues);
        let loaded = crate::load_preferences("graph = 7\n");
        assert_eq!(loaded.prefs.graph, GraphPrefs::default());
        assert_eq!(loaded.issues.len(), 1);
    }
}
