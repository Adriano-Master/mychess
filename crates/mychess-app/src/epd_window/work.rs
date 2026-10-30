//! The blocking work of the EPD window (spec M7-5, D3824): reading, writing,
//! ordering, searching and the batch edits of the books, all of it off the
//! interface thread. [`perform`] runs one [`EpdEffect`] and answers with the
//! message that reports its result.

use std::cmp::Ordering;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::Path;

use mychess_pgn::epd::{EpdBook, EpdError, EpdIndex, EpdOptions};

use super::message::{
    BatchDone, BatchOp, EpdEffect, EpdMessage, EpdSort, MAX_REPORT_LINES, OpenedEpd, SaveFailure,
    SavedEpd, SharedBook, SharedRows, SortKey, UnwritableKind,
};
use crate::error::{FileError, FileErrorKind};
use crate::message::Message;

/// Runs one effect of the EPD window.
pub(crate) async fn perform(effect: EpdEffect) -> Message {
    let message = match effect {
        EpdEffect::Open {
            request,
            path,
            options,
            create,
        } => {
            let failed = path.clone();
            let result = blocking(
                move || open(&path, options, create).map(Box::new),
                move || Err(internal(Some(failed))),
            )
            .await;
            EpdMessage::Opened { request, result }
        }
        EpdEffect::Save {
            request,
            path,
            book,
            options,
            compact,
        } => {
            let failed = path.clone();
            let result = blocking(
                move || save(&path, &book, options, compact),
                move || Err(SaveFailure::File(internal(Some(failed)))),
            )
            .await;
            EpdMessage::Saved { request, result }
        }
        EpdEffect::Rows {
            request,
            book,
            sort,
            filter,
        } => {
            let rows = blocking(
                move || SharedRows(std::sync::Arc::new(rows(book.book(), sort, &filter))),
                || SharedRows(std::sync::Arc::new(Vec::new())),
            )
            .await;
            EpdMessage::Rows { request, rows }
        }
        EpdEffect::Find {
            request,
            book,
            from,
            needle,
            forward,
        } => {
            let hit = blocking(
                move || book.book().find_text(from, &needle, forward),
                || None,
            )
            .await;
            EpdMessage::Found { request, hit }
        }
        EpdEffect::Batch { request, book, op } => {
            let result = blocking(move || Ok(batch(&book, &op)), || Err(internal(None))).await;
            EpdMessage::Batch { request, result }
        }
        EpdEffect::Tick {
            run,
            generation,
            delay,
        } => {
            tokio::time::sleep(delay).await;
            EpdMessage::Tick { run, generation }
        }
    };
    Message::Epd(message)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    on_panic: impl FnOnce() -> T,
) -> T {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|_| on_panic())
}

fn internal(path: Option<std::path::PathBuf>) -> FileError {
    FileError::new(FileErrorKind::Io(io::ErrorKind::Other), path)
}

/// The file error of a failure of the EPD reader or writer.
pub(crate) fn file_error(error: &EpdError, path: &Path) -> FileError {
    match error {
        EpdError::Io { source, .. } => FileError::from_io(source, Some(path.to_path_buf())),
        EpdError::LimitExceeded { max, .. } => FileError::new(
            FileErrorKind::TooLarge { limit: *max },
            Some(path.to_path_buf()),
        ),
        _ => internal(Some(path.to_path_buf())),
    }
}

/// Reads `path`, or creates it empty, and orders its records.
pub(crate) fn open(path: &Path, options: EpdOptions, create: bool) -> Result<OpenedEpd, FileError> {
    if create {
        EpdBook::new()
            .write_file_atomic(path, options)
            .map_err(|e| file_error(&e, path))?;
    }
    let canonical =
        fs::canonicalize(path).map_err(|e| FileError::from_io(&e, Some(path.into())))?;
    let load = EpdBook::read_file(&canonical, options).map_err(|e| file_error(&e, path))?;
    // Opening for writing (without truncating) tells what the system will
    // allow: permissions, and a file held by another process on Windows.
    let read_only = !create && OpenOptions::new().write(true).open(&canonical).is_err();
    let mut diagnostics = load.diagnostics;
    let mut dropped = load.dropped_diagnostics;
    if diagnostics.len() > MAX_REPORT_LINES {
        dropped = dropped.saturating_add((diagnostics.len() - MAX_REPORT_LINES) as u64);
        diagnostics.truncate(MAX_REPORT_LINES);
    }
    let rows = SharedRows(std::sync::Arc::new(rows(
        &load.book,
        EpdSort::default(),
        "",
    )));
    Ok(OpenedEpd {
        path: canonical,
        book: SharedBook::new(load.book),
        read_only,
        rows,
        diagnostics,
        dropped,
    })
}

/// Writes the book; compacts a copy first when asked.
pub(crate) fn save(
    path: &Path,
    book: &SharedBook,
    options: EpdOptions,
    compact: bool,
) -> Result<SavedEpd, SaveFailure> {
    let fail = |e: EpdError| match e {
        EpdError::Unwritable { opcode, reason } => SaveFailure::Unwritable {
            opcode,
            kind: UnwritableKind::of(reason),
        },
        other => SaveFailure::File(file_error(&other, path)),
    };
    if compact {
        let mut copy = book.book().clone();
        let moved = copy.compact();
        copy.write_file_atomic(path, options).map_err(fail)?;
        return Ok(SavedEpd {
            path: path.to_path_buf(),
            book: Some(SharedBook::new(copy)),
            moved,
        });
    }
    book.book().write_file_atomic(path, options).map_err(fail)?;
    Ok(SavedEpd {
        path: path.to_path_buf(),
        book: None,
        moved: Vec::new(),
    })
}

/// Applies `op` to a copy of the book; the copy is returned only if
/// something changed.
pub(crate) fn batch(book: &SharedBook, op: &BatchOp) -> BatchDone {
    let mut copy = book.book().clone();
    let mut moved = Vec::new();
    let changed = match op {
        BatchOp::SortAll => copy.sort_operations(),
        BatchOp::Strip(opcode) => strip_ignoring_case(&mut copy, opcode),
        BatchOp::Compact => {
            moved = copy.compact();
            moved.len()
        }
    };
    let keep = changed > 0 || matches!(op, BatchOp::Compact);
    BatchDone {
        book: keep.then(|| SharedBook::new(copy)),
        changed,
        moved,
    }
}

/// Removes every operation named `opcode`, whatever the case (rule 11,
/// D3811); returns how many records changed.
fn strip_ignoring_case(book: &mut EpdBook, opcode: &str) -> usize {
    let hits: Vec<_> = book
        .iter()
        .filter(|r| {
            r.operations()
                .iter()
                .any(|o| o.opcode.eq_ignore_ascii_case(opcode))
        })
        .map(|r| r.position().clone())
        .collect();
    for position in &hits {
        if let Some(record) = book.get_mut(position) {
            record
                .operations_mut()
                .retain(|o| !o.opcode.eq_ignore_ascii_case(opcode));
        }
    }
    hits.len()
}

/// Lowercases per character, a character at a time (the same rule as
/// `EpdBook::find_text`).
fn fold(text: &str) -> String {
    text.chars()
        .map(|c| {
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(one), None) => one,
                _ => c,
            }
        })
        .collect()
}

fn compare<K: Ord>(a: Option<&K>, b: Option<&K>, descending: bool) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) if descending => y.cmp(x),
        (Some(x), Some(y)) => x.cmp(y),
        // A record without the value is last either way.
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// The records that contain `filter` (any case), ordered by `sort`. The sort
/// is stable: ties keep the order of the file.
pub(crate) fn rows(book: &EpdBook, sort: EpdSort, filter: &str) -> Vec<EpdIndex> {
    let needle = fold(filter.trim());
    let passes = |record: &mychess_pgn::epd::EpdRecord| {
        needle.is_empty() || fold(&record.search_text()).contains(&needle)
    };
    let mut kept: Vec<(EpdIndex, &mychess_pgn::epd::EpdRecord)> = book
        .iter_indexed()
        .filter(|(_, record)| passes(record))
        .collect();
    match sort.key {
        SortKey::File => {
            if sort.descending {
                kept.reverse();
            }
            kept.into_iter().map(|(i, _)| i).collect()
        }
        SortKey::Fen => {
            let mut keyed: Vec<(Option<String>, EpdIndex)> = kept
                .into_iter()
                .map(|(i, r)| (Some(r.position().fen()), i))
                .collect();
            keyed.sort_by(|a, b| compare(a.0.as_ref(), b.0.as_ref(), sort.descending));
            keyed.into_iter().map(|(_, i)| i).collect()
        }
        SortKey::Id => {
            let mut keyed: Vec<(Option<&str>, EpdIndex)> =
                kept.into_iter().map(|(i, r)| (r.id(), i)).collect();
            keyed.sort_by(|a, b| compare(a.0.as_ref(), b.0.as_ref(), sort.descending));
            keyed.into_iter().map(|(_, i)| i).collect()
        }
        SortKey::Eval => {
            let mut keyed: Vec<(Option<i32>, EpdIndex)> =
                kept.into_iter().map(|(i, r)| (r.centipawns(), i)).collect();
            keyed.sort_by(|a, b| compare(a.0.as_ref(), b.0.as_ref(), sort.descending));
            keyed.into_iter().map(|(_, i)| i).collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_core::Position;
    use mychess_pgn::epd::EpdOperation;

    fn book() -> EpdBook {
        let start = Position::new();
        let mut b = EpdBook::new();
        let mv = |san: &str| {
            start
                .parse_san(san, mychess_core::SanMode::Tolerant)
                .expect("san")
                .mv
        };
        b.insert(
            start.clone(),
            vec![
                EpdOperation::new("id", "\"B\""),
                EpdOperation::new("ce", "30"),
            ],
        );
        b.insert(
            start.after(mv("e4")).expect("e4"),
            vec![
                EpdOperation::new("id", "\"A\""),
                EpdOperation::new("ce", "-10"),
            ],
        );
        b.insert(start.after(mv("d4")).expect("d4"), Vec::new());
        b
    }

    fn ids(book: &EpdBook, rows: &[EpdIndex]) -> Vec<String> {
        rows.iter()
            .map(|i| {
                book.get_by_index(*i)
                    .and_then(|r| r.id().map(str::to_owned))
                    .unwrap_or_else(|| "-".to_owned())
            })
            .collect()
    }

    #[test]
    fn rows_in_file_order_by_default() {
        let b = book();
        let r = rows(&b, EpdSort::default(), "");
        assert_eq!(ids(&b, &r), ["B", "A", "-"]);
    }

    #[test]
    fn rows_by_id_put_records_without_one_last_in_both_directions() {
        let b = book();
        let asc = rows(
            &b,
            EpdSort {
                key: SortKey::Id,
                descending: false,
            },
            "",
        );
        assert_eq!(ids(&b, &asc), ["A", "B", "-"]);
        let desc = rows(
            &b,
            EpdSort {
                key: SortKey::Id,
                descending: true,
            },
            "",
        );
        assert_eq!(ids(&b, &desc), ["B", "A", "-"]);
    }

    #[test]
    fn rows_by_eval_and_fen() {
        let b = book();
        let eval = rows(
            &b,
            EpdSort {
                key: SortKey::Eval,
                descending: true,
            },
            "",
        );
        assert_eq!(ids(&b, &eval), ["B", "A", "-"]);
        let fen = rows(
            &b,
            EpdSort {
                key: SortKey::Fen,
                descending: false,
            },
            "",
        );
        assert_eq!(fen.len(), 3);
        let texts: Vec<String> = fen
            .iter()
            .filter_map(|i| b.get_by_index(*i).map(|r| r.position().fen()))
            .collect();
        let mut sorted = texts.clone();
        sorted.sort();
        assert_eq!(texts, sorted);
    }

    #[test]
    fn the_filter_ignores_case_and_looks_at_fen_and_operations() {
        let b = book();
        assert_eq!(rows(&b, EpdSort::default(), "ce -10").len(), 1);
        assert_eq!(rows(&b, EpdSort::default(), "ID \"b\"").len(), 1);
        assert_eq!(rows(&b, EpdSort::default(), "rnbqkbnr").len(), 3);
        assert_eq!(rows(&b, EpdSort::default(), "zzz").len(), 0);
    }

    #[test]
    fn descending_file_order_reverses() {
        let b = book();
        let r = rows(
            &b,
            EpdSort {
                key: SortKey::File,
                descending: true,
            },
            "",
        );
        assert_eq!(ids(&b, &r), ["-", "A", "B"]);
    }

    #[test]
    fn open_creates_reads_and_tells_read_only() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("new.epd");
        let made = open(&path, EpdOptions::standard(), true).expect("create");
        assert!(made.book.book().is_empty());
        assert!(!made.read_only);
        assert!(path.exists());
        let again = open(&path, EpdOptions::standard(), false).expect("read");
        assert_eq!(again.rows.0.len(), 0);
        let missing = open(&dir.path().join("none.epd"), EpdOptions::standard(), false);
        assert_eq!(missing.expect_err("missing").kind, FileErrorKind::NotFound);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_without_write_permission_opens_read_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("ro.epd");
        fs::write(
            &path,
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - bm e4;\n",
        )
        .expect("write");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).expect("chmod");
        let opened = open(&path, EpdOptions::standard(), false).expect("read");
        // Root can write anywhere; the check only means something otherwise.
        if OpenOptions::new().write(true).open(&path).is_err() {
            assert!(opened.read_only);
        }
        assert_eq!(opened.book.book().len(), 1);
    }

    #[test]
    fn diagnostics_are_kept_up_to_the_limit() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("bad.epd");
        let mut text = String::new();
        for _ in 0..(MAX_REPORT_LINES + 50) {
            text.push_str("not a fen at all\n");
        }
        fs::write(&path, text).expect("write");
        let opened = open(&path, EpdOptions::standard(), false).expect("read");
        assert_eq!(opened.diagnostics.len(), MAX_REPORT_LINES);
        assert_eq!(opened.dropped, 50);
    }

    #[test]
    fn save_writes_and_compacts_a_copy() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("out.epd");
        let mut b = book();
        let first = b.iter_indexed().next().map(|(i, _)| i).expect("one");
        assert!(b.remove_at(first).is_some());
        let shared = SharedBook::new(b);
        let saved = save(&path, &shared, EpdOptions::standard(), true).expect("save");
        let compacted = saved.book.expect("compacted");
        assert_eq!(compacted.book().len(), 2);
        assert_eq!(saved.moved.len(), 2);
        let back = EpdBook::read_file(&path, EpdOptions::standard()).expect("read");
        assert_eq!(back.book.len(), 2);
        // The original is untouched by the compaction.
        assert_eq!(shared.book().len(), 2);
    }

    #[test]
    fn an_unwritable_record_is_reported_and_nothing_is_created() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("bad.epd");
        let mut b = EpdBook::new();
        b.insert(
            Position::new(),
            vec![EpdOperation::new("c0", "\"never closed")],
        );
        let error = save(&path, &SharedBook::new(b), EpdOptions::standard(), false)
            .expect_err("unwritable");
        assert!(
            matches!(
                error,
                SaveFailure::Unwritable {
                    kind: UnwritableKind::UnbalancedQuote,
                    ..
                }
            ),
            "{error:?}"
        );
        assert!(!path.exists());
    }

    #[test]
    fn batch_returns_a_book_only_when_something_changed() {
        let b = SharedBook::new(book());
        let none = batch(&b, &BatchOp::Strip("nope".into()));
        assert!(none.book.is_none());
        assert_eq!(none.changed, 0);
        let some = batch(&b, &BatchOp::Strip("CE".into()));
        assert_eq!(some.changed, 2);
        assert!(some.book.is_some());
        // The shared book was not touched.
        assert_eq!(
            b.book()
                .iter()
                .filter(|r| r.operation("ce").is_some())
                .count(),
            2
        );
    }
}
