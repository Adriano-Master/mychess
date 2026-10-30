//! The `[tablebase]` table of the preferences (spec M8-8, D2562).

use std::path::PathBuf;

use serde::Serialize;

/// Most folders of tables.
pub const MAX_DIRECTORIES: usize = mychess_tablebase::local::MAX_DIRECTORIES;
/// Longest folder path kept, in characters.
pub const MAX_PATH_CHARS: usize = 4096;

/// What the user configured about the tablebases.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct TablebasePrefs {
    /// Use the Syzygy files of `directories`.
    pub local: bool,
    /// The folders that hold the `.rtbw` and `.rtbz` files. Text, so that the
    /// file can always be written; a path that is not UTF-8 is not kept.
    pub directories: Vec<String>,
    /// Ask the Lichess online tablebase (off until the user turns it on).
    pub online: bool,
}

impl TablebasePrefs {
    /// Trims the paths, drops the empty, the too long and the repeated ones
    /// and keeps at most [`MAX_DIRECTORIES`].
    #[must_use]
    pub fn sanitized(mut self) -> TablebasePrefs {
        let mut seen: Vec<String> = Vec::new();
        for dir in std::mem::take(&mut self.directories) {
            let dir = dir.trim().to_owned();
            if dir.is_empty() || dir.chars().count() > MAX_PATH_CHARS || seen.contains(&dir) {
                continue;
            }
            if seen.len() < MAX_DIRECTORIES {
                seen.push(dir);
            }
        }
        self.directories = seen;
        self
    }

    /// The folders as paths.
    #[must_use]
    pub fn paths(&self) -> Vec<PathBuf> {
        self.directories.iter().map(PathBuf::from).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitized_drops_empty_repeated_and_excess_folders() {
        let prefs = TablebasePrefs {
            local: true,
            directories: vec![
                " a ".to_owned(),
                String::new(),
                "a".to_owned(),
                "x".repeat(MAX_PATH_CHARS + 1),
            ]
            .into_iter()
            .chain((0..20).map(|i| format!("d{i}")))
            .collect(),
            online: false,
        }
        .sanitized();
        assert_eq!(prefs.directories.len(), MAX_DIRECTORIES);
        assert_eq!(prefs.directories[0], "a");
        assert_eq!(prefs.directories[1], "d0");
    }
}
