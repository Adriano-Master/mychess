//! Cross-slice integration: a `Game` read from PGN feeds an `EpdBook`, which is
//! written, read back and queried with `deepest_match`.
//!
//! Spec: `docs/specs/m1-5-chess960-fechamento.md` ("Testes de integração entre as
//! fatias", `epd_from_pgn.rs`) and `m1-4-epd-eco.md` part A (`deepest_match`, `bm`, `id`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use mychess_core::{Game, Move, Position, SanMode};
use mychess_pgn::epd::{EpdBook, EpdOperation, EpdOptions};
use mychess_pgn::read::{PgnReader, ReadOptions};

const RUY: &str = "[Event \"Test\"]\n[Site \"?\"]\n[Date \"2024.01.01\"]\n[Round \"1\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\n1. e4 e5 (1... c5 2. Nf3) 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 1-0\n";

const TRANSPOSED: &str =
    "[Event \"Transposed\"]\n[Result \"*\"]\n\n1. Nf3 Nc6 2. e4 e5 3. Bb5 a6 *\n";

fn read_game(text: &str) -> Game {
    let mut it = PgnReader::new(text.as_bytes(), ReadOptions::default());
    it.next().expect("one game").expect("in-memory read").game
}

/// Positions of the main line (ply 0 first) and the move that leaves each of them.
fn mainline_positions(game: &Game) -> Vec<(Position, Option<Move>)> {
    let mut out = Vec::new();
    let mut ids = vec![game.root()];
    ids.extend(game.mainline());
    for (i, id) in ids.iter().enumerate() {
        let next = ids.get(i + 1).map(|n| game.mv(*n).unwrap());
        out.push((game.position_at(*id).unwrap().clone(), next));
    }
    out
}

fn book_up_to(game: &Game, last_ply: usize) -> EpdBook {
    let mut book = EpdBook::new();
    for (ply, (pos, next)) in mainline_positions(game).into_iter().enumerate() {
        if ply > last_ply {
            break;
        }
        let mut ops = vec![EpdOperation::new("id", format!("\"pos {ply}\""))];
        if let Some(mv) = next {
            ops.insert(0, EpdOperation::new("bm", pos.san(mv).unwrap()));
        }
        book.insert(pos, ops);
    }
    book
}

fn roundtrip(book: &EpdBook, options: EpdOptions) -> EpdBook {
    let mut bytes = Vec::new();
    book.write(&mut bytes, options).unwrap();
    let load = EpdBook::read(bytes.as_slice(), options).unwrap();
    assert!(
        load.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        load.diagnostics
    );
    load.book
}

#[test]
fn deepest_match_returns_the_last_ply_stored_after_write_and_read() {
    let game = read_game(RUY);
    let plies = game.mainline().count();
    assert_eq!(plies, 10);
    for last in [0usize, 1, 4, 7, plies] {
        let book = roundtrip(&book_up_to(&game, last), EpdOptions::default());
        assert_eq!(book.len(), last + 1, "last = {last}");
        assert_eq!(
            book.deepest_match(&game),
            Some(u32::try_from(last).unwrap()),
            "last = {last}"
        );
    }
}

#[test]
fn bm_and_id_survive_the_roundtrip_and_bm_resolves_to_the_mainline_move() {
    let game = read_game(RUY);
    let book = roundtrip(&book_up_to(&game, 10), EpdOptions::default());
    for (ply, (pos, next)) in mainline_positions(&game).into_iter().enumerate() {
        let rec = book.get(&pos).expect("position in the book");
        assert_eq!(rec.id(), Some(format!("pos {ply}").as_str()));
        match next {
            Some(mv) => assert_eq!(rec.bm().unwrap(), vec![mv], "ply {ply}"),
            None => assert_eq!(rec.bm().unwrap(), Vec::<Move>::new()),
        }
    }
}

#[test]
fn roundtrip_works_in_the_scid_dialect_too() {
    let game = read_game(RUY);
    let book = roundtrip(&book_up_to(&game, 6), EpdOptions::scid());
    assert_eq!(book.deepest_match(&game), Some(6));
}

#[test]
fn deepest_match_ignores_variations() {
    let game = read_game(RUY);
    // Book holds only the position after 1.e4 c5 2.Nf3 (inside the variation).
    let mut p = Position::new();
    for san in ["e4", "c5", "Nf3"] {
        let mv = p.parse_san(san, SanMode::Strict).unwrap().mv;
        p = p.after(mv).unwrap();
    }
    let mut book = EpdBook::new();
    book.append_position(&p);
    let book = roundtrip(&book, EpdOptions::default());
    assert_eq!(book.deepest_match(&game), None);
}

#[test]
fn deepest_match_follows_transpositions() {
    let game = read_game(RUY);
    let book = roundtrip(&book_up_to(&game, 6), EpdOptions::default());
    // 1.Nf3 Nc6 2.e4 e5 reaches the position of ply 4 of the Ruy Lopez; 3.Bb5 a6 reaches ply 6.
    let other = read_game(TRANSPOSED);
    assert_eq!(book.deepest_match(&other), Some(6));
}

#[test]
fn empty_book_and_empty_game_give_none_or_ply_zero() {
    let game = read_game(RUY);
    assert_eq!(EpdBook::new().deepest_match(&game), None);
    let empty = read_game("[Event \"x\"]\n[Result \"*\"]\n\n*\n");
    let book = roundtrip(&book_up_to(&game, 0), EpdOptions::default());
    assert_eq!(book.deepest_match(&empty), Some(0));
}

#[test]
fn written_epd_is_stable_when_read_and_written_again() {
    let game = read_game(RUY);
    let book = book_up_to(&game, 10);
    let mut first = Vec::new();
    book.write(&mut first, EpdOptions::default()).unwrap();
    let again = EpdBook::read(first.as_slice(), EpdOptions::default())
        .unwrap()
        .book;
    let mut second = Vec::new();
    again.write(&mut second, EpdOptions::default()).unwrap();
    assert_eq!(first, second);
    assert!(first.ends_with(b"\n"));
    assert!(!first.contains(&b'\r'));
}

#[test]
fn write_file_and_read_file_roundtrip_through_the_filesystem() {
    let game = read_game(RUY);
    let book = book_up_to(&game, 8);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("book.epd");
    book.write_file(&path, EpdOptions::default()).unwrap();
    let load = EpdBook::read_file(&path, EpdOptions::default()).unwrap();
    assert_eq!(load.book.deepest_match(&game), Some(8));
    assert_eq!(load.book.len(), 9);
}
