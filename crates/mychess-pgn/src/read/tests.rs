//! End-to-end tests of the reader: tags, movetext, error rules, encodings.

use std::fmt::Write as _;
use std::io::Write;

use flate2::Compression;
use flate2::write::GzEncoder;
use mychess_core::{Game, GameResult, NodeId, Position, SanMode};
use pretty_assertions::assert_eq;
use rstest::rstest;

use super::{
    Diagnostic, DiagnosticKind, ParsedGame, PgnReader, PreGameText, ReadOptions, Severity,
    Termination, parse_movetext, parse_pgn_text,
};

fn parse_with(pgn: &str, opts: &ReadOptions) -> ParsedGame {
    let mut games = parse_pgn_text(pgn, opts);
    assert!(!games.is_empty(), "no game in {pgn:?}");
    games.remove(0)
}

fn parse(pgn: &str) -> ParsedGame {
    parse_with(pgn, &ReadOptions::default())
}

/// `[Event "t"]` header followed by `moves`, plus `*` unless the text already
/// ends with a result token.
fn with_header(moves: &str) -> ParsedGame {
    let ends_with_result = ["*", "1-0", "0-1", "1/2-1/2"]
        .iter()
        .any(|r| moves.trim_end().ends_with(r));
    let tail = if ends_with_result { "" } else { " *" };
    parse(&format!("[Event \"t\"]\n\n{moves}{tail}\n"))
}

/// Like [`with_header`] but never adds a result.
fn no_result(moves: &str) -> ParsedGame {
    parse(&format!("[Event \"t\"]\n\n{moves}\n"))
}

fn node_text(game: &Game, id: NodeId, out: &mut String) {
    let parent = game.parent(id).unwrap().unwrap();
    let mv = game.mv(id).unwrap();
    if let Some(before) = game.comment_before(id).unwrap() {
        write!(out, " {{<{before}}}").unwrap();
    }
    let san = game
        .position_at(parent)
        .unwrap()
        .san_with(mv, mychess_core::SanSuffix::None);
    write!(out, " {}", san.unwrap()).unwrap();
    for nag in game.nags(id).unwrap() {
        write!(out, " {nag}").unwrap();
    }
    if let Some(c) = game.comment(id).unwrap() {
        write!(out, " {{{c}}}").unwrap();
    }
}

fn rest_text(game: &Game, id: NodeId, out: &mut String) {
    let children = game.children(id).unwrap();
    let Some((&first, others)) = children.split_first() else {
        return;
    };
    node_text(game, first, out);
    for &variation in others {
        out.push_str(" (");
        node_text(game, variation, out);
        rest_text(game, variation, out);
        out.push_str(" )");
    }
    rest_text(game, first, out);
}

/// The tree as text: `{init} e4 e5 ( c5 Nf3 ) Nf3`, `$n` for NAGs, `{<c}` for
/// a comment before a move.
fn dump(game: &Game) -> String {
    let mut out = String::new();
    if let Some(c) = game.initial_comment() {
        write!(out, " {{{c}}}").unwrap();
    }
    rest_text(game, game.root(), &mut out);
    out.trim().to_owned()
}

fn kinds(g: &ParsedGame) -> Vec<DiagnosticKind> {
    g.diagnostics.iter().map(|d| d.kind.clone()).collect()
}

fn has(g: &ParsedGame, kind: &DiagnosticKind) -> bool {
    g.diagnostics.iter().any(|d| &d.kind == kind)
}

fn count(g: &ParsedGame, pred: impl Fn(&DiagnosticKind) -> bool) -> usize {
    g.diagnostics.iter().filter(|d| pred(&d.kind)).count()
}

// ----------------------------------------------------------------------
// A clean game
// ----------------------------------------------------------------------

const CLEAN: &str = "[Event \"Open\"]\n[Site \"Rome\"]\n[Date \"2024.05.01\"]\n[Round \"1\"]\n\
[White \"Carlsen, M\"]\n[Black \"Nepo, I\"]\n[Result \"1-0\"]\n\n\
1. e4 e5 2. Nf3 {develops} Nc6 3. Bb5 a6 1-0\n";

#[test]
fn clean_game_has_no_diagnostics() {
    let g = parse(CLEAN);
    assert_eq!(g.diagnostics, vec![]);
    assert_eq!(g.termination, Termination::ResultToken);
    assert_eq!(g.number, 1);
    assert_eq!(g.first_line, 1);
    assert_eq!(dump(&g.game), "e4 e5 Nf3 {develops} Nc6 Bb5 a6");
    let t = g.game.tags();
    assert_eq!(
        (t.event.as_str(), t.white.as_str(), t.round.as_str()),
        ("Open", "Carlsen, M", "1")
    );
    assert_eq!(t.result, GameResult::White);
    assert_eq!(g.byte_range, 0..(CLEAN.len() as u64 - 1));
}

#[test]
fn reader_yields_games_in_order_with_numbers_and_lines() {
    let pgn = format!("{CLEAN}\n{CLEAN}");
    let games: Vec<_> = PgnReader::new(pgn.as_bytes(), ReadOptions::default())
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(games.len(), 2);
    assert_eq!((games[0].number, games[1].number), (1, 2));
    assert_eq!(games[1].first_line, 11);
    assert!(games.iter().all(|g| g.diagnostics.is_empty()));
}

#[test]
fn reader_reports_progress() {
    let mut r = PgnReader::new(CLEAN.as_bytes(), ReadOptions::default());
    assert_eq!(r.bytes_consumed(), 0);
    r.next().unwrap().unwrap();
    assert_eq!(r.bytes_consumed(), CLEAN.len() as u64 - 1);
}

// ----------------------------------------------------------------------
// Results (D25, D22)
// ----------------------------------------------------------------------

#[rstest]
#[case("1-0", GameResult::White)]
#[case("0-1", GameResult::Black)]
#[case("1/2-1/2", GameResult::Draw)]
#[case("1/2", GameResult::Draw)]
#[case("*", GameResult::Unknown)]
#[case("1:0", GameResult::White)]
#[case("0:1", GameResult::Black)]
#[case("1/2:1/2", GameResult::Draw)]
#[case("½-½", GameResult::Draw)]
fn every_result_format_ends_the_game(#[case] token: &str, #[case] expected: GameResult) {
    let g = with_header(&format!("1. e4 e5 {token}"));
    assert_eq!(g.game.tags().result, expected);
    assert_eq!(g.termination, Termination::ResultToken);
    assert_eq!(g.diagnostics, vec![], "{token}");
    assert_eq!(dump(&g.game), "e4 e5");
}

#[test]
fn movetext_result_wins_over_the_tag() {
    let g = parse("[Event \"t\"]\n[Result \"1-0\"]\n\n1. e4 0-1\n");
    assert_eq!(g.game.tags().result, GameResult::Black);
    assert_eq!(kinds(&g), vec![DiagnosticKind::ResultMismatch]);
}

#[test]
fn tag_result_is_used_without_a_token_and_a_missing_tag_is_not_a_mismatch() {
    let g = parse("[Event \"t\"]\n[Result \"1-0\"]\n\n1. e4 e5");
    assert_eq!(g.game.tags().result, GameResult::White);
    assert_eq!(kinds(&g), vec![DiagnosticKind::MissingResult]);
    assert_eq!(g.termination, Termination::EndOfInput);
    let g = parse("[Event \"t\"]\n\n1. e4 1-0\n");
    assert_eq!(g.game.tags().result, GameResult::White);
    assert_eq!(g.diagnostics, vec![]);
}

#[test]
fn next_header_without_result_ends_the_game() {
    let games = parse_pgn_text(
        "[Event \"a\"]\n\n1. e4 e5\n[Event \"b\"]\n\n1. d4 *\n",
        &ReadOptions::default(),
    );
    assert_eq!(games.len(), 2);
    assert_eq!(games[0].termination, Termination::NextGameHeader);
    assert_eq!(kinds(&games[0]), vec![DiagnosticKind::MissingResult]);
    assert_eq!(dump(&games[0].game), "e4 e5");
    assert_eq!(games[1].termination, Termination::ResultToken);
    assert_eq!(games[1].number, 2);
}

// ----------------------------------------------------------------------
// Move tolerances (D15 of slice 1 through the reader)
// ----------------------------------------------------------------------

#[rstest]
#[case("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. 0-0 Be7", "O-O")]
#[case("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7", "O-O")]
#[case("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. OO Be7", "O-O")]
#[case("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. o-o Be7", "O-O")]
fn castling_spellings(#[case] text: &str, #[case] expected: &str) {
    let g = with_header(text);
    assert_eq!(g.diagnostics, vec![], "{text}");
    let last_white = g.game.mainline().nth(8).unwrap();
    let parent = g.game.parent(last_white).unwrap().unwrap();
    let san = g
        .game
        .position_at(parent)
        .unwrap()
        .san(g.game.mv(last_white).unwrap());
    assert_eq!(san.unwrap(), expected);
}

#[rstest]
#[case("1. e4 d5 2. exd5 Qxd5", "e4 d5 exd5 Qxd5")]
#[case("1. e4 d5 2. ed Qxd5", "e4 d5 exd5 Qxd5")]
#[case("1. Pe4 e5", "e4 e5")]
#[case("1. e2e4 e7e5", "e4 e5")]
#[case("1. Ng1-f3 Nb8-c6", "Nf3 Nc6")]
#[case("1. Nf3 Nf6 2. Ng1 Ng8", "Nf3 Nf6 Ng1 Ng8")]
#[case("1. e4 e5 2. Nf3 Nc6 3. Nbd2", "e4 e5 Nf3 Nc6")]
#[case("1. e4 e5 2. Z0 Z0 3. Nf3", "e4 e5 -- -- Nf3")]
#[case("1. e4 e5 2. -- -- 3. Nf3", "e4 e5 -- -- Nf3")]
#[case("1. e4 e5 2. e4 {x}", "e4 e5")]
#[case("1.e4 e5 2.Nf3 Nc6", "e4 e5 Nf3 Nc6")]
#[case("1. e4 e5 2... Nf3", "e4 e5 Nf3")]
#[case("1. e4 1... e5 2. Nf3 2... Nc6", "e4 e5 Nf3 Nc6")]
#[case("1. e4 . . e5", "e4 e5")]
fn move_spellings(#[case] text: &str, #[case] expected: &str) {
    let g = with_header(text);
    let got = dump(&g.game);
    // `Nbd2` is ambiguous-free here but illegal for Black's turn count; the
    // table only checks what was read.
    assert!(
        got.starts_with(expected.split(" {").next().unwrap()),
        "{text}: {got}"
    );
}

#[test]
fn promotion_spellings_and_en_passant_suffix() {
    let fen = "[Event \"t\"]\n[FEN \"8/P6k/8/8/8/8/p6K/8 w - - 0 1\"]\n\n";
    for (mv, expect) in [
        ("a8=Q", "a8=Q"),
        ("a8Q", "a8=Q"),
        ("a8", "a8=Q"),
        ("a8=N", "a8=N"),
    ] {
        let g = parse(&format!("{fen}1. {mv} *\n"));
        assert_eq!(g.diagnostics, vec![], "{mv}");
        assert_eq!(dump(&g.game), expect, "{mv}");
    }
    let g = parse("[Event \"t\"]\n\n1. e4 a6 2. e5 d5 3. exd6 e.p. *\n");
    assert_eq!(g.diagnostics, vec![]);
    assert_eq!(dump(&g.game), "e4 a6 e5 d5 exd6");
    let g = parse("[Event \"t\"]\n\n1. e4 a6 2. e5 d5 3. exd6 ep *\n");
    assert_eq!(dump(&g.game), "e4 a6 e5 d5 exd6");
    let g = parse("[Event \"t\"]\n\n1. e4 a6 2. e5 d5 3. exd6e.p. *\n");
    assert_eq!(dump(&g.game), "e4 a6 e5 d5 exd6");
}

#[test]
fn check_marks_are_ignored_everywhere() {
    let g = with_header("1. e4 e5 2. Qh5 Nc6 3. Bc4 Nf6 4. Qxf7# +  ++ # 1-0");
    assert_eq!(g.diagnostics, vec![]);
    assert_eq!(g.game.mainline().count(), 7);
}

#[test]
fn strict_mode_rejects_non_canonical_moves() {
    let opts = ReadOptions {
        san_mode: SanMode::Strict,
        ..ReadOptions::default()
    };
    let g = parse_with(
        "[Event \"t\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. 0-0 *\n",
        &opts,
    );
    assert!(
        matches!(g.diagnostics[0].kind, DiagnosticKind::InvalidMove(_)),
        "{:?}",
        g.diagnostics
    );
    assert_eq!(g.game.mainline().count(), 8);
}

// ----------------------------------------------------------------------
// Glyphs, NAGs and suffixes (D22, rule 18)
// ----------------------------------------------------------------------

#[rstest]
#[case("1. e4! e5", "e4 $1 e5")]
#[case("1. e4!! e5", "e4 $3 e5")]
#[case("1. e4!? e5", "e4 $5 e5")]
#[case("1. e4?! e5", "e4 $6 e5")]
#[case("1. e4? e5??", "e4 $2 e5 $4")]
#[case("1. e4 ! e5", "e4 $1 e5")]
#[case("1. e4 $14 e5 $3", "e4 $14 e5 $3")]
#[case("1. e4 $14 $14 $1 e5", "e4 $14 $1 e5")]
#[case("1. e4$1 e5", "e4 $1 e5")]
#[case("1. e4 += e5", "e4 $14 e5")]
#[case("1. e4 +- e5", "e4 $18 e5")]
#[case("1. e4+- e5", "e4 $18 e5")]
#[case("1. e4!!+- e5", "e4 $3 $18 e5")]
#[case("1. e4!+ e5", "e4 $1 e5")]
#[case("1. e4 N e5", "e4 $146 e5")]
#[case("1. e4 RR e5", "e4 $145 e5")]
#[case("1. e4 BB e5", "e4 $151 e5")]
#[case("1. e4 Bb e5", "e4 $153 e5")]
#[case("1. e4 D e5", "e4 $201 e5")]
#[case("1. e4 ZZ e5", "e4 $22 e5")]
#[case("1. e4 =/& e5", "e4 $44 e5")]
#[case("1. e4 $0 e5", "e4 e5")]
fn glyphs_and_nags(#[case] text: &str, #[case] expected: &str) {
    let g = with_header(text);
    assert_eq!(g.diagnostics, vec![], "{text}");
    assert_eq!(dump(&g.game), expected, "{text}");
}

#[test]
fn nag_above_255_warns_and_is_dropped() {
    let g = with_header("1. e4 $256 e5 $255");
    assert_eq!(kinds(&g), vec![DiagnosticKind::NagOutOfRange]);
    assert_eq!(dump(&g.game), "e4 e5 $255");
}

#[test]
fn nag_without_a_move_is_dropped_with_a_warning() {
    let g = with_header("$14 1. e4 ( $1 1. d4 ) e5");
    assert_eq!(count(&g, |k| *k == DiagnosticKind::NagWithoutMove), 2);
    assert_eq!(dump(&g.game), "e4 ( d4 ) e5");
}

#[test]
fn unknown_symbols_after_a_move_warn() {
    let g = with_header("1. e4^ e5");
    assert!(
        has(&g, &DiagnosticKind::InvalidNagSymbol),
        "{:?}",
        g.diagnostics
    );
    assert_eq!(dump(&g.game), "e4 e5");
}

#[test]
fn dollar_without_digits_is_an_invalid_symbol() {
    let g = with_header("1. e4 $ e5");
    assert_eq!(kinds(&g), vec![DiagnosticKind::InvalidNagSymbol]);
    assert_eq!(dump(&g.game), "e4 e5");
}

#[rstest]
#[case("C")]
#[case("e9")]
#[case("xyz")]
#[case("Nz3")]
fn unexpected_tokens_do_not_stop_the_line(#[case] token: &str) {
    let g = with_header(&format!("1. e4 {token} e5 2. Nf3"));
    assert_eq!(kinds(&g), vec![DiagnosticKind::UnexpectedToken], "{token}");
    assert_eq!(g.diagnostics[0].text.as_deref(), Some(token));
    assert_eq!(dump(&g.game), "e4 e5 Nf3");
    assert!(
        g.game.initial_comment().is_none(),
        "no 'Error reading move' comment"
    );
}

// ----------------------------------------------------------------------
// Illegal moves (D21)
// ----------------------------------------------------------------------

#[test]
fn illegal_move_adds_the_error_comment_and_drops_the_rest_of_the_line() {
    let g = with_header("1. e4 e5 2. Nf6 Nc6 3. Bb5 *");
    assert_eq!(dump(&g.game), "e4 e5 {Error reading move: Nf6}");
    assert_eq!(g.diagnostics.len(), 1);
    let d = &g.diagnostics[0];
    assert!(matches!(d.kind, DiagnosticKind::IllegalMove(_)));
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.text.as_deref(), Some("Nf6"));
    assert_eq!((d.line, d.column), (3, 13));
    // The result token still ends the game.
    assert_eq!(g.termination, Termination::ResultToken);
}

#[test]
fn illegal_move_in_a_variation_spares_the_other_lines() {
    let g = with_header("1. e4 e5 ( 1... c5 2. Ke5 Nf3 ) ( 1... e6 2. d4 ) 2. Nf3 Nc6 *");
    assert_eq!(
        dump(&g.game),
        "e4 e5 ( c5 {Error reading move: Ke5} ) ( e6 d4 ) Nf3 Nc6"
    );
    assert_eq!(g.diagnostics.len(), 1);
}

#[test]
fn only_the_first_error_of_a_game_adds_a_comment() {
    let g = with_header(
        "1. e4 e5 ( 1... c5 2. Ke5 ) 2. Nf3 Nc6 ( 2... Nf6 3. Ke5 ) 3. Bb5 Qg5 4. Qg5 *",
    );
    let comments = dump(&g.game).matches("Error reading move").count();
    assert_eq!(comments, 1, "{}", dump(&g.game));
    let errors = count(&g, |k| matches!(k, DiagnosticKind::IllegalMove(_)));
    assert_eq!(errors, 3);
}

#[test]
fn variations_inside_the_discarded_remainder_are_skipped() {
    let g = with_header("1. e4 e5 2. Nf6 ( 2. Nf3 ( 2. d4 ) Nc6 ) 3. Bb5 a6 ( 3... Nf6 ) 4. Ba4 *");
    assert_eq!(dump(&g.game), "e4 e5 {Error reading move: Nf6}");
    assert_eq!(g.diagnostics.len(), 1, "{:?}", g.diagnostics);
}

#[test]
fn nested_error_does_not_stop_the_outer_line() {
    let g = with_header("1. e4 e5 ( 1... c5 ( 1... e6 2. Zz9 ) 2. Nf3 ) 2. Nf3 *");
    assert_eq!(dump(&g.game), "e4 e5 ( c5 Nf3 ) ( e6 ) Nf3");
    assert_ne!(g.diagnostics, vec![]);
}

#[test]
fn error_at_the_first_move_goes_to_the_initial_comment() {
    let g = with_header("1. Nf6 e5 *");
    assert_eq!(dump(&g.game), "{Error reading move: Nf6}");
    let g = with_header("1. e4 e5 ( 1... Ke5 ) 2. Nf3 *");
    assert_eq!(dump(&g.game), "e4 {Error reading move: Ke5} e5 Nf3");
}

#[test]
fn ambiguous_move_is_an_error_of_its_own_kind() {
    let g = parse("[Event \"t\"]\n[FEN \"4k3/8/8/8/8/2N3N1/8/4K3 w - - 0 1\"]\n\n1. Ne4 *\n");
    assert!(
        matches!(g.diagnostics[0].kind, DiagnosticKind::AmbiguousMove(_)),
        "{:?}",
        g.diagnostics
    );
}

#[test]
fn castling_after_the_right_was_lost_is_kept_with_a_warning() {
    // The king moved and came back, so O-O is no longer legal. Like the Scid,
    // the reader keeps the move and reports it.
    let g = with_header("1. e4 e5 2. Ke2 Ke7 3. Ke1 Ke8 4. Nf3 Nf6 5. Be2 Be7 6. O-O O-O 7. d3 *");
    assert_eq!(kinds(&g), vec![DiagnosticKind::IllegalCastling; 2]);
    assert!(
        g.diagnostics
            .iter()
            .all(|d| d.severity == Severity::Warning)
    );
    let d = dump(&g.game);
    assert!(d.contains("O-O O-O d3"), "{d}");
    assert!(!d.contains("Error reading move"), "{d}");
}

#[test]
fn junk_tokens_are_unexpected_without_trying_san() {
    for junk in ["xyz", "Ä4", "e4e4e4e4e4e4e4e4e4", "99"] {
        let g = with_header(&format!("1. e4 {junk} e5 *"));
        assert!(
            kinds(&g)
                .iter()
                .all(|k| *k == DiagnosticKind::UnexpectedToken),
            "{junk}: {:?}",
            g.diagnostics
        );
        assert!(dump(&g.game).contains("e4 e5"), "{junk}: {}", dump(&g.game));
    }
}

#[test]
fn diagnostics_are_capped_per_game() {
    let moves = "xyz ".repeat(300);
    let g = with_header(&moves);
    assert_eq!(g.diagnostics.len(), 101);
    assert_eq!(g.diagnostics[100].kind, DiagnosticKind::TooManyDiagnostics);
    let opts = ReadOptions {
        max_diagnostics_per_game: 3,
        ..ReadOptions::default()
    };
    let g = parse_with(&format!("[Event \"t\"]\n\n{moves}"), &opts);
    assert_eq!(g.diagnostics.len(), 4);
}

// ----------------------------------------------------------------------
// Variations (D29, rule 20)
// ----------------------------------------------------------------------

#[test]
fn nested_variations_five_levels() {
    let g = with_header(
        "1. e4 e5 ( 1... c5 2. Nf3 ( 2. c3 d5 ( 2... Nf6 3. e5 ( 3. d3 g6 ( 3... e6 4. d4 ( 4. g3 ) ) ) ) ) d6 ) 2. Nf3 *",
    );
    assert_eq!(g.diagnostics, vec![], "{:?}", g.diagnostics);
    assert_eq!(
        dump(&g.game),
        "e4 e5 ( c5 Nf3 ( c3 d5 ( Nf6 e5 ( d3 g6 ( e6 d4 ( g3 ) ) ) ) ) d6 ) Nf3"
    );
}

#[test]
fn variation_is_an_alternative_to_the_last_move() {
    let g = with_header("1. e4 e5 2. Nf3 ( 2. Bc4 Nf6 ) 2... Nc6 *");
    assert_eq!(dump(&g.game), "e4 e5 Nf3 ( Bc4 Nf6 ) Nc6");
}

#[test]
fn variation_before_the_first_move_is_skipped() {
    let g = with_header("( 1. d4 d5 ) 1. e4 e5");
    assert_eq!(kinds(&g), vec![DiagnosticKind::VariationBeforeFirstMove]);
    assert_eq!(dump(&g.game), "e4 e5");
}

#[test]
fn stray_close_paren_is_ignored() {
    let g = with_header("1. e4 ) e5 ) *");
    assert_eq!(count(&g, |k| *k == DiagnosticKind::UnbalancedCloseParen), 2);
    assert_eq!(dump(&g.game), "e4 e5");
}

#[test]
fn variation_open_at_the_end_is_closed() {
    let g = no_result("1. e4 e5 ( 1... c5 2. Nf3");
    assert!(has(&g, &DiagnosticKind::UnclosedVariation));
    assert!(has(&g, &DiagnosticKind::MissingResult));
    assert_eq!(dump(&g.game), "e4 e5 ( c5 Nf3 )");
}

#[test]
fn result_inside_a_variation_ends_the_game() {
    let g = with_header("1. e4 e5 ( 1... c5 1-0 2. Nf3 ) 2. Nf3");
    assert_eq!(g.termination, Termination::ResultToken);
    assert!(has(&g, &DiagnosticKind::UnclosedVariation));
    assert_eq!(g.game.tags().result, GameResult::White);
    assert_eq!(dump(&g.game), "e4 e5 ( c5 )");
}

#[test]
fn duplicate_variation_is_merged_with_a_warning() {
    let g = with_header("1. e4 e5 ( 1... e5 {same} ) 2. Nf3 *");
    assert_eq!(kinds(&g), vec![DiagnosticKind::DuplicateVariation]);
    assert_eq!(dump(&g.game), "e4 e5 {same} Nf3");
}

#[test]
fn thousands_of_nested_variations_do_not_overflow_the_stack() {
    let depth = 4000;
    // 1. Nf3 ( 1. Nc3 ( 1. Nf3 ( ... ) ) ): each level alternates.
    let mut text = String::from("1. e4 ");
    for i in 0..depth {
        text.push_str(if i % 2 == 0 { "( 1. d4 " } else { "( 1. e4 " });
    }
    for _ in 0..depth {
        text.push_str(") ");
    }
    text.push_str("e5 *");
    let g = with_header(&text);
    assert_eq!(g.termination, Termination::ResultToken);
    assert!(g.diagnostics.len() <= 101);
}

#[test]
fn many_unbalanced_parentheses_are_cheap() {
    let text = format!("1. e4 {} e5 *", "(".repeat(200_000));
    let g = with_header(&text);
    assert_eq!(g.game.mainline().count(), 1);
    assert!(g.diagnostics.len() <= 101);
    let g = with_header(&format!("1. e4 {} e5 *", ")".repeat(200_000)));
    assert_eq!(g.game.mainline().count(), 2);
}

// ----------------------------------------------------------------------
// Comments (D20, rule 19)
// ----------------------------------------------------------------------

#[test]
fn comments_attach_to_the_last_move_and_join() {
    let g = with_header("{start} 1. e4 {one} {two} e5 { three\n   four } *");
    assert_eq!(dump(&g.game), "{start} e4 {one two} e5 {three four}");
}

#[test]
fn comment_after_open_paren_is_comment_before() {
    let g = with_header("1. e4 e5 ( {why not} 1... c5 {sharp} ) *");
    assert_eq!(dump(&g.game), "e4 e5 ( {<why not} c5 {sharp} )");
}

#[test]
fn comment_after_a_variation_belongs_to_the_move_before_it() {
    let g = with_header("1. e4 e5 ( 1... c5 ) {late} 2. Nf3 *");
    assert_eq!(dump(&g.game), "e4 e5 {late} ( c5 ) Nf3");
}

#[test]
fn semicolon_comments_are_kept_or_dropped() {
    let pgn = "[Event \"t\"]\n\n1. e4 ; after e4\ne5 ; and e5\n2. Nf3 *\n";
    let g = parse(pgn);
    assert_eq!(dump(&g.game), "e4 {after e4} e5 {and e5} Nf3");
    let opts = ReadOptions {
        semicolon_comments: super::SemicolonComments::Drop,
        ..ReadOptions::default()
    };
    assert_eq!(dump(&parse_with(pgn, &opts).game), "e4 e5 Nf3");
}

#[test]
fn percent_lines_are_discarded_only_at_column_zero() {
    let g = with_header("1. e4\n% [Event \"x\"] 1-0\ne5\n 2. Nf3 *");
    assert_eq!(g.diagnostics, vec![]);
    assert_eq!(dump(&g.game), "e4 e5 Nf3");
    // Not at column 0: `%` is part of a word and is reported.
    let g = with_header("1. e4 %x e5 *");
    assert_eq!(kinds(&g), vec![DiagnosticKind::UnexpectedToken]);
}

#[test]
fn clock_line_inside_a_multiline_comment_does_not_end_the_game() {
    let g = parse(
        "[Event \"t\"]\n\n1. e4 {[%clk 0:03:00]\n[%eval 0.3]\n} e5 {x\n[%clk 0:02:59]} 2. Nf3 *\n",
    );
    assert_eq!(g.diagnostics, vec![]);
    assert_eq!(g.game.mainline().count(), 3);
}

#[test]
fn comment_whitespace_modes() {
    let pgn = "[Event \"t\"]\n\n1. e4 {  two   words\r\n and   more  } *\n";
    assert_eq!(dump(&parse(pgn).game), "e4 {two words and more}");
    let opts = ReadOptions {
        comment_whitespace: super::CommentWhitespace::Preserve,
        ..ReadOptions::default()
    };
    assert_eq!(
        dump(&parse_with(pgn, &opts).game),
        "e4 {two   words\n and   more}"
    );
}

#[test]
fn braces_do_not_nest_and_stray_close_brace_warns_once() {
    let g = with_header("1. e4 {a { b} e5 } } *");
    assert_eq!(dump(&g.game), "e4 {a { b} e5");
    assert_eq!(kinds(&g), vec![DiagnosticKind::StrayCloseBrace]);
}

#[test]
fn unterminated_comment_keeps_its_text() {
    let g = no_result("1. e4 e5 {never closed 2. Nf3");
    assert_eq!(dump(&g.game), "e4 e5 {never closed 2. Nf3}");
    assert_eq!(
        kinds(&g),
        vec![
            DiagnosticKind::UnterminatedComment,
            DiagnosticKind::MissingResult
        ]
    );
}

#[test]
fn comment_with_delimiters_glued_to_moves() {
    let g = with_header("1.e4{good}e5(1...c5)2.Nf3 *");
    assert_eq!(g.diagnostics, vec![]);
    assert_eq!(dump(&g.game), "e4 {good} e5 ( c5 ) Nf3");
}

#[test]
fn huge_comments_are_truncated_with_a_warning() {
    let opts = ReadOptions {
        max_comment_len: 100,
        ..ReadOptions::default()
    };
    let big = "word ".repeat(5000);
    let g = parse_with(&format!("[Event \"t\"]\n\n1. e4 {{{big}}} e5 *\n"), &opts);
    assert_eq!(kinds(&g), vec![DiagnosticKind::CommentTruncated]);
    let comment = g
        .game
        .comment(g.game.mainline().next().unwrap())
        .unwrap()
        .unwrap();
    assert!(
        comment.len() <= 100 && comment.starts_with("word word"),
        "{}",
        comment.len()
    );
}

#[test]
fn a_megabyte_comment_is_fast_and_kept() {
    let big = "x ".repeat(400_000);
    let g = parse(&format!("[Event \"t\"]\n\n1. e4 {{{big}}} e5 *\n"));
    assert_eq!(g.diagnostics, vec![]);
    let c = g
        .game
        .comment(g.game.mainline().next().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(c.len(), 799_999);
}

#[test]
fn many_comments_on_the_same_move_are_linear() {
    let text = "{c} ".repeat(100_000);
    let g = with_header(&format!("1. e4 {text} *"));
    let c = g
        .game
        .comment(g.game.mainline().next().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(c.len(), 100_000 * 2 - 1);
}

// ----------------------------------------------------------------------
// Tags (D23, D24)
// ----------------------------------------------------------------------

#[test]
fn tag_variants() {
    let g = parse(
        "[Event \"a \\\"quoted\\\" \\\\ name\"]\n[Site \"Rome\"] [Round \"7\"]\n[White \"Doe, \"Jack\"\"]\n\
[Black \"x\"]\n[Black \"y\"]\n[Date \"2024.13.40\"]\n[ECO \"Z99\"]\n[Result \"garbage\"]\n[Odd]\n\n1. e4 *\n",
    );
    let t = g.game.tags();
    assert_eq!(t.event, "a \"quoted\" \\ name");
    assert_eq!((t.site.as_str(), t.round.as_str()), ("Rome", "7"));
    assert_eq!(t.white, "Doe, \"Jack\"");
    assert_eq!(t.black, "y");
    assert_eq!(t.date.to_string(), "2024.??.??");
    assert_eq!(t.eco, None);
    assert_eq!(t.result, GameResult::Unknown);
    assert_eq!(kinds(&g), vec![DiagnosticKind::BadTag]);
    assert_eq!((g.diagnostics[0].line, g.diagnostics[0].column), (9, 1));
}

#[test]
fn player_ratings_and_names() {
    let g = parse(
        "[White \"Kasparov,G (2800)\"]\n[Black \"X (9999)\"]\n[WhiteRapid \"2750\"]\n[BlackElo \"2000\"]\n[BlackUSCF \"1900\"]\n\n*\n",
    );
    let t = g.game.tags();
    assert_eq!(t.white, "Kasparov, G");
    assert_eq!(
        t.white_rating.map(|r| (r.value.get(), r.kind.name())),
        Some((2800, "Elo"))
    );
    assert_eq!(t.black, "X");
    assert_eq!(t.black_rating.map(|r| r.value.get()), Some(4000));
    assert_eq!(t.get_extra("WhiteRapid"), Some("2750"));
    assert_eq!(t.get_extra("BlackElo"), Some("2000"));
    assert_eq!(t.get_extra("BlackUSCF"), Some("1900"));
    assert_eq!(kinds(&g), vec![DiagnosticKind::RatingTooLarge]);
}

#[test]
fn only_the_first_rating_type_is_the_rating() {
    let g = parse("[WhiteElo \"2700\"]\n[WhiteRapid \"2650\"]\n\n*\n");
    let t = g.game.tags();
    assert_eq!(t.white_rating.map(|r| r.value.get()), Some(2700));
    assert_eq!(t.get_extra("WhiteRapid"), Some("2650"));
}

#[test]
fn enormous_tag_name_and_value_are_truncated_with_a_warning() {
    let name = "N".repeat(5000);
    let value = "v".repeat(200_000);
    let g = parse(&format!("[Event \"t\"]\n[{name} \"{value}\"]\n\n1. e4 *\n"));
    assert_eq!(count(&g, |k| *k == DiagnosticKind::TagTooLong), 1);
    let (n, v) = &g.game.tags().extras()[0];
    assert_eq!((n.len(), v.len()), (1024, 64 * 1024));
}

#[test]
fn ignored_tags_option() {
    let opts = ReadOptions {
        ignored_tags: vec!["PlyCount".to_owned()],
        ..ReadOptions::default()
    };
    let g = parse_with("[PlyCount \"2\"]\n[Other \"1\"]\n\n*\n", &opts);
    assert_eq!(
        g.game.tags().extras(),
        [("Other".to_owned(), "1".to_owned())]
    );
}

#[test]
fn tags_on_several_lines_with_percent_and_semicolon_lines() {
    let g = parse("% a comment line\n[Event \"A\"]\n; another\n\n[Site \"B\"]\n\n1. e4 *\n");
    assert_eq!(
        (g.game.tags().event.as_str(), g.game.tags().site.as_str()),
        ("A", "B")
    );
    assert_eq!(
        g.diagnostics
            .iter()
            .filter(|d| d.severity != Severity::Info)
            .count(),
        0
    );
}

#[test]
fn scan_tags_returns_raw_pairs_without_reading_moves() {
    let mut chunker = super::GameChunker::new(
        &b"[Event \"A\"]\n[Event \"B\"]\n[White \"x, y (2000)\"]\n\n1. e4 Zz *\n"[..],
        1 << 20,
    );
    let raw = chunker.next().unwrap().unwrap();
    assert_eq!(
        raw.scan_tags(),
        vec![
            ("Event".to_owned(), "A".to_owned()),
            ("Event".to_owned(), "B".to_owned()),
            ("White".to_owned(), "x, y (2000)".to_owned()),
        ]
    );
}

// ----------------------------------------------------------------------
// FEN, variants (D26)
// ----------------------------------------------------------------------

const KP: &str = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";

#[rstest]
#[case(true)]
#[case(false)]
fn fen_defines_the_start_position_with_or_without_setup(#[case] setup: bool) {
    let set_up = if setup { "[SetUp \"1\"]\n" } else { "" };
    let g = parse(&format!(
        "[Event \"t\"]\n{set_up}[FEN \"{KP}\"]\n\n1. e4 Kd7 *\n"
    ));
    assert_eq!(g.diagnostics, vec![]);
    assert_eq!(g.game.start_position().fen(), KP);
    assert_eq!(dump(&g.game), "e4 Kd7");
    assert_eq!(g.game.tags().extras(), []);
}

#[test]
fn setup_zero_does_not_prevent_the_fen() {
    let g = parse(&format!("[SetUp \"0\"]\n[FEN \"{KP}\"]\n\n1. e4 *\n"));
    assert_eq!(g.game.start_position().fen(), KP);
}

#[test]
fn invalid_fen_comments_and_starts_from_the_standard_position() {
    let g = parse("[Event \"t\"]\n[SetUp \"1\"]\n[FEN \"not a fen\"]\n\n1. e4 e5 *\n");
    assert_eq!(kinds(&g), vec![DiagnosticKind::InvalidFen]);
    assert_eq!(g.diagnostics[0].severity, Severity::Error);
    assert_eq!(g.game.start_position(), &Position::new());
    assert_eq!(dump(&g.game), "{Error Invalid FEN: not a fen} e4 e5");
}

#[test]
fn unsupported_variant_returns_only_the_header() {
    let g = parse("[Event \"t\"]\n[Variant \"Crazyhouse\"]\n\n1. e4 e5 2. Nf3 *\n");
    assert_eq!(g.termination, Termination::UnsupportedVariant);
    assert_eq!(kinds(&g), vec![DiagnosticKind::UnsupportedVariant]);
    assert_eq!(g.game.node_count(), 1);
    assert_eq!(g.game.tags().get_extra("Variant"), Some("Crazyhouse"));
    let opts = ReadOptions {
        unsupported_variant: super::UnsupportedVariant::TryStandard,
        ..ReadOptions::default()
    };
    let g = parse_with(
        "[Event \"t\"]\n[Variant \"Crazyhouse\"]\n\n1. e4 e5 *\n",
        &opts,
    );
    assert_eq!(g.game.mainline().count(), 2);
    let g = parse("[Event \"t\"]\n[Variant \"Standard\"]\n\n1. e4 e5 *\n");
    assert_eq!(g.game.mainline().count(), 2);
}

// ----------------------------------------------------------------------
// Empty games, junk, end of file (D27, D28)
// ----------------------------------------------------------------------

#[test]
fn header_only_game_with_result_is_valid() {
    let g = parse("[Event \"t\"]\n[Result \"1-0\"]\n\n1-0\n");
    assert_eq!(g.diagnostics, vec![]);
    assert_eq!(g.game.node_count(), 1);
    assert_eq!(g.game.tags().result, GameResult::White);
    let g = parse("[Event \"t\"]\n\n*\n");
    assert_eq!(g.diagnostics, vec![]);
}

#[test]
fn header_at_the_end_of_the_file() {
    let games = parse_pgn_text(
        "[Event \"a\"]\n\n1. e4 *\n\n[Event \"b\"]\n[Site \"s\"]\n",
        &ReadOptions::default(),
    );
    assert_eq!(games.len(), 2);
    assert_eq!(kinds(&games[1]), vec![DiagnosticKind::HeaderOnlyAtEof]);
    assert_eq!(games[1].termination, Termination::EndOfInput);
    assert_eq!(games[1].game.tags().site, "s");
}

#[test]
fn consecutive_headers_form_one_game_and_the_last_value_wins() {
    let g = parse("[Event \"a\"]\n[Event \"b\"]\n\n1. e4 *\n");
    assert_eq!(g.game.tags().event, "b");
    assert_eq!(g.diagnostics, vec![]);
}

#[test]
fn header_without_blank_line_before_the_moves() {
    let g = parse("[Event \"a\"]\n1. e4 e5 *\n");
    assert_eq!(g.diagnostics, vec![]);
    assert_eq!(dump(&g.game), "e4 e5");
}

#[test]
fn junk_between_games_is_ignored_with_an_info_diagnostic() {
    let pgn =
        "preface\nline two\n[Event \"a\"]\n\n1. e4 *\nnoise between\n\n[Event \"b\"]\n\n1. d4 *\n";
    let games = parse_pgn_text(pgn, &ReadOptions::default());
    assert_eq!(games.len(), 2);
    for g in &games {
        assert_eq!(kinds(g), vec![DiagnosticKind::JunkBetweenGames]);
        assert_eq!(g.diagnostics[0].severity, Severity::Info);
        assert!(g.game.initial_comment().is_none());
    }
    assert_eq!(
        (games[0].diagnostics[0].line, games[0].diagnostics[0].column),
        (1, 1)
    );
    assert_eq!(games[0].pre_game_text, None, "Ignore keeps no text");
    assert_eq!(games[0].first_line, 3);
    assert_eq!(
        (games[1].diagnostics[0].line, games[1].diagnostics[0].column),
        (6, 1)
    );
    assert_eq!(games[1].pre_game_text, None);
    assert_eq!(games[1].first_line, 8);
    assert_eq!((games[0].number, games[1].number), (1, 2));
}

#[test]
fn junk_as_comment_option() {
    let opts = ReadOptions {
        pre_game_text: PreGameText::AsComment,
        ..ReadOptions::default()
    };
    let g = parse_with(
        "note one\nnote two\n[Event \"a\"]\n\n{moves} 1. e4 *\n",
        &opts,
    );
    assert_eq!(dump(&g.game), "{note one note two moves} e4");
}

#[test]
fn blank_text_between_games_is_not_junk() {
    let games = parse_pgn_text(
        "[Event \"a\"]\n\n*\n\n\n   \n[Event \"b\"]\n\n*\n",
        &ReadOptions::default(),
    );
    assert!(games.iter().all(|g| g.diagnostics.is_empty()));
}

#[test]
fn text_without_any_header_gives_no_game() {
    assert!(parse_pgn_text("1. e4 e5 *\n", &ReadOptions::default()).is_empty());
    assert!(parse_pgn_text("", &ReadOptions::default()).is_empty());
}

// ----------------------------------------------------------------------
// Line breaks, BOM, positions (D19, rule 8)
// ----------------------------------------------------------------------

fn positions(g: &ParsedGame) -> Vec<(DiagnosticKind, u64, u32, u64)> {
    g.diagnostics
        .iter()
        .map(|d| (d.kind.clone(), d.line, d.column, d.offset))
        .collect()
}

const WITH_ERRORS: &str = "[Event \"t\"]\n[Bad\n\n1. e4 xyz e5\n2. Nf3 Qq9 Nc6 *\n";

#[test]
fn diagnostic_positions_are_in_the_original_bytes() {
    let g = parse(WITH_ERRORS);
    assert_eq!(
        positions(&g),
        vec![
            (DiagnosticKind::BadTag, 2, 1, 12),
            (DiagnosticKind::UnexpectedToken, 4, 7, 24),
            (DiagnosticKind::UnexpectedToken, 5, 8, 38),
        ]
    );
}

#[rstest]
#[case("\r\n")]
#[case("\r")]
fn crlf_and_cr_give_the_same_game_and_lines(#[case] newline: &str) {
    let lf = parse(WITH_ERRORS);
    let other = parse(&WITH_ERRORS.replace('\n', newline));
    assert_eq!(dump(&other.game), dump(&lf.game));
    let lines = |g: &ParsedGame| {
        g.diagnostics
            .iter()
            .map(|d| (d.kind.clone(), d.line))
            .collect::<Vec<_>>()
    };
    assert_eq!(lines(&other), lines(&lf));
    // Columns are bytes of the original line: same here, offsets grow with CRLF.
    let cols = |g: &ParsedGame| g.diagnostics.iter().map(|d| d.column).collect::<Vec<_>>();
    assert_eq!(cols(&other), cols(&lf));
}

#[test]
fn bom_is_removed_and_offsets_count_it() {
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(WITH_ERRORS.as_bytes());
    let games: Vec<_> = PgnReader::new(&bytes[..], ReadOptions::default())
        .collect::<Result<_, _>>()
        .unwrap();
    let plain = parse(WITH_ERRORS);
    let a: Vec<_> = games[0]
        .diagnostics
        .iter()
        .map(|d| (d.line, d.column, d.offset))
        .collect();
    let b: Vec<_> = plain
        .diagnostics
        .iter()
        .map(|d| (d.line, d.column, d.offset + 3))
        .collect();
    assert_eq!(a, b);
    assert_eq!(dump(&games[0].game), dump(&plain.game));
    assert_eq!(games[0].byte_range.start, 3);
}

#[test]
fn no_break_space_between_tokens_is_white_space() {
    let g = with_header("1.\u{a0}e4\u{a0}e5\u{a0}2. Nf3 *");
    assert_eq!(g.diagnostics, vec![]);
    assert_eq!(dump(&g.game), "e4 e5 Nf3");
    let bytes = b"[Event \"t\"]\n\n1. e4\xa0e5\xa02. Nf3 *\n";
    let games: Vec<_> = PgnReader::new(&bytes[..], ReadOptions::default())
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(games[0].diagnostics, vec![]);
    assert_eq!(dump(&games[0].game), "e4 e5 Nf3");
}

// ----------------------------------------------------------------------
// Encodings (D18)
// ----------------------------------------------------------------------

fn read_bytes(bytes: &[u8]) -> Vec<ParsedGame> {
    PgnReader::new(bytes, ReadOptions::default())
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn utf8_without_bom_is_read_as_is() {
    let g = parse("[White \"Müller, Jörg\"]\n[Event \"£10 ♕ €\"]\n\n1. e4 {café ✓} *\n");
    assert_eq!(g.game.tags().white, "Müller, Jörg");
    assert_eq!(g.game.tags().event, "£10 ♕ €");
    assert_eq!(dump(&g.game), "e4 {café ✓}");
    assert_eq!(g.diagnostics, vec![]);
}

#[test]
fn latin1_text_is_decoded_with_the_fallback_encoding() {
    // é ü ß £ in Latin-1 / windows-1252, € only in windows-1252 (0x80).
    let bytes = b"[White \"Caf\xe9\"]\n[Event \"Stra\xdfe \xfc \xa310 \x80\"]\n\n1. e4 {\xe9t\xe9 \xfc} *\n";
    let games = read_bytes(bytes);
    assert_eq!(games.len(), 1);
    let g = &games[0];
    assert_eq!(g.game.tags().white, "Café");
    assert_eq!(g.game.tags().event, "Straße ü £10 €");
    assert_eq!(dump(&g.game), "e4 {été ü}");
    assert_eq!(kinds(g), vec![DiagnosticKind::EncodingFallback]);
    assert_eq!(g.diagnostics[0].severity, Severity::Info);
}

#[test]
fn mixed_encodings_do_not_damage_the_utf8_games() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice("[White \"Müller\"]\n\n1. e4 {ü £10} *\n\n".as_bytes());
    bytes.extend_from_slice(b"[White \"M\xfcller\"]\n\n1. d4 {\xfc \xa310} *\n\n");
    bytes.extend_from_slice("[White \"Müller\"]\n\n1. c4 {ü} *\n".as_bytes());
    let games = read_bytes(&bytes);
    assert_eq!(games.len(), 3);
    for g in &games {
        assert_eq!(g.game.tags().white, "Müller");
    }
    assert_eq!(dump(&games[0].game), "e4 {ü £10}");
    assert_eq!(dump(&games[1].game), "d4 {ü £10}");
    assert_eq!(dump(&games[2].game), "c4 {ü}");
}

#[test]
fn bom_utf8_with_invalid_bytes_uses_the_replacement_character() {
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(b"[White \"M\xfcller\"]\n\n*\n");
    let games = read_bytes(&bytes);
    assert_eq!(games[0].game.tags().white, "M\u{fffd}ller");
    assert_eq!(games[0].diagnostics, vec![]);
}

#[test]
fn pound_sign_is_not_turned_into_a_chess_piece() {
    let games = read_bytes(b"[Event \"Prize \xa310\"]\n\n*\n");
    assert_eq!(games[0].game.tags().event, "Prize £10");
}

#[test]
fn fallback_encoding_is_exposed_by_the_reader() {
    let bytes = b"[White \"Caf\xe9 \xe9\xe8\xe0\"]\n\n*\n";
    let mut r = PgnReader::new(&bytes[..], ReadOptions::default());
    assert_eq!(r.fallback_encoding(), encoding_rs::WINDOWS_1252);
    r.next().unwrap().unwrap();
    assert_eq!(r.fallback_encoding().name(), "windows-1252");
}

// ----------------------------------------------------------------------
// gzip and streaming
// ----------------------------------------------------------------------

#[test]
fn gzip_input_reads_like_plain_text() {
    let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
    enc.write_all(CLEAN.as_bytes()).unwrap();
    enc.write_all(CLEAN.as_bytes()).unwrap();
    let gz = enc.finish().unwrap();
    let games = read_bytes(&gz);
    assert_eq!(games.len(), 2);
    assert_eq!(dump(&games[1].game), dump(&parse(CLEAN).game));
    assert_eq!(games[1].first_line, 10);
}

#[test]
fn open_reads_files_and_gzip_files_by_content() {
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("a.pgn");
    std::fs::write(&plain, CLEAN).unwrap();
    let gz = dir.path().join("b.pgn.gz");
    let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
    enc.write_all(CLEAN.as_bytes()).unwrap();
    std::fs::write(&gz, enc.finish().unwrap()).unwrap();
    // A gzip file with a misleading name is still recognised.
    let odd = dir.path().join("c.txt");
    std::fs::copy(&gz, &odd).unwrap();
    for path in [plain, gz, odd] {
        let games: Vec<_> = PgnReader::open(&path, ReadOptions::default())
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(games.len(), 1, "{path:?}");
    }
    let missing = PgnReader::open(&dir.path().join("nope.pgn"), ReadOptions::default());
    assert!(matches!(missing, Err(super::PgnError::Open { .. })));
}

#[test]
fn truncated_gzip_yields_the_complete_games_then_an_error() {
    let mut text = String::new();
    for _ in 0..2000 {
        text.push_str(CLEAN);
        text.push('\n');
    }
    let mut enc = GzEncoder::new(Vec::new(), Compression::none());
    enc.write_all(text.as_bytes()).unwrap();
    let mut gz = enc.finish().unwrap();
    gz.truncate(gz.len() / 2);
    let items: Vec<_> = PgnReader::new(&gz[..], ReadOptions::default()).collect();
    let (ok, err): (Vec<_>, Vec<_>) = items.into_iter().partition(Result::is_ok);
    assert!(ok.len() > 100);
    assert_eq!(err.len(), 1);
    assert!(matches!(err[0], Err(super::PgnError::Gzip { .. })));
}

#[test]
fn oversize_game_is_reported_and_the_next_one_is_read() {
    let big = "x ".repeat(5000);
    let pgn = format!(
        "[Event \"a\"]\n\n1. e4 *\n[Event \"huge\"]\n\n1. d4 {{{big}}}\n[Event \"c\"]\n\n1. c4 *\n"
    );
    let opts = ReadOptions {
        max_game_bytes: 2000,
        ..ReadOptions::default()
    };
    let games = parse_pgn_text(&pgn, &opts);
    assert_eq!(games.len(), 3);
    assert_eq!(games[1].termination, Termination::Oversize);
    assert_eq!(kinds(&games[1]), vec![DiagnosticKind::GameTooLarge]);
    assert_eq!(games[1].game.tags().event, "huge");
    assert_eq!(games[1].game.node_count(), 1);
    assert_eq!(dump(&games[2].game), "c4");
    assert_eq!(games[2].number, 3);
}

// ----------------------------------------------------------------------
// parse_movetext
// ----------------------------------------------------------------------

#[test]
fn parse_movetext_reads_loose_moves_from_a_position() {
    let p = parse_movetext(
        "1. e4 e5 2. Nf3 {x} ( 2. Bc4 ) Nc6 1-0",
        &Position::new(),
        &ReadOptions::default(),
    );
    assert_eq!(dump(&p.game), "e4 e5 Nf3 {x} ( Bc4 ) Nc6");
    assert_eq!(p.result, Some(GameResult::White));
    assert_eq!(p.termination, Termination::ResultToken);
    assert_eq!(p.diagnostics, vec![]);
    let start = Position::from_fen(KP).unwrap();
    let p = parse_movetext("e4 Kd7 Zz", &start, &ReadOptions::default());
    assert_eq!(dump(&p.game), "e4 Kd7");
    assert_eq!(p.result, None);
    assert_eq!(p.diagnostics.len(), 1);
    assert_eq!((p.diagnostics[0].line, p.diagnostics[0].column), (1, 8));
}

#[test]
fn parse_movetext_of_empty_text() {
    let p = parse_movetext("", &Position::new(), &ReadOptions::default());
    assert_eq!(p.game.node_count(), 1);
    assert_eq!(p.termination, Termination::EndOfInput);
}

// ----------------------------------------------------------------------
// Robustness
// ----------------------------------------------------------------------

#[test]
fn arbitrary_bytes_never_panic() {
    // A cheap deterministic stand-in for fuzzing (proptest comes later).
    let mut seed = 0x1234_5678_u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let alphabet: &[u8] = b"[]{}()\"\\;%$!?+-=#. \n\r\txe4Nf3O01/2*\xc2\xa0\xff";
    for _ in 0..300 {
        let len = (next() % 400) as usize;
        let bytes: Vec<u8> = (0..len)
            .map(|_| alphabet[(next() as usize) % alphabet.len()])
            .collect();
        for g in PgnReader::new(&bytes[..], ReadOptions::default()).flatten() {
            drop(dump(&g.game));
        }
    }
}

#[test]
fn diagnostic_is_displayable() {
    let g = parse(WITH_ERRORS);
    let text: Vec<String> = g.diagnostics.iter().map(Diagnostic::to_string).collect();
    assert_eq!(text[0], "(game 1, line 2:1) malformed tag: [Bad");
}

#[test]
fn pathological_inputs_finish_and_stay_bounded() {
    // Quarter-megabyte units: enough to cross every limit, fast in debug builds.
    let mb = |n: usize| n << 18;
    let cases: Vec<(&str, String)> = vec![
        (
            "giant tag value",
            format!("[Event \"{}]\n\n*\n", "x".repeat(mb(4))),
        ),
        (
            "giant tag name",
            format!("[{} \"v\"]\n\n*\n", "N".repeat(mb(4))),
        ),
        (
            "many quotes",
            format!("[Event {}]\n\n*\n", "\" ".repeat(mb(2))),
        ),
        (
            "blank header",
            format!("[Event \"t\"]\n{}\n1. e4 *\n", " \n".repeat(mb(2))),
        ),
        (
            "one long word",
            format!("[Event \"t\"]\n\n{} *\n", "e".repeat(mb(4))),
        ),
        (
            "many unclosed braces",
            format!("[Event \"t\"]\n\n1. e4 {}\n", "{ ".repeat(mb(2))),
        ),
        ("only junk", "junk line\n".repeat(mb(1))),
        (
            "many dollars",
            format!("[Event \"t\"]\n\n1. e4 {} *\n", "$1".repeat(mb(2))),
        ),
        (
            "many tokens",
            format!("[Event \"t\"]\n\n{} *\n", "x1 ".repeat(mb(2))),
        ),
        (
            "nag symbols",
            format!("[Event \"t\"]\n\n1. e4{} *\n", "!?+-".repeat(mb(1))),
        ),
        ("many headers", "[Event \"t\"]\n".repeat(mb(1) / 4)),
    ];
    for (name, text) in cases {
        let games: Vec<_> = PgnReader::new(text.as_bytes(), ReadOptions::default())
            .collect::<Result<_, _>>()
            .unwrap();
        for g in &games {
            assert!(g.diagnostics.len() <= 101, "{name}");
        }
        assert!(games.len() <= 2, "{name}: {}", games.len());
    }
}

fn game_with_white(name: &[u8]) -> Vec<u8> {
    let mut bytes = b"[White \"".to_vec();
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(b"\"]\n\n1. e4 *\n\n");
    bytes
}

#[test]
fn plenty_of_western_text_in_latin1_is_decoded_right() {
    let names: [(&[u8], &str); 4] = [
        (b"Mu\xf1oz, Jos\xe9", "Muñoz, José"),
        (b"M\xfcller, J\xf6rg", "Müller, Jörg"),
        (b"Fran\xe7ois, \xc9mile", "François, Émile"),
        (b"Stra\xdfe, \xc5ke", "Straße, Åke"),
    ];
    let mut bytes = Vec::new();
    for _ in 0..5 {
        for (raw, _) in names {
            bytes.extend(game_with_white(raw));
        }
    }
    let games = read_bytes(&bytes);
    assert_eq!(games.len(), 20);
    for (i, g) in games.iter().enumerate() {
        assert_eq!(g.game.tags().white, names[i % 4].1, "game {i}");
    }
}

#[test]
fn cyrillic_windows_1251_is_detected_from_enough_evidence() {
    // "Карпов, Анатолий" and "Каспаров, Гарри" in windows-1251.
    let names: [&[u8]; 2] = [
        b"\xca\xe0\xf0\xef\xee\xe2, \xc0\xed\xe0\xf2\xee\xeb\xe8\xe9",
        b"\xca\xe0\xf1\xef\xe0\xf0\xee\xe2, \xc3\xe0\xf0\xf0\xe8",
    ];
    let mut bytes = Vec::new();
    for _ in 0..30 {
        for n in names {
            bytes.extend(game_with_white(n));
        }
    }
    let mut reader = PgnReader::new(&bytes[..], ReadOptions::default());
    let games: Vec<_> = reader.by_ref().collect::<Result<_, _>>().unwrap();
    assert_eq!(reader.fallback_encoding().name(), "windows-1251");
    // Later games are decoded with the detected encoding.
    assert_eq!(games[58].game.tags().white, "Карпов, Анатолий");
    assert_eq!(games[59].game.tags().white, "Каспаров, Гарри");
}

// ----------------------------------------------------------------------
// Limits, encodings and errors (review fixes)
// ----------------------------------------------------------------------

fn read_all(data: &[u8], opts: ReadOptions) -> Vec<Result<ParsedGame, super::PgnError>> {
    PgnReader::new(data, opts).collect()
}

#[test]
fn distinct_tags_are_bounded_and_fast() {
    let mut pgn = String::from("[Event \"t\"]\n");
    for i in 0..300_000 {
        writeln!(pgn, "[T{i} \"v\"]").unwrap();
    }
    pgn.push_str("\n1. e4 *\n");
    let started = std::time::Instant::now();
    let game = parse(&pgn);
    assert!(
        started.elapsed().as_secs() < 20,
        "tag handling is not linear"
    );
    assert_eq!(game.game.tags().extras().len(), 256);
    assert_eq!(game.game.tags().extras()[0].0, "T0");
    let too_many: Vec<_> = game
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::TooManyTags)
        .collect();
    assert_eq!(too_many.len(), 1);
    assert_eq!(too_many[0].severity, Severity::Warning);
    assert_eq!(game.game.mainline().count(), 1);
}

#[test]
fn tag_limit_is_an_option_and_spares_standard_tags() {
    let opts = ReadOptions::default().with_max_tags_per_game(2);
    let game = parse_with(
        "[A \"1\"]\n[B \"2\"]\n[C \"3\"]\n[Event \"kept\"]\n[Result \"1-0\"]\n\n1. e4 1-0\n",
        &opts,
    );
    assert_eq!(game.game.tags().extras().len(), 2);
    assert_eq!(game.game.tags().event, "kept");
    assert_eq!(game.game.tags().result, GameResult::White);
}

#[test]
fn node_limit_stops_the_moves_and_reports_it() {
    let opts = ReadOptions::default().with_max_nodes_per_game(3);
    let game = parse_with(
        "[Event \"t\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 1-0\n",
        &opts,
    );
    assert_eq!(game.game.mainline().count(), 3);
    assert_eq!(game.termination, Termination::NodeLimit);
    let errors: Vec<_> = game
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::TooManyNodes)
        .collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].severity, Severity::Error);
}

#[test]
fn node_limit_counts_variations_and_spares_the_next_game() {
    let opts = ReadOptions::default().with_max_nodes_per_game(4);
    let games = parse_pgn_text(
        "[Event \"a\"]\n\n1. e4 (1. d4 d5 2. c4) e5 2. Nf3 *\n\n[Event \"b\"]\n\n1. c4 *\n",
        &opts,
    );
    assert_eq!(games.len(), 2);
    assert_eq!(games[0].game.node_count(), 5);
    assert_eq!(games[0].termination, Termination::NodeLimit);
    assert_eq!(games[1].termination, Termination::ResultToken);
    assert!(
        games[1].diagnostics.is_empty(),
        "{:?}",
        games[1].diagnostics
    );
}

#[test]
fn default_limits_are_the_documented_ones() {
    let opts = ReadOptions::default();
    assert_eq!(opts.max_game_bytes, 16 << 20);
    assert_eq!(opts.max_nodes_per_game, 1_000_000);
    assert_eq!(opts.max_tags_per_game, 256);
}

#[test]
fn first_games_use_the_encoding_found_later_in_the_file() {
    // "Ivanov, Petr" in Cyrillic, windows-1251.
    let name = [
        0xC8, 0xE2, 0xE0, 0xED, 0xEE, 0xE2, 0x2C, 0x20, 0xCF, 0xB8, 0xF2, 0xF0,
    ];
    let mut data = Vec::new();
    for _ in 0..6 {
        data.extend(b"[Event \"x\"]\n[White \"");
        data.extend(name);
        data.extend(b"\"]\n\n1. e4 *\n\n");
    }
    let games = read_all(&data, ReadOptions::default());
    assert_eq!(games.len(), 6);
    for game in games {
        let game = game.unwrap();
        assert_eq!(
            game.game.tags().white,
            "Иванов, Пётр",
            "game {}",
            game.number
        );
    }
}

#[test]
fn a_comment_cut_inside_a_character_stays_utf8() {
    let comment = "€".repeat(600_000);
    let game = parse(&format!("[Event \"t\"]\n\n1. e4 {{{comment}}} *\n"));
    let text = game
        .game
        .comment(game.game.mainline().next().unwrap())
        .unwrap()
        .unwrap();
    assert!(text.len() <= 1 << 20);
    assert!(text.len() > (1 << 20) - 4);
    assert!(text.chars().all(|c| c == '€'));
    assert!(
        game.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::CommentTruncated)
    );
    assert!(
        !game
            .diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::EncodingFallback)
    );
}

#[test]
fn a_long_tag_cut_inside_a_character_stays_utf8() {
    let value = "€".repeat(30_000);
    let game = parse(&format!("[Event \"{value}\"]\n\n1. e4 *\n"));
    let event = &game.game.tags().event;
    assert!(event.len() > 64 * 1024 - 4 && event.len() <= 64 * 1024);
    assert!(event.chars().all(|c| c == '€'));
    assert!(
        game.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::TagTooLong)
    );
}

#[test]
fn utf16_input_is_rejected_not_read_as_zero_games() {
    for (bom, name) in [([0xff, 0xfe], "UTF-16LE"), ([0xfe, 0xff], "UTF-16BE")] {
        let mut data = bom.to_vec();
        data.extend(b"[\0E\0v\0");
        let items = read_all(&data, ReadOptions::default());
        assert_eq!(items.len(), 1);
        assert!(
            matches!(&items[0], Err(super::PgnError::UnsupportedEncoding { encoding }) if *encoding == name)
        );
    }
}

#[test]
fn text_outside_games_is_counted() {
    let mut reader = PgnReader::new(&b"1. e4 e5 *\n"[..], ReadOptions::default());
    assert!(reader.next().is_none());
    assert_eq!(reader.skipped_bytes(), 11);

    let pgn = "intro\n[Event \"a\"]\n\n1. e4 *\n\ntrailing\n";
    let mut reader = PgnReader::new(pgn.as_bytes(), ReadOptions::default());
    assert_eq!(reader.by_ref().count(), 1);
    assert_eq!(reader.skipped_bytes(), 6 + 11);
}

/// Yields `data`, then fails.
struct FailAfter<'a>(&'a [u8]);

impl std::io::Read for FailAfter<'_> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.0.is_empty() {
            return Err(std::io::Error::other("disk gone"));
        }
        let n = self.0.len().min(out.len());
        out[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}

#[test]
fn io_error_follows_the_readable_partial_game() {
    let data = b"[Event \"a\"]\n\n1. e4 *\n\n[Event \"b\"]\n\n1. d4 d5 2. c4";
    let items: Vec<_> = PgnReader::new(FailAfter(data), ReadOptions::default()).collect();
    assert_eq!(items.len(), 3);
    let last = items[1].as_ref().unwrap();
    assert_eq!(last.termination, Termination::EndOfInput);
    assert_eq!(last.game.mainline().count(), 3);
    // A failing file is an I/O error, not a gzip one.
    assert!(matches!(items[2], Err(super::PgnError::Io { .. })));
}

#[test]
fn gzip_errors_and_file_errors_are_told_apart() {
    let mut enc = GzEncoder::new(Vec::new(), Compression::none());
    enc.write_all(b"[Event \"a\"]\n\n1. e4 *\n").unwrap();
    let mut gz = enc.finish().unwrap();
    gz.truncate(gz.len() - 12);
    let failing: Vec<_> = PgnReader::new(FailAfter(&gz), ReadOptions::default()).collect();
    assert!(matches!(
        failing.last(),
        Some(Err(super::PgnError::Io { .. }))
    ));
    let corrupt: Vec<_> = PgnReader::new(&gz[..], ReadOptions::default()).collect();
    assert!(matches!(
        corrupt.last(),
        Some(Err(super::PgnError::Gzip { .. }))
    ));
}

#[test]
fn utf8_a_grave_is_not_split_at_its_a0_byte() {
    let game = no_result("1. e4 àxyz e5");
    let token = game
        .diagnostics
        .iter()
        .find(|d| d.kind == DiagnosticKind::UnexpectedToken)
        .unwrap();
    assert_eq!(token.text.as_deref(), Some("àxyz"));
    assert_eq!(game.game.mainline().count(), 2);
}

#[test]
fn tag_and_movetext_on_one_line_follow_the_scid() {
    // The Scid reads a tag line as a whole and drops what follows the
    // tags; the moves on that line are lost.
    let game = parse("[Event \"x\"] 1. e4 e5 *\n\n1. d4 *\n");
    assert_eq!(game.game.tags().event, "x");
    assert!(
        game.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::InvalidTextInHeader)
    );
}
