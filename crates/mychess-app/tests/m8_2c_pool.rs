//! M8-2c contract tests of the pool with several lanes (spec D2505, D2506):
//! an engine whose participant was replaced or left while it was out on a
//! search is ended when it comes back, and cancelling a lane does not touch
//! the others. Time is paused.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m8_2_common;

use std::time::Duration;

use m8_2_common::*;
use mychess_app::tournament::{EnginePool, PoolCommand, PreparePlan, SearchJob, SearchOutcome};
use mychess_app::{Think, ThinkLimits};
use mychess_core::Position;

const LONG: Duration = Duration::from_secs(60);

fn slow() -> Scripted {
    Scripted {
        delay: Duration::from_millis(500),
        ..Scripted::default()
    }
}

fn job(lane: usize, participant: usize, epoch: u64) -> SearchJob {
    let start = Position::new();
    SearchJob {
        lane,
        participant,
        think: Think {
            start: start.clone(),
            moves: Vec::new(),
            side: start.turn(),
            limits: ThinkLimits::Depth(5),
        },
        deadline: LONG,
        ponder: None,
        epoch,
    }
}

fn in_lane(lane: usize, white: usize, black: usize) -> PreparePlan {
    PreparePlan {
        lane,
        ..plan((white, "A"), (black, "B"))
    }
}

async fn prepared(fx: &Fixture, plans: &[PreparePlan]) -> EnginePool {
    let pool = fx.pool();
    for (i, p) in plans.iter().enumerate() {
        pool.prepare(p, i as u64 + 1).await.expect("ready");
    }
    pool
}

#[tokio::test(start_paused = true)]
async fn an_engine_of_a_participant_that_left_is_ended_when_it_comes_back() {
    let fx = Fixture::new(vec![("B", slow())]);
    let pool = prepared(&fx, &[in_lane(0, 0, 1)]).await;
    assert_eq!(pool.alive(), 2);
    let busy = tokio::spawn({
        let pool = pool.clone();
        async move { pool.search(job(0, 1, 10)).await }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    // The participant leaves while its engine thinks (the runner cancels
    // the lane and discards the participant).
    pool.command(PoolCommand::Discard(1)).await;
    pool.command(PoolCommand::CancelLane { lane: 0, epoch: 10 })
        .await;
    assert_eq!(busy.await.unwrap(), SearchOutcome::Cancelled);
    assert_eq!(
        pool.alive(),
        1,
        "the engine of the participant that left did not go back to the pool"
    );
}

#[tokio::test(start_paused = true)]
async fn a_replaced_participant_gets_a_new_engine_and_the_old_one_never_comes_back() {
    let fx = Fixture::new(vec![("B", slow())]);
    let pool = prepared(&fx, &[in_lane(0, 0, 1)]).await;
    let busy = tokio::spawn({
        let pool = pool.clone();
        async move { pool.search(job(0, 1, 10)).await }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    pool.command(PoolCommand::Discard(1)).await;
    pool.command(PoolCommand::CancelLane { lane: 0, epoch: 10 })
        .await;
    // The new engine of participant 1 starts at once, before the old one
    // is back.
    pool.prepare(&in_lane(0, 0, 1), 11).await.expect("ready");
    assert_eq!(fx.launches("B"), 2, "a second process for the new engine");
    assert_eq!(busy.await.unwrap(), SearchOutcome::Cancelled);
    assert_eq!(pool.alive(), 2, "the new one stays, the old one is gone");
    // The new engine plays.
    assert!(matches!(
        pool.search(job(0, 1, 12)).await,
        SearchOutcome::Moved(_)
    ));
    assert_eq!(pool.alive(), 2);
}

#[tokio::test(start_paused = true)]
async fn cancelling_one_lane_leaves_the_search_of_another_alone() {
    let fx = Fixture::new(vec![("A", slow()), ("B", slow())]);
    let pool = prepared(&fx, &[in_lane(0, 0, 1), in_lane(1, 0, 1)]).await;
    assert_eq!(pool.alive(), 4, "two processes per participant");
    let first = tokio::spawn({
        let pool = pool.clone();
        async move { pool.search(job(0, 0, 20)).await }
    });
    let second = tokio::spawn({
        let pool = pool.clone();
        async move { pool.search(job(1, 0, 21)).await }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    pool.command(PoolCommand::CancelLane { lane: 0, epoch: 20 })
        .await;
    assert_eq!(first.await.unwrap(), SearchOutcome::Cancelled);
    assert!(matches!(second.await.unwrap(), SearchOutcome::Moved(_)));
    assert_eq!(pool.alive(), 4);
}

#[tokio::test(start_paused = true)]
async fn pruning_ends_the_engines_of_the_lane_that_do_not_play() {
    let fx = Fixture::new(Vec::new());
    let pool = fx.pool();
    pool.prepare(&in_lane(0, 0, 1), 1).await.expect("ready");
    pool.prepare(&in_lane(1, 0, 1), 2).await.expect("ready");
    assert_eq!(pool.alive(), 4);
    let next = PreparePlan {
        prune_lane: true,
        ..in_lane(0, 2, 3)
    };
    pool.prepare(&next, 3).await.expect("ready");
    // Lane 0 holds only its new pair; lane 1 is untouched.
    assert_eq!(pool.alive(), 4);
}
