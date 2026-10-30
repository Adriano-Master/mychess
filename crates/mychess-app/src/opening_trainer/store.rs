//! What the trainer remembers between runs: `opening_trainer.toml` (spec
//! M8-5, D2222-D2227).
//!
//! The file sits beside `preferences.toml`. Reading is tolerant (a wrong
//! value only resets its own field); an unreadable file is kept as
//! `opening_trainer.toml.corrupt`; a file of a newer version is read for the
//! fields this version knows and **never written back**. Blocking: only
//! [`perform`](super::effect::perform) calls [`load_file`] and
//! [`save_file`].

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use super::config::{OpeningConfig, RepSourceKind, TrainSide};
use super::stats::{PosStat, TOP_BOX};
use crate::error::{FileError, FileErrorKind};
use crate::fsutil::write_atomic;

/// The name of the file.
pub const STORE_FILE: &str = "opening_trainer.toml";
/// The version written.
pub const STORE_VERSION: i64 = 1;
/// Repertoires whose score is kept; the least recently used goes first.
pub const MAX_REPS: usize = 8;
/// Positions kept per repertoire (D2225).
pub const MAX_POSITIONS_PER_REP: usize = 60_000;
/// Largest file read, in bytes. It must stay above the largest file that
/// [`save_file`] can write (a test checks it).
const MAX_STORE_BYTES: u64 = 64 << 20;
/// Longest text kept for a key or a path, in characters.
const MAX_TEXT_CHARS: usize = 1024;

/// The path of the store, beside the preferences file.
pub fn store_path(preferences: &Path) -> PathBuf {
    preferences.with_file_name(STORE_FILE)
}

/// The score of one repertoire (D2222).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RepRecord {
    key: String,
    positions: BTreeMap<u64, PosStat>,
}

/// Everything saved in `opening_trainer.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OpeningStore {
    /// The configuration of the last session.
    pub config: OpeningConfig,
    reps: Vec<RepRecord>,
}

impl OpeningStore {
    /// Keys that are not remembered between runs.
    fn persistent(key: &str) -> bool {
        !key.is_empty()
    }

    fn index_of(&self, key: &str) -> Option<usize> {
        self.reps.iter().position(|r| r.key == key)
    }

    /// Makes `key` the most recently used repertoire (creating it) and drops
    /// the oldest one beyond [`MAX_REPS`].
    pub fn use_rep(&mut self, key: &str) {
        let record = match self.index_of(key) {
            Some(i) => self.reps.remove(i),
            None => RepRecord {
                key: key.to_owned(),
                positions: BTreeMap::new(),
            },
        };
        self.reps.push(record);
        let overflow = self.reps.len().saturating_sub(MAX_REPS);
        self.reps.drain(..overflow);
    }

    /// The record of a position of `key`.
    pub fn stat(&self, key: &str, hash: u64) -> Option<&PosStat> {
        self.reps
            .iter()
            .find(|r| r.key == key)
            .and_then(|r| r.positions.get(&hash))
    }

    /// The record of a position of `key`, created when missing; `None` when
    /// the repertoire is full (D2225) or unknown.
    pub fn stat_mut(&mut self, key: &str, hash: u64) -> Option<&mut PosStat> {
        let record = self.reps.iter_mut().find(|r| r.key == key)?;
        if !record.positions.contains_key(&hash) && record.positions.len() >= MAX_POSITIONS_PER_REP
        {
            return None;
        }
        Some(record.positions.entry(hash).or_default())
    }

    /// How many positions have a record in `key`.
    pub fn recorded(&self, key: &str) -> usize {
        self.reps
            .iter()
            .find(|r| r.key == key)
            .map_or(0, |r| r.positions.len())
    }

    /// The records of `key`.
    pub fn positions(&self, key: &str) -> impl Iterator<Item = (&u64, &PosStat)> {
        self.reps
            .iter()
            .find(|r| r.key == key)
            .into_iter()
            .flat_map(|r| r.positions.iter())
    }

    /// Forgets the score of `key` (D2226).
    pub fn reset(&mut self, key: &str) {
        self.reps.retain(|r| r.key != key);
    }

    /// Adds what a store loaded from disk has to this one, which may already
    /// have changed: positions already recorded in memory win; the
    /// configuration is taken only when this store has not been used yet.
    pub fn merge_loaded(&mut self, loaded: OpeningStore, used: bool) {
        for record in loaded.reps {
            let known = self.index_of(&record.key);
            if let Some(mine) = known.and_then(|i| self.reps.get_mut(i)) {
                for (hash, stat) in record.positions {
                    if mine.positions.len() < MAX_POSITIONS_PER_REP {
                        mine.positions.entry(hash).or_insert(stat);
                    }
                }
            } else if known.is_none() {
                self.reps.insert(0, record);
                let overflow = self.reps.len().saturating_sub(MAX_REPS);
                // The loaded ones are the oldest: they go first.
                self.reps.drain(..overflow);
            }
        }
        if !used {
            self.config = loaded.config;
        }
    }
}

/// What reading the file found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoadedStore {
    /// The data (the defaults when the file is missing or unreadable).
    pub store: OpeningStore,
    /// The file could not be read (it may have been set aside).
    pub unreadable: bool,
    /// The file was written by a newer version of the program.
    pub newer: bool,
    /// The file must not be written: newer version, or read failure that may
    /// pass.
    pub read_only: bool,
}

fn get_uint(table: &Table, key: &str) -> Option<u64> {
    table
        .get(key)
        .and_then(Value::as_integer)
        .and_then(|n| u64::try_from(n).ok())
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

fn parse_config(table: Option<&Table>) -> OpeningConfig {
    let defaults = OpeningConfig::default();
    let Some(t) = table else {
        return defaults;
    };
    OpeningConfig {
        side: get_str(t, "side")
            .and_then(TrainSide::from_name)
            .unwrap_or(defaults.side),
        source: get_str(t, "source")
            .and_then(RepSourceKind::from_name)
            .unwrap_or(defaults.source),
        file: get_str(t, "file")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from),
        max_depth: get_uint(t, "max_depth")
            .and_then(|n| u8::try_from(n).ok())
            .unwrap_or(defaults.max_depth),
        strict: get_bool(t, "strict").unwrap_or(defaults.strict),
        opponent_best: get_bool(t, "opponent_best").unwrap_or(defaults.opponent_best),
        no_repeat: get_bool(t, "no_repeat").unwrap_or(defaults.no_repeat),
        only_flagged: get_bool(t, "only_flagged").unwrap_or(defaults.only_flagged),
        show_candidates: get_bool(t, "show_candidates").unwrap_or(defaults.show_candidates),
        only_marked: get_bool(t, "only_marked").unwrap_or(defaults.only_marked),
    }
    .sanitized()
}

/// The `u64` hash as the `i64` that TOML can hold (same bits).
fn hash_to_i64(hash: u64) -> i64 {
    i64::from_ne_bytes(hash.to_ne_bytes())
}

fn hash_from_i64(value: i64) -> u64 {
    u64::from_ne_bytes(value.to_ne_bytes())
}

fn parse_position(value: &Value) -> Option<(u64, PosStat)> {
    let row = value.as_array()?;
    let int = |i: usize| row.get(i).and_then(Value::as_integer);
    let small = |i: usize| int(i).and_then(|n| u32::try_from(n).ok());
    let hash = hash_from_i64(int(0)?);
    let box_no = int(5)
        .and_then(|n| u8::try_from(n).ok())
        .unwrap_or(0)
        .min(TOP_BOX);
    Some((
        hash,
        PosStat {
            seen: small(1).unwrap_or(0),
            good: small(2).unwrap_or(0),
            dubious: small(3).unwrap_or(0),
            out: small(4).unwrap_or(0),
            box_no,
            due: small(6).unwrap_or(0),
        },
    ))
}

/// Reads the text of a store; `None` when it is not TOML at all.
pub fn parse_store(text: &str) -> Option<OpeningStore> {
    parse_versioned(text).map(|(store, _)| store)
}

fn parse_versioned(text: &str) -> Option<(OpeningStore, i64)> {
    let root: Table = text.parse().ok()?;
    let version = root
        .get("version")
        .and_then(Value::as_integer)
        .unwrap_or(STORE_VERSION);
    let mut store = OpeningStore {
        config: parse_config(root.get("config").and_then(Value::as_table)),
        ..OpeningStore::default()
    };
    if let Some(Value::Array(reps)) = root.get("rep") {
        for rep in reps.iter().filter_map(Value::as_table).take(MAX_REPS) {
            let Some(key) = get_str(rep, "key").filter(|k| OpeningStore::persistent(k)) else {
                continue;
            };
            let mut positions = BTreeMap::new();
            if let Some(rows) = rep.get("pos").and_then(Value::as_array) {
                for row in rows.iter().take(MAX_POSITIONS_PER_REP) {
                    if let Some((hash, stat)) = parse_position(row) {
                        positions.insert(hash, stat);
                    }
                }
            }
            store.reps.push(RepRecord {
                key: key.to_owned(),
                positions,
            });
        }
    }
    Some((store, version))
}

fn int(n: u64) -> Value {
    Value::Integer(i64::try_from(n).unwrap_or(i64::MAX))
}

fn position_row(hash: u64, s: &PosStat) -> Value {
    Value::Array(vec![
        Value::Integer(hash_to_i64(hash)),
        int(u64::from(s.seen)),
        int(u64::from(s.good)),
        int(u64::from(s.dubious)),
        int(u64::from(s.out)),
        int(u64::from(s.box_no)),
        int(u64::from(s.due)),
    ])
}

/// The text of a store.
///
/// # Errors
///
/// The serializer refused a value (not expected).
pub fn render_store(store: &OpeningStore) -> Result<String, toml::ser::Error> {
    let c = &store.config;
    let mut config = Table::new();
    config.insert("side".into(), Value::String(c.side.as_str().into()));
    config.insert("source".into(), Value::String(c.source.as_str().into()));
    if let Some(file) = &c.file {
        config.insert("file".into(), Value::String(file.display().to_string()));
    }
    config.insert("max_depth".into(), int(u64::from(c.max_depth)));
    config.insert("strict".into(), Value::Boolean(c.strict));
    config.insert("opponent_best".into(), Value::Boolean(c.opponent_best));
    config.insert("no_repeat".into(), Value::Boolean(c.no_repeat));
    config.insert("only_flagged".into(), Value::Boolean(c.only_flagged));
    config.insert("show_candidates".into(), Value::Boolean(c.show_candidates));
    config.insert("only_marked".into(), Value::Boolean(c.only_marked));
    let reps: Vec<Value> = store
        .reps
        .iter()
        .filter(|r| OpeningStore::persistent(&r.key) && !r.positions.is_empty())
        .map(|r| {
            let mut t = Table::new();
            t.insert("key".into(), Value::String(r.key.clone()));
            t.insert(
                "pos".into(),
                Value::Array(
                    r.positions
                        .iter()
                        .take(MAX_POSITIONS_PER_REP)
                        .map(|(h, s)| position_row(*h, s))
                        .collect(),
                ),
            );
            Value::Table(t)
        })
        .collect();
    let mut root = Table::new();
    root.insert("version".into(), Value::Integer(STORE_VERSION));
    root.insert("config".into(), Value::Table(config));
    if !reps.is_empty() {
        root.insert("rep".into(), Value::Array(reps));
    }
    toml::to_string(&root)
}

fn read_limited(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_STORE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    Ok((u64::try_from(bytes.len()).is_ok_and(|n| n <= MAX_STORE_BYTES)).then_some(bytes))
}

/// Renames the file to `<name>.corrupt`; `false` when that failed.
fn set_aside(path: &Path) -> bool {
    let mut name = path.as_os_str().to_owned();
    name.push(".corrupt");
    std::fs::rename(path, PathBuf::from(name)).is_ok()
}

/// Reads the store at `path`: a missing file gives the defaults; a file that
/// was read and is too large, is not UTF-8 or is not TOML is renamed to
/// `<name>.corrupt` (defaults, `unreadable`); a file that could not be read
/// at all stays where it is and the store is `read_only` (D2225); a file of
/// a newer version is read for the known fields and never written.
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
pub fn save_file(path: &Path, store: &OpeningStore) -> Result<(), FileError> {
    let fail = |e: &std::io::Error| FileError::from_io(e, Some(path.to_path_buf()));
    let text = render_store(store).map_err(|error| {
        tracing::error!(%error, "could not render the opening trainer store");
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

    fn sample() -> OpeningStore {
        let mut store = OpeningStore::default();
        store.config.side = TrainSide::Both;
        store.config.source = RepSourceKind::File;
        store.config.file = Some(PathBuf::from("rep.bin"));
        store.config.max_depth = 24;
        store.config.strict = false;
        store.config.no_repeat = true;
        store.use_rep("a.si4|w");
        *store.stat_mut("a.si4|w", u64::MAX).expect("stat") = PosStat {
            seen: 5,
            good: 3,
            dubious: 1,
            out: 2,
            box_no: 4,
            due: 20_000,
        };
        store.stat_mut("a.si4|w", 7).expect("stat").seen = 1;
        store.use_rep("b.bin|b");
        store.stat_mut("b.bin|b", 9).expect("stat").out = 4;
        store
    }

    #[test]
    fn the_store_round_trips_including_hashes_above_i64_max() {
        let store = sample();
        let text = render_store(&store).expect("render");
        assert_eq!(parse_store(&text), Some(store));
    }

    #[test]
    fn a_wrong_value_only_resets_its_own_field() {
        let text = "version = 1\n[config]\nside = \"purple\"\nmax_depth = 200\nstrict = false\n";
        let store = parse_store(text).expect("toml");
        assert_eq!(store.config.side, TrainSide::White);
        assert_eq!(store.config.max_depth, super::super::config::DEPTH_RANGE.1);
        assert!(!store.config.strict);
        // A number that does not even fit in a byte goes back to the default.
        let text = "[config]\nmax_depth = 300\n";
        let store = parse_store(text).expect("toml");
        assert_eq!(store.config.max_depth, super::super::config::DEFAULT_DEPTH);
    }

    #[test]
    fn malformed_rows_are_skipped_and_the_box_is_clamped() {
        let text = "[[rep]]\nkey = \"k\"\npos = [[1, 2, 3, 4, 5, 99, 6], [\"x\"], [], [2]]\n";
        let store = parse_store(text).expect("toml");
        let s = store.stat("k", 1).expect("row");
        assert_eq!((s.seen, s.box_no, s.due), (2, TOP_BOX, 6));
        assert_eq!(store.recorded("k"), 2, "only the numeric rows are kept");
    }

    #[test]
    fn the_least_recently_used_repertoire_is_dropped_past_the_limit() {
        let mut store = OpeningStore::default();
        for i in 0..=MAX_REPS {
            store.use_rep(&format!("rep{i}"));
            store.stat_mut(&format!("rep{i}"), 1).expect("stat").seen = 1;
        }
        assert!(store.stat("rep0", 1).is_none());
        assert!(store.stat(&format!("rep{MAX_REPS}"), 1).is_some());
    }

    #[test]
    fn a_full_repertoire_refuses_new_positions_but_updates_old_ones() {
        let mut store = OpeningStore::default();
        store.use_rep("k");
        for hash in 0..MAX_POSITIONS_PER_REP as u64 {
            store.stat_mut("k", hash).expect("room");
        }
        assert!(store.stat_mut("k", u64::MAX).is_none());
        assert!(store.stat_mut("k", 5).is_some());
    }

    #[test]
    fn reset_forgets_only_that_repertoire() {
        let mut store = sample();
        store.reset("a.si4|w");
        assert!(store.stat("a.si4|w", 7).is_none());
        assert!(store.stat("b.bin|b", 9).is_some());
    }

    #[test]
    fn merging_keeps_what_is_in_memory() {
        let mut memory = OpeningStore::default();
        memory.use_rep("k");
        memory.stat_mut("k", 1).expect("stat").seen = 9;
        let mut loaded = OpeningStore::default();
        loaded.use_rep("k");
        loaded.stat_mut("k", 1).expect("stat").seen = 1;
        loaded.stat_mut("k", 2).expect("stat").seen = 2;
        memory.merge_loaded(loaded, true);
        assert_eq!(memory.stat("k", 1).map(|s| s.seen), Some(9));
        assert_eq!(memory.stat("k", 2).map(|s| s.seen), Some(2));
    }

    #[test]
    fn the_largest_file_that_can_be_written_fits_in_what_is_read() {
        let worst = PosStat {
            seen: u32::MAX,
            good: u32::MAX,
            dubious: u32::MAX,
            out: u32::MAX,
            box_no: TOP_BOX,
            due: u32::MAX,
        };
        let mut store = OpeningStore::default();
        store.use_rep("k");
        for i in 0..100u64 {
            *store.stat_mut("k", u64::MAX - i).expect("room") = worst;
        }
        let text = render_store(&store).expect("render");
        let per_position = text.len() / 100 + 1;
        let widest = per_position * MAX_POSITIONS_PER_REP * MAX_REPS;
        assert!(
            (widest as u64) < MAX_STORE_BYTES,
            "{widest} bytes must fit in {MAX_STORE_BYTES}"
        );
    }

    #[test]
    fn files_are_loaded_saved_and_protected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(STORE_FILE);
        assert_eq!(load_file(&path), LoadedStore::default());
        let store = sample();
        save_file(&path, &store).expect("save");
        let loaded = load_file(&path);
        assert_eq!(loaded.store, store);
        assert!(!loaded.read_only && !loaded.newer);
        // A newer version is read and protected.
        std::fs::write(&path, "version = 99\n[config]\nside = \"black\"\n").expect("write");
        let newer = load_file(&path);
        assert!(newer.newer && newer.read_only);
        assert_eq!(newer.store.config.side, TrainSide::Black);
        // Garbage is set aside.
        std::fs::write(&path, "not [ toml").expect("write");
        let bad = load_file(&path);
        assert!(bad.unreadable && !bad.read_only);
        assert!(!path.exists());
        assert!(dir.path().join(format!("{STORE_FILE}.corrupt")).exists());
    }
}
