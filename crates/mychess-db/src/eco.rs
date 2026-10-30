// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_eco_base`,
// `sc_base_ecoStats`, `recalcFlagCounts`, `sc_game_novelty`, `sc_eco_game`)
// and src/misc.cpp (`eco_LastSubCode`). Copyright (c) 1999-2003 Shane Hudson;
// Scid vs PC contributors. Scid is distributed under the GNU General Public
// License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! ECO statistics, classification of a database, opening identification and
//! novelty search (`docs/specs/m5-7-eco-e-novidade.md`, slice M5-7a,
//! decisions D1180 to D1197).
//!
//! | Part | Content |
//! |---|---|
//! | [`EcoStatsTable`] | games and results per ECO prefix (the ECO browser) |
//! | [`classify_database`], [`classify_preview`] | `sc_eco base`: write (or count) the ECO code of each game |
//! | [`identify_opening`] | `sc_eco game`: the deepest book position of a game |
//! | [`find_novelty`] | `sc_game novelty`: the first move not seen in a database |
//!
//! `Si4Writer::set_eco` and `Si4Writer::set_eco_batch` (in `si4::write`)
//! write the two ECO bytes of an index entry. Nothing here touches the
//! interface: the dialogs and the browser belong to slice M5-7b.

mod classify;
mod identify;
mod novelty;
mod stats;
#[cfg(test)]
mod testutil;

pub use classify::{
    ClassifyError, ClassifyReport, ClassifySelection, classify_database, classify_preview,
};
pub use identify::{OpeningMatch, identify_opening};
pub use novelty::{Novelty, NoveltyError, NoveltyOptions, find_novelty};
pub use stats::{EcoChild, EcoScope, EcoStats, EcoStatsTable, InvalidPrefix, bar_scale};
