//! Start-up of the program (spec M9-4, ADR 0018): where the files of the user
//! live, portable mode, the files named on the command line, the first-run
//! welcome and the tip of the day.
//!
//! - [`resolve_layout`] / [`detect_inputs`]: the `MyChess` folder of the user
//!   (`~/MyChess`), `$MYCHESS_HOME`, and the portable mode of `portable.flag`
//!   (D4000, D4013). Pure, with the inputs injected; the only blocking part
//!   is the writability probe, which the front-end runs before the window
//!   exists (like `peek_preferences`).
//! - [`config_dir`] / [`data_dir`]: the configuration and data directories of
//!   the program, **one** place that every store asks (the portable mode
//!   moves them next to the executable).
//! - [`prepare_user_folder`]: creates `Engine/`, `Databases/` and `Books/`
//!   and finds the engines in `Engine/`; idempotent, never overwrites (D4001,
//!   D4008). Blocking: it runs as [`StartEffect`].
//! - [`LaunchFile`], [`classify_launch_path`]: what a file named on the
//!   command line is opened as.
//! - [`StartPrefs`], [`TIP_COUNT`]: the `[start]` preferences and the tips.

mod dirs;
mod effect;
mod folders;
mod launch;
mod layout;
mod message;
mod prefs;
mod tips;

pub use dirs::{AppDirs, config_dir, data_dir, install_app_dirs};
pub use effect::StartEffect;
pub(crate) use effect::perform;
pub use folders::{
    FolderProblem, FolderReport, SUBFOLDERS, engine_folder, find_bundled_engines,
    prepare_user_folder,
};
pub use launch::{LaunchFile, MAX_LAUNCH_FILES, classify_launch_path};
pub use layout::{
    HOME_ENV, LayoutInputs, LayoutMode, LayoutWarning, PORTABLE_FLAG, ROOT_FOLDER, UserLayout,
    detect_inputs, is_writable_dir, resolve_layout,
};
pub use message::{StartInfo, StartMessage, StartNotice, StartState, WelcomeState};
pub use prefs::StartPrefs;
pub use tips::{TIP_COUNT, tip_key};
