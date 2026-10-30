//! Tests of the block cut by bytes of the scan (D3205): the limits are
//! parameters of `scan_games_with_limits`, so a tiny byte limit forces it.

use std::sync::Mutex;

use mychess_core::Game;

use super::{GameMatcher, Shortcut, scan_games, scan_games_with_limits};
use crate::filter::{Filter, FilterOp};
use crate::progress::{CancelToken, NoProgress};
use crate::si4::{GameFlags, IndexEntry, RecordError, Si4Database, Si4Writer};

const GAMES: u32 = 6;

fn game_of(plies: usize) -> Game {
    const CYCLE: [&str; 4] = ["g1f3", "g8f6", "f3g1", "f6g8"];
    let mut g = Game::new();
    let mut at = g.root();
    for uci in CYCLE.iter().cycle().take(plies) {
        let mv = g.position_at(at).map(|p| p.parse_uci(uci));
        let Ok(Ok(mv)) = mv else { panic!("bad move") };
        at = g.add_move(at, mv).expect("legal move");
    }
    g
}

/// Six games of growing length, so the records differ in size.
fn build() -> (tempfile::TempDir, Si4Database) {
    let dir = tempfile::tempdir().expect("tempdir");
    let base = dir.path().join("db");
    let mut w = Si4Writer::create(&base, 1).expect("create");
    for i in 0..GAMES {
        w.add_game(&game_of(2 + 6 * i as usize), GameFlags::empty())
            .expect("add");
    }
    drop(w);
    let db = Si4Database::open(&base).expect("open");
    (dir, db)
}

/// Logs the order of prefilter (block assembly) and evaluate (block
/// evaluation) calls; matches the games with a record of at least 12 bytes
/// and a value that depends on the record size.
struct Logger {
    log: Mutex<Vec<(char, u32)>>,
}

impl GameMatcher for Logger {
    fn prefilter(&self, entry: &IndexEntry) -> Shortcut {
        if let Ok(mut log) = self.log.lock() {
            log.push(('P', entry.length));
        }
        Shortcut::Examine
    }

    fn evaluate(&self, entry: &IndexEntry, record: &[u8]) -> Result<Option<u8>, RecordError> {
        if let Ok(mut log) = self.log.lock() {
            log.push(('E', entry.length));
        }
        let n = u8::try_from(record.len() % 200).unwrap_or(1);
        Ok((record.len() >= 12).then_some(n.max(1)))
    }
}

fn logger() -> Logger {
    Logger {
        log: Mutex::new(Vec::new()),
    }
}

fn pool<R: Send>(threads: usize, f: impl FnOnce() -> R + Send) -> R {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("pool")
        .install(f)
}

fn run(
    db: &mut Si4Database,
    matcher: &Logger,
    max_games: usize,
    max_bytes: usize,
) -> (Vec<u8>, Vec<(char, u32)>) {
    let mut filter = Filter::new(GAMES);
    filter.fill(0);
    scan_games_with_limits(
        db,
        matcher,
        FilterOp::Reset,
        &mut filter,
        &mut NoProgress,
        &CancelToken::new(),
        max_games,
        max_bytes,
    )
    .expect("scan");
    let values = (1..=GAMES).map(|n| filter.get(n)).collect();
    let log = matcher.log.lock().expect("log").clone();
    (values, log)
}

fn default_values(db: &mut Si4Database) -> Vec<u8> {
    let mut filter = Filter::new(GAMES);
    filter.fill(0);
    scan_games(
        db,
        &logger(),
        FilterOp::Reset,
        &mut filter,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .expect("scan");
    (1..=GAMES).map(|n| filter.get(n)).collect()
}

#[test]
fn byte_limit_smaller_than_any_record_makes_one_game_blocks() {
    let (_dir, mut db) = build();
    let expected = default_values(&mut db);
    let m = logger();
    // One thread: the log shows the order block by block.
    let (values, log) = pool(1, || run(&mut db, &m, 2048, 1));
    assert_eq!(values, expected);
    // Every record is bigger than the limit and is still examined, one per
    // block: prefilter then evaluate for each game.
    assert_eq!(log.len(), 2 * GAMES as usize);
    for pair in log.chunks(2) {
        assert_eq!(pair[0].0, 'P');
        assert_eq!(pair[1].0, 'E');
        assert_eq!(pair[0].1, pair[1].1);
    }
}

#[test]
fn byte_limit_cuts_the_block_after_the_record_that_crosses_it() {
    let (_dir, mut db) = build();
    let len1 = db.entry(1).expect("entry").length as usize;
    let len2 = db.entry(2).expect("entry").length as usize;
    // Games 1 and 2 together cross the limit; game 1 alone does not.
    let limit = len1 + len2 / 2 + 1;
    let m = logger();
    let (values, log) = pool(1, || run(&mut db, &m, 2048, limit));
    assert_eq!(values, default_values(&mut db));
    let kinds: String = log.iter().map(|(k, _)| *k).take(4).collect();
    assert_eq!(kinds, "PPEE", "the first block holds games 1 and 2");
}

#[test]
fn game_limit_also_cuts_the_block() {
    let (_dir, mut db) = build();
    let m = logger();
    let (values, log) = pool(1, || run(&mut db, &m, 3, usize::MAX));
    assert_eq!(values, default_values(&mut db));
    let kinds: String = log.iter().map(|(k, _)| *k).collect();
    assert_eq!(kinds, "PPPEEEPPPEEE");
}

#[test]
fn result_does_not_depend_on_threads_with_tiny_blocks() {
    let (_dir, mut db) = build();
    let expected = default_values(&mut db);
    assert!(expected.iter().any(|&v| v != 0), "some game must match");
    for threads in [1, 2, 4] {
        for bytes in [1, 40, 1 << 20] {
            let m = logger();
            let (values, _) = pool(threads, || run(&mut db, &m, 2048, bytes));
            assert_eq!(values, expected, "{threads} threads, {bytes} bytes");
        }
    }
}
