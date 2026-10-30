//! The tree preferences in the `[tree]` table of `preferences.toml` (spec
//! M5-2, D970), and the app-side mirrors of the engine's sort and mode.
//!
//! `mychess-gui` cannot name `mychess-db`, so [`TreeSort`] and
//! [`TreeFastMode`] mirror `mychess_db::tree::{Sort, FastMode}` and convert
//! to them.

use mychess_db::tree::{FastMode, Sort};
use serde::{Serialize, Serializer};

/// How the rows of the tree are ordered (Scid: `-sort`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TreeSort {
    /// By move text, ascending.
    Alphabetical,
    /// By ECO code, descending.
    Eco,
    /// By number of games, descending.
    #[default]
    Frequency,
    /// By score (best for the side to move first).
    Score,
}

impl TreeSort {
    /// Every order, in menu order.
    pub const ALL: [TreeSort; 4] = [
        TreeSort::Alphabetical,
        TreeSort::Eco,
        TreeSort::Frequency,
        TreeSort::Score,
    ];

    /// The spelling used in the preferences file.
    pub fn name(self) -> &'static str {
        match self {
            TreeSort::Alphabetical => "alphabetical",
            TreeSort::Eco => "eco",
            TreeSort::Frequency => "frequency",
            TreeSort::Score => "score",
        }
    }

    /// Reads the spelling used in the preferences file.
    pub fn from_name(text: &str) -> Option<TreeSort> {
        TreeSort::ALL.into_iter().find(|s| s.name() == text)
    }

    pub(crate) fn engine(self) -> Sort {
        match self {
            TreeSort::Alphabetical => Sort::Alpha,
            TreeSort::Eco => Sort::Eco,
            TreeSort::Frequency => Sort::Frequency,
            TreeSort::Score => Sort::Score,
        }
    }
}

/// Which games a refresh examines (Scid: `-fastmode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TreeFastMode {
    /// Every game (the "slow" mode).
    #[default]
    Slow,
    /// Only the games of the last full search; can miss transpositions.
    Fast,
    /// A fast pass, then a full one that replaces it.
    FastThenSlow,
}

impl TreeFastMode {
    /// Every mode, in menu order.
    pub const ALL: [TreeFastMode; 3] = [
        TreeFastMode::Slow,
        TreeFastMode::Fast,
        TreeFastMode::FastThenSlow,
    ];

    /// The spelling used in the preferences file.
    pub fn name(self) -> &'static str {
        match self {
            TreeFastMode::Slow => "slow",
            TreeFastMode::Fast => "fast",
            TreeFastMode::FastThenSlow => "fast_then_slow",
        }
    }

    /// Reads the spelling used in the preferences file.
    pub fn from_name(text: &str) -> Option<TreeFastMode> {
        TreeFastMode::ALL.into_iter().find(|m| m.name() == text)
    }

    pub(crate) fn engine(self) -> FastMode {
        match self {
            TreeFastMode::Slow => FastMode::Slow,
            TreeFastMode::Fast => FastMode::Fast,
            TreeFastMode::FastThenSlow => FastMode::FastThenSlow,
        }
    }
}

/// The persisted options of the tree panel (spec M5-2, D970).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreePrefs {
    /// Hide the average rating, performance, average year and ECO columns.
    pub short: bool,
    /// The order of the rows.
    pub sort: TreeSort,
    /// Which games a refresh examines.
    pub fast_mode: TreeFastMode,
    /// "Adjust filter": the main filter of the base follows the tree.
    pub auto_adjust: bool,
    /// Capacity of the cache of trees, within [`CACHE_SIZE_RANGE`] (spec
    /// M5-3b, D1760).
    pub cache_size: usize,
    /// Save the cache of the trees when the panel closes and when the
    /// program ends (D1761).
    pub cache_autosave: bool,
}

/// The smallest and the largest size of the cache of trees (Scid: the
/// `Cache size` menu of the tree window).
pub const CACHE_SIZE_RANGE: std::ops::RangeInclusive<usize> = 250..=10_000;

/// The sizes the menu offers; the default is the middle one.
pub const CACHE_SIZES: [usize; 6] = [250, 500, 1000, 2000, 5000, 10_000];

/// The default size of the cache (the Scid's 1000).
pub const DEFAULT_CACHE_SIZE: usize = 1000;

impl Default for TreePrefs {
    fn default() -> TreePrefs {
        TreePrefs {
            short: true,
            sort: TreeSort::default(),
            fast_mode: TreeFastMode::default(),
            auto_adjust: false,
            cache_size: DEFAULT_CACHE_SIZE,
            cache_autosave: false,
        }
    }
}

#[derive(Serialize)]
struct Table {
    short: bool,
    sort: &'static str,
    fast_mode: &'static str,
    auto_adjust: bool,
    cache_size: u64,
    cache_autosave: bool,
}

impl Serialize for TreePrefs {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Table {
            short: self.short,
            sort: self.sort.name(),
            fast_mode: self.fast_mode.name(),
            auto_adjust: self.auto_adjust,
            cache_size: u64::try_from(self.cache_size).unwrap_or(1000),
            cache_autosave: self.cache_autosave,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for sort in TreeSort::ALL {
            assert_eq!(TreeSort::from_name(sort.name()), Some(sort));
        }
        for mode in TreeFastMode::ALL {
            assert_eq!(TreeFastMode::from_name(mode.name()), Some(mode));
        }
        assert_eq!(TreeSort::from_name("Frequency"), None);
    }

    #[test]
    fn the_file_round_trips_and_tolerates_garbage() {
        let prefs = crate::Preferences {
            tree: TreePrefs {
                short: false,
                sort: TreeSort::Score,
                fast_mode: TreeFastMode::FastThenSlow,
                auto_adjust: true,
                cache_size: 2000,
                cache_autosave: true,
            },
            ..crate::Preferences::default()
        };
        let text = crate::render_preferences(&prefs);
        assert!(text.contains("[tree]"), "{text}");
        let loaded = crate::load_preferences(&text);
        assert_eq!(loaded.prefs.tree, prefs.tree);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);

        let loaded = crate::load_preferences(
            "[tree]\nshort = 3\nsort = \"x\"\nfast_mode = \"fast\"\nauto_adjust = true\n",
        );
        let want = TreePrefs {
            fast_mode: TreeFastMode::Fast,
            auto_adjust: true,
            ..TreePrefs::default()
        };
        assert_eq!(loaded.prefs.tree, want);
        assert_eq!(loaded.issues.len(), 2);
        let loaded = crate::load_preferences("tree = 1\n");
        assert_eq!(loaded.prefs.tree, TreePrefs::default());
    }
}
