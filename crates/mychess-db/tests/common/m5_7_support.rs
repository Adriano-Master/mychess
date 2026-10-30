//! Helpers of the M5-7a contract tests (ECO statistics, classification,
//! opening identification and novelty). Games are built from SAN text and
//! written with `Si4Writer`.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::missing_panics_doc
)]

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use mychess_core::{Eco, EcoBook, Game, GameResult, PartialDate, Position, SanMode};
use mychess_db::si4::{FlagOp, GameFlags, Si4Database, Si4Writer};

pub fn eco(text: &str) -> Eco {
    text.parse().unwrap()
}

pub fn code(text: &str) -> u16 {
    eco(text).to_scid_code()
}

pub fn date(y: u16, m: u8, d: u8) -> PartialDate {
    PartialDate::new(Some(y), Some(m), Some(d)).unwrap()
}

/// A game from SAN moves played from the standard start.
pub fn san_game(sans: &[&str]) -> Game {
    let mut game = Game::new();
    let mut node = game.root();
    for san in sans {
        let position = game.position_at(node).unwrap().clone();
        let parsed = position.parse_san(san, SanMode::Strict).unwrap();
        node = game.add_move(node, parsed.mv).unwrap();
    }
    game
}

/// A game to be written: moves plus the index fields the tests care about.
#[derive(Clone)]
pub struct Spec {
    pub game: Game,
    pub eco: Option<Eco>,
    pub result: GameResult,
    pub date: Option<PartialDate>,
    pub deleted: bool,
}

pub fn spec(sans: &[&str]) -> Spec {
    Spec {
        game: san_game(sans),
        eco: None,
        result: GameResult::Unknown,
        date: None,
        deleted: false,
    }
}

impl Spec {
    pub fn eco(mut self, text: &str) -> Spec {
        self.eco = Some(eco(text));
        self
    }
    pub fn result(mut self, result: GameResult) -> Spec {
        self.result = result;
        self
    }
    pub fn date(mut self, d: PartialDate) -> Spec {
        self.date = Some(d);
        self
    }
    pub fn deleted(mut self) -> Spec {
        self.deleted = true;
        self
    }
}

/// Writes `specs` to a new database in `dir`; returns the base path.
pub fn write_db(dir: &Path, specs: &[Spec]) -> PathBuf {
    let base = dir.join("db");
    let mut writer = Si4Writer::create(&base, 1).unwrap();
    for (i, s) in specs.iter().enumerate() {
        let mut game = s.game.clone();
        game.tags_mut().result = s.result;
        game.tags_mut().eco = s.eco;
        if let Some(d) = s.date {
            game.tags_mut().date = d;
        }
        let n = writer.add_game(&game, GameFlags::empty()).unwrap();
        assert_eq!(usize::try_from(n).unwrap(), i + 1);
        if s.deleted {
            writer.set_flag(n, GameFlags::DELETE, FlagOp::Set).unwrap();
        }
    }
    base
}

pub fn open(base: &Path) -> Si4Database {
    Si4Database::open(base).unwrap()
}

/// The raw ECO code of every entry.
pub fn codes(base: &Path) -> Vec<u16> {
    let db = open(base);
    (1..=db.len())
        .map(|n| db.entry(n).unwrap().eco_code)
        .collect()
}

pub fn index_bytes(base: &Path) -> Vec<u8> {
    std::fs::read(base.with_extension("si4")).unwrap()
}

/// Header size and entry size of the `.si4` (docs/formatos/si4.md).
pub const HEADER_LEN: usize = 182;
pub const ENTRY_LEN: usize = 47;
/// Offset of the two ECO bytes in the file for game `n` (1-based).
pub fn eco_offset(n: usize) -> usize {
    HEADER_LEN + ENTRY_LEN * (n - 1) + 23
}

/// A small book with a start entry, transposable lines and extension codes.
pub const BOOK_TEXT: &str = "\
A00a \"Start\" *
C20 \"Kings pawn\" 1.e4 e5 *
C40a \"Kings knight\" 1.e4 e5 2.Nf3 *
C44b \"Scotch-ish\" 1.e4 e5 2.Nf3 Nc6 3.d4 *
B01 \"Scandinavian\" 1.e4 d5 *
A45 \"Indian\" 1.d4 Nf6 *
";

pub fn book() -> EcoBook {
    EcoBook::parse(BOOK_TEXT).unwrap()
}

pub fn pos_of(fen: &str) -> Position {
    Position::from_fen(fen).unwrap()
}

/// A deterministic non-repeating line of `plies` half-moves.
pub fn long_line(plies: usize) -> Vec<String> {
    for seed in 1u64.. {
        let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut pos = Position::new();
        let mut seen = HashSet::new();
        let mut line = Vec::new();
        'walk: while line.len() < plies {
            let moves = pos.legal_moves();
            if moves.is_empty() {
                break;
            }
            for _ in 0..8 {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                let pick = (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as usize % moves.len();
                let mv = moves[pick];
                let san = pos.san(mv).unwrap();
                let mut next = pos.clone();
                next.play(mv).unwrap();
                if seen.insert(next.fen().split(' ').take(2).collect::<Vec<_>>().join(" ")) {
                    line.push(san);
                    pos = next;
                    continue 'walk;
                }
            }
            break;
        }
        if line.len() == plies {
            return line;
        }
    }
    unreachable!()
}

pub fn numbered(line: &[String]) -> String {
    let mut out = String::new();
    for (i, san) in line.iter().enumerate() {
        if i % 2 == 0 {
            write!(out, "{}.", i / 2 + 1).unwrap();
        }
        out.push_str(san);
        out.push(' ');
    }
    out
}

pub fn deep_book(line: &[String]) -> EcoBook {
    EcoBook::parse(&format!(
        "A00a \"Start\" *\nC20 \"m40\" {} *\nC30 \"m60\" {} *\nC31 \"m61\" {} *\n",
        numbered(&line[..40]),
        numbered(&line[..60]),
        numbered(&line[..61]),
    ))
    .unwrap()
}
