// Scid: src/optable.cpp `OpTable::SelectGames` 2292-2329 and src/tkscid.cpp
// `sc_report_select` 14619-14652. Copyright (c) 2001-2003 Shane Hudson. Scid
// is distributed under the GNU General Public License (see its COPYING);
// this file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The games of a report that match an element of the text (decision D3622):
//! a theme, a note, a move order, an endgame class or all of them.

use super::model::{EndTheme, ReportCore, ReportLine};
use crate::filter::Filter;

/// What the user clicked in the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selector {
    /// Every game of the report (`all`).
    All,
    /// The games that show positional theme `n` (0 to 9).
    Theme(usize),
    /// The games listed in note `n`.
    Note(u32),
    /// The games that reach the position by the move order with this ID.
    MoveOrder(u32),
    /// The games that end in endgame class `n` (0 to 7, [`EndTheme`] order).
    Endgame(usize),
}

impl Selector {
    fn accepts(self, line: &ReportLine) -> bool {
        match self {
            Selector::All => true,
            Selector::Theme(n) => line.has_theme(n),
            Selector::Note(n) => line.note == n,
            Selector::MoveOrder(id) => line.move_order == id,
            Selector::Endgame(n) => EndTheme::from_index(n) == Some(line.end_theme),
        }
    }
}

/// The filter of the games of `core` that match `selector`. Each game keeps
/// the **ply of the reference position** (the filter value is the ply plus
/// one, as the Scid sets it), so loading it shows that position.
///
/// `games` is the number of games of the database. Only the games kept as
/// lines can be selected (the lines of an excluded move and the ones past the
/// limit of games are not).
pub fn select_games(core: &ReportCore, selector: Selector, games: u32) -> Filter {
    let mut filter = Filter::new(games);
    filter.fill(0);
    for line in core.lines.iter().filter(|l| selector.accepts(l)) {
        let value = u8::try_from(line.start_ply.saturating_add(1)).unwrap_or(u8::MAX);
        filter.set(line.game, value);
    }
    filter
}
