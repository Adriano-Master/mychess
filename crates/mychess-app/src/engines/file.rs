//! Reading and writing `engines.toml` (D424). Blocking: only
//! [`perform`](crate::perform) may call it, from `spawn_blocking`.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::error::StoreError;
use super::model::EngineList;
use super::store::{LoadedEngineList, load_engine_list, render_engine_list};
use crate::error::FileError;
use crate::fsutil::write_atomic;

/// Name of the file inside the configuration directory.
pub const ENGINES_FILE_NAME: &str = "engines.toml";

/// Largest file read or written, in bytes.
pub const MAX_ENGINES_FILE_BYTES: u64 = 4 << 20;

const BOM: &[u8] = &[0xef, 0xbb, 0xbf];

/// Where the list lives: `engines.toml` in the user's configuration
/// directory (next to `preferences.toml`), or `None` when there is none.
pub fn engines_path() -> Option<PathBuf> {
    crate::start::config_dir().map(|dir| dir.join(ENGINES_FILE_NAME))
}

/// The copy made before each save (`engines.bak`, as the Scid does).
pub fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("bak")
}

/// Where an unreadable file is kept (`engines.corrupt`).
pub fn corrupt_path(path: &Path) -> PathBuf {
    path.with_extension("corrupt")
}

fn file_error(error: &std::io::Error, path: &Path) -> StoreError {
    StoreError::File(FileError::from_io(error, Some(path.to_path_buf())))
}

fn read_limited(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_ENGINES_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    Ok((bytes.len() as u64 <= MAX_ENGINES_FILE_BYTES).then_some(bytes))
}

/// Keeps a copy of a file that could not be used, so the next save does not
/// destroy it. Best effort.
fn keep_corrupt(path: &Path) {
    std::fs::copy(path, corrupt_path(path)).ok();
}

/// Reads the list at `path`. A missing file is an empty list without remarks.
///
/// # Errors
/// An unreadable, too large, non-UTF-8 or non-TOML file, or one with an
/// unknown version, is an error and the caller keeps its current list. Except
/// for a newer version (which the next save backs up) and an I/O failure, a
/// copy of the file is kept as `engines.corrupt`.
pub fn load_engine_file(path: &Path) -> Result<LoadedEngineList, StoreError> {
    let bytes = match read_limited(path) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            keep_corrupt(path);
            return Err(StoreError::TooLarge {
                limit: MAX_ENGINES_FILE_BYTES,
            });
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LoadedEngineList::default());
        }
        Err(e) => return Err(file_error(&e, path)),
    };
    let Ok(text) = std::str::from_utf8(bytes.strip_prefix(BOM).unwrap_or(&bytes)) else {
        keep_corrupt(path);
        return Err(StoreError::NotUtf8);
    };
    let result = load_engine_list(text);
    if matches!(result, Err(StoreError::Syntax)) {
        keep_corrupt(path);
    }
    result
}

/// The `version` of the file at `path` when it is newer than ours.
fn newer_version_on_disk(path: &Path) -> Option<i64> {
    let bytes = read_limited(path).ok()??;
    let text = std::str::from_utf8(bytes.strip_prefix(BOM).unwrap_or(&bytes)).ok()?;
    match load_engine_list(text) {
        Err(StoreError::NewerVersion { found }) => Some(found),
        _ => None,
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Writes the list atomically, creating the directory. The file it replaces
/// is first copied to `engines.bak`; one written by a newer version is also
/// copied, once, to `engines.toml.bak-v<N>`.
///
/// # Errors
/// [`StoreError::TooLarge`] when the text exceeds [`MAX_ENGINES_FILE_BYTES`]
/// (nothing is written) and [`StoreError::File`] for I/O failures.
pub fn save_engine_file(path: &Path, list: &EngineList) -> Result<(), StoreError> {
    let text = render_engine_list(list);
    if text.len() as u64 > MAX_ENGINES_FILE_BYTES {
        return Err(StoreError::TooLarge {
            limit: MAX_ENGINES_FILE_BYTES,
        });
    }
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| file_error(&e, path))?;
    }
    if let Some(version) = newer_version_on_disk(path) {
        let backup = with_suffix(path, &format!(".bak-v{version}"));
        if !backup.exists() {
            std::fs::copy(path, &backup).map_err(|e| file_error(&e, path))?;
        }
    } else if path.is_file() && load_engine_list_ok(path) {
        // Best effort: a failed backup must not block the save.
        std::fs::copy(path, backup_path(path)).ok();
    }
    write_atomic(path, |file| {
        file.write_all(text.as_bytes())?;
        file.flush()
    })
    .map_err(|e| file_error(&e, path))
}

/// Whether the file at `path` is a readable list (so it deserves a backup).
fn load_engine_list_ok(path: &Path) -> bool {
    read_limited(path)
        .ok()
        .flatten()
        .and_then(|b| String::from_utf8(b.strip_prefix(BOM).unwrap_or(&b).to_vec()).ok())
        .is_some_and(|t| load_engine_list(&t).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::model::{EngineDraft, Hotkey};

    fn list() -> EngineList {
        let mut l = EngineList::new();
        let a = l.add(EngineDraft::new("Stockfish", "sf")).unwrap();
        l.add(EngineDraft::new("Crafty", "crafty")).unwrap();
        l.set_hotkey(a, Some(Hotkey::F3)).unwrap();
        l
    }

    fn tmp_leftovers(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".tmp"))
            .collect()
    }

    #[test]
    fn missing_file_is_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load_engine_file(&dir.path().join(ENGINES_FILE_NAME)).unwrap();
        assert!(loaded.list.is_empty() && loaded.issues.is_empty());
    }

    #[test]
    fn save_then_load_creates_directories_and_leaves_no_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cfg").join(ENGINES_FILE_NAME);
        let l = list();
        save_engine_file(&path, &l).unwrap();
        assert_eq!(load_engine_file(&path).unwrap().list, l);
        assert!(tmp_leftovers(path.parent().unwrap()).is_empty());
    }

    #[test]
    fn second_save_backs_up_the_previous_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ENGINES_FILE_NAME);
        let first = list();
        save_engine_file(&path, &first).unwrap();
        assert!(!backup_path(&path).exists());
        save_engine_file(&path, &EngineList::new()).unwrap();
        let bak = load_engine_file(&backup_path(&path)).unwrap();
        assert_eq!(bak.list, first);
        assert!(load_engine_file(&path).unwrap().list.is_empty());
    }

    #[test]
    fn corrupt_file_is_an_error_and_a_copy_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ENGINES_FILE_NAME);
        std::fs::write(&path, "[[engine\nname = ").unwrap();
        assert_eq!(load_engine_file(&path), Err(StoreError::Syntax));
        assert_eq!(
            std::fs::read_to_string(corrupt_path(&path)).unwrap(),
            "[[engine\nname = "
        );
        // A corrupt file is not promoted to the backup by a later save.
        save_engine_file(&path, &list()).unwrap();
        assert!(!backup_path(&path).exists());
    }

    #[test]
    fn non_utf8_and_oversized_files_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ENGINES_FILE_NAME);
        std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(load_engine_file(&path), Err(StoreError::NotUtf8));
        let big = vec![b'#'; usize::try_from(MAX_ENGINES_FILE_BYTES).unwrap() + 1];
        std::fs::write(&path, big).unwrap();
        assert!(matches!(
            load_engine_file(&path),
            Err(StoreError::TooLarge { .. })
        ));
    }

    #[test]
    fn newer_version_is_an_error_and_is_backed_up_before_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ENGINES_FILE_NAME);
        std::fs::write(&path, "version = 7\nfuture = true\n").unwrap();
        assert_eq!(
            load_engine_file(&path),
            Err(StoreError::NewerVersion { found: 7 })
        );
        assert!(!corrupt_path(&path).exists());
        save_engine_file(&path, &list()).unwrap();
        let kept = with_suffix(&path, ".bak-v7");
        assert_eq!(
            std::fs::read_to_string(kept).unwrap(),
            "version = 7\nfuture = true\n"
        );
    }

    #[test]
    fn bom_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ENGINES_FILE_NAME);
        let mut bytes = BOM.to_vec();
        bytes.extend_from_slice(render_engine_list(&list()).as_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(load_engine_file(&path).unwrap().list, list());
    }

    #[test]
    fn a_directory_in_the_way_is_a_file_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = load_engine_file(dir.path());
        assert!(matches!(err, Err(StoreError::File(_))));
    }
}
