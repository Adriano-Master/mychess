// Scid: tcl/start.tcl 796-818 (defaults of the report options),
// tcl/tools/optable.tcl 270-395 (ranges), src/tkscid.cpp `sc_report_create`
// 14514-14611 (limits). Copyright (c) 1999-2004 Shane Hudson; Scid vs PC
// contributors. Scid is distributed under the GNU General Public License (see
// its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! The options of a report (`docs/specs/m7-2-relatorio-de-abertura.md`,
//! section 3, decision D3603).

use bitflags::bitflags;

/// The most games kept for the statistics (Scid: `OPTABLE_MAX_LINES`).
pub const MAX_GAMES_LIMIT: u32 = 100_000;
/// The most games in the theory table (Scid: `OPTABLE_MAX_TABLE_LINES`).
pub const MAX_TABLE_GAMES_LIMIT: u32 = 10_000;
/// The largest value of the count options (`Oldest`, `Newest`, ...).
pub const MAX_COUNT_OPTION: u8 = 10;

bitflags! {
    /// The on/off sections of the report.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct SectionSet: u8 {
        /// The statistics table (`Stats`).
        const STATS = 1;
        /// The popularity table (`Popular`).
        const POPULAR = 1 << 1;
        /// Average rating and performance (`AvgPerf`).
        const AVG_PERF = 1 << 2;
        /// Lengths and frequencies of the results (`Results`).
        const RESULTS = 1 << 3;
        /// The moves from the position, the tree (`MovesFrom`).
        const MOVES_FROM = 1 << 4;
        /// The positional themes (`Themes`).
        const THEMES = 1 << 5;
        /// The endgame classes (`Endgames`).
        const ENDGAMES = 1 << 6;
    }
}

impl Default for SectionSet {
    /// Every section on (the defaults of the Scid).
    fn default() -> SectionSet {
        SectionSet::all()
    }
}

/// The options of an opening (or player) report. The defaults are those of
/// the Scid; [`ReportOptions::normalized`] limits every value to its range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportOptions {
    /// The on/off sections.
    pub sections: SectionSet,
    /// How many of the oldest games to list (0 to 10).
    pub oldest: u8,
    /// How many of the newest games to list (0 to 10).
    pub newest: u8,
    /// How many of the most frequent players to list (0 to 10).
    pub most_frequent: u8,
    /// The most frequent players with White.
    pub most_frequent_white: bool,
    /// The most frequent players with Black.
    pub most_frequent_black: bool,
    /// How many of the highest-rated games to list (0 to 10).
    pub high_rating: u8,
    /// How many of the shortest wins to list (0 to 10).
    pub shortest: u8,
    /// The shortest wins of White.
    pub shortest_white: bool,
    /// The shortest wins of Black.
    pub shortest_black: bool,
    /// How many move orders to list (0 to 10).
    pub move_orders: u8,
    /// The most games in the theory table (0 turns the table off; at most
    /// [`MAX_TABLE_GAMES_LIMIT`]).
    pub max_table_games: u32,
    /// The most games kept for the statistics (1 to [`MAX_GAMES_LIMIT`]).
    pub max_games: u32,
    /// Extra moves of the notes of the theory table (0 to 10).
    pub extra_moves: u8,
    /// The number of rows of the theory table; 0 is automatic.
    pub table_rows: u8,
    /// The move of the position left out of the theory table (opening
    /// report only), in SAN; `None` leaves nothing out.
    pub exclude: Option<String>,
}

impl Default for ReportOptions {
    fn default() -> ReportOptions {
        ReportOptions {
            sections: SectionSet::default(),
            oldest: 5,
            newest: 5,
            most_frequent: 6,
            most_frequent_white: true,
            most_frequent_black: true,
            high_rating: 8,
            shortest: 5,
            shortest_white: true,
            shortest_black: true,
            move_orders: 8,
            max_table_games: 500,
            max_games: 10_000,
            extra_moves: 1,
            table_rows: 0,
            exclude: None,
        }
    }
}

impl ReportOptions {
    /// The options with every value limited to its range (rule 7: `0` games
    /// for the statistics becomes 1, as in the Scid). An empty or `none`
    /// exclusion is dropped.
    #[must_use]
    pub fn normalized(&self) -> ReportOptions {
        let count = |v: u8| v.min(MAX_COUNT_OPTION);
        ReportOptions {
            sections: self.sections,
            oldest: count(self.oldest),
            newest: count(self.newest),
            most_frequent: count(self.most_frequent),
            most_frequent_white: self.most_frequent_white,
            most_frequent_black: self.most_frequent_black,
            high_rating: count(self.high_rating),
            shortest: count(self.shortest),
            shortest_white: self.shortest_white,
            shortest_black: self.shortest_black,
            move_orders: count(self.move_orders),
            max_table_games: self.max_table_games.min(MAX_TABLE_GAMES_LIMIT),
            max_games: self.max_games.clamp(1, MAX_GAMES_LIMIT),
            extra_moves: count(self.extra_moves),
            table_rows: self.table_rows.min(super::table::MAX_ROWS),
            exclude: self
                .exclude
                .as_deref()
                .map(str::trim)
                .filter(|e| !e.is_empty() && !e.eq_ignore_ascii_case("none"))
                .map(str::to_owned),
        }
    }

    /// Whether the section 1 ("Statistics and history") is shown.
    pub fn has_stats_section(&self) -> bool {
        self.sections
            .intersects(SectionSet::STATS | SectionSet::POPULAR)
            || self.oldest > 0
            || self.newest > 0
            || self.has_frequent_players()
    }

    /// Whether the most frequent players are listed (for either colour).
    pub fn has_frequent_players(&self) -> bool {
        self.most_frequent > 0 && (self.most_frequent_white || self.most_frequent_black)
    }

    /// Whether the section 2 ("Ratings and performance") is shown.
    pub fn has_ratings_section(&self) -> bool {
        self.sections.contains(SectionSet::AVG_PERF) || self.high_rating > 0
    }

    /// Whether the shortest wins are listed (for either colour).
    pub fn has_shortest(&self) -> bool {
        self.shortest > 0 && (self.shortest_white || self.shortest_black)
    }

    /// Whether the section 3 ("Result trends") is shown. The Scid tests
    /// `ShortestBlack || ShortestBlack` here (a defect, rule 9); this is
    /// `Results || (Shortest > 0 && (White || Black))`.
    pub fn has_trends_section(&self) -> bool {
        self.sections.contains(SectionSet::RESULTS) || self.has_shortest()
    }

    /// Whether the section 4 ("Moves and themes") is shown.
    pub fn has_moves_section(&self) -> bool {
        self.move_orders > 0
            || self
                .sections
                .intersects(SectionSet::MOVES_FROM | SectionSet::THEMES | SectionSet::ENDGAMES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_those_of_the_scid() {
        let o = ReportOptions::default();
        assert_eq!((o.oldest, o.newest, o.most_frequent), (5, 5, 6));
        assert_eq!((o.high_rating, o.shortest, o.move_orders), (8, 5, 8));
        assert_eq!(
            (o.max_table_games, o.max_games, o.extra_moves),
            (500, 10_000, 1)
        );
        assert_eq!(o.sections, SectionSet::all());
    }

    #[test]
    fn normalized_limits_every_value() {
        let o = ReportOptions {
            oldest: 99,
            max_games: 0,
            max_table_games: 1_000_000,
            extra_moves: 200,
            table_rows: 200,
            exclude: Some("  none ".into()),
            ..ReportOptions::default()
        }
        .normalized();
        assert_eq!(o.oldest, 10);
        assert_eq!(o.max_games, 1);
        assert_eq!(o.max_table_games, MAX_TABLE_GAMES_LIMIT);
        assert_eq!(o.extra_moves, 10);
        assert_eq!(o.table_rows, 20);
        assert_eq!(o.exclude, None);
        let big = ReportOptions {
            max_games: u32::MAX,
            ..ReportOptions::default()
        }
        .normalized();
        assert_eq!(big.max_games, MAX_GAMES_LIMIT);
    }

    #[test]
    fn trends_section_uses_white_or_black_not_black_twice() {
        let o = ReportOptions {
            sections: SectionSet::empty(),
            shortest_white: true,
            shortest_black: false,
            ..ReportOptions::default()
        };
        assert!(o.has_trends_section());
        let none = ReportOptions {
            shortest_white: false,
            shortest_black: false,
            ..o
        };
        assert!(!none.has_trends_section());
    }
}
