//! Compares the PGN reader of `mychess-pgn` (full parse: tags, move legality, game tree)
//! with the `pgn-reader` crate used as oracle (syntax only, no legality check, no tree).
//! The ratio is informational (spec `m1-3-pgn.md` D38 / `m1-5-chess960-fechamento.md`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    missing_docs,
    clippy::cast_possible_truncation,
    clippy::format_push_string
)]

use std::hint::black_box;
use std::io::Cursor;
use std::ops::ControlFlow;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use mychess_pgn::read::{PgnReader, ReadOptions};
use pgn_reader::{Reader, SanPlus, Visitor};

const LINES: &[&str] = &[
    "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 6. Re1 b5 7. Bb3 d6 8. c3 O-O 9. h3 Nb8 10. d4 Nbd7 11. c4 c6 12. cxb5 axb5 13. Nc3 Bb7 14. Bg5 b4 15. Nb1 h6 16. Bh4 c5 17. dxe5 Nxe4 18. Bxe7 Qxe7 19. exd6 Qf6 20. Nbd2 Nxd6 21. Nc4 Nxc4 22. Bxc4 Nb6 23. Ne5 Rae8 24. Bxf7+ Rxf7 25. Nxf7 Rxe1+ 26. Qxe1 Kxf7 27. Qe3 Qg5 28. Qxg5 hxg5 29. b3 Ke6 30. a3 Kd6 31. axb4 cxb4 32. Ra5 Nd5 33. f3 Bc8 34. Kf2 Bf5 35. Ra7 g6 36. Ra6+ Kc5 37. Ke1 Nf4 38. g3 Nxh3 39. Kd2 Kb5 40. Rd6 Kc5 41. Ra6 Nf2 42. g4 Bd3 43. Re6 1/2-1/2",
    "1. d4 Nf6 2. c4 e6 3. Nc3 Bb4 4. e3 O-O 5. Bd3 d5 6. Nf3 c5 7. O-O Nc6 8. a3 Bxc3 9. bxc3 dxc4 10. Bxc4 Qc7 11. Bd3 e5 12. Qc2 Re8 13. Nxe5 Nxe5 14. dxe5 Qxe5 15. f4 Qc7 16. e4 Bg4 17. Rf3 Rad8 18. Bb2 Bh5 19. Rh3 Bg6 1/2-1/2",
];

fn corpus(target: usize) -> Vec<u8> {
    let mut s = String::new();
    let mut i = 0usize;
    while s.len() < target {
        s.push_str(&format!(
            "[Event \"Bench {i}\"]\n[Site \"Rust\"]\n[Date \"2024.01.{:02}\"]\n[Round \"{}\"]\n[White \"Player, A{i}\"]\n[Black \"Player, B{i}\"]\n[Result \"*\"]\n[WhiteElo \"2500\"]\n[BlackElo \"2400\"]\n[ECO \"C80\"]\n\n{}\n\n",
            1 + i % 28,
            i % 9,
            LINES[i % LINES.len()]
        ));
        i += 1;
    }
    s.into_bytes()
}

struct CountMoves;

impl Visitor for CountMoves {
    type Tags = ();
    type Movetext = usize;
    type Output = usize;

    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        ControlFlow::Continue(())
    }

    fn begin_movetext(&mut self, _tags: Self::Tags) -> ControlFlow<Self::Output, Self::Movetext> {
        ControlFlow::Continue(0)
    }

    fn san(&mut self, movetext: &mut Self::Movetext, _san: SanPlus) -> ControlFlow<Self::Output> {
        *movetext += 1;
        ControlFlow::Continue(())
    }

    fn end_game(&mut self, movetext: Self::Movetext) -> Self::Output {
        movetext
    }
}

fn bench_oracle(c: &mut Criterion) {
    let data = corpus(8 << 20);
    let mut group = c.benchmark_group("read_vs_oracle");
    group.sample_size(10);
    group.throughput(Throughput::Bytes(data.len() as u64));
    group.bench_function("mychess_pgn_full_parse", |b| {
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
    group.bench_function("pgn_reader_syntax_only", |b| {
        b.iter(|| {
            let mut reader = Reader::new(Cursor::new(black_box(data.as_slice())));
            let mut n = 0usize;
            while let Some(moves) = reader.read_game(&mut CountMoves).unwrap() {
                n += moves;
            }
            black_box(n)
        });
    });
    group.finish();
}

criterion_group!(benches, bench_oracle);
criterion_main!(benches);
