//! Tests of the engine pool with a scripted UCI player.

use std::fmt::Write;
use std::sync::{Arc, Mutex, PoisonError};

use mychess_core::{Color, Position};
use mychess_engine::testing::ManualEngine;
use mychess_engine::{EngineConfig, Protocol};

use super::*;
use crate::engines::EngineId;
use crate::play::ThinkLimits;

/// How a scripted engine behaves.
#[derive(Debug, Clone)]
pub(crate) struct Script {
    /// Time to think before the `bestmove`.
    pub(crate) delay: Duration,
    /// Time to answer after a `ponderhit`.
    pub(crate) hit_delay: Duration,
    /// Closes the pipe at the first `go`.
    pub(crate) crash_on_go: bool,
    /// Never answers `stop`.
    pub(crate) ignore_stop: bool,
    /// Adds `ponder <move>` to the `bestmove`.
    pub(crate) ponder: bool,
    /// Declares the `Ponder` option.
    pub(crate) declares_ponder: bool,
}

impl Default for Script {
    fn default() -> Script {
        Script {
            delay: Duration::ZERO,
            hit_delay: Duration::ZERO,
            crash_on_go: false,
            ignore_stop: false,
            ponder: false,
            declares_ponder: true,
        }
    }
}

pub(crate) type Log = Arc<Mutex<Vec<String>>>;

pub(crate) fn lines(log: &Log) -> Vec<String> {
    log.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

fn position_of(words: &[&str]) -> Position {
    let moves_at = words.iter().position(|w| *w == "moves");
    let head = &words[1..moves_at.unwrap_or(words.len())];
    let mut position = match head.first() {
        Some(&"fen") => Position::from_fen(&head[1..].join(" ")).unwrap_or_default(),
        _ => Position::new(),
    };
    if let Some(at) = moves_at {
        for text in &words[at + 1..] {
            match position.parse_uci(text) {
                Ok(m) if position.play(m).is_ok() => {}
                _ => break,
            }
        }
    }
    position
}

/// The first legal move by UCI text, with the reply to ponder on.
fn choose(position: &Position, ponder: bool) -> String {
    let mut moves: Vec<_> = position
        .legal_moves()
        .into_iter()
        .map(|m| (position.uci(m), m))
        .collect();
    moves.sort_by(|a, b| a.0.cmp(&b.0));
    let Some((text, mv)) = moves.into_iter().next() else {
        return "bestmove (none)".to_owned();
    };
    let mut line = format!("bestmove {text}");
    if ponder && let Ok(after) = position.after(mv) {
        let mut replies: Vec<String> = after
            .legal_moves()
            .into_iter()
            .map(|m| after.uci(m))
            .collect();
        replies.sort();
        if let Some(reply) = replies.first() {
            write!(line, " ponder {reply}").ok();
        }
    }
    line
}

async fn answer(manual: &mut ManualEngine, position: &Position, script: &Script) {
    manual.say("info depth 10 score cp 25").await;
    manual.say(&choose(position, script.ponder)).await;
}

async fn run_player(mut manual: ManualEngine, script: Script, log: Log) {
    let options: &[&str] = if script.declares_ponder {
        &[
            "option name Hash type spin default 16 min 1 max 1024",
            "option name Ponder type check default false",
        ]
    } else {
        &["option name Hash type spin default 16 min 1 max 1024"]
    };
    let seen = manual.handshake("Scripted 1.0", options).await;
    log.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .extend(seen);
    let mut position = Position::new();
    // The position of a search that waits for `ponderhit`, and one that
    // waits for its delay.
    let mut pondering: Option<Position> = None;
    let mut pending: Option<(Instant, Position)> = None;
    loop {
        let line = match pending.clone() {
            Some((until, found)) => {
                tokio::select! {
                    () = tokio::time::sleep_until(until) => {
                        pending = None;
                        answer(&mut manual, &found, &script).await;
                        continue;
                    }
                    line = manual.next_line() => line,
                }
            }
            None => manual.next_line().await,
        };
        let Some(line) = line else { return };
        log.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line.clone());
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.first().copied() {
            Some("isready") => manual.say("readyok").await,
            Some("position") => position = position_of(&words),
            Some("go") => {
                if script.crash_on_go {
                    manual.close().await;
                    return;
                }
                if words.contains(&"ponder") {
                    pondering = Some(position.clone());
                } else {
                    pending = Some((Instant::now() + script.delay, position.clone()));
                }
            }
            Some("ponderhit") => {
                if let Some(found) = pondering.take() {
                    pending = Some((Instant::now() + script.hit_delay, found));
                }
            }
            Some("stop") if !script.ignore_stop => {
                let target = pondering.take().or(pending.take().map(|(_, p)| p));
                if let Some(found) = target {
                    manual.say(&choose(&found, false)).await;
                }
            }
            Some("quit") => return,
            _ => {}
        }
    }
}

/// A pool whose engines are scripted by name; `logs` has the received lines
/// of every launched engine, `launches` counts the starts.
pub(crate) struct Fixture {
    pub(crate) pool: EnginePool,
    pub(crate) launcher: Launcher,
    pub(crate) logs: Arc<Mutex<Vec<(String, Log)>>>,
}

impl Fixture {
    pub(crate) fn new(scripts: Vec<(&'static str, Script)>) -> Fixture {
        let logs: Arc<Mutex<Vec<(String, Log)>>> = Arc::default();
        let recorded = Arc::clone(&logs);
        let launcher: Launcher = Arc::new(move |config: EngineConfig| {
            let script = scripts
                .iter()
                .find(|(name, _)| *name == config.name)
                .map(|(_, s)| s.clone())
                .unwrap_or_default();
            let log: Log = Arc::default();
            recorded
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((config.name.clone(), Arc::clone(&log)));
            Box::pin(async move {
                let (manual, engine, events) = ManualEngine::launch(config);
                tokio::spawn(run_player(manual, script, log));
                Ok((engine, events))
            })
        });
        Fixture {
            pool: EnginePool::new(Arc::clone(&launcher)),
            launcher,
            logs,
        }
    }

    pub(crate) fn launches(&self, name: &str) -> usize {
        self.logs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(n, _)| n == name)
            .count()
    }

    /// The lines received by the last launch of `name`.
    pub(crate) fn lines(&self, name: &str) -> Vec<String> {
        let logs = self.logs.lock().unwrap_or_else(PoisonError::into_inner);
        logs.iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, l)| lines(l))
            .unwrap_or_default()
    }
}

pub(crate) fn entry(name: &str) -> EngineEntry {
    EngineEntry {
        id: EngineId(1),
        name: name.to_owned(),
        command: "fake".into(),
        args: Vec::new(),
        working_dir: None,
        protocol: Protocol::Uci,
        elo: None,
        url: None,
        last_used: None,
        options: Vec::new(),
        multipv: 1,
        low_priority: None,
    }
}

pub(crate) fn plan(white: (usize, &str), black: (usize, &str), restart: bool) -> PreparePlan {
    let launch = |(participant, name): (usize, &str)| LaunchRequest {
        participant,
        entry: entry(name),
        options: vec![("Hash".to_owned(), "32".to_owned())],
        defaults: RunDefaults::default(),
    };
    PreparePlan {
        lane: 0,
        sides: [white.0, black.0],
        restart: [restart, restart],
        prune_lane: false,
        opponents: [white.1.to_owned(), black.1.to_owned()],
        launches: [launch(white), launch(black)],
    }
}

pub(crate) fn think(moves: &[&str], limits: ThinkLimits) -> Think {
    let start = Position::new();
    let mut position = start.clone();
    let mut played = Vec::new();
    for text in moves {
        let mv = position.parse_uci(text).expect("uci");
        position.play(mv).expect("play");
        played.push(mv);
    }
    Think {
        start,
        moves: played,
        side: position.turn(),
        limits,
    }
}

fn job(participant: usize, think: Think, deadline: Duration) -> SearchJob {
    SearchJob {
        lane: 0,
        participant,
        think,
        deadline,
        ponder: None,
        epoch: 1,
    }
}

const D5: ThinkLimits = ThinkLimits::Depth(5);

#[tokio::test(start_paused = true)]
async fn prepare_starts_the_engines_and_sends_a_new_game() {
    let fx = Fixture::new(Vec::new());
    let ready = fx
        .pool
        .prepare(&plan((0, "A"), (1, "B"), false), 1)
        .await
        .expect("ready");
    assert_eq!(ready[0].participant, 0);
    assert_eq!(ready[0].engine_name.as_deref(), Some("Scripted 1.0"));
    assert!(ready[0].can_ponder);
    assert_eq!(fx.pool.alive(), 2);
    // Settle the pipes.
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(
        fx.lines("A").iter().any(|l| l == "ucinewgame"),
        "{:?}",
        fx.lines("A")
    );
    assert!(
        fx.lines("A")
            .iter()
            .any(|l| l == "setoption name Hash value 32"),
        "the uniform option is sent: {:?}",
        fx.lines("A")
    );
}

#[tokio::test(start_paused = true)]
async fn engines_live_across_games_unless_restarted() {
    let fx = Fixture::new(Vec::new());
    fx.pool
        .prepare(&plan((0, "A"), (1, "B"), false), 1)
        .await
        .expect("1");
    fx.pool
        .prepare(&plan((1, "B"), (0, "A"), false), 1)
        .await
        .expect("2");
    assert_eq!((fx.launches("A"), fx.launches("B")), (1, 1));
    fx.pool
        .prepare(&plan((0, "A"), (1, "B"), true), 1)
        .await
        .expect("3");
    assert_eq!((fx.launches("A"), fx.launches("B")), (2, 2));
    assert_eq!(fx.pool.alive(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_dead_engine_is_started_again_for_the_next_game() {
    let fx = Fixture::new(vec![(
        "A",
        Script {
            crash_on_go: true,
            ..Script::default()
        },
    )]);
    fx.pool
        .prepare(&plan((0, "A"), (1, "B"), false), 1)
        .await
        .expect("1");
    let out = fx
        .pool
        .search(job(0, think(&[], D5), Duration::from_secs(5)))
        .await;
    assert!(matches!(out, SearchOutcome::Dead(_)), "{out:?}");
    assert_eq!(fx.pool.alive(), 1, "only B is left");
    fx.pool
        .prepare(&plan((0, "A"), (1, "B"), false), 1)
        .await
        .expect("2");
    assert_eq!(fx.launches("A"), 2);
    assert_eq!(fx.launches("B"), 1);
}

#[tokio::test(start_paused = true)]
async fn the_time_of_the_move_is_measured_in_the_task() {
    let fx = Fixture::new(vec![(
        "A",
        Script {
            delay: Duration::from_millis(300),
            ..Script::default()
        },
    )]);
    fx.pool
        .prepare(&plan((0, "A"), (1, "B"), false), 1)
        .await
        .expect("ready");
    let out = fx
        .pool
        .search(job(0, think(&[], D5), Duration::from_secs(10)))
        .await;
    let SearchOutcome::Moved(reply) = out else {
        panic!("expected a move, got {out:?}");
    };
    assert!(
        reply.spent >= Duration::from_millis(300) && reply.spent < Duration::from_millis(400),
        "{:?}",
        reply.spent
    );
    assert_eq!(
        reply.mv.map(|m| Position::new().uci(m)),
        Some("a2a3".to_owned())
    );
    assert_eq!(reply.depth, Some(10));
    assert_eq!(reply.score, Some(EngineScore::Cp(25)));
}

#[tokio::test(start_paused = true)]
async fn the_limits_reach_the_engine_as_go_commands() {
    let fx = Fixture::new(Vec::new());
    fx.pool
        .prepare(&plan((0, "A"), (1, "B"), false), 1)
        .await
        .expect("ready");
    let clock = mychess_engine::Clock {
        white_time: Duration::from_secs(60),
        black_time: Duration::from_secs(59),
        white_increment: Duration::from_secs(1),
        black_increment: Duration::from_secs(1),
        moves_to_go: None,
    };
    for limits in [
        ThinkLimits::Clock(clock),
        ThinkLimits::MoveTime(Duration::from_millis(750)),
        ThinkLimits::Depth(7),
        ThinkLimits::Nodes(900),
    ] {
        let out = fx
            .pool
            .search(job(0, think(&[], limits), Duration::from_secs(10)))
            .await;
        assert!(matches!(out, SearchOutcome::Moved(_)), "{out:?}");
    }
    let sent = fx.lines("A");
    for expected in [
        "go wtime 60000 btime 59000 winc 1000 binc 1000",
        "go movetime 750",
        "go depth 7",
        "go nodes 900",
    ] {
        assert!(sent.iter().any(|l| l == expected), "{expected}: {sent:?}");
    }
    assert!(sent.iter().any(|l| l == "position startpos"), "{sent:?}");
}

#[tokio::test(start_paused = true)]
async fn a_silent_engine_gets_stop_and_is_killed_after_a_second() {
    let fx = Fixture::new(vec![(
        "A",
        Script {
            delay: Duration::from_secs(3600),
            ignore_stop: true,
            ..Script::default()
        },
    )]);
    fx.pool
        .prepare(&plan((0, "A"), (1, "B"), false), 1)
        .await
        .expect("ready");
    let started = Instant::now();
    let out = fx
        .pool
        .search(job(0, think(&[], D5), Duration::from_secs(2)))
        .await;
    assert_eq!(out, SearchOutcome::NoAnswer { killed: true });
    let took = started.elapsed();
    assert!(
        took >= Duration::from_secs(3) && took < Duration::from_millis(3200),
        "deadline 2 s + 1 s for the stop: {took:?}"
    );
    assert!(fx.lines("A").iter().any(|l| l == "stop"));
    assert_eq!(fx.pool.alive(), 1, "the killed engine left the pool");
}

#[tokio::test(start_paused = true)]
async fn a_late_answer_after_stop_keeps_the_engine() {
    let fx = Fixture::new(vec![(
        "A",
        Script {
            delay: Duration::from_secs(3600),
            ..Script::default()
        },
    )]);
    fx.pool
        .prepare(&plan((0, "A"), (1, "B"), false), 1)
        .await
        .expect("ready");
    let out = fx
        .pool
        .search(job(0, think(&[], D5), Duration::from_secs(2)))
        .await;
    assert_eq!(out, SearchOutcome::NoAnswer { killed: false });
    assert_eq!(fx.pool.alive(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_cancelled_search_stops_the_engine() {
    let fx = Fixture::new(vec![(
        "A",
        Script {
            delay: Duration::from_secs(3600),
            ..Script::default()
        },
    )]);
    fx.pool
        .prepare(&plan((0, "A"), (1, "B"), false), 1)
        .await
        .expect("ready");
    let pool = fx.pool.clone();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        pool.cancel(1);
    });
    let out = fx
        .pool
        .search(job(0, think(&[], D5), Duration::from_secs(600)))
        .await;
    canceller.await.expect("task");
    assert_eq!(out, SearchOutcome::Cancelled);
    assert_eq!(fx.pool.alive(), 2, "the engine answered the stop");
    // A later epoch is not cancelled.
    let again = SearchJob {
        epoch: 2,
        ..job(1, think(&[], D5), Duration::from_secs(10))
    };
    assert!(matches!(
        fx.pool.search(again).await,
        SearchOutcome::Moved(_)
    ));
}

#[tokio::test(start_paused = true)]
async fn a_missing_engine_is_reported_dead() {
    let fx = Fixture::new(Vec::new());
    let out = fx
        .pool
        .search(job(5, think(&[], D5), Duration::from_secs(1)))
        .await;
    assert!(matches!(out, SearchOutcome::Dead(_)));
}

#[tokio::test(start_paused = true)]
async fn ponderhit_starts_the_clock_only_at_the_hit() {
    let script = Script {
        ponder: true,
        hit_delay: Duration::from_millis(200),
        ..Script::default()
    };
    let fx = Fixture::new(vec![("A", script.clone()), ("B", script)]);
    fx.pool
        .prepare(&plan((0, "A"), (1, "B"), false), 1)
        .await
        .expect("ready");
    // White moves a2a3 and expects a7a5 as reply.
    let first = fx
        .pool
        .search(job(0, think(&[], D5), Duration::from_secs(10)))
        .await;
    let SearchOutcome::Moved(first) = first else {
        panic!("{first:?}")
    };
    let expected = first.ponder.expect("ponder move");
    let b_job = SearchJob {
        lane: 0,
        participant: 1,
        think: think(&["a2a3"], D5),
        deadline: Duration::from_secs(10),
        ponder: Some(PonderJob {
            participant: 0,
            think: think(&["a2a3", "a7a5"], D5),
            expected,
        }),
        epoch: 1,
    };
    assert!(matches!(
        fx.pool.search(b_job).await,
        SearchOutcome::Moved(_)
    ));
    // Time passes while A ponders; it must not count against A.
    tokio::time::sleep(Duration::from_secs(30)).await;
    let SearchOutcome::Moved(reply) = fx
        .pool
        .search(job(
            0,
            think(&["a2a3", "a7a5"], D5),
            Duration::from_secs(10),
        ))
        .await
    else {
        panic!("A moved")
    };
    assert!(
        reply.spent >= Duration::from_millis(200) && reply.spent < Duration::from_secs(1),
        "{:?}",
        reply.spent
    );
    // The reply is legal in the position after the expected move.
    let mut real = Position::new();
    for u in ["a2a3", "a7a5"] {
        let m = real.parse_uci(u).expect("uci");
        real.play(m).expect("legal");
    }
    assert_eq!(real.turn(), Color::White);
    assert!(reply.mv.is_some_and(|m| real.is_legal(m)));
    let lines = fx.lines("A");
    assert_eq!(
        lines.iter().filter(|l| *l == "ponderhit").count(),
        1,
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("go ponder")),
        "{lines:?}"
    );
    assert_eq!(
        lines.iter().filter(|l| l.starts_with("go ")).count(),
        2,
        "go (move 1) and go ponder; the hit needs no third go: {lines:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_wrong_ponder_guess_discards_the_old_search() {
    let script = Script {
        ponder: true,
        ..Script::default()
    };
    let fx = Fixture::new(vec![("A", script.clone()), ("B", script)]);
    fx.pool
        .prepare(&plan((0, "A"), (1, "B"), false), 1)
        .await
        .expect("ready");
    let first = fx
        .pool
        .search(job(0, think(&[], D5), Duration::from_secs(10)))
        .await;
    let SearchOutcome::Moved(first) = first else {
        panic!("{first:?}")
    };
    // The expected move is a7a5, but Black plays something else (the job
    // tells the pool what is expected; the think says what was played).
    let expected = first.ponder.expect("ponder move");
    let b_job = SearchJob {
        lane: 0,
        participant: 1,
        think: think(&["a2a3"], D5),
        deadline: Duration::from_secs(10),
        ponder: Some(PonderJob {
            participant: 0,
            think: think(&["a2a3", "a7a5"], D5),
            expected,
        }),
        epoch: 1,
    };
    assert!(matches!(
        fx.pool.search(b_job).await,
        SearchOutcome::Moved(_)
    ));
    // White moves after a different reply, b7b6.
    let out = fx
        .pool
        .search(job(
            0,
            think(&["a2a3", "b7b6"], D5),
            Duration::from_secs(10),
        ))
        .await;
    assert!(matches!(out, SearchOutcome::Moved(_)), "{out:?}");
    let lines = fx.lines("A");
    assert!(!lines.iter().any(|l| l == "ponderhit"), "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|l| l == "position startpos moves a2a3 b7b6"),
        "{lines:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn stopping_the_pondering_makes_the_next_search_normal() {
    let script = Script {
        ponder: true,
        ..Script::default()
    };
    let fx = Fixture::new(vec![("A", script.clone()), ("B", script)]);
    fx.pool
        .prepare(&plan((0, "A"), (1, "B"), false), 1)
        .await
        .expect("ready");
    let SearchOutcome::Moved(first) = fx
        .pool
        .search(job(0, think(&[], D5), Duration::from_secs(10)))
        .await
    else {
        panic!("move");
    };
    let expected = first.ponder.expect("ponder move");
    let b_job = SearchJob {
        lane: 0,
        participant: 1,
        think: think(&["a2a3"], D5),
        deadline: Duration::from_secs(10),
        ponder: Some(PonderJob {
            participant: 0,
            think: think(&["a2a3", "a7a5"], D5),
            expected,
        }),
        epoch: 1,
    };
    fx.pool.search(b_job).await;
    fx.pool.stop_pondering();
    let out = fx
        .pool
        .search(job(
            0,
            think(&["a2a3", "a7a5"], D5),
            Duration::from_secs(10),
        ))
        .await;
    assert!(matches!(out, SearchOutcome::Moved(_)), "{out:?}");
    assert!(!fx.lines("A").iter().any(|l| l == "ponderhit"));
}

#[tokio::test(start_paused = true)]
async fn shutdown_empties_the_pool() {
    let fx = Fixture::new(Vec::new());
    fx.pool
        .prepare(&plan((0, "A"), (1, "B"), false), 1)
        .await
        .expect("ready");
    fx.pool.shutdown().await;
    assert_eq!(fx.pool.alive(), 0);
}

#[tokio::test(start_paused = true)]
async fn an_engine_that_comes_back_after_the_shutdown_is_ended() {
    // Regression (D1307): the search in flight put its engine back into the
    // emptied pool and the process outlived the tournament.
    let fx = Fixture::new(vec![(
        "A",
        Script {
            delay: Duration::from_secs(3600),
            ..Script::default()
        },
    )]);
    fx.pool
        .prepare(&plan((0, "A"), (1, "B"), false), 1)
        .await
        .expect("ready");
    let pool = fx.pool.clone();
    let search = tokio::spawn(async move {
        pool.search(job(0, think(&[], D5), Duration::from_secs(7200)))
            .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    fx.pool.cancel(1);
    fx.pool.shutdown().await;
    assert_eq!(search.await.expect("task"), SearchOutcome::Cancelled);
    assert_eq!(fx.pool.alive(), 0, "the engine was ended, not kept");
}

#[tokio::test(start_paused = true)]
async fn an_engine_put_over_a_busy_place_ends_the_one_it_replaces() {
    // Regression (D2532): `put` dropped the engine that `insert` returned and
    // its process outlived the tournament.
    let fx = Fixture::new(Vec::new());
    let plan = plan((0, "A"), (1, "B"), false);
    fx.pool.prepare(&plan, 1).await.expect("ready");
    let old = fx.pool.take(0, 0).expect("the engine of A");
    // The place is empty now: the next game starts another A there.
    fx.pool.prepare(&plan, 2).await.expect("ready again");
    assert_eq!(fx.launches("A"), 2);
    fx.pool.put(0, 0, old);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(fx.pool.alive(), 2, "one engine per place");
    let quit = fx
        .logs
        .lock()
        .expect("logs")
        .iter()
        .filter(|(name, log)| name == "A" && lines(log).iter().any(|l| l == "quit"))
        .count();
    assert_eq!(quit, 1, "the replaced engine was told to quit");
}

#[tokio::test(start_paused = true)]
async fn a_prepare_after_a_shutdown_opens_the_pool_again() {
    // "Stop after the game" shuts the pool down and "Continue" prepares again.
    let fx = Fixture::new(Vec::new());
    let plan = plan((0, "A"), (1, "B"), false);
    fx.pool.prepare(&plan, 1).await.expect("ready");
    fx.pool.shutdown().await;
    fx.pool.prepare(&plan, 2).await.expect("ready again");
    assert_eq!(fx.pool.alive(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_cancel_stops_an_engine_that_is_still_starting() {
    // Regression (D1308): `Abort` waited up to a minute for a launch.
    let launcher: Launcher = Arc::new(|_| Box::pin(std::future::pending()));
    let pool = EnginePool::new(launcher);
    let task_pool = pool.clone();
    let prepare =
        tokio::spawn(async move { task_pool.prepare(&plan((0, "A"), (1, "B"), false), 7).await });
    tokio::time::sleep(Duration::from_secs(2)).await;
    let asked = Instant::now();
    pool.cancel(7);
    let result = prepare.await.expect("task");
    assert!(result.is_err());
    assert!(
        asked.elapsed() <= Duration::from_millis(60),
        "stopped within one poll: {:?}",
        asked.elapsed()
    );
    assert_eq!(pool.alive(), 0);
}

#[tokio::test(start_paused = true)]
async fn a_prepare_of_a_cancelled_epoch_does_not_start_anything() {
    let fx = Fixture::new(Vec::new());
    fx.pool.cancel(9);
    let out = fx.pool.prepare(&plan((0, "A"), (1, "B"), false), 9).await;
    assert!(out.is_err());
    assert_eq!(fx.launches("A"), 0);
}
