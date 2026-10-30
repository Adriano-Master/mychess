//! The book window: the controller of two Polyglot books, and the link of
//! the opening book with the annotation and the report (spec M5-6,
//! D1650-D1668).

use mychess_book::{Book, BookError, MAX_WEIGHT};
use mychess_core::{CastlingMode, Game, Move, NodeId, Position};

use std::path::{Path, PathBuf};

use super::AppState;
use crate::annotate::BookProbe;
use crate::bases::OpenControl;
use crate::book::{
    BookCancel, BookIssue, BookMessage, BookNotice, BookPrefs, BookSide, BookSlot, BookTarget,
    BookUi, BookView, BranchExported, BranchStart, CreatedBook, Job, MergedBook, SharedBook,
    WeightScale, build_view,
};
use crate::effect::Effect;
use crate::games_export::fix_destination;
use crate::prefs::{PrefChange, PrefsMessage};
use crate::state::GameOrigin;

/// The most lines a branch import takes, and the longest line (D1656).
const MAX_BRANCH_LINES: usize = 5_000;
const MAX_BRANCH_PLIES: usize = 600;

/// The moves and weights of the book in `position`, in the file order.
fn pairs(book: &Book, position: &Position) -> Vec<(Move, u16)> {
    book.moves(position)
        .into_iter()
        .map(|m| (m.mv, m.weight))
        .collect()
}

/// What an edit of a book reports.
enum Edit {
    /// Nothing to say.
    Quiet,
    /// This many moves were added.
    Added(usize),
    /// This many moves were removed.
    Removed(usize),
    /// The moves were copied.
    Copied,
}

/// Every line of the subtree of `from`, variations included, as lists of
/// moves (D1656). `None` when the branch is too big.
fn branch_lines(game: &Game, from: NodeId) -> Option<Vec<(Vec<Move>, u16)>> {
    let mut lines = Vec::new();
    let mut path: Vec<Move> = Vec::new();
    // (node, depth of the node: its moves in `path`)
    let mut stack: Vec<(NodeId, usize)> = Vec::new();
    for child in game.children(from).ok()?.iter().rev() {
        stack.push((*child, 1));
    }
    while let Some((node, depth)) = stack.pop() {
        path.truncate(depth - 1);
        path.push(game.mv(node).ok()?);
        if path.len() > MAX_BRANCH_PLIES {
            return None;
        }
        let children = game.children(node).ok()?;
        if children.is_empty() {
            if lines.len() >= MAX_BRANCH_LINES {
                return None;
            }
            lines.push((path.clone(), 1));
        } else {
            for child in children.iter().rev() {
                stack.push((*child, depth + 1));
            }
        }
    }
    Some(lines)
}

/// The weight typed in a field (D1654).
fn parse_weight(text: &str) -> Result<u16, BookIssue> {
    let number: u64 = text.trim().parse().map_err(|_| BookIssue::NotANumber)?;
    match u16::try_from(number) {
        Ok(0) => Err(BookIssue::ZeroWeight),
        Ok(w) if w <= MAX_WEIGHT => Ok(w),
        _ => Err(BookIssue::WeightTooLarge),
    }
}

impl AppState {
    /// The book window.
    pub fn book(&self) -> &BookUi {
        &self.book
    }

    /// Whether a book of the window has changes that are not saved (the
    /// front-end asks before the program quits, D1792).
    pub fn book_has_unsaved(&self) -> bool {
        self.book.has_unsaved()
    }

    /// A snapshot of the book window for the position of the cursor.
    pub fn book_view(&self) -> BookView {
        let filter = self
            .bases
            .current_slot()
            .map_or(0, super::super::bases::BaseSlot::filtered);
        build_view(
            &self.book,
            self.session.game(),
            self.session.cursor(),
            self.prefs.book.opening_book.as_ref(),
            filter,
        )
    }

    /// The opening book as a probe of the annotation and the report, when
    /// one is chosen and loaded (D1663).
    pub(crate) fn opening_probe(&self) -> Option<&dyn BookProbe> {
        self.book.opening.as_ref().map(|b| b as &dyn BookProbe)
    }

    /// Asks for the opening book of the preferences to be read, or drops
    /// the loaded one when there is none (D1662).
    pub(crate) fn book_request_opening(&mut self, effects: &mut Vec<Effect>) {
        self.book.jobs.retain(|j| !matches!(j, Job::Opening { .. }));
        self.book.opening_issue = None;
        let Some(path) = self.prefs.book.opening_book.clone() else {
            self.book.opening = None;
            return;
        };
        let request = self.new_request();
        self.book.jobs.push(Job::Opening {
            request,
            path: path.clone(),
        });
        effects.push(Effect::BookLoad {
            request,
            target: BookTarget::Opening,
            path,
        });
    }

    fn book_issue(&mut self, issue: BookIssue) {
        self.book.notice = Some(BookNotice::Issue(issue));
    }

    /// The controller of the window.
    #[allow(clippy::too_many_lines)] // one flat match over the messages reads best
    pub(crate) fn book_message(&mut self, message: BookMessage, effects: &mut Vec<Effect>) {
        match message {
            BookMessage::SetOpen(open) => self.book.open = open,
            BookMessage::Load { side, path } => {
                if let Some(issue) = self.book_side_blocked(side) {
                    return self.book_issue(issue);
                }
                let request = self.new_request();
                self.book.notice = None;
                self.book.jobs.push(Job::Load { request, side });
                effects.push(Effect::BookLoad {
                    request,
                    target: BookTarget::Side(side),
                    path,
                });
            }
            BookMessage::New { side } => {
                if let Some(issue) = self.book_side_blocked(side) {
                    return self.book_issue(issue);
                }
                self.book.notice = None;
                self.book.slots[side.index()] =
                    Some(BookSlot::new(SharedBook::new(Book::new()), false));
            }
            BookMessage::Close { side } => {
                if self.book.busy_side(side) {
                    return self.book_issue(BookIssue::Busy);
                }
                if self.book.is_dirty(side) {
                    self.book.confirm_close = Some(side);
                } else {
                    self.book.slots[side.index()] = None;
                }
            }
            BookMessage::ConfirmClose(yes) => {
                if let Some(side) = self.book.confirm_close.take()
                    && yes
                    && !self.book.busy_side(side)
                {
                    self.book.slots[side.index()] = None;
                }
            }
            BookMessage::SetWeight { side, mv, text } => match parse_weight(&text) {
                Ok(weight) => self.book_edit(side, move |book, _, position, _| {
                    let mut list = pairs(book, position);
                    let at = list.iter().position(|(m, _)| *m == mv);
                    list.get_mut(at.ok_or(BookIssue::IllegalMove)?)
                        .ok_or(BookIssue::IllegalMove)?
                        .1 = weight;
                    book.set_position_moves(position, &list)
                        .map(|()| Edit::Quiet)
                        .map_err(|e| BookIssue::of(&e))
                }),
                Err(issue) => self.book_issue(issue),
            },
            BookMessage::ScaleWeight { side, mv, scale } => {
                self.book_edit(side, move |book, _, position, _| {
                    let mut list = pairs(book, position);
                    let entry = list
                        .iter_mut()
                        .find(|(m, _)| *m == mv)
                        .ok_or(BookIssue::IllegalMove)?;
                    entry.1 = match scale {
                        WeightScale::Double => entry.1.saturating_mul(2).min(MAX_WEIGHT),
                        WeightScale::Half => (entry.1 / 2).max(1),
                    };
                    book.set_position_moves(position, &list)
                        .map(|()| Edit::Quiet)
                        .map_err(|e| BookIssue::of(&e))
                });
            }
            BookMessage::RemoveMove { side, mv } => {
                self.book_edit(side, move |book, _, position, _| {
                    let mut list = pairs(book, position);
                    let before = list.len();
                    list.retain(|(m, _)| *m != mv);
                    if list.len() == before {
                        return Err(BookIssue::IllegalMove);
                    }
                    book.set_position_moves(position, &list)
                        .map(|()| Edit::Removed(1))
                        .map_err(|e| BookIssue::of(&e))
                });
            }
            BookMessage::AddMove { side, mv } => {
                self.book_edit(side, move |book, _, position, _| {
                    add_move(book, position, mv)
                });
            }
            BookMessage::AddGameMove { side } => {
                self.book_edit(side, |book, game, position, cursor| {
                    let next = game
                        .children(cursor)
                        .ok()
                        .and_then(|c| c.first().copied())
                        .and_then(|n| game.mv(n).ok())
                        .ok_or(BookIssue::NothingToAdd)?;
                    add_move(book, position, next)
                });
            }
            BookMessage::AddLine { side } => {
                self.book_edit(side, |book, game, position, cursor| {
                    let line = continuation(game, cursor);
                    if line.is_empty() {
                        return Err(BookIssue::NothingToAdd);
                    }
                    book.add_line(&line, position, 1)
                        .map(Edit::Added)
                        .map_err(|e| BookIssue::of(&e))
                });
            }
            BookMessage::RemoveLine { side } => {
                self.book_edit(side, |book, game, position, cursor| {
                    let line = continuation(game, cursor);
                    if line.is_empty() {
                        return Err(BookIssue::NothingToAdd);
                    }
                    book.remove_line(&line, position)
                        .map(Edit::Removed)
                        .map_err(|e| BookIssue::of(&e))
                });
            }
            BookMessage::ImportBranch { side } => {
                self.book_edit(side, |book, game, position, cursor| {
                    let lines = branch_lines(game, cursor).ok_or(BookIssue::BranchTooBig)?;
                    if lines.is_empty() {
                        return Err(BookIssue::NothingToAdd);
                    }
                    book.add_lines(&lines, position)
                        .map(Edit::Added)
                        .map_err(|e| BookIssue::of(&e))
                });
            }
            BookMessage::CopyPosition { to } => self.book_copy(to),
            BookMessage::Save { side } => self.book_save(side, None, effects),
            BookMessage::SaveAs { side, path } => self.book_save(side, Some(path), effects),
            BookMessage::CreateFromFilter { side } => self.book_create(side, effects),
            BookMessage::CancelCreate => {
                // The job leaves the list at once: the answer that may still
                // come is dropped (unknown request) and the side is free.
                let at = self
                    .book
                    .jobs
                    .iter()
                    .position(|j| matches!(j, Job::Create { .. }));
                if let Some(Job::Create { cancel, .. }) = at.map(|i| self.book.jobs.remove(i)) {
                    cancel.cancel();
                    self.book.notice = Some(BookNotice::CreateCancelled);
                }
            }
            BookMessage::ExportBranch { side, path } => self.book_export(side, &path, effects),
            BookMessage::SetExportOptions(options) => self.book.export = options.normalized(),
            BookMessage::CancelExport => {
                // The job stays in the list until it answers: it may have
                // passed its last check and renamed the file already, and
                // a new export to the same file must not start meanwhile
                // (D2170). The answer says what really happened (D2171).
                let mut stopped = false;
                for job in &mut self.book.jobs {
                    if let Job::Export {
                        control, stopping, ..
                    } = job
                    {
                        control.cancel();
                        *stopping = true;
                        stopped = true;
                    }
                }
                if stopped {
                    self.book.notice = Some(BookNotice::ExportStopping);
                }
            }
            BookMessage::Exported { request, result } => {
                let Some(Job::Export { stopping, .. }) = self.book.take_job(request) else {
                    return;
                };
                match result {
                    Ok(BranchExported { lines, .. }) if stopping => {
                        self.book.notice = Some(BookNotice::ExportFinishedAnyway { lines });
                    }
                    Ok(BranchExported {
                        lines, truncated, ..
                    }) => self.book.notice = Some(BookNotice::Exported { lines, truncated }),
                    Err(BookIssue::Cancelled) => {
                        self.book.notice = Some(BookNotice::ExportCancelled);
                    }
                    Err(issue) => self.book_issue(issue),
                }
            }
            BookMessage::Merge => self.book_merge(effects),
            BookMessage::SetMergeMode(mode) => self.book.merge_mode = mode,
            BookMessage::SetCreateOptions(options) => self.book.create = options.normalized(),
            BookMessage::UseAsOpening(side) => {
                let path = self
                    .book
                    .slot(side)
                    .and_then(|s| s.book.get().path().map(Path::to_path_buf));
                match path {
                    Some(path) => self.book_choose_opening(Some(path), effects),
                    None => self.book_issue(BookIssue::NoPath),
                }
            }
            BookMessage::ClearOpening => self.book_choose_opening(None, effects),
            BookMessage::DismissNotice => self.book.notice = None,
            BookMessage::SlotLoaded {
                request,
                side,
                result,
            } => {
                if !matches!(self.book.take_job(request), Some(Job::Load { side: s, .. }) if s == side)
                {
                    return;
                }
                match result {
                    Ok(book) => {
                        self.book.notice = None;
                        self.book.slots[side.index()] = Some(BookSlot::new(book, false));
                    }
                    Err(issue) => self.book_issue(issue),
                }
            }
            BookMessage::OpeningLoaded { request, result } => {
                let Some(Job::Opening { path, .. }) = self.book.take_job(request) else {
                    return;
                };
                // A choice made while the file was being read wins.
                if self.prefs.book.opening_book.as_deref() != Some(path.as_path()) {
                    return;
                }
                match result {
                    Ok(book) => {
                        self.book.opening = Some(book);
                        self.book.opening_issue = None;
                        self.book.notice = Some(BookNotice::OpeningLoaded);
                    }
                    Err(issue) => {
                        self.book.opening = None;
                        self.book.opening_issue = Some(issue);
                    }
                }
            }
            BookMessage::Saved { request, result } => {
                let Some(Job::Save { side, revision, .. }) = self.book.take_job(request) else {
                    return;
                };
                match result {
                    Ok(saved) => {
                        let reload = {
                            let path = saved.get().path();
                            path.is_some() && path == self.prefs.book.opening_book.as_deref()
                        };
                        if let Some(slot) = self.book.slot_mut(side)
                            && slot.revision == revision
                        {
                            slot.book = saved;
                            slot.unsaved = false;
                        }
                        self.book.notice = Some(BookNotice::Saved);
                        // The opening book is the file that was just written.
                        if reload {
                            self.book_request_opening(effects);
                        }
                    }
                    Err(issue) => self.book_issue(issue),
                }
            }
            BookMessage::Created { request, result } => {
                let Some(Job::Create { side, .. }) = self.book.take_job(request) else {
                    return;
                };
                match result {
                    Ok(CreatedBook {
                        book,
                        used,
                        skipped,
                    }) => {
                        let entries = book.get().len();
                        self.book.slots[side.index()] = Some(BookSlot::new(book, true));
                        self.book.notice = Some(BookNotice::Created {
                            used,
                            skipped,
                            entries,
                        });
                    }
                    Err(BookIssue::Cancelled) => {
                        self.book.notice = Some(BookNotice::CreateCancelled);
                    }
                    Err(issue) => self.book_issue(issue),
                }
            }
            BookMessage::Merged { request, result } => {
                if !matches!(self.book.take_job(request), Some(Job::Merge { .. })) {
                    return;
                }
                match result {
                    Ok(MergedBook {
                        book,
                        written,
                        skipped,
                    }) => {
                        self.book.slots[BookSide::First.index()] = Some(BookSlot::new(book, true));
                        self.book.notice = Some(BookNotice::Merged { written, skipped });
                    }
                    Err(issue) => self.book_issue(issue),
                }
            }
        }
    }

    /// Why the book of `side` cannot be replaced now.
    fn book_side_blocked(&self, side: BookSide) -> Option<BookIssue> {
        if self.book.busy_side(side) {
            Some(BookIssue::Busy)
        } else if self.book.is_dirty(side) {
            Some(BookIssue::UnsavedChanges)
        } else {
            None
        }
    }

    /// Runs `edit` on the book of `side` for the position of the cursor
    /// (D1654-D1657) and reports what it did.
    fn book_edit(
        &mut self,
        side: BookSide,
        edit: impl FnOnce(&mut Book, &Game, &Position, NodeId) -> Result<Edit, BookIssue>,
    ) {
        if self.book.busy_side(side) {
            return self.book_issue(BookIssue::Busy);
        }
        let game = self.session.game();
        let cursor = self.session.cursor();
        let Ok(position) = game.position_at(cursor) else {
            return self.book_issue(BookIssue::Internal);
        };
        if position.castling_mode() != CastlingMode::Standard {
            return self.book_issue(BookIssue::Unsupported);
        }
        let Some(slot) = self.book.slots[side.index()].as_mut() else {
            return self.book_issue(BookIssue::NoBook);
        };
        if slot.book.get().is_read_only() {
            return self.book_issue(BookIssue::ReadOnly);
        }
        match edit(slot.book.edit(), game, position, cursor) {
            Ok(done) => {
                slot.revision += 1;
                self.book.notice = match done {
                    Edit::Quiet => None,
                    Edit::Added(n) => Some(BookNotice::Added(n)),
                    Edit::Removed(n) => Some(BookNotice::Removed(n)),
                    Edit::Copied => Some(BookNotice::Copied),
                };
            }
            Err(issue) => self.book_issue(issue),
        }
    }

    /// Copies the moves of the position from the other book into `to`.
    fn book_copy(&mut self, to: BookSide) {
        let list = {
            let Ok(position) = self.session.game().position_at(self.session.cursor()) else {
                return self.book_issue(BookIssue::Internal);
            };
            match self.book.slot(to.other()) {
                Some(slot) => pairs(slot.book.get(), position),
                None => return self.book_issue(BookIssue::NoBook),
            }
        };
        if list.is_empty() {
            return self.book_issue(BookIssue::NothingToAdd);
        }
        self.book_edit(to, move |book, _, position, _| {
            book.set_position_moves(position, &list)
                .map(|()| Edit::Copied)
                .map_err(|e| BookIssue::of(&e))
        });
    }

    fn book_save(&mut self, side: BookSide, path: Option<PathBuf>, effects: &mut Vec<Effect>) {
        if self.book.busy_side(side) {
            return self.book_issue(BookIssue::Busy);
        }
        let Some(slot) = self.book.slot(side) else {
            return self.book_issue(BookIssue::NoBook);
        };
        if path.is_none() && slot.book.get().path().is_none() {
            return self.book_issue(BookIssue::NoPath);
        }
        let (book, revision) = (slot.book.clone(), slot.revision);
        let request = self.new_request();
        self.book.notice = None;
        self.book.jobs.push(Job::Save {
            request,
            side,
            revision,
        });
        effects.push(Effect::BookSave {
            request,
            book,
            path,
        });
    }

    fn book_create(&mut self, side: BookSide, effects: &mut Vec<Effect>) {
        if let Some(issue) = self.book_side_blocked(side) {
            return self.book_issue(issue);
        }
        let Some(slot) = self.bases.current_slot() else {
            return self.book_issue(BookIssue::NoGames);
        };
        let games: Vec<u32> = slot.filter().iter_games().collect();
        if games.is_empty() {
            return self.book_issue(BookIssue::NoGames);
        }
        let source = slot.source.clone();
        let request = self.new_request();
        self.book.notice = None;
        let cancel = BookCancel::new();
        self.book.jobs.push(Job::Create {
            request,
            side,
            cancel: cancel.clone(),
        });
        effects.push(Effect::BookCreate {
            request,
            source,
            games,
            options: self.book.create,
            cancel,
        });
    }

    /// Starts the export of the branch of the book of `side` that starts at
    /// the cursor (D2160-D2166).
    fn book_export(&mut self, side: BookSide, path: &Path, effects: &mut Vec<Effect>) {
        if self
            .book
            .jobs
            .iter()
            .any(|j| matches!(j, Job::Export { .. }))
        {
            return self.book_issue(BookIssue::Busy);
        }
        let Some(slot) = self.book.slot(side) else {
            return self.book_issue(BookIssue::NoBook);
        };
        let Ok(start) = self.session.game().position_at(self.session.cursor()) else {
            return self.book_issue(BookIssue::Internal);
        };
        if start.castling_mode() != CastlingMode::Standard {
            return self.book_issue(BookIssue::Unsupported);
        }
        if slot.book.get().moves(start).is_empty() {
            return self.book_issue(BookIssue::EmptyBranch);
        }
        let Some(dest) = fix_destination(path) else {
            return self.book_issue(BookIssue::BadFileName);
        };
        let (book, start) = (slot.book.clone(), BranchStart(start.clone()));
        // Everything the program reads or may write later (D2166).
        let mut protected: Vec<PathBuf> = self
            .bases
            .iter()
            .filter_map(|(_, slot)| slot.path.clone())
            .collect();
        if let GameOrigin::PgnFile { path, .. } | GameOrigin::SavedFile { path } = &self.origin {
            protected.push(path.clone());
        }
        for slot in self.book.slots.iter().flatten() {
            if let Some(path) = slot.book.get().path() {
                protected.push(path.to_path_buf());
            }
        }
        let control = OpenControl::new();
        let request = self.new_request();
        self.book.notice = None;
        self.book.jobs.push(Job::Export {
            request,
            control: control.clone(),
            stopping: false,
        });
        effects.push(Effect::BookExport {
            request,
            book,
            start,
            options: self.book.export,
            confirmed: dest == path,
            dest,
            protected,
            control,
        });
    }

    fn book_merge(&mut self, effects: &mut Vec<Effect>) {
        if self.book.busy_side(BookSide::First) {
            return self.book_issue(BookIssue::Busy);
        }
        let (Some(first), Some(second)) = (
            self.book.slot(BookSide::First),
            self.book.slot(BookSide::Second),
        ) else {
            return self.book_issue(BookIssue::NoBook);
        };
        if first.is_dirty() {
            return self.book_issue(BookIssue::UnsavedChanges);
        }
        let (first, second) = (first.book.clone(), second.book.clone());
        let request = self.new_request();
        self.book.notice = None;
        self.book.jobs.push(Job::Merge { request });
        effects.push(Effect::BookMerge {
            request,
            first,
            second,
            mode: self.book.merge_mode,
        });
    }

    /// Chooses (or clears) the opening book, saving the preference and
    /// reading the file.
    fn book_choose_opening(&mut self, path: Option<PathBuf>, effects: &mut Vec<Effect>) {
        let change = PrefChange::Book(BookPrefs { opening_book: path });
        self.prefs_message(PrefsMessage::Set(change), effects);
    }
}

/// Adds `mv` with weight 1 to the moves of `position`.
fn add_move(book: &mut Book, position: &Position, mv: Move) -> Result<Edit, BookIssue> {
    let mut list = pairs(book, position);
    if list.iter().any(|(m, _)| *m == mv) {
        return Err(BookIssue::AlreadyThere);
    }
    list.push((mv, 1));
    book.set_position_moves(position, &list)
        .map(|()| Edit::Added(1))
        .map_err(|e: BookError| BookIssue::of(&e))
}

/// The main continuation of the game after `cursor`.
fn continuation(game: &Game, cursor: NodeId) -> Vec<Move> {
    game.mainline_from(cursor)
        .map(|line| line.filter_map(|n| game.mv(n).ok()).collect())
        .unwrap_or_default()
}
