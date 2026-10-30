//! M8-2c contract tests of the Elo, the LOS and the report of time (spec
//! D2510-D2516) over whole tournaments played by the state machine, plus the
//! limits of the machine (D2501).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::float_cmp,
    clippy::needless_range_loop,
    clippy::assert_is_empty
)]

mod m8_2_common;

use std::time::Duration;

use m8_2_common::*;
use mychess_app::tournament::{
    ExportFormat, ExportLabels, ExportTable, ParallelLimits, Phase, Reliability, SearchOutcome,
    SystemResources, TimeControl, TimeVerdict, Tournament, TournamentConfig, export_file_name,
    export_ratings, export_time,
};
use mychess_core::GameResult;

fn play(config: TournamentConfig, policy: Policy) -> Sim {
    let mut sim = Sim::new(config, policy);
    let actions = sim.t.begin(sim.now);
    sim.run(actions);
    assert_eq!(sim.t.phase(), Phase::Finished);
    sim
}

#[test]
fn the_table_of_ratings_follows_the_results() {
    // P0 beats everybody twice; P1 and P2 draw each other.
    let policy = decide_by(|ctx| {
        if ctx.white == 0 {
            GameResult::White
        } else if ctx.black == 0 {
            GameResult::Black
        } else {
            GameResult::Draw
        }
    });
    let sim = play(
        TournamentConfig {
            cycles: 2,
            ..cfg(3)
        },
        policy,
    );
    let table = sim.t.ratings();
    assert_eq!(table.len(), 3);
    assert_eq!(table[0].participant, 0);
    assert_eq!(table[0].rank, 1);
    let top = &table[0].estimate;
    assert_eq!(
        (top.record.wins, top.record.draws, top.record.losses),
        (4, 0, 0)
    );
    assert_eq!(top.elo, Some(f64::INFINITY));
    assert_eq!(top.reliability, Reliability::VeryUncertain);
    assert!(top.los.expect("los") > 0.97);
    // The others lost twice to P0 and drew once... each: 0 wins, 2 draws, 2 losses.
    for row in &table[1..] {
        let r = row.estimate.record;
        assert_eq!((r.wins, r.draws, r.losses), (0, 2, 2));
        assert!(row.estimate.elo.expect("elo") < 0.0);
        assert!(row.estimate.interval.is_some());
    }
    // Same order as the standings.
    let by_points: Vec<usize> = sim.t.standings().iter().map(|r| r.participant).collect();
    assert_eq!(by_points[0], 0);
}

#[test]
fn the_los_matrix_is_complementary() {
    let sim = play(
        TournamentConfig {
            cycles: 3,
            ..cfg(3)
        },
        decide_by(|ctx| {
            if ctx.white < ctx.black {
                GameResult::White
            } else {
                GameResult::Black
            }
        }),
    );
    let m = sim.t.los_matrix();
    for i in 0..3 {
        assert_eq!(m[i][i], None);
        for j in 0..3 {
            if i != j {
                let (a, b) = (m[i][j].expect("a"), m[j][i].expect("b"));
                assert!((a + b - 1.0).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn nothing_is_estimated_before_the_first_game() {
    let t = tournament(cfg(3));
    for row in t.ratings() {
        assert_eq!(row.estimate.elo, None);
        assert_eq!(row.estimate.reliability, Reliability::Empty);
    }
    assert!(
        t.time_report()
            .iter()
            .all(|r| r.verdict == TimeVerdict::NoData)
    );
}

/// Moves that take `ms(participant)` milliseconds.
fn slow(ms: fn(usize) -> u64) -> Policy {
    Box::new(move |job, ctx| {
        let mut outcome = scripted_result(job, GameResult::White);
        let participant = if job.think.side == mychess_core::Color::White {
            ctx.white
        } else {
            ctx.black
        };
        if let SearchOutcome::Moved(reply) = &mut outcome {
            reply.spent = Duration::from_millis(ms(participant));
        }
        outcome
    })
}

#[test]
fn the_report_of_time_tells_who_respects_the_clock() {
    let config = TournamentConfig {
        time: TimeControl::MoveTime { ms: 1000 },
        cycles: 1,
        ..cfg(3)
    };
    // P0 takes 400 ms, P1 1200 ms (over the limit with slack, under the
    // leeway), P2 2000 ms (over 175%: the flag falls).
    let sim = play(config, slow(|p| [400, 1200, 2000][p]));
    let report = sim.t.time_report();
    assert_eq!(report.len(), 3);
    assert_eq!(report[0].mean_ms, Some(400));
    assert_eq!(report[0].verdict, TimeVerdict::Respects);
    assert_eq!(report[1].verdict, TimeVerdict::Overruns);
    assert_eq!(report[1].overhead_ms, Some(200));
    assert_eq!(report[1].stats.max_ms, 1200);
    assert_eq!(report[2].verdict, TimeVerdict::Forfeits);
    assert!(report[2].stats.forfeits >= 1);
    // A flag is a loss for the engine that was too slow, not for the other.
    assert_eq!(report[0].stats.forfeits, 0);
    assert_eq!(report[1].stats.forfeits, 0);
}

#[test]
fn a_clock_game_records_the_lowest_clock_and_the_share_used() {
    let config = TournamentConfig {
        time: TimeControl::Clock {
            base_ms: 60_000,
            increment_ms: 1_000,
        },
        cycles: 1,
        ..cfg(2)
    };
    let sim = play(config, slow(|_| 2_000));
    for row in sim.t.time_report() {
        assert!(row.stats.min_left_ms.is_some());
        let used = row.clock_used_percent.expect("share");
        assert!(used > 0.0 && used < 100.0, "{used}");
        assert_eq!(row.verdict, TimeVerdict::Respects);
    }
}

#[test]
fn moves_of_the_book_and_the_opening_are_not_timed() {
    // Depth games: only the engine moves are counted.
    let sim = play(cfg(2), slow(|_| 50));
    let report = sim.t.time_report();
    let moves: u64 = report.iter().map(|r| r.stats.moves).sum();
    let plies = sim.games[0].1.mainline().count() as u64;
    assert_eq!(moves, plies, "every ply was an engine move here");
}

#[test]
fn the_report_survives_the_saved_state() {
    let config = TournamentConfig {
        time: TimeControl::MoveTime { ms: 1000 },
        cycles: 1,
        ..cfg(3)
    };
    let sim = play(config, slow(|p| [400, 1200, 2000][p]));
    let saved = sim.t.to_saved();
    let text = mychess_app::tournament::render_state(&saved).expect("render");
    let back = mychess_app::tournament::parse_state(&text).expect("parse");
    let resumed = Tournament::resume(back, None, None).expect("resume");
    assert_eq!(resumed.time_report(), sim.t.time_report());
}

#[test]
fn a_state_file_from_before_the_report_still_loads() {
    let t = tournament(cfg(2));
    let mut saved = t.to_saved();
    saved.time_stats.clear();
    let text = mychess_app::tournament::render_state(&saved).expect("render");
    let old: String = text
        .lines()
        .filter(|l| !l.starts_with("time_stats") && !l.starts_with("[[time_stats"))
        .collect::<Vec<_>>()
        .join("\n");
    let back = mychess_app::tournament::parse_state(&old).expect("parse");
    let resumed = Tournament::resume(back, None, None).expect("resume");
    assert_eq!(resumed.time_report().len(), 2);
}

#[test]
fn the_new_tables_are_exported_in_every_format() {
    let sim = play(
        TournamentConfig {
            cycles: 2,
            ..cfg(3)
        },
        decide_by(|ctx| {
            if ctx.white == 0 {
                GameResult::White
            } else {
                GameResult::Black
            }
        }),
    );
    let names = sim.t.config().names();
    let labels = ExportLabels::default();
    let ratings = sim.t.ratings();
    let csv = export_ratings(&names, &ratings, &labels, ExportFormat::Csv);
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 4, "{csv}");
    assert!(lines[0].starts_with("Rank,Name,Played"), "{csv}");
    // Numbers that begin with a sign are not quoted as formulas.
    assert!(!csv.contains("'+") && !csv.contains("'-"), "{csv}");
    let time = export_time(
        &names,
        &sim.t.time_report(),
        &labels,
        ExportFormat::Markdown,
    );
    assert!(time.contains("Verdict") && time.contains("P0"), "{time}");
    for format in [ExportFormat::Text, ExportFormat::Html] {
        assert!(!export_ratings(&names, &ratings, &labels, format).is_empty());
        assert!(!export_time(&names, &sim.t.time_report(), &labels, format).is_empty());
    }
    assert_eq!(
        export_file_name("a:b", ExportTable::Ratings, ExportFormat::Csv),
        "a_b-ratings.csv"
    );
    assert!(export_file_name("t", ExportTable::Time, ExportFormat::Html).ends_with("-time.html"));
}

#[test]
fn the_limits_follow_the_machine() {
    let machine = SystemResources {
        cores: 16,
        ram_available_mb: Some(15_000),
    };
    let l = ParallelLimits::compute(machine, Some(1), Some(128), false);
    assert_eq!((l.hard_max, l.recommended), (14, 7));
    let small = SystemResources {
        cores: 2,
        ram_available_mb: Some(1_000),
    };
    let l = ParallelLimits::compute(small, Some(2), Some(256), true);
    assert_eq!((l.hard_max, l.recommended), (1, 1));
    assert_eq!(l.clamp(8), 1);
}

#[test]
fn probing_the_machine_gives_at_least_one_core() {
    let r = SystemResources::probe();
    assert!(r.cores >= 1);
}
