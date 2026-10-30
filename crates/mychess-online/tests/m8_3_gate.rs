//! Contract tests of the `RequestGate` (spec m8-3, D1308, D1341). The gate has
//! no socket, so every test runs on a paused Tokio clock.

#![allow(clippy::assert_is_empty, clippy::type_complexity)]

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use mychess_online::{Error, GateConfig, Priority, RequestGate};

fn gate() -> RequestGate {
    RequestGate::new(GateConfig::default())
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[test]
fn default_tuning_is_sixty_seconds_up_to_five_minutes() {
    let cfg = GateConfig::default();
    assert_eq!(cfg.base_block, secs(60));
    assert_eq!(cfg.max_block, secs(300));
}

#[tokio::test(start_paused = true)]
async fn consecutive_429s_double_the_block_up_to_the_cap() {
    let g = gate();
    let got: Vec<_> = (0..6).map(|_| g.record_status(429, None)).collect();
    assert_eq!(
        got,
        [60, 120, 240, 300, 300, 300].map(|s| Some(secs(s))),
        "60 s, doubling, capped at 5 min"
    );
}

#[tokio::test(start_paused = true)]
async fn custom_tuning_is_honored() {
    let g = RequestGate::new(GateConfig {
        base_block: Duration::from_millis(10),
        max_block: Duration::from_millis(40),
    });
    let got: Vec<_> = (0..4).map(|_| g.record_status(429, None)).collect();
    assert_eq!(
        got,
        [10, 20, 40, 40].map(|ms| Some(Duration::from_millis(ms)))
    );
}

#[tokio::test(start_paused = true)]
async fn a_429_refuses_normal_requests_without_waiting_until_the_block_ends() {
    let g = gate();
    assert_eq!(g.blocked_for(), None);
    g.record_status(429, None);
    assert_eq!(g.blocked_for(), Some(secs(60)));

    tokio::time::advance(secs(25)).await;
    let before = tokio::time::Instant::now();
    let Err(Error::RateLimited { retry_after }) = g.acquire(Priority::Normal).await else {
        panic!("expected RateLimited");
    };
    assert_eq!(
        tokio::time::Instant::now(),
        before,
        "refusal must not sleep (no network, no waiting)"
    );
    assert_eq!(retry_after, secs(35));
    assert_eq!(
        g.check_not_blocked(),
        Err(Error::RateLimited {
            retry_after: secs(35)
        })
    );

    tokio::time::advance(secs(34)).await;
    assert!(g.acquire(Priority::Normal).await.is_err(), "1 s left");
    tokio::time::advance(secs(1)).await;
    assert_eq!(
        g.blocked_for(),
        None,
        "exactly 60 s later the block is over"
    );
    assert!(g.acquire(Priority::Normal).await.is_ok());
    assert_eq!(g.check_not_blocked(), Ok(()));
}

#[tokio::test(start_paused = true)]
async fn a_second_429_during_the_block_extends_it() {
    let g = gate();
    g.record_status(429, None);
    tokio::time::advance(secs(30)).await;
    assert_eq!(g.record_status(429, None), Some(secs(120)));
    assert_eq!(g.blocked_for(), Some(secs(120)));
}

#[tokio::test(start_paused = true)]
async fn the_block_never_shrinks() {
    let g = gate();
    g.record_status(429, Some(secs(300)));
    // A later 429 with a small value must not shorten the longer block.
    g.record_status(429, Some(secs(1)));
    assert!(g.blocked_for().expect("blocked") > secs(250));
}

#[tokio::test(start_paused = true)]
async fn retry_after_wins_only_when_larger_than_the_default_block() {
    let g = gate();
    assert_eq!(g.record_status(429, Some(secs(90))), Some(secs(90)));

    let g = gate();
    assert_eq!(g.record_status(429, Some(secs(10))), Some(secs(60)));

    let g = gate();
    assert_eq!(
        g.record_status(429, Some(secs(86_400))),
        Some(secs(3600)),
        "capped at one hour"
    );
    assert_eq!(g.blocked_for(), Some(secs(3600)));
}

#[tokio::test(start_paused = true)]
async fn doubling_resets_only_after_the_block_is_over_and_a_success_arrives() {
    let g = gate();
    g.record_status(429, None); // 60, next 120
    // A success while still blocked must not reset the doubling.
    g.record_status(200, None);
    tokio::time::advance(secs(10)).await;
    g.record_status(200, None);
    assert_eq!(g.record_status(429, None), Some(secs(120)));

    // Block over, then a success: back to 60.
    tokio::time::advance(secs(121)).await;
    assert_eq!(g.blocked_for(), None);
    g.record_status(200, None);
    assert_eq!(g.record_status(429, None), Some(secs(60)));
}

#[tokio::test(start_paused = true)]
async fn non_429_statuses_never_start_a_block() {
    let g = gate();
    for status in [200, 400, 401, 403, 404, 500, 502, 503] {
        assert_eq!(g.record_status(status, None), None, "{status}");
    }
    assert_eq!(g.blocked_for(), None);
}

#[tokio::test(start_paused = true)]
async fn urgent_requests_ignore_the_block_and_the_queue_but_extend_it() {
    let g = gate();
    let _held = g.acquire(Priority::Normal).await.expect("normal");
    // Neither the occupied slot nor ...
    assert!(g.acquire(Priority::Urgent).await.is_ok());
    // ... the block stops them.
    g.record_status(429, None);
    assert!(g.acquire(Priority::Urgent).await.is_ok());
    // A 429 seen by an urgent request blocks the normal ones.
    let g2 = gate();
    let _p = g2.acquire(Priority::Urgent).await.expect("urgent");
    g2.record_status(429, None);
    assert!(matches!(
        g2.acquire(Priority::Normal).await,
        Err(Error::RateLimited { .. })
    ));
}

#[tokio::test(start_paused = true)]
async fn urgent_permits_do_not_occupy_the_normal_slot() {
    let g = gate();
    let _u1 = g.acquire(Priority::Urgent).await.expect("u1");
    let _u2 = g.acquire(Priority::Urgent).await.expect("u2");
    assert!(g.acquire(Priority::Normal).await.is_ok());
}

#[tokio::test(start_paused = true)]
async fn only_one_normal_request_is_in_flight_at_a_time() {
    let g = gate();
    let current = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let (g, current, peak) = (g.clone(), current.clone(), peak.clone());
        tasks.push(tokio::spawn(async move {
            let permit = g.acquire(Priority::Normal).await.expect("permit");
            let now = current.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(50)).await;
            current.fetch_sub(1, Ordering::SeqCst);
            drop(permit);
        }));
    }
    for t in tasks {
        t.await.expect("join");
    }
    assert_eq!(peak.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn dropping_a_permit_frees_the_slot_even_if_the_holder_failed() {
    let g = gate();
    {
        let _p = g.acquire(Priority::Normal).await.expect("permit");
    }
    let second = tokio::time::timeout(secs(1), g.acquire(Priority::Normal)).await;
    assert!(matches!(second, Ok(Ok(_))));
}

#[tokio::test(start_paused = true)]
async fn a_429_that_arrives_while_queued_fails_the_waiting_request() {
    let g = gate();
    let held = g.acquire(Priority::Normal).await.expect("held");
    let g2 = g.clone();
    let waiter = tokio::spawn(async move { g2.acquire(Priority::Normal).await.map(|_| ()) });
    tokio::time::sleep(secs(1)).await;
    assert!(!waiter.is_finished());
    g.record_status(429, None);
    drop(held);
    let res = waiter.await.expect("join");
    assert!(matches!(res, Err(Error::RateLimited { .. })), "{res:?}");
}

#[tokio::test(start_paused = true)]
async fn clones_share_the_block_and_the_slot() {
    let g = gate();
    let c = g.clone();
    let _p = g.acquire(Priority::Normal).await.expect("permit");
    let waiter = tokio::spawn(async move { c.acquire(Priority::Normal).await.map(|_| ()) });
    tokio::time::sleep(secs(1)).await;
    assert!(
        !waiter.is_finished(),
        "the clone must wait for the same slot"
    );
    waiter.abort();

    let a = gate();
    let b = a.clone();
    a.record_status(429, None);
    assert!(b.blocked_for().is_some());
}
