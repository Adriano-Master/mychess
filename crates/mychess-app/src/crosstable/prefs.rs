//! The `[crosstable]` table of `preferences.toml` and the mirrors of the
//! options of the engine that the front-end may name (spec M6-4, D3355,
//! D3391). `mychess-gui` cannot name `mychess-db`.

use mychess_db::{CrosstableFormat, CrosstableOptions, CrosstableSort};
use serde::{Serialize, Serializer};

use crate::panel_calc::named_enum;

named_enum! {
    /// The format of the table (menu "Format").
    pub enum CrossFormat {
        /// Pick one by the number of players and games.
        #[default]
        Auto => "auto",
        /// Every player against every other.
        AllPlayAll => "all-play-all",
        /// One column per round.
        Swiss => "swiss",
        /// Knockout: one block per round.
        Knockout => "knockout",
    }
}

named_enum! {
    /// The order of the rows (menu "Sort by").
    pub enum CrossSort {
        /// By points, then the tie-breaks.
        #[default]
        Score => "score",
        /// By name.
        Name => "name",
        /// By country.
        Country => "country",
        /// By rating, highest first.
        Rating => "rating",
    }
}

impl CrossFormat {
    pub(crate) fn engine(self) -> CrosstableFormat {
        match self {
            CrossFormat::Auto => CrosstableFormat::Auto,
            CrossFormat::AllPlayAll => CrosstableFormat::AllPlayAll,
            CrossFormat::Swiss => CrosstableFormat::Swiss,
            CrossFormat::Knockout => CrosstableFormat::Knockout,
        }
    }
}

impl CrossSort {
    pub(crate) fn engine(self) -> CrosstableSort {
        match self {
            CrossSort::Score => CrosstableSort::Score,
            CrossSort::Name => CrosstableSort::Name,
            CrossSort::Country => CrosstableSort::Country,
            CrossSort::Rating => CrosstableSort::Rating,
        }
    }
}

/// One checkbox of the "Options" menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CrossFlag {
    /// Ages in years (needs a spelling file, D3365).
    Ages,
    /// Nationalities (needs a spelling file).
    Countries,
    /// Win/loss/draw tallies.
    Tallies,
    /// Ratings.
    Ratings,
    /// Titles (needs a spelling file).
    Titles,
    /// Tie-break scores.
    Tiebreaks,
    /// Include the games marked for deletion.
    IncludeDeleted,
    /// Colour information (Swiss only).
    SwissColors,
    /// Numbered columns (all-play-all only).
    NumberedColumns,
    /// Tie-break by head-to-head.
    HeadToHead,
    /// Tie-break by wins.
    TiebreakWins,
    /// Three points for a win.
    ThreePoints,
    /// A blank row between groups of equal score.
    SeparateGroups,
    /// Alternate row colours (front-end only).
    ColorRows,
}

/// The options of the table and of its window, in `[crosstable]`. The
/// defaults are the Scid's (spec 2.1).
// Each flag is one independent menu item.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct CrosstablePrefs {
    /// The format.
    pub format: CrossFormat,
    /// The order of the rows.
    pub sort: CrossSort,
    /// Show ages.
    pub show_ages: bool,
    /// Show nationalities.
    pub show_countries: bool,
    /// Show the `(+ - =)` tallies.
    pub show_tallies: bool,
    /// Show ratings and performances.
    pub show_ratings: bool,
    /// Show titles.
    pub show_titles: bool,
    /// Show the tie-break score.
    pub show_tiebreaks: bool,
    /// Include games marked for deletion.
    pub include_deleted: bool,
    /// Swiss: show the colours.
    pub swiss_colors: bool,
    /// All-play-all: number the columns.
    pub numbered_columns: bool,
    /// Tie-break by head-to-head.
    pub head_to_head: bool,
    /// Tie-break by wins.
    pub tiebreak_wins: bool,
    /// Three points for a win.
    pub three_points: bool,
    /// Blank row between groups of equal score.
    pub separate_groups: bool,
    /// Alternate row colours.
    pub color_rows: bool,
    /// The colour of the alternate rows, `#rrggbb` (`crosscolor`, grey80).
    pub row_color: String,
}

impl Default for CrosstablePrefs {
    fn default() -> CrosstablePrefs {
        CrosstablePrefs {
            format: CrossFormat::Auto,
            sort: CrossSort::Score,
            show_ages: true,
            show_countries: true,
            show_tallies: true,
            show_ratings: true,
            show_titles: true,
            show_tiebreaks: false,
            include_deleted: false,
            swiss_colors: true,
            numbered_columns: false,
            head_to_head: false,
            tiebreak_wins: false,
            three_points: false,
            separate_groups: false,
            color_rows: true,
            row_color: DEFAULT_ROW_COLOR.to_owned(),
        }
    }
}

/// `grey80` of Tk.
pub const DEFAULT_ROW_COLOR: &str = "#cccccc";

/// Whether `text` is `#rrggbb`.
pub(crate) fn is_hex_color(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text.chars().skip(1).all(|c| c.is_ascii_hexdigit())
}

impl CrosstablePrefs {
    /// The preferences with a valid row colour.
    #[must_use]
    pub fn sanitized(mut self) -> CrosstablePrefs {
        if !is_hex_color(&self.row_color) {
            DEFAULT_ROW_COLOR.clone_into(&mut self.row_color);
        }
        self
    }

    /// Turns a checkbox on or off.
    pub fn set(&mut self, flag: CrossFlag, on: bool) {
        let field = match flag {
            CrossFlag::Ages => &mut self.show_ages,
            CrossFlag::Countries => &mut self.show_countries,
            CrossFlag::Tallies => &mut self.show_tallies,
            CrossFlag::Ratings => &mut self.show_ratings,
            CrossFlag::Titles => &mut self.show_titles,
            CrossFlag::Tiebreaks => &mut self.show_tiebreaks,
            CrossFlag::IncludeDeleted => &mut self.include_deleted,
            CrossFlag::SwissColors => &mut self.swiss_colors,
            CrossFlag::NumberedColumns => &mut self.numbered_columns,
            CrossFlag::HeadToHead => &mut self.head_to_head,
            CrossFlag::TiebreakWins => &mut self.tiebreak_wins,
            CrossFlag::ThreePoints => &mut self.three_points,
            CrossFlag::SeparateGroups => &mut self.separate_groups,
            CrossFlag::ColorRows => &mut self.color_rows,
        };
        *field = on;
    }

    /// The options as the engine takes them.
    pub(crate) fn options(&self) -> CrosstableOptions {
        CrosstableOptions {
            format: self.format.engine(),
            sort: self.sort.engine(),
            show_ages: self.show_ages,
            show_countries: self.show_countries,
            show_tallies: self.show_tallies,
            show_ratings: self.show_ratings,
            show_titles: self.show_titles,
            show_tiebreaks: self.show_tiebreaks,
            include_deleted: self.include_deleted,
            swiss_colors: self.swiss_colors,
            numbered_columns: self.numbered_columns,
            tiebreak_head_to_head: self.head_to_head,
            tiebreak_wins: self.tiebreak_wins,
            three_points: self.three_points,
            separate_score_groups: self.separate_groups,
            decimal_point: '.',
        }
    }
}

#[derive(Serialize)]
struct Table<'a> {
    format: &'static str,
    sort: &'static str,
    show_ages: bool,
    show_countries: bool,
    show_tallies: bool,
    show_ratings: bool,
    show_titles: bool,
    show_tiebreaks: bool,
    include_deleted: bool,
    swiss_colors: bool,
    numbered_columns: bool,
    head_to_head: bool,
    tiebreak_wins: bool,
    three_points: bool,
    separate_groups: bool,
    color_rows: bool,
    row_color: &'a str,
}

impl Serialize for CrosstablePrefs {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Table {
            format: self.format.name(),
            sort: self.sort.name(),
            show_ages: self.show_ages,
            show_countries: self.show_countries,
            show_tallies: self.show_tallies,
            show_ratings: self.show_ratings,
            show_titles: self.show_titles,
            show_tiebreaks: self.show_tiebreaks,
            include_deleted: self.include_deleted,
            swiss_colors: self.swiss_colors,
            numbered_columns: self.numbered_columns,
            head_to_head: self.head_to_head,
            tiebreak_wins: self.tiebreak_wins,
            three_points: self.three_points,
            separate_groups: self.separate_groups,
            color_rows: self.color_rows,
            row_color: &self.row_color,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_those_of_scid() {
        let o = CrosstablePrefs::default().options();
        assert_eq!(o, CrosstableOptions::default());
    }

    #[test]
    fn names_round_trip() {
        for f in CrossFormat::ALL {
            assert_eq!(CrossFormat::from_name(f.name()), Some(*f));
        }
        for s in CrossSort::ALL {
            assert_eq!(CrossSort::from_name(s.name()), Some(*s));
        }
        assert_eq!(CrossSort::from_name("x"), None);
    }

    #[test]
    fn a_bad_row_colour_is_replaced() {
        let p = CrosstablePrefs {
            row_color: "red".into(),
            ..CrosstablePrefs::default()
        };
        assert_eq!(p.sanitized().row_color, DEFAULT_ROW_COLOR);
        assert!(is_hex_color("#A0b1C2"));
        assert!(!is_hex_color("#12345"));
    }
}
