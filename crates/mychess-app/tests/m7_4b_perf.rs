//! M7-4b performance: the extra formats on 100 000 games (run in release:
//! `cargo test --release -p mychess-app --test m7_4b_perf -- --ignored
//! --nocapture`; the numbers go to `docs/benchmarks.md`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m7_4b_common;

use std::time::Instant;

use m7_4b_common::*;
use tempfile::tempdir;

#[test]
#[ignore = "performance: run in release with --ignored --nocapture"]
fn exporting_100_000_games_in_every_format() {
    const GAMES: usize = 100_000;
    let dir = tempdir().unwrap();
    let started = Instant::now();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "big", &numbered(GAMES));
    rig.set_assets();
    println!(
        "opening a database of {GAMES} games: {:?}",
        started.elapsed()
    );
    for (format, name) in [
        (ExportFormat::Html, "big.html"),
        (ExportFormat::Latex, "big.tex"),
        (ExportFormat::Epd, "big.epd"),
        (ExportFormat::List, "big.txt"),
        (ExportFormat::Csv, "big.csv"),
        (ExportFormat::Viewer, "viewer.html"),
    ] {
        rig.gx(GamesExportMessage::Open);
        rig.format(format);
        if format == ExportFormat::Epd {
            rig.edit(ExportEdit::EpdFinalPosition(true));
        }
        let dest = dir.path().join(name);
        let (effects, _) = rig.gx_choose(&dest);
        let started = Instant::now();
        rig.gx_run(effects);
        let elapsed = started.elapsed();
        let size = std::fs::metadata(&dest).map_or(0, |m| m.len());
        println!("{format:?}: {elapsed:?}, {} MiB", size / (1024 * 1024));
        assert!(dest.exists(), "{format:?}");
        assert!(
            elapsed.as_secs() < 300,
            "{format:?} took {elapsed:?} for {GAMES} games"
        );
        assert!(!has_temp(dir.path()));
    }
}
