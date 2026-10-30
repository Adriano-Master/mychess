//! Benchmarks of the ECO book: loading `scid.eco`, classifying one game and
//! classifying a batch of random legal games (spec `m1-4-epd-eco.md`, D53).

#![allow(clippy::print_stderr, clippy::single_match_else)]

use std::hint::black_box;
use std::path::Path;

use criterion::{Criterion, criterion_group, criterion_main};
use mychess_core::{ClassifyOptions, EcoBook, Game, Move, Position, SanMode};

/// Small deterministic generator (no `rand` dependency, reproducible runs).
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

fn random_game(rng: &mut Lcg, plies: usize) -> Vec<Move> {
    let mut pos = Position::new();
    let mut out = Vec::with_capacity(plies);
    for _ in 0..plies {
        let moves = pos.legal_moves();
        if moves.is_empty() {
            break;
        }
        let idx = usize::try_from(rng.next()).unwrap_or(0) % moves.len();
        let mv = moves[idx];
        if pos.play(mv).is_err() {
            break;
        }
        out.push(mv);
    }
    out
}

fn to_game(moves: &[Move]) -> Game {
    let mut g = Game::new();
    let mut node = g.root();
    for &mv in moves {
        node = g.add_move(node, mv).expect("legal move");
    }
    g
}

fn load_scid_eco() -> Option<String> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/scid_vs_pc-4.27/scid.eco");
    match std::fs::read(&path) {
        Ok(bytes) => String::from_utf8(bytes).ok(),
        Err(_) => {
            eprintln!(
                "skipping ECO benchmarks that need {}: not found",
                path.display()
            );
            None
        }
    }
}

fn bench_load(c: &mut Criterion) {
    let Some(text) = load_scid_eco() else { return };
    c.bench_function("eco/load_scid_eco", |b| {
        b.iter(|| EcoBook::parse(black_box(&text)).expect("scid.eco parses"));
    });
}

fn bench_classify(c: &mut Criterion) {
    let Some(text) = load_scid_eco() else { return };
    let book = EcoBook::parse(&text).expect("scid.eco parses");

    // An 80-ply game that starts with a real Ruy Lopez and wanders on.
    let mut pos = Position::new();
    let mut moves = Vec::new();
    for san in "e4 e5 Nf3 Nc6 Bb5 a6 Ba4 Nf6 O-O Be7 Re1 b5 Bb3 d6 c3 O-O h3".split_whitespace() {
        let mv = pos.parse_san(san, SanMode::Strict).expect("san").mv;
        pos.play(mv).expect("play");
        moves.push(mv);
    }
    let mut rng = Lcg(7);
    while moves.len() < 80 {
        let legal = pos.legal_moves();
        if legal.is_empty() {
            break;
        }
        let mv = legal[usize::try_from(rng.next()).unwrap_or(0) % legal.len()];
        pos.play(mv).expect("play");
        moves.push(mv);
    }
    let game = to_game(&moves);
    c.bench_function("eco/classify_80_plies", |b| {
        b.iter(|| book.classify(black_box(&game), ClassifyOptions::game()));
    });

    // Batch: 10 000 random legal games, classified in "database" mode.
    let mut rng = Lcg(42);
    let games: Vec<Game> = (0..10_000)
        .map(|_| {
            let len = 20 + usize::try_from(rng.next() % 80).unwrap_or(0);
            to_game(&random_game(&mut rng, len))
        })
        .collect();
    let mut group = c.benchmark_group("eco");
    group.sample_size(20);
    group.bench_function("classify_batch_10000_random_games", |b| {
        b.iter(|| {
            let mut hits = 0usize;
            for g in &games {
                if book
                    .classify(black_box(g), ClassifyOptions::batch())
                    .is_some()
                {
                    hits += 1;
                }
            }
            hits
        });
    });
    group.finish();
}

criterion_group!(benches, bench_load, bench_classify);
criterion_main!(benches);
