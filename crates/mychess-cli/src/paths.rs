//! Names of the database files.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The three files of a database.
pub(crate) const EXTENSIONS: [&str; 3] = ["si4", "sn4", "sg4"];

/// `path` without `.si4`, `.sn4` or `.sg4` (any letter case); other names are
/// returned as they are.
pub(crate) fn strip_db_extension(path: &Path) -> PathBuf {
    let known = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|k| k.eq_ignore_ascii_case(e)));
    if known {
        path.with_extension("")
    } else {
        path.to_owned()
    }
}

/// The default database for a PGN file: the name without `.gz` and without
/// the last extension (`pgnscid.cpp`).
pub(crate) fn default_base(pgn: &Path) -> PathBuf {
    let mut base = pgn.to_owned();
    let is_gz = base
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("gz"));
    if is_gz {
        base.set_extension("");
    }
    if base.extension().is_some() {
        base.set_extension("");
    }
    base
}

/// `stem` followed by `.ext` (a dot in the stem is kept).
pub(crate) fn with_suffix(stem: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = stem.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The existing file `<stem>.<ext>`. An exact match always wins; only when
/// there is none is a file that differs just in letter case accepted (the
/// Windows and macOS file systems would treat it as the same file). When
/// several differ only in case, the first in byte order is taken, so the
/// answer does not depend on the order of the directory listing.
pub(crate) fn find_existing(stem: &Path, ext: &str) -> Option<PathBuf> {
    let exact = with_suffix(stem, &format!(".{ext}"));
    if exact.is_file() {
        return Some(exact);
    }
    let parent = match exact.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let wanted = exact.file_name()?.to_string_lossy().to_lowercase();
    let mut candidates: Vec<PathBuf> = fs::read_dir(parent)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().to_lowercase() == wanted)
        .map(|e| e.path())
        .filter(|path| path.is_file())
        .collect();
    candidates.sort();
    candidates.into_iter().next()
}

/// Whether any of the three files of the database exists.
pub(crate) fn database_exists(stem: &Path) -> bool {
    EXTENSIONS.iter().any(|e| find_existing(stem, e).is_some())
}

/// The files of the database that exist (`.si4`, `.sn4`, `.sg4` and the
/// `.stc` cache), each one as [`find_existing`] resolves it.
pub(crate) fn database_files(stem: &Path) -> Vec<PathBuf> {
    EXTENSIONS
        .iter()
        .copied()
        .chain(["stc"])
        .filter_map(|ext| find_existing(stem, ext))
        .collect()
}

/// Files of the database that differ from the ones in `kept` only in letter
/// case (`X.SI4` next to `x.si4`). The writer of `mychess-db` refuses to
/// create a database over them, because on Windows and macOS they would be
/// the same file; `import --force` must know before it deletes anything.
pub(crate) fn case_variants(stem: &Path, kept: &[PathBuf]) -> Vec<PathBuf> {
    let Some(name) = stem.file_name() else {
        return Vec::new();
    };
    let wanted: Vec<String> = EXTENSIONS
        .iter()
        .map(|ext| format!("{}.{ext}", name.to_string_lossy()).to_lowercase())
        .collect();
    let parent = match stem.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| wanted.contains(&e.file_name().to_string_lossy().to_lowercase()))
        // By file name, not by path: `read_dir(".")` yields `./g.si4` while a
        // stem without a directory gives `g.si4`, the same file.
        .filter(|e| {
            !kept
                .iter()
                .any(|path| path.file_name() == Some(e.file_name().as_os_str()))
        })
        .map(|e| e.path())
        .collect();
    found.sort();
    found
}

/// Removes exactly `files` (from [`database_files`]); a file that is already
/// gone is not an error.
pub(crate) fn remove_files(files: &[PathBuf]) -> io::Result<()> {
    for path in files {
        match fs::remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

/// Removes the files of the database (and the `.stc` cache); a missing file
/// is not an error.
pub(crate) fn remove_database(stem: &Path) -> io::Result<()> {
    remove_files(&database_files(stem))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_only_database_extensions() {
        assert_eq!(strip_db_extension(Path::new("a/b.SI4")), Path::new("a/b"));
        assert_eq!(strip_db_extension(Path::new("a/b.sg4")), Path::new("a/b"));
        assert_eq!(
            strip_db_extension(Path::new("a/b.pgn")),
            Path::new("a/b.pgn")
        );
        assert_eq!(strip_db_extension(Path::new("a/b")), Path::new("a/b"));
    }

    #[test]
    fn default_base_drops_gz_and_pgn() {
        assert_eq!(default_base(Path::new("x/games.pgn")), Path::new("x/games"));
        assert_eq!(
            default_base(Path::new("x/games.PGN.gz")),
            Path::new("x/games")
        );
        assert_eq!(default_base(Path::new("games")), Path::new("games"));
        assert_eq!(default_base(Path::new("a.b/games")), Path::new("a.b/games"));
    }

    #[test]
    fn finds_files_ignoring_extension_case() {
        let dir = tempfile::tempdir().unwrap();
        let stem = dir.path().join("Base");
        assert!(!database_exists(&stem));
        fs::write(dir.path().join("Base.SI4"), b"x").unwrap();
        assert!(database_exists(&stem));
        assert!(find_existing(&stem, "si4").is_some());
        remove_database(&stem).unwrap();
        assert!(!database_exists(&stem));
    }

    #[test]
    fn an_exact_match_wins_over_one_that_differs_in_case() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("x.si4"), b"lower").unwrap();
        fs::write(dir.path().join("X.SI4"), b"upper").unwrap();
        if fs::read(dir.path().join("x.si4")).unwrap() != b"lower" {
            return; // case-insensitive file system: one file only
        }
        let stem = dir.path().join("x");
        assert_eq!(find_existing(&stem, "si4"), Some(dir.path().join("x.si4")));
        let files = database_files(&stem);
        assert_eq!(files, vec![dir.path().join("x.si4")]);
        remove_database(&stem).unwrap();
        assert!(dir.path().join("X.SI4").exists(), "X.SI4 must survive");
        assert!(!dir.path().join("x.si4").exists());
        // With no exact match left, the other name is the one found.
        assert_eq!(find_existing(&stem, "si4"), Some(dir.path().join("X.SI4")));
    }

    #[test]
    fn case_variants_are_the_files_that_are_not_the_ones_found() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("x.si4"), b"lower").unwrap();
        fs::write(dir.path().join("X.SI4"), b"upper").unwrap();
        if fs::read(dir.path().join("x.si4")).unwrap() != b"lower" {
            return; // case-insensitive file system: one file only
        }
        let stem = dir.path().join("x");
        let kept = database_files(&stem);
        assert_eq!(case_variants(&stem, &kept), vec![dir.path().join("X.SI4")]);
        assert!(
            case_variants(&stem, &[dir.path().join("X.SI4")]).contains(&dir.path().join("x.si4"))
        );
    }

    #[test]
    fn lists_the_files_that_exist() {
        let dir = tempfile::tempdir().unwrap();
        let stem = dir.path().join("b");
        assert_eq!(database_files(&stem).len(), 0);
        fs::write(dir.path().join("b.si4"), b"").unwrap();
        fs::write(dir.path().join("b.STC"), b"").unwrap();
        assert_eq!(
            database_files(&stem),
            vec![dir.path().join("b.si4"), dir.path().join("b.STC")]
        );
    }

    #[test]
    fn suffix_keeps_dots_of_the_stem() {
        assert_eq!(
            with_suffix(Path::new("a.b/c.d"), ".err"),
            Path::new("a.b/c.d.err")
        );
    }
}
