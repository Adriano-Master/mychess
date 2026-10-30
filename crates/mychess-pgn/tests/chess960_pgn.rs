//! Chess960 in PGN: `Variant` tag, `SetUp`/`FEN` output, round-trips, EPD.
//!
//! Spec: `docs/specs/m1-5-chess960-fechamento.md` (D58, D61, rule 9).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use mychess_core::{FenCastlingStyle, FenMode, Game, Position, SanMode, Variant};
use mychess_pgn::epd::{EpdBook, EpdOptions};
use mychess_pgn::read::{
    DiagnosticKind, ParsedGame, PgnReader, ReadOptions, Termination, UnsupportedVariant,
};
use mychess_pgn::write::{WriteOptions, game_to_string};

fn read_one(text: &str, opts: &ReadOptions) -> ParsedGame {
    let mut games: Vec<ParsedGame> = PgnReader::new(text.as_bytes(), opts.clone())
        .map(|r| r.expect("in-memory read"))
        .collect();
    assert_eq!(games.len(), 1);
    games.remove(0)
}

fn read_default(text: &str) -> ParsedGame {
    read_one(text, &ReadOptions::default())
}

/// Plays `sans` from `start` into a main line.
fn play(start: Position, sans: &[&str]) -> Game {
    let mut game = Game::from_position(start);
    let mut cur = game.root();
    for san in sans {
        let pos = game.position_at(cur).unwrap().clone();
        let mv = pos.parse_san(san, SanMode::Strict).expect("legal").mv;
        cur = game.add_move(cur, mv).expect("add");
    }
    game
}

fn mainline_sans(game: &Game) -> Vec<String> {
    let mut parent = game.root();
    let mut out = Vec::new();
    for n in game.mainline() {
        out.push(
            game.position_at(parent)
                .unwrap()
                .san(game.mv(n).unwrap())
                .unwrap(),
        );
        parent = n;
    }
    out
}

fn assert_same(a: &Game, b: &Game) {
    assert_eq!(a.variant(), b.variant());
    assert_eq!(a.start_position(), b.start_position());
    assert_eq!(a.start_position().fen(), b.start_position().fen());
    assert_eq!(mainline_sans(a), mainline_sans(b));
    let end_a = a.mainline().last().map(|n| a.position_at(n).unwrap().fen());
    let end_b = b.mainline().last().map(|n| b.position_at(n).unwrap().fen());
    assert_eq!(end_a, end_b);
}

fn roundtrip(game: &Game) -> Game {
    let text = game_to_string(game, &WriteOptions::default()).unwrap();
    let parsed = read_default(&text);
    assert_eq!(parsed.termination, Termination::ResultToken, "{text}");
    assert!(
        parsed
            .diagnostics
            .iter()
            .all(|d| d.kind != DiagnosticKind::Chess960Detected),
        "{text}"
    );
    parsed.game
}

#[test]
fn writer_emits_variant_setup_fen_in_order_for_every_start() {
    for n in [0u16, 518, 959] {
        let game = play(Position::chess960_start(n).unwrap(), &[]);
        let text = game_to_string(&game, &WriteOptions::default()).unwrap();
        let fen = game.start_position().fen();
        let tail = format!("[Variant \"Chess960\"]\n[SetUp \"1\"]\n[FEN \"{fen}\"]\n\n");
        assert!(text.contains(&tail), "n={n}: {text}");
        assert_eq!(text.matches("[Variant").count(), 1);
    }
}

#[test]
fn writer_fen_castling_style_shredder() {
    let game = Game::from_position(Position::chess960_start(0).unwrap());
    let opts = WriteOptions::default().with_fen_castling(FenCastlingStyle::Shredder);
    let text = game_to_string(&game, &opts).unwrap();
    assert!(text.contains("BBQNNRKR w HFhf - 0 1"), "{text}");
    let default = game_to_string(&game, &WriteOptions::default()).unwrap();
    assert!(default.contains("BBQNNRKR w KQkq - 0 1"), "{default}");
    // Both read back to the same game.
    assert_same(&read_default(&text).game, &read_default(&default).game);
}

#[test]
fn standard_fen_game_has_no_variant_tag() {
    let pos = Position::from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1").unwrap();
    let text = game_to_string(&Game::from_position(pos), &WriteOptions::default()).unwrap();
    assert!(!text.contains("Variant"), "{text}");
    assert!(text.contains("[SetUp \"1\"]\n[FEN "));
}

#[test]
fn roundtrip_positions_0_and_518() {
    for n in [0u16, 518] {
        let start = Position::chess960_start(n).unwrap();
        let game = play(start.clone(), &["e4", "e5", "Nf3"]);
        let back = roundtrip(&game);
        assert_eq!(back.variant(), Variant::Chess960);
        assert_same(&game, &back);
    }
}

#[test]
fn roundtrip_position_0_with_moves() {
    let start = Position::chess960_start(0).unwrap();
    let first = start.legal_moves().into_iter().next().unwrap();
    let san = start.san(first).unwrap();
    let game = play(start, &[san.as_str()]);
    assert_same(&game, &roundtrip(&game));
}

#[test]
fn roundtrip_castling_with_fixed_king_and_rook() {
    // King c1 and rook b1: O-O-O leaves the king where it is.
    let start =
        Position::from_fen_with("4k3/8/8/8/8/8/8/1RK5 w B - 0 1", FenMode::Chess960).unwrap();
    let game = play(start, &["O-O-O", "Kf8"]);
    assert_eq!(
        game.position_at(game.mainline().next().unwrap())
            .unwrap()
            .fen(),
        "4k3/8/8/8/8/8/8/2KR4 b - - 1 1"
    );
    let back = roundtrip(&game);
    assert_eq!(mainline_sans(&back)[0], "O-O-O");
    assert_same(&game, &back);
}

#[test]
fn roundtrip_castling_king_side_rook_stays_fixed() {
    let start =
        Position::from_fen_with("4k3/8/8/8/8/8/8/6KR w H - 0 1", FenMode::Chess960).unwrap();
    let game = play(start, &["O-O", "Kd7"]);
    assert_same(&game, &roundtrip(&game));
}

#[test]
fn variant_tag_spellings_select_chess960() {
    for v in [
        "Chess960",
        "chess 960",
        "Fischerandom",
        "Fischerrandom",
        "Fischer-Random",
        "fischer_random_chess",
        "FRC",
        "960",
        "Freestyle",
    ] {
        let text = format!("[Variant \"{v}\"]\n\n1. e4 e5 *\n");
        let pg = read_default(&text);
        assert_eq!(pg.game.variant(), Variant::Chess960, "{v}");
        assert_eq!(pg.game.start_position().chess960_number(), Some(518), "{v}");
        assert!(pg.diagnostics.is_empty(), "{v}: {:?}", pg.diagnostics);
        assert!(
            pg.game
                .tags()
                .pgn_pairs()
                .iter()
                .all(|(n, _)| n != "Variant")
        );
    }
}

#[test]
fn standard_variant_spellings_stay_standard() {
    for v in [
        "Standard",
        "chess",
        "Normal",
        "From Position",
        "",
        "FromPosition",
    ] {
        let text = format!("[Variant \"{v}\"]\n\n1. e4 e5 *\n");
        let pg = read_default(&text);
        assert_eq!(pg.game.variant(), Variant::Standard, "{v:?}");
        assert!(pg.diagnostics.is_empty(), "{v:?}: {:?}", pg.diagnostics);
    }
}

#[test]
fn shredder_fen_with_variant_is_read() {
    let text = "[Variant \"Chess960\"]\n[SetUp \"1\"]\n\
                [FEN \"bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w HFhf - 0 1\"]\n\n\
                1. e4 e5 *\n";
    let pg = read_default(text);
    assert_eq!(pg.game.variant(), Variant::Chess960);
    assert_eq!(pg.game.start_position().chess960_number(), Some(0));
    assert!(pg.diagnostics.is_empty(), "{:?}", pg.diagnostics);
}

#[test]
fn variant_960_with_fen_in_standard_looking_form_uses_960_mode() {
    let text = "[Variant \"Chess960\"]\n[FEN \"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\"]\n\n1. e4 *\n";
    let pg = read_default(text);
    assert_eq!(pg.game.variant(), Variant::Chess960);
    assert_eq!(pg.game.start_position().chess960_number(), Some(518));
}

#[test]
fn chess960_fen_without_variant_is_detected() {
    let text = "[SetUp \"1\"]\n[FEN \"bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq - 0 1\"]\n\n1. e4 *\n";
    let pg = read_default(text);
    assert_eq!(pg.game.variant(), Variant::Chess960);
    let detected: Vec<_> = pg
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::Chess960Detected)
        .collect();
    assert_eq!(detected.len(), 1, "{:?}", pg.diagnostics);
    assert_eq!(detected[0].severity, mychess_pgn::read::Severity::Info);
    assert_eq!(pg.diagnostics.len(), 1);
}

#[test]
fn standard_fen_without_variant_is_not_detected() {
    let text = "[SetUp \"1\"]\n[FEN \"4k3/8/8/8/8/8/8/4K2R w K - 0 1\"]\n\n1. Rh2 *\n";
    let pg = read_default(text);
    assert_eq!(pg.game.variant(), Variant::Standard);
    assert!(pg.diagnostics.is_empty(), "{:?}", pg.diagnostics);
}

#[test]
fn unsupported_variant_returns_header_only() {
    let text = "[Event \"X\"]\n[Variant \"Crazyhouse\"]\n\n1. e4 e5 *\n";
    let pg = read_default(text);
    assert_eq!(pg.termination, Termination::UnsupportedVariant);
    assert_eq!(pg.game.tags().event, "X");
    assert!(
        pg.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::UnsupportedVariant)
    );
    assert_eq!(pg.game.mainline().count(), 0);
}

#[test]
fn try_standard_still_reads_unsupported_variants() {
    let opts = ReadOptions {
        unsupported_variant: UnsupportedVariant::TryStandard,
        ..ReadOptions::default()
    };
    let pg = read_one("[Variant \"Atomic\"]\n\n1. e4 e5 *\n", &opts);
    assert_eq!(pg.game.mainline().count(), 2);
}

#[test]
fn epd_reads_and_writes_a_960_position() {
    let line = "bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq - bm e4;\n";
    let load = EpdBook::read(line.as_bytes(), EpdOptions::default()).unwrap();
    let rec = load.book.iter().next().unwrap();
    assert_eq!(rec.position().variant(), Variant::Chess960);
    assert_eq!(rec.position().chess960_number(), Some(0));
    let mut out = Vec::new();
    load.book.write(&mut out, EpdOptions::default()).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.starts_with("bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq -"),
        "{text}"
    );
    let again = EpdBook::read(text.as_bytes(), EpdOptions::default()).unwrap();
    assert_eq!(again.book.len(), 1);
    assert_eq!(again.book.iter().next().unwrap().position(), rec.position());
}
