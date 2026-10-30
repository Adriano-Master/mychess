//! The dialogs: set up position, PGN import, game information; and the
//! clipboard answers (spec M2-2, D105-D123, D133-D140).

use mychess_core::{ClassifyOptions, PartialDate};
use mychess_pgn::write::{LineWidth, game_to_string};

use super::AppState;
use crate::clipboard::{ClipboardError, ClipboardPurpose, read_pasted_fen};
use crate::effect::Effect;
use crate::error::{FileError, RequestId};
use crate::files::{PgnLoad, PgnSource, ScanOutcome};
use crate::flow::{AppNotice, MovetextOutcome, Replacement};
use crate::game_info::{
    DateDraft, DatePart, FieldIssue, FieldIssueKind, GameInfoDialog, GameInfoMessage,
    IssueSeverity, TagField,
};
use crate::import::{ImportDialog, ImportMessage, ImportReport};
use crate::setup::{SetupAnalysis, SetupMessage};

/// How much clipboard text goes into a FEN field of the setup dialog.
const FEN_FIELD_CHARS: usize = 4096;

impl AppState {
    // ---- set up position ----

    pub(crate) fn setup_message(&mut self, message: SetupMessage, effects: &mut Vec<Effect>) {
        if self.setup.is_none() {
            return;
        }
        match message {
            SetupMessage::Cancel => self.setup = None,
            SetupMessage::PasteFen => {
                effects.push(Effect::ReadClipboard(ClipboardPurpose::SetupFen));
            }
            SetupMessage::Ok => {
                let Some(setup) = self.setup.as_mut() else {
                    return;
                };
                let SetupAnalysis::Valid { position, .. } = &setup.analysis else {
                    return;
                };
                let position = position.clone();
                setup.remember_current();
                self.setup_history.clone_from(&setup.fen_history);
                // The dialog stays open until the replacement is applied, so
                // "Cancel" in the question brings the user back to it.
                self.begin_replace(Replacement::FromPosition(position), effects);
            }
            other => {
                if let Some(setup) = self.setup.as_mut() {
                    setup.apply(other);
                }
            }
        }
    }

    // ---- clipboard ----

    pub(crate) fn clipboard_read(
        &mut self,
        purpose: ClipboardPurpose,
        result: Result<String, ClipboardError>,
        effects: &mut Vec<Effect>,
    ) {
        let checked = result.and_then(|text| ClipboardError::check(&text).map(|()| text));
        let text = match checked {
            Ok(text) => text,
            Err(ClipboardError::Empty) if purpose == ClipboardPurpose::ImportPrefill => return,
            Err(error) => {
                self.notice = Some(AppNotice::Clipboard(error));
                return;
            }
        };
        match purpose {
            ClipboardPurpose::Fen => match read_pasted_fen(&text) {
                Ok(position) => self.begin_replace(Replacement::FromPosition(position), effects),
                Err(error) => self.notice = Some(AppNotice::PasteFen(error)),
            },
            ClipboardPurpose::SetupFen => {
                if let Some(setup) = self.setup.as_mut() {
                    let field: String = text.chars().take(FEN_FIELD_CHARS).collect();
                    setup.apply(SetupMessage::FenEdited(field));
                    setup.apply(SetupMessage::ApplyFenText);
                }
            }
            ClipboardPurpose::ImportPrefill => {
                if let Some(dialog) = self.import.as_mut()
                    && dialog.text.is_empty()
                {
                    dialog.text = text;
                }
            }
            ClipboardPurpose::Moves => {
                let request = self.new_request();
                self.flow.movetext = Some(request);
                effects.push(Effect::ParseMovetext {
                    request,
                    start: self.session.position().clone(),
                    text,
                });
            }
        }
    }

    pub(crate) fn movetext_parsed(&mut self, request: RequestId, outcome: MovetextOutcome) {
        if self.flow.movetext == Some(request) {
            self.flow.movetext = None;
            self.pasted_moves = Some(outcome);
        }
    }

    // ---- PGN import ----

    pub(crate) fn import_message(&mut self, message: ImportMessage, effects: &mut Vec<Effect>) {
        match message {
            ImportMessage::Open => {
                self.import = Some(ImportDialog::default());
                effects.push(Effect::ReadClipboard(ClipboardPurpose::ImportPrefill));
            }
            ImportMessage::Close => self.import = None,
            ImportMessage::TextEdited(text) => {
                if let Some(dialog) = self.import.as_mut() {
                    dialog.text = text;
                }
            }
            ImportMessage::PasteCurrentGame => {
                let options = self
                    .prefs
                    .pgn_export
                    .write_options()
                    .with_width(LineWidth::Columns(70));
                let Ok(pgn) = game_to_string(self.session.game(), &options) else {
                    return;
                };
                if let Some(dialog) = self.import.as_mut() {
                    if !dialog.text.is_empty() && !dialog.text.ends_with('\n') {
                        dialog.text.push('\n');
                    }
                    dialog.text.push_str(&pgn);
                }
            }
            ImportMessage::Clear => {
                if let Some(dialog) = self.import.as_mut() {
                    *dialog = ImportDialog::default();
                }
            }
            ImportMessage::Import => {
                let request = self.new_request();
                if let Some(dialog) = self.import.as_mut() {
                    dialog.request = Some(request);
                    effects.push(Effect::ScanPgn {
                        request,
                        source: PgnSource::Text(dialog.text.clone()),
                    });
                }
            }
        }
    }

    /// The scan of the import dialog finished.
    pub(crate) fn import_scanned(
        &mut self,
        result: Result<ScanOutcome, FileError>,
        effects: &mut Vec<Effect>,
    ) {
        if let Some(dialog) = self.import.as_mut() {
            dialog.request = None;
        }
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => {
                self.notice = Some(AppNotice::File(error));
                return;
            }
        };
        match outcome.load {
            PgnLoad::Single(loaded) => {
                let mut report = loaded.report.clone();
                report.notices.extend(outcome.notices);
                self.set_import_report(report.clone());
                self.open_report = Some(report);
                self.begin_replace(
                    Replacement::Loaded {
                        game: loaded.game,
                        origin: crate::files::GameOrigin::Clipboard,
                        dirty: true,
                        collection: None,
                    },
                    effects,
                );
            }
            PgnLoad::Many(collection) => {
                self.set_import_report(ImportReport {
                    notices: outcome.notices,
                    ..ImportReport::default()
                });
                self.open_chooser(collection);
            }
            PgnLoad::None { .. } => {
                self.set_import_report(ImportReport {
                    notices: outcome.notices,
                    ..ImportReport::default()
                });
            }
        }
    }

    fn set_import_report(&mut self, report: ImportReport) {
        if let Some(dialog) = self.import.as_mut() {
            dialog.report = Some(report);
        }
    }

    // ---- game information ----

    pub(crate) fn info_message(&mut self, message: GameInfoMessage, effects: &mut Vec<Effect>) {
        match message {
            GameInfoMessage::Open => {
                if self.discard_prompt.is_none() && self.setup.is_none() {
                    self.info_dialog = Some(GameInfoDialog::new(
                        self.session.game().tags(),
                        self.last_tags.clone(),
                    ));
                }
            }
            GameInfoMessage::Cancel => self.info_dialog = None,
            GameInfoMessage::Apply => self.apply_tags(),
            GameInfoMessage::Today(target) => {
                let request = self.new_request();
                if let Some(dialog) = self.info_dialog.as_mut() {
                    dialog.pending_today = Some((request, target));
                    effects.push(Effect::QueryToday { request });
                }
            }
            GameInfoMessage::ClassifyEco => self.classify_eco(),
            GameInfoMessage::UsePrevious => {
                if let Some(dialog) = self.info_dialog.as_mut() {
                    dialog.use_previous();
                    dialog.revalidate();
                }
            }
            other => {
                if let Some(dialog) = self.info_dialog.as_mut()
                    && dialog.edit(&other, &self.names)
                {
                    dialog.revalidate();
                }
            }
        }
    }

    /// "Apply": one undo step, or the problems when something blocks.
    fn apply_tags(&mut self) {
        // The draft would overwrite the tags the run adds (D897).
        if self.refuse_edit() || self.refuse_shootout_edit() {
            return;
        }
        let Some(dialog) = self.info_dialog.as_mut() else {
            return;
        };
        let validation = dialog.draft.validate();
        dialog.issues = validation.issues;
        let Some(tags) = validation.tags else {
            return;
        };
        let edited = self.session.edit(|game, _| {
            *game.tags_mut() = tags.clone();
            Ok(())
        });
        if edited.is_ok() {
            self.names.observe_tags(&tags);
            self.info_dialog = None;
        }
    }

    fn classify_eco(&mut self) {
        let Some(dialog) = self.info_dialog.as_mut() else {
            return;
        };
        let Some(book) = &self.eco_book else {
            dialog.issues = vec![FieldIssue {
                field: TagField::Eco,
                severity: IssueSeverity::Warning,
                kind: FieldIssueKind::EcoBookNotLoaded,
            }];
            return;
        };
        dialog.issues.retain(|i| i.field != TagField::Eco);
        if let Some(found) = book.classify(self.session.game(), ClassifyOptions::game()) {
            dialog.draft.eco = found.eco(true).to_string();
        }
    }

    pub(crate) fn today(&mut self, request: RequestId, date: PartialDate) {
        if self.eco_today(request, date) {
            return;
        }
        let Some(dialog) = self.info_dialog.as_mut() else {
            return;
        };
        let Some((pending, target)) = dialog.pending_today else {
            return;
        };
        if pending != request {
            return;
        }
        dialog.pending_today = None;
        let parts = DateDraft::from_date(date);
        for (part, text) in [
            (DatePart::Year, parts.year),
            (DatePart::Month, parts.month),
            (DatePart::Day, parts.day),
        ] {
            dialog.edit(&GameInfoMessage::SetDate(target, part, text), &self.names);
        }
        dialog.revalidate();
    }
}
