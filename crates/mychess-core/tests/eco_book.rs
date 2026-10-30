//! Integration tests for the ECO book (`EcoBook`, classification, summary).
//!
//! Contract: `docs/specs/m1-4-epd-eco.md`, "Parte B: ECO" (rules 20-34,
//! D39-D41, D48-D53) and Scid's `PBook::ReadEcoFile` / `EcoSummary` /
//! `EcoFind` (`src/pbook.cpp`) and `sc_eco` (`src/tkscid.cpp`).

#![allow(
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::cast_possible_truncation,
    clippy::redundant_closure_for_method_calls
)]

use mychess_core::{
    ClassifyOptions, CorruptReason, EcoBook, EcoBookError, EcoDiagnostic, EcoParseOptions, Game,
    Move, OnError, Position, SanMode, translate_eco_name,
};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;

// ---------------------------------------------------------------- helpers

/// Plays SAN words from the standard start; returns start, moves and the end position.
fn line_from(start: &Position, sans: &str) -> (Vec<Move>, Position) {
    let mut pos = start.clone();
    let mut mvs = Vec::new();
    for san in sans.split_whitespace() {
        let mv = pos
            .parse_san(san, SanMode::Strict)
            .unwrap_or_else(|e| panic!("bad san {san}: {e}"))
            .mv;
        pos.play(mv).expect("legal");
        mvs.push(mv);
    }
    (mvs, pos)
}

fn std_line(sans: &str) -> (Vec<Move>, Position) {
    line_from(&Position::new(), sans)
}

fn game_from(start: Position, sans: &str) -> Game {
    let (mvs, _) = line_from(&start, sans);
    let mut g = Game::from_position(start);
    let mut node = g.root();
    for mv in mvs {
        node = g.add_move(node, mv).expect("add");
    }
    g
}

fn game(sans: &str) -> Game {
    game_from(Position::new(), sans)
}

fn skip_opts() -> EcoParseOptions {
    let mut o = EcoParseOptions::default();
    o.on_error = OnError::SkipEntry;
    o
}

fn max_ply_opts(limit: u32) -> ClassifyOptions {
    let mut o = ClassifyOptions::game();
    o.max_ply = Some(limit);
    o
}

fn parse(text: &str) -> EcoBook {
    EcoBook::parse(text).unwrap_or_else(|e| panic!("parse failed: {e}"))
}

fn corrupt(text: &str) -> (usize, CorruptReason) {
    match EcoBook::parse(text) {
        Err(EcoBookError::Corrupt { line, reason }) => (line, reason),
        other => panic!("expected Corrupt, got {other:?}"),
    }
}

fn codes(book: &EcoBook) -> Vec<String> {
    book.entries().iter().map(|e| e.eco().to_string()).collect()
}

fn found(book: &EcoBook, g: &Game, o: ClassifyOptions) -> Option<(u32, String)> {
    book.classify(g, o)
        .map(|m| (m.ply, m.entry().eco().to_string()))
}

const SAMPLE: &str = "# comment\n\
    A00a \"Start\"  *\n\
    A00b \"Barnes Opening\"  1.f3 *\n\
    A00c \"Anderssen\"  1.a3 *\n\
    B20 \"Sicilian\" 1.e4 c5 *\n\
    B90 \"Najdorf\" 1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4 Nf6 5.Nc3 a6 *\n\
    B90a \"Najdorf, English\" 1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4 Nf6 5.Nc3 a6 6.Be3 *\n\
    B91 \"Najdorf: Zagreb\" 1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4 Nf6 5.Nc3 a6 6.g3 *\n\
    B92 \"Najdorf: Opocensky\" 1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4 Nf6 5.Nc3 a6 6.Be2 *\n\
    E20 \"Nimzo-Indian Defence\" 1.d4 Nf6 2.c4 e6 3.Nc3 Bb4 *\n";

// ---------------------------------------------------------------- syntax

#[test]
fn parses_empty_text_into_empty_book() {
    let book = parse("");
    assert_eq!(book.len(), 0);
    assert!(book.is_empty());
    assert_eq!(book.fewest_pieces(), 32);
}

#[test]
fn parses_text_with_only_comments_and_blank_lines() {
    let book = parse("# only\n\n   \n# comments\n\n");
    assert!(book.is_empty());
}

#[rstest]
#[case("A00", "A00")]
#[case("A00a", "A00a")]
#[case("A00a1", "A00a1")]
#[case("B90z4", "B90z4")]
#[case("E99b2", "E99b2")]
#[case("C42", "C42")]
fn parses_code_forms(#[case] code: &str, #[case] shown: &str) {
    let book = parse(&format!("{code} \"X\" 1.e4 *"));
    assert_eq!(book.len(), 1);
    assert_eq!(book.entries()[0].eco().to_string(), shown);
}

#[test]
fn parses_start_entry_as_initial_position() {
    let book = parse("A00a \"Start\" *\n");
    let e = &book.entries()[0];
    assert_eq!(e.name(), "Start");
    assert_eq!(e.moves(), "");
    assert_eq!(e.plies(), 0);
    assert_eq!(e.position(), &Position::new());
}

#[test]
fn parses_entry_fields_for_simple_line() {
    let book = parse("# c\nA00b \"Barnes Opening\"  1.f3 *\n");
    let e = &book.entries()[0];
    assert_eq!(e.eco().to_string(), "A00b");
    assert_eq!(e.name(), "Barnes Opening");
    assert_eq!(e.moves(), "1.f3");
    assert_eq!(e.plies(), 1);
    assert_eq!(e.line(), 2, "line of the closing star");
    assert_eq!(e.position(), &std_line("f3").1);
}

#[test]
fn collapses_whitespace_and_newlines_in_moves() {
    let book = parse("A00b \"X\"\n   1.e4\t e5 \r\n\n  2.Nf3   Nc6\n   *\n");
    let e = &book.entries()[0];
    assert_eq!(e.moves(), "1.e4 e5 2.Nf3 Nc6");
    assert_eq!(e.plies(), 4);
    assert_eq!(e.line(), 5);
}

#[test]
fn keeps_utf8_names_intact() {
    let book = parse("A00l \"Van Geet: T\u{fc}bingen G\u{e4}mbit \u{f6}\" 1.Nc3 *");
    assert_eq!(
        book.entries()[0].name(),
        "Van Geet: T\u{fc}bingen G\u{e4}mbit \u{f6}"
    );
}

#[test]
fn name_may_contain_star_hash_and_digits() {
    let book = parse("B00 \"Odd * # 1.e4 name\" 1.e4 *");
    assert_eq!(book.entries()[0].name(), "Odd * # 1.e4 name");
    assert_eq!(book.entries()[0].plies(), 1);
}

#[test]
fn ignores_text_between_code_and_opening_quote() {
    let book = parse("A00b   junk here e5 \"Name\" 1.f3 *");
    assert_eq!(book.entries()[0].name(), "Name");
}

#[test]
fn hash_comment_lines_are_discarded_between_entries() {
    let book = parse("# A00 \"no\" 1.e4 *\nB00 \"yes\" 1.e4 *\n# trailing C00\n");
    assert_eq!(codes(&book), ["B00"]);
}

#[test]
fn text_before_first_code_is_skipped() {
    let book = parse("  some preamble 123 !\n\nB00 \"Yes\" 1.e4 *");
    assert_eq!(book.len(), 1);
}

#[test]
fn castling_oo_and_ooo_are_read_as_castles() {
    let book = parse(
        "D00 \"Both\" 1.d4 d5 2.Nc3 Nc6 3.Bf4 Bf5 4.Qd2 Qd7 5.OOO OOO *\n\
         C50 \"Short\" 1.e4 e5 2.Nf3 Nc6 3.Bc4 Bc5 4.OO Nf6 *\n",
    );
    let (_, pos) = std_line("d4 d5 Nc3 Nc6 Bf4 Bf5 Qd2 Qd7 O-O-O O-O-O");
    assert_eq!(book.entries()[0].position(), &pos);
    let (_, pos) = std_line("e4 e5 Nf3 Nc6 Bc4 Bc5 O-O Nf6");
    assert_eq!(book.entries()[1].position(), &pos);
}

#[test]
fn castling_with_dashes_is_also_accepted() {
    let book = parse("C50 \"Short\" 1.e4 e5 2.Nf3 Nc6 3.Bc4 Bc5 4.O-O *");
    assert_eq!(book.entries()[0].plies(), 7);
}

#[test]
fn lowercase_piece_letters_are_uppercased() {
    // `n`, `q`, `r`, `k` are pieces for Scid; `b` is a pawn.
    let book = parse("C40 \"Low\" 1.e4 e5 2.nf3 nc6 3.qe2 qe7 *");
    let (_, pos) = std_line("e4 e5 Nf3 Nc6 Qe2 Qe7");
    assert_eq!(book.entries()[0].position(), &pos);
}

#[test]
fn lowercase_b_is_a_pawn_not_a_bishop() {
    // `bb2` would be a bishop move if `b` were a piece; as a pawn move it is
    // illegal (no pawn reaches b2 from b-file).
    let (line, reason) = corrupt("A01 \"x\" 1.b3 e5 2.bb2 *");
    assert_eq!(line, 1);
    assert!(
        matches!(reason, CorruptReason::IllegalMove { .. }),
        "{reason:?}"
    );
}

#[test]
fn lowercase_b_pawn_capture_is_read() {
    let book = parse("B00 \"x\" 1.e4 d5 2.exd5 c6 3.dxc6 Nf6 4.cxb7 Nbd7 5.bxa8=Q *");
    assert_eq!(book.entries()[0].plies(), 9);
}

#[test]
fn promotion_with_and_without_equals_sign_agrees() {
    let a = parse("B00 \"a\" 1.e4 d5 2.exd5 c6 3.dxc6 Nf6 4.cxb7 Nbd7 5.bxa8=Q *");
    let b = parse("B00 \"a\" 1.e4 d5 2.exd5 c6 3.dxc6 Nf6 4.cxb7 Nbd7 5.bxa8Q+ *");
    assert_eq!(a.entries()[0].position(), b.entries()[0].position());
    let (_, pos) = std_line("e4 d5 exd5 c6 dxc6 Nf6 cxb7 Nbd7 bxa8=Q");
    assert_eq!(a.entries()[0].position(), &pos);
}

#[test]
fn pawn_promotion_to_knight_is_read() {
    let book = parse("B00 \"u\" 1.e4 d5 2.exd5 c6 3.dxc6 Nf6 4.cxb7 Nbd7 5.bxa8=N *");
    let (_, pos) = std_line("e4 d5 exd5 c6 dxc6 Nf6 cxb7 Nbd7 bxa8=N");
    assert_eq!(book.entries()[0].position(), &pos);
}

#[test]
fn parentheses_and_loose_numbers_are_dropped_as_continuation() {
    // Spec rule 24: the Scid reads "(...)" text as a continuation of the line.
    let book = parse("C60 \"x\" 1.e4 e5 2.Nf3 Nc6 (3.Bb5 a6) 4 *");
    assert_eq!(book.entries()[0].plies(), 6);
    let (_, pos) = std_line("e4 e5 Nf3 Nc6 Bb5 a6");
    assert_eq!(book.entries()[0].position(), &pos);
}

#[test]
fn check_and_annotation_symbols_in_moves_are_dropped() {
    let book = parse("C20 \"x\" 1.e4 e5 2.Qh5 Nc6 3.Bc4 Nf6 4.Qxf7# *");
    assert_eq!(book.entries()[0].plies(), 7);
}

#[test]
fn multiple_entries_share_prefix_lines() {
    let book = parse(SAMPLE);
    assert_eq!(book.len(), 9);
    assert_eq!(
        codes(&book),
        [
            "A00a", "A00b", "A00c", "B20", "B90", "B90a", "B91", "B92", "E20"
        ]
    );
}

#[test]
fn entries_are_in_insertion_order() {
    let book = parse("C00 \"z\" 1.e4 e6 *\nA00b \"a\" 1.f3 *\nB00 \"m\" 1.e4 *");
    assert_eq!(codes(&book), ["C00", "A00b", "B00"]);
}

#[test]
fn last_entry_without_trailing_newline_is_read() {
    let book = parse("A00b \"a\" 1.f3 *");
    assert_eq!(book.len(), 1);
}

#[test]
fn fewest_pieces_tracks_the_smallest_position() {
    let book =
        parse("B00 \"a\" 1.e4 *\nB01 \"b\" 1.e4 d5 2.exd5 Qxd5 *\nB02 \"c\" 1.e4 d5 2.exd5 *");
    assert_eq!(book.fewest_pieces(), 30);
    let all32 = parse("A00a \"Start\" *");
    assert_eq!(all32.fewest_pieces(), 32);
}

// ---------------------------------------------------------------- errors

#[test]
fn bad_code_with_missing_digit_reports_line() {
    let (line, reason) = corrupt("# c\nA00a \"ok\" *\nB2 \"bad\" 1.e4 *\n");
    assert_eq!(reason, CorruptReason::BadCode);
    assert_eq!(line, 3);
}

#[test]
fn bad_code_when_letter_is_followed_by_non_digits() {
    let (line, reason) = corrupt("Bxx \"bad\" 1.e4 *");
    assert_eq!((line, reason), (1, CorruptReason::BadCode));
}

#[test]
fn bad_code_at_end_of_text() {
    let (line, reason) = corrupt("A");
    assert_eq!((line, reason), (1, CorruptReason::BadCode));
    let (_, reason) = corrupt("A0");
    assert_eq!(reason, CorruptReason::BadCode);
}

#[test]
fn stray_capital_after_star_starts_a_corrupt_entry() {
    // Spec rule 20: letters A-E lost after a `*` start an entry.
    let (line, reason) = corrupt("A00a \"Start\" *\nBut this is stray\n");
    assert_eq!(reason, CorruptReason::BadCode);
    assert_eq!(line, 2);
}

#[test]
fn missing_closing_quote_is_corrupt() {
    let (line, reason) = corrupt("A00a \"Start\" *\nB20 \"Sicilian 1.e4 c5 *");
    assert_eq!(reason, CorruptReason::UnterminatedName);
    assert_eq!(line, 2);
}

#[test]
fn missing_opening_quote_is_corrupt() {
    let (_, reason) = corrupt("B20 Sicilian 1.e4 c5 *");
    assert_eq!(reason, CorruptReason::UnterminatedName);
}

#[test]
fn missing_star_is_corrupt() {
    let (line, reason) = corrupt("B20 \"Sicilian\" 1.e4 c5\n2.Nf3");
    assert_eq!(reason, CorruptReason::UnterminatedMoves);
    assert_eq!(line, 2);
}

#[test]
fn illegal_move_reports_the_line_of_the_star() {
    let (line, reason) = corrupt("A00a \"Start\" *\nB20 \"x\"\n 1.e4\n c5 2.Ke5 *\n");
    assert_eq!(line, 4);
    assert!(
        matches!(reason, CorruptReason::IllegalMove { .. }),
        "{reason:?}"
    );
}

#[test]
fn illegal_move_reason_names_the_offending_word() {
    let (_, reason) = corrupt("B20 \"x\" 1.e4 e5 2.Ke5 *");
    match reason {
        CorruptReason::IllegalMove { text } => assert!(text.contains("Ke5"), "{text}"),
        other => panic!("{other:?}"),
    }
}

#[rstest]
#[case::unknown_initial_letter("A00 \"x\" 1.e4 x5 *")]
#[case::lowercase_o_castle("A00 \"x\" 1.e4 e5 2.Nf3 Nc6 3.Bc4 Bc5 4.oo *")]
#[case::castle_with_no_right("A00 \"x\" 1.OO *")]
#[case::castle_triple_o_too_long("A00 \"x\" 1.e4 e5 2.OOOO *")]
#[case::pawn_move_blocked("A00 \"x\" 1.e4 e5 2.e5 *")]
#[case::wrong_color_move("A00 \"x\" 1.e4 Nf3 *")]
#[case::capture_nothing("A00 \"x\" 1.exd5 *")]
#[case::ambiguous_knight("A00 \"x\" 1.Nf3 Nf6 2.Nd2 *")]
#[case::pawn_two_ranks_blocked("A00 \"x\" 1.Nf3 Nf6 2.Ng1 Ng8 3.e5 e4 4.e6 *")]
#[case::king_into_check(
    "A00 \"x\" 1.e4 e5 2.Ke2 Ke7 3.Kd3 Kd6 4.Kc3 Kc6 5.Kb3 Kb6 6.Ka4 Ka6 7.Kb3 Kb6 8.Kb4 *"
)]
fn illegal_or_unreadable_moves_are_corrupt(#[case] text: &str) {
    let (line, reason) = corrupt(text);
    assert_eq!(line, 1);
    assert!(
        matches!(reason, CorruptReason::IllegalMove { .. }),
        "{reason:?}"
    );
}

#[test]
fn oversized_name_is_rejected_as_too_large() {
    let text = format!("A00 \"{}\" 1.e4 *", "x".repeat((1 << 20) + 10));
    let (_, reason) = corrupt(&text);
    assert_eq!(reason, CorruptReason::TooLarge);
}

#[test]
fn oversized_moves_are_rejected_as_too_large() {
    let text = format!("A00 \"x\" {} *", "e".repeat((1 << 20) + 10));
    let (_, reason) = corrupt(&text);
    assert_eq!(reason, CorruptReason::TooLarge);
}

#[test]
fn huge_unterminated_entry_does_not_panic() {
    let text = format!("A00 \"{}", "y".repeat(3 << 20));
    assert!(EcoBook::parse(&text).is_err());
}

#[test]
fn corrupt_error_leaves_no_partial_book() {
    // `parse` returns Err, never a partial Ok.
    assert!(EcoBook::parse("A00a \"Start\" *\nB20 \"x\" 1.Zz *").is_err());
}

#[test]
fn skip_entry_drops_illegal_entry_and_reports_it() {
    let text = "A00b \"a\" 1.f3 *\nB20 \"bad\"\n 1.e4 Ke5 *\nA00c \"c\" 1.a3 *\n";
    let report =
        EcoBook::parse_with(text, skip_opts()).expect("skip mode never fails on corrupt entries");
    assert_eq!(codes(&report.book), ["A00b", "A00c"]);
    assert_eq!(report.diagnostics.len(), 1);
    match &report.diagnostics[0] {
        EcoDiagnostic::EntrySkipped { line, reason } => {
            assert_eq!(*line, 3);
            assert!(matches!(reason, CorruptReason::IllegalMove { .. }));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn skip_entry_survives_several_bad_entries_in_a_row() {
    let text = "A00b \"a\" 1.zz *\nA00c \"b\" 1.yy *\nA00d \"ok\" 1.h3 *\n";
    let opts = skip_opts();
    let r = EcoBook::parse_with(text, opts).expect("ok");
    assert_eq!(codes(&r.book), ["A00d"]);
    assert_eq!(r.diagnostics.len(), 2);
}

#[test]
fn skip_entry_with_unterminated_tail_ends_without_error() {
    let opts = skip_opts();
    let r = EcoBook::parse_with("A00b \"a\" 1.f3 *\nB20 \"open 1.e4", opts).expect("ok");
    assert_eq!(r.book.len(), 1);
    assert!(r.diagnostics.iter().any(|d| matches!(
        d,
        EcoDiagnostic::EntrySkipped {
            reason: CorruptReason::UnterminatedName,
            ..
        }
    )));
}

#[test]
fn abort_is_the_default_policy() {
    assert_eq!(EcoParseOptions::default().on_error, OnError::Abort);
    let r = EcoBook::parse_with("A00 \"x\" 1.zz *", EcoParseOptions::default());
    assert!(matches!(r, Err(EcoBookError::Corrupt { line: 1, .. })));
}

// ------------------------------------------------------------- duplicates

#[test]
fn repeated_position_last_entry_wins_and_keeps_first_slot() {
    let text = "A00b \"first\" 1.f3 *\nA00c \"other\" 1.e4 *\nA00d \"second\" 1.f3 *\n";
    let r = EcoBook::parse_with(text, EcoParseOptions::default()).expect("parse");
    assert_eq!(r.book.len(), 2);
    assert_eq!(r.entries_read, 3);
    assert_eq!(
        codes(&r.book),
        ["A00d", "A00c"],
        "overwritten data in first slot"
    );
    assert_eq!(r.book.entries()[0].name(), "second");
    assert_eq!(r.book.entries()[0].line(), 3);
    assert_eq!(
        r.diagnostics,
        vec![EcoDiagnostic::DuplicatePosition {
            line: 3,
            first_line: 1
        }]
    );
}

#[test]
fn transposed_lines_count_as_the_same_position() {
    let text =
        "E20 \"one\" 1.d4 Nf6 2.c4 e6 3.Nc3 Bb4 *\nE20 \"two\" 1.c4 e6 2.Nc3 Nf6 3.d4 Bb4 *\n";
    let r = EcoBook::parse_with(text, EcoParseOptions::default()).expect("parse");
    assert_eq!(r.book.len(), 1);
    assert_eq!(r.book.entries()[0].name(), "two");
    assert_eq!(
        r.diagnostics,
        vec![EcoDiagnostic::DuplicatePosition {
            line: 2,
            first_line: 1
        }]
    );
}

#[test]
fn three_entries_for_one_position_report_first_line_each_time() {
    let text = "A00b \"1\" 1.f3 *\nA00b \"2\" 1.f3 *\nA00b \"3\" 1.f3 *\n";
    let r = EcoBook::parse_with(text, EcoParseOptions::default()).expect("parse");
    assert_eq!(r.book.len(), 1);
    assert_eq!(r.book.entries()[0].name(), "3");
    assert_eq!(
        r.diagnostics,
        vec![
            EcoDiagnostic::DuplicatePosition {
                line: 2,
                first_line: 1
            },
            EcoDiagnostic::DuplicatePosition {
                line: 3,
                first_line: 1
            },
        ]
    );
}

#[test]
fn two_start_entries_are_a_duplicate() {
    let r = EcoBook::parse_with(
        "A00a \"Start\" *\nA00 \"Again\" *\n",
        EcoParseOptions::default(),
    )
    .expect("parse");
    assert_eq!(r.book.len(), 1);
    assert_eq!(r.book.entries()[0].name(), "Again");
    assert_eq!(r.diagnostics.len(), 1);
}

#[test]
fn fewest_pieces_ignores_overwritten_nothing_when_book_has_no_captures() {
    let r = parse("A00b \"1\" 1.f3 *\nA00b \"2\" 1.f3 *");
    assert_eq!(r.fewest_pieces(), 32);
}

// ---------------------------------------------------------------- lookup

#[test]
fn lookup_finds_entry_by_position_and_ignores_clocks() {
    let book = parse(SAMPLE);
    let (_, pos) = std_line("e4 c5");
    assert_eq!(book.lookup(&pos).map(|e| e.name()), Some("Sicilian"));
    let fen = pos.fen();
    // Same placement, side, castling and ep: different clocks.
    let parts: Vec<&str> = fen.split(' ').collect();
    let other = Position::from_fen(&format!(
        "{} {} {} {} 17 40",
        parts[0], parts[1], parts[2], parts[3]
    ))
    .expect("fen");
    assert_eq!(book.lookup(&other).map(|e| e.name()), Some("Sicilian"));
}

#[test]
fn lookup_distinguishes_side_to_move_and_castling_rights() {
    let book = parse(SAMPLE);
    let (_, pos) = std_line("e4 c5");
    let swapped = Position::from_fen(&pos.fen().replace(" w ", " b ")).expect("fen");
    assert!(book.lookup(&swapped).is_none());
    let no_castle = Position::from_fen(&pos.fen().replace("KQkq", "-")).expect("fen");
    assert!(book.lookup(&no_castle).is_none());
}

#[test]
fn lookup_ignores_a_pseudo_legal_only_en_passant_square() {
    // D40: only a legal en passant matters; after 1.e4 no black pawn can take.
    let book = parse(SAMPLE);
    let (_, pos) = std_line("e4");
    assert!(book.lookup(&pos).is_none());
    let book2 = parse("B00 \"King pawn\" 1.e4 *");
    let with_ep = Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1")
        .expect("fen");
    assert!(
        book2.lookup(&with_ep).is_some(),
        "illegal ep square must not split positions"
    );
}

#[test]
fn lookup_misses_unknown_position() {
    let book = parse(SAMPLE);
    assert!(book.lookup(&std_line("h4").1).is_none());
}

// -------------------------------------------------------- classification

#[test]
fn classifies_deepest_position_of_main_line() {
    let book = parse(SAMPLE);
    let g = game("e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3 h6 h3");
    assert_eq!(
        found(&book, &g, ClassifyOptions::game()),
        Some((11, "B90a".into()))
    );
}

#[test]
fn classifies_with_the_first_deepest_not_the_first_shallowest() {
    let book = parse(SAMPLE);
    let g = game("e4 c5 Nf3");
    assert_eq!(
        found(&book, &g, ClassifyOptions::game()),
        Some((2, "B20".into()))
    );
}

#[rstest]
#[case("d4 Nf6 c4 e6 Nc3 Bb4")]
#[case("c4 e6 Nc3 Nf6 d4 Bb4")]
#[case("d4 e6 c4 Nf6 Nc3 Bb4")]
fn transpositions_reach_the_same_entry(#[case] sans: &str) {
    let book = parse(SAMPLE);
    let g = game(sans);
    assert_eq!(
        found(&book, &g, ClassifyOptions::game()),
        Some((6, "E20".into()))
    );
}

#[test]
fn returning_to_the_initial_position_matches_start_entry() {
    let book = parse(SAMPLE);
    let g = game("Nf3 Nf6 Ng1 Ng8");
    assert_eq!(
        found(&book, &g, ClassifyOptions::game()),
        Some((4, "A00a".into()))
    );
}

#[test]
fn empty_game_matches_start_at_ply_zero() {
    let book = parse(SAMPLE);
    assert_eq!(
        found(&book, &Game::new(), ClassifyOptions::game()),
        Some((0, "A00a".into()))
    );
}

#[test]
fn empty_game_without_start_entry_has_no_match() {
    let book = parse("B20 \"Sicilian\" 1.e4 c5 *");
    assert!(
        book.classify(&Game::new(), ClassifyOptions::game())
            .is_none()
    );
}

#[test]
fn game_without_any_matching_position_has_no_match() {
    let book = parse("B20 \"Sicilian\" 1.e4 c5 *");
    assert!(
        book.classify(&game("d4 d5 c4"), ClassifyOptions::game())
            .is_none()
    );
}

#[test]
fn empty_book_never_matches() {
    let book = parse("");
    assert!(
        book.classify(&game("e4 e5"), ClassifyOptions::game())
            .is_none()
    );
    assert!(
        book.classify(&Game::new(), ClassifyOptions::batch())
            .is_none()
    );
}

#[test]
fn variations_are_ignored_when_classifying() {
    let book = parse("B20 \"Sicilian\" 1.e4 c5 *\nA00c \"Anderssen\" 1.a3 *");
    let mut g = Game::new();
    let start = Position::new();
    let a3 = start.parse_san("a3", SanMode::Strict).expect("a3").mv;
    let e4 = start.parse_san("e4", SanMode::Strict).expect("e4").mv;
    g.add_move(g.root(), a3).expect("a3");
    let var = g.add_move(g.root(), e4).expect("var");
    let after_e4 = start.after(e4).expect("play");
    let c5 = after_e4.parse_san("c5", SanMode::Strict).expect("c5").mv;
    g.add_move(var, c5).expect("c5");
    assert_eq!(
        found(&book, &g, ClassifyOptions::game()),
        Some((1, "A00c".into()))
    );
}

#[test]
fn classify_line_matches_classify_on_the_same_moves() {
    let book = parse(SAMPLE);
    let (mvs, _) = std_line("e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 g3");
    let m = book
        .classify_line(&Position::new(), &mvs, ClassifyOptions::game())
        .expect("match");
    assert_eq!(
        (m.ply, m.entry().eco().to_string()),
        (11, "B91".to_string())
    );
}

#[test]
fn classify_line_with_no_moves_matches_start_position_entry() {
    let book = parse(SAMPLE);
    let m = book
        .classify_line(&Position::new(), &[], ClassifyOptions::game())
        .expect("start");
    assert_eq!(m.ply, 0);
}

#[test]
fn classify_line_stops_at_the_first_illegal_move() {
    let book = parse(SAMPLE);
    let (mut mvs, _) = std_line("e4");
    let again = mvs[0];
    mvs.push(again); // e2e4 again: illegal for Black
    let (more, _) = std_line("e4 c5");
    mvs.push(more[1]);
    assert!(
        book.classify_line(&Position::new(), &mvs, ClassifyOptions::game())
            .is_some_and(|m| m.ply == 0),
        "later moves must not be applied after an illegal one"
    );
}

#[test]
fn game_with_null_move_does_not_panic_and_keeps_earlier_match() {
    let book = parse(SAMPLE);
    let mut g = game("e4 c5");
    let tip = g.mainline().last().expect("node");
    let n = g.add_move(tip, Move::Null).expect("null");
    let pos = g.position_at(n).expect("pos").clone();
    let nf6 = pos.parse_san("Nf6", SanMode::Strict).expect("nf6").mv;
    g.add_move(n, nf6).expect("nf6");
    assert_eq!(
        found(&book, &g, ClassifyOptions::game()),
        Some((2, "B20".into()))
    );
}

#[test]
fn line_with_null_move_does_not_panic() {
    let book = parse(SAMPLE);
    let (mut mvs, _) = std_line("e4 c5");
    mvs.push(Move::Null);
    let m = book
        .classify_line(&Position::new(), &mvs, ClassifyOptions::game())
        .expect("match before the null");
    assert_eq!(m.ply, 2);
}

#[test]
fn null_move_first_gives_start_match_only() {
    let book = parse(SAMPLE);
    let (_, _) = std_line("e4");
    let m = book
        .classify_line(&Position::new(), &[Move::Null], ClassifyOptions::game())
        .expect("start");
    assert_eq!(m.ply, 0);
}

#[test]
fn game_starting_from_fen_is_classified() {
    let book = parse(SAMPLE);
    // Position after 1.e4 c5 as the game's start: plies count from the game start.
    let start = std_line("e4 c5").1;
    let g = game_from(start.clone(), "Nf3 d6");
    assert_eq!(
        found(&book, &g, ClassifyOptions::game()),
        Some((0, "B20".into()))
    );
    let book2 = parse("B20 \"Sicilian\" 1.e4 c5 *\nB50 \"Sicilian 2.Nf3 d6\" 1.e4 c5 2.Nf3 d6 *");
    assert_eq!(
        found(&book2, &g, ClassifyOptions::game()),
        Some((2, "B50".into()))
    );
}

#[test]
fn game_starting_from_unrelated_fen_has_no_match() {
    let book = parse(SAMPLE);
    let pos = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").expect("fen");
    let g = game_from(pos, "e4 Kd7");
    assert!(book.classify(&g, ClassifyOptions::game()).is_none());
}

/// 56 plies of knight shuffles (back at the start) followed by `tail`.
fn long_line(tail: &str) -> String {
    format!("{}{}", "Nf3 Nf6 Ng1 Ng8 ".repeat(14), tail)
}

#[test]
fn batch_mode_limits_search_to_sixty_plies() {
    // Entry X is reached at ply 60, Y at ply 61.
    let text = format!(
        "B00 \"X\" {} *\nB01 \"Y\" {} *\n",
        long_line("e4 e5 Nf3 Nc6"),
        long_line("e4 e5 Nf3 Nc6 Bb5")
    );
    let book = parse(&text);
    assert_eq!(book.len(), 2);
    let g = game(&long_line("e4 e5 Nf3 Nc6 Bb5"));
    assert_eq!(
        found(&book, &g, ClassifyOptions::game()),
        Some((61, "B01".into()))
    );
    assert_eq!(
        found(&book, &g, ClassifyOptions::batch()),
        Some((60, "B00".into()))
    );
    assert_eq!(
        found(&book, &g, ClassifyOptions::default()),
        Some((61, "B01".into()))
    );
}

#[test]
fn batch_mode_returns_none_when_only_match_is_beyond_sixty() {
    let text = format!("B01 \"Y\" {} *\n", long_line("e4 e5 Nf3 Nc6 Bb5"));
    let book = parse(&text);
    let g = game(&long_line("e4 e5 Nf3 Nc6 Bb5"));
    assert!(book.classify(&g, ClassifyOptions::batch()).is_none());
    assert_eq!(
        found(&book, &g, ClassifyOptions::game()),
        Some((61, "B01".into()))
    );
}

#[test]
fn classify_line_honours_batch_limit_too() {
    let text = format!("B01 \"Y\" {} *\n", long_line("e4 e5 Nf3 Nc6 Bb5"));
    let book = parse(&text);
    let (mvs, _) = std_line(&long_line("e4 e5 Nf3 Nc6 Bb5"));
    assert!(
        book.classify_line(&Position::new(), &mvs, ClassifyOptions::batch())
            .is_none()
    );
    assert!(
        book.classify_line(&Position::new(), &mvs, ClassifyOptions::game())
            .is_some()
    );
}

#[test]
fn match_at_exactly_sixty_plies_is_found_in_batch_mode() {
    let text = format!("B00 \"X\" {} *\n", long_line("e4 e5 Nf3 Nc6"));
    let book = parse(&text);
    let g = game(&long_line("e4 e5 Nf3 Nc6"));
    assert_eq!(
        found(&book, &g, ClassifyOptions::batch()),
        Some((60, "B00".into()))
    );
}

#[test]
fn eighty_ply_game_is_classified_without_limit() {
    let book = parse(SAMPLE);
    let g = game(&("e4 c5 ".to_owned() + &"Nf3 Nf6 Ng1 Ng8 ".repeat(19)));
    let m = book.classify(&g, ClassifyOptions::game()).expect("match");
    assert_eq!(
        m.ply,
        2 + 4 * 19,
        "game ends at a book position: 1.e4 c5 + full shuffles"
    );
}

#[test]
fn custom_max_ply_bounds_the_search_inclusively() {
    let book = parse(SAMPLE);
    let g = game("e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6");
    for (limit, expected) in [
        (0, Some((0, "A00a"))),
        (1, Some((0, "A00a"))),
        (2, Some((2, "B20"))),
        (9, Some((2, "B20"))),
        (10, Some((10, "B90"))),
        (500, Some((10, "B90"))),
    ] {
        let got = found(&book, &g, max_ply_opts(limit));
        assert_eq!(
            got,
            expected.map(|(p, c)| (p, c.to_owned())),
            "max_ply {limit}"
        );
    }
}

#[test]
fn match_eco_returns_basic_code_when_not_extended() {
    let book = parse("A00b \"Barnes\" 1.f3 *");
    let g = game("f3");
    let m = book.classify(&g, ClassifyOptions::game()).expect("match");
    assert_eq!(m.eco(true).to_string(), "A00b");
    assert_eq!(m.eco(false).to_string(), "A00");
}

#[test]
fn classification_options_defaults_match_the_spec() {
    assert_eq!(ClassifyOptions::game().max_ply, None);
    assert_eq!(ClassifyOptions::batch().max_ply, Some(60));
    assert_eq!(ClassifyOptions::default(), ClassifyOptions::game());
}

// ----------------------------------------------------------- summary/find

fn labels(entries: &[&mychess_core::EcoEntry]) -> Vec<String> {
    entries.iter().map(|e| e.eco().to_string()).collect()
}

#[test]
fn summary_with_empty_prefix_shows_one_entry_per_letter() {
    let book = parse(SAMPLE);
    assert_eq!(labels(&book.summary("")), ["A00a", "B20", "E20"]);
}

#[test]
fn summary_with_letter_shows_one_entry_per_tens() {
    let book = parse(SAMPLE);
    assert_eq!(labels(&book.summary("B")), ["B20", "B90"]);
    assert_eq!(labels(&book.summary("A")), ["A00a"]);
}

#[test]
fn summary_with_letter_and_digit_shows_one_per_code() {
    let book = parse(SAMPLE);
    assert_eq!(labels(&book.summary("B9")), ["B90", "B91", "B92"]);
}

#[test]
fn summary_with_full_basic_code_shows_all_extensions() {
    let book = parse(SAMPLE);
    assert_eq!(labels(&book.summary("B90")), ["B90", "B90a"]);
    assert_eq!(labels(&book.summary("A00")), ["A00a", "A00b", "A00c"]);
}

#[test]
fn summary_with_extended_prefix_filters_exactly() {
    let book = parse(SAMPLE);
    assert_eq!(labels(&book.summary("B90a")), ["B90a"]);
    assert_eq!(labels(&book.summary("A00b")), ["A00b"]);
}

#[test]
fn summary_of_unknown_prefix_is_empty() {
    let book = parse(SAMPLE);
    assert!(book.summary("C").is_empty());
    assert!(book.summary("Z99").is_empty());
}

#[test]
fn summary_preserves_insertion_order_not_code_order() {
    let book = parse("C00 \"c\" 1.e4 e6 *\nA00b \"a\" 1.f3 *\nB00 \"b\" 1.e4 *");
    assert_eq!(labels(&book.summary("")), ["C00", "A00b", "B00"]);
}

#[test]
fn summary_items_expose_name_and_moves() {
    let book = parse(SAMPLE);
    let items = book.summary("B90");
    assert_eq!(items[1].name(), "Najdorf, English");
    assert!(items[1].moves().starts_with("1.e4 c5"));
    assert!(items[1].moves().ends_with("6.Be3"));
}

#[test]
fn find_matches_code_and_name_text_case_sensitively() {
    let book = parse(SAMPLE);
    assert_eq!(labels(&book.find("Najdorf")), ["B90", "B90a", "B91", "B92"]);
    assert!(book.find("najdorf").is_empty(), "find is case sensitive");
    assert_eq!(labels(&book.find("B90")), ["B90", "B90a"]);
    assert_eq!(labels(&book.find("Zagreb")), ["B91"]);
    assert_eq!(labels(&book.find("[Start]")), ["A00a"]);
}

#[test]
fn find_does_not_search_the_moves() {
    let book = parse(SAMPLE);
    assert!(book.find("Nxd4").is_empty());
}

#[test]
fn find_without_match_is_empty() {
    assert!(parse(SAMPLE).find("Grob").is_empty());
}

// --------------------------------------------------------------- translate

#[test]
fn translate_replaces_every_occurrence() {
    let out = translate_eco_name("Queen Queen Gambit", &[("Queen", "Dame")]);
    assert_eq!(out, "Dame Dame Gambit");
}

#[test]
fn translate_applies_pairs_in_reverse_registration_order() {
    // The last registered pair is tried first (Scid prepends to its list).
    let glossary = [("Queen", "Dame"), ("Dame Opening", "Damenspiel")];
    assert_eq!(
        translate_eco_name("Queen Opening", &glossary),
        "Dame Opening"
    );
    // Forward order would have given "Damenspiel".
}

#[test]
fn translate_reverse_order_lets_earlier_pair_rewrite_later_output() {
    let glossary = [("B", "C"), ("A", "B")];
    // reverse: A->B first, then B->C
    assert_eq!(translate_eco_name("A", &glossary), "C");
}

#[test]
fn translate_handles_accented_names() {
    let glossary = [
        ("d\u{e9}fense", "Verteidigung"),
        ("Verteidigung sicilienne", "Sizilianisch"),
    ];
    assert_eq!(
        translate_eco_name("d\u{e9}fense sicilienne", &glossary),
        "Verteidigung sicilienne"
    );
    assert_eq!(
        translate_eco_name("T\u{fc}bingen", &[("\u{fc}", "ue")]),
        "Tuebingen"
    );
}

#[test]
fn translate_without_glossary_is_identity() {
    assert_eq!(translate_eco_name("Sicilian", &[]), "Sicilian");
    assert_eq!(translate_eco_name("", &[("a", "b")]), "");
}

#[test]
fn translate_with_empty_from_does_not_loop_or_panic() {
    assert_eq!(translate_eco_name("abc", &[("", "x")]), "abc");
}

// -------------------------------------------------------------- properties

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn parser_never_panics_on_arbitrary_text(text in ".{0,400}") {
        drop(EcoBook::parse(&text));
        let opts = skip_opts();
        drop(EcoBook::parse_with(&text, opts));
    }

    #[test]
    fn parser_never_panics_on_eco_like_noise(
        text in "(([A-E][0-9a-z]{0,3}|#|\"|\\*|[a-hKQRBNOkqrn1-8x=+#. \\n()-]){0,60}){0,12}"
    ) {
        drop(EcoBook::parse(&text));
        let opts = skip_opts();
        drop(EcoBook::parse_with(&text, opts));
    }

    #[test]
    fn classify_of_random_legal_game_is_coherent_with_lookup(
        choices in proptest::collection::vec(any::<u16>(), 0..70)
    ) {
        let book = parse(SAMPLE);
        // Walk a legal random game, biased to start in the book's territory.
        let mut pos = Position::new();
        let mut g = Game::new();
        let mut node = g.root();
        let mut positions = vec![pos.clone()];
        for c in &choices {
            let moves = pos.legal_moves();
            if moves.is_empty() { break; }
            let m = moves[usize::from(*c) % moves.len()];
            pos = pos.after(m).expect("legal");
            node = g.add_move(node, m).expect("add");
            positions.push(pos.clone());
        }
        let n = positions.len() as u32 - 1;
        for opts in [ClassifyOptions::game(), ClassifyOptions::batch()] {
            let got = book.classify(&g, opts);
            let limit = opts.max_ply.map_or(n, |m| m.min(n)) as usize;
            let expected = (0..=limit).rev().find_map(|p| book.lookup(&positions[p]).map(|e| (p as u32, e)));
            match (got, expected) {
                (None, None) => {}
                (Some(m), Some((p, e))) => {
                    prop_assert!(m.ply <= n);
                    prop_assert_eq!(m.ply, p);
                    prop_assert_eq!(m.entry().eco(), e.eco());
                    prop_assert_eq!(m.entry().position(), &positions[p as usize]);
                }
                (g, e) => prop_assert!(false, "classify {:?} vs lookup {:?}", g.map(|m| m.ply), e.map(|x| x.0)),
            }
        }
    }

    #[test]
    fn classify_line_agrees_with_classify(
        choices in proptest::collection::vec(any::<u16>(), 0..40)
    ) {
        let book = parse(SAMPLE);
        let mut pos = Position::new();
        let mut g = Game::new();
        let mut node = g.root();
        let mut mvs = Vec::new();
        for c in &choices {
            let moves = pos.legal_moves();
            if moves.is_empty() { break; }
            let m = moves[usize::from(*c) % moves.len()];
            pos = pos.after(m).expect("legal");
            node = g.add_move(node, m).expect("add");
            mvs.push(m);
        }
        let a = book.classify(&g, ClassifyOptions::game()).map(|m| (m.ply, m.entry().eco()));
        let b = book.classify_line(&Position::new(), &mvs, ClassifyOptions::game()).map(|m| (m.ply, m.entry().eco()));
        prop_assert_eq!(a, b);
    }
}

// ------------------------------------------------------------------ compat

fn scid_reference() -> std::path::PathBuf {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/scid_vs_pc-4.27");
    assert!(
        dir.is_dir(),
        "Scid vs PC reference not found at {} (see docs/referencia-scid.md)",
        dir.display()
    );
    dir
}

fn load_scid_eco() -> mychess_core::EcoReport {
    let bytes = std::fs::read(scid_reference().join("scid.eco")).expect("read scid.eco");
    let text = String::from_utf8(bytes).expect("scid.eco is UTF-8");
    EcoBook::parse_with(&text, EcoParseOptions::default()).expect("scid.eco parses")
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_eco_loads_scid_eco() {
    let r = load_scid_eco();
    assert_eq!(r.entries_read, 10_360);
    assert_eq!(r.book.len(), 10_360);
    assert_eq!(r.diagnostics, vec![], "no repeated positions, no errors");
    assert_eq!(r.book.fewest_pieces(), 21);
    let first = &r.book.entries()[0];
    assert_eq!(
        (first.eco().to_string(), first.name(), first.moves()),
        ("A00a".to_owned(), "Start", "")
    );
    assert!(first.position() == &Position::new());
    let basic = r
        .book
        .entries()
        .iter()
        .filter(|e| e.eco().to_string().len() == 3)
        .count();
    assert_eq!(basic, 2_537);
    assert_eq!(r.book.len() - basic, 7_823);
    assert!(r.book.entries().iter().all(|e| e.plies() <= 36));
    assert!(
        r.book.entries().iter().any(|e| e.name().contains('\u{fc}')),
        "umlauts preserved"
    );
    for ch in ['\u{e4}', '\u{f6}', '\u{fc}'] {
        assert!(
            r.book.entries().iter().any(|e| e.name().contains(ch)),
            "{ch}"
        );
    }
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_eco_every_entry_classifies_to_itself() {
    let book = load_scid_eco().book;
    for e in book.entries() {
        // Replay the entry's own line through the parser, as a Game.
        let one = EcoBook::parse(&format!("A00 \"t\" {} *", e.moves())).expect("reparse");
        assert_eq!(one.entries()[0].position(), e.position(), "{}", e.moves());
        let m = book
            .lookup(e.position())
            .unwrap_or_else(|| panic!("entry {} not found by lookup", e.eco()));
        assert_eq!(m.eco(), e.eco());
        assert_eq!(m.name(), e.name());
    }
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_eco_classifies_each_entry_line_with_its_own_length() {
    let book = load_scid_eco().book;
    for e in book.entries() {
        // Build the game from the entry's own moves via a fresh position walk.
        let mut pos = Position::new();
        let mut mvs = Vec::new();
        let mut g = Game::new();
        let mut node = g.root();
        for word in e.moves().split_whitespace() {
            let san: String = word
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.')
                .to_owned();
            if san.is_empty() {
                continue;
            }
            let mv = pos
                .parse_san(&san, SanMode::Tolerant)
                .unwrap_or_else(|x| panic!("{word}: {x}"))
                .mv;
            pos.play(mv).expect("play");
            node = g.add_move(node, mv).expect("add");
            mvs.push(mv);
        }
        let m = book.classify(&g, ClassifyOptions::game()).expect("match");
        assert_eq!(m.ply as usize, mvs.len(), "{} {}", e.eco(), e.moves());
        assert_eq!(m.entry().position(), e.position());
        assert_eq!(m.entry().eco(), e.eco(), "{}", e.moves());
        let m2 = book
            .classify_line(&Position::new(), &mvs, ClassifyOptions::batch())
            .expect("match");
        assert_eq!(m2.ply as usize, mvs.len());
    }
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_eco_spec_vectors_with_scid_eco() {
    let book = load_scid_eco().book;
    let cases: [(&str, u32, &str, &str); 8] = [
        (
            "e4 e5 Nf3 Nc6 Bb5 a6 Ba4 Nf6 O-O Be7 Re1 b5 Bb3 d6 c3 O-O h3",
            17,
            "C92a",
            "Spanish: Closed, 9.h3",
        ),
        (
            "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6",
            10,
            "B90a",
            "Sicilian: Najdorf",
        ),
        ("d4 Nf6 c4 e6 Nc3 Bb4", 6, "E20", "Nimzo-Indian Defence"),
        ("c4 e6 Nc3 Nf6 d4 Bb4", 6, "E20", "Nimzo-Indian Defence"),
        ("b4", 1, "A00p", "Polish (Sokolsky; Orang-Utan)"),
        ("a3 a6", 1, "A00f", "Anderssen Opening"),
        (
            "e4 e5 Nf3 Nc6 Bb5 a6 Ba4 Nf6 O-O Be7 Re1 b5 Bb3 O-O c3 d5 exd5 Nxd5 Nxe5 Nxe5 Rxe5 c6",
            22,
            "C89f",
            "Spanish: Marshall, 11.Rxe5 c6",
        ),
        ("Nf3 Nf6 Ng1 Ng8", 4, "A00a", "Start"),
    ];
    for (sans, ply, code, name) in cases {
        let m = book
            .classify(&game(sans), ClassifyOptions::game())
            .unwrap_or_else(|| panic!("no match for {sans}"));
        assert_eq!(
            (
                m.ply,
                m.entry().eco().to_string(),
                m.entry().name().to_owned()
            ),
            (ply, code.to_owned(), name.to_owned()),
            "{sans}"
        );
    }
    let m = book
        .classify(&Game::new(), ClassifyOptions::game())
        .expect("start");
    assert_eq!(
        (m.ply, m.entry().eco().to_string(), m.entry().name()),
        (0, "A00a".to_owned(), "Start")
    );
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_eco_summary_and_find_over_scid_eco() {
    let book = load_scid_eco().book;
    let top = book.summary("");
    assert_eq!(top.len(), 5, "one entry per letter A-E");
    assert_eq!(top[0].eco().to_string(), "A00a");
    let all_b9 = book.summary("B90");
    assert!(
        all_b9
            .iter()
            .all(|e| e.eco().to_string().starts_with("B90"))
    );
    assert!(!all_b9.is_empty());
    // Full-code prefix shows everything: same as filtering by hand.
    let by_hand = book
        .entries()
        .iter()
        .filter(|e| e.eco().to_string().starts_with("B90"))
        .count();
    assert_eq!(all_b9.len(), by_hand);
    assert!(!book.find("Najdorf").is_empty());
    assert!(
        book.find("Najdorf")
            .iter()
            .all(|e| e.name().contains("Najdorf") || e.eco().to_string().contains("Najdorf"))
    );
}
