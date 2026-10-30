//! Compatibility checks against the Scid vs PC books in
//! `reference/scid_vs_pc-4.27/books/` (not versioned). Each test is `#[ignore]`
//! and also skips itself when the files are missing. Run them with
//! `cargo test -p mychess-book -- --ignored`.

use std::path::PathBuf;

use mychess_book::{Book, MergeMode, code_to_text, merge, polyglot_key};
use mychess_core::{Move, Position};

fn books_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/scid_vs_pc-4.27/books")
}

fn open(name: &str) -> Option<Book> {
    let path = books_dir().join(name);
    if !path.exists() {
        return None;
    }
    Some(Book::open_readonly(path).expect("book opens"))
}

fn play(ucis: &[&str]) -> Position {
    let mut pos = Position::new();
    for u in ucis {
        let m = pos.parse_uci(u).expect("legal");
        pos.play(m).expect("play");
    }
    pos
}

fn text_of(book: &Book, key: u64) -> Vec<(String, u16)> {
    book.entries()
        .iter()
        .filter(|e| e.key == key)
        .map(|e| (code_to_text(e.mv).unwrap_or_default(), e.weight))
        .collect()
}

#[test]
#[ignore = "needs reference/scid_vs_pc-4.27/books"]
fn compat_all_books_open_sorted_with_zero_learn() {
    for name in ["gm2600.bin", "Elo2400.bin", "Performance.bin", "varied.bin"] {
        let Some(book) = open(name) else { continue };
        let report = book.validate();
        assert_eq!(report.trailing_bytes, 0, "{name}");
        assert!(report.was_sorted, "{name}");
        assert!(book.entries().iter().all(|e| e.learn == 0), "{name}");
        assert!(!book.is_empty(), "{name}");
    }
}

#[test]
#[ignore = "needs reference/scid_vs_pc-4.27/books"]
fn compat_gm2600_start_position() {
    let Some(book) = open("gm2600.bin") else {
        return;
    };
    let start = Position::new();
    assert_eq!(polyglot_key(&start), 0x463b_9618_1691_fc9c);
    let entries = text_of(&book, 0x463b_9618_1691_fc9c);
    assert_eq!(entries.len(), 10);
    assert_eq!(entries[0], ("d2d4".to_string(), 12936));
    assert_eq!(book.moves(&start).len(), 10);
}

#[test]
#[ignore = "needs reference/scid_vs_pc-4.27/books"]
fn compat_elo2400_start_and_after_e4() {
    let Some(book) = open("Elo2400.bin") else {
        return;
    };
    let start = Position::new();
    assert_eq!(
        text_of(&book, polyglot_key(&start))[0],
        ("e2e4".to_string(), 17949)
    );
    let after = play(&["e2e4"]);
    assert_eq!(polyglot_key(&after), 0x823c_9b50_fd11_4196);
    let entries = text_of(&book, 0x823c_9b50_fd11_4196);
    assert_eq!(entries.len(), 11);
    assert_eq!(
        entries[..3],
        [
            ("c7c5".to_string(), 14841),
            ("e7e5".to_string(), 6312),
            ("e7e6".to_string(), 3921)
        ]
    );
}

#[test]
#[ignore = "needs reference/scid_vs_pc-4.27/books"]
fn compat_performance_and_varied_have_unit_weights() {
    let start = Position::new();
    for (name, count) in [("Performance.bin", 3), ("varied.bin", 6)] {
        let Some(book) = open(name) else { continue };
        let moves = book.moves(&start);
        assert_eq!(moves.len(), count, "{name}");
        assert!(moves.iter().all(|m| m.weight == 1), "{name}");
    }
}

#[test]
#[ignore = "needs reference/scid_vs_pc-4.27/books"]
fn compat_performance_promotion_entry() {
    let Some(book) = open("Performance.bin") else {
        return;
    };
    let entry = book
        .entries()
        .iter()
        .find(|e| e.key == 0x74e4_2282_fec4_2916 && e.mv == 0x4d3b)
        .expect("entry present");
    assert_eq!(entry.weight, 6);
    assert_eq!(code_to_text(0x4d3b).as_deref(), Some("e7d8q"));
}

#[test]
#[ignore = "needs reference/scid_vs_pc-4.27/books"]
fn compat_castling_is_listed_as_a_castle_move() {
    // Search every book for a position on the Italian Game main line where
    // White castles.
    let pos = play(&[
        "e2e4", "e7e5", "g1f3", "b8c6", "f1c4", "f8c5", "c2c3", "g8f6", "d2d3", "d7d6",
    ]);
    let mut seen = false;
    for name in ["gm2600.bin", "Elo2400.bin", "Performance.bin", "varied.bin"] {
        let Some(book) = open(name) else { continue };
        for m in book.moves(&pos) {
            if let Move::Castle { .. } = m.mv {
                assert_eq!(m.code, 0x0107);
                seen = true;
            }
        }
    }
    assert!(seen, "no book lists O-O in the Italian Game position");
}

#[test]
#[ignore = "needs reference/scid_vs_pc-4.27/books"]
fn compat_merge_of_real_books_stays_sorted() {
    let (Some(a), Some(b)) = (open("gm2600.bin"), open("Elo2400.bin")) else {
        return;
    };
    for mode in [MergeMode::KeepFirst, MergeMode::SumWeights] {
        let (m, report) = merge(&a, &b, mode);
        assert!(m.was_sorted());
        assert_eq!(report.written, m.len());
        assert!(m.entries().windows(2).all(|w| w[0].key <= w[1].key));
    }
}
