//! Ready-to-draw snapshots of the ECO tools and the arithmetic of the bar
//! chart (spec M5-7b). Numbers only: the front-end formats them.

use mychess_core::{EcoBook, PartialDate, translate_eco_name};
use mychess_db::{EcoStats, bar_scale};

use super::message::{BrowserScope, ClassifyChoice, NoveltyOutcome};
use super::ui::ClassifyResult;
use crate::bases::SlotId;

/// Entries of the book shown at once; more are cut off and
/// [`EcoBrowserView::truncated`] says so (the whole book has over ten
/// thousand).
pub(crate) const MAX_ENTRIES: usize = 500;

/// Height and widths of the bars are given out of this many parts.
pub const BAR_UNITS: u16 = 1000;

/// An opening of the book, as the list of the browser shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcoEntryRow {
    /// The extended code (`B90a2`).
    pub code: String,
    /// The name, translated with the glossary of the interface.
    pub name: String,
    /// The moves as written in the file (`1.e4 c5 2.Nf3 ...`).
    pub moves: String,
}

/// How deep the node of the browser is (the title changes with it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcoLevel {
    /// The whole ECO tree.
    All,
    /// A letter or a letter and a digit (`B`, `B9`).
    Section,
    /// A code (`B90`) or a code and a subcode letter (`B90a`).
    Code,
}

impl EcoLevel {
    /// The level of the node `prefix`.
    pub fn of(prefix: &str) -> EcoLevel {
        match prefix.len() {
            0 => EcoLevel::All,
            1 | 2 => EcoLevel::Section,
            _ => EcoLevel::Code,
        }
    }
}

/// A bar of the chart: a child of the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcoBar {
    /// The node to go to when it is clicked (`B9`).
    pub prefix: String,
    /// The label under the bar: the prefix, or the subcode letter under a
    /// code (as `eco.tcl` does).
    pub label: String,
    /// The counters of the child.
    pub stats: EcoStats,
    /// Height of the whole bar, out of [`BAR_UNITS`] (the largest bar of the
    /// chart is full height).
    pub height: u16,
    /// Parts of the bar, out of [`BAR_UNITS`]: White wins, draws (and games
    /// without a result, as the draws line of `eco.tcl`), Black wins. They
    /// add up to [`BAR_UNITS`] unless the bar is empty.
    pub parts: [u16; 3],
}

/// A horizontal line of the grid of the chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EcoGridLine {
    /// The number of games the line stands for.
    pub value: u32,
    /// Its height, out of [`BAR_UNITS`].
    pub height: u16,
}

/// The browser, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct EcoBrowserView<'a> {
    /// The database it looks at.
    pub slot: SlotId,
    /// Games in the database.
    pub total_games: u32,
    /// Games with an ECO code (the whole table).
    pub classified: u32,
    /// The node.
    pub prefix: &'a str,
    /// How deep the node is.
    pub level: EcoLevel,
    /// The counters of the node.
    pub stats: EcoStats,
    /// The children of the node, with their counters (none under a code
    /// and a subcode letter).
    pub bars: Vec<EcoBar>,
    /// The lines of the grid.
    pub grid: Vec<EcoGridLine>,
    /// The count of the largest bar (at least 1).
    pub max: u32,
    /// The openings of the book under the node, or the hits of the search.
    pub entries: &'a [EcoEntryRow],
    /// There were more entries than `MAX_ENTRIES`.
    pub truncated: bool,
    /// An ECO file is loaded.
    pub book_loaded: bool,
    /// The statistics are being counted.
    pub building: bool,
    /// The text of the search field.
    pub find: &'a str,
    /// The search the list shows, if it shows one.
    pub finding: Option<&'a str>,
    /// Which games are counted.
    pub scope: BrowserScope,
    /// The filter of the database leaves out some games, so counting only
    /// the filter makes a difference.
    pub filter_active: bool,
}

/// The "Classify games" dialog, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct EcoClassifyView<'a> {
    /// The database to classify.
    pub slot: SlotId,
    /// Games in the database.
    pub total_games: u32,
    /// Games in its filter.
    pub filter_games: u32,
    /// The games to look at.
    pub choice: ClassifyChoice,
    /// The text of the date of [`ClassifyChoice::Since`].
    pub since: &'a str,
    /// The date typed is not a date.
    pub invalid_date: bool,
    /// Extended codes (`B12j`) instead of basic ones (`B12`).
    pub extended: bool,
    /// Today's date is known (the last year and month need it).
    pub today_known: bool,
    /// An ECO file is loaded.
    pub book_loaded: bool,
    /// The codes can be written in the file (an SI4 database that is not
    /// read-only); else only a count is possible.
    pub can_write: bool,
    /// A run or a count is in flight.
    pub running: bool,
    /// How the last run ended.
    pub result: Option<&'a ClassifyResult>,
}

/// The "Find novelty" dialog, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct EcoNoveltyView<'a> {
    /// The reference database.
    pub reference: SlotId,
    /// Every database in use, the Clipbase included.
    pub references: Vec<SlotId>,
    /// Only older games count.
    pub older_only: bool,
    /// The date of the game of the editor (shown next to "only older").
    pub game_date: PartialDate,
    /// The game has moves.
    pub has_moves: bool,
    /// A search is in flight.
    pub running: bool,
    /// How the last search ended.
    pub result: Option<&'a NoveltyOutcome>,
}

/// The height and the parts of the bar of `stats`, with `max` the count of
/// the largest bar: `(height, [white, draw, black])`, each out of
/// [`BAR_UNITS`]. An empty bar is `(0, [0, 0, 0])`; one that is not empty is
/// at least one unit high.
pub fn bar_parts(stats: &EcoStats, max: u32) -> (u16, [u16; 3]) {
    if stats.count == 0 {
        return (0, [0; 3]);
    }
    let units = u64::from(BAR_UNITS);
    let count = u64::from(stats.count);
    let height = (count * units / u64::from(max.max(stats.count))).clamp(1, units);
    let white = u64::from(stats.white) * units / count;
    let black = u64::from(stats.black) * units / count;
    let draw = units.saturating_sub(white + black);
    let narrow = |v: u64| u16::try_from(v).unwrap_or(BAR_UNITS);
    (narrow(height), [narrow(white), narrow(draw), narrow(black)])
}

/// The lines of the grid for a chart whose largest bar has `max` games: one
/// every [`bar_scale`] games, up to `max`.
pub fn grid_lines(max: u32) -> Vec<EcoGridLine> {
    let max = max.max(1);
    let step = bar_scale(max);
    let mut lines = Vec::new();
    let mut value = step;
    while value <= max && lines.len() < 64 {
        let height = u64::from(value) * u64::from(BAR_UNITS) / u64::from(max);
        lines.push(EcoGridLine {
            value,
            height: u16::try_from(height).unwrap_or(BAR_UNITS),
        });
        value = value.saturating_add(step);
    }
    lines
}

/// The openings under `prefix` (the summary of the node) or, with
/// `finding`, the ones whose `code [name]` contains the text, without
/// regard to case, in the original or the translated name (D1722). The
/// second value tells that more than `MAX_ENTRIES` were left out.
pub(crate) fn entries_for(
    book: &EcoBook,
    glossary: &[(String, String)],
    prefix: &str,
    finding: Option<&str>,
) -> (Vec<EcoEntryRow>, bool) {
    let pairs: Vec<(&str, &str)> = glossary
        .iter()
        .map(|(from, to)| (from.as_str(), to.as_str()))
        .collect();
    let row = |entry: &mychess_core::EcoEntry| EcoEntryRow {
        code: entry.eco().to_string(),
        name: translate_eco_name(entry.name(), &pairs),
        moves: entry.moves().to_owned(),
    };
    let mut rows = Vec::new();
    let mut truncated = false;
    match finding {
        None => {
            for entry in book.summary(prefix) {
                if rows.len() == MAX_ENTRIES {
                    truncated = true;
                    break;
                }
                rows.push(row(entry));
            }
        }
        Some(text) => {
            let needle = text.to_lowercase();
            for entry in book.entries() {
                let code = entry.eco().to_string();
                let original = format!("{code} [{}]", entry.name()).to_lowercase();
                let hit = original.contains(&needle) || {
                    let translated = translate_eco_name(entry.name(), &pairs);
                    format!("{code} [{translated}]")
                        .to_lowercase()
                        .contains(&needle)
                };
                if !hit {
                    continue;
                }
                if rows.len() == MAX_ENTRIES {
                    truncated = true;
                    break;
                }
                rows.push(row(entry));
            }
        }
    }
    (rows, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(white: u32, draw: u32, black: u32, none: u32) -> EcoStats {
        EcoStats {
            count: white + draw + black + none,
            white,
            draw,
            black,
            none,
        }
    }

    #[test]
    fn the_largest_bar_is_full_height_and_the_parts_add_up() {
        let (height, parts) = bar_parts(&stats(2, 1, 1, 0), 4);
        assert_eq!(height, BAR_UNITS);
        assert_eq!(parts, [500, 250, 250]);
        assert_eq!(parts.iter().sum::<u16>(), BAR_UNITS);
    }

    #[test]
    fn games_without_a_result_join_the_draws() {
        let (_, parts) = bar_parts(&stats(0, 1, 0, 1), 2);
        assert_eq!(parts, [0, 1000, 0]);
    }

    #[test]
    fn a_small_bar_is_at_least_one_unit_and_an_empty_one_is_nothing() {
        assert_eq!(bar_parts(&stats(1, 0, 0, 0), 1_000_000).0, 1);
        assert_eq!(bar_parts(&EcoStats::default(), 10), (0, [0; 3]));
    }

    #[test]
    fn the_grid_follows_the_scale_of_eco_tcl() {
        let lines = grid_lines(100);
        let values: Vec<u32> = lines.iter().map(|l| l.value).collect();
        assert_eq!(values, [25, 50, 75, 100]);
        assert_eq!(lines[3].height, BAR_UNITS);
        assert_eq!(grid_lines(3), []);
        let one: Vec<u32> = grid_lines(21).iter().map(|l| l.value).collect();
        assert_eq!(one, [10, 20]);
    }

    #[test]
    fn the_level_follows_the_length_of_the_prefix() {
        assert_eq!(EcoLevel::of(""), EcoLevel::All);
        assert_eq!(EcoLevel::of("B9"), EcoLevel::Section);
        assert_eq!(EcoLevel::of("B90"), EcoLevel::Code);
        assert_eq!(EcoLevel::of("B90a"), EcoLevel::Code);
    }

    const BOOK: &str =
        "A00a \"Start\" *\nB20 \"Sicilian Defence\" 1.e4 c5 *\nC20 \"King's Pawn\" 1.e4 e5 *\n";

    #[test]
    fn the_search_ignores_case_and_looks_at_the_translation() {
        let book = EcoBook::parse(BOOK).unwrap();
        let glossary = vec![("Defence".to_owned(), "Defesa".to_owned())];
        let (rows, cut) = entries_for(&book, &glossary, "", Some("sicilian"));
        assert_eq!(rows.len(), 1);
        assert!(!cut);
        assert_eq!(rows[0].name, "Sicilian Defesa");
        let (rows, _) = entries_for(&book, &glossary, "", Some("DEFESA"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].code, "B20");
        assert!(
            entries_for(&book, &glossary, "", Some("nothing"))
                .0
                .is_empty()
        );
    }

    #[test]
    fn the_summary_lists_the_codes_under_the_node() {
        let book = EcoBook::parse(BOOK).unwrap();
        let (rows, _) = entries_for(&book, &[], "B", None);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].moves, "1.e4 c5");
    }
}
