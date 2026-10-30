//! Performance of the opening report (spec `m7-2-relatorio-de-abertura.md`,
//! D3627): a report over 100 000 games in under five seconds. Ignored: run
//! with `cargo test -p mychess-app --release --test m7_2b_perf -- --ignored
//! --nocapture`.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod m7_2b_common;

use std::fmt::Write as _;
use std::time::Instant;

use m7_2b_common::*;

/// Openings the synthetic games start with; the rest is made of a few more
/// moves chosen by the generator, so many games share the position.
const LINES: [&str; 8] = [
    "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 6. Re1 b5 7. Bb3 d6",
    "1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 4. c3 Nf6 5. d3 d6 6. O-O O-O 7. Nbd2 a6",
    "1. e4 e5 2. Nf3 Nc6 3. d4 exd4 4. Nxd4 Nf6 5. Nxc6 bxc6 6. e5 Qe7 7. Qe2 Nd5",
    "1. e4 e5 2. Nf3 Nf6 3. Nxe5 d6 4. Nf3 Nxe4 5. d4 d5 6. Bd3 Nc6 7. O-O Be7",
    "1. e4 e5 2. Nf3 d6 3. d4 exd4 4. Nxd4 Nf6 5. Nc3 Be7 6. Be2 O-O 7. O-O Nc6",
    "1. e4 e5 2. Nf3 Nc6 3. Bb5 Nf6 4. O-O Nxe4 5. d4 Nd6 6. Bxc6 dxc6 7. dxe5 Nf5",
    "1. e4 e5 2. Nf3 Nc6 3. Bb5 f5 4. Nc3 fxe4 5. Nxe4 d5 6. Nxe5 dxe4 7. Nxc6 Qg5",
    "1. e4 c5 2. Nf3 d6 3. d4 cxd4 4. Nxd4 Nf6 5. Nc3 a6 6. Be3 e5 7. Nb3 Be6",
];

/// A PGN of `count` games, the same on every run.
fn big_pgn(count: usize) -> String {
    let results = ["1-0", "0-1", "1/2-1/2"];
    let mut seed = 0x9e37_79b9_u32;
    let mut next = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        usize::try_from(seed >> 8).unwrap()
    };
    let mut out = String::with_capacity(count * 330);
    for i in 0..count {
        let result = results[next() % 3];
        let year = 1990 + next() % 35;
        writeln!(
            out,
            "[Event \"E{}\"]\n[Site \"S{}\"]\n[Date \"{year}.01.01\"]\n[Round \"1\"]\n\
             [White \"White {}\"]\n[Black \"Black {}\"]\n[Result \"{result}\"]\n\
             [WhiteElo \"{}\"]\n[BlackElo \"{}\"]\n\n{} {result}\n",
            i % 500,
            i % 50,
            next() % 3000,
            next() % 3000,
            2000 + next() % 800,
            2000 + next() % 800,
            LINES[next() % LINES.len()],
        )
        .unwrap();
    }
    out
}

#[test]
#[ignore = "performance: run in release with --ignored"]
fn a_report_over_100000_games_takes_under_five_seconds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.pgn");
    std::fs::write(&path, big_pgn(100_000)).unwrap();
    let mut rig = Rig::new();
    rig.send(BasesMessage::Open(path));
    let slot = rig.state.bases.current();
    assert_eq!(rig.state.bases.slot(slot).unwrap().total(), 100_000);
    // The position after 1.e4 e5 2.Nf3: most of the games are under it.
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    rig.send(GameMessage::Navigate(mychess_app::Nav::GoToMainlinePly(3)));
    let effects = rig.raw(OpeningReportMessage::Generate);
    let compute = Rig::compute_of(&effects).expect("a job");
    let started = Instant::now();
    let answer = rig.perform(compute);
    let elapsed = started.elapsed();
    println!("opening report over 100000 games: {elapsed:?}");
    rig.raw(answer);
    let view = rig.state.opening_report_view();
    assert_eq!(view.phase, ReportPhase::Ready);
    assert!(view.report.unwrap().game_count() > 50_000);
    assert!(elapsed.as_secs_f64() < 5.0, "{elapsed:?}");
}
