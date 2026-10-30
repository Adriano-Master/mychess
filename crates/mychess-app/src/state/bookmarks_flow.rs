//! Bookmarks: adding, going to a game, the editor and the file (spec
//! M3-6, D671-D680; Scid: `tcl/file/bookmark.tcl`).
//!
//! Pure bookkeeping: the file and the search for the game are
//! [`Effect`]s.

use std::path::Path;

use super::{AppState, PendingBookmarkFind};
use crate::bases::BaseError;
use crate::bookmarks::{
    BookmarkEditor, BookmarkId, BookmarkNotice, Bookmarks, BookmarksIssue, BookmarksMessage,
    EditorMessage, Folder, GameBookmark, LoadedBookmarks, NEW_FOLDER_TITLE, NodeKind, TreeError,
    bookmarks_path, default_title,
};
use crate::effect::Effect;
use crate::error::{FileError, RequestId};
use crate::flow::{AppNotice, BrowserLanding};
use crate::session::{GameMessage, Nav};

impl AppState {
    /// Why the game being edited cannot be bookmarked, or the bookmark
    /// (D673): it must be a game of an open database other than the
    /// Clipbase, whose path is valid UTF-8.
    ///
    /// # Errors
    ///
    /// The notice that explains the refusal.
    pub fn current_bookmark(&self) -> Result<GameBookmark, BookmarkNotice> {
        let id = self.bases.current();
        if id.is_clipbase() {
            return Err(BookmarkNotice::CannotAdd);
        }
        let slot = self.bases.slot(id).ok_or(BookmarkNotice::CannotAdd)?;
        let (Some(path), Some(number)) = (slot.path.as_ref(), slot.current_game) else {
            return Err(BookmarkNotice::CannotAdd);
        };
        if path.to_str().is_none() {
            return Err(BookmarkNotice::PathNotUtf8);
        }
        let game = self.session.game();
        let tags = game.tags();
        // The ply on the main line: a cursor inside a variation counts the
        // main-line moves before the variation (D672).
        let ply = game.path_to(self.session.cursor()).map_or(0, |nodes| {
            nodes
                .iter()
                .take_while(|n| !game.is_variation(**n).unwrap_or(true))
                .count()
        });
        let mut bookmark = GameBookmark {
            title: String::new(),
            base: path.clone(),
            game: number,
            ply: u32::try_from(ply).unwrap_or(u32::MAX),
            white: tags.white.clone(),
            black: tags.black.clone(),
            year: tags.date.year().unwrap_or(0),
            site: tags.site.clone(),
            round: tags.round.clone(),
            result: tags.result.as_str().to_owned(),
        };
        bookmark.title = default_title(&bookmark);
        Ok(bookmark)
    }

    /// Whether the game being edited can be bookmarked.
    pub fn can_bookmark_current(&self) -> bool {
        self.current_bookmark().is_ok()
    }

    pub(crate) fn bookmarks_message(
        &mut self,
        message: BookmarksMessage,
        effects: &mut Vec<Effect>,
    ) {
        match message {
            // Answers to effects: always handled.
            BookmarksMessage::Loaded(loaded) => self.bookmarks_loaded(loaded, effects),
            BookmarksMessage::Saved(result) => self.bookmarks_saved(&result, effects),
            BookmarksMessage::Found { request, result } => {
                self.bookmark_found(request, result, effects);
            }
            BookmarksMessage::Edit(message) => self.editor_message(message, effects),
            _ if self.modal_open() => {}
            BookmarksMessage::AddCurrent => self.add_current_bookmark(effects),
            BookmarksMessage::Go(id) => self.go_to_bookmark(id, effects),
            BookmarksMessage::OpenEditor => {
                if !self.flow.bookmarks_loading {
                    self.bookmark_editor = Some(BookmarkEditor {
                        working: self.bookmarks.clone(),
                        selected: None,
                        title: String::new(),
                    });
                }
            }
        }
    }

    fn notice_bookmark(&mut self, notice: BookmarkNotice) {
        self.notice = Some(AppNotice::Bookmark(notice));
    }

    fn add_current_bookmark(&mut self, effects: &mut Vec<Effect>) {
        let bookmark = match self.current_bookmark() {
            Ok(bookmark) => bookmark,
            Err(notice) => return self.notice_bookmark(notice),
        };
        if self.flow.bookmarks_loading {
            self.flow.bookmarks_added_early.push(bookmark.clone());
        }
        match self
            .bookmarks
            .insert_before_first_folder(NodeKind::Game(bookmark))
        {
            Ok(_) => {
                self.notice_bookmark(BookmarkNotice::Added);
                self.request_bookmarks_save(effects);
            }
            Err(_) => self.notice_bookmark(BookmarkNotice::TreeFull),
        }
    }

    // ---- going to a bookmark ----

    fn go_to_bookmark(&mut self, id: BookmarkId, effects: &mut Vec<Effect>) {
        let Some(bookmark) = self.bookmarks.get(id).and_then(|node| node.game()).cloned() else {
            return;
        };
        self.flow.bookmark_go = None;
        if let Some(slot) = self.bases.find_by_path(&bookmark.base) {
            if self.bases.current() != slot && self.switching_blocked_for_bookmark() {
                return;
            }
            self.select_slot(slot);
            self.bookmark_search(slot, bookmark, effects);
            return;
        }
        self.start_open_base(bookmark.base.clone(), effects);
        // Only a request that really started waits for the database.
        if self
            .flow
            .open_base
            .as_ref()
            .is_some_and(|open| open.path == bookmark.base)
        {
            self.flow.bookmark_go = Some(bookmark);
        }
    }

    fn switching_blocked_for_bookmark(&self) -> bool {
        self.modal_open() || self.overwrite_prompt.is_some()
    }

    /// The pending bookmark for the database at `path`, if it is the one
    /// being opened.
    pub(crate) fn take_bookmark_go(&mut self, path: &Path) -> Option<GameBookmark> {
        if self
            .flow
            .bookmark_go
            .as_ref()
            .is_some_and(|b| b.base == path)
        {
            self.flow.bookmark_go.take()
        } else {
            None
        }
    }

    /// Starts the search for the game of `bookmark` in slot `slot`.
    pub(crate) fn bookmark_search(
        &mut self,
        slot: crate::bases::SlotId,
        bookmark: GameBookmark,
        effects: &mut Vec<Effect>,
    ) {
        let Some(source) = self.bases.slot(slot).map(|s| s.source.clone()) else {
            return;
        };
        let request = self.new_request();
        self.flow.bookmark_find = Some(PendingBookmarkFind {
            request,
            slot,
            bookmark: bookmark.clone(),
        });
        effects.push(Effect::FindBookmarkGame {
            request,
            source,
            bookmark: Box::new(bookmark),
        });
    }

    fn bookmark_found(
        &mut self,
        request: RequestId,
        result: Result<Option<u32>, BaseError>,
        effects: &mut Vec<Effect>,
    ) {
        if self.flow.bookmark_find.as_ref().map(|f| f.request) != Some(request) {
            return;
        }
        let Some(pending) = self.flow.bookmark_find.take() else {
            return;
        };
        let title = pending.bookmark.title.clone();
        let number = match result {
            Ok(Some(number)) => number,
            Ok(None) => return self.notice_bookmark(BookmarkNotice::GameNotFound { title }),
            Err(error) => {
                self.notice = Some(AppNotice::Bases(crate::bases::BasesNotice::Failed(error)));
                return;
            }
        };
        if self.bases.slot(pending.slot).is_none() {
            return;
        }
        if self.bases.current() != pending.slot {
            if self.switching_blocked_for_bookmark() {
                self.notice = Some(AppNotice::Bases(crate::bases::BasesNotice::NotActivated));
                return;
            }
            self.select_slot(pending.slot);
        }
        let already = self
            .bases
            .slot(pending.slot)
            .is_some_and(|s| s.current_game == Some(number));
        if already {
            // The game is in the editor: only move to the position.
            drop(
                self.session
                    .update(GameMessage::Navigate(Nav::GoToMainlinePly(
                        pending.bookmark.ply,
                    ))),
            );
            return;
        }
        let landing = BrowserLanding {
            ply: usize::try_from(pending.bookmark.ply).unwrap_or(usize::MAX),
            close_browser: false,
            flipped: self.board_flipped,
            browser: RequestId(0),
        };
        self.request_base_game(pending.slot, number, Some(landing), effects);
    }

    // ---- the file ----

    fn bookmarks_loaded(&mut self, loaded: LoadedBookmarks, effects: &mut Vec<Effect>) {
        self.flow.bookmarks_loading = false;
        self.bookmarks = loaded.bookmarks;
        let early = std::mem::take(&mut self.flow.bookmarks_added_early);
        let added = !early.is_empty();
        // Each one goes before the first folder, after the ones added before
        // it, so the order of addition is kept (D705).
        for bookmark in early {
            self.bookmarks
                .insert_before_first_folder(NodeKind::Game(bookmark))
                .ok();
        }
        match loaded.issue {
            Some(BookmarksIssue::Unreadable) => {
                self.notice_bookmark(BookmarkNotice::FileUnreadable);
            }
            Some(BookmarksIssue::NewerVersion(_)) => {
                self.notice_bookmark(BookmarkNotice::NewerVersion);
            }
            Some(BookmarksIssue::Dropped(count)) => {
                self.notice_bookmark(BookmarkNotice::Dropped { count });
            }
            None => {}
        }
        if added {
            self.request_bookmarks_save(effects);
        }
    }

    /// Asks for a save; at most one is in flight and a request made
    /// meanwhile becomes one more save with the newest list (D677).
    pub(crate) fn request_bookmarks_save(&mut self, effects: &mut Vec<Effect>) {
        let Some(prefs) = self.flow.prefs_path.as_deref() else {
            return;
        };
        if self.flow.bookmarks_loading {
            // `bookmarks_loaded` saves what was added meanwhile.
            return;
        }
        if self.flow.bookmarks_save_in_flight {
            self.flow.bookmarks_resave = true;
            return;
        }
        self.flow.bookmarks_save_in_flight = true;
        effects.push(Effect::SaveBookmarks {
            path: bookmarks_path(prefs),
            bookmarks: Box::new(self.bookmarks.clone()),
        });
    }

    fn bookmarks_saved(&mut self, result: &Result<(), FileError>, effects: &mut Vec<Effect>) {
        self.flow.bookmarks_save_in_flight = false;
        if result.is_err() {
            self.notice_bookmark(BookmarkNotice::SaveFailed);
        }
        if std::mem::take(&mut self.flow.bookmarks_resave) {
            self.request_bookmarks_save(effects);
        } else if std::mem::take(&mut self.flow.quit_waiting_bookmarks) {
            self.finish_quit(effects);
        }
    }

    // ---- the editor ----

    fn editor_message(&mut self, message: EditorMessage, effects: &mut Vec<Effect>) {
        if self.bookmark_editor.is_none() {
            return;
        }
        let current = self.current_bookmark();
        let Some(editor) = self.bookmark_editor.as_mut() else {
            return;
        };
        let mut notice = None;
        match message {
            EditorMessage::Select(id) => {
                let found =
                    id.and_then(|id| editor.working.get(id).map(|n| (id, n.title().to_owned())));
                (editor.selected, editor.title) = match found {
                    Some((id, title)) => (Some(id), title),
                    None => (None, String::new()),
                };
            }
            EditorMessage::Rename(text) => {
                if let Some(id) = editor.selected {
                    editor.working.rename(id, &text).ok();
                    editor.title = text
                        .chars()
                        .take(crate::bookmarks::MAX_TITLE_CHARS)
                        .collect();
                }
            }
            EditorMessage::MoveUp => edit(editor, Bookmarks::move_up, &mut notice),
            EditorMessage::MoveDown => edit(editor, Bookmarks::move_down, &mut notice),
            EditorMessage::Indent => edit(editor, Bookmarks::indent, &mut notice),
            EditorMessage::Outdent => edit(editor, Bookmarks::outdent, &mut notice),
            EditorMessage::NewFolder => {
                let kind = NodeKind::Folder(Folder {
                    title: NEW_FOLDER_TITLE.to_owned(),
                    children: Vec::new(),
                });
                insert(editor, kind, &mut notice);
            }
            EditorMessage::AddCurrent => match current {
                Ok(bookmark) => insert(editor, NodeKind::Game(bookmark), &mut notice),
                Err(refusal) => notice = Some(refusal),
            },
            EditorMessage::Delete => {
                if let Some(id) = editor.selected.take() {
                    editor.working.remove(id).ok();
                    editor.title.clear();
                }
            }
            EditorMessage::Cancel => self.bookmark_editor = None,
            EditorMessage::Ok => self.commit_editor(effects),
            EditorMessage::Go => {
                let selected = self.bookmark_editor.as_ref().and_then(|e| {
                    let id = e.selected?;
                    e.working.get(id)?.game().map(|_| id)
                });
                if let Some(id) = selected {
                    self.commit_editor(effects);
                    self.bookmarks_message(BookmarksMessage::Go(id), effects);
                }
            }
        }
        if let Some(notice) = notice {
            self.notice_bookmark(notice);
        }
    }

    fn commit_editor(&mut self, effects: &mut Vec<Effect>) {
        let Some(editor) = self.bookmark_editor.take() else {
            return;
        };
        let mut working = editor.working;
        working.fill_empty_titles();
        if working != self.bookmarks {
            self.bookmarks = working;
            self.request_bookmarks_save(effects);
        }
    }
}

fn edit(
    editor: &mut BookmarkEditor,
    operation: fn(&mut Bookmarks, BookmarkId) -> Result<(), TreeError>,
    notice: &mut Option<BookmarkNotice>,
) {
    if let Some(id) = editor.selected
        && operation(&mut editor.working, id) == Err(TreeError::TooDeep)
    {
        *notice = Some(BookmarkNotice::TreeFull);
    }
}

fn insert(editor: &mut BookmarkEditor, kind: NodeKind, notice: &mut Option<BookmarkNotice>) {
    match editor.working.insert_after(editor.selected, kind) {
        Ok(id) => {
            editor.title = editor
                .working
                .get(id)
                .map(|n| n.title().to_owned())
                .unwrap_or_default();
            editor.selected = Some(id);
        }
        Err(_) => *notice = Some(BookmarkNotice::TreeFull),
    }
}
