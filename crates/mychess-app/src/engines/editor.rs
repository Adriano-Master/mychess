//! The engine editor as pure state (spec M4-3, "Editar"): the text fields as
//! the user types them, the check of the whole form and the results of the
//! disk check and of the test.

use std::path::PathBuf;

use mychess_engine::{EngineIdentity, Protocol};

use super::args::{join_args, split_args};
use super::check::CommandWarning;
use super::error::{ArgsError, ValidationError};
use super::model::{EngineDraft, EngineEntry, EngineId, Hotkey};
use super::run::EngineFailure;
use crate::error::RequestId;

/// Why the form cannot be saved.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditorError {
    /// A field is not acceptable.
    Invalid(ValidationError),
    /// The arguments have an unclosed quote.
    Args(ArgsError),
    /// The Elo is not a number from 0 to 65535 (the limit, 4000, is
    /// [`ValidationError::EloOutOfRange`]).
    EloNotANumber,
}

impl EditorError {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            EditorError::Invalid(ValidationError::EmptyName) => "engines-error-empty-name",
            EditorError::Invalid(ValidationError::EmptyCommand) => "engines-error-empty-command",
            EditorError::Invalid(ValidationError::EloOutOfRange(_)) => "engines-error-elo-range",
            EditorError::Invalid(ValidationError::NonUtf8Path) => "engines-error-path-not-utf8",
            EditorError::Invalid(_) => "engines-error-too-long",
            EditorError::Args(_) => "engines-error-args-quote",
            EditorError::EloNotANumber => "engines-error-elo-number",
        }
    }
}

/// The state of the "Test engine" button.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TestState {
    /// The probe is running.
    Running,
    /// The engine answered: who it is (and, for UCI, its options).
    Passed(Box<EngineIdentity>),
    /// The probe failed.
    Failed(EngineFailure),
}

/// The state of the "Detect protocol" button.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DetectState {
    /// The detection is running.
    Running,
    /// The protocol found; a suggestion, applied by
    /// [`EditorMessage::UseDetected`].
    Found(Protocol),
    /// Neither protocol answered.
    Failed(EngineFailure),
}

/// An edit of the editor form.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EditorMessage {
    /// The name field.
    SetName(String),
    /// The command field (also what "Browse" fills).
    SetCommand(String),
    /// The working directory field (empty: the engine's folder).
    SetWorkingDir(String),
    /// The arguments, as one line of text.
    SetArgs(String),
    /// The web page.
    SetUrl(String),
    /// The Elo, as text (empty or `0`: unknown).
    SetElo(String),
    /// UCI or XBoard.
    SetProtocol(Protocol),
    /// The key (F2-F4) or none.
    SetHotkey(Option<Hotkey>),
    /// Checks the command and the folder on disk (a warning, not a block).
    Check,
    /// Starts the engine, asks who it is and ends it.
    Test,
    /// Tries UCI and then XBoard.
    Detect,
    /// Applies the protocol that detection found.
    UseDetected,
    /// Validates and saves.
    Save,
    /// Closes without saving.
    Cancel,
}

/// The engine form. `id` is `None` for a new engine.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineEditor {
    /// The engine being edited; `None` for a new one.
    pub id: Option<EngineId>,
    /// Name\*.
    pub name: String,
    /// Command\*.
    pub command: String,
    /// Working directory.
    pub working_dir: String,
    /// Arguments as one line (see [`split_args`]).
    pub args: String,
    /// Web page.
    pub url: String,
    /// Elo, as typed.
    pub elo: String,
    /// The protocol.
    pub protocol: Protocol,
    /// The key.
    pub hotkey: Option<Hotkey>,
    /// Why the last save was refused.
    pub error: Option<EditorError>,
    /// What the disk check found (empty: nothing, or not checked).
    pub warnings: Vec<CommandWarning>,
    /// The result of the last "Test engine".
    pub test: Option<TestState>,
    /// The result of the last "Detect protocol".
    pub detect: Option<DetectState>,
    /// The fields the form does not show (saved options, last use,
    /// `MultiPV`, priority): kept as they are.
    pub(crate) base: EngineDraft,
    pub(crate) check_request: Option<RequestId>,
    pub(crate) test_request: Option<RequestId>,
    pub(crate) detect_request: Option<RequestId>,
}

impl EngineEditor {
    /// A form for a new engine.
    pub fn new_engine() -> EngineEditor {
        EngineEditor::from_draft(None, &EngineDraft::new("", ""), None)
    }

    /// A form for `entry`, with the key it has.
    pub fn for_entry(entry: &EngineEntry, hotkey: Option<Hotkey>) -> EngineEditor {
        EngineEditor::from_draft(Some(entry.id), &entry.to_draft(), hotkey)
    }

    fn from_draft(
        id: Option<EngineId>,
        draft: &EngineDraft,
        hotkey: Option<Hotkey>,
    ) -> EngineEditor {
        let text = |p: &PathBuf| p.to_string_lossy().into_owned();
        EngineEditor {
            id,
            name: draft.name.clone(),
            command: text(&draft.command),
            working_dir: draft.working_dir.as_ref().map(text).unwrap_or_default(),
            args: join_args(&draft.args),
            url: draft.url.clone().unwrap_or_default(),
            elo: draft.elo.map(|e| e.to_string()).unwrap_or_default(),
            protocol: draft.protocol,
            hotkey,
            error: None,
            warnings: Vec::new(),
            test: None,
            detect: None,
            base: draft.clone(),
            check_request: None,
            test_request: None,
            detect_request: None,
        }
    }

    /// Applies a field edit. Any edit clears the old error and the results
    /// that no longer describe the form.
    pub(crate) fn set(&mut self, message: &EditorMessage) {
        self.error = None;
        match message {
            EditorMessage::SetName(v) => self.name.clone_from(v),
            EditorMessage::SetCommand(v) => {
                self.command.clone_from(v);
                self.forget_results();
            }
            EditorMessage::SetWorkingDir(v) => {
                self.working_dir.clone_from(v);
                self.forget_results();
            }
            EditorMessage::SetArgs(v) => {
                self.args.clone_from(v);
                self.test = None;
            }
            EditorMessage::SetUrl(v) => self.url.clone_from(v),
            EditorMessage::SetElo(v) => self.elo.clone_from(v),
            EditorMessage::SetProtocol(p) => {
                self.protocol = *p;
                self.test = None;
            }
            EditorMessage::SetHotkey(k) => self.hotkey = *k,
            _ => {}
        }
    }

    fn forget_results(&mut self) {
        self.warnings.clear();
        self.test = None;
        self.detect = None;
        self.check_request = None;
        self.test_request = None;
        self.detect_request = None;
    }

    /// The whole form as a validated draft (pure; the disk is not looked at).
    ///
    /// # Errors
    ///
    /// The first problem found, see [`EditorError`].
    pub fn draft(&self) -> Result<EngineDraft, EditorError> {
        let elo = match self.elo.trim() {
            "" => None,
            text => Some(
                text.parse::<u16>()
                    .map_err(|_| EditorError::EloNotANumber)?,
            ),
        };
        let command = self.command.trim();
        let working_dir = self.working_dir.trim();
        let mut draft = self.base.clone();
        draft.name.clone_from(&self.name);
        draft.command = PathBuf::from(command);
        draft.args = split_args(&self.args).map_err(EditorError::Args)?;
        draft.working_dir = (!working_dir.is_empty()).then(|| PathBuf::from(working_dir));
        draft.protocol = self.protocol;
        draft.elo = elo;
        draft.url = Some(self.url.clone());
        draft.normalized().map_err(EditorError::Invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled() -> EngineEditor {
        let mut editor = EngineEditor::new_engine();
        editor.set(&EditorMessage::SetName("  Stockfish ".to_owned()));
        editor.set(&EditorMessage::SetCommand("/usr/bin/stockfish".to_owned()));
        editor
    }

    #[test]
    fn a_new_form_is_empty_and_refuses_to_save() {
        assert_eq!(
            EngineEditor::new_engine().draft(),
            Err(EditorError::Invalid(ValidationError::EmptyName))
        );
    }

    #[test]
    fn the_form_becomes_a_normalized_draft() {
        let mut editor = filled();
        editor.set(&EditorMessage::SetArgs("--threads 4 \"a b\"".to_owned()));
        editor.set(&EditorMessage::SetElo("3500".to_owned()));
        editor.set(&EditorMessage::SetWorkingDir("  ".to_owned()));
        editor.set(&EditorMessage::SetUrl("  ".to_owned()));
        let draft = editor.draft().expect("valid");
        assert_eq!(draft.name, "Stockfish");
        assert_eq!(draft.args, ["--threads", "4", "a b"]);
        assert_eq!(
            (draft.elo, draft.working_dir, draft.url),
            (Some(3500), None, None)
        );
    }

    #[test]
    fn bad_fields_are_reported_with_their_own_error() {
        let mut editor = filled();
        editor.set(&EditorMessage::SetElo("abc".to_owned()));
        assert_eq!(editor.draft(), Err(EditorError::EloNotANumber));
        editor.set(&EditorMessage::SetElo("4001".to_owned()));
        assert_eq!(
            editor.draft(),
            Err(EditorError::Invalid(ValidationError::EloOutOfRange(4001)))
        );
        editor.set(&EditorMessage::SetElo("0".to_owned()));
        assert_eq!(editor.draft().expect("0 is unknown").elo, None);
        editor.set(&EditorMessage::SetArgs("\"open".to_owned()));
        assert_eq!(
            editor.draft(),
            Err(EditorError::Args(ArgsError::UnclosedQuote))
        );
    }

    #[test]
    fn editing_the_command_forgets_the_old_test_and_check() {
        let mut editor = filled();
        editor.test = Some(TestState::Running);
        editor.warnings = vec![CommandWarning::CommandNotFound];
        editor.error = Some(EditorError::EloNotANumber);
        editor.set(&EditorMessage::SetCommand("other".to_owned()));
        assert!(editor.test.is_none() && editor.warnings.is_empty() && editor.error.is_none());
    }

    #[test]
    fn fields_the_form_does_not_show_survive_a_round_trip() {
        let mut base = EngineDraft::new("A", "a");
        base.options = vec![("Hash".to_owned(), "64".to_owned())];
        base.multipv = 3;
        base.low_priority = Some(true);
        let mut list = crate::engines::EngineList::new();
        let id = list.add(base).expect("add");
        let entry = list.get(id).expect("entry");
        let draft = EngineEditor::for_entry(entry, Some(Hotkey::F3))
            .draft()
            .expect("valid");
        assert_eq!(draft.options, entry.options);
        assert_eq!((draft.multipv, draft.low_priority), (3, Some(true)));
    }
}
