//! The messages, notices and dialog state of the engine management (spec
//! M4-3, D424-D436).

use std::path::PathBuf;

use mychess_engine::{EngineEvent, EngineIdentity, Protocol};

use super::check::CommandWarning;
use super::editor::EditorMessage;
use super::error::StoreError;
use super::find::FoundEngine;
use super::model::{EngineId, Hotkey};
use super::options::{OptionsDialog, OptionsMessage};
use super::run::{CommandError, CommandKind, EngineFailure, RunId};
use super::scid::{ImportReport, ScidImport};
use super::store::LoadedEngineList;
use crate::error::{FileError, RequestId};
use crate::prefs::AnalysisPrefs;

/// What "Start" does once the engine is ready (spec rule 13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StartMode {
    /// Start analysing the position shown (the normal start).
    #[default]
    Analyze,
    /// Leave the engine ready and idle (the Scid's right click, "nostart").
    Idle,
}

/// An event of the engine management: the user's commands and the answers
/// to the effects.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EngineMessage {
    // ---- the dialog and the list ----
    /// Opens the "Engines" dialog.
    OpenDialog,
    /// Closes it (and the editor, if open).
    CloseDialog,
    /// Selects a row (`None`: none).
    Select(Option<EngineId>),
    /// Typing a letter selects the next engine whose name starts with it.
    SelectInitial(char),
    /// Opens the editor on a new engine.
    New,
    /// Opens the editor on an engine.
    Edit(EngineId),
    /// Adds a copy named `<name>-new`.
    Copy(EngineId),
    /// Asks to delete (the confirmation names the engine and its command).
    Delete(EngineId),
    /// The user confirmed the deletion.
    ConfirmDelete(EngineId),
    /// The user gave up deleting.
    CancelDelete,
    /// Moves an engine one place up.
    MoveUp(EngineId),
    /// Moves an engine one place down.
    MoveDown(EngineId),
    /// Gives a key to an engine (taking it from its owner) or frees its key.
    SetHotkey(EngineId, Option<Hotkey>),
    /// An edit of the editor form.
    Editor(EditorMessage),
    /// Saves one UCI option of the engine (and applies it at once if the
    /// engine is running).
    SetOption {
        /// The engine of the list.
        engine: EngineId,
        /// The option name.
        name: String,
        /// The value, as text.
        value: String,
    },
    /// Changes the analysis preferences (D431).
    SetAnalysisPrefs(AnalysisPrefs),
    /// Opens the UCI options dialog of an engine (D430, D740).
    OpenOptions(EngineId),
    /// An edit of the UCI options dialog.
    Options(OptionsMessage),

    // ---- executions ----
    /// Starts the engine, or ends it if it runs (D433).
    Start(EngineId, StartMode),
    /// Ends an execution (and drops it from the list of runs).
    Stop(RunId),
    /// Starts again an engine that failed.
    Restart(RunId),
    /// F2, F3 or F4: starts the engine of the key, or ends it if it runs.
    ToggleHotkey(Hotkey),
    /// The space bar: turns the analysis of the first running engine on or
    /// off.
    ToggleMainAnalysis,

    // ---- finding and importing ----
    /// Looks for installed engines (only lists files).
    FindInstalled,
    /// The user marked a found engine: it is probed and, if it answers,
    /// added.
    AddFound(PathBuf),
    /// Imports an `engines.dat` of the Scid.
    ImportScid(PathBuf),
    /// Clears the report of the last import and search.
    DismissReports,

    // ---- answers to effects ----
    /// `engines.toml` was read.
    Loaded(Box<Result<LoadedEngineList, StoreError>>),
    /// `engines.toml` was written.
    Saved(Result<(), StoreError>),
    /// The disk check of the editor finished.
    Checked {
        /// The request.
        request: RequestId,
        /// The remarks.
        warnings: Vec<CommandWarning>,
    },
    /// The probe of the editor finished.
    Tested {
        /// The request.
        request: RequestId,
        /// Who the engine is, or why it failed.
        result: Box<Result<EngineIdentity, EngineFailure>>,
    },
    /// The protocol detection finished.
    Detected {
        /// The request.
        request: RequestId,
        /// The protocol, or why none was found.
        result: Result<Protocol, EngineFailure>,
    },
    /// The search for installed engines finished.
    Found {
        /// The request.
        request: RequestId,
        /// What was found.
        engines: Vec<FoundEngine>,
    },
    /// A found engine was probed.
    FoundProbed {
        /// The request.
        request: RequestId,
        /// The executable.
        path: PathBuf,
        /// Its protocol and name, or why it failed.
        result: Result<(Protocol, String), EngineFailure>,
    },
    /// An `engines.dat` was read.
    ScidRead {
        /// The request.
        request: RequestId,
        /// What it holds, or why it could not be read.
        result: Box<Result<ScidImport, FileError>>,
    },
    /// An engine started and finished its handshake.
    Started {
        /// The execution.
        run: RunId,
        /// Who the engine is.
        identity: Box<EngineIdentity>,
        /// When, in seconds since the Unix epoch (for "last used").
        at: i64,
    },
    /// An engine did not start.
    StartFailed {
        /// The execution.
        run: RunId,
        /// Why.
        failure: EngineFailure,
    },
    /// The engine of an execution was ended.
    Stopped(RunId),
    /// Every engine was ended (quit).
    ShutdownDone,
    /// A command was handed to an engine, or refused.
    CommandDone {
        /// The execution.
        run: RunId,
        /// The outcome.
        result: Result<(), CommandError>,
    },
    /// A command was refused or found the queue of the engine full.
    /// Sent instead of [`EngineMessage::CommandDone`] for those two errors,
    /// so the receiver knows which command it was (spec M4-6, D895).
    CommandFailed {
        /// The execution.
        run: RunId,
        /// Which command.
        kind: CommandKind,
        /// Why.
        error: CommandError,
    },
    /// An event of a running engine (delivered by the front-end's
    /// subscription to [`EngineHub::events`](super::EngineHub::events)).
    Event {
        /// The execution.
        run: RunId,
        /// The event, already rate limited by the engine crate.
        event: Box<EngineEvent>,
    },
}

/// The "Engines" dialog.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EngineDialog {
    /// The selected row.
    pub selected: Option<EngineId>,
    /// The engine whose deletion waits for the user's answer.
    pub confirm_delete: Option<EngineId>,
    /// What "Find installed engines" found; empty until asked.
    pub found: Vec<FoundEngine>,
    /// A search is running.
    pub finding: bool,
    /// Probes of found engines in flight.
    pub probing: usize,
    /// The report of the last import.
    pub import: Option<ImportReport>,
    /// The UCI options dialog, when open (D430, D740).
    pub options: Option<OptionsDialog>,
}

/// A remark about the engine management, as data (D-notice, like
/// [`BookmarkNotice`](crate::BookmarkNotice)).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EngineNotice {
    /// `engines.toml` could not be read; the list is empty and a copy of the
    /// file was kept.
    FileUnreadable,
    /// `engines.toml` was written by a newer version.
    NewerVersion,
    /// Some entries or fields of `engines.toml` were dropped or reset.
    FileFixed {
        /// How many remarks the reading made.
        count: usize,
    },
    /// `engines.toml` could not be written.
    SaveFailed,
    /// The key has no engine.
    NoEngineOnKey(Hotkey),
    /// No engine is running to turn its analysis on or off.
    NoEngineRunning,
    /// The list is full.
    ListFull,
    /// The engine is not in the list (it was deleted meanwhile).
    NotFound,
    /// The entry cannot be stored as it is (for example, a copy whose name
    /// would pass the length limit).
    EntryInvalid,
    /// The command, arguments or folder changed while the engine runs: the
    /// change applies on the next start.
    RestartToApply,
    /// Many engines are running (D433).
    ManyRunning {
        /// How many executions there are.
        count: usize,
    },
    /// An engine did not start.
    StartFailed {
        /// The name of the engine.
        name: String,
        /// Why.
        failure: EngineFailure,
    },
    /// An engine ended by itself.
    Crashed {
        /// The name of the engine.
        name: String,
        /// Why.
        failure: EngineFailure,
    },
    /// An engine refused a command (an option out of range, `MultiPV`,
    /// Chess960...).
    CommandRefused,
    /// A found engine did not answer the probe, so it was not added.
    FoundRejected {
        /// The executable.
        path: PathBuf,
        /// Why.
        failure: EngineFailure,
    },
    /// An `engines.dat` was imported.
    Imported {
        /// Engines added.
        added: usize,
        /// Skipped as duplicates.
        duplicates: usize,
        /// Skipped as unusable.
        invalid: usize,
    },
    /// The `engines.dat` has no engine.
    NothingToImport,
    /// The `engines.dat` could not be read.
    ImportFailed(FileError),
}

impl EngineNotice {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            EngineNotice::FileUnreadable => "engines-notice-file-unreadable",
            EngineNotice::NewerVersion => "engines-notice-newer-version",
            EngineNotice::FileFixed { .. } => "engines-notice-file-fixed",
            EngineNotice::SaveFailed => "engines-notice-save-failed",
            EngineNotice::NoEngineOnKey(_) => "engines-notice-no-engine-on-key",
            EngineNotice::NoEngineRunning => "engines-notice-no-engine-running",
            EngineNotice::ListFull => "engines-notice-list-full",
            EngineNotice::NotFound => "engines-notice-not-found",
            EngineNotice::EntryInvalid => "engines-notice-entry-invalid",
            EngineNotice::RestartToApply => "engines-notice-restart-to-apply",
            EngineNotice::ManyRunning { .. } => "engines-notice-many-running",
            EngineNotice::StartFailed { failure, .. }
            | EngineNotice::Crashed { failure, .. }
            | EngineNotice::FoundRejected { failure, .. } => failure.message_key(),
            EngineNotice::CommandRefused => "engines-notice-command-refused",
            EngineNotice::Imported { .. } => "engines-notice-imported",
            EngineNotice::NothingToImport => "engines-notice-nothing-to-import",
            EngineNotice::ImportFailed(error) => error.message_key(),
        }
    }
}
