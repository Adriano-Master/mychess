//! Building a book from games (spec D1112).
//!
//! Scid delegates to the external `polyglot make-book`; the formulas here are
//! those of `src/polyglot/book_make.cpp`.

use std::collections::HashMap;

use mychess_core::{Color, GameResult, Move, Position};

use crate::book::Book;
use crate::codec::encode_move;
use crate::error::BookError;
use crate::format::Entry;
use crate::zobrist::polyglot_key;

/// `n` reaching this value halves the statistics of the position.
const COUNT_MAX: u32 = 16384;

/// The options of `polyglot make-book`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuildOptions {
    /// Only the first `max_ply` plies of each game are used (default 1024).
    pub max_ply: u32,
    /// Moves seen in fewer games are dropped (default 3).
    pub min_game: u32,
    /// Minimum expected score in percent, `0..=100` (default 0).
    pub min_score: f64,
    /// Keep only the moves of White (drops Black's).
    pub only_white: bool,
    /// Keep only the moves of Black (drops White's). Setting both flags
    /// drops everything.
    pub only_black: bool,
    /// Every kept move gets weight 1.
    pub uniform: bool,
}

impl Default for BuildOptions {
    fn default() -> BuildOptions {
        BuildOptions {
            max_ply: 1024,
            min_game: 3,
            min_score: 0.0,
            only_white: false,
            only_black: false,
            uniform: false,
        }
    }
}

/// What a build did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BuildReport {
    /// Games offered to the builder.
    pub games_seen: u64,
    /// Games counted in the statistics.
    pub games_used: u64,
    /// Games dropped because a move was illegal.
    pub games_illegal: u64,
    /// Games dropped because they do not start from the standard position.
    pub games_non_standard_start: u64,
    /// Distinct (position, move) pairs before filtering.
    pub entries_before_filter: usize,
    /// Entries in the book.
    pub entries_kept: usize,
}

#[derive(Debug)]
struct Acc {
    key: u64,
    mv: u16,
    n: u32,
    sum: u32,
    color: Color,
}

/// Accumulates games and produces a [`Book`].
#[derive(Debug, Default)]
pub struct BookBuilder {
    options: BuildOptions,
    accs: Vec<Acc>,
    by_key: HashMap<u64, Vec<usize>>,
    report: BuildReport,
}

impl BookBuilder {
    /// A builder with `options`.
    pub fn new(options: BuildOptions) -> BookBuilder {
        BookBuilder {
            options,
            ..BookBuilder::default()
        }
    }

    /// Adds a game played from the standard start position. A game with an
    /// illegal move is dropped whole, counted in the report, and reported as
    /// [`BookError::IllegalMove`]. The result `*` counts as a draw (Scid).
    pub fn add_game(
        &mut self,
        moves: impl IntoIterator<Item = Move>,
        result: GameResult,
    ) -> Result<(), BookError> {
        self.report.games_seen += 1;
        // Stage the game so a late illegal move leaves no trace.
        let mut staged: Vec<(u64, u16, Color, i32)> = Vec::new();
        let mut pos = Position::new();
        // Result from the point of view of the side to move: +1 win.
        let mut score = match result {
            GameResult::White => 1,
            GameResult::Black => -1,
            _ => 0,
        };
        let limit = usize::try_from(self.options.max_ply).unwrap_or(usize::MAX);
        for (ply, m) in moves.into_iter().take(limit).enumerate() {
            let code = encode_move(m).filter(|_| pos.is_legal(m));
            let Some(code) = code else {
                self.report.games_illegal += 1;
                return Err(BookError::IllegalMove { ply });
            };
            staged.push((polyglot_key(&pos), code, pos.turn(), score));
            if pos.play(m).is_err() {
                self.report.games_illegal += 1;
                return Err(BookError::IllegalMove { ply });
            }
            score = -score;
        }
        for (key, code, color, score) in staged {
            self.record(key, code, color, score);
        }
        self.report.games_used += 1;
        Ok(())
    }

    /// Like [`BookBuilder::add_game`], but for a game with its own start
    /// position: anything but the standard start is dropped and counted.
    pub fn add_game_from(
        &mut self,
        start: &Position,
        moves: impl IntoIterator<Item = Move>,
        result: GameResult,
    ) -> Result<(), BookError> {
        if *start == Position::new() {
            return self.add_game(moves, result);
        }
        self.report.games_seen += 1;
        self.report.games_non_standard_start += 1;
        Ok(())
    }

    fn record(&mut self, key: u64, mv: u16, color: Color, score: i32) {
        let ids = self.by_key.entry(key).or_default();
        let found = ids
            .iter()
            .copied()
            .find(|i| self.accs.get(*i).is_some_and(|a| a.mv == mv));
        let id = if let Some(id) = found {
            id
        } else {
            self.accs.push(Acc {
                key,
                mv,
                n: 0,
                sum: 0,
                color,
            });
            let id = self.accs.len() - 1;
            ids.push(id);
            id
        };
        let mut halve = false;
        if let Some(acc) = self.accs.get_mut(id) {
            acc.n += 1;
            acc.sum += u32::try_from(score + 1).unwrap_or(0);
            halve = acc.n >= COUNT_MAX;
        }
        if halve {
            for i in ids.iter() {
                if let Some(acc) = self.accs.get_mut(*i) {
                    acc.n = acc.n.div_ceil(2);
                    acc.sum = acc.sum.div_ceil(2);
                }
            }
        }
    }

    /// Filters, sorts and returns the book with the report.
    pub fn finish(mut self) -> (Book, BuildReport) {
        let o = self.options;
        let min_score = if o.min_score.is_nan() {
            0.0
        } else {
            o.min_score.clamp(0.0, 100.0) / 100.0
        };
        self.report.entries_before_filter = self.accs.len();
        let mut kept: Vec<Entry> = self
            .accs
            .iter()
            .filter(|a| a.n >= o.min_game && a.sum > 0)
            .filter(|a| f64::from(a.sum) / f64::from(a.n) / 2.0 >= min_score)
            .filter(|a| {
                !((o.only_black && a.color == Color::White)
                    || (o.only_white && a.color == Color::Black))
            })
            .map(|a| Entry {
                key: a.key,
                mv: a.mv,
                weight: if o.uniform {
                    1
                } else {
                    u16::try_from(a.sum).unwrap_or(u16::MAX)
                },
                learn: 0,
            })
            .filter(|e| e.weight != 0)
            .collect();
        kept.sort_by(|a, b| a.key.cmp(&b.key).then(b.weight.cmp(&a.weight)));
        self.report.entries_kept = kept.len();
        (Book::from_entries(kept), self.report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(ucis: &[&str]) -> Vec<Move> {
        let mut pos = Position::new();
        ucis.iter()
            .map(|u| {
                let m = pos.parse_uci(u).expect("legal");
                pos.play(m).expect("play");
                m
            })
            .collect()
    }

    fn opts() -> BuildOptions {
        BuildOptions {
            min_game: 1,
            ..BuildOptions::default()
        }
    }

    #[test]
    fn weights_follow_the_result_of_the_side_to_move() {
        let mut b = BookBuilder::new(opts());
        b.add_game(line(&["e2e4", "e7e5"]), GameResult::White)
            .expect("game");
        b.add_game(line(&["e2e4", "c7c5"]), GameResult::Draw)
            .expect("game");
        b.add_game(line(&["d2d4"]), GameResult::Black)
            .expect("game");
        let (book, report) = b.finish();
        let start = Position::new();
        let moves = book.moves(&start);
        // e4: win (2) + draw (1) = 3; d4: loss = 0 -> dropped.
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].weight, 3);
        let after = start
            .after(start.parse_uci("e2e4").expect("legal"))
            .expect("play");
        // Black: e5 lost (0, dropped); c5 draw (1).
        assert_eq!(book.moves(&after).len(), 1);
        assert_eq!(book.moves(&after)[0].weight, 1);
        assert_eq!(report.games_used, 3);
        assert!(book.entries().iter().all(|e| e.learn == 0));
        assert!(book.was_sorted());
    }

    #[test]
    fn filters_and_uniform() {
        let make = |o: BuildOptions| {
            let mut b = BookBuilder::new(o);
            for _ in 0..3 {
                b.add_game(line(&["e2e4", "e7e5"]), GameResult::White)
                    .expect("game");
            }
            b.add_game(line(&["e2e4", "e7e5"]), GameResult::Draw)
                .expect("game");
            b.finish().0
        };
        assert_eq!(make(BuildOptions::default()).len(), 2);
        assert_eq!(
            make(BuildOptions {
                only_white: true,
                min_game: 1,
                ..BuildOptions::default()
            })
            .len(),
            1
        );
        assert_eq!(
            make(BuildOptions {
                only_black: true,
                min_game: 1,
                ..BuildOptions::default()
            })
            .len(),
            1
        );
        assert_eq!(
            make(BuildOptions {
                min_game: 5,
                ..BuildOptions::default()
            })
            .len(),
            0
        );
        let uniform = make(BuildOptions {
            uniform: true,
            min_game: 1,
            ..BuildOptions::default()
        });
        assert!(uniform.entries().iter().all(|e| e.weight == 1));
        let strict = make(BuildOptions {
            min_score: 60.0,
            min_game: 1,
            ..BuildOptions::default()
        });
        assert_eq!(strict.len(), 1); // e4: 7/4/2 = 0.875; e5: 1/4/2 = 0.125
    }

    #[test]
    fn illegal_game_is_dropped_and_counted() {
        let mut b = BookBuilder::new(opts());
        let mut bad = line(&["e2e4"]);
        bad.push(bad[0]);
        assert!(b.add_game(bad, GameResult::White).is_err());
        b.add_game_from(
            &Position::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").expect("fen"),
            [],
            GameResult::Draw,
        )
        .expect("counted");
        let (book, report) = b.finish();
        assert!(book.is_empty());
        assert_eq!(
            (
                report.games_seen,
                report.games_illegal,
                report.games_non_standard_start
            ),
            (2, 1, 1)
        );
    }

    #[test]
    fn statistics_are_halved_at_16384() {
        let mut b = BookBuilder::new(opts());
        let e4 = line(&["e2e4"]);
        for _ in 0..16384 {
            b.add_game(e4.clone(), GameResult::White).expect("game");
        }
        // n hits 16384 -> (16384+1)/2 = 8192; sum 32768 -> 16384.
        let (book, _) = b.finish();
        assert_eq!(book.entries()[0].weight, 16384);
    }

    #[test]
    fn max_ply_truncates_games() {
        let mut b = BookBuilder::new(BuildOptions {
            max_ply: 1,
            min_game: 1,
            ..BuildOptions::default()
        });
        b.add_game(line(&["e2e4", "e7e5"]), GameResult::White)
            .expect("game");
        assert_eq!(b.finish().0.len(), 1);
    }
}
