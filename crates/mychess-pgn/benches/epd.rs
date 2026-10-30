//! EPD benchmarks: `epd_read` and `epd_write` over synthetic lines (spec
//! `m1-5-chess960-fechamento.md`, "Benchmarks criterion pendentes"). The spec asks for
//! 1 million lines; `RECORDS` is 200 000 to keep a run short (the cost is linear).
//! Positions are distinct (each record is a random legal playout).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use mychess_core::Position;
use mychess_pgn::epd::{EpdBook, EpdOperation, EpdOptions};

const RECORDS: usize = 200_000;

/// Small deterministic generator (reproducible runs, no `rand`).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

/// Builds `RECORDS` distinct positions with `bm`, `id` and `ce` operations. Games are
/// played from the start position; each game contributes all its positions.
fn build_book() -> EpdBook {
    let mut rng = Lcg(0x5EED_1234_ABCD_9876);
    let mut book = EpdBook::new();
    while book.len() < RECORDS {
        let mut pos = Position::new();
        for _ in 0..60 {
            let moves = pos.legal_moves();
            if moves.is_empty() {
                break;
            }
            let m = moves[usize::try_from(rng.next()).unwrap() % moves.len()];
            let ops = vec![
                EpdOperation::new("bm", pos.san(m).unwrap()),
                EpdOperation::new("id", format!("\"synthetic {}\"", book.len())),
                EpdOperation::new("ce", "25"),
            ];
            book.insert(pos.clone(), ops);
            pos = pos.after(m).unwrap();
            if book.len() >= RECORDS {
                break;
            }
        }
    }
    book
}

fn bench_epd(c: &mut Criterion) {
    let book = build_book();
    let options = EpdOptions::default();
    let mut text = Vec::new();
    book.write(&mut text, options).unwrap();

    let mut group = c.benchmark_group("epd");
    group.sample_size(10);
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.bench_function("epd_read", |b| {
        b.iter(|| {
            let load = EpdBook::read(black_box(text.as_slice()), options).unwrap();
            black_box(load.book.len())
        });
    });
    group.bench_function("epd_write", |b| {
        let mut sink = Vec::with_capacity(text.len());
        b.iter(|| {
            sink.clear();
            black_box(&book).write(&mut sink, options).unwrap();
            black_box(sink.len())
        });
    });
    group.finish();
}

criterion_group!(benches, bench_epd);
criterion_main!(benches);
