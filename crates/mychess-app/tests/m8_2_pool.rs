//! M8-2a contract tests of the private engine pool (spec D1267, D1268,
//! D1270, D1291, D1302, D1307, D1308): time measured in the task, deadline,
//! cancellation within 50 ms, ponder hit and miss, shutdown. Time is paused
//! (`start_paused`), so nothing here sleeps for real.
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

mod m8_2_common;

use std::time::Duration;

use m8_2_common::*;
use mychess_app::tournament::{PonderJob, PoolCommand, SearchJob, SearchOutcome};
use mychess_app::{Think, ThinkLimits};
use mychess_core::{Color, Position};
use mychess_engine::Score;
use tokio::time::Instant;

fn think_after(moves: &[&str], limits: ThinkLimits) -> Think {
    let start = Position::new();
    let mut position = start.clone();
    let mut played = Vec::new();
    for text in moves {
        let m = position.parse_uci(text).unwrap();
        position.play(m).unwrap();
        played.push(m);
    }
    Think {
        start,
        moves: played,
        side: position.turn(),
        limits,
    }
}

fn job(participant: usize, moves: &[&str], deadline: Duration, epoch: u64) -> SearchJob {
    SearchJob {
        lane: 0,
        participant,
        think: think_after(moves, ThinkLimits::Depth(5)),
        deadline,
        ponder: None,
        epoch,
    }
}

const LONG: Duration = Duration::from_secs(60);

async fn ready(fx: &Fixture) -> mychess_app::tournament::EnginePool {
    let pool = fx.pool();
    pool.prepare(&plan((0, "A"), (1, "B")), 1)
        .await
        .expect("ready");
    pool
}

#[tokio::test(start_paused = true)]
async fn the_time_of_a_move_is_measured_inside_the_task_from_go_to_bestmove() {
    let fx = Fixture::new(vec![(
        "A",
        Scripted {
            delay: Duration::from_millis(300),
            ..Scripted::default()
        },
    )]);
    let pool = ready(&fx).await;
    let SearchOutcome::Moved(reply) = pool.search(job(0, &[], LONG, 2)).await else {
        panic!("a move")
    };
    assert!(
        reply.spent >= Duration::from_millis(300),
        "{:?}",
        reply.spent
    );
    assert!(
        reply.spent < Duration::from_millis(400),
        "{:?}",
        reply.spent
    );
    assert!(reply.mv.is_some());
    assert_eq!(pool.alive(), 2, "the engine goes back to the pool");
}

#[tokio::test(start_paused = true)]
async fn the_score_comes_from_the_white_point_of_view_whoever_moves() {
    // D1302: the engine says "cp 25" for the side to move.
    let fx = Fixture::new(Vec::new());
    let pool = ready(&fx).await;
    let SearchOutcome::Moved(white) = pool.search(job(0, &[], LONG, 2)).await else {
        panic!("white")
    };
    assert_eq!(white.score, Some(Score::Cp(25)));
    let SearchOutcome::Moved(black) = pool.search(job(1, &["a2a3"], LONG, 3)).await else {
        panic!("black")
    };
    assert_eq!(
        black.score,
        Some(Score::Cp(-25)),
        "Black's +25 is -25 for White"
    );
    assert_eq!(black.depth, Some(10));
}

#[tokio::test(start_paused = true)]
async fn cancel_stops_a_long_search_within_about_fifty_milliseconds() {
    let fx = Fixture::new(vec![(
        "A",
        Scripted {
            delay: Duration::from_secs(3_600),
            ..Scripted::default()
        },
    )]);
    let pool = ready(&fx).await;
    let p = pool.clone();
    let started = Instant::now();
    let task =
        tokio::spawn(async move { p.search(job(0, &[], Duration::from_secs(7_200), 5)).await });
    tokio::time::sleep(Duration::from_millis(10)).await;
    pool.cancel(5);
    let outcome = task.await.unwrap();
    assert_eq!(outcome, SearchOutcome::Cancelled);
    let waited = started.elapsed();
    assert!(waited <= Duration::from_millis(150), "took {waited:?}");
    assert_eq!(pool.alive(), 2, "a polite stop keeps the engine");
}

#[tokio::test(start_paused = true)]
async fn a_cancel_of_a_later_epoch_also_stops_earlier_searches_but_not_newer_ones() {
    let fx = Fixture::new(vec![(
        "A",
        Scripted {
            delay: Duration::from_millis(500),
            ..Scripted::default()
        },
    )]);
    let pool = ready(&fx).await;
    pool.cancel(7);
    // An older epoch is cancelled at once.
    let old = pool.search(job(0, &[], LONG, 6)).await;
    assert_eq!(old, SearchOutcome::Cancelled);
    // A newer one runs.
    let newer = pool.search(job(0, &[], LONG, 8)).await;
    assert!(matches!(newer, SearchOutcome::Moved(_)), "{newer:?}");
}

#[tokio::test(start_paused = true)]
async fn the_watchdog_stops_then_kills_an_engine_that_ignores_stop() {
    let fx = Fixture::new(vec![(
        "A",
        Scripted {
            delay: Duration::from_secs(3_600),
            ignore_stop: true,
            ..Scripted::default()
        },
    )]);
    let pool = ready(&fx).await;
    let started = Instant::now();
    let outcome = pool
        .search(job(0, &[], Duration::from_millis(500), 2))
        .await;
    assert_eq!(outcome, SearchOutcome::NoAnswer { killed: true });
    let took = started.elapsed();
    assert!(took >= Duration::from_millis(500), "{took:?}");
    assert!(
        took <= Duration::from_millis(1_700),
        "stop + 1 s grace: {took:?}"
    );
    assert_eq!(pool.alive(), 1, "the killed engine left the pool");
    // The log shows that a `stop` was sent before the kill.
    assert!(fx.lines("A").iter().any(|l| l == "stop"));
}

#[tokio::test(start_paused = true)]
async fn an_engine_that_answers_the_stop_after_the_deadline_is_kept() {
    let fx = Fixture::new(vec![(
        "A",
        Scripted {
            delay: Duration::from_secs(3_600),
            ..Scripted::default()
        },
    )]);
    let pool = ready(&fx).await;
    let outcome = pool
        .search(job(0, &[], Duration::from_millis(500), 2))
        .await;
    assert_eq!(outcome, SearchOutcome::NoAnswer { killed: false });
    assert_eq!(pool.alive(), 2);
}

#[tokio::test(start_paused = true)]
async fn an_engine_that_dies_on_go_is_reported_dead_and_leaves_the_pool() {
    let fx = Fixture::new(vec![(
        "A",
        Scripted {
            crash_on_go: true,
            ..Scripted::default()
        },
    )]);
    let pool = ready(&fx).await;
    let outcome = pool.search(job(0, &[], LONG, 2)).await;
    assert!(matches!(outcome, SearchOutcome::Dead(_)), "{outcome:?}");
    assert_eq!(pool.alive(), 1);
    // And asking an engine that is no longer there is also a failure.
    assert!(matches!(
        pool.search(job(0, &[], LONG, 3)).await,
        SearchOutcome::Dead(_)
    ));
}

#[tokio::test(start_paused = true)]
async fn prepare_sends_ucinewgame_and_the_options_and_reuses_living_engines() {
    let fx = Fixture::new(Vec::new());
    let pool = fx.pool();
    let mut p = plan((0, "A"), (1, "B"));
    p.launches[0].options = vec![("Hash".into(), "32".into())];
    let ready = pool.prepare(&p, 1).await.unwrap();
    assert_eq!(ready[0].engine_name.as_deref(), Some("Scripted 1.0"));
    assert!(ready[0].can_ponder);
    tokio::time::sleep(Duration::from_millis(10)).await;
    let first = fx.lines("A").iter().filter(|l| *l == "ucinewgame").count();
    assert!(first >= 1);
    pool.prepare(&p, 2).await.unwrap();
    assert_eq!(
        (fx.launches("A"), fx.launches("B")),
        (1, 1),
        "engines stay alive between games"
    );
    tokio::time::sleep(Duration::from_millis(10)).await;
    let lines = fx.lines("A");
    assert_eq!(
        lines.iter().filter(|l| *l == "ucinewgame").count(),
        first + 1,
        "one new game per game"
    );
    assert!(
        lines.iter().any(|l| l == "setoption name Hash value 32"),
        "{lines:?}"
    );
    p.restart = [true, false];
    pool.prepare(&p, 3).await.unwrap();
    assert_eq!((fx.launches("A"), fx.launches("B")), (2, 1));
}

#[tokio::test(start_paused = true)]
async fn an_engine_without_the_ponder_option_is_not_offered_ponder() {
    let fx = Fixture::new(vec![(
        "B",
        Scripted {
            declares_ponder: false,
            ..Scripted::default()
        },
    )]);
    let pool = fx.pool();
    let ready = pool.prepare(&plan((0, "A"), (1, "B")), 1).await.unwrap();
    assert!(ready[0].can_ponder);
    assert!(!ready[1].can_ponder);
}

#[tokio::test(start_paused = true)]
async fn a_slow_start_is_cancelled_in_about_fifty_milliseconds() {
    // D1308.
    let fx = Fixture::new(vec![(
        "A",
        Scripted {
            slow_start: Duration::from_mins(50),
            ..Scripted::default()
        },
    )]);
    let pool = fx.pool();
    let p = pool.clone();
    let started = Instant::now();
    let task = tokio::spawn(async move { p.prepare(&plan((0, "A"), (1, "B")), 4).await });
    tokio::time::sleep(Duration::from_millis(20)).await;
    pool.cancel(4);
    let result = task.await.unwrap();
    let failure = result.expect_err("cancelled");
    assert_eq!(failure.participant, 0);
    assert!(
        started.elapsed() <= Duration::from_millis(200),
        "{:?}",
        started.elapsed()
    );
    // The cancelled epoch stays cancelled: another start of it is refused at once.
    assert!(pool.prepare(&plan((0, "A"), (1, "B")), 3).await.is_err());
}

// ---- ponder ----

fn ponder_job(expected: &str) -> PonderJob {
    // Participant 0 ponders on the reply `expected` to its first move a2a3.
    PonderJob {
        participant: 0,
        think: Think {
            moves: {
                let mut pos = Position::new();
                ["a2a3", expected]
                    .iter()
                    .map(|u| {
                        let m = pos.parse_uci(u).unwrap();
                        pos.play(m).unwrap();
                        m
                    })
                    .collect()
            },
            ..think_after(&[], ThinkLimits::Depth(5))
        },
        expected: {
            let mut pos = Position::new();
            let m = pos.parse_uci("a2a3").unwrap();
            pos.play(m).unwrap();
            pos.parse_uci(expected).unwrap()
        },
    }
}

fn count(lines: &[String], prefix: &str) -> usize {
    lines.iter().filter(|l| l.starts_with(prefix)).count()
}

#[tokio::test(start_paused = true)]
async fn a_ponder_hit_sends_ponderhit_instead_of_a_new_go_and_counts_time_from_the_hit() {
    let fx = Fixture::new(vec![(
        "A",
        Scripted {
            hit_delay: Duration::from_millis(200),
            ..Scripted::default()
        },
    )]);
    let pool = ready(&fx).await;
    // A moves a2a3 (first legal); B, thinking, lets A ponder on a7a5 (B's
    // first legal move).
    pool.search(job(0, &[], LONG, 2)).await;
    let mut b = job(1, &["a2a3"], LONG, 3);
    b.ponder = Some(ponder_job("a7a5"));
    let SearchOutcome::Moved(reply) = pool.search(b).await else {
        panic!("B moved")
    };
    assert_eq!(pool.alive(), 2);
    assert_eq!(
        reply
            .mv
            .map(|m| Position::new()
                .after(Position::new().parse_uci("a2a3").unwrap())
                .unwrap()
                .uci(m))
            .as_deref(),
        Some("a7a5")
    );
    // Time passes while A "ponders"; it must not count against A.
    tokio::time::sleep(Duration::from_secs(30)).await;
    let SearchOutcome::Moved(a_reply) = pool.search(job(0, &["a2a3", "a7a5"], LONG, 4)).await
    else {
        panic!("A moved")
    };
    assert!(
        a_reply.spent >= Duration::from_millis(200) && a_reply.spent < Duration::from_secs(1),
        "{:?}",
        a_reply.spent
    );
    let lines = fx.lines("A");
    assert_eq!(count(&lines, "ponderhit"), 1, "{lines:?}");
    assert!(
        lines.iter().any(|l| l.starts_with("go ponder")),
        "{lines:?}"
    );
    assert_eq!(
        count(&lines, "go "),
        2,
        "go (move 1) and go ponder; the hit needs no third go: {lines:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_ponder_miss_stops_the_guess_and_searches_the_real_position() {
    let fx = Fixture::new(Vec::new());
    let pool = ready(&fx).await;
    pool.search(job(0, &[], LONG, 2)).await;
    let mut b = job(1, &["a2a3"], LONG, 3);
    b.ponder = Some(ponder_job("b7b5")); // wrong guess: B plays a7a5
    pool.search(b).await;
    let SearchOutcome::Moved(a_reply) = pool.search(job(0, &["a2a3", "a7a5"], LONG, 4)).await
    else {
        panic!("A moved")
    };
    let lines = fx.lines("A");
    assert_eq!(count(&lines, "ponderhit"), 0, "{lines:?}");
    assert!(lines.iter().any(|l| l == "stop"), "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("position startpos moves a2a3 a7a5")),
        "the real position was searched: {lines:?}"
    );
    // The reply is legal in the real position, not in the guessed one.
    let mut real = Position::new();
    for u in ["a2a3", "a7a5"] {
        let m = real.parse_uci(u).unwrap();
        real.play(m).unwrap();
    }
    assert!(real.is_legal(a_reply.mv.unwrap()));
    assert_eq!(real.turn(), Color::White);
}

#[tokio::test(start_paused = true)]
async fn stopping_the_ponder_forgets_the_guess_so_the_next_search_is_normal() {
    let fx = Fixture::new(Vec::new());
    let pool = ready(&fx).await;
    pool.search(job(0, &[], LONG, 2)).await;
    let mut b = job(1, &["a2a3"], LONG, 3);
    b.ponder = Some(ponder_job("a7a5"));
    pool.search(b).await;
    pool.command(PoolCommand::StopPonder).await;
    pool.search(job(0, &["a2a3", "a7a5"], LONG, 4)).await;
    let lines = fx.lines("A");
    assert_eq!(count(&lines, "ponderhit"), 0, "{lines:?}");
}

// ---- shutdown ----

#[tokio::test(start_paused = true)]
async fn shutdown_empties_the_pool_and_an_engine_coming_back_afterwards_is_ended() {
    let fx = Fixture::new(vec![(
        "A",
        Scripted {
            delay: Duration::from_secs(1),
            ..Scripted::default()
        },
    )]);
    let pool = ready(&fx).await;
    let p = pool.clone();
    let task = tokio::spawn(async move { p.search(job(0, &[], LONG, 2)).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    pool.command(PoolCommand::Shutdown).await;
    assert!(matches!(
        task.await.unwrap(),
        SearchOutcome::Moved(_) | SearchOutcome::Cancelled
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(pool.alive(), 0, "no engine survives the shutdown");
    // The next game opens the pool again.
    pool.prepare(&plan((0, "A"), (1, "B")), 9).await.unwrap();
    assert_eq!(pool.alive(), 2);
}

#[tokio::test(start_paused = true)]
async fn discard_ends_one_engine_only() {
    let fx = Fixture::new(Vec::new());
    let pool = ready(&fx).await;
    pool.command(PoolCommand::Discard(0)).await;
    assert_eq!(pool.alive(), 1);
    pool.command(PoolCommand::Discard(5)).await;
    assert_eq!(pool.alive(), 1);
}
