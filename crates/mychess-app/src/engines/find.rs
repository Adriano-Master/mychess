//! Looking for installed engines (spec M4-3, D427). It only lists files: it
//! never runs anything. Every engine enters the list only after the user
//! marks it and it passes the probe.

use std::path::{Path, PathBuf};

use super::check::{FileProbe, FsProbe};

/// File names (without extension) of the engines the search knows.
pub const KNOWN_ENGINES: &[&str] = &[
    "stockfish",
    "lc0",
    "komodo",
    "dragon",
    "berserk",
    "ethereal",
    "rubichess",
    "igel",
    "koivisto",
    "fruit",
    "toga",
    "crafty",
    "gnuchess",
    "phalanx",
];

/// An executable that looks like a known engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundEngine {
    /// The file name without extension, as found.
    pub name: String,
    /// The executable.
    pub path: PathBuf,
}

/// Index in [`KNOWN_ENGINES`] of the engine a file stem names: the stem is
/// the name, or starts with it followed by something that is not a letter
/// (`stockfish_16`, `lc0-v0.31`), ignoring case.
fn known_index(stem: &str) -> Option<usize> {
    let lower = stem.to_lowercase();
    KNOWN_ENGINES.iter().position(|known| {
        lower
            .strip_prefix(known)
            .is_some_and(|rest| rest.chars().next().is_none_or(|c| !c.is_alphabetic()))
    })
}

fn has_engine_extension(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str());
    match ext {
        None => !cfg!(windows),
        Some(ext) if cfg!(windows) => ext.eq_ignore_ascii_case("exe"),
        // On Unix a dot in the name (`lc0-v0.31`) is not an extension; the
        // executable bit decides.
        Some(_) => true,
    }
}

/// The known engines found directly inside `dirs` (not recursive), without
/// duplicates, sorted by the order of [`KNOWN_ENGINES`] and then by path.
/// Reads directories: call it from `perform`.
pub fn find_installed_engines(dirs: &[PathBuf]) -> Vec<FoundEngine> {
    find_with(dirs, &FsProbe)
}

fn find_with(dirs: &[PathBuf], probe: &impl FileProbe) -> Vec<FoundEngine> {
    let mut found: Vec<(usize, FoundEngine)> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(stem) = file_stem(&path) else {
                continue;
            };
            let Some(index) = known_index(&stem) else {
                continue;
            };
            if !has_engine_extension(&path)
                || !probe.is_file(&path)
                || !probe.is_executable(&path)
                || found.iter().any(|(_, f)| f.path == path)
            {
                continue;
            }
            found.push((index, FoundEngine { name: stem, path }));
        }
    }
    found.sort_by(|a, b| (a.0, &a.1.path).cmp(&(b.0, &b.1.path)));
    found.into_iter().map(|(_, f)| f).collect()
}

fn file_stem(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    if cfg!(windows) {
        Some(path.file_stem()?.to_str()?.to_owned())
    } else {
        Some(name.to_owned())
    }
}

/// The usual places besides the `PATH` (D427). On Windows, the folders one
/// level under `%ProgramFiles%` (this reads that directory).
pub fn usual_engine_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
    } else if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(root) = std::env::var_os(var).map(PathBuf::from)
                && let Ok(entries) = std::fs::read_dir(&root)
            {
                dirs.push(root.clone());
                dirs.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
            }
        }
    } else {
        dirs.push(PathBuf::from("/usr/games"));
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/usr/local/games"));
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_exe(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, b"#!/bin/sh\n").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        path
    }

    fn exe(name: &str) -> String {
        if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_owned()
        }
    }

    #[test]
    fn known_names_match_ignoring_case_and_suffixes() {
        assert_eq!(known_index("Stockfish"), Some(0));
        assert_eq!(known_index("stockfish_16_x64"), Some(0));
        assert_eq!(known_index("lc0-v0.31"), Some(1));
        assert_eq!(known_index("stockfishy"), None);
        assert_eq!(known_index("notepad"), None);
    }

    #[test]
    fn finds_executables_in_the_order_of_the_known_list() {
        let dir = tempfile::tempdir().expect("tempdir");
        let toga = make_exe(dir.path(), &exe("toga"));
        let sf = make_exe(dir.path(), &exe("Stockfish"));
        make_exe(dir.path(), &exe("unrelated"));
        let found = find_installed_engines(&[dir.path().to_path_buf()]);
        let paths: Vec<_> = found.iter().map(|f| f.path.clone()).collect();
        assert_eq!(paths, [sf, toga]);
    }

    #[test]
    fn missing_directory_and_duplicate_directories_are_harmless() {
        let dir = tempfile::tempdir().expect("tempdir");
        make_exe(dir.path(), &exe("crafty"));
        let found = find_installed_engines(&[
            dir.path().join("nope"),
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
        ]);
        assert_eq!(found.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_without_the_execute_bit_is_not_offered() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("stockfish");
        std::fs::write(&path, b"data").expect("write");
        assert!(find_installed_engines(&[dir.path().to_path_buf()]).is_empty());
    }

    #[test]
    fn the_search_never_runs_anything() {
        // A script that would leave a marker if run.
        let dir = tempfile::tempdir().expect("tempdir");
        let marker = dir.path().join("ran");
        let script = dir.path().join(exe("stockfish"));
        std::fs::write(
            &script,
            format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        )
        .expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
                .expect("chmod");
        }
        assert_eq!(find_installed_engines(&[dir.path().to_path_buf()]).len(), 1);
        assert!(!marker.exists());
    }
}
