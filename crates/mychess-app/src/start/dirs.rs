//! The configuration and data directories of the program: one place that
//! every store asks, so the portable mode (D4013) can move them.

use std::path::PathBuf;
use std::sync::OnceLock;

use directories::ProjectDirs;

/// Both directories, replaced as a pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirs {
    /// Where `preferences.toml`, `engines.toml` and the bookmarks live.
    pub config: PathBuf,
    /// Where the sounds, patterns, tournaments and the spelling file live.
    pub data: PathBuf,
}

static OVERRIDE: OnceLock<AppDirs> = OnceLock::new();

/// Replaces the directories of the system for the whole process (portable
/// mode). Only the first call counts; returns whether this one did. Call it
/// once, from `main`, before anything reads a path.
pub fn install_app_dirs(dirs: AppDirs) -> bool {
    OVERRIDE.set(dirs).is_ok()
}

fn system_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("org", "mychess", "MyChess")
}

/// The configuration directory: the installed override, else the one of the
/// system (`None` when the system has none).
pub fn config_dir() -> Option<PathBuf> {
    match OVERRIDE.get() {
        Some(dirs) => Some(dirs.config.clone()),
        None => system_dirs().map(|d| d.config_dir().to_path_buf()),
    }
}

/// The data directory: the installed override, else the one of the system.
pub fn data_dir() -> Option<PathBuf> {
    match OVERRIDE.get() {
        Some(dirs) => Some(dirs.data.clone()),
        None => system_dirs().map(|d| d.data_dir().to_path_buf()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_an_override_the_system_directories_are_used() {
        // No test of this crate installs the override (it is process-wide).
        assert_eq!(
            config_dir(),
            system_dirs().map(|d| d.config_dir().to_path_buf())
        );
        assert_eq!(
            data_dir(),
            system_dirs().map(|d| d.data_dir().to_path_buf())
        );
    }
}
