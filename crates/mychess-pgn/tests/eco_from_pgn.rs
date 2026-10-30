//! Cross-slice integration: PGN games are classified with an ECO book, the code is
//! stored in `Tags::eco`, written back to PGN and read again.
//!
//! Spec: `docs/specs/m1-5-chess960-fechamento.md` ("Testes de integração entre as
//! fatias", `eco_from_pgn.rs`, and the `compat_*` list). Tests without the `compat_`
//! prefix use a tiny inline book; the ones with it read `scid.eco` from `reference/`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use mychess_core::{ClassifyOptions, Eco, EcoBook, Game, SanMode, Tags};
use mychess_pgn::eco_file::read_eco_file;
use mychess_pgn::read::{PgnReader, ReadOptions, Severity};
use mychess_pgn::write::{WriteOptions, game_to_string};

/// Synthetic mini book (not Scid data): three lines, one of them reached by transposition.
const MINI_BOOK: &str = "\
C60 \"Ruy Lopez\" 1.e4 e5 2.Nf3 Nc6 3.Bb5 *
C68 \"Ruy Lopez: Exchange\" 1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 4.Bxc6 *
B90 \"Sicilian: Najdorf\" 1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4 Nf6 5.Nc3 a6 *
E20 \"Nimzo-Indian\" 1.d4 Nf6 2.c4 e6 3.Nc3 Bb4 *
";

const RUY: &str = "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 *";
const NAJDORF: &str = "1. e4 c5 2. Nf3 d6 3. d4 cxd4 4. Nxd4 Nf6 5. Nc3 a6 6. Be3 e5 *";
const NIMZO_TRANSPOSED: &str = "1. c4 e6 2. Nc3 Nf6 3. d4 Bb4 4. e3 O-O *";
const NO_BOOK_LINE: &str = "1. g4 d5 2. Bg2 Nf6 *";

fn pgn_of(moves: &str) -> String {
    format!("[Event \"Test\"]\n[Result \"*\"]\n\n{moves}\n")
}

fn read_games(text: &str) -> Vec<Game> {
    PgnReader::new(text.as_bytes(), ReadOptions::default())
        .map(|r| {
            let g = r.expect("in-memory read");
            assert!(
                g.diagnostics.iter().all(|d| d.severity <= Severity::Info),
                "{:?}",
                g.diagnostics
            );
            g.game
        })
        .collect()
}

/// Reads, classifies, stores the code, writes and reads again.
fn classify_and_roundtrip(book: &EcoBook, moves: &str, extended: bool) -> (Option<Eco>, Game) {
    let mut game = read_games(&pgn_of(moves)).remove(0);
    let code = book
        .classify(&game, ClassifyOptions::game())
        .map(|m| m.eco(extended));
    game.tags_mut().eco = code;
    let text = game_to_string(&game, &WriteOptions::default()).unwrap();
    let back = read_games(&text).remove(0);
    assert!(game.same_content(&back), "{text}");
    (code, back)
}

#[test]
fn classified_code_is_stored_written_and_read_back() {
    let book = EcoBook::parse(MINI_BOOK).unwrap();
    for (moves, expected) in [
        (RUY, "C60"),
        ("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Bxc6 dxc6 *", "C68"),
        (NAJDORF, "B90"),
        (NIMZO_TRANSPOSED, "E20"),
    ] {
        let (code, back) = classify_and_roundtrip(&book, moves, true);
        assert_eq!(
            code.map(|c| c.to_string()).as_deref(),
            Some(expected),
            "{moves}"
        );
        assert_eq!(back.tags().eco, code, "{moves}");
    }
}

#[test]
fn ruy_lopez_without_exchange_gets_the_shorter_entry() {
    let book = EcoBook::parse(MINI_BOOK).unwrap();
    let (code, back) = classify_and_roundtrip(&book, "1. e4 e5 2. Nf3 Nc6 3. Bb5 Nf6 *", true);
    assert_eq!(code.unwrap().to_string(), "C60");
    assert_eq!(back.tags().eco, code);
}

#[test]
fn unclassified_game_keeps_no_eco_tag_through_the_roundtrip() {
    let book = EcoBook::parse(MINI_BOOK).unwrap();
    let (code, back) = classify_and_roundtrip(&book, NO_BOOK_LINE, true);
    assert_eq!(code, None);
    assert_eq!(back.tags().eco, None);
    let text = game_to_string(&back, &WriteOptions::default()).unwrap();
    assert!(!text.contains("[ECO"), "{text}");
}

#[test]
fn eco_tag_is_written_in_the_pgn_text() {
    let book = EcoBook::parse(MINI_BOOK).unwrap();
    let mut game = read_games(&pgn_of(NAJDORF)).remove(0);
    game.tags_mut().eco = book
        .classify(&game, ClassifyOptions::game())
        .map(|m| m.eco(true));
    let text = game_to_string(&game, &WriteOptions::default()).unwrap();
    assert!(text.contains("[ECO \"B90\"]"), "{text}");
}

#[test]
fn a_preexisting_eco_tag_is_read_and_can_be_replaced() {
    let book = EcoBook::parse(MINI_BOOK).unwrap();
    let text = format!("[Event \"x\"]\n[ECO \"A00\"]\n[Result \"*\"]\n\n{RUY}\n");
    let mut game = read_games(&text).remove(0);
    assert_eq!(game.tags().eco.unwrap().to_string(), "A00");
    game.tags_mut().eco = book
        .classify(&game, ClassifyOptions::game())
        .map(|m| m.eco(true));
    let back = read_games(&game_to_string(&game, &WriteOptions::default()).unwrap()).remove(0);
    assert_eq!(back.tags().eco.unwrap().to_string(), "C60");
}

#[test]
fn many_games_in_one_file_keep_their_own_codes() {
    let book = EcoBook::parse(MINI_BOOK).unwrap();
    let mut pgn = String::new();
    let mut expected = Vec::new();
    for moves in [RUY, NAJDORF, NO_BOOK_LINE, NIMZO_TRANSPOSED] {
        let mut g = read_games(&pgn_of(moves)).remove(0);
        g.tags_mut().eco = book
            .classify(&g, ClassifyOptions::game())
            .map(|m| m.eco(true));
        expected.push(g.tags().eco);
        pgn.push_str(&game_to_string(&g, &WriteOptions::default()).unwrap());
        pgn.push('\n');
    }
    let got: Vec<_> = read_games(&pgn).iter().map(|g| g.tags().eco).collect();
    assert_eq!(got, expected);
}

#[test]
fn default_tags_have_no_eco() {
    assert_eq!(Tags::new().eco, None);
}

// ------------------------------------------------------------------ compat

fn scid_reference() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/scid_vs_pc-4.27");
    assert!(
        dir.is_dir(),
        "Scid vs PC reference not found at {} (see docs/referencia-scid.md)",
        dir.display()
    );
    dir
}

fn load_scid_eco() -> EcoBook {
    read_eco_file(scid_reference().join("scid.eco"))
        .expect("read scid.eco")
        .book
}

fn game_of_entry(moves: &str) -> Game {
    let mut game = Game::new();
    let mut node = game.root();
    let mut pos = game.start_position().clone();
    for token in moves.split_whitespace() {
        let san = token.rsplit('.').next().unwrap_or(token);
        if san.is_empty() || san == "*" {
            continue;
        }
        let mv = pos.parse_san(san, SanMode::Tolerant).expect("entry san").mv;
        pos = pos.after(mv).unwrap();
        node = game.add_move(node, mv).unwrap();
    }
    game
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_eco_roundtrips_through_pgn_for_ruy_lopez_najdorf_and_nimzo_transposition() {
    let book = load_scid_eco();
    for (moves, extended, basic) in [
        (
            "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 6. Re1 b5 7. Bb3 d6 8. c3 O-O 9. h3 *",
            "C92a",
            "C92",
        ),
        (
            "1. e4 c5 2. Nf3 d6 3. d4 cxd4 4. Nxd4 Nf6 5. Nc3 a6 *",
            "B90a",
            "B90",
        ),
        ("1. c4 e6 2. Nc3 Nf6 3. d4 Bb4 *", "E20", "E20"),
        ("1. d4 Nf6 2. c4 e6 3. Nc3 Bb4 *", "E20", "E20"),
    ] {
        let (code, back) = classify_and_roundtrip(&book, moves, true);
        assert_eq!(code.unwrap().to_string(), extended, "{moves}");
        assert_eq!(back.tags().eco, code);
        let (code, back) = classify_and_roundtrip(&book, moves, false);
        assert_eq!(code.unwrap().to_string(), basic, "{moves}");
        assert_eq!(back.tags().eco, code);
    }
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_eco_roundtrips_through_pgn() {
    // Every entry's own line classifies at least to its own code and survives PGN.
    let book = load_scid_eco();
    let mut pgn = String::new();
    let mut expected = Vec::new();
    for e in book.entries() {
        let mut g = game_of_entry(e.moves());
        let m = book
            .classify(&g, ClassifyOptions::game())
            .expect("entry classifies");
        assert_eq!(m.entry().position(), e.position(), "line {}", e.line());
        g.tags_mut().eco = Some(m.eco(true));
        expected.push(g.tags().eco);
        pgn.push_str(&game_to_string(&g, &WriteOptions::default()).unwrap());
        pgn.push('\n');
    }
    let back = read_games(&pgn);
    assert_eq!(back.len(), book.len());
    let got: Vec<_> = back.iter().map(|g| g.tags().eco).collect();
    assert_eq!(got, expected);
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_pgn_roundtrip_of_scid_eco_lines() {
    let book = load_scid_eco();
    for opts in [WriteOptions::default(), WriteOptions::scid_export()] {
        let mut pgn = String::new();
        let mut games = Vec::new();
        for e in book.entries() {
            let mut g = game_of_entry(e.moves());
            g.tags_mut().event = e.name().to_owned();
            g.tags_mut().eco = Some(e.eco());
            pgn.push_str(&game_to_string(&g, &opts).unwrap());
            pgn.push('\n');
            games.push(g);
        }
        let back = read_games(&pgn);
        assert_eq!(back.len(), games.len());
        for (a, b) in games.iter().zip(&back) {
            assert!(a.same_content(b), "{}", a.tags().event);
        }
    }
}
