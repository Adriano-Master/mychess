//! The recent files list (spec M2-2, D131; Scid: `tcl/file/recent.tcl`).

use std::path::{Path, PathBuf};

use serde::{Serialize, Serializer};

/// Most files remembered.
pub const MAX_RECENT_FILES: usize = 10;

/// Whether two paths name the same file in the list. Windows and macOS file
/// systems ignore the case by default (`multiplataforma.md`), so there the
/// ASCII case is ignored too.
pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    if cfg!(any(windows, target_os = "macos")) {
        return a
            .to_str()
            .zip(b.to_str())
            .is_some_and(|(x, y)| x.eq_ignore_ascii_case(y));
    }
    false
}

/// Recently used files, newest first, without duplicates.
///
/// Paths are compared as given: the caller passes the canonical form
/// (`perform` canonicalises after a successful open or save).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecentFiles(Vec<PathBuf>);

impl RecentFiles {
    /// Moves `path` to the front (adding it if new); the oldest entry is
    /// dropped beyond [`MAX_RECENT_FILES`].
    pub fn push(&mut self, path: PathBuf) {
        self.0.retain(|p| !same_path(p, &path));
        self.0.insert(0, path);
        self.0.truncate(MAX_RECENT_FILES);
    }

    /// Forgets `path` (a file that could not be opened).
    pub fn remove(&mut self, path: &Path) {
        self.0.retain(|p| !same_path(p, path));
    }

    /// Forgets every file.
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// The files, newest first.
    pub fn iter(&self) -> impl Iterator<Item = &PathBuf> {
        self.0.iter()
    }

    /// The nth file (0 is the newest).
    pub fn get(&self, index: usize) -> Option<&PathBuf> {
        self.0.get(index)
    }

    /// How many files are remembered.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the list is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for RecentFiles {
    /// Paths that are not UTF-8 are left out (TOML holds text only).
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let texts: Vec<&str> = self.0.iter().filter_map(|p| p.to_str()).collect();
        texts.serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(n: usize) -> PathBuf {
        PathBuf::from(format!("dir with space/g{n}.pgn"))
    }

    #[test]
    fn clear_forgets_every_file() {
        let mut recent = RecentFiles::default();
        recent.push(path(1));
        recent.clear();
        assert!(recent.is_empty());
    }

    #[test]
    fn case_only_differences_are_one_file_where_the_file_system_ignores_case() {
        let mut recent = RecentFiles::default();
        recent.push(PathBuf::from("Games/Big.pgn"));
        recent.push(PathBuf::from("games/big.PGN"));
        let expected = if cfg!(any(windows, target_os = "macos")) {
            1
        } else {
            2
        };
        assert_eq!(recent.len(), expected);
    }

    #[test]
    fn push_moves_to_front_without_duplicates() {
        let mut recent = RecentFiles::default();
        recent.push(path(1));
        recent.push(path(2));
        recent.push(path(1));
        let list: Vec<_> = recent.iter().cloned().collect();
        assert_eq!(list, [path(1), path(2)]);
    }

    #[test]
    fn the_eleventh_pushes_out_the_oldest() {
        let mut recent = RecentFiles::default();
        for n in 0..=MAX_RECENT_FILES {
            recent.push(path(n));
        }
        assert_eq!(recent.len(), MAX_RECENT_FILES);
        assert_eq!(recent.get(0), Some(&path(MAX_RECENT_FILES)));
        assert!(recent.iter().all(|p| *p != path(0)));
    }

    #[test]
    fn remove_forgets_a_file() {
        let mut recent = RecentFiles::default();
        recent.push(path(1));
        recent.push(path(2));
        recent.remove(&path(1));
        assert_eq!(recent.len(), 1);
        recent.remove(&path(9));
        assert_eq!(recent.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_are_not_serialised() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let mut recent = RecentFiles::default();
        recent.push(PathBuf::from(OsStr::from_bytes(b"bad\xff.pgn")));
        recent.push(path(1));
        let text = toml::to_string(&std::collections::BTreeMap::from([("recent", &recent)]))
            .expect("serialise");
        assert!(text.contains("g1.pgn"));
        assert!(!text.contains("bad"));
    }
}
