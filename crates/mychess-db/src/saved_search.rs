//! Saved header searches (M3, slice 7; D635 to D648 of
//! `docs/specs/m3-7-buscas-salvas.md`).
//!
//! A [`SavedSearch`] is a named [`HeaderSearch`] stored as one TOML file per
//! search in a directory chosen by the caller (this crate never decides where
//! the configuration lives). [`import_sso`] reads the header-search subset of
//! a Scid `.sso` file without evaluating any Tcl.
//!
//! - [`save`], [`load`], [`list`], [`delete`]: the collection of a directory;
//! - [`SavedSearch::to_toml`] and [`SavedSearch::from_toml`]: the text format;
//! - [`file_stem_for`]: how a name becomes a file name.

mod advanced;
mod format;
mod sso;
mod sso_material;

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::search::HeaderSearch;

pub use advanced::{
    AdvancedQuery, SavedAdvanced, delete_advanced, list_advanced, load_advanced, save_advanced,
};
pub use sso::{SsoReport, SsoWarning, SsoWarningKind, import_sso, import_sso_bytes};
pub use sso_material::{
    AnySsoReport, MaterialSsoReport, import_any_sso, import_any_sso_bytes, import_material_sso,
    import_material_sso_bytes,
};

/// Largest TOML or `.sso` file that is read (D642).
pub const MAX_FILE_BYTES: u64 = 1 << 20;
/// Longest search name, in characters (D635).
pub const MAX_NAME_CHARS: usize = 100;
/// Most files looked at in one directory.
const MAX_FILES: usize = 10_000;
/// Longest file name stem (D640).
const MAX_STEM_CHARS: usize = 80;

/// Why a saved search could not be saved, loaded or imported.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SavedSearchError {
    /// A file or directory operation failed.
    #[error("i/o error on {path}")]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// The cause.
        #[source]
        source: std::io::Error,
    },
    /// The search name is not acceptable (D635).
    #[error("invalid search name: {reason}")]
    InvalidName {
        /// What is wrong.
        reason: &'static str,
    },
    /// The `created` date is not `yyyy-mm-dd`.
    #[error("invalid creation date {value:?}")]
    InvalidDate {
        /// The text, clipped.
        value: String,
    },
    /// No saved search has this name.
    #[error("no saved search named {name:?}")]
    NotFound {
        /// The name asked for.
        name: String,
    },
    /// The file is bigger than [`MAX_FILE_BYTES`].
    #[error("file is larger than {limit} bytes")]
    TooLarge {
        /// The limit.
        limit: u64,
    },
    /// The file contains NUL bytes: it is not text.
    #[error("file is not text")]
    NotText,
    /// The TOML is malformed or has a field of the wrong type.
    #[error("invalid TOML: {message}")]
    Toml {
        /// The parser message.
        message: String,
    },
    /// The file is not a MyChess saved search.
    #[error("not a mychess saved search file")]
    WrongFormat,
    /// The file was written by a newer format version (D637).
    #[error("unsupported saved search version {found}")]
    UnsupportedVersion {
        /// The version in the file.
        found: i64,
    },
    /// A value is outside its domain (D335, D638).
    #[error("invalid value for {field}: {value:?} ({reason})")]
    InvalidValue {
        /// The field (TOML key or Scid variable).
        field: String,
        /// The text, clipped to 64 characters.
        value: String,
        /// What was expected.
        reason: String,
    },
    /// The `.sso` file uses Tcl that is not accepted (D643).
    #[error("unsupported Tcl syntax at line {line}: {reason}")]
    Syntax {
        /// The line where the statement starts.
        line: usize,
        /// What is wrong.
        reason: &'static str,
    },
    /// The `.sso` file is not a header search (D644).
    #[error("unsupported search type {found:?}")]
    UnsupportedSearchType {
        /// The `searchType` value.
        found: String,
    },
    /// The file is a saved search of another kind than the one asked for
    /// (header searches and advanced searches share a directory).
    #[error("saved search of another kind than the one asked for ({found})")]
    WrongKind {
        /// The `kind` of the file (`header` when it has none).
        found: String,
    },
}

/// A named header search (D635).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedSearch {
    /// Display name, 1 to [`MAX_NAME_CHARS`] characters, no control
    /// characters, no spaces at the ends.
    pub name: String,
    /// Creation date `yyyy-mm-dd`, supplied by the caller (informative).
    pub created: Option<String>,
    /// The search.
    pub search: HeaderSearch,
}

impl SavedSearch {
    /// A saved search without a creation date.
    ///
    /// # Errors
    ///
    /// [`SavedSearchError::InvalidName`] if the name is not acceptable.
    pub fn new(name: &str, search: HeaderSearch) -> Result<SavedSearch, SavedSearchError> {
        let name = name.trim();
        validate_name(name)?;
        Ok(SavedSearch {
            name: name.to_owned(),
            created: None,
            search,
        })
    }

    /// The TOML text of the search (D636).
    ///
    /// # Errors
    ///
    /// Invalid name or date, or a serialization failure.
    pub fn to_toml(&self) -> Result<String, SavedSearchError> {
        format::to_toml(self)
    }

    /// Reads the TOML text of a saved search.
    ///
    /// # Errors
    ///
    /// [`SavedSearchError::Toml`], [`WrongFormat`](SavedSearchError::WrongFormat),
    /// [`UnsupportedVersion`](SavedSearchError::UnsupportedVersion) or
    /// [`InvalidValue`](SavedSearchError::InvalidValue).
    pub fn from_toml(text: &str) -> Result<SavedSearch, SavedSearchError> {
        format::from_toml(text)
    }
}

pub(crate) fn validate_name(name: &str) -> Result<(), SavedSearchError> {
    let bad = |reason| Err(SavedSearchError::InvalidName { reason });
    if name.is_empty() {
        return bad("empty");
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return bad("too long");
    }
    if name.chars().any(char::is_control) {
        return bad("control character");
    }
    if name.trim() != name {
        return bad("space at the start or the end");
    }
    Ok(())
}

pub(crate) fn validate_created(text: &str) -> Result<(), SavedSearchError> {
    let b = text.as_bytes();
    let ok = b.len() == 10
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        });
    if ok {
        Ok(())
    } else {
        Err(SavedSearchError::InvalidDate {
            value: text.chars().take(32).collect(),
        })
    }
}

/// An entry of [`list`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedSearchEntry {
    /// The file.
    pub path: PathBuf,
    /// The search name.
    pub name: String,
    /// The creation date, if recorded.
    pub created: Option<String>,
}

/// A `.toml` file that [`list`] could not use.
#[derive(Debug)]
pub struct SkippedFile {
    /// The file.
    pub path: PathBuf,
    /// Why.
    pub error: SavedSearchError,
}

/// The result of [`list`].
#[derive(Debug, Default)]
pub struct SavedSearchList {
    /// Valid searches, sorted by name (case-insensitive) then by path.
    pub entries: Vec<SavedSearchEntry>,
    /// Files that failed to load.
    pub skipped: Vec<SkippedFile>,
}

fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> SavedSearchError + '_ {
    move |source| SavedSearchError::Io {
        path: path.to_owned(),
        source,
    }
}

/// The file name stem derived from a search name (D640): characters
/// forbidden on Windows and control characters become `_`, lowercase,
/// no leading or trailing dots and spaces, Windows reserved names prefixed
/// with `_`, at most 80 characters, never empty.
pub fn file_stem_for(name: &str) -> String {
    let mapped: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .flat_map(char::to_lowercase)
        .collect();
    let trimmed = mapped.trim_matches(['.', ' ']);
    let mut stem: String = trimmed.chars().take(MAX_STEM_CHARS).collect();
    stem = stem.trim_end_matches(['.', ' ']).to_owned();
    if stem.is_empty() {
        return "search".to_owned();
    }
    if is_reserved_windows_name(&stem) {
        stem.insert(0, '_');
    }
    stem
}

fn is_reserved_windows_name(stem: &str) -> bool {
    // Reserved with or without an extension: look at the part before a dot.
    let base = stem.split('.').next().unwrap_or(stem).trim_end();
    let upper = base.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|p| {
            upper
                .strip_prefix(p)
                .is_some_and(|n| matches!(n.as_bytes(), [b'1'..=b'9']))
        })
}

fn has_toml_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
}

/// Reads a file as text, refusing more than [`MAX_FILE_BYTES`] (D642).
pub(crate) fn read_text(path: &Path) -> Result<String, SavedSearchError> {
    let file = File::open(path).map_err(io_error(path))?;
    let len = file.metadata().map_err(io_error(path))?.len();
    if len > MAX_FILE_BYTES {
        return Err(SavedSearchError::TooLarge {
            limit: MAX_FILE_BYTES,
        });
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error(path))?;
    decode_text(&bytes)
}

/// UTF-8 (with or without BOM), else Windows-1252; NUL bytes are refused.
pub(crate) fn decode_text(bytes: &[u8]) -> Result<String, SavedSearchError> {
    if u64::try_from(bytes.len()).map_or(true, |n| n > MAX_FILE_BYTES) {
        return Err(SavedSearchError::TooLarge {
            limit: MAX_FILE_BYTES,
        });
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if bytes.contains(&0) {
        return Err(SavedSearchError::NotText);
    }
    Ok(match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => encoding_rs::WINDOWS_1252
            .decode_without_bom_handling(bytes)
            .0
            .into_owned(),
    })
}

/// Loads one saved search file.
///
/// # Errors
///
/// I/O, size, text or format errors.
pub fn load_file(path: &Path) -> Result<SavedSearch, SavedSearchError> {
    SavedSearch::from_toml(&read_text(path)?)
}

/// Every `.toml` file of `dir` with its result, sorted by file name. A
/// missing directory is empty.
type Scanned = Vec<(PathBuf, Result<SavedSearch, SavedSearchError>)>;

fn scan(dir: &Path) -> Result<Scanned, SavedSearchError> {
    scan_with(dir, load_file)
}

/// The files of a directory with the result of loading each.
type Loaded<T> = Vec<(PathBuf, Result<T, SavedSearchError>)>;

/// Like [`scan`] with the loader of the kind of search wanted.
fn scan_with<T>(
    dir: &Path,
    load: impl Fn(&Path) -> Result<T, SavedSearchError>,
) -> Result<Loaded<T>, SavedSearchError> {
    let reader = match fs::read_dir(dir) {
        Ok(reader) => reader,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_error(dir)(e)),
    };
    let mut paths = Vec::new();
    for entry in reader.take(MAX_FILES) {
        let path = entry.map_err(io_error(dir))?.path();
        // `metadata` follows symbolic links; only regular files count.
        if has_toml_extension(&path) && fs::metadata(&path).is_ok_and(|m| m.is_file()) {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths
        .into_iter()
        .map(|p| {
            let result = load(&p);
            (p, result)
        })
        .collect())
}

/// Lists the saved searches of `dir` (D639). A missing directory gives an
/// empty list; files that cannot be loaded go to
/// [`skipped`](SavedSearchList::skipped).
///
/// # Errors
///
/// [`SavedSearchError::Io`] if the directory cannot be read.
pub fn list(dir: &Path) -> Result<SavedSearchList, SavedSearchError> {
    let mut out = SavedSearchList::default();
    for (path, result) in scan(dir)? {
        match result {
            Ok(s) => out.entries.push(SavedSearchEntry {
                path,
                name: s.name,
                created: s.created,
            }),
            // Advanced searches share the directory: not an error.
            Err(SavedSearchError::WrongKind { .. }) => {}
            Err(error) => out.skipped.push(SkippedFile { path, error }),
        }
    }
    out.entries
        .sort_by_cached_key(|e| (e.name.to_lowercase(), e.path.clone()));
    Ok(out)
}

/// Loads the search called `name` from `dir`.
///
/// # Errors
///
/// [`SavedSearchError::NotFound`] if there is none, or a directory error.
pub fn load(dir: &Path, name: &str) -> Result<SavedSearch, SavedSearchError> {
    let name = name.trim();
    scan(dir)?
        .into_iter()
        .filter_map(|(_, r)| r.ok())
        .find(|s| s.name == name)
        .ok_or_else(|| SavedSearchError::NotFound {
            name: name.to_owned(),
        })
}

/// Deletes the search called `name`; `false` if there was none.
///
/// # Errors
///
/// I/O errors.
pub fn delete(dir: &Path, name: &str) -> Result<bool, SavedSearchError> {
    let name = name.trim();
    let found = scan(dir)?
        .into_iter()
        .find(|(_, r)| r.as_ref().is_ok_and(|s| s.name == name));
    match found {
        Some((path, _)) => {
            fs::remove_file(&path).map_err(io_error(&path))?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Saves `saved` in `dir` and returns the file path (D639 to D641). A search
/// with the same name is replaced; otherwise the file name comes from
/// [`file_stem_for`], with `-2`, `-3` ... when it is taken by another file.
/// The directory is created if needed.
///
/// # Errors
///
/// Invalid name or date, or I/O errors; on error the previous file is intact.
pub fn save(dir: &Path, saved: &SavedSearch) -> Result<PathBuf, SavedSearchError> {
    let text = saved.to_toml()?;
    fs::create_dir_all(dir).map_err(io_error(dir))?;
    let existing = scan(dir)?
        .into_iter()
        .find(|(_, r)| r.as_ref().is_ok_and(|s| s.name == saved.name))
        .map(|(p, _)| p);
    let path = match existing {
        Some(path) => path,
        None => free_path(dir, &file_stem_for(&saved.name))?,
    };
    write_atomic(&path, text.as_bytes())?;
    Ok(path)
}

fn free_path(dir: &Path, stem: &str) -> Result<PathBuf, SavedSearchError> {
    for n in 1..=MAX_FILES {
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
    Err(SavedSearchError::Io {
        path: dir.to_owned(),
        source: std::io::Error::new(std::io::ErrorKind::AlreadyExists, "too many files"),
    })
}

/// Writes `bytes` to a hidden temporary in the same directory, closes it and
/// renames it over `path`; the temporary is removed on error (D641). The one
/// implementation of the atomic write for the saved searches and the
/// patterns, which only map the error.
///
/// # Errors
///
/// The I/O error of the failing step; [`std::io::ErrorKind::InvalidInput`]
/// when `path` has no file name.
pub(crate) fn write_atomic_io(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "no file name",
        ));
    };
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    let result = (|| {
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            // The handle is dropped here, before the rename (Windows).
        }
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        // Best effort: the original error is the one to report.
        drop(fs::remove_file(&tmp));
    }
    result
}

/// [`write_atomic_io`] with the errors of the saved searches (an I/O error
/// carries the destination path).
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), SavedSearchError> {
    if path.file_name().is_none() {
        return Err(SavedSearchError::InvalidName {
            reason: "no file name",
        });
    }
    write_atomic_io(path, bytes).map_err(io_error(path))
}

#[cfg(test)]
mod tests;
