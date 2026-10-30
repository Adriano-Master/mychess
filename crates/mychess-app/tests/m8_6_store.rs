//! M8-6a (spec D2317, D2318): `calvar.toml`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

use std::fmt::Write;
use std::path::Path;

use mychess_app::calvar::{
    CalvarConfig, CalvarStore, HistoryEntry, MAX_HISTORY, PositionSource, STORE_FILE,
    STORE_VERSION, load_file, parse_store, render_store, save_file, store_path,
};

fn entry(n: u64, score: u8) -> HistoryEntry {
    HistoryEntry {
        when: n,
        score,
        found: 2,
        good: 3,
        lines: 3,
        crucial: n.is_multiple_of(2),
        white_to_move: !n.is_multiple_of(3),
        fen: "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".to_owned(),
        detail: None,
    }
}

#[test]
fn the_file_sits_beside_the_preferences() {
    let path = store_path(Path::new("/a/b/preferences.toml"));
    assert_eq!(path.file_name().unwrap(), STORE_FILE);
    assert_eq!(path.parent(), Path::new("/a/b").into());
}

#[test]
fn everything_survives_a_save_and_a_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join(STORE_FILE);
    let mut store = CalvarStore {
        config: CalvarConfig {
            source: PositionSource::Fen,
            fen: "8/8/8/8/8/8/8/K6k w - - 0 1".to_owned(),
            engine: Some(7),
            lines_required: 4,
            multipv: 8,
            root_ms: 60_000,
            line_ms: 20_000,
            time_limit_s: 600,
            follow_board: true,
        },
        ..CalvarStore::default()
    };
    store.record(entry(1_700_000_000, 80));
    store.record(entry(1_700_000_100, 40));
    save_file(&path, &store).unwrap();
    let loaded = load_file(&path);
    assert!(!loaded.unreadable && !loaded.newer && !loaded.read_only);
    assert_eq!(loaded.store, store);
    assert_eq!(loaded.store.stats.done, 2);
    assert_eq!(loaded.store.stats.best_score, 80);
    assert_eq!(loaded.store.stats.mean_score(), Some(60));
}

#[test]
fn a_missing_file_gives_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = load_file(&dir.path().join(STORE_FILE));
    assert_eq!(loaded.store, CalvarStore::default());
    assert!(!loaded.unreadable && !loaded.read_only);
}

#[test]
fn the_history_keeps_only_the_last_hundred_entries() {
    let mut store = CalvarStore::default();
    for n in 0..(MAX_HISTORY as u64 + 25) {
        store.record(entry(n, 50));
    }
    assert_eq!(store.history.len(), MAX_HISTORY);
    assert_eq!(store.history[0].when, 25, "the oldest went first");
    assert_eq!(
        store.stats.done as usize,
        MAX_HISTORY + 25,
        "counters keep all"
    );
    // A file with more entries than the cap is cut on reading too.
    let mut text = format!("version = {STORE_VERSION}\n");
    for n in 0..(MAX_HISTORY + 10) {
        writeln!(text, "[[history]]\nwhen = {n}\nscore = 10\nfen = \"x\"").unwrap();
    }
    assert_eq!(parse_store(&text).unwrap().history.len(), MAX_HISTORY);
}

#[test]
fn reset_clears_counters_and_history_but_not_the_configuration() {
    let mut store = CalvarStore::default();
    store.config.multipv = 9;
    store.record(entry(1, 70));
    store.reset();
    assert_eq!(store.stats.done, 0);
    assert!(store.history.is_empty());
    assert_eq!(store.config.multipv, 9);
}

#[test]
fn a_wrong_value_resets_only_that_field_and_a_bad_entry_is_skipped() {
    let text = "[config]\nmultipv = \"lots\"\nline_ms = 2500\nfollow_board = 3\n\
                [stats]\ndone = -4\nbest_score = 900\n\
                [[history]]\nscore = 10\n\
                [[history]]\nwhen = 5\nscore = 150\nfen = \"ok\"\n";
    let store = parse_store(text).unwrap();
    assert_eq!(store.config.multipv, CalvarConfig::default().multipv);
    assert_eq!(store.config.line_ms, 2500);
    assert!(!store.config.follow_board);
    assert_eq!(store.stats.done, 0);
    assert_eq!(
        store.stats.best_score, 0,
        "900 is no score: back to the default"
    );
    assert_eq!(store.history.len(), 1, "the one without `when` is skipped");
    assert_eq!(store.history[0].score, 100);
}

#[test]
fn values_out_of_range_come_back_inside_it() {
    let store = parse_store("[config]\nmultipv = 200\nroot_ms = 1\ntime_limit_s = 7\n").unwrap();
    assert_eq!(store.config.multipv, 10);
    assert_eq!(store.config.root_ms, 200);
    assert_eq!(store.config.time_limit_s, 30);
}

#[test]
fn a_file_that_is_not_toml_is_set_aside_not_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(STORE_FILE);
    std::fs::write(&path, "[[[ nonsense").unwrap();
    let loaded = load_file(&path);
    assert!(loaded.unreadable);
    assert!(!loaded.read_only);
    assert!(!path.exists());
    assert!(dir.path().join("calvar.toml.corrupt").exists());
}

#[test]
fn a_directory_in_place_of_the_file_is_left_alone_and_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(STORE_FILE);
    std::fs::create_dir(&path).unwrap();
    let loaded = load_file(&path);
    assert!(loaded.unreadable && loaded.read_only);
    assert!(path.is_dir());
}

#[test]
fn a_newer_version_is_read_but_flagged_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(STORE_FILE);
    let text = format!(
        "version = {}\n[stats]\ndone = 3\nfuture_field = 1\n[unknown]\nx = 1\n",
        STORE_VERSION + 1
    );
    std::fs::write(&path, &text).unwrap();
    let loaded = load_file(&path);
    assert!(loaded.newer && loaded.read_only && !loaded.unreadable);
    assert_eq!(loaded.store.stats.done, 3);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "untouched");
}

#[test]
fn a_utf8_bom_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(STORE_FILE);
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(b"[stats]\ndone = 2\n");
    std::fs::write(&path, bytes).unwrap();
    assert_eq!(load_file(&path).store.stats.done, 2);
}

#[test]
fn a_huge_file_is_set_aside_without_being_read_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(STORE_FILE);
    std::fs::write(&path, vec![b'#'; (8 << 20) + 10]).unwrap();
    let loaded = load_file(&path);
    assert!(loaded.unreadable);
    assert!(dir.path().join("calvar.toml.corrupt").exists());
}

#[test]
fn rendering_then_parsing_is_the_identity_for_the_defaults() {
    let store = CalvarStore::default();
    let text = render_store(&store).unwrap();
    assert_eq!(parse_store(&text), Some(store));
}
