//! Checks of an entry that need the disk (D429). They are warnings, never
//! blockers (the Scid only complains when it starts the engine), and the
//! file system is behind [`FileProbe`] so tests can fake it.

use std::path::{Path, PathBuf};

/// The file system questions the checks ask.
pub trait FileProbe {
    /// Whether `path` is a regular file (after following links).
    fn is_file(&self, path: &Path) -> bool;
    /// Whether `path` is a directory.
    fn is_dir(&self, path: &Path) -> bool;
    /// Whether the file may be run (permission bits on Unix; any file on
    /// Windows, where the extension decides).
    fn is_executable(&self, path: &Path) -> bool;
}

/// The real file system.
#[derive(Debug, Clone, Copy, Default)]
pub struct FsProbe;

impl FileProbe for FsProbe {
    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn is_executable(&self, path: &Path) -> bool {
        platform::is_executable(path)
    }
}

#[cfg(unix)]
mod platform {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    pub(super) fn is_executable(path: &Path) -> bool {
        std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
}

#[cfg(not(unix))]
mod platform {
    use std::path::Path;

    pub(super) fn is_executable(path: &Path) -> bool {
        path.is_file()
    }
}

/// A remark about an entry; shown, not enforced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandWarning {
    /// The command does not exist (nor is it on the `PATH`).
    CommandNotFound,
    /// The file exists but cannot be run.
    NotExecutable,
    /// The working directory does not exist.
    WorkingDirMissing,
}

/// The directories of the `PATH` environment variable.
pub fn path_dirs_from_env() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default()
}

/// Looks for problems in `command` and `working_dir`. A command with a
/// single component (`stockfish`) is looked up in `path_dirs`, as the system
/// would when starting it.
pub fn check_command(
    command: &Path,
    working_dir: Option<&Path>,
    path_dirs: &[PathBuf],
    probe: &impl FileProbe,
) -> Vec<CommandWarning> {
    let mut out = Vec::new();
    let bare = command.components().count() == 1 && !command.is_absolute();
    let found = if bare {
        path_dirs.iter().find_map(|dir| {
            let candidate = dir.join(command);
            if probe.is_file(&candidate) {
                return Some(candidate);
            }
            if cfg!(windows) {
                let mut with_ext = candidate.into_os_string();
                with_ext.push(".exe");
                let with_ext = PathBuf::from(with_ext);
                return probe.is_file(&with_ext).then_some(with_ext);
            }
            None
        })
    } else {
        probe.is_file(command).then(|| command.to_path_buf())
    };
    match found {
        None => out.push(CommandWarning::CommandNotFound),
        Some(path) if !probe.is_executable(&path) => out.push(CommandWarning::NotExecutable),
        Some(_) => {}
    }
    if working_dir.is_some_and(|d| !probe.is_dir(d)) {
        out.push(CommandWarning::WorkingDirMissing);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[derive(Default)]
    struct Fake {
        files: HashSet<PathBuf>,
        dirs: HashSet<PathBuf>,
        exec: HashSet<PathBuf>,
    }

    impl FileProbe for Fake {
        fn is_file(&self, p: &Path) -> bool {
            self.files.contains(p)
        }
        fn is_dir(&self, p: &Path) -> bool {
            self.dirs.contains(p)
        }
        fn is_executable(&self, p: &Path) -> bool {
            self.exec.contains(p)
        }
    }

    fn base() -> PathBuf {
        std::env::temp_dir().join("mychess-fake")
    }

    #[test]
    fn existing_executable_has_no_warning() {
        let sf = base().join("sf");
        let mut f = Fake::default();
        f.files.insert(sf.clone());
        f.exec.insert(sf.clone());
        f.dirs.insert(base());
        assert!(check_command(&sf, Some(&base()), &[], &f).is_empty());
    }

    #[test]
    fn missing_not_executable_and_missing_dir() {
        let sf = base().join("sf");
        let mut f = Fake::default();
        assert_eq!(
            check_command(&sf, None, &[], &f),
            [CommandWarning::CommandNotFound]
        );
        f.files.insert(sf.clone());
        assert_eq!(
            check_command(&sf, Some(&base()), &[], &f),
            [
                CommandWarning::NotExecutable,
                CommandWarning::WorkingDirMissing
            ]
        );
    }

    #[test]
    fn bare_names_are_searched_in_the_path() {
        let dir = base().join("bin");
        let mut f = Fake::default();
        let found = dir.join("stockfish");
        f.files.insert(found.clone());
        f.exec.insert(found);
        assert!(
            check_command(Path::new("stockfish"), None, std::slice::from_ref(&dir), &f).is_empty()
        );
        assert_eq!(
            check_command(Path::new("nothing"), None, &[dir], &f),
            [CommandWarning::CommandNotFound]
        );
    }

    #[test]
    fn real_file_system_agrees() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("engine");
        std::fs::write(&file, b"#!/bin/sh\n").unwrap();
        let warnings = check_command(&file, Some(dir.path()), &[], &FsProbe);
        #[cfg(unix)]
        assert_eq!(warnings, [CommandWarning::NotExecutable]);
        #[cfg(not(unix))]
        assert!(warnings.is_empty());
        let missing = dir.path().join("missing");
        assert_eq!(
            check_command(&missing, None, &[], &FsProbe),
            [CommandWarning::CommandNotFound]
        );
    }
}
