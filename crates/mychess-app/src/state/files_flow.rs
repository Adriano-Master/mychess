//! Commands, opening and saving files, the game chooser (spec M2-2,
//! D124-D131).

use std::path::PathBuf;
use std::sync::Arc;

use mychess_pgn::write::game_to_string;

use super::{AppState, PendingParse, PendingSave, SaveCheck};
use crate::chooser::{ChooserMessage, GameChooser};
use crate::effect::Effect;
use crate::error::{FileError, FileErrorKind, RequestId};
use crate::files::{
    GameOrigin, LoadedGame, PgnCollection, PgnLoad, PgnSource, SaveMode, ScanOutcome,
};
use crate::flow::{
    AppNotice, Command, DiscardPrompt, OverwritePrompt, OverwriteReason, Pending, Replacement,
};
use crate::host::PickKind;
use crate::import::{ImportIssue, ImportReport};
use crate::session::Revision;

impl AppState {
    /// Handles a [`Command`].
    pub(crate) fn command(&mut self, command: Command, effects: &mut Vec<Effect>) {
        if self.modal_open() {
            return;
        }
        // The game belongs to the run while it lasts: saving it would write
        // a half-annotated game (D1552).
        if matches!(command, Command::Save | Command::SaveAs | Command::Append)
            && self.annotation_running()
        {
            self.annotate_notice(crate::annotate::AnnotateNotice::Busy);
            return;
        }
        // The commands that replace the game or open a dialog that does.
        if matches!(
            command,
            Command::NewGame
                | Command::NewChess960(_)
                | Command::Open
                | Command::OpenRecent(_)
                | Command::NextGame
                | Command::PrevGame
                | Command::FirstGame
                | Command::LastGame
                | Command::PasteFen
                | Command::PasteMoves
                | Command::SetupBoard
        ) && self.refuse_edit()
        {
            return;
        }
        match command {
            Command::NewGame => self.begin_replace(Replacement::New, effects),
            Command::NewChess960(number) => {
                if let Ok(position) = mychess_core::Position::chess960_start(number) {
                    self.begin_replace(Replacement::FromPosition(position), effects);
                }
            }
            Command::Open => self.ask_for_file(PickKind::OpenPgn, None, effects),
            Command::OpenRecent(index) => {
                if let Some(path) = self.prefs.files.recent.get(index).cloned() {
                    self.open_path(path, effects);
                }
            }
            Command::ClearRecent => {
                self.prefs.files.recent.clear();
                self.request_prefs_save(effects);
            }
            Command::Save | Command::SaveAs | Command::Append if self.flow.save.is_some() => {
                self.notice = Some(AppNotice::SaveInProgress);
            }
            Command::Save => match &self.origin {
                GameOrigin::SavedFile { path } => {
                    let path = path.clone();
                    self.start_save(
                        path,
                        SaveMode::Overwrite,
                        None,
                        SaveCheck::default(),
                        effects,
                    );
                }
                _ => self.ask_for_file(PickKind::SavePgn, None, effects),
            },
            Command::SaveAs => self.ask_for_file(PickKind::SavePgn, None, effects),
            Command::Append => self.ask_for_file(PickKind::AppendPgn, None, effects),
            Command::NextGame => self.step_collection(|index, _| index.checked_add(1), effects),
            Command::PrevGame => self.step_collection(|index, _| index.checked_sub(1), effects),
            Command::FirstGame => self.step_collection(|_, _| Some(0), effects),
            Command::LastGame => self.step_collection(|_, total| total.checked_sub(1), effects),
            Command::CopyFen => effects.push(Effect::WriteClipboard(self.session.position().fen())),
            Command::PasteFen => effects.push(Effect::ReadClipboard(
                crate::clipboard::ClipboardPurpose::Fen,
            )),
            Command::CopyPgn => {
                let options = self.prefs.pgn_export.clipboard_options();
                match game_to_string(self.session.game(), &options) {
                    Ok(text) => effects.push(Effect::WriteClipboard(text)),
                    Err(_) => self.notice = Some(AppNotice::ClipboardWriteFailed),
                }
            }
            Command::PasteMoves => effects.push(Effect::ReadClipboard(
                crate::clipboard::ClipboardPurpose::Moves,
            )),
            Command::SetupBoard => self.open_setup(),
        }
    }

    /// Loads another game of the open file; at the ends nothing happens.
    fn step_collection(
        &mut self,
        target: impl FnOnce(usize, usize) -> Option<usize>,
        effects: &mut Vec<Effect>,
    ) {
        let GameOrigin::PgnFile { index, total, .. } = self.origin else {
            return;
        };
        let Some(collection) = self.collection.clone() else {
            return;
        };
        match target(index, total) {
            Some(next) if next != index && next < collection.len() => {
                self.start_parse(collection, next, effects);
            }
            _ => {}
        }
    }

    /// Opens a file chosen by the user: in a slot (D503) once the front-end
    /// asks for it, else through the PGN scan and the chooser (M2-2).
    pub(super) fn open_path(&mut self, path: PathBuf, effects: &mut Vec<Effect>) {
        // An EPD file goes to the EPD window, by its extension (spec M7-5,
        // D3802).
        if crate::epd_window::is_epd_path(&path) {
            return self.epd_start_open(path, false, effects);
        }
        if self.open_pgn_in_slots {
            self.start_open_base(path, effects);
        } else {
            self.start_scan(path, effects);
        }
    }

    fn start_scan(&mut self, path: PathBuf, effects: &mut Vec<Effect>) {
        let request = self.new_request();
        self.flow.scan = Some(request);
        effects.push(Effect::ScanPgn {
            request,
            source: PgnSource::File(path),
        });
    }

    fn start_parse(
        &mut self,
        collection: Arc<PgnCollection>,
        index: usize,
        effects: &mut Vec<Effect>,
    ) {
        let request = self.new_request();
        self.flow.parse = Some(PendingParse {
            request,
            collection: collection.clone(),
            index,
        });
        effects.push(Effect::ParseCollectionGame {
            request,
            collection,
            index,
        });
    }

    /// The answer of a file dialog.
    pub(crate) fn file_picked(
        &mut self,
        request: RequestId,
        path: Option<PathBuf>,
        effects: &mut Vec<Effect>,
    ) {
        if self.mask_file_picked(request, path.as_deref(), effects) {
            return;
        }
        if self.file_import_file_picked(request, path.as_deref(), effects) {
            return;
        }
        if self.games_export_file_picked(request, path.as_deref(), effects) {
            return;
        }
        if self.spelling_file_picked(request, path.clone(), effects) {
            return;
        }
        if self.epd_file_picked(request, path.clone(), effects) {
            return;
        }
        if self.flow.pick.as_ref().map(|p| p.request) != Some(request) {
            return;
        }
        let Some(pick) = self.flow.pick.take() else {
            return;
        };
        let Some(path) = path else {
            // The file dialog of "Save" before quitting was cancelled: the
            // quit is given up (D3505).
            if pick.then == Some(Pending::Quit) {
                self.spelling_quit_undone(effects);
            }
            if pick.then == Some(Pending::Maintenance) {
                self.maintenance_give_up();
            }
            return;
        };
        match pick.kind {
            PickKind::OpenPgn => self.open_path(path, effects),
            PickKind::SavePgn => {
                self.save_destination(path, SaveMode::Overwrite, pick.then, effects);
            }
            PickKind::AppendPgn => {
                self.save_destination(path, SaveMode::Append, pick.then, effects);
            }
            PickKind::OpenTreeBase => self.tree_open_base_as_tree(path, effects),
            // Asked and answered by the masks (`mask_file_picked`).
            // Also asked and answered by the import of PGN files.
            PickKind::OpenMask
            | PickKind::SaveMask
            | PickKind::ImportPgn
            | PickKind::NewBase
            | PickKind::OpenSpelling
            | PickKind::OpenEpd
            | PickKind::NewEpd
            | PickKind::SaveEpd
            // Asked and answered by the export (`games_export_file_picked`).
            | PickKind::ExportDoc { .. }
            | PickKind::ExportImage { .. } => {}
            PickKind::TablebaseDir => self.tablebase_add_directory(&path, effects),
            PickKind::OpenEco => {
                let change = crate::prefs::PrefChange::EcoFile(Some(path));
                self.prefs_message(crate::prefs::PrefsMessage::Set(change), effects);
            }
        }
    }

    /// The answer of a PGN scan: an open file or the import dialog.
    pub(crate) fn pgn_scanned(
        &mut self,
        request: RequestId,
        path: Option<PathBuf>,
        result: Result<ScanOutcome, FileError>,
        effects: &mut Vec<Effect>,
    ) {
        if self.import.as_ref().and_then(|d| d.request) == Some(request) {
            self.import_scanned(result, effects);
        } else if self.flow.scan == Some(request) {
            self.flow.scan = None;
            self.open_scanned(path, result, effects);
        }
    }

    fn open_scanned(
        &mut self,
        path: Option<PathBuf>,
        result: Result<ScanOutcome, FileError>,
        effects: &mut Vec<Effect>,
    ) {
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => {
                // Only a file that is gone leaves the list; one that cannot be
                // read right now (permissions, a bad file) may come back.
                if error.kind == FileErrorKind::NotFound
                    && let Some(path) = path
                {
                    self.prefs.files.recent.remove(&path);
                    self.prefs_dirty = true;
                }
                self.notice = Some(AppNotice::File(error));
                return;
            }
        };
        let canonical = outcome.path.clone().or(path);
        if let Some(canonical) = &canonical {
            self.prefs.files.recent.push(canonical.clone());
            self.prefs.files.last_open_dir = canonical.parent().map(PathBuf::from);
            self.prefs_dirty = true;
        }
        match outcome.load {
            PgnLoad::Single(loaded) => {
                let partial = outcome.notices.iter().any(|n| {
                    matches!(
                        n.issue,
                        ImportIssue::Truncated { .. } | ImportIssue::TextOutsideGames { .. }
                    )
                });
                let mut report = loaded.report.clone();
                report.notices.extend(outcome.notices);
                self.open_report = Some(report);
                let origin = canonical.map_or(GameOrigin::Clipboard, |path| GameOrigin::PgnFile {
                    path,
                    index: 0,
                    total: 1,
                    partial,
                });
                self.begin_replace(
                    Replacement::Loaded {
                        game: loaded.game,
                        origin,
                        dirty: false,
                        collection: None,
                    },
                    effects,
                );
            }
            PgnLoad::Many(collection) => {
                self.open_report = Some(ImportReport {
                    notices: outcome.notices,
                    ..ImportReport::default()
                });
                self.open_chooser(collection);
            }
            PgnLoad::None { .. } => {}
        }
    }

    /// Shows the chooser and learns the names of the collection.
    pub(crate) fn open_chooser(&mut self, collection: Arc<PgnCollection>) {
        for header in collection.headers() {
            self.names.observe_header(header);
        }
        self.chooser = Some(GameChooser::new(collection));
    }

    pub(crate) fn chooser_message(&mut self, message: &ChooserMessage, effects: &mut Vec<Effect>) {
        if *message == ChooserMessage::Cancel {
            self.chooser = None;
            return;
        }
        let Some(chooser) = self.chooser.as_mut() else {
            return;
        };
        if let Some(index) = chooser.update(message) {
            let collection = chooser.collection.clone();
            self.start_parse(collection, index, effects);
        }
    }

    pub(crate) fn collection_game_parsed(
        &mut self,
        request: RequestId,
        result: Result<LoadedGame, FileError>,
        effects: &mut Vec<Effect>,
    ) {
        if self.flow.parse.as_ref().map(|p| p.request) != Some(request) {
            return;
        }
        let Some(pending) = self.flow.parse.take() else {
            return;
        };
        let loaded = match result {
            Ok(loaded) => loaded,
            Err(error) => {
                self.notice = Some(AppNotice::File(error));
                return;
            }
        };
        let (origin, dirty) = match pending.collection.path() {
            Some(path) => (
                GameOrigin::PgnFile {
                    path: path.to_path_buf(),
                    index: pending.index,
                    total: pending.collection.len(),
                    partial: collection_is_partial(&pending.collection),
                },
                false,
            ),
            // A collection from pasted text counts as unsaved.
            None => (GameOrigin::Clipboard, true),
        };
        self.open_report = Some(loaded.report.clone());
        self.begin_replace(
            Replacement::Loaded {
                game: loaded.game,
                origin,
                dirty,
                collection: Some(pending.collection),
            },
            effects,
        );
    }

    /// The answer of a save.
    pub(crate) fn pgn_saved(
        &mut self,
        request: RequestId,
        mode: SaveMode,
        result: Result<PathBuf, FileError>,
        effects: &mut Vec<Effect>,
    ) {
        if self.flow.save.as_ref().map(|s| s.request) != Some(request) {
            return;
        }
        let Some(save) = self.flow.save.take() else {
            return;
        };
        let path = match result {
            Ok(path) => path,
            Err(error) => {
                let quit = save.then == Some(Pending::Quit);
                let maintenance = save.then == Some(Pending::Maintenance);
                self.save_refused(save, mode, error);
                // The operation that waited for the save is dropped too.
                if maintenance && self.overwrite_prompt.is_none() {
                    self.maintenance_give_up();
                }
                // A failed save gives the quit up, unless the question about
                // overwriting is open (D3505).
                if quit && self.overwrite_prompt.is_none() {
                    self.spelling_quit_undone(effects);
                }
                return;
            }
        };
        let unchanged = self.session.revision() == save.revision;
        self.mark_saved_if_unchanged(save.revision);
        self.prefs.files.recent.push(path.clone());
        self.prefs.files.last_save_dir = path.parent().map(PathBuf::from);
        self.prefs_dirty = true;
        // Only a game that is still the one that was written takes the new
        // origin: after a switch of slot or a new game the editor holds
        // something else.
        if unchanged {
            self.names.observe_tags(self.session.game().tags());
        }
        if unchanged && mode == SaveMode::Overwrite {
            self.origin = GameOrigin::SavedFile { path };
            self.collection = None;
        }
        match save.then {
            Some(pending) if unchanged => self.proceed(pending, effects),
            // The game changed while it was being written: what the user
            // agreed to save is not what is on screen, so ask again.
            Some(pending) => self.discard_prompt = Some(DiscardPrompt { pending }),
            None => {}
        }
    }

    /// A save failed, or `perform` refused it until the user agrees.
    fn save_refused(&mut self, save: PendingSave, mode: SaveMode, error: FileError) {
        let (games, reason) = match (&error.kind, &save.check.protected) {
            (FileErrorKind::NeedsConfirmation, Some(p)) => (p.games, p.reason),
            (FileErrorKind::AlreadyExists, _) if save.check.create_only => {
                (0, OverwriteReason::ExistingFile)
            }
            _ => {
                self.notice = Some(AppNotice::File(error));
                return;
            }
        };
        if mode == SaveMode::Overwrite {
            self.overwrite_prompt = Some(OverwritePrompt {
                path: save.path,
                games,
                reason,
                then: save.then,
            });
        } else {
            self.notice = Some(AppNotice::File(error));
        }
    }

    /// The game counts as saved only if it is the one that was written.
    fn mark_saved_if_unchanged(&mut self, revision: Revision) {
        if self.session.revision() == revision {
            self.session.mark_saved();
        }
    }

    /// Hands the quit to the preferences flow after the question.
    pub(crate) fn quit(&mut self, effects: &mut Vec<Effect>) {
        if self.discard_prompt.is_some() {
            return;
        }
        self.flow.quit_asked.clear();
        self.mask_quit_reset();
        // The run ends first: its edits stay, so the question about
        // unsaved changes is asked as usual (D897).
        if self.annotation_running() {
            let mut stop = Vec::new();
            self.annotate_finish(crate::annotate::Outcome::Cancelled, true, &mut stop);
            effects.extend(stop);
            // Between two games of a batch there is no run to finish: the
            // batch itself is cancelled, and the quit waits for its step
            // (D1550).
            if let Some(batch) = self.flow.annotate_batch.as_mut() {
                batch.cancelled = true;
            }
        }
        // The browser is a layer over the program: leaving closes it and
        // cancels the games it was reading (D610).
        self.close_browser();
        if self.flow.save.as_ref().is_some_and(|s| s.then.is_some()) {
            // A save that something waits for is being written; its own
            // outcome decides what happens next.
            self.notice = Some(AppNotice::SaveInProgress);
            // Unless that save is for the quit, this one is given up (D3505).
            if self.flow.save.as_ref().and_then(|s| s.then.as_ref()) != Some(&Pending::Quit) {
                self.spelling_quit_undone(effects);
            }
            return;
        }
        self.proceed_or_ask(Pending::Quit, effects);
    }
}

/// Whether a collection read from a file lacks part of the file: reading
/// stopped at a limit, or text outside the games was skipped.
fn collection_is_partial(collection: &PgnCollection) -> bool {
    collection.truncated().is_some() || collection.skipped_bytes() > 0
}
