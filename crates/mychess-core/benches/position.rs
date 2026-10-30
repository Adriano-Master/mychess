//! Position-level benchmarks: `from_fen`, `fen`, `legal_moves`, `zobrist` and
//! `parse_san` (strict and tolerant) over a corpus of real moves.
//!
//! Spec: `docs/specs/m1-5-chess960-fechamento.md` ("Benchmarks criterion pendentes").
//! Baselines go to `docs/benchmarks.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::hint::black_box;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use mychess_core::{Position, SanMode};

/// Opening lines of well-known games (movetext only); legality is asserted when the corpus is built.
const GAMES: &[&str] = &[
    "e4 e5 Nf3 Nc6 Bb5 a6 Ba4 Nf6 O-O Be7 Re1 b5 Bb3 d6 c3 O-O h3 Nb8 d4 Nbd7 c4 c6 cxb5 axb5 Nc3 Bb7 Bg5 b4 Nb1 h6 Bh4 c5 dxe5 Nxe4 Bxe7 Qxe7 exd6 Qf6 Nbd2 Nxd6 Nc4 Nxc4 Bxc4 Nb6 Ne5 Rae8 Bxf7+ Rxf7 Nxf7 Rxe1+ Qxe1 Kxf7 Qe3 Qg5 Qxg5 hxg5 b3 Ke6 a3 Kd6 axb4 cxb4 Ra5 Nd5 f3 Bc8 Kf2 Bf5 Ra7 g6 Ra6+ Kc5 Ke1 Nf4 g3 Nxh3 Kd2 Kb5 Rd6 Kc5 Ra6 Nf2 g4 Bd3 Re6",
    "d4 Nf6 c4 e6 Nc3 Bb4 e3 O-O Bd3 d5 Nf3 c5 O-O Nc6 a3 Bxc3 bxc3 dxc4 Bxc4 Qc7 Bd3 e5 Qc2 Re8 Nxe5 Nxe5 dxe5 Qxe5 f4 Qc7 e4 Bg4 Rf3 Rad8 Bb2 Bh5 Rh3 Bg6",
    "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3 e5 Nb3 Be6 f3 Be7 Qd2 O-O O-O-O Nbd7 g4 b5 g5 b4 Ne2 Ne8 f4 a5 f5 a4",
    "c4 e5 Nc3 Nf6 Nf3 Nc6 g3 d5 cxd5 Nxd5 Bg2 Nb6 O-O Be7 a3 O-O b4 Be6 d3 Qd7 Bb2 f6 Rc1 Rfd8 Nd2 Bf7 Nc4 Nxc4 dxc4 Bxc4 Nb5 Bxb5 Qb3",
    "e4 e6 d4 d5 Nc3 Bb4 e5 c5 a3 Bxc3+ bxc3 Ne7 Qg4 Qc7 Qxg7 Rg8 Qxh7 cxd4 Kd1 Bd7 Qd3 Nbc6 Nf3 O-O-O",
];

fn positions_and_sans() -> Vec<(Position, String)> {
    let mut out = Vec::new();
    for game in GAMES {
        let mut pos = Position::new();
        for san in game.split_whitespace() {
            let mv = pos
                .parse_san(san, SanMode::Strict)
                .expect("corpus is legal")
                .mv;
            out.push((pos.clone(), san.to_owned()));
            pos = pos.after(mv).unwrap();
        }
    }
    out
}

fn positions() -> Vec<Position> {
    let mut v: Vec<Position> = positions_and_sans().into_iter().map(|(p, _)| p).collect();
    v.push(Position::chess960_start(518).unwrap());
    v.push(Position::chess960_start(959).unwrap());
    v
}

fn bench_fen(c: &mut Criterion) {
    let positions = positions();
    let fens: Vec<String> = positions.iter().map(Position::fen).collect();
    let n = positions.len() as u64;
    let mut group = c.benchmark_group("position");
    group.throughput(Throughput::Elements(n));
    group.bench_function("from_fen", |b| {
        b.iter(|| {
            for f in &fens {
                black_box(Position::from_fen(black_box(f)).unwrap());
            }
        });
    });
    group.bench_function("fen", |b| {
        b.iter(|| {
            for p in &positions {
                black_box(black_box(p).fen());
            }
        });
    });
    group.bench_function("legal_moves", |b| {
        b.iter(|| {
            for p in &positions {
                black_box(black_box(p).legal_moves());
            }
        });
    });
    group.bench_function("zobrist", |b| {
        b.iter(|| {
            for p in &positions {
                black_box(black_box(p).zobrist());
            }
        });
    });
    group.finish();
}

fn bench_parse_san(c: &mut Criterion) {
    let pairs = positions_and_sans();
    let mut group = c.benchmark_group("core_parse_san");
    group.throughput(Throughput::Elements(pairs.len() as u64));
    for (name, mode) in [("strict", SanMode::Strict), ("tolerant", SanMode::Tolerant)] {
        group.bench_function(name, |b| {
            b.iter_batched(
                || (),
                |()| {
                    for (pos, san) in &pairs {
                        black_box(pos.parse_san(black_box(san), mode).unwrap().mv);
                    }
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, bench_fen, bench_parse_san);
criterion_main!(benches);
