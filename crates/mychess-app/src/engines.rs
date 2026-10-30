//! Engine management (spec M4-3). Phase 1 is the pure model and its
//! persistence (D424-D429, D432, D684-D693); phase 2 wires it to `Message`,
//! `Effect` and `AppState` (D720 onwards): executions ([`EngineRuns`]), the
//! bridge to the processes ([`EngineHub`]), the dialog and editor state, the
//! search for installed engines and the F2-F4 keys.
//!
//! - [`EngineList`], [`EngineEntry`], [`EngineDraft`]: the list and its pure
//!   operations (add, edit, copy, remove, move, one owner per F2-F4 key).
//! - [`split_args`] / [`join_args`]: arguments as one line of text.
//! - [`load_engine_list`] / [`render_engine_list`]: `engines.toml` as text.
//! - [`load_engine_file`] / [`save_engine_file`]: the file (blocking; call
//!   from `perform`), atomic, with `engines.bak`.
//! - [`parse_scid_engines_dat`] and [`EngineList::import_scid`]: import from
//!   the Scid.
//! - [`check_command`]: disk checks behind [`FileProbe`].
//! - [`EngineEntry::to_config`]: the `mychess_engine::EngineConfig`.
//! - [`EngineMessage`], [`EngineNotice`], [`EngineDialog`], [`EngineEditor`]:
//!   the messages, remarks and dialog state.
//! - [`EngineHub`], [`EngineRuns`], [`EngineFailure`]: executions and the
//!   engine processes.
//! - [`find_installed_engines`]: the search of D427 (only lists files).

mod args;
mod check;
mod config;
mod editor;
mod error;
mod file;
mod find;
mod message;
mod model;
pub(crate) mod ops;
mod options;
mod run;
mod scid;
mod store;

#[cfg(test)]
mod flow_tests;

pub use args::{join_args, split_args};
pub use check::{CommandWarning, FileProbe, FsProbe, check_command, path_dirs_from_env};
pub use config::RunDefaults;
pub(crate) use config::option_value;
pub use editor::{DetectState, EditorError, EditorMessage, EngineEditor, TestState};
pub use error::{ArgsError, ListError, StoreError, ValidationError};
pub use file::{
    ENGINES_FILE_NAME, MAX_ENGINES_FILE_BYTES, backup_path, corrupt_path, engines_path,
    load_engine_file, save_engine_file,
};
pub use find::{FoundEngine, KNOWN_ENGINES, find_installed_engines, usual_engine_dirs};
pub use message::{EngineDialog, EngineMessage, EngineNotice, StartMode};
pub use model::{
    COPY_SUFFIX, EngineDraft, EngineEntry, EngineId, EngineList, Hotkey, MAX_ELO, MAX_ENGINES,
    MAX_ITEMS, MAX_MULTIPV, MAX_TEXT_CHARS,
};
pub use mychess_engine::{
    Capabilities, EngineEvent, EngineIdentity, EngineOption, EngineWarning, ExitInfo, ExitReason,
    OptionError, OptionKind, OptionValue, Phase, Protocol,
};
pub use options::{
    IMPORTANT_OPTIONS, OptionsDialog, OptionsMessage, OptionsStatus, default_text, ordered_options,
    spin_step,
};
pub use run::{
    AnalysisTarget, CommandError, CommandKind, EngineEventStream, EngineFailure, EngineHub,
    EngineRun, EngineRunState, EngineRuns, FailureKind, LaunchFuture, LaunchResult, Launcher,
    MANY_RUNS, RunId, SHUTDOWN_WAIT,
};
pub use scid::{
    ImportReport, ImportWarning, MAX_BRACE_DEPTH, MAX_SCID_BYTES, MAX_SCID_ENGINES, ScidEngine,
    ScidImport, ScidWarning, parse_scid_engines_dat,
};
pub use store::{
    ENGINES_VERSION, LoadIssue, LoadedEngineList, load_engine_list, render_engine_list,
};
