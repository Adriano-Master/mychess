// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_eco_base`). Copyright
// (c) 1999-2003 Shane Hudson; Scid vs PC contributors. Scid is distributed
// under the GNU General Public License (see its COPYING); this file is part
// of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Classification of a whole database (D1186): the ECO code of each game from
//! the deepest book position of its first 60 plies.

use mychess_core::{CastlingMode, ClassifyOptions, Eco, EcoBook, Move, PartialDate, Position};
use thiserror::Error;

use crate::filter::Filter;
use crate::progress::{CancelToken, ProgressSink};
use crate::si4::{
    GameFlags, IndexEntry, RawMove, RecordError, Si4Error, Si4Writer, start_fen_of_record,
    walk_mainline,
};
use crate::source::GameSource;

/// Games between two looks at the cancel token (and flushes of the writes).
const CHECK_EVERY: u32 = 1000;

/// Which games [`classify_database`] looks at.
#[derive(Debug, Clone, Copy)]
pub enum ClassifySelection<'a> {
    /// Every game; the old code is replaced, or erased if nothing matches.
    All,
    /// Only the games without an ECO code.
    NoCode,
    /// Only the games of the filter.
    Filter(&'a Filter),
    /// Only the games played on or after the date; games with an unknown
    /// date are skipped.
    Since(PartialDate),
}

/// What a classification did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClassifyReport {
    /// Games processed, with or without a match (empty games and games that
    /// could not be read are not counted).
    pub classified: u32,
    /// Games whose code differs from the stored one. With
    /// [`classify_preview`] these are the games that *would* change.
    pub changed: u32,
    /// Entries written to the index (always 0 for a preview).
    pub written: u32,
    /// Games skipped because the entry or the record is unreadable.
    pub unreadable: u32,
    /// Whether the cancel token stopped the run; what was written stays.
    pub cancelled: bool,
}

/// Why a classification stopped.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ClassifyError {
    /// The ECO book has no entries (Scid: "No ECO Book is loaded").
    #[error("the ECO book is empty")]
    EmptyBook,
    /// Writing the index failed. What was written before stays; `report`
    /// counts the games processed and the entries written up to the failure.
    #[error("cannot write the ECO codes")]
    Write {
        /// The write error.
        #[source]
        source: Si4Error,
        /// The partial result up to the failure.
        report: ClassifyReport,
    },
}

/// Reads entries and records for the classification, whatever the database.
trait RecordSource {
    fn len(&self) -> u32;
    fn entry(&mut self, n: u32) -> Result<IndexEntry, Si4Error>;
    /// Runs `f` on the record bytes of game `n`.
    fn with_record<T>(
        &mut self,
        n: u32,
        entry: &IndexEntry,
        f: impl FnOnce(&[u8]) -> T,
    ) -> Result<T, Si4Error>;
}

struct WriterSource<'a> {
    writer: &'a mut Si4Writer,
    buf: Vec<u8>,
}

impl RecordSource for WriterSource<'_> {
    fn len(&self) -> u32 {
        self.writer.len()
    }

    fn entry(&mut self, n: u32) -> Result<IndexEntry, Si4Error> {
        self.writer.entry_at(n)
    }

    fn with_record<T>(
        &mut self,
        n: u32,
        entry: &IndexEntry,
        f: impl FnOnce(&[u8]) -> T,
    ) -> Result<T, Si4Error> {
        self.writer.record_into(n, entry, &mut self.buf)?;
        Ok(f(&self.buf))
    }
}

struct ReadSource<'a, D: GameSource + ?Sized>(&'a mut D);

impl<D: GameSource + ?Sized> RecordSource for ReadSource<'_, D> {
    fn len(&self) -> u32 {
        self.0.len()
    }

    fn entry(&mut self, n: u32) -> Result<IndexEntry, Si4Error> {
        self.0.entry(n)
    }

    fn with_record<T>(
        &mut self,
        n: u32,
        _entry: &IndexEntry,
        f: impl FnOnce(&[u8]) -> T,
    ) -> Result<T, Si4Error> {
        self.0.record_bytes(n).map(f)
    }
}

/// Classifies the games of a database opened for writing and stores the
/// codes in the index (`sc_eco base`). See D1186 for the rules.
///
/// A code is written only if it differs from the stored one. The writes are
/// flushed every 1000 games, so a cancel keeps what was already stored; there
/// is no transaction. The progress is reported at the same points.
///
/// # Errors
///
/// [`ClassifyError::EmptyBook`] for a book without entries, and
/// [`ClassifyError::Write`] (with the partial report) when the index cannot
/// be written. A cancel before the first game returns a report with
/// `cancelled` set and writes nothing.
pub fn classify_database(
    writer: &mut Si4Writer,
    book: &EcoBook,
    selection: ClassifySelection<'_>,
    extended: bool,
    cancel: &CancelToken,
    progress: &mut dyn ProgressSink,
) -> Result<ClassifyReport, ClassifyError> {
    let mut source = WriterSource {
        writer,
        buf: Vec::new(),
    };
    run(
        &mut source,
        book,
        selection,
        extended,
        cancel,
        progress,
        &mut |source, changes| source.writer.set_eco_batch_counted(changes),
    )
}

/// Like [`classify_database`] on a database that cannot be written (a
/// read-only base, a PGN file): nothing is stored and
/// [`ClassifyReport::changed`] tells how many games would change.
///
/// # Errors
///
/// [`ClassifyError::EmptyBook`] for a book without entries.
pub fn classify_preview<D: GameSource + ?Sized>(
    db: &mut D,
    book: &EcoBook,
    selection: ClassifySelection<'_>,
    extended: bool,
    cancel: &CancelToken,
    progress: &mut dyn ProgressSink,
) -> Result<ClassifyReport, ClassifyError> {
    run(
        &mut ReadSource(db),
        book,
        selection,
        extended,
        cancel,
        progress,
        &mut |_, _| (0, Ok(())),
    )
}

type Changes = [(u32, Option<Eco>)];

/// Stores a batch; returns how many entries reached the disk and the outcome.
type Flush<S> = dyn FnMut(&mut S, &Changes) -> (u32, Result<(), Si4Error>);

fn run<S: RecordSource>(
    source: &mut S,
    book: &EcoBook,
    selection: ClassifySelection<'_>,
    extended: bool,
    cancel: &CancelToken,
    progress: &mut dyn ProgressSink,
    flush: &mut Flush<S>,
) -> Result<ClassifyReport, ClassifyError> {
    if book.is_empty() {
        return Err(ClassifyError::EmptyBook);
    }
    if cancel.is_cancelled() {
        return Ok(ClassifyReport {
            cancelled: true,
            ..ClassifyReport::default()
        });
    }
    let total = source.len();
    let since = match selection {
        ClassifySelection::Since(date) => Some(date.to_scid_u32()),
        _ => None,
    };
    let mut report = ClassifyReport::default();
    let mut pending: Vec<(u32, Option<Eco>)> = Vec::new();
    let mut scratch = Scratch::default();
    let mut countdown = CHECK_EVERY;
    for n in 1..=total {
        countdown -= 1;
        if countdown == 0 {
            countdown = CHECK_EVERY;
            flush_pending(source, flush, &mut pending, &mut report)?;
            progress.progress(u64::from(n - 1), u64::from(total));
            if cancel.is_cancelled() {
                report.cancelled = true;
                return Ok(report);
            }
        }
        let Ok(entry) = source.entry(n) else {
            report.unreadable += 1;
            continue;
        };
        if entry.length == 0 {
            continue;
        }
        let skip = match selection {
            ClassifySelection::All => false,
            ClassifySelection::NoCode => entry.eco_code != 0,
            ClassifySelection::Filter(filter) => !filter.contains(n),
            ClassifySelection::Since(_) => {
                entry.date.year().is_none()
                    || since.is_some_and(|start| entry.date.to_scid_u32() < start)
            }
        };
        if skip {
            continue;
        }
        let start_flag = entry.flags.contains(GameFlags::START);
        let classified = source
            .with_record(n, &entry, |record| {
                classify_record(record, start_flag, book, extended, &mut scratch)
            })
            .map_err(|_| ());
        let Ok(Ok(code)) = classified else {
            report.unreadable += 1;
            continue;
        };
        report.classified += 1;
        if code.map_or(0, Eco::to_scid_code) != entry.eco_code {
            report.changed += 1;
            pending.push((n, code));
        }
    }
    flush_pending(source, flush, &mut pending, &mut report)?;
    progress.progress(u64::from(total), u64::from(total));
    Ok(report)
}

/// Stores the pending changes and counts what reached the disk; a failure
/// carries the partial report.
fn flush_pending<S: RecordSource>(
    source: &mut S,
    flush: &mut Flush<S>,
    pending: &mut Vec<(u32, Option<Eco>)>,
    report: &mut ClassifyReport,
) -> Result<(), ClassifyError> {
    let (written, result) = flush(source, pending);
    pending.clear();
    report.written = report.written.saturating_add(written);
    result.map_err(|source| ClassifyError::Write {
        source,
        report: *report,
    })
}

/// Buffers reused between games.
#[derive(Default)]
struct Scratch {
    moves: Vec<RawMove>,
}

/// The code of one record: the deepest book position among the first
/// `batch` plies, stopping when the material is below the book's minimum.
///
/// A record that breaks after some moves is classified by the moves read, as
/// Scid does; one that breaks before the first move is an error.
fn classify_record(
    record: &[u8],
    start_flag: bool,
    book: &EcoBook,
    extended: bool,
    scratch: &mut Scratch,
) -> Result<Option<Eco>, RecordError> {
    let limit = ClassifyOptions::batch()
        .max_ply
        .map_or(usize::MAX, |p| p as usize);
    let moves = &mut scratch.moves;
    moves.clear();
    let walked = walk_mainline(record, |_, next| match next {
        Some(mv) if moves.len() < limit => {
            moves.push(mv);
            std::ops::ControlFlow::<()>::Continue(())
        }
        Some(_) => std::ops::ControlFlow::Break(()),
        None => std::ops::ControlFlow::Continue(()),
    });
    if let Err(error) = walked
        && moves.is_empty()
    {
        return Err(error);
    }
    let mut position = if start_flag {
        match start_fen_of_record(record)? {
            Some(fen) => {
                let text = String::from_utf8_lossy(&fen);
                match Position::from_fen(&text) {
                    Ok(position) => position,
                    Err(_) => return Err(RecordError::new(0, "invalid start position")),
                }
            }
            None => Position::new(),
        }
    } else {
        Position::new()
    };
    if position.castling_mode() != CastlingMode::Standard {
        return Ok(None);
    }
    let pick = |entry: &mychess_core::EcoEntry| {
        if extended {
            entry.eco()
        } else {
            entry.eco().basic()
        }
    };
    let mut best = book.lookup(&position).map(pick);
    let fewest = usize::try_from(book.fewest_pieces()).unwrap_or(usize::MAX);
    for raw in moves.iter() {
        let mv = if raw.is_null() {
            Move::Null
        } else {
            match position.find_move(raw.from, raw.to, raw.promotion) {
                Some(mv) => mv,
                None => break,
            }
        };
        if position.play(mv).is_err() || position.pieces().take(fewest).count() < fewest {
            break;
        }
        if let Some(entry) = book.lookup(&position) {
            best = Some(pick(entry));
        }
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{game_from_sans, make_db};
    use super::*;
    use crate::NoProgress;
    use crate::si4::Si4Database;

    const BOOK: &str = "A00a \"Start\" *\nC20 \"KP\" 1.e4 e5 *\nC20b \"KP2\" 1.e4 e5 2.Nf3 *\nB01 \"Scandi\" 1.e4 d5 *\n";

    fn book() -> EcoBook {
        EcoBook::parse(BOOK).unwrap()
    }

    fn codes(base: &std::path::Path) -> Vec<u16> {
        let db = Si4Database::open(base).unwrap();
        (1..=db.len())
            .map(|n| db.entry(n).unwrap().eco_code)
            .collect()
    }

    fn eco(t: &str) -> u16 {
        t.parse::<Eco>().unwrap().to_scid_code()
    }

    fn sample(dir: &tempfile::TempDir) -> std::path::PathBuf {
        make_db(
            dir.path(),
            &[
                game_from_sans(&["e4", "e5", "Nf3", "Nc6"]),
                game_from_sans(&["d4"]),
                game_from_sans(&["Nf3", "e5", "e4"]),
            ],
        )
    }

    fn run(base: &std::path::Path, selection: ClassifySelection<'_>, ext: bool) -> ClassifyReport {
        let mut writer = Si4Writer::open(base).unwrap();
        classify_database(
            &mut writer,
            &book(),
            selection,
            ext,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap()
    }

    #[test]
    fn classifies_all_games_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let base = sample(&dir);
        let report = run(&base, ClassifySelection::All, true);
        assert_eq!(report.classified, 3);
        assert_eq!(report.changed, 3);
        assert_eq!(report.written, 3);
        // The third game transposes into 1.e4 e5 2.Nf3.
        assert_eq!(codes(&base), [eco("C20b"), eco("A00a"), eco("C20b")]);
        let again = run(&base, ClassifySelection::All, true);
        assert_eq!((again.classified, again.changed, again.written), (3, 0, 0));
    }

    #[test]
    fn basic_codes_and_nocode_selection() {
        let dir = tempfile::tempdir().unwrap();
        let base = sample(&dir);
        {
            let mut writer = Si4Writer::open(&base).unwrap();
            writer.set_eco(2, Some("B01".parse().unwrap())).unwrap();
        }
        let report = run(&base, ClassifySelection::NoCode, false);
        assert_eq!(report.classified, 2);
        assert_eq!(codes(&base), [eco("C20"), eco("B01"), eco("C20")]);
    }

    #[test]
    fn all_erases_the_code_of_a_game_that_matches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let base = sample(&dir);
        {
            let mut writer = Si4Writer::open(&base).unwrap();
            writer.set_eco(2, Some("B01".parse().unwrap())).unwrap();
        }
        let no_start = EcoBook::parse("C20 \"KP\" 1.e4 e5 *\n").unwrap();
        let mut writer = Si4Writer::open(&base).unwrap();
        let report = classify_database(
            &mut writer,
            &no_start,
            ClassifySelection::All,
            true,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
        drop(writer);
        assert_eq!(report.classified, 3);
        assert_eq!(codes(&base)[1], 0);
    }

    #[test]
    fn preview_counts_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let base = sample(&dir);
        let mut db = Si4Database::open(&base).unwrap();
        let report = classify_preview(
            &mut db,
            &book(),
            ClassifySelection::All,
            true,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
        assert_eq!((report.changed, report.written), (3, 0));
        assert_eq!(codes(&base), [0, 0, 0]);
    }

    #[test]
    fn an_empty_book_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let base = sample(&dir);
        let mut writer = Si4Writer::open(&base).unwrap();
        let result = classify_database(
            &mut writer,
            &EcoBook::default(),
            ClassifySelection::All,
            true,
            &CancelToken::new(),
            &mut NoProgress,
        );
        assert!(matches!(result, Err(ClassifyError::EmptyBook)));
    }

    #[test]
    fn a_cancel_before_the_first_game_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let base = sample(&dir);
        let mut writer = Si4Writer::open(&base).unwrap();
        let cancel = CancelToken::new();
        cancel.cancel();
        let report = classify_database(
            &mut writer,
            &book(),
            ClassifySelection::All,
            true,
            &cancel,
            &mut NoProgress,
        )
        .unwrap();
        drop(writer);
        assert!(report.cancelled);
        assert_eq!((report.classified, report.written), (0, 0));
        assert_eq!(codes(&base), [0, 0, 0]);
    }

    #[test]
    fn a_start_position_the_core_rejects_is_an_unreadable_record() {
        // Black is in check with White to move: the list position accepts it,
        // the core does not.
        let mut record = vec![0u8, 1];
        record.extend_from_slice(b"4k3/8/8/8/8/8/4R3/4K3 w - - 0 1\0");
        record.push(15);
        let mut scratch = Scratch::default();
        let result = classify_record(&record, true, &book(), true, &mut scratch);
        assert_eq!(result.unwrap_err().reason(), "invalid start position");
    }

    #[test]
    fn set_eco_changes_only_the_two_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let base = sample(&dir);
        let path = base.with_extension("si4");
        let before = std::fs::read(&path).unwrap();
        let mut writer = Si4Writer::open(&base).unwrap();
        assert!(writer.set_eco(2, Some("E99z4".parse().unwrap())).unwrap());
        assert!(!writer.set_eco(2, Some("E99z4".parse().unwrap())).unwrap());
        assert!(writer.set_eco(99, None).is_err());
        drop(writer);
        let after = std::fs::read(&path).unwrap();
        let diff: Vec<usize> = (0..before.len())
            .filter(|&i| before[i] != after[i])
            .collect();
        let start = 182 + 47 + 23;
        assert_eq!(diff, [start, start + 1]);
        assert_eq!(&after[start..start + 2], &65500u16.to_be_bytes());
    }
}
