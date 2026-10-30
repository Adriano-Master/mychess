//! The `[player_report]` table of the preferences (spec M7-3, section 4 and
//! decision M7-3/D3663). The history of the names is the one of
//! `[player_tools]` (shared with the player information).

use std::path::PathBuf;

use mychess_core::Color;
use mychess_db::report::{MAX_COUNT_OPTION, MAX_TABLE_GAMES_LIMIT, NameMatch, PlayerReportOptions};
use serde::{Serialize, Serializer};

use crate::panel_calc::named_enum;

use crate::opening_report::{ReportFormat, ReportPart};

named_enum! {
    /// The colour of the player (`White`/`Black` of the dialog).
    pub enum PlayerColor {
        /// With the White pieces.
        #[default]
        White => "white",
        /// With the Black pieces.
        Black => "black",
    }
}

named_enum! {
    /// Where the games of the report start ("Start position" of the dialog).
    pub enum PlayerStart {
        /// "Game start": every game of the player with the colour.
        #[default]
        Beginning => "beginning",
        /// "Current position": only the games that reach the board position.
        Position => "position",
    }
}

named_enum! {
    /// How the name is matched against the names of the database (M7-3/D3651).
    pub enum NameMatching {
        /// The name as it is in the name base.
        #[default]
        Exact => "exact",
        /// The names that start with the text, ignoring the case.
        Prefix => "prefix",
        /// The names that contain the text, ignoring the case.
        Contains => "contains",
    }
}

macro_rules! serialize_by_name {
    ($($ty:ty),+) => {$(
        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.name())
            }
        }
    )+};
}
serialize_by_name!(PlayerColor, PlayerStart, NameMatching);

impl PlayerColor {
    /// The colour as the rules know it.
    pub fn color(self) -> Color {
        match self {
            PlayerColor::White => Color::White,
            PlayerColor::Black => Color::Black,
        }
    }
}

impl From<Color> for PlayerColor {
    fn from(color: Color) -> PlayerColor {
        match color {
            Color::White => PlayerColor::White,
            Color::Black => PlayerColor::Black,
        }
    }
}

impl NameMatching {
    pub(crate) fn engine(self) -> NameMatch {
        match self {
            NameMatching::Exact => NameMatch::Exact,
            NameMatching::Prefix => NameMatch::Prefix,
            NameMatching::Contains => NameMatch::Contains,
        }
    }
}

/// The options of the report and what the dialog and the save remember, in
/// `[player_report]`. The defaults are those of the Scid
/// (`tcl/start.tcl` 820-834).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlayerReportPrefs {
    /// The statistics table (`Stats`).
    pub stats: bool,
    /// The oldest games to list, 0 to 10.
    pub oldest: u8,
    /// The newest games to list, 0 to 10.
    pub newest: u8,
    /// The most frequent opponents to list, 0 to 10.
    pub opponents: u8,
    /// Lengths and frequencies of the results (`Results`).
    pub results: bool,
    /// Average rating and performance (`AvgPerf`).
    pub avg_perf: bool,
    /// The games of the highest average rating to list, 0 to 10.
    pub high_rating: u8,
    /// The groups of ECO codes to list, 0 to 10.
    pub eco_codes: u8,
    /// The positional themes (`Themes`).
    pub themes: bool,
    /// The endgame classes (`Endgames`).
    pub endgames: bool,
    /// The most games in the theory table, 0 to 10 000 (0 turns it off).
    pub max_table_games: u32,
    /// Extra moves of the notes, 0 to 10.
    pub extra_moves: u8,
    /// The colour the dialog proposes.
    pub last_color: PlayerColor,
    /// The start position the dialog proposes.
    pub last_start: PlayerStart,
    /// The matching the dialog proposes.
    pub matching: NameMatching,
    /// "Empty the Clipbase and copy the games to it" (off by default).
    pub copy_to_clipbase: bool,
    /// The part the save dialog proposes.
    pub last_part: ReportPart,
    /// The format the save dialog proposes.
    pub last_format: ReportFormat,
    /// The folder of the last save.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_dir: Option<PathBuf>,
}

impl Default for PlayerReportPrefs {
    fn default() -> PlayerReportPrefs {
        let o = PlayerReportOptions::default();
        PlayerReportPrefs {
            stats: o.stats,
            oldest: o.oldest,
            newest: o.newest,
            opponents: o.opponents,
            results: o.results,
            avg_perf: o.avg_perf,
            high_rating: o.high_rating,
            eco_codes: o.eco_codes,
            themes: o.themes,
            endgames: o.endgames,
            max_table_games: o.max_table_games,
            extra_moves: o.extra_moves,
            last_color: PlayerColor::White,
            last_start: PlayerStart::Beginning,
            matching: NameMatching::Exact,
            copy_to_clipbase: false,
            last_part: ReportPart::Full,
            last_format: ReportFormat::Html,
            last_dir: None,
        }
    }
}

impl PlayerReportPrefs {
    /// The value with every field out of its range back at the default (a
    /// wrong value never makes an error).
    #[must_use]
    pub fn sanitized(mut self) -> PlayerReportPrefs {
        let d = PlayerReportPrefs::default();
        let count = |v: u8, default: u8| if v <= MAX_COUNT_OPTION { v } else { default };
        self.oldest = count(self.oldest, d.oldest);
        self.newest = count(self.newest, d.newest);
        self.opponents = count(self.opponents, d.opponents);
        self.high_rating = count(self.high_rating, d.high_rating);
        self.eco_codes = count(self.eco_codes, d.eco_codes);
        self.extra_moves = count(self.extra_moves, d.extra_moves);
        if self.max_table_games > MAX_TABLE_GAMES_LIMIT {
            self.max_table_games = d.max_table_games;
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

    /// The options the engine takes.
    pub fn options(&self) -> PlayerReportOptions {
        PlayerReportOptions {
            stats: self.stats,
            oldest: self.oldest,
            newest: self.newest,
            opponents: self.opponents,
            results: self.results,
            avg_perf: self.avg_perf,
            high_rating: self.high_rating,
            eco_codes: self.eco_codes,
            themes: self.themes,
            endgames: self.endgames,
            max_table_games: self.max_table_games,
            extra_moves: self.extra_moves,
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
        let p = PlayerReportPrefs::default();
        assert_eq!(p.options(), PlayerReportOptions::default());
        assert_eq!(p.options().max_table_games, 500);
        assert!(!p.copy_to_clipbase);
        assert_eq!(*Preferences::default().player_report, p);
    }

    #[test]
    fn values_round_trip_through_the_file() {
        let prefs = Preferences {
            player_report: Box::new(PlayerReportPrefs {
                stats: false,
                oldest: 0,
                newest: 10,
                opponents: 3,
                max_table_games: 10_000,
                last_color: PlayerColor::Black,
                last_start: PlayerStart::Position,
                matching: NameMatching::Contains,
                copy_to_clipbase: true,
                last_part: ReportPart::TableOnly,
                last_format: ReportFormat::Latex,
                last_dir: Some(PathBuf::from("reports")),
                ..PlayerReportPrefs::default()
            }),
            ..Preferences::default()
        };
        let text = render_preferences(&prefs);
        assert!(text.contains("[player_report]"), "{text}");
        let loaded = load_preferences(&text);
        assert_eq!(*loaded.prefs.player_report, *prefs.player_report);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn invalid_values_go_back_to_the_default() {
        let loaded = load_preferences(
            "[player_report]\noldest = 99\nmax_table_games = 99999\nopponents = -1\n\
             last_color = \"green\"\nmatching = 4\nstats = \"yes\"\nnewest = 3\n",
        );
        let p = &loaded.prefs.player_report;
        let d = PlayerReportPrefs::default();
        assert_eq!(p.oldest, d.oldest);
        assert_eq!(p.max_table_games, d.max_table_games);
        assert_eq!(p.opponents, d.opponents);
        assert_eq!(p.last_color, d.last_color);
        assert_eq!(p.matching, d.matching);
        assert_eq!(p.stats, d.stats);
        assert_eq!(p.newest, 3, "a good value next to bad ones is kept");
        assert!(!loaded.issues.is_empty());
    }

    #[test]
    fn a_change_is_sanitized() {
        let mut prefs = Preferences::default();
        PrefChange::PlayerReport(Box::new(PlayerReportPrefs {
            eco_codes: 200,
            max_table_games: 3,
            ..PlayerReportPrefs::default()
        }))
        .apply(&mut prefs);
        assert_eq!(prefs.player_report.eco_codes, 6);
        assert_eq!(prefs.player_report.max_table_games, 3);
    }
}
