//! Reading and writing the preferences file. Blocking: only
//! [`perform`](crate::perform) calls it, from `spawn_blocking`.

use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use toml::Table;

use super::{
    LoadedPreferences, PREFERENCES_VERSION, Preferences, PrefsIssue, try_render_preferences,
};
use crate::error::{FileError, FileErrorKind};
use crate::fsutil::write_atomic;

/// `path` with `suffix` appended to the file name.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Keeps an unreadable file as `<name>.corrupt` (replacing an older one).
fn quarantine(path: &Path) {
    // Best effort: if the rename fails the file is simply read again (and
    // rejected again) next time.
    std::fs::rename(path, with_suffix(path, ".corrupt")).ok();
}

/// Largest preferences file that is read (the real one is a few KiB).
const MAX_PREFS_BYTES: u64 = 1 << 20;

/// The UTF-8 byte order mark that some editors add.
const BOM: &[u8] = &[0xef, 0xbb, 0xbf];

/// Reads at most [`MAX_PREFS_BYTES`]; `Ok(None)` when the file is larger.
fn read_limited(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_PREFS_BYTES + 1)
        .read_to_end(&mut bytes)?;
    Ok((bytes.len() as u64 <= MAX_PREFS_BYTES).then_some(bytes))
}

fn unreadable() -> LoadedPreferences {
    LoadedPreferences {
        prefs: Preferences::default(),
        issues: vec![PrefsIssue::Unreadable],
    }
}

/// Reads the preferences at `path`. A missing file gives the defaults
/// without remarks; a file that cannot be read, is too large, is not UTF-8
/// (a BOM is accepted) or is not TOML is renamed to `<name>.corrupt`, so the
/// next save does not destroy it, and the defaults come with
/// [`PrefsIssue::Unreadable`].
pub(crate) fn load_file(path: &Path) -> LoadedPreferences {
    let bytes = match read_limited(path) {
        Ok(Some(bytes)) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return LoadedPreferences::default();
        }
        // Too large, or unreadable for another reason: set it aside so the
        // next save does not destroy it.
        Ok(None) | Err(_) => {
            // A directory is not a file of ours: leave it alone (the next
            // save fails and says so).
            if !path.is_dir() {
                quarantine(path);
            }
            return unreadable();
        }
    };
    let content = bytes.strip_prefix(BOM).unwrap_or(&bytes);
    let Ok(text) = std::str::from_utf8(content) else {
        quarantine(path);
        return unreadable();
    };
    let loaded = super::load_preferences(text);
    if loaded.issues.contains(&PrefsIssue::Unreadable) {
        quarantine(path);
    }
    loaded
}

/// The `version` of the file at `path`, when it is newer than ours.
fn newer_version_on_disk(path: &Path) -> Option<i64> {
    let bytes = read_limited(path).ok()??;
    let text = std::str::from_utf8(bytes.strip_prefix(BOM).unwrap_or(&bytes)).ok()?;
    let version = text.parse::<Table>().ok()?.get("version")?.as_integer()?;
    (version > i64::from(PREFERENCES_VERSION)).then_some(version)
}

/// Writes the preferences atomically, creating the directory. Before it
/// replaces a file written by a newer version, that file is copied to
/// `<name>.bak-v<N>` (once).
pub(crate) fn save_file(path: &Path, prefs: &Preferences) -> Result<(), FileError> {
    let fail = |e: &std::io::Error| FileError::from_io(e, Some(path.to_path_buf()));
    // Render first: a value the serializer refuses must leave the file (and
    // the directory) untouched, not replace it with an empty one (D1310).
    let text = try_render_preferences(prefs).map_err(|error| {
        tracing::error!(%error, "could not render the preferences");
        FileError::new(
            FileErrorKind::Io(std::io::ErrorKind::InvalidData),
            Some(path.to_path_buf()),
        )
    })?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| fail(&e))?;
    }
    if let Some(version) = newer_version_on_disk(path) {
        let backup = with_suffix(path, &format!(".bak-v{version}"));
        if !backup.exists() {
            std::fs::copy(path, &backup).map_err(|e| fail(&e))?;
        }
    }
    write_atomic(path, |file| {
        file.write_all(text.as_bytes())?;
        file.flush()
    })
    .map_err(|e| fail(&e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::ThemePref;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let loaded = load_file(&dir.path().join("none.toml"));
        assert_eq!(loaded, LoadedPreferences::default());
    }

    #[test]
    fn save_creates_directories_and_replaces() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a").join("b c").join("preferences.toml");
        let mut prefs = Preferences::default();
        prefs.general.theme = ThemePref::Dark;
        save_file(&path, &prefs).expect("first save");
        prefs.general.theme = ThemePref::Light;
        save_file(&path, &prefs).expect("second save");
        assert_eq!(load_file(&path).prefs.general.theme, ThemePref::Light);
        let siblings = std::fs::read_dir(path.parent().expect("parent"))
            .expect("dir")
            .count();
        assert_eq!(siblings, 1, "no temporary files are left");
    }

    #[test]
    fn corrupt_files_are_quarantined_and_replaced_next_time() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("preferences.toml");
        std::fs::write(&path, "= = broken").expect("write");
        let loaded = load_file(&path);
        assert_eq!(loaded.issues, vec![PrefsIssue::Unreadable]);
        assert!(!path.exists());
        let corrupt = dir.path().join("preferences.toml.corrupt");
        assert_eq!(std::fs::read(&corrupt).expect("corrupt"), b"= = broken");
        std::fs::write(&path, "again broken [").expect("write");
        drop(load_file(&path));
        assert_eq!(std::fs::read(&corrupt).expect("corrupt"), b"again broken [");
    }

    #[test]
    fn non_utf8_files_are_quarantined() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("preferences.toml");
        std::fs::write(&path, [0xff, 0xfe, 0x00, 0x41]).expect("write");
        let loaded = load_file(&path);
        assert_eq!(loaded.issues, vec![PrefsIssue::Unreadable]);
        assert!(dir.path().join("preferences.toml.corrupt").exists());
    }

    #[test]
    fn a_utf8_bom_is_accepted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("preferences.toml");
        let mut bytes = vec![0xef, 0xbb, 0xbf];
        bytes.extend_from_slice(b"version = 1\n[general]\ntheme = \"dark\"\n");
        std::fs::write(&path, bytes).expect("write");
        let loaded = load_file(&path);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
        assert_eq!(loaded.prefs.general.theme, ThemePref::Dark);
        assert!(path.exists(), "a good file is not moved");
    }

    #[test]
    fn oversized_files_are_quarantined_not_read() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("preferences.toml");
        let mut big = b"# ".to_vec();
        big.resize(usize::try_from(MAX_PREFS_BYTES).expect("fits") + 10, b'x');
        std::fs::write(&path, &big).expect("write");
        let loaded = load_file(&path);
        assert_eq!(loaded.issues, vec![PrefsIssue::Unreadable]);
        assert!(!path.exists());
        assert!(dir.path().join("preferences.toml.corrupt").exists());
    }

    #[test]
    fn a_directory_in_place_of_the_file_is_left_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("preferences.toml");
        std::fs::create_dir(&path).expect("dir");
        let loaded = load_file(&path);
        assert_eq!(loaded.issues, vec![PrefsIssue::Unreadable]);
        assert!(path.is_dir());
        assert!(!dir.path().join("preferences.toml.corrupt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_cannot_be_read_is_kept_aside_so_a_save_cannot_destroy_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("preferences.toml");
        std::fs::write(&path, "[board]\ntheme = \"wood\"\n").expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        if File::open(&path).is_ok() {
            return; // running as root: permissions do not apply
        }
        let loaded = load_file(&path);
        assert_eq!(loaded.issues, vec![PrefsIssue::Unreadable]);
        assert!(!path.exists());
        assert!(dir.path().join("preferences.toml.corrupt").exists());
    }

    #[test]
    fn newer_files_are_backed_up_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("preferences.toml");
        std::fs::write(&path, "version = 99\nfuture = true\n").expect("write");
        let loaded = load_file(&path);
        assert!(
            loaded
                .issues
                .contains(&PrefsIssue::NewerVersion { found: 99 })
        );
        save_file(&path, &loaded.prefs).expect("save");
        let backup = dir.path().join("preferences.toml.bak-v99");
        assert_eq!(
            std::fs::read_to_string(&backup).expect("backup"),
            "version = 99\nfuture = true\n"
        );
        save_file(&path, &loaded.prefs).expect("save again");
        assert_eq!(
            std::fs::read_to_string(&backup).expect("backup"),
            "version = 99\nfuture = true\n"
        );
    }

    #[test]
    fn a_value_the_serializer_refuses_does_not_wipe_the_file() {
        // Regression (D1310): `unwrap_or_default` wrote an empty file.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("preferences.toml");
        let mut prefs = Preferences::default();
        prefs.general.theme = ThemePref::Dark;
        save_file(&path, &prefs).expect("first save");
        let before = std::fs::read(&path).expect("read");
        assert!(!before.is_empty());
        prefs.tournament.participants = vec![crate::tournament::Participant::new(u64::MAX, "A")];
        assert!(save_file(&path, &prefs).is_err());
        assert_eq!(std::fs::read(&path).expect("read"), before);
    }

    #[test]
    fn saving_into_a_missing_read_only_place_fails_cleanly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let blocker = dir.path().join("file");
        std::fs::write(&blocker, "x").expect("write");
        // A directory cannot be created below a regular file.
        let path = blocker.join("preferences.toml");
        assert!(save_file(&path, &Preferences::default()).is_err());
    }
}
