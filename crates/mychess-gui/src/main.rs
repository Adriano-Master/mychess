//! The `mychess` executable: command line, folders, logging, preferences and
//! the window.

// On Windows the release binaries are GUI-subsystem programs, so no console
// window opens beside the application. Debug builds keep the console (logs).
// A side effect: `--help` and `--version` print nowhere when started from a
// Windows terminal.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use anyhow::Context;
use mychess_gui::cli::{self, Cli};
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    // `--help`, `--version` and wrong options end here with their own exit
    // code (0 for the first two, 2 for a mistake).
    let cli = match Cli::parse_args(std::env::args_os()) {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();
    // Where the files live (portable mode moves the configuration too); it
    // must be settled before anything asks for a path.
    let layout = cli::resolve(&cli);
    if let Some(dirs) = layout.dirs.clone() {
        mychess_app::start::install_app_dirs(dirs);
    }
    for warning in &layout.warnings {
        tracing::warn!(?warning, "start-up folders");
    }
    let launch = cli::start_info(&cli, &layout);
    // The sound sink: silent when there is no audio device (spec M8-11).
    mychess_gui::audio::install();
    let prefs_path = mychess_gui::default_preferences_path();
    mychess_gui::run(prefs_path, Some(launch)).context("the user interface stopped with an error")
}
