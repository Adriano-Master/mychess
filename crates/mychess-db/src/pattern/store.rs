//! User patterns on disk (spec M6-2, D3277, rules 23 and 24).
//!
//! One TOML file per pattern in a directory chosen by the caller (the app
//! uses `patterns/` inside `ProjectDirs::data_dir()`; the directory is a
//! parameter so that tests and other front ends can inject their own). The
//! real name of the pattern is **inside** the file; the file name is only a
//! safe rendering of it ([`file_stem_for`]).
//!
//! - Writes are atomic: the bytes go to a hidden temporary in the same
//!   directory, the handle is closed, then `rename` replaces the target
//!   (works on Linux, Windows and macOS).
//! - Reads refuse more than [`MAX_PATTERN_FILE_BYTES`] before reading and
//!   never block on a file that is not a regular one.
//! - A directory holds at most [`MAX_PATTERNS`] patterns; a hostile or
//!   damaged file is an error value (or a `broken` entry of the list), never
//!   a panic.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use super::model::{MAX_NAME_CHARS, MAX_PATTERN_FILE_BYTES, Pattern, PatternError, PatternFile};
use crate::saved_search::{file_stem_for, write_atomic_io};

#[cfg(test)]
thread_local! {
    /// Files read by [`load_pattern`] on this thread (tests count them to
    /// prove that an import lists the directory once).
    static LOADS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Most pattern files looked at (and kept) in one directory.
pub const MAX_PATTERNS: usize = 1000;

/// One entry of [`list_patterns`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternSummary {
    /// The pattern name (the file stem for a broken file).
    pub name: String,
    /// The description (empty for a broken file).
    pub description: String,
    /// Creation date `yyyy-mm-dd`, if the file has one.
    pub created: Option<String>,
    /// The file.
    pub path: PathBuf,
    /// Why the file could not be read; `None` for a good pattern.
    pub broken: Option<String>,
}

fn has_toml_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
}

fn io_other(message: &str) -> PatternError {
    PatternError::Io(std::io::Error::other(message.to_owned()))
}

/// Reads and validates one pattern file.
///
/// # Errors
///
/// [`PatternError::TooLarge`] before reading anything big, [`PatternError::Io`]
/// (also for a directory or other non-regular file), and the errors of
/// [`PatternFile::from_bytes`].
pub fn load_pattern(path: &Path) -> Result<PatternFile, PatternError> {
    #[cfg(test)]
    LOADS.with(|c| c.set(c.get() + 1));
    // `metadata` follows symlinks; a FIFO or device is refused before `open`
    // so that reading cannot block.
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io_other("not a regular file"));
    }
    if meta.len() > MAX_PATTERN_FILE_BYTES {
        return Err(PatternError::TooLarge {
            size: meta.len(),
            max: MAX_PATTERN_FILE_BYTES,
        });
    }
    let mut bytes = Vec::new();
    // The file may have grown since `metadata`: read one byte more than the
    // limit so that `from_bytes` reports `TooLarge`.
    File::open(path)?
        .take(MAX_PATTERN_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    PatternFile::from_bytes(&bytes)
}

/// Paths of the `.toml` regular files of `dir` (hidden files and any other
/// kind of entry are skipped **before** counting), sorted by file name
/// without regard to case (then by path) and cut at [`MAX_PATTERNS`]: the
/// sort makes the cut deterministic and files that are not patterns never
/// push a pattern out of the list.
fn pattern_paths(dir: &Path) -> Result<Vec<PathBuf>, PatternError> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let hidden = entry.file_name().to_string_lossy().starts_with('.');
        if hidden || !has_toml_extension(&path) {
            continue;
        }
        if fs::metadata(&path).is_ok_and(|m| m.is_file()) {
            paths.push(path);
        }
    }
    paths.sort_by_cached_key(|p| {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        (name, p.clone())
    });
    paths.truncate(MAX_PATTERNS);
    Ok(paths)
}

/// The patterns of `dir`, sorted by name (case-insensitive) then by path. A
/// file that cannot be read is listed with [`PatternSummary::broken`] set, so
/// the interface can offer to delete it; it does not stop the list. A
/// directory that does not exist is an empty list.
///
/// # Errors
///
/// [`PatternError::Io`] if the directory exists but cannot be read.
pub fn list_patterns(dir: &Path) -> Result<Vec<PatternSummary>, PatternError> {
    let paths = match pattern_paths(dir) {
        Ok(paths) => paths,
        Err(PatternError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };
    let mut out: Vec<PatternSummary> = paths
        .into_iter()
        .map(|path| match load_pattern(&path) {
            Ok(file) => PatternSummary {
                name: file.pattern.name,
                description: file.pattern.description,
                created: file.created,
                path,
                broken: None,
            },
            Err(error) => PatternSummary {
                name: path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                description: String::new(),
                created: None,
                path,
                broken: Some(error.to_string()),
            },
        })
        .collect();
    out.sort_by_cached_key(|s| (s.name.to_lowercase(), s.path.clone()));
    Ok(out)
}

/// The path of the good pattern called `name`, if any.
fn find_by_name(dir: &Path, name: &str) -> Result<Option<PathBuf>, PatternError> {
    Ok(find_listed(&list_patterns(dir)?, name))
}

/// The path of the good pattern called `name` in an existing listing.
fn find_listed(list: &[PatternSummary], name: &str) -> Option<PathBuf> {
    list.iter()
        .find(|s| s.broken.is_none() && s.name == name)
        .map(|s| s.path.clone())
}

/// Loads the pattern called `name` from `dir`.
///
/// # Errors
///
/// [`PatternError::NotFound`] if there is none, or a directory error.
pub fn load_pattern_named(dir: &Path, name: &str) -> Result<PatternFile, PatternError> {
    let name = name.trim();
    match find_by_name(dir, name)? {
        Some(path) => load_pattern(&path),
        None => Err(PatternError::NotFound(name.to_owned())),
    }
}

/// A free `<stem>.toml`, `<stem>-2.toml`... in `dir`.
fn free_path(dir: &Path, stem: &str) -> Result<PathBuf, PatternError> {
    for n in 1..=MAX_PATTERNS * 2 {
        let file = if n == 1 {
            format!("{stem}.toml")
        } else {
            format!("{stem}-{n}.toml")
        };
        let path = dir.join(file);
        if fs::symlink_metadata(&path).is_err() {
            return Ok(path);
        }
    }
    Err(io_other("too many files with the same name"))
}

/// [`write_atomic_io`] with the errors of the patterns.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), PatternError> {
    if path.file_name().is_none() {
        return Err(PatternError::InvalidName("no file name".to_owned()));
    }
    Ok(write_atomic_io(path, bytes)?)
}

/// Saves `file` in `dir` (created if needed) and returns the file path. A
/// pattern with the same name is replaced; otherwise the file name comes from
/// [`file_stem_for`], with `-2`, `-3`... when taken by another file. On any
/// error the previous file is intact.
///
/// # Errors
///
/// The validation errors of [`PatternFile::to_toml`], [`PatternError::Io`],
/// or an I/O error saying the directory already holds [`MAX_PATTERNS`]
/// patterns.
pub fn save_pattern(dir: &Path, file: &PatternFile) -> Result<PathBuf, PatternError> {
    let text = file.to_toml()?;
    fs::create_dir_all(dir)?;
    let listing = list_patterns(dir)?;
    save_listed(dir, file, &text, &listing)
}

/// The tail of [`save_pattern`] on a listing of `dir` already made, so that
/// one save (or import) reads the directory once.
fn save_listed(
    dir: &Path,
    file: &PatternFile,
    text: &str,
    listing: &[PatternSummary],
) -> Result<PathBuf, PatternError> {
    let path = if let Some(path) = find_listed(listing, &file.pattern.name) {
        path
    } else {
        if listing.len() >= MAX_PATTERNS {
            return Err(io_other("too many patterns in the directory"));
        }
        free_path(dir, &file_stem_for(&file.pattern.name))?
    };
    write_atomic(&path, text.as_bytes())?;
    Ok(path)
}

/// Deletes the pattern called `name`.
///
/// # Errors
///
/// [`PatternError::NotFound`] if there is none, or an I/O error.
pub fn delete_pattern(dir: &Path, name: &str) -> Result<(), PatternError> {
    let name = name.trim();
    match find_by_name(dir, name)? {
        Some(path) => Ok(fs::remove_file(path)?),
        None => Err(PatternError::NotFound(name.to_owned())),
    }
}

/// Deletes one file of the list, good or broken (the interface's "delete" for
/// a [`PatternSummary`]). The path must be a `.toml` file directly inside
/// `dir`, and it must come from [`list_patterns`] (its `path` field, built
/// with the same `dir`): the comparison with `dir` is textual, so a
/// different spelling of the same directory is refused.
///
/// # Errors
///
/// [`PatternError::InvalidName`] for any other path, or an I/O error.
pub fn delete_pattern_file(dir: &Path, path: &Path) -> Result<(), PatternError> {
    if path.parent() != Some(dir) || !has_toml_extension(path) {
        return Err(PatternError::InvalidName(path.display().to_string()));
    }
    Ok(fs::remove_file(path)?)
}

/// Copies a pattern file from outside into `dir` (the "import" of the
/// window): the source is read with all the limits of [`load_pattern`] and
/// validated, and it never replaces a pattern: if the name is taken, ` (2)`,
/// ` (3)`... is added to the name.
///
/// # Errors
///
/// Any error of [`load_pattern`] and [`save_pattern`].
pub fn import_pattern(dir: &Path, source: &Path) -> Result<PathBuf, PatternError> {
    let mut file = load_pattern(source)?;
    // One listing of the directory serves the choice of the name and the
    // save. A broken file is not a pattern and does not take a name.
    let listing = list_patterns(dir)?;
    let taken: HashSet<&str> = listing
        .iter()
        .filter(|s| s.broken.is_none())
        .map(|s| s.name.as_str())
        .collect();
    let base = file.pattern.name.clone();
    let mut n = 1u32;
    while taken.contains(file.pattern.name.as_str()) {
        n += 1;
        if n > 1000 {
            return Err(io_other("too many patterns with the same name"));
        }
        let suffix = format!(" ({n})");
        let keep = MAX_NAME_CHARS.saturating_sub(suffix.chars().count());
        let head: String = base.chars().take(keep).collect();
        file.pattern.name = format!("{head}{suffix}");
    }
    let text = file.to_toml()?;
    fs::create_dir_all(dir)?;
    save_listed(dir, &file, &text, &listing)
}

/// Writes `file` to `dest` (chosen by the user), atomically.
///
/// # Errors
///
/// Validation and I/O errors.
pub fn export_pattern(dest: &Path, file: &PatternFile) -> Result<(), PatternError> {
    let text = file.to_toml()?;
    write_atomic(dest, text.as_bytes())
}

/// Whether `dir` already holds a good pattern with the name of `pattern`
/// (compared after trimming, like [`load_pattern_named`]): used by the
/// interface to warn about a replacement.
///
/// # Errors
///
/// [`PatternError::Io`] if the directory exists but cannot be read.
pub fn name_exists(dir: &Path, pattern: &Pattern) -> Result<bool, PatternError> {
    Ok(find_by_name(dir, pattern.name.trim())?.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::model::{Condition, Scope, Side};

    fn file(name: &str) -> PatternFile {
        PatternFile::new(Pattern {
            name: name.into(),
            description: "d".into(),
            scope: Scope::default(),
            root: Condition::SideToMove { side: Side::White },
        })
    }

    #[test]
    fn save_load_list_delete_roundtrip() {
        let dir = tempfile::tempdir().expect("tmp");
        let sub = dir.path().join("patterns");
        let path = save_pattern(&sub, &file("Fork: N")).expect("save");
        assert!(path.starts_with(&sub));
        assert_eq!(load_pattern(&path).expect("load"), file("Fork: N"));
        let list = list_patterns(&sub).expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Fork: N");
        delete_pattern(&sub, "Fork: N").expect("delete");
        assert!(matches!(
            delete_pattern(&sub, "Fork: N"),
            Err(PatternError::NotFound(_))
        ));
        assert_eq!(list_patterns(&sub).expect("list").len(), 0);
    }

    #[test]
    fn missing_directory_lists_nothing() {
        let dir = tempfile::tempdir().expect("tmp");
        let list = list_patterns(&dir.path().join("nope")).expect("list");
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn same_stem_gets_a_suffix_and_same_name_replaces() {
        let dir = tempfile::tempdir().expect("tmp");
        let a = save_pattern(dir.path(), &file("A/B")).expect("a");
        let b = save_pattern(dir.path(), &file("a_b")).expect("b");
        assert_ne!(a, b);
        let again = save_pattern(dir.path(), &file("A/B")).expect("again");
        assert_eq!(a, again);
        assert_eq!(list_patterns(dir.path()).expect("list").len(), 2);
    }

    #[test]
    fn import_lists_the_directory_once() {
        let dir = tempfile::tempdir().expect("tmp");
        let outside = tempfile::tempdir().expect("tmp");
        let text = file("X").to_toml().expect("toml");
        fs::write(dir.path().join("x.toml"), &text).expect("write");
        for n in 2..=40 {
            let f = file(&format!("X ({n})"));
            fs::write(
                dir.path().join(format!("x{n}.toml")),
                f.to_toml().expect("toml"),
            )
            .expect("write");
        }
        let src = outside.path().join("in.toml");
        fs::write(&src, &text).expect("write");
        LOADS.with(|c| c.set(0));
        let path = import_pattern(dir.path(), &src).expect("import");
        // One read of the source and one of each of the 40 files.
        assert_eq!(LOADS.with(std::cell::Cell::get), 41);
        assert_eq!(load_pattern(&path).expect("load").pattern.name, "X (41)");
    }

    #[test]
    fn save_lists_the_directory_once() {
        let dir = tempfile::tempdir().expect("tmp");
        for n in 0..10 {
            save_pattern(dir.path(), &file(&format!("P{n}"))).expect("save");
        }
        LOADS.with(|c| c.set(0));
        save_pattern(dir.path(), &file("Q")).expect("save");
        assert_eq!(LOADS.with(std::cell::Cell::get), 10);
    }
}
