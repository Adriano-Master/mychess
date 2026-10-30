//! Creating the `MyChess` folder and finding the engines in it (D4001,
//! D4008). Blocking; it runs inside [`StartEffect`](super::StartEffect).

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::engines::{FoundEngine, find_installed_engines};

/// The three folders of `MyChess`, in this order. Fixed, in English, never
/// translated or renamed by the program (D4001).
pub const SUBFOLDERS: [&str; 3] = ["Engine", "Databases", "Books"];

/// How many subfolders of `Engine/` are searched for engines.
const MAX_ENGINE_SUBFOLDERS: usize = 32;

/// Something that stopped a folder from being created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderProblem {
    /// The path exists and is a file.
    IsFile(PathBuf),
    /// The folder could not be created (read-only home, quota...).
    CreateFailed {
        /// The folder.
        path: PathBuf,
        /// Why.
        kind: ErrorKind,
    },
}

impl FolderProblem {
    /// The path the problem is about.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            FolderProblem::IsFile(path) | FolderProblem::CreateFailed { path, .. } => path,
        }
    }
}

/// What [`prepare_user_folder`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderReport {
    /// The `MyChess` folder asked for.
    pub root: PathBuf,
    /// The folders that were created now.
    pub created: Vec<PathBuf>,
    /// The folders that were already there (also under another case).
    pub existing: Vec<PathBuf>,
    /// The folders that could not be created.
    pub problems: Vec<FolderProblem>,
    /// The engines found in `Engine/` (not probed).
    pub engines: Vec<FoundEngine>,
}

impl FolderReport {
    /// Whether every folder is there.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.problems.is_empty()
    }
}

/// The subfolder `name` of `parent`: an existing directory whose name
/// differs only in case (so `engine/` is reused and `Engine/` is not created
/// beside it, D4001), else `parent/name`.
fn child_dir(parent: &Path, name: &str) -> PathBuf {
    if let Ok(entries) = std::fs::read_dir(parent) {
        let mut same: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
            })
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        // The exact spelling wins; the rest is ordered so the pick is stable.
        same.sort_by_key(|p| (p.file_name().is_none_or(|n| n != name), p.clone()));
        if let Some(found) = same.into_iter().next() {
            return found;
        }
    }
    parent.join(name)
}

/// The `Engine/` folder of a `MyChess` folder (existing under any case).
#[must_use]
pub fn engine_folder(root: &Path) -> PathBuf {
    child_dir(root, SUBFOLDERS[0])
}

/// The known engines in `engine_dir` and in its subfolders one level down
/// (`Engine/stockfish/stockfish`), not probed. Reads directories.
#[must_use]
pub fn find_bundled_engines(engine_dir: &Path) -> Vec<FoundEngine> {
    let mut dirs = vec![engine_dir.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(engine_dir) {
        let mut subs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        subs.sort();
        subs.truncate(MAX_ENGINE_SUBFOLDERS);
        dirs.extend(subs);
    }
    find_installed_engines(&dirs)
}

/// Creates `root` and its three subfolders when missing and looks for
/// engines in `Engine/`. Idempotent: whatever exists is left untouched, and
/// nothing is written inside the folders. A failure is a
/// [`FolderProblem`], never a panic; the other folders are still tried.
#[must_use]
pub fn prepare_user_folder(root: &Path) -> FolderReport {
    let mut report = FolderReport {
        root: root.to_path_buf(),
        created: Vec::new(),
        existing: Vec::new(),
        problems: Vec::new(),
        engines: Vec::new(),
    };
    if root.exists() && !root.is_dir() {
        report
            .problems
            .push(FolderProblem::IsFile(root.to_path_buf()));
        return report;
    }
    if !ensure_dir(root, &mut report) {
        return report;
    }
    for name in SUBFOLDERS {
        let path = child_dir(root, name);
        ensure_dir(&path, &mut report);
    }
    report.engines = find_bundled_engines(&engine_folder(root));
    report
}

/// Makes sure `path` is a directory; records what happened.
fn ensure_dir(path: &Path, report: &mut FolderReport) -> bool {
    if path.is_dir() {
        report.existing.push(path.to_path_buf());
        return true;
    }
    if path.exists() {
        report
            .problems
            .push(FolderProblem::IsFile(path.to_path_buf()));
        return false;
    }
    match std::fs::create_dir_all(path) {
        Ok(()) => {
            report.created.push(path.to_path_buf());
            true
        }
        Err(error) => {
            report.problems.push(FolderProblem::CreateFailed {
                path: path.to_path_buf(),
                kind: error.kind(),
            });
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(paths: &[PathBuf]) -> Vec<String> {
        paths
            .iter()
            .filter_map(|p| p.file_name()?.to_str().map(str::to_owned))
            .collect()
    }

    #[test]
    fn creates_the_three_folders_in_an_empty_home() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("Usuário x").join("MyChess");
        let report = prepare_user_folder(&root);
        assert!(report.is_complete(), "{report:?}");
        assert_eq!(
            names(&report.created),
            ["MyChess", "Engine", "Databases", "Books"]
        );
        for name in SUBFOLDERS {
            assert!(root.join(name).is_dir(), "{name}");
        }
        assert!(report.engines.is_empty());
    }

    #[test]
    fn a_second_run_creates_nothing_and_touches_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("MyChess");
        drop(prepare_user_folder(&root));
        let mine = root.join("Databases").join("mine.pgn");
        std::fs::write(&mine, b"keep me").expect("write");
        let second = prepare_user_folder(&root);
        assert!(second.created.is_empty(), "{second:?}");
        assert_eq!(second.existing.len(), 4);
        assert_eq!(std::fs::read(&mine).expect("read"), b"keep me");
    }

    #[test]
    fn a_deleted_folder_is_made_again_but_files_are_never_restored() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("MyChess");
        drop(prepare_user_folder(&root));
        std::fs::remove_dir(root.join("Books")).expect("remove");
        let report = prepare_user_folder(&root);
        assert_eq!(names(&report.created), ["Books"]);
        assert_eq!(
            std::fs::read_dir(root.join("Books")).expect("read").count(),
            0
        );
    }

    #[test]
    fn the_root_as_a_file_is_a_problem_not_a_panic() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("MyChess");
        std::fs::write(&root, b"file").expect("write");
        let report = prepare_user_folder(&root);
        assert_eq!(report.problems, vec![FolderProblem::IsFile(root.clone())]);
        assert!(!report.is_complete());
        assert_eq!(std::fs::read(&root).expect("read"), b"file");
    }

    #[test]
    fn a_subfolder_as_a_file_does_not_stop_the_others() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("MyChess");
        std::fs::create_dir(&root).expect("mkdir");
        std::fs::write(root.join("Engine"), b"file").expect("write");
        let report = prepare_user_folder(&root);
        assert_eq!(report.problems.len(), 1);
        assert_eq!(report.problems[0].path(), root.join("Engine"));
        assert!(root.join("Databases").is_dir() && root.join("Books").is_dir());
    }

    #[test]
    fn an_impossible_location_reports_the_failure() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, b"file").expect("write");
        // A directory cannot be created under a file.
        let report = prepare_user_folder(&blocker.join("MyChess"));
        assert_eq!(report.problems.len(), 1);
        assert!(matches!(
            report.problems[0],
            FolderProblem::CreateFailed { .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_folder_with_another_case_is_reused() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("MyChess");
        std::fs::create_dir_all(root.join("engine")).expect("mkdir");
        let report = prepare_user_folder(&root);
        assert!(!root.join("Engine").exists(), "no twin folder");
        assert_eq!(engine_folder(&root), root.join("engine"));
        assert_eq!(names(&report.created), ["Databases", "Books"]);
    }

    #[cfg(unix)]
    #[test]
    fn finds_an_executable_engine_one_level_down() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("MyChess");
        let dir = root.join("Engine").join("stockfish");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let exe = dir.join("stockfish");
        std::fs::write(&exe, b"#!/bin/sh\n").expect("write");
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        std::fs::write(dir.join("LICENSE-stockfish.txt"), b"GPL").expect("write");
        let report = prepare_user_folder(&root);
        assert_eq!(report.engines.len(), 1);
        assert_eq!(report.engines[0].path, exe);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_is_not_executable_is_not_an_engine() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join("Engine");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("stockfish"), b"data").expect("write");
        assert!(find_bundled_engines(&dir).is_empty());
    }
}
