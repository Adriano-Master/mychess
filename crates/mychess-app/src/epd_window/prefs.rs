//! The `[epd]` table of the preferences (spec M7-5, D3823).

use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use mychess_pgn::epd::EpdOptions;
use serde::Serialize;

/// The most recent files kept.
pub const MAX_RECENT_EPD: usize = 10;

/// Seconds per position the analysis accepts (Scid: 1-300).
pub const SECONDS_RANGE: RangeInclusive<u32> = 1..=300;

/// The seconds per position the dialog starts with.
pub const DEFAULT_SECONDS: u32 = 3;

/// What the analysis of the positions does (Scid: `epdAnnotateMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AnnotateMode {
    /// Count the best moves found (Scid mode 1).
    Count,
    /// Write the analysis into the records (Scid mode 0, the default).
    #[default]
    Annotate,
    /// Both (Scid mode 2).
    Both,
}

impl AnnotateMode {
    /// The name in the file.
    pub fn name(self) -> &'static str {
        match self {
            AnnotateMode::Count => "count",
            AnnotateMode::Annotate => "annotate",
            AnnotateMode::Both => "both",
        }
    }

    /// The mode of a name in the file.
    pub fn from_name(name: &str) -> Option<AnnotateMode> {
        match name {
            "count" => Some(AnnotateMode::Count),
            "annotate" => Some(AnnotateMode::Annotate),
            "both" => Some(AnnotateMode::Both),
            _ => None,
        }
    }

    /// Whether the mode writes the analysis.
    pub fn annotates(self) -> bool {
        matches!(self, AnnotateMode::Annotate | AnnotateMode::Both)
    }

    /// Whether the mode compares with `bm`/`am`.
    pub fn counts(self) -> bool {
        matches!(self, AnnotateMode::Count | AnnotateMode::Both)
    }
}

/// How `;` and `\` are written (D3806).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Dialect {
    /// Standard EPD: no escapes, quotes protect the `;`.
    #[default]
    Standard,
    /// The Scid's: `\s` for `;` and `\\` for `\`.
    Scid,
}

impl Dialect {
    /// The name in the file.
    pub fn name(self) -> &'static str {
        match self {
            Dialect::Standard => "standard",
            Dialect::Scid => "scid",
        }
    }

    /// The dialect of a name in the file.
    pub fn from_name(name: &str) -> Option<Dialect> {
        match name {
            "standard" => Some(Dialect::Standard),
            "scid" => Some(Dialect::Scid),
            _ => None,
        }
    }

    /// The options of the reader and the writer for this dialect.
    pub fn options(self) -> EpdOptions {
        match self {
            Dialect::Standard => EpdOptions::standard(),
            Dialect::Scid => EpdOptions::scid(),
        }
    }
}

/// What the EPD window remembers, in `[epd]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EpdPrefs {
    /// Turn the board so that the side to move is at the bottom when a
    /// record is loaded (Scid always does; D3808).
    pub flip_to_side_to_move: bool,
    /// Seconds per position of the analysis (1-300).
    pub annotate_seconds: u32,
    /// What the analysis does.
    pub annotate_mode: AnnotateMode,
    /// The engine of the analysis (an `EngineId`); `None` takes the first
    /// one that runs, or the one of F2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotate_engine: Option<u64>,
    /// How the files are written and read.
    pub dialect: Dialect,
    /// The directory of the last EPD file chosen.
    #[serde(skip_serializing_if = "path_is_unsavable")]
    pub last_dir: Option<PathBuf>,
    /// Recently opened or saved files, newest first, at most
    /// [`MAX_RECENT_EPD`].
    #[serde(serialize_with = "serialize_paths")]
    pub recent: Vec<PathBuf>,
    /// Ask before a new analysis replaces the undo of the last one
    /// (D3817); the front-end reads it, the state does not ask by itself.
    pub confirm_run_undo: bool,
}

#[allow(clippy::ref_option)] // the signature serde's `skip_serializing_if` wants
fn path_is_unsavable(path: &Option<PathBuf>) -> bool {
    path.as_ref().is_none_or(|p| p.to_str().is_none())
}

/// TOML can only hold UTF-8 text: other paths are left out.
fn serialize_paths<S: serde::Serializer>(
    paths: &[PathBuf],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let texts: Vec<&str> = paths.iter().filter_map(|p| p.to_str()).collect();
    let mut seq = serializer.serialize_seq(Some(texts.len()))?;
    for text in texts {
        seq.serialize_element(text)?;
    }
    seq.end()
}

impl Default for EpdPrefs {
    fn default() -> EpdPrefs {
        EpdPrefs {
            flip_to_side_to_move: true,
            annotate_seconds: DEFAULT_SECONDS,
            annotate_mode: AnnotateMode::Annotate,
            annotate_engine: None,
            dialect: Dialect::Standard,
            last_dir: None,
            recent: Vec::new(),
            confirm_run_undo: true,
        }
    }
}

impl EpdPrefs {
    /// The value with the numbers brought into range and the recent list
    /// without repeats, cut at [`MAX_RECENT_EPD`].
    #[must_use]
    pub fn sanitized(mut self) -> EpdPrefs {
        if !SECONDS_RANGE.contains(&self.annotate_seconds) {
            self.annotate_seconds = self
                .annotate_seconds
                .clamp(*SECONDS_RANGE.start(), *SECONDS_RANGE.end());
        }
        let mut seen: Vec<PathBuf> = Vec::new();
        for path in std::mem::take(&mut self.recent) {
            if !seen.contains(&path) {
                seen.push(path);
            }
        }
        seen.truncate(MAX_RECENT_EPD);
        self.recent = seen;
        self
    }

    /// Puts `path` first in the recent list and remembers its directory.
    pub fn remember(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(MAX_RECENT_EPD);
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            self.last_dir = Some(dir.to_path_buf());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::{PrefChange, Preferences, load_preferences, render_preferences};
    use std::fmt::Write as _;

    #[test]
    fn defaults_follow_the_spec() {
        let prefs = EpdPrefs::default();
        assert!(prefs.flip_to_side_to_move);
        assert_eq!(prefs.annotate_seconds, 3);
        assert_eq!(prefs.annotate_mode, AnnotateMode::Annotate);
        assert_eq!(prefs.dialect, Dialect::Standard);
        assert!(prefs.confirm_run_undo);
        assert_eq!(*Preferences::default().epd, prefs);
    }

    #[test]
    fn remember_keeps_ten_without_repeats() {
        let mut prefs = EpdPrefs::default();
        for i in 0..14 {
            prefs.remember(&PathBuf::from(format!("/x/f{i}.epd")));
        }
        prefs.remember(&PathBuf::from("/x/f10.epd"));
        assert_eq!(prefs.recent.len(), MAX_RECENT_EPD);
        assert_eq!(prefs.recent[0], PathBuf::from("/x/f10.epd"));
        assert_eq!(
            prefs
                .recent
                .iter()
                .filter(|p| p.ends_with("f10.epd"))
                .count(),
            1
        );
        assert_eq!(prefs.last_dir, Some(PathBuf::from("/x")));
    }

    #[test]
    fn a_change_is_sanitized() {
        let mut prefs = Preferences::default();
        PrefChange::Epd(Box::new(EpdPrefs {
            annotate_seconds: 9999,
            recent: (0..30)
                .map(|i| PathBuf::from(format!("/a/{i}.epd")))
                .collect(),
            ..EpdPrefs::default()
        }))
        .apply(&mut prefs);
        assert_eq!(prefs.epd.annotate_seconds, 300);
        assert_eq!(prefs.epd.recent.len(), MAX_RECENT_EPD);
    }

    #[test]
    fn values_round_trip_through_the_file() {
        let epd = EpdPrefs {
            flip_to_side_to_move: false,
            annotate_seconds: 17,
            annotate_mode: AnnotateMode::Both,
            annotate_engine: Some(5),
            dialect: Dialect::Scid,
            last_dir: Some(PathBuf::from("/data/epd")),
            recent: vec![
                PathBuf::from("/data/epd/a.epd"),
                PathBuf::from("/data/b.epd.gz"),
            ],
            confirm_run_undo: false,
        };
        let prefs = Preferences {
            epd: Box::new(epd.clone()),
            ..Preferences::default()
        };
        let text = render_preferences(&prefs);
        assert!(text.contains("[epd]"), "{text}");
        let loaded = load_preferences(&text);
        assert_eq!(*loaded.prefs.epd, epd);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn invalid_values_go_back_to_the_default() {
        for text in [
            "[epd]\nannotate_seconds = \"x\"\n",
            "[epd]\nannotate_mode = \"nope\"\n",
            "[epd]\ndialect = 3\n",
            "[epd]\nflip_to_side_to_move = \"yes\"\n",
            "[epd]\nrecent = 4\n",
        ] {
            let loaded = load_preferences(text);
            assert_eq!(*loaded.prefs.epd, EpdPrefs::default(), "{text}");
            assert!(!loaded.issues.is_empty(), "{text}");
        }
    }

    #[test]
    fn out_of_range_seconds_are_clamped_and_recent_is_cut() {
        let mut text = String::from("[epd]\nannotate_seconds = 5000\nrecent = [");
        for i in 0..25 {
            write!(text, "\"/r/{i}.epd\", ").expect("string");
        }
        text.push_str("1]\n");
        let loaded = load_preferences(&text);
        assert_eq!(loaded.prefs.epd.annotate_seconds, 300);
        assert_eq!(loaded.prefs.epd.recent.len(), MAX_RECENT_EPD);
        assert!(!loaded.issues.is_empty());
    }
}
