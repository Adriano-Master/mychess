//! What the training remembers between runs: `tactics.toml` (spec M8-4,
//! D1924, D1926, D1927).
//!
//! The file sits beside `preferences.toml`. Reading is tolerant (a wrong
//! value only resets its own field); an unreadable file is kept as
//! `tactics.toml.corrupt`. Blocking: only [`perform`](super::effect::perform)
//! calls [`load_file`] and [`save_file`].

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use toml::{Table, Value};

use super::config::{SourceKind, TacticsConfig, TacticsMode, TacticsOrder};
use super::stats::{ModeStats, TacticsStats};
use crate::error::{FileError, FileErrorKind};
use crate::fsutil::write_atomic;

/// The name of the file.
pub const STORE_FILE: &str = "tactics.toml";
/// The version written.
pub const STORE_VERSION: i64 = 1;
/// Sources whose solved problems are kept; the oldest goes first (D1926).
pub const MAX_SOURCES: usize = 16;
/// Solved problems kept per source (D1926).
pub const MAX_SOLVED_PER_SOURCE: usize = 60_000;
/// Largest file read, in bytes. It must stay above the largest file that
/// [`save_file`] can write: [`MAX_SOURCES`] sources of
/// [`MAX_SOLVED_PER_SOURCE`] ids of up to 19 digits and a separator is about
/// 21 MB (D1955).
const MAX_STORE_BYTES: u64 = 32 << 20;
/// Longest text kept for a key or a path, in characters.
const MAX_TEXT_CHARS: usize = 1024;

/// The path of the store, beside the preferences file.
pub fn store_path(preferences: &Path) -> PathBuf {
    preferences.with_file_name(STORE_FILE)
}

/// The problems solved in one source (D1904).
#[derive(Debug, Clone, PartialEq, Eq)]
struct SolvedSet {
    key: String,
    /// Ascending, without repetition.
    ids: Vec<u64>,
}

/// Everything saved in `tactics.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TacticsStore {
    /// The configuration of the last session.
    pub config: TacticsConfig,
    /// The score.
    pub stats: TacticsStats,
    sources: Vec<SolvedSet>,
}

impl TacticsStore {
    /// Keys that are not remembered between runs (the Clipbase has none).
    fn persistent(key: &str) -> bool {
        !key.is_empty()
    }

    /// The ids solved in `key`, ascending.
    pub fn solved_ids(&self, key: &str) -> Arc<Vec<u64>> {
        Arc::new(
            self.sources
                .iter()
                .find(|s| s.key == key)
                .map(|s| s.ids.clone())
                .unwrap_or_default(),
        )
    }

    /// Whether `id` was solved in `key`.
    pub fn is_solved(&self, key: &str, id: u64) -> bool {
        self.sources
            .iter()
            .find(|s| s.key == key)
            .is_some_and(|s| s.ids.binary_search(&id).is_ok())
    }

    /// How many problems were solved in `key`.
    pub fn solved_count(&self, key: &str) -> usize {
        self.sources
            .iter()
            .find(|s| s.key == key)
            .map_or(0, |s| s.ids.len())
    }

    /// Records that `id` was solved in `key`.
    pub fn mark_solved(&mut self, key: &str, id: u64) {
        let existing = self.sources.iter().position(|s| s.key == key);
        let position = existing.unwrap_or_else(|| {
            self.sources.push(SolvedSet {
                key: key.to_owned(),
                ids: Vec::new(),
            });
            self.sources.len() - 1
        });
        // The source used last goes to the end, so the oldest is dropped.
        let mut set = self.sources.remove(position);
        if let Err(at) = set.ids.binary_search(&id)
            && set.ids.len() < MAX_SOLVED_PER_SOURCE
        {
            set.ids.insert(at, id);
        }
        self.sources.push(set);
        let overflow = self.sources.len().saturating_sub(MAX_SOURCES);
        self.sources.drain(..overflow);
    }

    /// Forgets the solved problems of `key` (D1927).
    pub fn reset_source(&mut self, key: &str) {
        self.sources.retain(|s| s.key != key);
    }

    /// Clears the counters of `mode` (D1927).
    pub fn reset_stats(&mut self, mode: TacticsMode) {
        *self.stats.of_mut(mode) = ModeStats::default();
    }

    /// Adds what a store loaded from disk has to this one, which may
    /// already have changed: the solved problems are joined; the config and
    /// the score are taken only when this store has not been used yet.
    pub fn merge_loaded(&mut self, loaded: TacticsStore, used: bool) {
        for set in loaded.sources {
            for id in set.ids {
                self.mark_solved(&set.key, id);
            }
        }
        if !used {
            self.config = loaded.config;
            self.stats = loaded.stats;
        }
    }
}

/// What reading the file found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoadedStore {
    /// The data (the defaults when the file is missing or unreadable).
    pub store: TacticsStore,
    /// The file could not be read (it may have been set aside).
    pub unreadable: bool,
    /// The file was written by a newer version of the program (D1954).
    pub newer: bool,
    /// The file must not be written: it is from a newer version, or it could
    /// not be read for a reason that may pass (D1954, D1956).
    pub read_only: bool,
}

fn get_uint(table: &Table, key: &str) -> Option<u64> {
    table
        .get(key)
        .and_then(Value::as_integer)
        .and_then(|n| u64::try_from(n).ok())
}

fn get_u32(table: &Table, key: &str) -> Option<u32> {
    get_uint(table, key).and_then(|n| u32::try_from(n).ok())
}

fn get_bool(table: &Table, key: &str) -> Option<bool> {
    table.get(key).and_then(Value::as_bool)
}

fn get_str<'a>(table: &'a Table, key: &str) -> Option<&'a str> {
    table
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| s.chars().count() <= MAX_TEXT_CHARS)
}

fn parse_mode_stats(table: Option<&Table>) -> ModeStats {
    let Some(t) = table else {
        return ModeStats::default();
    };
    ModeStats {
        shown: get_u32(t, "shown").unwrap_or(0),
        perfect: get_u32(t, "perfect").unwrap_or(0),
        with_mistakes: get_u32(t, "with_mistakes").unwrap_or(0),
        with_help: get_u32(t, "with_help").unwrap_or(0),
        failed: get_u32(t, "failed").unwrap_or(0),
        skipped: get_u32(t, "skipped").unwrap_or(0),
        total_ms: get_uint(t, "total_ms").unwrap_or(0),
        streak: get_u32(t, "streak").unwrap_or(0),
        best_streak: get_u32(t, "best_streak").unwrap_or(0),
    }
}

fn parse_config(table: Option<&Table>) -> TacticsConfig {
    let defaults = TacticsConfig::default();
    let Some(t) = table else {
        return defaults;
    };
    let small = |key: &str, default: u8| {
        get_uint(t, key)
            .and_then(|n| u8::try_from(n).ok())
            .unwrap_or(default)
    };
    let wide = |key: &str, default: u16| {
        get_uint(t, key)
            .and_then(|n| u16::try_from(n).ok())
            .unwrap_or(default)
    };
    TacticsConfig {
        mode: get_str(t, "mode")
            .and_then(TacticsMode::from_name)
            .unwrap_or(defaults.mode),
        order: get_str(t, "order")
            .and_then(TacticsOrder::from_name)
            .unwrap_or(defaults.order),
        source: get_str(t, "source")
            .and_then(SourceKind::from_name)
            .unwrap_or(defaults.source),
        file: get_str(t, "file")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from),
        mate_min: small("mate_min", defaults.mate_min),
        mate_max: small("mate_max", defaults.mate_max),
        difficulty_min: small("difficulty_min", defaults.difficulty_min),
        difficulty_max: small("difficulty_max", defaults.difficulty_max),
        elo_min: wide("elo_min", defaults.elo_min),
        elo_max: wide("elo_max", defaults.elo_max),
        include_solved: get_bool(t, "include_solved").unwrap_or(defaults.include_solved),
        max_attempts: small("max_attempts", defaults.max_attempts),
        hints: get_bool(t, "hints").unwrap_or(defaults.hints),
        accept_longer: get_bool(t, "accept_longer").unwrap_or(defaults.accept_longer),
        scan_mates: get_bool(t, "scan_mates").unwrap_or(defaults.scan_mates),
        second_opinion: get_bool(t, "second_opinion").unwrap_or(defaults.second_opinion),
        flagged_only: get_bool(t, "flagged_only").unwrap_or(defaults.flagged_only),
        time_limit_s: wide("time_limit_s", defaults.time_limit_s),
    }
    .sanitized()
}

/// Reads the text of a store; `None` when it is not TOML at all.
pub fn parse_store(text: &str) -> Option<TacticsStore> {
    parse_versioned(text).map(|(store, _)| store)
}

/// Like [`parse_store`], with the `version` the file declares (1 when it
/// declares none).
fn parse_versioned(text: &str) -> Option<(TacticsStore, i64)> {
    let root: Table = text.parse().ok()?;
    let version = root
        .get("version")
        .and_then(Value::as_integer)
        .unwrap_or(STORE_VERSION);
    let mut store = TacticsStore {
        config: parse_config(root.get("config").and_then(Value::as_table)),
        ..TacticsStore::default()
    };
    if let Some(stats) = root.get("stats").and_then(Value::as_table) {
        store.stats.mate = parse_mode_stats(stats.get("mate").and_then(Value::as_table));
        store.stats.best = parse_mode_stats(stats.get("best").and_then(Value::as_table));
    }
    if let Some(Value::Array(sources)) = root.get("sources") {
        for source in sources.iter().filter_map(Value::as_table).take(MAX_SOURCES) {
            let Some(key) = get_str(source, "key").filter(|k| TacticsStore::persistent(k)) else {
                continue;
            };
            let mut ids: Vec<u64> = source
                .get("solved")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_integer)
                        .filter_map(|n| u64::try_from(n).ok())
                        .take(MAX_SOLVED_PER_SOURCE)
                        .collect()
                })
                .unwrap_or_default();
            ids.sort_unstable();
            ids.dedup();
            store.sources.push(SolvedSet {
                key: key.to_owned(),
                ids,
            });
        }
    }
    Some((store, version))
}

fn int(n: u64) -> Value {
    Value::Integer(i64::try_from(n).unwrap_or(i64::MAX))
}

fn mode_table(s: &ModeStats) -> Value {
    let mut t = Table::new();
    t.insert("shown".into(), int(u64::from(s.shown)));
    t.insert("perfect".into(), int(u64::from(s.perfect)));
    t.insert("with_mistakes".into(), int(u64::from(s.with_mistakes)));
    t.insert("with_help".into(), int(u64::from(s.with_help)));
    t.insert("failed".into(), int(u64::from(s.failed)));
    t.insert("skipped".into(), int(u64::from(s.skipped)));
    t.insert("total_ms".into(), int(s.total_ms));
    t.insert("streak".into(), int(u64::from(s.streak)));
    t.insert("best_streak".into(), int(u64::from(s.best_streak)));
    Value::Table(t)
}

/// The text of a store.
///
/// # Errors
///
/// The serializer refused a value (not expected).
pub fn render_store(store: &TacticsStore) -> Result<String, toml::ser::Error> {
    let c = &store.config;
    let mut config = Table::new();
    config.insert("mode".into(), Value::String(c.mode.as_str().into()));
    config.insert("order".into(), Value::String(c.order.as_str().into()));
    config.insert("source".into(), Value::String(c.source.as_str().into()));
    if let Some(file) = &c.file {
        config.insert("file".into(), Value::String(file.display().to_string()));
    }
    config.insert("mate_min".into(), int(u64::from(c.mate_min)));
    config.insert("mate_max".into(), int(u64::from(c.mate_max)));
    config.insert("difficulty_min".into(), int(u64::from(c.difficulty_min)));
    config.insert("difficulty_max".into(), int(u64::from(c.difficulty_max)));
    config.insert("elo_min".into(), int(u64::from(c.elo_min)));
    config.insert("elo_max".into(), int(u64::from(c.elo_max)));
    config.insert("include_solved".into(), Value::Boolean(c.include_solved));
    config.insert("max_attempts".into(), int(u64::from(c.max_attempts)));
    config.insert("hints".into(), Value::Boolean(c.hints));
    config.insert("accept_longer".into(), Value::Boolean(c.accept_longer));
    config.insert("scan_mates".into(), Value::Boolean(c.scan_mates));
    config.insert("second_opinion".into(), Value::Boolean(c.second_opinion));
    config.insert("flagged_only".into(), Value::Boolean(c.flagged_only));
    config.insert("time_limit_s".into(), int(u64::from(c.time_limit_s)));
    let mut stats = Table::new();
    stats.insert("mate".into(), mode_table(&store.stats.mate));
    stats.insert("best".into(), mode_table(&store.stats.best));
    let sources: Vec<Value> = store
        .sources
        .iter()
        .filter(|s| TacticsStore::persistent(&s.key))
        .map(|s| {
            let mut t = Table::new();
            t.insert("key".into(), Value::String(s.key.clone()));
            t.insert(
                "solved".into(),
                Value::Array(s.ids.iter().map(|id| int(*id)).collect()),
            );
            Value::Table(t)
        })
        .collect();
    let mut root = Table::new();
    root.insert("version".into(), Value::Integer(STORE_VERSION));
    root.insert("config".into(), Value::Table(config));
    root.insert("stats".into(), Value::Table(stats));
    if !sources.is_empty() {
        root.insert("sources".into(), Value::Array(sources));
    }
    toml::to_string(&root)
}

fn read_limited(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_STORE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    Ok((bytes.len() as u64 <= MAX_STORE_BYTES).then_some(bytes))
}

/// Renames the file to `<name>.corrupt`; `false` when that failed.
fn set_aside(path: &Path) -> bool {
    let mut name = path.as_os_str().to_owned();
    name.push(".corrupt");
    std::fs::rename(path, PathBuf::from(name)).is_ok()
}

/// Reads the store at `path`: a missing file gives the defaults. A file
/// that was read and is too large, is not UTF-8 or is not TOML is renamed to
/// `<name>.corrupt` and the defaults come with `unreadable`. A file that
/// could not be read at all (permissions, a sharing violation on Windows) is
/// left where it is and the store becomes `read_only`, so the defaults never
/// replace it (D1956). A file of a newer version is read for the fields this
/// version knows and is never written (D1954).
pub fn load_file(path: &Path) -> LoadedStore {
    // A file that could not be moved away stays where it is, and the
    // defaults must not be written over it either (D1956).
    let set_aside_and_default = |path: &Path| LoadedStore {
        unreadable: true,
        read_only: path.is_dir() || !set_aside(path),
        ..LoadedStore::default()
    };
    let bytes = match read_limited(path) {
        Ok(Some(bytes)) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return LoadedStore::default(),
        Ok(None) => return set_aside_and_default(path),
        Err(_) => {
            return LoadedStore {
                unreadable: true,
                read_only: true,
                ..LoadedStore::default()
            };
        }
    };
    let content = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    let parsed = std::str::from_utf8(content).ok().and_then(parse_versioned);
    match parsed {
        Some((store, version)) => {
            let newer = version > STORE_VERSION;
            LoadedStore {
                store,
                unreadable: false,
                newer,
                read_only: newer,
            }
        }
        None => set_aside_and_default(path),
    }
}

/// Writes the store atomically, creating the directory.
///
/// # Errors
///
/// A [`FileError`] when the file cannot be written.
pub fn save_file(path: &Path, store: &TacticsStore) -> Result<(), FileError> {
    let fail = |e: &std::io::Error| FileError::from_io(e, Some(path.to_path_buf()));
    let text = render_store(store).map_err(|error| {
        tracing::error!(%error, "could not render the tactics store");
        FileError::new(
            FileErrorKind::Io(std::io::ErrorKind::InvalidData),
            Some(path.to_path_buf()),
        )
    })?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| fail(&e))?;
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

    fn sample() -> TacticsStore {
        let mut store = TacticsStore::default();
        store.config.mode = TacticsMode::BestMove;
        store.config.order = TacticsOrder::Random;
        store.config.file = Some(PathBuf::from("problems.pgn"));
        store.config.mate_max = 4;
        store.config.accept_longer = true;
        store.config.scan_mates = false;
        store.config.second_opinion = true;
        store.config.flagged_only = true;
        store.stats.mate.shown = 7;
        store.stats.mate.perfect = 3;
        store.stats.best.best_streak = 5;
        store.mark_solved("a.pgn", 30);
        store.mark_solved("a.pgn", 10);
        store.mark_solved("b.pgn", 99);
        store
    }

    #[test]
    fn the_store_round_trips() {
        let store = sample();
        let text = render_store(&store).expect("render");
        assert_eq!(parse_store(&text), Some(store));
    }

    #[test]
    fn the_solved_ids_are_kept_sorted_and_without_repetition() {
        let mut store = TacticsStore::default();
        for id in [5, 3, 5, 9, 3] {
            store.mark_solved("k", id);
        }
        assert_eq!(*store.solved_ids("k"), vec![3, 5, 9]);
        assert!(store.is_solved("k", 9));
        assert!(!store.is_solved("k", 4));
        assert!(!store.is_solved("other", 3));
    }

    #[test]
    fn a_wrong_value_resets_only_its_own_field() {
        let text = "version = 1\n[config]\nmode = \"chess\"\nmate_max = 4\nhints = \"yes\"\n\
                    [stats.mate]\nshown = -3\nperfect = 2\n";
        let store = parse_store(text).expect("toml");
        assert_eq!(store.config.mode, TacticsMode::Mate);
        assert_eq!(store.config.mate_max, 4);
        assert!(store.config.hints);
        assert_eq!(store.stats.mate.shown, 0);
        assert_eq!(store.stats.mate.perfect, 2);
    }

    #[test]
    fn numbers_out_of_range_are_brought_into_range() {
        let text = "[config]\nmax_attempts = 200\nmate_min = 9\n";
        let store = parse_store(text).expect("toml");
        assert_eq!(store.config.max_attempts, 9);
        // 9 is brought to 5, which is above the default maximum of 3: the
        // bounds swap.
        assert_eq!((store.config.mate_min, store.config.mate_max), (3, 5));
    }

    #[test]
    fn a_file_that_is_not_toml_is_rejected() {
        assert_eq!(parse_store("this is = = not toml"), None);
    }

    #[test]
    fn at_most_sixteen_sources_are_kept_and_the_oldest_goes_first() {
        let mut store = TacticsStore::default();
        for i in 0..20 {
            store.mark_solved(&format!("s{i}"), 1);
        }
        assert!(!store.is_solved("s0", 1));
        assert!(!store.is_solved("s3", 1));
        assert!(store.is_solved("s4", 1));
        assert!(store.is_solved("s19", 1));
    }

    #[test]
    fn the_clipbase_with_no_key_is_not_written() {
        let mut store = TacticsStore::default();
        store.mark_solved("", 4);
        let text = render_store(&store).expect("render");
        assert!(!text.contains("sources"));
        assert!(parse_store(&text).expect("toml").solved_ids("").is_empty());
    }

    #[test]
    fn resetting_clears_one_source_or_one_mode_only() {
        let mut store = sample();
        store.reset_source("a.pgn");
        assert_eq!(store.solved_count("a.pgn"), 0);
        assert_eq!(store.solved_count("b.pgn"), 1);
        store.reset_stats(TacticsMode::Mate);
        assert_eq!(store.stats.mate, ModeStats::default());
        assert_eq!(store.stats.best.best_streak, 5);
    }

    #[test]
    fn merging_joins_the_solved_sets_and_keeps_a_used_score() {
        let mut live = TacticsStore::default();
        live.mark_solved("a.pgn", 1);
        live.stats.mate.shown = 1;
        live.merge_loaded(sample(), true);
        assert_eq!(*live.solved_ids("a.pgn"), vec![1, 10, 30]);
        assert_eq!(live.stats.mate.shown, 1);
        let mut fresh = TacticsStore::default();
        fresh.merge_loaded(sample(), false);
        assert_eq!(fresh.stats.mate.shown, 7);
        assert_eq!(fresh.config.mode, TacticsMode::BestMove);
    }

    #[test]
    fn save_then_load_through_the_file_system() {
        let dir = tempfile::tempdir().expect("dir");
        let path = store_path(&dir.path().join("preferences.toml"));
        assert_eq!(path.file_name().and_then(|n| n.to_str()), Some(STORE_FILE));
        let store = sample();
        save_file(&path, &store).expect("save");
        let loaded = load_file(&path);
        assert!(!loaded.unreadable);
        assert_eq!(loaded.store, store);
    }

    #[test]
    fn the_largest_store_that_can_be_written_can_be_read_back() {
        let mut store = TacticsStore::default();
        for source in 0..MAX_SOURCES {
            let key = format!("source-{source}.pgn");
            for n in 0..MAX_SOLVED_PER_SOURCE {
                // 19 digits: the widest ids the file can hold.
                store.mark_solved(&key, i64::MAX as u64 - (MAX_SOLVED_PER_SOURCE - n) as u64);
            }
        }
        let text = render_store(&store).expect("renders");
        assert!(
            (text.len() as u64) <= MAX_STORE_BYTES,
            "{} bytes written, {MAX_STORE_BYTES} read",
            text.len()
        );
    }

    #[test]
    fn a_file_of_a_newer_version_is_read_but_marked_read_only() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join(STORE_FILE);
        std::fs::write(
            &path,
            "version = 99\nfuture_key = 1\n[config]\nmode = \"best\"\n",
        )
        .expect("write");
        let loaded = load_file(&path);
        assert!(loaded.newer && loaded.read_only);
        assert!(!loaded.unreadable);
        assert_eq!(loaded.store.config.mode, TacticsMode::BestMove);
        assert!(path.exists(), "the file stays where it is");
    }

    #[test]
    fn a_corrupt_file_that_cannot_be_set_aside_is_not_overwritten_either() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join(STORE_FILE);
        std::fs::write(&path, "= = =").expect("write");
        // A full directory where the `.corrupt` name should go: the rename
        // fails on every platform.
        let target = dir.path().join("tactics.toml.corrupt");
        std::fs::create_dir(&target).expect("dir");
        std::fs::write(target.join("keep"), "x").expect("write");
        let loaded = load_file(&path);
        assert!(loaded.unreadable && loaded.read_only);
        assert!(path.exists(), "the file stays where it is");
    }

    #[test]
    fn a_file_that_cannot_be_opened_is_not_renamed_and_not_overwritten() {
        // A directory where the file should be: opening it for reading fails
        // with an I/O error that is not a content problem.
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join(STORE_FILE);
        std::fs::create_dir(&path).expect("dir as file");
        let loaded = load_file(&path);
        assert!(loaded.unreadable && loaded.read_only);
        assert!(path.is_dir());
        assert!(!dir.path().join("tactics.toml.corrupt").exists());
    }

    #[test]
    fn a_file_from_before_the_search_for_mates_gets_it_on() {
        // `scan_mates` is a key that older files do not have (D1975).
        let store = parse_store("version = 1\n[config]\nmode = \"mate\"\n").expect("parses");
        assert!(store.config.scan_mates);
        let off = parse_store("[config]\nscan_mates = false\n").expect("parses");
        assert!(!off.config.scan_mates);
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let dir = tempfile::tempdir().expect("dir");
        let loaded = load_file(&dir.path().join(STORE_FILE));
        assert_eq!(loaded, LoadedStore::default());
    }

    #[test]
    fn a_corrupt_file_is_set_aside_and_the_defaults_come_back() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join(STORE_FILE);
        std::fs::write(&path, "= = =").expect("write");
        let loaded = load_file(&path);
        assert!(loaded.unreadable);
        assert!(!path.exists());
        assert!(dir.path().join("tactics.toml.corrupt").exists());
    }

    #[test]
    fn a_file_with_a_byte_order_mark_is_read() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join(STORE_FILE);
        let mut bytes = vec![0xef, 0xbb, 0xbf];
        bytes.extend_from_slice(b"[stats.mate]\nshown = 4\n");
        std::fs::write(&path, bytes).expect("write");
        assert_eq!(load_file(&path).store.stats.mate.shown, 4);
    }
}
