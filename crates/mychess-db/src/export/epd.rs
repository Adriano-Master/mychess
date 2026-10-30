// Behaviour derived from Scid vs PC 4.27, tcl/menus.tcl (692-757: export of
// the filter positions to EPD). Copyright (c) 1999-2003 Shane Hudson; Scid vs
// PC contributors. Scid is distributed under the GNU General Public License
// (see its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! Export of the positions of a filter to an EPD file (decisions D3713 and
//! D3714 of `docs/specs/m7-4-exportacao.md`).
//!
//! For each game the caller gives the **ply** of the filter (the position the
//! search found, `None` for a game without one). As in Scid, a game
//! contributes a position only when it has a ply or starts from a position
//! other than the standard one; with
//! [`EpdExportOptions::final_position`] every game contributes the position
//! at the end of its main line instead. The line is the FEN without the
//! counters (four fields) plus the operations asked for. Positions that
//! repeat (same placement, side, castling and en passant field as
//! [`EpdBook`] writes it) are written once, the first one, unless
//! [`EpdExportOptions::dedupe`] is off.
//!
//! No game is loaded into an editor: the export reads games only.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mychess_core::{Game, Position};
use mychess_pgn::epd::{EpdBook, EpdOperation, EpdOptions};

use super::text_file::{Append, Job, Step, push_fmt, run, with_extension_ci};
use super::{ExportError, ExportItem, ExportSkipReason};
use crate::progress::{CancelToken, ProgressSink};

/// A game to export and the ply of the filter for it.
#[derive(Debug)]
pub struct EpdItem {
    /// The game.
    pub item: ExportItem,
    /// The ply the filter found (half moves of the main line from the start
    /// position), `None` when the filter has none for this game (`Some(0)`
    /// counts as none, as in Scid).
    pub ply: Option<u32>,
}

impl EpdItem {
    /// An item.
    pub fn new(item: ExportItem, ply: Option<u32>) -> EpdItem {
        EpdItem { item, ply }
    }
}

/// Options of [`export_epd`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpdExportOptions {
    /// Write each distinct position once (default `true`).
    pub dedupe: bool,
    /// Use the position at the end of the main line of every game, ignoring
    /// the ply of the filter.
    pub final_position: bool,
    /// Add the `hmvc` and `fmvn` operations.
    pub hmvc_fmvn: bool,
    /// Add `id "<White>-<Black>, <Event> <year>"`.
    pub id: bool,
    /// Add `c0 "<game number>"` (the number in the database; nothing for a
    /// game in memory).
    pub game_number: bool,
    /// Add to the end of an existing file.
    pub append: bool,
    /// Throw the output away when cancelled (default `true`).
    pub discard_on_cancel: bool,
}

impl Default for EpdExportOptions {
    fn default() -> EpdExportOptions {
        EpdExportOptions {
            dedupe: true,
            final_position: false,
            hmvc_fmvn: false,
            id: false,
            game_number: false,
            append: false,
            discard_on_cancel: true,
        }
    }
}

/// The result of [`export_epd`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EpdExportReport {
    /// The file (the destination with `.epd` added when needed).
    pub path: PathBuf,
    /// Lines written.
    pub written: u64,
    /// Positions left out because an earlier game had the same one.
    pub repeated: u64,
    /// Games left out because they have no position to give (no ply and a
    /// standard start).
    pub not_selected: u64,
    /// Games that could not be read.
    pub skipped: Vec<(u32, ExportSkipReason)>,
    /// The export was cancelled.
    pub interrupted: bool,
    /// Time spent.
    pub elapsed: Duration,
}

/// Replaces what an EPD string operand cannot hold.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '"' => '\'',
            ';' => ',',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect()
}

fn pick_position(game: &Game, ply: Option<u32>, final_position: bool) -> Option<Position> {
    let start = game.start_position();
    let at = |n: usize| -> Position {
        match n.checked_sub(1).and_then(|i| game.mainline().nth(i)) {
            Some(id) => game
                .position_at(id)
                .cloned()
                .unwrap_or_else(|_| start.clone()),
            None => start.clone(),
        }
    };
    if final_position {
        return Some(at(game.mainline().count()));
    }
    // Scid: `sc_pos location` is 0 at the start, which counts as no ply.
    if let Some(p) = ply.filter(|p| *p > 0) {
        let n = usize::try_from(p).unwrap_or(usize::MAX);
        return Some(at(n.min(game.mainline().count())));
    }
    (start.fen() != Position::new().fen()).then(|| start.clone())
}

fn key_of(pos: &Position) -> String {
    pos.fen()
        .split_ascii_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Writes the positions of `games` to the EPD file `dest` (`.epd` is added
/// when the name does not end in it).
///
/// # Errors
///
/// [`ExportError::NothingToExport`] when no line was written (no file is
/// created or changed); [`ExportError::Io`] for files.
pub fn export_epd<I>(
    games: I,
    dest: &Path,
    options: &EpdExportOptions,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<EpdExportReport, ExportError>
where
    I: IntoIterator<Item = EpdItem>,
{
    let games = games.into_iter();
    let dest = with_extension_ci(dest, "epd");
    let (low, high) = games.size_hint();
    let job = Job {
        dest: &dest,
        start: "",
        end: "",
        append: if options.append {
            Append::ToEnd
        } else {
            Append::No
        },
        discard_on_cancel: options.discard_on_cancel,
        discard_if_empty: true,
        total: u64::try_from(high.unwrap_or(low)).unwrap_or(0),
    };
    let epd_options = EpdOptions::standard().with_write_clocks(options.hmvc_fmvn);
    let mut seen: HashSet<String> = HashSet::new();
    let (mut repeated, mut not_selected) = (0u64, 0u64);
    let result = run(games, &job, "", progress, cancel, |it, buf| {
        let (number, game) = match it.item {
            ExportItem::Game(g) => (0, Ok(g)),
            ExportItem::Stored { number, game } => (
                number,
                game.map_err(|e| ExportSkipReason::Unreadable(e.to_string())),
            ),
        };
        let game = match game {
            Ok(g) => g,
            Err(reason) => return Step::Skipped(number, reason),
        };
        let Some(pos) = pick_position(&game, it.ply, options.final_position) else {
            not_selected += 1;
            return Step::Ignored;
        };
        if options.dedupe && !seen.insert(key_of(&pos)) {
            repeated += 1;
            return Step::Ignored;
        }
        let mut ops = Vec::new();
        if options.id {
            let t = game.tags();
            let mut text = format!(
                "{}-{}, {}",
                clean(&t.white),
                clean(&t.black),
                clean(&t.event)
            );
            if let Some(y) = t.date.year() {
                push_fmt(&mut text, format_args!(" {y}"));
            }
            ops.push(EpdOperation::new("id", format!("\"{text}\"")));
        }
        if options.game_number && number > 0 {
            ops.push(EpdOperation::new("c0", format!("\"{number}\"")));
        }
        let mut book = EpdBook::new();
        book.insert(pos, ops);
        let mut bytes = Vec::new();
        if book.write(&mut bytes, epd_options).is_err() {
            return Step::Skipped(
                number,
                ExportSkipReason::Unwritable("cannot write the EPD line".to_owned()),
            );
        }
        match String::from_utf8(bytes) {
            Ok(text) => {
                buf.push_str(&text);
                Step::Written
            }
            Err(_) => Step::Skipped(
                number,
                ExportSkipReason::Unwritable("EPD line is not UTF-8".to_owned()),
            ),
        }
    })?;
    if result.written == 0 && !result.interrupted {
        return Err(ExportError::NothingToExport);
    }
    Ok(EpdExportReport {
        path: dest,
        written: if result.discarded { 0 } else { result.written },
        repeated,
        not_selected,
        skipped: result.skipped,
        interrupted: result.interrupted,
        elapsed: result.elapsed,
    })
}
