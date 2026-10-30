//! M5-2a review regressions, part 1: the lock is taken per call (D976, D986).
//!
//! A tree search in flight must not hold the database for the whole scan:
//! the code that takes `source.read()` in `update` (list, text search, sort,
//! Clipbase) keeps working, and a cancel gets through even when somebody else
//! holds the lock.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m5_2_common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use m5_2_common::*;
use mychess_app::{CopyScope, ListColumn, PrefChange, PrefsMessage};
use tempfile::tempdir;

/// A generous bound for "returns", never for "returns fast".
const WATCHDOG: Duration = Duration::from_secs(60);

fn many_games(n: usize) -> String {
    let mut text = String::new();
    for i in 0..n {
        let moves = if i % 3 == 0 {
            "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7"
        } else {
            "1. d4 d5 2. c4 e6 3. Nc3 Nf6 4. Bg5 Be7 5. e3 O-O"
        };
        text.push_str(&game_pgn(moves, "1-0", 2400, 2300, "2001.01.01", None));
    }
    text
}

/// A Clipbase with `n` games, selected, with the tree open on it.
fn clipbase_with_tree(dir: &std::path::Path, n: usize) -> Rig {
    let mut rig = Rig::new();
    let source = rig.open_pgn(dir, "big.pgn", &many_games(n));
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::ConfirmCopy(
        false,
    ))));
    rig.send(BasesMessage::List(ListMessage::CopyGames {
        from: source,
        to: SlotId::CLIPBASE,
        scope: CopyScope::Filter,
    }));
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    rig.open_tree();
    assert_eq!(
        rig.state.bases.slot(SlotId::CLIPBASE).unwrap().total(),
        u32::try_from(n).unwrap()
    );
    rig
}

/// Runs `job` on its own thread and runtime. Returns the receiver of the
/// answer and a flag raised when the search ended.
fn spawn_job(
    job: Box<TreeJob>,
    barrier: Arc<Barrier>,
) -> (mpsc::Receiver<Message>, Arc<AtomicBool>) {
    let (tx, rx) = mpsc::channel();
    let done = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&done);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        barrier.wait();
        let message = rt.block_on(perform(Effect::TreeSearch { job }));
        flag.store(true, Ordering::SeqCst);
        drop(tx.send(message));
    });
    (rx, done)
}

#[test]
fn list_operations_are_served_while_a_search_scans_a_big_clipbase() {
    let dir = tempdir().unwrap();
    let mut rig = clipbase_with_tree(dir.path(), 20_000);
    let slot = SlotId::CLIPBASE;
    // A new position: the search of it is in flight (the host owns it).
    let mut flight = jobs(rig.raw(GameMessage::PlayMove {
        mv: rig.state.session.position().parse_uci("e2e4").unwrap(),
        options: PlayOptions::default(),
    }));
    assert_eq!(flight.len(), 1);
    let barrier = Arc::new(Barrier::new(2));
    let (rx, done) = spawn_job(flight.remove(0), Arc::clone(&barrier));
    barrier.wait();

    // Cheap operations of `update`, repeated while the scan runs. The
    // longest wait of one of them must be a small fraction of the scan.
    let started = Instant::now();
    let mut worst = Duration::ZERO;
    let mut served_while_running = 0_u32;
    let mut round = 0_u32;
    while !done.load(Ordering::SeqCst) && started.elapsed() < WATCHDOG {
        round += 1;
        let before = Instant::now();
        rig.raw(BasesMessage::SetListFirst {
            slot,
            first: 1 + round % 50,
        });
        rig.raw(BasesMessage::SelectRow {
            slot,
            number: Some(1 + round % 50),
        });
        rig.raw(BasesMessage::SetSearchText {
            slot,
            text: format!("W{round}"),
        });
        let waited = before.elapsed();
        worst = worst.max(waited);
        if !done.load(Ordering::SeqCst) {
            served_while_running += 1;
        }
    }
    let message = rx.recv_timeout(WATCHDOG).expect("the search ends");
    let scan = started.elapsed();
    assert!(!was_cancelled(&message));
    assert!(round >= 1);
    assert!(
        served_while_running >= 1,
        "no operation of the list was served before the scan ended"
    );
    // A whole-scan lock would make the first round wait for the scan.
    assert!(
        worst < scan / 2 || scan < Duration::from_millis(50),
        "an operation waited {worst:?} of a {scan:?} scan"
    );
}

/// Aborts the process if `work` does not finish: a lock that is never freed
/// would otherwise hang the suite instead of failing.
fn with_deadlock_guard<T>(what: &'static str, work: impl FnOnce() -> T) -> T {
    let (tx, rx) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        if rx.recv_timeout(WATCHDOG) == Err(mpsc::RecvTimeoutError::Timeout) {
            #[allow(clippy::print_stderr)]
            {
                eprintln!("DEADLOCK: {what} did not return in {WATCHDOG:?}");
            }
            std::process::abort();
        }
    });
    let out = work();
    drop(tx);
    out
}

/// Runs the effects (and the ones their answers produce) to the end.
fn settle(rig: &mut Rig, effects: Vec<Effect>) {
    let mut queue: std::collections::VecDeque<Effect> = effects.into();
    while let Some(effect) = queue.pop_front() {
        if effect.runs_on_host() {
            continue;
        }
        let answer = rig.perform(effect);
        queue.extend(rig.raw(answer));
    }
}

#[test]
fn clipbase_paste_clear_sort_and_text_search_do_not_wait_for_the_scan() {
    let dir = tempdir().unwrap();
    let mut rig = clipbase_with_tree(dir.path(), 20_000);
    let slot = SlotId::CLIPBASE;
    let mut flight = jobs(rig.raw(GameMessage::PlayMove {
        mv: rig.state.session.position().parse_uci("d2d4").unwrap(),
        options: PlayOptions::default(),
    }));
    assert_eq!(flight.len(), 1);
    let barrier = Arc::new(Barrier::new(2));
    let (rx, done) = spawn_job(flight.remove(0), Arc::clone(&barrier));
    barrier.wait();

    with_deadlock_guard("operations of the list during a scan", || {
        // Sort and text search are jobs of the list: they take `read()`.
        let sort = rig.raw(BasesMessage::List(ListMessage::SortBy {
            slot,
            column: ListColumn::White,
        }));
        settle(&mut rig, sort);
        rig.raw(BasesMessage::SetSearchText {
            slot,
            text: "W".to_owned(),
        });
        let find = rig.raw(BasesMessage::List(ListMessage::FilterByText { slot }));
        settle(&mut rig, find);
        // Paste and clear of the Clipbase.
        let paste = rig.raw(BasesMessage::ClipbasePaste);
        settle(&mut rig, paste);
        let clear = rig.raw(BasesMessage::ClipbaseClear);
        settle(&mut rig, clear);
    });
    assert!(rig.state.list_job().is_none(), "the list jobs ended");
    assert_eq!(rig.state.bases.slot(slot).unwrap().total(), 0, "cleared");

    // The search ends; its answer (cancelled or stale) must leave a
    // consistent tree: nothing searching for ever.
    let answer = rx.recv_timeout(WATCHDOG).expect("the search ends");
    let more = rig.raw(answer);
    settle(&mut rig, more);
    assert!(done.load(Ordering::SeqCst));
    assert!(!rig.state.tree().searching());
    assert_eq!(rig.view().total.map_or(0, |t| t.games), 0);
}

#[test]
fn a_search_blocked_on_a_contended_lock_can_still_be_cancelled() {
    let dir = tempdir().unwrap();
    let mut rig = clipbase_with_tree(dir.path(), 2_000);
    let slot = SlotId::CLIPBASE;
    let source = rig.state.bases.slot(slot).unwrap().source.clone();
    let mut flight = jobs(rig.raw(GameMessage::PlayMove {
        mv: rig.state.session.position().parse_uci("e2e4").unwrap(),
        options: PlayOptions::default(),
    }));
    assert_eq!(flight.len(), 1);
    let job = flight.remove(0);
    let handle = (*job).clone();

    // Somebody else (a long import, say) owns the database.
    let guard = source.lock();
    let barrier = Arc::new(Barrier::new(2));
    let (rx, done) = spawn_job(job, Arc::clone(&barrier));
    barrier.wait();
    assert!(
        rx.recv_timeout(Duration::from_millis(100)).is_err(),
        "the search waits for the lock"
    );
    handle.cancel();
    assert!(!done.load(Ordering::SeqCst));
    drop(guard);

    let answer = rx
        .recv_timeout(WATCHDOG)
        .expect("the cancelled search ends");
    assert!(was_cancelled(&answer), "{answer:?}");
    // The answer of a cancelled search is harmless.
    let more = rig.raw(answer);
    settle(&mut rig, more);
    assert!(!rig.state.tree().searching());
}

#[test]
fn closing_the_panel_cancels_a_search_that_waits_for_the_lock() {
    let dir = tempdir().unwrap();
    let mut rig = clipbase_with_tree(dir.path(), 2_000);
    let slot = SlotId::CLIPBASE;
    let source = rig.state.bases.slot(slot).unwrap().source.clone();
    let mut flight = jobs(rig.raw(GameMessage::PlayMove {
        mv: rig.state.session.position().parse_uci("e2e4").unwrap(),
        options: PlayOptions::default(),
    }));
    let job = flight.remove(0);
    let guard = source.lock();
    let barrier = Arc::new(Barrier::new(2));
    let (rx, _done) = spawn_job(job, Arc::clone(&barrier));
    barrier.wait();
    // `update` must return although the database is taken by another party:
    // it never waits for the search thread.
    let effects = with_deadlock_guard("TreeMessage::Close", || {
        // (`Close` does not read the source of other parties.)
        rig.raw(TreeMessage::Close)
    });
    assert_eq!(jobs(effects).len(), 0);
    drop(guard);
    let answer = rx.recv_timeout(WATCHDOG).expect("the search ends");
    let more = rig.raw(answer);
    settle(&mut rig, more);
    assert!(!rig.state.tree().searching());
    assert_eq!(rig.view().body, TreeBody::Closed);
}
