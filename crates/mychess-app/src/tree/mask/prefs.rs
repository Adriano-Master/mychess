//! The mask preferences, in the `[tree_mask]` table of `preferences.toml`
//! (spec M5-4, D1819, D1831, D1861).

use std::path::{Path, PathBuf};

use serde::{Serialize, Serializer};

/// Whether two paths are the same file name: without regard to case on
/// Windows and macOS, whose file systems do not tell them apart (D1879).
pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(any(windows, target_os = "macos")) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// The longest list of recent mask files (Scid: `maxRecent 10`).
pub const MAX_RECENT_MASKS: usize = 10;

/// The persisted options of the tree masks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskPrefs {
    /// Recently opened or saved mask files, newest first, with no repeated
    /// path. A file that is gone stays here until the user removes it
    /// (D1819).
    pub recent: Vec<PathBuf>,
    /// Open the newest recent file when the tree panel opens without a mask
    /// (D1820). Scid: `TreeOptAutomask`.
    pub auto_load: bool,
    /// Draw the markers, the color and the NAG of the moves (D1831; the
    /// Scid starts with it off, the MyChess with it on).
    pub show_markers: bool,
    /// Show the whole comment in a tool tip.
    pub tooltips: bool,
    /// The directory of the last mask file chosen.
    pub last_dir: Option<PathBuf>,
}

impl Default for MaskPrefs {
    fn default() -> MaskPrefs {
        MaskPrefs {
            recent: Vec::new(),
            auto_load: false,
            show_markers: true,
            tooltips: true,
            last_dir: None,
        }
    }
}

impl MaskPrefs {
    /// Puts `path` first in the recent list (and removes it from where it
    /// was), keeping at most [`MAX_RECENT_MASKS`], and remembers its
    /// directory.
    pub fn push_recent(&mut self, path: &Path) {
        self.recent.retain(|p| !same_path(p, path));
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(MAX_RECENT_MASKS);
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            self.last_dir = Some(dir.to_path_buf());
        }
    }

    /// Takes `path` out of the recent list; whether it was there.
    pub fn remove_recent(&mut self, path: &Path) -> bool {
        let before = self.recent.len();
        self.recent.retain(|p| !same_path(p, path));
        self.recent.len() != before
    }

    /// The list as a file may give it: no repeated path, at most
    /// [`MAX_RECENT_MASKS`].
    pub(crate) fn sanitized(mut self) -> MaskPrefs {
        let mut seen: Vec<PathBuf> = Vec::new();
        for path in std::mem::take(&mut self.recent) {
            if seen.len() < MAX_RECENT_MASKS && !seen.iter().any(|p| same_path(p, &path)) {
                seen.push(path);
            }
        }
        self.recent = seen;
        self
    }
}

/// TOML holds UTF-8 text only: other paths are left out (D131).
fn text(path: &Path) -> Option<&str> {
    path.to_str()
}

#[derive(Serialize)]
struct Table<'a> {
    recent: Vec<&'a str>,
    auto_load: bool,
    show_markers: bool,
    tooltips: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_dir: Option<&'a str>,
}

impl Serialize for MaskPrefs {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Table {
            recent: self.recent.iter().filter_map(|p| text(p)).collect(),
            auto_load: self.auto_load,
            show_markers: self.show_markers,
            tooltips: self.tooltips,
            last_dir: self.last_dir.as_deref().and_then(text),
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_is_newest_first_without_repeats_and_capped() {
        let mut prefs = MaskPrefs::default();
        for n in 0..12 {
            prefs.push_recent(Path::new(&format!("dir/m{n}.stm")));
        }
        assert_eq!(prefs.recent.len(), MAX_RECENT_MASKS);
        assert_eq!(prefs.recent[0], Path::new("dir/m11.stm"));
        prefs.push_recent(Path::new("dir/m5.stm"));
        assert_eq!(prefs.recent[0], Path::new("dir/m5.stm"));
        assert_eq!(
            prefs.recent.iter().filter(|p| *p == "dir/m5.stm").count(),
            1
        );
        assert_eq!(prefs.last_dir.as_deref(), Some(Path::new("dir")));
        assert!(prefs.remove_recent(Path::new("dir/m5.stm")));
        assert!(!prefs.remove_recent(Path::new("dir/m5.stm")));
    }

    #[test]
    fn defaults_show_markers_and_tooltips() {
        let prefs = MaskPrefs::default();
        assert!(prefs.show_markers && prefs.tooltips && !prefs.auto_load);
    }

    #[test]
    fn sanitizing_drops_repeats() {
        let prefs = MaskPrefs {
            recent: vec!["a.stm".into(), "b.stm".into(), "a.stm".into()],
            ..MaskPrefs::default()
        }
        .sanitized();
        assert_eq!(
            prefs.recent,
            [PathBuf::from("a.stm"), PathBuf::from("b.stm")]
        );
    }
}
