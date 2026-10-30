//! M4-1 contract tests: the protocol log (spec "Comportamento observável" 9,
//! D614): order, bounded ring, truncation, incremental snapshots, `save_to`.

#[path = "m4_1_common/helpers.rs"]
mod common;

use std::time::Duration;

use common::*;
use mychess_engine::{
    DEFAULT_LOG_LINES, EngineCommand, LogEntry, LogHandle, LogKind, MAX_LOG_ENTRY_BYTES,
};

fn texts(entries: &[LogEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|e| format!("{}{}", e.kind.prefix(), e.text))
        .collect()
}

#[test]
fn the_defaults_are_the_ones_of_the_spec() {
    assert_eq!(DEFAULT_LOG_LINES, 5000);
    assert_eq!(MAX_LOG_ENTRY_BYTES, 4096);
    assert_eq!(LogKind::Sent.prefix(), "> ");
    assert_eq!(LogKind::Received.prefix(), "< ");
    assert_eq!(LogKind::Stderr.prefix(), "! ");
    assert_eq!(LogKind::Note.prefix(), "# ");
}

#[test]
fn a_new_log_is_empty_and_snapshot_of_nothing_is_empty() {
    let log = LogHandle::new(10);
    assert_eq!(log.len(), 0);
    assert_eq!(log.len(), 0);
    assert_eq!(log.snapshot_since(0), (Vec::new(), 0));
    assert_eq!(log.snapshot_since(99), (Vec::new(), 0));
}

#[test]
fn entries_keep_arrival_order_kind_and_dense_sequence_numbers() {
    let log = LogHandle::new(10);
    log.push(LogKind::Sent, "uci");
    log.push(LogKind::Received, "uciok");
    log.push(LogKind::Stderr, "warning: hash small");
    log.push(LogKind::Note, "session ended");
    let (entries, next) = log.snapshot_since(0);
    assert_eq!(next, 4);
    assert_eq!(
        texts(&entries),
        [
            "> uci",
            "< uciok",
            "! warning: hash small",
            "# session ended"
        ]
    );
    assert_eq!(
        entries.iter().map(|e| e.seq).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
}

#[test]
fn the_ring_drops_the_oldest_entries_and_never_reuses_a_sequence_number() {
    let log = LogHandle::new(5000);
    for i in 0..5003 {
        log.push(LogKind::Received, &format!("line {i}"));
    }
    assert_eq!(log.len(), 5000);
    let (entries, next) = log.snapshot_since(0);
    assert_eq!(next, 5003);
    assert_eq!(entries.first().unwrap().seq, 3);
    assert_eq!(entries.first().unwrap().text, "line 3");
    assert_eq!(entries.last().unwrap().seq, 5002);
}

#[test]
fn zero_lines_turns_the_log_off() {
    let log = LogHandle::new(0);
    for _ in 0..10 {
        log.push(LogKind::Sent, "uci");
    }
    assert_eq!(log.len(), 0);
    assert_eq!(log.snapshot_since(0), (Vec::new(), 0));
}

#[test]
fn a_ring_of_one_keeps_only_the_latest() {
    let log = LogHandle::new(1);
    log.push(LogKind::Sent, "a");
    log.push(LogKind::Sent, "b");
    let (entries, next) = log.snapshot_since(0);
    assert_eq!(texts(&entries), ["> b"]);
    assert_eq!(next, 2);
}

#[test]
fn entries_at_the_limit_are_kept_whole_and_longer_ones_are_cut() {
    let log = LogHandle::new(10);
    log.push(LogKind::Received, &"a".repeat(MAX_LOG_ENTRY_BYTES));
    log.push(LogKind::Received, &"b".repeat(MAX_LOG_ENTRY_BYTES + 1));
    log.push(LogKind::Received, &"c".repeat(1 << 20));
    let (entries, _) = log.snapshot_since(0);
    assert_eq!(entries[0].text, "a".repeat(MAX_LOG_ENTRY_BYTES));
    assert!(
        entries[1]
            .text
            .starts_with(&"b".repeat(MAX_LOG_ENTRY_BYTES))
    );
    assert!(
        entries[1].text.contains("1 more bytes"),
        "{}",
        entries[1].text
    );
    assert!(
        entries[2]
            .text
            .starts_with(&"c".repeat(MAX_LOG_ENTRY_BYTES))
    );
    assert!(
        entries[2]
            .text
            .contains(&format!("{} more bytes", (1 << 20) - MAX_LOG_ENTRY_BYTES))
    );
    for e in &entries {
        assert!(e.text.len() < MAX_LOG_ENTRY_BYTES + 64, "{}", e.text.len());
    }
}

#[test]
fn cutting_never_splits_a_multibyte_character() {
    for (unit, size) in [("\u{e9}", 2), ("\u{4e2d}", 3), ("\u{1f600}", 4)] {
        // Every byte offset around the limit.
        for pad in 0..size {
            let text = format!("{}{}", "x".repeat(pad), unit.repeat(MAX_LOG_ENTRY_BYTES));
            let log = LogHandle::new(2);
            log.push(LogKind::Received, &text);
            let (entries, _) = log.snapshot_since(0);
            assert!(entries[0].text.contains("more bytes"));
            assert!(entries[0].text.len() <= MAX_LOG_ENTRY_BYTES + 64);
        }
    }
}

#[test]
fn a_million_lines_use_bounded_memory() {
    let log = LogHandle::new(DEFAULT_LOG_LINES);
    for i in 0..1_000_000_u32 {
        log.push(LogKind::Received, "info depth 12 score cp 20 pv e2e4 e7e5");
        if i % 250_000 == 0 {
            assert!(log.len() <= DEFAULT_LOG_LINES);
        }
    }
    assert_eq!(log.len(), DEFAULT_LOG_LINES);
    let (entries, next) = log.snapshot_since(0);
    assert_eq!(next, 1_000_000);
    assert_eq!(entries.len(), DEFAULT_LOG_LINES);
    // Megabyte lines: the ring holds at most 5000 x ~4 KiB.
    let big = LogHandle::new(DEFAULT_LOG_LINES);
    let line = "z".repeat(1 << 20);
    for _ in 0..6000 {
        big.push(LogKind::Received, &line);
    }
    let bytes: usize = big.snapshot_since(0).0.iter().map(|e| e.text.len()).sum();
    assert!(bytes < 32 * 1024 * 1024, "{bytes} bytes retained");
}

#[test]
fn incremental_snapshots_neither_lose_nor_repeat_entries() {
    let log = LogHandle::new(100);
    let mut cursor = 0;
    let mut seen = Vec::new();
    for round in 0..50 {
        for i in 0..=(round % 7) {
            log.push(LogKind::Received, &format!("r{round}-{i}"));
        }
        let (entries, next) = log.snapshot_since(cursor);
        seen.extend(entries.into_iter().map(|e| e.seq));
        assert!(next >= cursor);
        cursor = next;
    }
    let unique: std::collections::BTreeSet<_> = seen.iter().copied().collect();
    assert_eq!(seen.len(), unique.len(), "no duplicates");
    assert_eq!(seen, (0..cursor).collect::<Vec<_>>(), "no gaps");
    // Asking from the end gives nothing; from far beyond too.
    assert_eq!(log.snapshot_since(cursor).0.len(), 0);
    assert_eq!(log.snapshot_since(cursor + 1000).0.len(), 0);
}

#[test]
fn a_cursor_older_than_the_ring_yields_what_is_left() {
    let log = LogHandle::new(3);
    for i in 0..10 {
        log.push(LogKind::Sent, &format!("{i}"));
    }
    let (entries, next) = log.snapshot_since(2);
    assert_eq!(next, 10);
    assert_eq!(texts(&entries), ["> 7", "> 8", "> 9"]);
}

#[test]
fn concurrent_writers_and_a_reader_see_a_consistent_log() {
    let log = LogHandle::new(100_000);
    let writers: Vec<_> = (0..4)
        .map(|w| {
            let log = log.clone();
            std::thread::spawn(move || {
                for i in 0..2000 {
                    log.push(LogKind::Received, &format!("w{w}-{i}"));
                }
            })
        })
        .collect();
    let mut cursor = 0;
    let mut got = 0;
    while writers.iter().any(|w| !w.is_finished()) {
        let (entries, next) = log.snapshot_since(cursor);
        got += entries.len();
        cursor = next;
    }
    for w in writers {
        w.join().unwrap();
    }
    let (entries, next) = log.snapshot_since(cursor);
    got += entries.len();
    assert_eq!(got, 8000);
    assert_eq!(next, 8000);
}

#[test]
fn save_writes_prefixed_lines_with_bare_newlines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("engine.log");
    let log = LogHandle::new(10);
    log.push(LogKind::Sent, "uci");
    log.push(LogKind::Received, "id name X");
    log.push(LogKind::Stderr, "oops");
    log.push(LogKind::Note, "hi");
    log.save_to(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes, b"> uci\n< id name X\n! oops\n# hi\n");
    assert!(!bytes.contains(&b'\r'));
}

#[test]
fn saving_an_empty_log_makes_an_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.log");
    LogHandle::new(10).save_to(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"");
}

#[test]
fn saving_over_an_existing_file_replaces_it_completely() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("engine.log");
    std::fs::write(&path, "x".repeat(10_000)).unwrap();
    let log = LogHandle::new(10);
    log.push(LogKind::Sent, "uci");
    log.save_to(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"> uci\n");
}

#[test]
fn saving_into_a_missing_directory_is_a_typed_io_error_and_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("no-such-dir").join("engine.log");
    let log = LogHandle::new(10);
    log.push(LogKind::Sent, "uci");
    let err = log.save_to(&path).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    assert!(!dir.path().join("no-such-dir").exists());
}

#[test]
fn saving_onto_a_directory_fails_without_destroying_it() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("a-directory");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("keep.txt"), "keep").unwrap();
    let log = LogHandle::new(10);
    log.push(LogKind::Sent, "uci");
    assert!(log.save_to(&target).is_err());
    assert_eq!(
        std::fs::read_to_string(target.join("keep.txt")).unwrap(),
        "keep"
    );
}

#[test]
fn saving_leaves_no_temporary_file_behind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("engine.log");
    let log = LogHandle::new(10);
    log.push(LogKind::Sent, "uci");
    log.save_to(&path).unwrap();
    log.save_to(&path).unwrap();
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, [std::ffi::OsString::from("engine.log")]);
}

/// The brief and `multiplataforma.md` ("escrita segura") ask for a write that
/// replaces the destination atomically (temporary file in the same directory,
/// then `rename`): a reader that already has the old file open must keep
/// seeing the old content in full. A truncating `fs::write` fails this.
#[cfg(unix)]
#[test]
fn save_replaces_the_file_atomically_through_a_rename() {
    use std::io::Read;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("engine.log");
    std::fs::write(&path, "OLD CONTENT OLD CONTENT\n").unwrap();
    let mut old_reader = std::fs::File::open(&path).unwrap();
    let log = LogHandle::new(10);
    log.push(LogKind::Sent, "uci");
    log.save_to(&path).unwrap();
    let mut seen_by_old_reader = String::new();
    old_reader.read_to_string(&mut seen_by_old_reader).unwrap();
    assert_eq!(
        seen_by_old_reader, "OLD CONTENT OLD CONTENT\n",
        "the destination was truncated in place instead of replaced by a rename"
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"> uci\n");
}

// ---------------------------------------------------------------------------
// the session writes the log
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn the_session_logs_sent_and_received_lines_in_wire_order() {
    let (mut peer, engine, mut events) = ready_with(config(), &[HASH]).await;
    start_search(&mut peer, &engine, &mut events, depth(KPK, 2)).await;
    peer.say("info depth 1 score cp 3 pv e2e4").await;
    peer.say("bestmove e2e4").await;
    next_best(&mut events).await;
    let (entries, _) = engine.log().snapshot_since(0);
    let wire: Vec<String> = texts(&entries)
        .into_iter()
        .filter(|l| l.starts_with("> ") || l.starts_with("< "))
        .collect();
    let expected = [
        "> uci",
        "< id name Manual",
        "< id author tests",
        "< option name Hash type spin default 16 min 1 max 1024",
        "< uciok",
        "> ucinewgame",
        "> isready",
        "< readyok",
        &format!("> position fen {KPK}"),
        "> go depth 2",
        "< info depth 1 score cp 3 pv e2e4",
        "< bestmove e2e4",
    ];
    assert_eq!(wire, expected);
}

#[tokio::test(start_paused = true)]
async fn a_zero_sized_log_stays_empty_during_a_whole_session() {
    let mut cfg = config();
    cfg.log_lines = 0;
    let (mut peer, engine, mut events) = ready_with(cfg, &[]).await;
    start_search(&mut peer, &engine, &mut events, depth(KPK, 1)).await;
    peer.say("bestmove e2e4").await;
    next_best(&mut events).await;
    assert_eq!(engine.log().len(), 0);
}

#[tokio::test(start_paused = true)]
async fn the_session_log_is_bounded_by_the_configured_size() {
    let mut cfg = config();
    cfg.log_lines = 50;
    let (mut peer, engine, mut events) = ready_with(cfg, &[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    for depth in 1..=500 {
        peer.say(&format!("info depth {depth} score cp 1 pv e2e4"))
            .await;
    }
    peer.say("bestmove e2e4").await;
    next_best(&mut events).await;
    assert_eq!(engine.log().len(), 50);
    let (entries, _) = engine.log().snapshot_since(0);
    assert_eq!(entries.last().unwrap().text, "bestmove e2e4");
}

#[tokio::test(start_paused = true)]
async fn the_log_of_one_engine_can_be_read_while_it_runs() {
    let (mut peer, engine, mut events) = ready_with(config(), &[]).await;
    let (_, mid) = engine.log().snapshot_since(0);
    engine.send(EngineCommand::Search(infinite(KPK))).unwrap();
    line(&mut peer).await;
    line(&mut peer).await;
    until(&mut events, |e| {
        matches!(e, mychess_engine::EngineEvent::SearchStarted(_)).then_some(())
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1)).await;
    let (new, _) = engine.log().snapshot_since(mid);
    let wire = texts(&new);
    assert!(wire.iter().any(|l| l.starts_with("> position")), "{wire:?}");
    assert!(wire.iter().any(|l| l == "> go infinite"), "{wire:?}");
}
