//! The blocking work of the book window, run by [`perform`](crate::perform)
//! on `spawn_blocking` (spec M5-6, D1652).

use std::path::{Path, PathBuf};

use mychess_book::{Book, BookBuilder, merge};
use mychess_core::{Game, Move, Position};
use mychess_db::GameSource;
use mychess_db::{ExportItem, ExportOptions, export_pgn};
use mychess_pgn::write::{Encoding, Newline, WriteOptions};

use super::message::{BookMessage, BranchExported, CreatedBook, MergedBook};
use super::types::{
    BookCancel, BookCreateOptions, BookExportOptions, BookIssue, BookMergeMode, BookSide,
    BranchStart, SharedBook,
};
use crate::bases::{OpenControl, SharedSource};
use crate::effect::Effect;
use crate::error::RequestId;
use crate::message::Message;
use crate::play::{BookSelection, PlayBook};

/// Where a loaded book goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookTarget {
    /// A side of the window.
    Side(BookSide),
    /// The opening book of the annotation and the report.
    Opening,
}

/// Reads the book at `path` for editing.
pub(crate) fn load_slot(path: &Path) -> Result<SharedBook, BookIssue> {
    Book::open(path)
        .map(SharedBook::new)
        .map_err(|e| BookIssue::of_read(&e))
}

/// Reads the book at `path` for playing from it (read-only).
pub(crate) fn load_opening(path: &Path) -> Result<PlayBook, BookIssue> {
    Book::open_readonly(path)
        .map(|book| PlayBook::new(book, BookSelection::Best))
        .map_err(|e| BookIssue::of_read(&e))
}

/// Writes a copy of `book` (to `path`, or to its own file) and returns the
/// copy, bound to the file and clean.
pub(crate) fn save_book(book: &SharedBook, path: Option<&Path>) -> Result<SharedBook, BookIssue> {
    let mut copy = book.get().clone();
    let saved = match path {
        Some(path) => copy.save_as(path),
        None => copy.save(),
    };
    saved.map(|()| SharedBook::new(copy)).map_err(|e| {
        // A folder that went away is a write failure, not a missing book.
        match BookIssue::of(&e) {
            BookIssue::NotFound => BookIssue::WriteFailed,
            other => other,
        }
    })
}

/// Builds a book from the games `numbers` of `source` (D1660). A game that
/// cannot be read counts as skipped.
pub(crate) fn create_book(
    source: &SharedSource,
    numbers: &[u32],
    options: BookCreateOptions,
    cancel: &BookCancel,
) -> Result<CreatedBook, BookIssue> {
    if numbers.is_empty() {
        return Err(BookIssue::NoGames);
    }
    let mut builder = BookBuilder::new(options.normalized().build_options());
    let mut unreadable = 0_u64;
    for &number in numbers {
        // The partial book is dropped: nothing was written anywhere (D1791).
        if cancel.is_cancelled() {
            return Err(BookIssue::Cancelled);
        }
        let game = {
            let mut db = source.lock();
            GameSource::game(&mut *db, number)
        };
        let Ok(game) = game else {
            unreadable += 1;
            continue;
        };
        let moves: Vec<_> = game.mainline().filter_map(|n| game.mv(n).ok()).collect();
        // An illegal move or a set-up start is counted by the builder.
        drop(builder.add_game_from(game.start_position(), moves, game.tags().result));
    }
    let (book, report) = builder.finish();
    Ok(CreatedBook {
        book: SharedBook::new(book),
        used: report.games_used,
        skipped: report.games_seen.saturating_sub(report.games_used) + unreadable,
    })
}

/// Merges two books (D1661).
pub(crate) fn merge_books(
    first: &SharedBook,
    second: &SharedBook,
    mode: BookMergeMode,
) -> MergedBook {
    let (book, report) = merge(first.get(), second.get(), mode.core());
    MergedBook {
        book: SharedBook::new(book),
        written: report.written,
        skipped: report.skipped,
    }
}

/// The lines of the branch of `book` that starts at `start` (D2160): depth
/// first, the heaviest move first, a line ends at `max_plies` or where the
/// book has no move left (after the weight filter). The flag says that the
/// limit of lines cut the branch short.
fn branch_lines(
    book: &Book,
    start: &Position,
    options: BookExportOptions,
    control: &OpenControl,
) -> Result<(Vec<Vec<Move>>, bool), BookIssue> {
    let max_plies = usize::try_from(options.max_plies).unwrap_or(usize::MAX);
    let max_lines = usize::try_from(options.max_lines).unwrap_or(usize::MAX);
    let mut lines: Vec<Vec<Move>> = Vec::new();
    let mut truncated = false;
    // (position, the moves that lead to it)
    let mut stack: Vec<(Position, Vec<Move>)> = vec![(start.clone(), Vec::new())];
    while let Some((position, path)) = stack.pop() {
        if control.is_cancelled() {
            return Err(BookIssue::Cancelled);
        }
        let moves: Vec<_> = if path.len() < max_plies {
            book.moves_sorted_by_weight(&position)
                .into_iter()
                .filter(|m| m.weight >= options.min_weight)
                .collect()
        } else {
            Vec::new()
        };
        if moves.is_empty() {
            if path.is_empty() {
                continue;
            }
            if lines.len() >= max_lines {
                truncated = true;
                break;
            }
            lines.push(path);
            continue;
        }
        // Reversed, so the heaviest move is taken first.
        for m in moves.iter().rev() {
            if let Ok(next) = position.after(m.mv) {
                let mut line = path.clone();
                line.push(m.mv);
                stack.push((next, line));
            }
        }
    }
    Ok((lines, truncated))
}

fn line_game(start: &Position, line: &[Move]) -> Option<Game> {
    let mut game = Game::from_position(start.clone());
    "Opening book branch".clone_into(&mut game.tags_mut().event);
    let mut node = game.root();
    for &mv in line {
        node = game.add_move(node, mv).ok()?;
    }
    Some(game)
}

/// Writes the branch of `book` from `start` as PGN, one game per line, to
/// `dest` (D2160-D2166). The file is written in a temporary in the same
/// directory and renamed at the end, so stopping the job or a failure leaves
/// the destination as it was.
pub(crate) fn export_branch(
    book: &SharedBook,
    start: &BranchStart,
    options: BookExportOptions,
    dest: &Path,
    confirmed: bool,
    protected: &[PathBuf],
    control: &OpenControl,
) -> Result<BranchExported, BookIssue> {
    if control.is_cancelled() {
        return Err(BookIssue::Cancelled);
    }
    if crate::games_export::is_protected(dest, protected) {
        return Err(BookIssue::ProtectedFile);
    }
    if dest.exists() {
        if !confirmed {
            return Err(BookIssue::FileExists);
        }
        // Renaming over a read-only file works on Unix and fails on Windows.
        if std::fs::metadata(dest).is_ok_and(|m| m.permissions().readonly()) {
            return Err(BookIssue::WriteFailed);
        }
    }
    let options = options.normalized();
    let (lines, truncated) = branch_lines(book.get(), &start.0, options, control)?;
    if lines.is_empty() {
        return Err(BookIssue::EmptyBranch);
    }
    let total = u64::try_from(lines.len()).unwrap_or(u64::MAX);
    let mut export = ExportOptions::default();
    export.pgn = WriteOptions::default().with_newline(Newline::Lf);
    export.encoding = Encoding::Utf8;
    export.total = Some(total);
    export.discard_on_cancel = true;
    export.discard_if_empty = true;
    let mut progress = control.reporter();
    progress(0, total);
    let begin = start.0.clone();
    // A line that cannot be built is a defect, never a silent omission
    // (D2173).
    let games = lines
        .iter()
        .map(|line| line_game(&begin, line).ok_or(BookIssue::Internal))
        .collect::<Result<Vec<_>, _>>()?;
    let items = games
        .into_iter()
        .map(|game| ExportItem::Game(Box::new(game)));
    let done = export_pgn(items, dest, &export, &mut progress, &control.token())
        .map_err(|_| BookIssue::WriteFailed)?;
    if done.interrupted {
        return Err(BookIssue::Cancelled);
    }
    Ok(BranchExported {
        path: done.path,
        lines: usize::try_from(done.written).unwrap_or(usize::MAX),
        truncated,
    })
}

/// The unit of the effects: what to do and who asked.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, BookIssue> + Send + 'static,
) -> Result<T, BookIssue> {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or(Err(BookIssue::Internal))
}

/// Runs one of the book effects. Any other effect is a defect of the caller
/// and answers with nothing useful.
pub(crate) async fn perform_book(effect: Effect) -> Message {
    match effect {
        Effect::BookLoad {
            request,
            target,
            path,
        } => load(request, target, path).await,
        Effect::BookSave {
            request,
            book,
            path,
        } => {
            let result = blocking(move || save_book(&book, path.as_deref())).await;
            Message::Book(BookMessage::Saved { request, result })
        }
        Effect::BookCreate {
            request,
            source,
            games,
            options,
            cancel,
        } => {
            let result = blocking(move || create_book(&source, &games, options, &cancel)).await;
            Message::Book(BookMessage::Created { request, result })
        }
        Effect::BookMerge {
            request,
            first,
            second,
            mode,
        } => {
            let result = blocking(move || Ok(merge_books(&first, &second, mode))).await;
            Message::Book(BookMessage::Merged { request, result })
        }
        Effect::BookExport {
            request,
            book,
            start,
            options,
            dest,
            confirmed,
            protected,
            control,
        } => {
            let result = blocking(move || {
                export_branch(
                    &book, &start, options, &dest, confirmed, &protected, &control,
                )
            })
            .await;
            Message::Book(BookMessage::Exported { request, result })
        }
        _ => Message::HostEffectNotHandled,
    }
}

async fn load(request: RequestId, target: BookTarget, path: PathBuf) -> Message {
    match target {
        BookTarget::Side(side) => {
            let result = blocking(move || load_slot(&path)).await;
            Message::Book(BookMessage::SlotLoaded {
                request,
                side,
                result,
            })
        }
        BookTarget::Opening => {
            let result = blocking(move || load_opening(&path)).await;
            Message::Book(BookMessage::OpeningLoaded { request, result })
        }
    }
}

#[cfg(test)]
mod tests {
    use mychess_core::{Game, GameResult, Position};
    use mychess_db::si4::{GameFlags, Si4Database, Si4Writer};

    use super::*;
    use crate::bases::BaseSource;

    fn book_with_e4() -> (Book, Position) {
        let start = Position::new();
        let e4 = start.parse_uci("e2e4").expect("uci");
        let mut book = Book::new();
        book.set_position_moves(&start, &[(e4, 10)]).expect("set");
        (book, start)
    }

    #[test]
    fn a_book_goes_to_disk_and_comes_back() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("a.bin");
        let (book, start) = book_with_e4();
        let saved = save_book(&SharedBook::new(book), Some(&path)).expect("save");
        assert!(!saved.get().is_dirty());
        assert_eq!(saved.get().path(), Some(path.as_path()));
        let back = load_slot(&path).expect("load");
        assert_eq!(back.get().moves(&start).len(), 1);
        let opening = load_opening(&path).expect("opening");
        assert!(opening.has_moves(&start));
    }

    #[test]
    fn failures_have_a_reason_and_no_book_is_written_without_a_path() {
        let dir = tempfile::tempdir().expect("dir");
        assert_eq!(
            load_slot(&dir.path().join("none.bin")).err(),
            Some(BookIssue::NotFound)
        );
        let bad = dir.path().join("bad.bin");
        std::fs::write(&bad, b"xyz").expect("write");
        assert_eq!(load_slot(&bad).err(), Some(BookIssue::TooShort));
        let (book, _) = book_with_e4();
        assert_eq!(
            save_book(&SharedBook::new(book), None).err(),
            Some(BookIssue::NoPath)
        );
    }

    fn game_e4_e5(result: GameResult) -> Game {
        let mut game = Game::new();
        let mut node = game.root();
        for uci in ["e2e4", "e7e5"] {
            let mv = game
                .position_at(node)
                .expect("position")
                .parse_uci(uci)
                .expect("uci");
            node = game.add_move(node, mv).expect("move");
        }
        game.tags_mut().result = result;
        game
    }

    #[test]
    fn a_book_is_made_from_the_games_of_a_database() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("games");
        let mut writer = Si4Writer::create(&path, 1).expect("create");
        for result in [GameResult::White, GameResult::Draw] {
            writer
                .add_game(&game_e4_e5(result), GameFlags::empty())
                .expect("add");
        }
        drop(writer);
        let db = Si4Database::open(&path).expect("open");
        let source = SharedSource::new(BaseSource::Si4(Box::new(db)));
        let options = BookCreateOptions {
            min_games: 1,
            ..BookCreateOptions::default()
        };
        let made = create_book(&source, &[1, 2, 99], options, &BookCancel::new()).expect("made");
        assert_eq!(made.used, 2);
        assert_eq!(made.skipped, 1, "game 99 does not exist");
        let start = Position::new();
        let moves = made.book.get().moves(&start);
        assert_eq!(moves.len(), 1);
        assert_eq!(
            create_book(&source, &[], options, &BookCancel::new()).err(),
            Some(BookIssue::NoGames)
        );
    }

    #[test]
    fn two_books_are_merged() {
        let (first, start) = book_with_e4();
        let d4 = start.parse_uci("d2d4").expect("uci");
        let mut second = Book::new();
        second.set_position_moves(&start, &[(d4, 5)]).expect("set");
        let merged = merge_books(
            &SharedBook::new(first),
            &SharedBook::new(second),
            BookMergeMode::SumWeights,
        );
        assert_eq!(merged.book.get().moves(&start).len(), 2);
        assert!(merged.written >= 2);
    }
}
