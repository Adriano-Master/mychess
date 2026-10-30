//! What the exercise remembers between runs: `calvar.toml` (spec M8-6,
//! D2317, D2318).
//!
//! The file sits beside `preferences.toml`. Reading is tolerant (a wrong
//! value only resets its own field); a file that was read and is not valid
//! is kept as `calvar.toml.corrupt`; one from a newer version is read for
//! the fields this version knows and never written. Blocking: only
//! [`perform`](super::effect::perform) calls [`load_file`] and
//! [`save_file`].

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use super::config::{CalvarConfig, MAX_FEN_CHARS, PositionSource};
use super::review::ReviewRecord;
use crate::error::{FileError, FileErrorKind};
use crate::fsutil::write_atomic;

/// The name of the file.
pub const STORE_FILE: &str = "calvar.toml";
/// The version written.
pub const STORE_VERSION: i64 = 1;
/// Exercises kept in the history; the oldest goes first (D2317).
pub const MAX_HISTORY: usize = 100;
/// Largest file read, in bytes (D2317).
const MAX_STORE_BYTES: u64 = 8 << 20;

/// The path of the store, beside the preferences file.
pub fn store_path(preferences: &Path) -> PathBuf {
    preferences.with_file_name(STORE_FILE)
}

/// One finished exercise (D2317).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    /// When it ended, in seconds since the Unix epoch (the front-end says).
    pub when: u64,
    /// The score, 0 to 100.
    pub score: u8,
    /// Good moves found.
    pub found: u8,
    /// Good moves there were.
    pub good: u8,
    /// Lines the user gave.
    pub lines: u8,
    /// A crucial move was missed.
    pub crucial: bool,
    /// White was to move.
    pub white_to_move: bool,
    /// The position.
    pub fen: String,
    /// The lines, evaluations and analysis, for the review (M8-6d, D2367).
    /// `None` for entries written before the review existed.
    pub detail: Option<Box<ReviewRecord>>,
}

/// The counters of all the exercises (D2317).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CalvarStats {
    /// Exercises finished.
    pub done: u32,
    /// The sum of the scores.
    pub total_score: u64,
    /// The best score.
    pub best_score: u8,
    /// Lines given.
    pub lines: u32,
    /// Good moves found.
    pub found: u32,
    /// Good moves there were.
    pub good: u32,
}

impl CalvarStats {
    /// The mean score, if any exercise was done.
    pub fn mean_score(&self) -> Option<u8> {
        if self.done == 0 {
            return None;
        }
        u8::try_from(self.total_score / u64::from(self.done)).ok()
    }
}

/// Everything saved in `calvar.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CalvarStore {
    /// The configuration of the last exercise.
    pub config: CalvarConfig,
    /// The counters.
    pub stats: CalvarStats,
    /// The last exercises, oldest first.
    pub history: Vec<HistoryEntry>,
}

impl CalvarStore {
    /// Records a finished exercise in the counters and the history.
    pub fn record(&mut self, entry: HistoryEntry) {
        let s = &mut self.stats;
        s.done = s.done.saturating_add(1);
        s.total_score = s.total_score.saturating_add(u64::from(entry.score));
        s.best_score = s.best_score.max(entry.score);
        s.lines = s.lines.saturating_add(u32::from(entry.lines));
        s.found = s.found.saturating_add(u32::from(entry.found));
        s.good = s.good.saturating_add(u32::from(entry.good));
        self.history.push(entry);
        let extra = self.history.len().saturating_sub(MAX_HISTORY);
        self.history.drain(..extra);
    }

    /// Clears the counters and the history (D2318).
    pub fn reset(&mut self) {
        self.stats = CalvarStats::default();
        self.history.clear();
    }
}

/// What reading the file found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoadedStore {
    /// The data (the defaults when the file is missing or unreadable).
    pub store: CalvarStore,
    /// The file could not be read (it may have been set aside).
    pub unreadable: bool,
    /// The file was written by a newer version of the program.
    pub newer: bool,
    /// The file must not be written.
    pub read_only: bool,
}

fn get_uint(table: &Table, key: &str) -> Option<u64> {
    table
        .get(key)
        .and_then(Value::as_integer)
        .and_then(|n| u64::try_from(n).ok())
}

fn get_u8(table: &Table, key: &str) -> Option<u8> {
    get_uint(table, key).and_then(|n| u8::try_from(n).ok())
}

fn get_bool(table: &Table, key: &str) -> Option<bool> {
    table.get(key).and_then(Value::as_bool)
}

fn get_str<'a>(table: &'a Table, key: &str) -> Option<&'a str> {
    table
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| s.chars().count() <= MAX_FEN_CHARS)
}

fn parse_config(table: Option<&Table>) -> CalvarConfig {
    let defaults = CalvarConfig::default();
    let Some(t) = table else {
        return defaults;
    };
    CalvarConfig {
        source: get_str(t, "source")
            .and_then(PositionSource::from_name)
            .unwrap_or(defaults.source),
        fen: get_str(t, "fen").map_or(defaults.fen, str::to_owned),
        engine: get_uint(t, "engine"),
        lines_required: get_u8(t, "lines_required").unwrap_or(defaults.lines_required),
        multipv: get_u8(t, "multipv").unwrap_or(defaults.multipv),
        root_ms: get_uint(t, "root_ms").unwrap_or(defaults.root_ms),
        line_ms: get_uint(t, "line_ms").unwrap_or(defaults.line_ms),
        time_limit_s: get_uint(t, "time_limit_s")
            .and_then(|n| u16::try_from(n).ok())
            .unwrap_or(defaults.time_limit_s),
        follow_board: get_bool(t, "follow_board").unwrap_or(defaults.follow_board),
    }
    .sanitized()
}

fn parse_stats(table: Option<&Table>) -> CalvarStats {
    let Some(t) = table else {
        return CalvarStats::default();
    };
    let u32_of = |key: &str| get_uint(t, key).and_then(|n| u32::try_from(n).ok());
    CalvarStats {
        done: u32_of("done").unwrap_or(0),
        total_score: get_uint(t, "total_score").unwrap_or(0),
        best_score: get_u8(t, "best_score").unwrap_or(0).min(100),
        lines: u32_of("lines").unwrap_or(0),
        found: u32_of("found").unwrap_or(0),
        good: u32_of("good").unwrap_or(0),
    }
}

fn parse_entry(t: &Table) -> Option<HistoryEntry> {
    Some(HistoryEntry {
        when: get_uint(t, "when")?,
        score: get_u8(t, "score")?.min(100),
        found: get_u8(t, "found").unwrap_or(0),
        good: get_u8(t, "good").unwrap_or(0),
        lines: get_u8(t, "lines").unwrap_or(0),
        crucial: get_bool(t, "crucial").unwrap_or(false),
        white_to_move: get_bool(t, "white_to_move").unwrap_or(true),
        fen: get_str(t, "fen")?.to_owned(),
        detail: t
            .get("detail")
            .and_then(Value::as_table)
            .and_then(ReviewRecord::from_table)
            .map(Box::new),
    })
}

/// Reads a store from text: `None` when it is not TOML.
pub fn parse_store(text: &str) -> Option<CalvarStore> {
    parse_versioned(text).map(|(store, _)| store)
}

fn parse_versioned(text: &str) -> Option<(CalvarStore, i64)> {
    let root: Table = text.parse().ok()?;
    let version = root
        .get("version")
        .and_then(Value::as_integer)
        .unwrap_or(STORE_VERSION);
    let mut store = CalvarStore {
        config: parse_config(root.get("config").and_then(Value::as_table)),
        stats: parse_stats(root.get("stats").and_then(Value::as_table)),
        history: Vec::new(),
    };
    if let Some(Value::Array(entries)) = root.get("history") {
        store.history = entries
            .iter()
            .filter_map(Value::as_table)
            .filter_map(parse_entry)
            .collect();
        let extra = store.history.len().saturating_sub(MAX_HISTORY);
        store.history.drain(..extra);
    }
    Some((store, version))
}

fn int(n: u64) -> Value {
    Value::Integer(i64::try_from(n).unwrap_or(i64::MAX))
}

/// The text of a store.
///
/// # Errors
///
/// The serializer refused a value (not expected).
pub fn render_store(store: &CalvarStore) -> Result<String, toml::ser::Error> {
    let c = &store.config;
    let mut config = Table::new();
    config.insert("source".into(), Value::String(c.source.as_str().into()));
    config.insert("fen".into(), Value::String(c.fen.clone()));
    if let Some(engine) = c.engine {
        config.insert("engine".into(), int(engine));
    }
    config.insert("lines_required".into(), int(u64::from(c.lines_required)));
    config.insert("multipv".into(), int(u64::from(c.multipv)));
    config.insert("root_ms".into(), int(c.root_ms));
    config.insert("line_ms".into(), int(c.line_ms));
    config.insert("time_limit_s".into(), int(u64::from(c.time_limit_s)));
    config.insert("follow_board".into(), Value::Boolean(c.follow_board));
    let s = &store.stats;
    let mut stats = Table::new();
    stats.insert("done".into(), int(u64::from(s.done)));
    stats.insert("total_score".into(), int(s.total_score));
    stats.insert("best_score".into(), int(u64::from(s.best_score)));
    stats.insert("lines".into(), int(u64::from(s.lines)));
    stats.insert("found".into(), int(u64::from(s.found)));
    stats.insert("good".into(), int(u64::from(s.good)));
    let history: Vec<Value> = store
        .history
        .iter()
        .map(|e| {
            let mut t = Table::new();
            t.insert("when".into(), int(e.when));
            t.insert("score".into(), int(u64::from(e.score)));
            t.insert("found".into(), int(u64::from(e.found)));
            t.insert("good".into(), int(u64::from(e.good)));
            t.insert("lines".into(), int(u64::from(e.lines)));
            t.insert("crucial".into(), Value::Boolean(e.crucial));
            t.insert("white_to_move".into(), Value::Boolean(e.white_to_move));
            t.insert("fen".into(), Value::String(e.fen.clone()));
            if let Some(detail) = &e.detail {
                t.insert("detail".into(), Value::Table(detail.to_table()));
            }
            Value::Table(t)
        })
        .collect();
    let mut root = Table::new();
    root.insert("version".into(), Value::Integer(STORE_VERSION));
    root.insert("config".into(), Value::Table(config));
    root.insert("stats".into(), Value::Table(stats));
    if !history.is_empty() {
        root.insert("history".into(), Value::Array(history));
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

/// Reads the store at `path` (D2317): a missing file gives the defaults; a
/// file that was read and is too large, not UTF-8 or not TOML is set aside
/// as `.corrupt`; one that could not be read is left and the store becomes
/// `read_only`; one of a newer version is read and never written.
pub fn load_file(path: &Path) -> LoadedStore {
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
    match std::str::from_utf8(content).ok().and_then(parse_versioned) {
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
pub fn save_file(path: &Path, store: &CalvarStore) -> Result<(), FileError> {
    let fail = |e: &std::io::Error| FileError::from_io(e, Some(path.to_path_buf()));
    let text = render_store(store).map_err(|error| {
        tracing::error!(%error, "could not render the calvar store");
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

    fn entry(score: u8) -> HistoryEntry {
        HistoryEntry {
            when: 1_700_000_000,
            score,
            found: 2,
            good: 3,
            lines: 3,
            crucial: false,
            white_to_move: true,
            fen: "8/8/8/8/8/8/8/K6k w - - 0 1".to_owned(),
            detail: None,
        }
    }

    #[test]
    fn a_store_survives_the_round_trip() {
        let mut store = CalvarStore::default();
        store.config.engine = Some(4);
        store.config.follow_board = true;
        store.config.fen = "x".to_owned();
        store.record(entry(70));
        store.record(entry(90));
        let text = render_store(&store).expect("render");
        assert_eq!(parse_store(&text), Some(store));
    }

    #[test]
    fn the_history_keeps_the_last_hundred() {
        let mut store = CalvarStore::default();
        for n in 0..150_u32 {
            let mut e = entry(50);
            e.when = u64::from(n);
            store.record(e);
        }
        assert_eq!(store.history.len(), MAX_HISTORY);
        assert_eq!(store.history[0].when, 50);
        assert_eq!(store.stats.done, 150);
        assert_eq!(store.stats.mean_score(), Some(50));
    }

    #[test]
    fn a_wrong_value_resets_only_its_own_field() {
        let text = "[config]\nmultipv = \"many\"\nline_ms = 2000\n[stats]\nbest_score = 400\n";
        let store = parse_store(text).expect("parse");
        assert_eq!(store.config.multipv, CalvarConfig::default().multipv);
        assert_eq!(store.config.line_ms, 2000);
        assert_eq!(store.stats.best_score, 0, "400 is no score: the default");
    }

    #[test]
    fn not_toml_is_none() {
        assert_eq!(parse_store("[[["), None);
    }
}
