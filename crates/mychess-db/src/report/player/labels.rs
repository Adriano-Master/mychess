// Scid: tcl/lang/english.tcl 1172-1251 (`Oprep*`, `PReport*`). Copyright (c)
// Shane Hudson and the Scid vs PC contributors; Scid is distributed under
// the GNU General Public License (see its COPYING); this file is part of
// MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The words of the exported documents of the player report (decision
//! M7-3/D3664). The shared words are in [`ReportLabels`]; the application fills
//! both from Fluent.

use crate::report::labels::ReportLabels;

/// The words of the text, HTML and LaTeX documents of a player report. A
/// text with `{n}` has the name or the moves where the `{n}` is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerLabels {
    /// The words shared with the opening report.
    pub base: ReportLabels,
    /// `Player`.
    pub player: String,
    /// `with the White pieces`.
    pub with_white: String,
    /// `with the Black pieces`.
    pub with_black: String,
    /// `after {n}` (the moves from the start, when there is a position).
    pub after: String,
    /// `Openings` (the groups of ECO codes).
    pub openings: String,
    /// `Most frequent opponents`.
    pub most_frequent_opponents: String,
    /// `Wins`.
    pub wins: String,
    /// `Draws`.
    pub draws: String,
    /// `Losses`.
    pub losses: String,
    /// `from the point of view of {n}`.
    pub pov: String,
    /// `No games of {n}`.
    pub none: String,
    /// `Player rating`.
    pub player_rating: String,
    /// `Opponents rating`.
    pub opponents_rating: String,
    /// `Player performance`.
    pub player_perf: String,
    /// `Opponents performance`.
    pub opponents_perf: String,
}

impl Default for PlayerLabels {
    fn default() -> PlayerLabels {
        let s = String::from;
        PlayerLabels {
            base: ReportLabels::default(),
            player: s("Player"),
            with_white: s("with the White pieces"),
            with_black: s("with the Black pieces"),
            after: s("after {n}"),
            openings: s("Openings"),
            most_frequent_opponents: s("Most frequent opponents"),
            wins: s("Wins"),
            draws: s("Draws"),
            losses: s("Losses"),
            pov: s("from the point of view of {n}"),
            none: s("No games of {n}"),
            player_rating: s("Player rating"),
            opponents_rating: s("Opponents rating"),
            player_perf: s("Player performance"),
            opponents_perf: s("Opponents performance"),
        }
    }
}
