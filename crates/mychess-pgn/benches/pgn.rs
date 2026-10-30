//! Benchmarks of the PGN reader and writer (spec `m1-3-pgn.md`, D38: reading at
//! least 15 MB/s per core, writing at least 50 MB/s). The input is synthetic and built
//! in memory; no file is read.

#![allow(
    clippy::format_push_string,
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_possible_truncation
)]

use std::hint::black_box;
use std::io::Cursor;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use mychess_core::{Game, Position, SanMode};
use mychess_pgn::read::{GameChunker, PgnReader, ReadOptions};
use mychess_pgn::write::{WriteOptions, game_to_string};

const LINES: &[&str] = &[
    "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 6. Re1 b5 7. Bb3 d6 8. c3 O-O 9. h3 Nb8 10. d4 Nbd7 11. c4 c6 12. cxb5 axb5 13. Nc3 Bb7 14. Bg5 b4 15. Nb1 h6 16. Bh4 c5 17. dxe5 Nxe4 18. Bxe7 Qxe7 19. exd6 Qf6 20. Nbd2 Nxd6 21. Nc4 Nxc4 22. Bxc4 Nb6 23. Ne5 Rae8 24. Bxf7+ Rxf7 25. Nxf7 Rxe1+ 26. Qxe1 Kxf7 27. Qe3 Qg5 28. Qxg5 hxg5 29. b3 Ke6 30. a3 Kd6 31. axb4 cxb4 32. Ra5 Nd5 33. f3 Bc8 34. Kf2 Bf5 35. Ra7 g6 36. Ra6+ Kc5 37. Ke1 Nf4 38. g3 Nxh3 39. Kd2 Kb5 40. Rd6 Kc5 41. Ra6 Nf2 42. g4 Bd3 43. Re6 1/2-1/2",
    "1. d4 Nf6 2. c4 e6 3. Nc3 Bb4 4. e3 O-O 5. Bd3 d5 6. Nf3 c5 7. O-O Nc6 8. a3 Bxc3 9. bxc3 dxc4 10. Bxc4 Qc7 11. Bd3 e5 12. Qc2 Re8 13. Nxe5 Nxe5 14. dxe5 Qxe5 15. f4 Qc7 16. e4 Bg4 17. Rf3 Rad8 18. Bb2 Bh5 19. Rh3 Bg6 1/2-1/2",
];

fn annotated(i: usize) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "[Event \"Bench {i}\"]\n[Site \"Rust\"]\n[Date \"2024.01.{:02}\"]\n[Round \"{}\"]\n[White \"Player, A{i}\"]\n[Black \"Player, B{i}\"]\n[Result \"*\"]\n[WhiteElo \"2500\"]\n[BlackElo \"2400\"]\n[ECO \"C80\"]\n\n",
        1 + i % 28,
        i % 9
    ));
    let line = LINES[i % LINES.len()];
    s.push_str(line);
    s.push_str("\n\n");
    s
}

fn plain_corpus(target: usize) -> Vec<u8> {
    let mut s = String::new();
    let mut i = 0;
    while s.len() < target {
        s.push_str(&annotated(i));
        i += 1;
    }
    s.into_bytes()
}

/// Same moves with comments, NAGs and a variation per game.
fn rich_corpus(target: usize) -> Vec<u8> {
    let mut s = String::new();
    let mut i = 0;
    while s.len() < target {
        let g = annotated(i)
            .replace("1. e4 e5 2. Nf3 Nc6", "1. e4 $1 {The king's pawn [%clk 0:10:00]} e5 (1... c5 2. Nf3 d6 {sicilian}) 2. Nf3 $14 Nc6 {develops}")
            .replace("1. d4 Nf6 2. c4", "1. d4 {queen's pawn} Nf6 (1... d5 2. c4 $5) 2. c4");
        s.push_str(&g);
        i += 1;
    }
    s.into_bytes()
}

fn bench_read(c: &mut Criterion) {
    let plain = plain_corpus(8 << 20);
    let rich = rich_corpus(8 << 20);
    let mut group = c.benchmark_group("read_games");
    group.sample_size(10);
    for (name, data) in [("plain_8MB", &plain), ("annotated_8MB", &rich)] {
        group.throughput(Throughput::Bytes(data.len() as u64));
        group.bench_function(name, |b| {
            b.iter(|| {
                let mut n = 0usize;
                for g in PgnReader::new(
                    Cursor::new(black_box(data.as_slice())),
                    ReadOptions::default(),
                ) {
                    n += g.unwrap().game.node_count();
                }
                black_box(n)
            });
        });
    }
    group.finish();
}

fn bench_scan_tags(c: &mut Criterion) {
    let data = plain_corpus(8 << 20);
    let raws: Vec<_> = GameChunker::new(&data[..], 16 << 20)
        .map(Result::unwrap)
        .collect();
    let mut group = c.benchmark_group("scan_tags");
    group.throughput(Throughput::Bytes(data.len() as u64));
    group.bench_function("tags_only_8MB", |b| {
        b.iter(|| {
            raws.iter()
                .map(|r| black_box(r.scan_tags()).len())
                .sum::<usize>()
        });
    });
    group.bench_function("chunker_only_8MB", |b| {
        b.iter(|| {
            GameChunker::new(black_box(&data[..]), 16 << 20)
                .map(|r| r.unwrap().bytes().len())
                .sum::<usize>()
        });
    });
    group.finish();
}

fn bench_write(c: &mut Criterion) {
    let data = rich_corpus(2 << 20);
    let games: Vec<Game> = PgnReader::new(&data[..], ReadOptions::default())
        .map(|g| g.unwrap().game)
        .collect();
    let opts = WriteOptions::default();
    let bytes: usize = games
        .iter()
        .map(|g| game_to_string(g, &opts).unwrap().len())
        .sum();
    let mut group = c.benchmark_group("write_game");
    group.sample_size(10);
    group.throughput(Throughput::Bytes(bytes as u64));
    group.bench_function("default_options", |b| {
        b.iter(|| {
            games
                .iter()
                .map(|g| game_to_string(g, &opts).unwrap().len())
                .sum::<usize>()
        });
    });
    let scid = WriteOptions::scid_export();
    group.bench_function("scid_export", |b| {
        b.iter(|| {
            games
                .iter()
                .map(|g| game_to_string(g, &scid).unwrap().len())
                .sum::<usize>()
        });
    });
    group.finish();
}

fn bench_parse_san(c: &mut Criterion) {
    let tokens: Vec<&str> = LINES[0]
        .split_whitespace()
        .filter(|w| !w.ends_with('.') && !w.contains('/'))
        .collect();
    let mut group = c.benchmark_group("parse_san");
    group.throughput(Throughput::Elements(tokens.len() as u64));
    for (name, mode) in [("strict", SanMode::Strict), ("tolerant", SanMode::Tolerant)] {
        group.bench_function(name, |b| {
            b.iter_batched(
                Position::new,
                |mut pos| {
                    for t in &tokens {
                        let m = pos.parse_san(black_box(t), mode).unwrap().mv;
                        pos.play(m).unwrap();
                    }
                    pos
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_read,
    bench_scan_tags,
    bench_write,
    bench_parse_san
);
criterion_main!(benches);
