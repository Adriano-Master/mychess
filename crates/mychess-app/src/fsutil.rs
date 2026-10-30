//! Atomic file replacement and path helpers shared by the PGN writer and the
//! preferences (spec M2-2, D128 and D145; `.claude/rules/multiplataforma.md`).
//!
//! The data goes to a temporary file in the **same directory** as the target,
//! is flushed to disk, gets the permissions of the file it replaces and is
//! then renamed over the target (`rename` replaces an existing file on Linux,
//! Windows and macOS). A failure before the rename leaves the target intact
//! and the temporary file is removed when it is dropped.
//!
//! Limits of the technique, by design: the target becomes a **new file**, so
//! extra hard links to the old one, its ACLs, extended attributes and (on
//! Windows) alternate data streams are not carried over. A symbolic link at
//! the target is followed: the real file behind it is replaced and the link
//! stays a link.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tempfile::NamedTempFile;

/// Replaces `dest` with what `write` puts in a fresh temporary file.
pub(crate) fn write_atomic(
    dest: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let dest = resolve_symlink(dest);
    let dest = dest.as_path();
    let dir = match dest.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let mut tmp = NamedTempFile::new_in(dir)?;
    write(tmp.as_file_mut())?;
    tmp.as_file().sync_all()?;
    platform::copy_permissions(tmp.as_file(), dest);
    persist_with_retry(tmp, dest, platform::RENAME_RETRY_DELAYS, std::thread::sleep)
}

/// The file a symbolic link at `dest` points to (so the link survives the
/// replacement); `dest` itself when it is not a link or cannot be resolved.
fn resolve_symlink(dest: &Path) -> PathBuf {
    let is_link = std::fs::symlink_metadata(dest).is_ok_and(|m| m.file_type().is_symlink());
    if !is_link {
        return dest.to_path_buf();
    }
    if let Ok(real) = std::fs::canonicalize(dest) {
        return real;
    }
    // A dangling link: write where it points.
    match std::fs::read_link(dest) {
        Ok(target) if target.is_absolute() => target,
        Ok(target) => dest
            .parent()
            .map_or(target.clone(), |dir| dir.join(&target)),
        Err(_) => dest.to_path_buf(),
    }
}

fn persist_with_retry(
    tmp: NamedTempFile,
    dest: &Path,
    delays: &[Duration],
    sleep: impl FnMut(Duration),
) -> io::Result<()> {
    let mut slot = Some(tmp);
    retry(delays, platform::is_sharing_violation, sleep, || {
        let Some(tmp) = slot.take() else {
            return Err(io::Error::other("temporary file already consumed"));
        };
        tmp.persist(dest).map(drop).map_err(|e| {
            slot = Some(e.file);
            e.error
        })
    })
}

/// Runs `op`; after a failure that `retryable` accepts, waits for each of
/// `delays` in turn and tries again. The last error is returned.
fn retry<T>(
    delays: &[Duration],
    retryable: impl Fn(&io::Error) -> bool,
    mut sleep: impl FnMut(Duration),
    mut op: impl FnMut() -> io::Result<T>,
) -> io::Result<T> {
    let mut waits = delays.iter();
    loop {
        match op() {
            Err(e) if retryable(&e) => match waits.next() {
                Some(delay) => sleep(*delay),
                None => return Err(e),
            },
            other => return other,
        }
    }
}

/// The canonical form of `path`, or its absolute form, or the path as is.
/// On Windows the `\\?\` prefix of the canonical form is removed.
pub(crate) fn canonical_or_absolute(path: &Path) -> PathBuf {
    let resolved = std::fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf());
    platform::simplify(resolved)
}

/// Whether `a` and `b` name the same file: equal canonical paths or, where
/// the platform has file identities (Unix), the same device and inode.
pub(crate) fn same_file(a: &Path, b: &Path) -> bool {
    if canonical_or_absolute(a) == canonical_or_absolute(b) {
        return true;
    }
    match (platform::file_id(a), platform::file_id(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// Removes the Windows "verbatim" prefix from the text of a path when that
/// does not change its meaning: `\\?\C:\dir` becomes `C:\dir` and
/// `\\?\UNC\server\share` becomes `\\server\share`. Anything else
/// (device paths, names the Win32 layer would alter, paths of 260
/// characters or more) is returned as `None`, meaning "keep it".
///
/// Pure text transformation, so it is tested on every platform.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub(crate) fn strip_verbatim_prefix(text: &str) -> Option<String> {
    const MAX_PATH: usize = 260;
    let rest = text.strip_prefix(r"\\?\")?;
    let (simple, body) = if let Some(unc) = rest.strip_prefix(r"UNC\") {
        (format!(r"\\{unc}"), unc)
    } else {
        let mut chars = rest.chars();
        let drive = chars.next()?;
        if !drive.is_ascii_alphabetic() || chars.next() != Some(':') {
            return None;
        }
        let after = chars.as_str();
        if !after.is_empty() && !after.starts_with('\\') {
            return None;
        }
        (rest.to_owned(), after)
    };
    let altered_by_win32 = body.split('\\').any(|part| {
        part.ends_with(' ')
            || part.ends_with('.')
            || part.contains('/')
            || reserved_device_name(part)
    });
    if altered_by_win32 || simple.chars().count() >= MAX_PATH {
        return None;
    }
    Some(simple)
}

/// A component such as `CON` or `nul.txt`, which Win32 maps to a device.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn reserved_device_name(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or("").trim_end();
    let upper = stem.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        upper.strip_prefix(prefix).is_some_and(|n| {
            matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
                || matches!(n, "\u{b9}" | "\u{b2}" | "\u{b3}")
        })
    })
}

/// The few places that differ between operating systems.
mod platform {
    use std::fs::File;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    /// `tempfile` creates files readable by the owner only (mode 0600 on
    /// Unix). Give the new file the permissions of the one it replaces, or
    /// 0644 when it is new. Best effort: a failure keeps the default.
    #[cfg(unix)]
    pub(super) fn copy_permissions(tmp: &File, dest: &Path) {
        use std::fs::Permissions;
        use std::os::unix::fs::PermissionsExt;

        let mode = std::fs::metadata(dest).map_or(0o644, |m| m.permissions().mode() & 0o7777);
        tmp.set_permissions(Permissions::from_mode(mode)).ok();
    }

    /// Windows keeps ACLs and attributes of the replaced file out of reach
    /// of `rename`; the default of the new file is acceptable.
    #[cfg(not(unix))]
    pub(super) fn copy_permissions(_tmp: &File, _dest: &Path) {}

    /// Windows: an antivirus scanner or indexer may hold the target open for
    /// a moment, which makes the rename fail; three short retries.
    #[cfg(windows)]
    pub(super) const RENAME_RETRY_DELAYS: &[Duration] = &[
        Duration::from_millis(20),
        Duration::from_millis(50),
        Duration::from_millis(100),
    ];

    /// Unix `rename` replaces open files; no retry.
    #[cfg(not(windows))]
    pub(super) const RENAME_RETRY_DELAYS: &[Duration] = &[];

    /// `ERROR_ACCESS_DENIED` (5), `ERROR_SHARING_VIOLATION` (32) and
    /// `ERROR_LOCK_VIOLATION` (33) while replacing a file in use.
    #[cfg(windows)]
    pub(super) fn is_sharing_violation(error: &io::Error) -> bool {
        matches!(error.raw_os_error(), Some(5 | 32 | 33))
    }

    /// Never retries outside Windows.
    #[cfg(not(windows))]
    pub(super) fn is_sharing_violation(_error: &io::Error) -> bool {
        false
    }

    /// Removes `\\?\` from a canonical path (see
    /// [`strip_verbatim_prefix`](super::strip_verbatim_prefix)).
    #[cfg(windows)]
    pub(super) fn simplify(path: PathBuf) -> PathBuf {
        path.to_str()
            .and_then(super::strip_verbatim_prefix)
            .map_or(path, PathBuf::from)
    }

    /// Canonical paths of Linux and macOS have no such prefix.
    #[cfg(not(windows))]
    pub(super) fn simplify(path: PathBuf) -> PathBuf {
        path
    }

    /// `(device, inode)` of an existing file.
    #[cfg(unix)]
    pub(super) fn file_id(path: &Path) -> Option<(u64, u64)> {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
    }

    /// Windows has no cheap identity in `std` (canonical paths are used).
    #[cfg(not(unix))]
    pub(super) fn file_id(_path: &Path) -> Option<(u64, u64)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn replaces_the_target_and_leaves_no_temporary() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("a b.txt");
        write_atomic(&dest, |f| f.write_all(b"one")).expect("first");
        write_atomic(&dest, |f| f.write_all(b"two")).expect("second");
        assert_eq!(std::fs::read(&dest).expect("read"), b"two");
        let entries = std::fs::read_dir(dir.path()).expect("dir").count();
        assert_eq!(entries, 1);
    }

    #[test]
    fn failure_keeps_the_target_and_removes_the_temporary() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("keep.txt");
        std::fs::write(&dest, b"old").expect("write");
        let result = write_atomic(&dest, |f| {
            f.write_all(b"partial")?;
            Err(io::Error::other("boom"))
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(&dest).expect("read"), b"old");
        assert_eq!(std::fs::read_dir(dir.path()).expect("dir").count(), 1);
    }

    #[test]
    fn missing_directory_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("nope").join("x.txt");
        assert!(write_atomic(&dest, |f| f.write_all(b"x")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn new_files_are_world_readable_and_old_modes_are_kept() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("m.txt");
        write_atomic(&dest, |f| f.write_all(b"x")).expect("write");
        let mode = std::fs::metadata(&dest).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        write_atomic(&dest, |f| f.write_all(b"y")).expect("write");
        let mode = std::fs::metadata(&dest).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn verbatim_prefixes_are_removed_only_when_safe() {
        let s = strip_verbatim_prefix;
        assert_eq!(
            s(r"\\?\C:\Users\a b\x.pgn").as_deref(),
            Some(r"C:\Users\a b\x.pgn")
        );
        assert_eq!(s(r"\\?\c:\").as_deref(), Some(r"c:\"));
        assert_eq!(
            s(r"\\?\UNC\srv\share\x.pgn").as_deref(),
            Some(r"\\srv\share\x.pgn")
        );
        // Not verbatim, or not a drive: left alone.
        assert_eq!(s(r"C:\x.pgn"), None);
        assert_eq!(s(r"\\?\Volume{1234}\x"), None);
        assert_eq!(s(r"\\.\COM1"), None);
        assert_eq!(s(r"\\?\C"), None);
        // Names that Win32 would change or map to devices keep the prefix.
        assert_eq!(s(r"\\?\C:\dir\nul.txt"), None);
        assert_eq!(s("\\\\?\\C:\\dir\\COM\u{b9}"), None);
        assert_eq!(s(r"\\?\C:\dir \x"), None);
        assert_eq!(s(r"\\?\C:\dir\x."), None);
        let long = format!(r"\\?\C:\{}", "a".repeat(300));
        assert_eq!(s(&long), None);
    }

    #[test]
    fn retry_waits_between_attempts_only_for_retryable_errors() {
        let delays = [
            Duration::from_millis(20),
            Duration::from_millis(50),
            Duration::from_millis(100),
        ];
        let mut slept = Vec::new();
        let mut calls = 0;
        let result = retry(
            &delays,
            |e| e.kind() == io::ErrorKind::WouldBlock,
            |d| slept.push(d),
            || {
                calls += 1;
                if calls < 3 {
                    Err(io::Error::from(io::ErrorKind::WouldBlock))
                } else {
                    Ok(calls)
                }
            },
        );
        assert_eq!(result.expect("third attempt works"), 3);
        assert_eq!(slept, delays[..2]);

        // Gives up after the last delay and returns the last error.
        slept.clear();
        calls = 0;
        let result: io::Result<()> = retry(
            &delays,
            |_| true,
            |d| slept.push(d),
            || {
                calls += 1;
                Err(io::Error::from(io::ErrorKind::WouldBlock))
            },
        );
        assert!(result.is_err());
        assert_eq!(calls, 4);
        assert_eq!(slept, delays);

        // Other errors are not retried.
        slept.clear();
        calls = 0;
        let result: io::Result<()> = retry(
            &delays,
            |_| false,
            |d| slept.push(d),
            || {
                calls += 1;
                Err(io::Error::from(io::ErrorKind::NotFound))
            },
        );
        assert!(result.is_err());
        assert_eq!((calls, slept.len()), (1, 0));
    }

    #[test]
    fn a_successful_persist_never_sleeps() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("t.txt");
        let tmp = NamedTempFile::new_in(dir.path()).expect("tmp");
        let mut slept = 0;
        persist_with_retry(tmp, &dest, &[Duration::from_millis(1)], |_| slept += 1)
            .expect("persist");
        assert_eq!(slept, 0);
        assert!(dest.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_target_stays_a_link_and_the_real_file_is_replaced() {
        let dir = tempfile::tempdir().expect("tempdir");
        let real_dir = dir.path().join("real");
        std::fs::create_dir(&real_dir).expect("dir");
        let real = real_dir.join("data.txt");
        std::fs::write(&real, b"old").expect("write");
        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        write_atomic(&link, |f| f.write_all(b"new")).expect("write");
        assert!(
            std::fs::symlink_metadata(&link)
                .expect("meta")
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read(&real).expect("read"), b"new");
        // No temporary file was left in the directory of the link.
        assert_eq!(std::fs::read_dir(dir.path()).expect("dir").count(), 2);
        assert_eq!(std::fs::read_dir(&real_dir).expect("dir").count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_symlink_is_written_where_it_points() {
        let dir = tempfile::tempdir().expect("tempdir");
        let link = dir.path().join("dangling.txt");
        std::os::unix::fs::symlink("target.txt", &link).expect("symlink");
        write_atomic(&link, |f| f.write_all(b"made")).expect("write");
        assert!(
            std::fs::symlink_metadata(&link)
                .expect("meta")
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read(dir.path().join("target.txt")).expect("read"),
            b"made"
        );
    }

    #[test]
    fn same_file_sees_through_dots_and_links() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("a.pgn");
        std::fs::write(&file, b"x").expect("write");
        let other = dir.path().join("b.pgn");
        std::fs::write(&other, b"x").expect("write");
        std::fs::create_dir(dir.path().join("sub")).expect("dir");
        let dotted = dir.path().join("sub").join("..").join("a.pgn");
        assert!(same_file(&file, &dotted));
        assert!(!same_file(&file, &other));
        assert!(!same_file(&file, &dir.path().join("missing.pgn")));
        #[cfg(unix)]
        {
            let link = dir.path().join("l.pgn");
            std::os::unix::fs::symlink(&file, &link).expect("symlink");
            assert!(same_file(&file, &link));
            let hard = dir.path().join("h.pgn");
            std::fs::hard_link(&file, &hard).expect("hard link");
            assert!(same_file(&file, &hard));
        }
    }
}
