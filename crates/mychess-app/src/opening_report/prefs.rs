//! The `[opening_report]` table of the preferences (spec M7-2, section 3,
//! decisions D3603 and D3624).

use std::path::PathBuf;

use mychess_db::report::{
    MAX_COUNT_OPTION, MAX_GAMES_LIMIT, MAX_TABLE_GAMES_LIMIT, ReportOptions, SectionSet,
};
use serde::{Serialize, Serializer};

use crate::panel_calc::named_enum;

named_enum! {
    /// Which part of the report is written (`reportType` of the Scid).
    pub enum ReportPart {
        /// The whole report, with the theory table when it is on.
        #[default]
        Full => "full",
        /// The report without the theory table.
        Compact => "compact",
        /// Only the theory table.
        TableOnly => "table",
    }
}

named_enum! {
    /// The format of a saved report (`reportFormat` of the Scid).
    pub enum ReportFormat {
        /// Plain text, `.txt`.
        Text => "text",
        /// An HTML document, `.html` (the default of the Scid).
        #[default]
        Html => "html",
        /// A LaTeX document, `.tex`.
        Latex => "latex",
    }
}

impl Serialize for ReportPart {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl Serialize for ReportFormat {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl ReportPart {
    pub(crate) fn engine(self) -> mychess_db::report::ReportPart {
        match self {
            ReportPart::Full => mychess_db::report::ReportPart::Full,
            ReportPart::Compact => mychess_db::report::ReportPart::Compact,
            ReportPart::TableOnly => mychess_db::report::ReportPart::TableOnly,
        }
    }
}

impl ReportFormat {
    /// The extension the format adds to a file name, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            ReportFormat::Text => "txt",
            ReportFormat::Html => "html",
            ReportFormat::Latex => "tex",
        }
    }

    /// Whether `ext` (without the dot, in any case) is an extension the
    /// format accepts: `.txt`; `.html` and `.htm`; `.tex` and `.ltx`.
    pub fn accepts_extension(self, ext: &str) -> bool {
        let accepted: &[&str] = match self {
            ReportFormat::Text => &["txt"],
            ReportFormat::Html => &["html", "htm"],
            ReportFormat::Latex => &["tex", "ltx"],
        };
        accepted.iter().any(|a| a.eq_ignore_ascii_case(ext))
    }
}

/// Smallest and largest size of the board of the window, in logical pixels.
pub const BOARD_SIZE_RANGE: std::ops::RangeInclusive<u8> = 30..=60;

/// The size the board of the window starts with (`optable.tcl`: 40).
pub const DEFAULT_BOARD_SIZE: u8 = 40;

/// The options of the report and what the window remembers, in
/// `[opening_report]`. The defaults are those of the Scid
/// (`tcl/start.tcl` 796-818).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpeningReportPrefs {
    /// The statistics table (`Stats`).
    pub stats: bool,
    /// The popularity table (`Popular`).
    pub popular: bool,
    /// Average rating and performance (`AvgPerf`).
    pub avg_perf: bool,
    /// Lengths and frequencies of the results (`Results`).
    pub results: bool,
    /// The moves from the position (`MovesFrom`).
    pub moves_from: bool,
    /// The positional themes (`Themes`).
    pub themes: bool,
    /// The endgame classes (`Endgames`).
    pub endgames: bool,
    /// The oldest games to list, 0 to 10.
    pub oldest: u8,
    /// The newest games to list, 0 to 10.
    pub newest: u8,
    /// The most frequent players to list, 0 to 10.
    pub most_frequent: u8,
    /// The most frequent players with White.
    pub most_frequent_white: bool,
    /// The most frequent players with Black.
    pub most_frequent_black: bool,
    /// The highest-rated games to list, 0 to 10.
    pub high_rating: u8,
    /// The shortest wins to list, 0 to 10.
    pub shortest: u8,
    /// The shortest wins of White.
    pub shortest_white: bool,
    /// The shortest wins of Black.
    pub shortest_black: bool,
    /// The move orders to list, 0 to 10.
    pub move_orders: u8,
    /// The most games in the theory table, 0 to 10 000 (0 turns it off).
    pub max_table_games: u32,
    /// The most games kept for the statistics, 0 to 100 000.
    pub max_games: u32,
    /// Extra moves of the notes, 0 to 10.
    pub extra_moves: u8,
    /// The part the save dialog proposes.
    pub last_part: ReportPart,
    /// The format the save dialog proposes.
    pub last_format: ReportFormat,
    /// The folder of the last save.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_dir: Option<PathBuf>,
    /// The size of the board of the window, 30 to 60.
    pub board_size: u8,
}

impl Default for OpeningReportPrefs {
    fn default() -> OpeningReportPrefs {
        let o = ReportOptions::default();
        OpeningReportPrefs {
            stats: o.sections.contains(SectionSet::STATS),
            popular: o.sections.contains(SectionSet::POPULAR),
            avg_perf: o.sections.contains(SectionSet::AVG_PERF),
            results: o.sections.contains(SectionSet::RESULTS),
            moves_from: o.sections.contains(SectionSet::MOVES_FROM),
            themes: o.sections.contains(SectionSet::THEMES),
            endgames: o.sections.contains(SectionSet::ENDGAMES),
            oldest: o.oldest,
            newest: o.newest,
            most_frequent: o.most_frequent,
            most_frequent_white: o.most_frequent_white,
            most_frequent_black: o.most_frequent_black,
            high_rating: o.high_rating,
            shortest: o.shortest,
            shortest_white: o.shortest_white,
            shortest_black: o.shortest_black,
            move_orders: o.move_orders,
            max_table_games: o.max_table_games,
            max_games: o.max_games,
            extra_moves: o.extra_moves,
            last_part: ReportPart::Full,
            last_format: ReportFormat::Html,
            last_dir: None,
            board_size: DEFAULT_BOARD_SIZE,
        }
    }
}

impl OpeningReportPrefs {
    /// The value with every field out of its range back at the default
    /// (D3624: a wrong value never makes an error).
    #[must_use]
    pub fn sanitized(mut self) -> OpeningReportPrefs {
        let d = OpeningReportPrefs::default();
        let count = |v: u8, default: u8| if v <= MAX_COUNT_OPTION { v } else { default };
        self.oldest = count(self.oldest, d.oldest);
        self.newest = count(self.newest, d.newest);
        self.most_frequent = count(self.most_frequent, d.most_frequent);
        self.high_rating = count(self.high_rating, d.high_rating);
        self.shortest = count(self.shortest, d.shortest);
        self.move_orders = count(self.move_orders, d.move_orders);
        self.extra_moves = count(self.extra_moves, d.extra_moves);
        if self.max_table_games > MAX_TABLE_GAMES_LIMIT {
            self.max_table_games = d.max_table_games;
        }
        if self.max_games > MAX_GAMES_LIMIT {
            self.max_games = d.max_games;
        }
        if !BOARD_SIZE_RANGE.contains(&self.board_size) {
            self.board_size = d.board_size;
        }
        if self
            .last_dir
            .as_deref()
            .is_some_and(|p| p.as_os_str().is_empty())
        {
            self.last_dir = None;
        }
        self
    }

    /// The options the engine takes (`exclude` is not a preference).
    pub fn options(&self) -> ReportOptions {
        let mut sections = SectionSet::empty();
        for (on, flag) in [
            (self.stats, SectionSet::STATS),
            (self.popular, SectionSet::POPULAR),
            (self.avg_perf, SectionSet::AVG_PERF),
            (self.results, SectionSet::RESULTS),
            (self.moves_from, SectionSet::MOVES_FROM),
            (self.themes, SectionSet::THEMES),
            (self.endgames, SectionSet::ENDGAMES),
        ] {
            sections.set(flag, on);
        }
        ReportOptions {
            sections,
            oldest: self.oldest,
            newest: self.newest,
            most_frequent: self.most_frequent,
            most_frequent_white: self.most_frequent_white,
            most_frequent_black: self.most_frequent_black,
            high_rating: self.high_rating,
            shortest: self.shortest,
            shortest_white: self.shortest_white,
            shortest_black: self.shortest_black,
            move_orders: self.move_orders,
            max_table_games: self.max_table_games,
            max_games: self.max_games,
            extra_moves: self.extra_moves,
            table_rows: 0,
            exclude: None,
        }
        .normalized()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::{PrefChange, Preferences, load_preferences, render_preferences};

    #[test]
    fn defaults_are_those_of_the_scid() {
        let p = OpeningReportPrefs::default();
        assert_eq!(p.options(), ReportOptions::default().normalized());
        assert_eq!(
            (p.last_part, p.last_format),
            (ReportPart::Full, ReportFormat::Html)
        );
        assert_eq!(p.board_size, 40);
        assert_eq!(*Preferences::default().opening_report, p);
    }

    #[test]
    fn extensions_are_accepted_in_any_case() {
        assert!(ReportFormat::Html.accepts_extension("HTM"));
        assert!(ReportFormat::Latex.accepts_extension("Ltx"));
        assert!(ReportFormat::Text.accepts_extension("TXT"));
        assert!(!ReportFormat::Text.accepts_extension("html"));
    }

    #[test]
    fn values_round_trip_through_the_file() {
        let prefs = Preferences {
            opening_report: Box::new(OpeningReportPrefs {
                stats: false,
                oldest: 0,
                newest: 10,
                most_frequent_white: false,
                max_table_games: 0,
                max_games: 100_000,
                last_part: ReportPart::TableOnly,
                last_format: ReportFormat::Latex,
                last_dir: Some(PathBuf::from("reports")),
                board_size: 60,
                ..OpeningReportPrefs::default()
            }),
            ..Preferences::default()
        };
        let text = render_preferences(&prefs);
        assert!(text.contains("[opening_report]"), "{text}");
        let loaded = load_preferences(&text);
        assert_eq!(*loaded.prefs.opening_report, *prefs.opening_report);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn invalid_values_go_back_to_the_default() {
        let loaded = load_preferences(
            "[opening_report]\noldest = 99\nmax_games = -4\nboard_size = 7\n\
             last_part = \"everything\"\nlast_format = 3\nstats = \"yes\"\nnewest = 3\n",
        );
        let p = &loaded.prefs.opening_report;
        let d = OpeningReportPrefs::default();
        assert_eq!(p.oldest, d.oldest);
        assert_eq!(p.max_games, d.max_games);
        assert_eq!(p.board_size, d.board_size);
        assert_eq!(p.last_part, d.last_part);
        assert_eq!(p.last_format, d.last_format);
        assert_eq!(p.stats, d.stats);
        // A good value next to the bad ones is kept.
        assert_eq!(p.newest, 3);
        assert!(!loaded.issues.is_empty());
    }

    #[test]
    fn a_change_is_sanitized_and_defaults_come_back() {
        let mut prefs = Preferences::default();
        PrefChange::OpeningReport(Box::new(OpeningReportPrefs {
            shortest: 200,
            max_table_games: 3,
            ..OpeningReportPrefs::default()
        }))
        .apply(&mut prefs);
        assert_eq!(prefs.opening_report.shortest, 5);
        assert_eq!(prefs.opening_report.max_table_games, 3);
        PrefChange::OpeningReport(Box::default()).apply(&mut prefs);
        assert_eq!(*prefs.opening_report, OpeningReportPrefs::default());
    }
}
