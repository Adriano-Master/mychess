//! Reading whole book files and writing them atomically (spec D1101, D1110).

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::BookError;

/// Largest book file accepted (128 MiB; real books have a few MB).
pub const MAX_FILE_SIZE: u64 = 128 * 1024 * 1024;

/// Longest part of the file name copied into a temporary name, so that the
/// whole name stays below the 255-byte limit of common file systems.
const MAX_TEMP_NAME_BYTES: usize = 150;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A simulated failure while saving, for tests of the atomic write.
///
/// Not part of the stable API.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveFault {
    /// Fail after writing half of the temporary file.
    AfterPartialWrite,
    /// Fail after the temporary file is complete, before the rename.
    BeforeRename,
}

/// Reads the whole file, refusing files above [`MAX_FILE_SIZE`].
///
/// Also returns whether the file is writable according to its permission bits
/// (approximate: it does not test the current user). Anything that is not a
/// regular file (FIFO, device, directory) is refused.
pub(crate) fn read_book_file(path: &Path) -> Result<(Vec<u8>, bool), BookError> {
    let io = |e| BookError::io(path, e);
    // Checked before opening: opening a FIFO would block.
    if !fs::metadata(path).map_err(io)?.is_file() {
        return Err(io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        )));
    }
    let file = File::open(path).map_err(io)?;
    let meta = file.metadata().map_err(io)?;
    if meta.len() > MAX_FILE_SIZE {
        return Err(BookError::TooLarge {
            size: meta.len(),
            limit: MAX_FILE_SIZE,
        });
    }
    let mut bytes = Vec::new();
    // `take` also protects against a file that grows while we read it.
    let read = file
        .take(MAX_FILE_SIZE + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if read as u64 > MAX_FILE_SIZE {
        return Err(BookError::TooLarge {
            size: read as u64,
            limit: MAX_FILE_SIZE,
        });
    }
    Ok((bytes, !meta.permissions().readonly()))
}

/// Writes `bytes` to a temporary file next to `path` and renames it over
/// `path`. The temporary file is removed on failure.
///
/// If `path` is a symlink, the file it points to is updated and the link is
/// kept. If the destination exists, its permissions are copied to the new
/// file (D1121).
pub(crate) fn write_atomic(
    path: &Path,
    bytes: &[u8],
    fault: Option<SaveFault>,
) -> Result<(), BookError> {
    let resolved = resolve_target(path);
    let path = resolved.as_path();
    let permissions = match fs::metadata(path) {
        Ok(meta) => Some(meta.permissions()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(BookError::io(path, e)),
    };
    let (tmp_path, mut tmp) = create_temp(path)?;
    let result = (|| -> std::io::Result<()> {
        if fault == Some(SaveFault::AfterPartialWrite) {
            tmp.write_all(bytes.get(..bytes.len() / 2).unwrap_or_default())?;
            return Err(std::io::Error::other("simulated failure while writing"));
        }
        tmp.write_all(bytes)?;
        tmp.flush()?;
        if let Some(p) = permissions {
            tmp.set_permissions(p)?;
        }
        tmp.sync_all()?;
        // The Windows file system locks open files: close before renaming.
        drop(tmp);
        if fault == Some(SaveFault::BeforeRename) {
            return Err(std::io::Error::other("simulated failure before rename"));
        }
        fs::rename(&tmp_path, path)
    })();
    if let Err(e) = result {
        drop(fs::remove_file(&tmp_path));
        return Err(BookError::io(path, e));
    }
    Ok(())
}

/// Follows a symlink at `path` to the file it points to; any other path (or a
/// link that cannot be resolved, e.g. dangling) is returned unchanged.
fn resolve_target(path: &Path) -> PathBuf {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
        }
        _ => path.to_path_buf(),
    }
}

/// At most [`MAX_TEMP_NAME_BYTES`] bytes of `name`, cut at a char boundary.
fn truncate_name(name: &str) -> &str {
    let mut end = 0;
    for (i, c) in name.char_indices() {
        let next = i + c.len_utf8();
        if next > MAX_TEMP_NAME_BYTES {
            break;
        }
        end = next;
    }
    name.get(..end).unwrap_or_default()
}

/// Creates a fresh temporary file in the directory of `path`.
fn create_temp(path: &Path) -> Result<(PathBuf, File), BookError> {
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let name = path
        .file_name()
        .map_or_else(|| "book".into(), |n| n.to_string_lossy().into_owned());
    let name = truncate_name(&name);
    let mut last_error = None;
    for _ in 0..16 {
        let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_path = dir.join(format!(".{name}.{}.{n}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
        {
            Ok(file) => return Ok((tmp_path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last_error = Some(e),
            Err(e) => return Err(BookError::io(path, e)),
        }
    }
    Err(BookError::io(
        path,
        last_error.unwrap_or_else(|| std::io::Error::other("no temporary name available")),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leftovers(dir: &Path) -> usize {
        fs::read_dir(dir).map_or(0, Iterator::count)
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("b.bin");
        write_atomic(&path, b"one", None).expect("write");
        write_atomic(&path, b"two", None).expect("rewrite");
        assert_eq!(fs::read(&path).expect("read"), b"two");
        assert_eq!(leftovers(dir.path()), 1);
    }

    #[test]
    fn faults_keep_the_original_and_clean_up() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("b.bin");
        write_atomic(&path, b"original", None).expect("write");
        for fault in [SaveFault::AfterPartialWrite, SaveFault::BeforeRename] {
            assert!(write_atomic(&path, b"replacement", Some(fault)).is_err());
            assert_eq!(fs::read(&path).expect("read"), b"original");
            assert_eq!(leftovers(dir.path()), 1);
        }
    }

    #[test]
    fn long_names_get_a_short_temporary_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let name = format!("{}.bin", "é".repeat(120));
        let path = dir.path().join(name);
        let (tmp, _file) = create_temp(&path).expect("temp");
        let len = tmp.file_name().map_or(0, std::ffi::OsStr::len);
        assert!(len <= 255, "{len}");
        assert_eq!(truncate_name("aé"), "aé");
        assert!(truncate_name(&"é".repeat(100)).len() <= MAX_TEMP_NAME_BYTES);
    }

    #[test]
    fn non_regular_files_are_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(matches!(
            read_book_file(dir.path()),
            Err(BookError::Io { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn save_keeps_permissions_and_symlinks() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().expect("tempdir");
        let real = dir.path().join("real.bin");
        fs::write(&real, b"old").expect("write");
        fs::set_permissions(&real, fs::Permissions::from_mode(0o640)).expect("chmod");
        write_atomic(&real, b"new", None).expect("save");
        let mode = fs::metadata(&real).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o640);

        let link = dir.path().join("link.bin");
        symlink(&real, &link).expect("symlink");
        write_atomic(&link, b"via link", None).expect("save through link");
        assert!(
            fs::symlink_metadata(&link)
                .expect("meta")
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&real).expect("read"), b"via link");
        assert_eq!(leftovers(dir.path()), 2);
    }

    #[test]
    fn missing_file_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(matches!(
            read_book_file(&dir.path().join("none.bin")),
            Err(BookError::Io { .. })
        ));
    }
}
