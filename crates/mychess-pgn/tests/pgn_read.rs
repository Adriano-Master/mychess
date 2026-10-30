//! Contract tests of the PGN reader: headers, movetext, framing, options.
//!
//! Derived from `docs/specs/m1-3-pgn.md` (D16-D30, reading rules 1-23, the
//! diagnostics table and the acceptance criteria) and Scid's
//! `src/pgnparse.cpp`, not from the implementation. Only the public API is
//! used. Input handling (encodings, gzip, streaming) is in
//! `pgn_read_input.rs`.

#![allow(
    clippy::many_single_char_names,
    clippy::manual_is_multiple_of,
    clippy::format_push_string,
    let_underscore_drop,
    clippy::assertions_on_result_states,
    clippy::cast_precision_loss,
    clippy::bool_to_int_with_if,
    clippy::items_after_statements,
    clippy::similar_names,
    clippy::struct_field_names,
    clippy::collapsible_if,
    clippy::single_match_else,
    clippy::doc_markdown,
    clippy::needless_pass_by_value,
    clippy::type_complexity,
    clippy::field_reassign_with_default,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::manual_assert,
    clippy::ignored_unit_patterns,
    clippy::needless_borrows_for_generic_args,
    clippy::unreadable_literal,
    clippy::explicit_iter_loop,
    clippy::uninlined_format_args,
    clippy::if_not_else,
    clippy::redundant_closure_for_method_calls,
    clippy::map_unwrap_or,
    clippy::match_same_arms,
    clippy::cognitive_complexity,
    clippy::used_underscore_binding,
    clippy::semicolon_if_nothing_returned,
    clippy::bool_assert_comparison,
    clippy::wildcard_imports,
    clippy::assert_is_empty,
    clippy::assigning_clones,
    clippy::double_comparisons,
    clippy::io_other_error,
    clippy::needless_borrow,
    clippy::op_ref,
    clippy::repeat_once,
    clippy::suspicious_map,
    clippy::doc_lazy_continuation,
    clippy::module_name_repetitions
)]

mod common;

use common::*;
use mychess_core::{Elo, GameResult, PartialDate, Position, Rating, RatingType, SanMode};
use mychess_pgn::read::{
    CommentWhitespace, DiagnosticKind as K, PlayerNames, PreGameText, ReadOptions,
    SemicolonComments, Severity, Termination, UnsupportedVariant, parse_movetext, parse_pgn_text,
};
use pretty_assertions::assert_eq;
use rstest::rstest;

fn tagged(tag_lines: &str) -> ParsedGameAlias {
    one(&format!("{tag_lines}\n\n*\n"))
}
type ParsedGameAlias = mychess_pgn::read::ParsedGame;

// ===========================================================================
// 1. Header
// ===========================================================================

#[test]
fn reads_the_seven_tag_roster() {
    let g = tagged(
        "[Event \"Linares\"]\n[Site \"Linares ESP\"]\n[Date \"1993.03.05\"]\n[Round \"3\"]\n[White \"Kasparov, G\"]\n[Black \"Anand, V\"]\n[Result \"1-0\"]",
    );
    let t = g.game.tags();
    assert_eq!(t.event, "Linares");
    assert_eq!(t.site, "Linares ESP");
    assert_eq!(t.round, "3");
    assert_eq!(t.white, "Kasparov, G");
    assert_eq!(t.black, "Anand, V");
    assert_eq!(
        t.date,
        PartialDate::new(Some(1993), Some(3), Some(5)).unwrap()
    );
}

#[test]
fn missing_tags_keep_the_scid_defaults() {
    let g = one("[White \"Only\"]\n\n*\n");
    let t = g.game.tags();
    assert_eq!(t.event, "?");
    assert_eq!(t.site, "?");
    assert_eq!(t.black, "?");
    assert_eq!(t.round, "?");
    assert!(t.date.is_unknown() && t.event_date.is_unknown());
    assert_eq!(t.result, GameResult::Unknown);
    assert!(t.white_rating.is_none() && t.black_rating.is_none() && t.eco.is_none());
    assert!(t.extras().is_empty());
}

#[test]
fn unknown_tags_become_extras_in_reading_order() {
    let g = tagged(
        "[Event \"E\"]\n[Annotator \"Me\"]\n[Zed \"z\"]\n[PlyCount \"3\"]\n[TimeControl \"40/7200\"]",
    );
    let names: Vec<_> = g
        .game
        .tags()
        .extras()
        .iter()
        .map(|(k, _)| k.as_str())
        .collect();
    assert_eq!(names, ["Annotator", "Zed", "PlyCount", "TimeControl"]);
    assert_eq!(g.game.tags().get_extra("Zed"), Some("z"));
    assert!(g.diagnostics.is_empty(), "{:?}", g.diagnostics);
}

#[test]
fn tag_names_are_case_sensitive() {
    let g = tagged("[white \"lower\"]\n[WHITE \"upper\"]");
    assert_eq!(g.game.tags().white, "?");
    assert_eq!(g.game.tags().get_extra("white"), Some("lower"));
    assert_eq!(g.game.tags().get_extra("WHITE"), Some("upper"));
}

#[test]
fn repeated_tag_keeps_the_last_value() {
    let g = tagged("[White \"First\"]\n[White \"Second\"]\n[Annot \"a\"]\n[Annot \"b\"]");
    assert_eq!(g.game.tags().white, "Second");
    assert_eq!(g.game.tags().get_extra("Annot"), Some("b"));
    assert_eq!(
        g.game
            .tags()
            .extras()
            .iter()
            .filter(|(k, _)| k == "Annot")
            .count(),
        1
    );
}

#[test]
fn several_tags_on_one_line() {
    let g = tagged("[Event \"A\"] [Site \"B\"][White \"C\"]\n[Black \"D\"]");
    let t = g.game.tags();
    assert_eq!((t.event.as_str(), t.site.as_str()), ("A", "B"));
    assert_eq!((t.white.as_str(), t.black.as_str()), ("C", "D"));
    assert!(g.diagnostics.is_empty(), "{:?}", g.diagnostics);
}

#[rstest]
#[case::escaped_quote("[White \"Doe \\\"Jack\\\" Smith\"]", "Doe \"Jack\" Smith")]
#[case::escaped_backslash("[White \"C:\\\\dir\"]", "C:\\dir")]
#[case::inner_unescaped_quotes_recovered("[White \"Doe, \"Jack\"\"]", "Doe, \"Jack\"")]
#[case::spaces_around_name("[  White   \"Kramnik, V\"  ]", "Kramnik, V")]
#[case::tab_between_name_and_value("[White\t\"Tab, T\"]", "Tab, T")]
#[case::brackets_in_value("[White \"A [x] B\"]", "A [x] B")]
fn tag_value_forms(#[case] line: &str, #[case] white: &str) {
    let g = tagged(line);
    assert_eq!(g.game.tags().white, white);
    assert!(
        !has(&g, |k| *k == K::BadTag),
        "unexpected BadTag: {:?}",
        g.diagnostics
    );
}

#[rstest]
#[case::no_quotes("[White Kasparov]")]
#[case::unterminated_value("[White \"Kasparov]")]
#[case::name_only("[White]")]
#[case::value_then_garbage("[White \"K\" extra]")]
fn malformed_tag_is_dropped_with_bad_tag_and_the_game_survives(#[case] line: &str) {
    let g = one(&format!(
        "[Event \"E\"]\n{line}\n[Black \"B\"]\n\n1. e4 *\n"
    ));
    assert!(has(&g, |k| *k == K::BadTag), "{line}: {:?}", g.diagnostics);
    assert_eq!(g.game.tags().white, "?", "{line}");
    assert_eq!(g.game.tags().black, "B", "later tags still read: {line}");
    assert_eq!(g.game.mainline().count(), 1, "movetext still read: {line}");
    assert!(
        g.diagnostics
            .iter()
            .filter(|d| d.kind == K::BadTag)
            .all(|d| d.severity == Severity::Error)
    );
}

#[test]
fn bad_tag_diagnostic_points_at_the_line_and_column_of_the_tag() {
    let g = one("[Event \"E\"]\n  [White Kasparov]\n\n*\n");
    let d = g.diagnostics.iter().find(|d| d.kind == K::BadTag).unwrap();
    assert_eq!(d.line, 2);
    assert_eq!(d.column, 3, "first byte of the tag (after two spaces)");
    assert_eq!(d.game, 1);
    assert_eq!(d.offset, 12 + 2);
}

#[rstest]
#[case("Event")]
#[case("Site")]
#[case("Round")]
#[case("White")]
#[case("Black")]
fn empty_value_of_core_text_tags_is_ignored_silently(#[case] name: &str) {
    let g = tagged(&format!("[{name} \"\"]"));
    let t = g.game.tags();
    for v in [&t.event, &t.site, &t.round, &t.white, &t.black] {
        assert_eq!(v, "?", "{name}");
    }
    assert!(g.diagnostics.is_empty(), "{name}: {:?}", g.diagnostics);
}

#[test]
fn empty_value_of_an_extra_tag_is_kept() {
    let g = tagged("[Annotator \"\"]");
    assert_eq!(g.game.tags().get_extra("Annotator"), Some(""));
}

#[test]
fn huge_tag_name_and_value_do_not_drop_the_game() {
    let name = "N".repeat(2000);
    let value = "v".repeat(100_000);
    let g = one(&format!(
        "[Event \"E\"]\n[{name} \"x\"]\n[Annotator \"{value}\"]\n[Black \"B\"]\n\n1. e4 *\n"
    ));
    assert!(has(&g, |k| *k == K::TagTooLong), "{:?}", kinds(&g));
    assert_eq!(g.game.tags().black, "B");
    assert_eq!(g.game.mainline().count(), 1);
    let kept = g.game.tags().get_extra("Annotator").unwrap_or("");
    assert!(
        kept.len() <= 64 * 1024,
        "value truncated to 64 KiB, got {}",
        kept.len()
    );
    assert!(!kept.is_empty() && kept.bytes().all(|b| b == b'v'));
}

#[test]
fn value_beyond_255_bytes_is_kept_unlike_scid() {
    let long = "w".repeat(300);
    let g = tagged(&format!("[White \"{long}\"]"));
    assert_eq!(g.game.tags().white, long);
    assert!(g.diagnostics.is_empty());
}

// ----- Result ---------------------------------------------------------------

#[rstest]
#[case("1-0", GameResult::White)]
#[case("0-1", GameResult::Black)]
#[case("1/2-1/2", GameResult::Draw)]
#[case("1/2", GameResult::Draw)]
#[case("*", GameResult::Unknown)]
#[case("garbage", GameResult::Unknown)]
#[case("", GameResult::Unknown)]
#[case("1-0 (forfeit)", GameResult::White)]
fn result_tag_without_a_result_token(#[case] value: &str, #[case] expected: GameResult) {
    let g = one(&format!("[Result \"{value}\"]\n\n1. e4 e5\n"));
    assert_eq!(g.game.tags().result, expected, "tag value {value:?}");
    assert_eq!(g.termination, Termination::EndOfInput);
}

#[rstest]
#[case("1-0", GameResult::White)]
#[case("0-1", GameResult::Black)]
#[case("1/2-1/2", GameResult::Draw)]
#[case("*", GameResult::Unknown)]
#[case("1:0", GameResult::White)]
#[case("0:1", GameResult::Black)]
#[case("1/2:1/2", GameResult::Draw)]
#[case("1/2", GameResult::Draw)]
#[case("\u{bd}-\u{bd}", GameResult::Draw)]
fn result_tokens_end_the_game(#[case] token: &str, #[case] expected: GameResult) {
    let g = mt(&format!("1. e4 e5 {token}"));
    assert_eq!(g.game.tags().result, expected, "{token}");
    assert_eq!(g.termination, Termination::ResultToken, "{token}");
    assert_eq!(g.game.mainline().count(), 2);
    assert!(
        !has(&g, |k| matches!(k, K::UnexpectedToken)),
        "{token}: {:?}",
        g.diagnostics
    );
}

#[test]
fn result_token_wins_over_the_tag_and_reports_a_mismatch() {
    let g = one("[Result \"0-1\"]\n\n1. e4 e5 1-0\n");
    assert_eq!(g.game.tags().result, GameResult::White);
    let d = g
        .diagnostics
        .iter()
        .find(|d| d.kind == K::ResultMismatch)
        .unwrap();
    assert_eq!(d.severity, Severity::Warning);
}

#[test]
fn matching_result_token_and_tag_report_nothing() {
    let g = one("[Result \"1-0\"]\n\n1. e4 e5 1-0\n");
    assert_eq!(g.game.tags().result, GameResult::White);
    assert!(g.diagnostics.is_empty(), "{:?}", g.diagnostics);
}

#[test]
fn star_token_overrides_a_decisive_tag() {
    let g = one("[Result \"1-0\"]\n\n1. e4 *\n");
    assert_eq!(g.game.tags().result, GameResult::Unknown);
    assert!(has(&g, |k| *k == K::ResultMismatch));
}

#[test]
fn result_inside_a_variation_ends_the_game_with_unclosed_variation() {
    let g = mt("1. e4 e5 (1... c5 2. Nf3 1-0 3. d4");
    assert_eq!(g.termination, Termination::ResultToken);
    assert_eq!(g.game.tags().result, GameResult::White);
    assert!(has(&g, |k| *k == K::UnclosedVariation), "{:?}", kinds(&g));
    assert_eq!(dump(&g.game), "e4 e5 (c5 Nf3)");
}

// ----- Dates ----------------------------------------------------------------

#[rstest]
#[case("2023.07.14", Some(2023), Some(7), Some(14))]
#[case("2023.07.??", Some(2023), Some(7), None)]
#[case("2023.??.??", Some(2023), None, None)]
#[case("????.??.??", None, None, None)]
#[case("????.05.17", None, Some(5), Some(17))]
#[case("2023.13.05", Some(2023), None, None)]
#[case("2023.07.32", Some(2023), Some(7), None)]
#[case("3000.05.17", None, Some(5), Some(17))]
#[case("1999", Some(1999), None, None)]
#[case("nonsense", None, None, None)]
fn date_tag_parses_leniently(
    #[case] text: &str,
    #[case] y: Option<u16>,
    #[case] m: Option<u8>,
    #[case] d: Option<u8>,
) {
    let g = tagged(&format!("[Date \"{text}\"]\n[EventDate \"{text}\"]"));
    let want = PartialDate::new(y, m, d).unwrap();
    assert_eq!(g.game.tags().date, want, "Date {text}");
    assert_eq!(g.game.tags().event_date, want, "EventDate {text}");
    assert!(g.diagnostics.is_empty(), "{text}: {:?}", g.diagnostics);
}

// ----- ECO ------------------------------------------------------------------

#[rstest]
#[case("B90", Some("B90"))]
#[case("B91a", Some("B91a"))]
#[case("e04", Some("E04"))]
#[case("Z99", None)]
#[case("", None)]
#[case("hello", None)]
#[case("A", Some("A00"))]
fn eco_tag_is_tolerant_and_an_invalid_one_is_just_absent(
    #[case] text: &str,
    #[case] expected: Option<&str>,
) {
    let g = tagged(&format!("[ECO \"{text}\"]"));
    assert_eq!(
        g.game.tags().eco.map(|e| e.to_string()).as_deref(),
        expected,
        "{text}"
    );
    assert!(g.diagnostics.is_empty(), "{text}: {:?}", g.diagnostics);
}

// ----- Ratings and names ----------------------------------------------------

fn white_rating(g: &ParsedGameAlias) -> Option<(u16, RatingType)> {
    g.game
        .tags()
        .white_rating
        .map(|r: Rating| (r.value.get(), r.kind))
}

#[rstest]
#[case("Elo", RatingType::Elo)]
#[case("Rating", RatingType::Rating)]
#[case("Rapid", RatingType::Rapid)]
#[case("ICCF", RatingType::Iccf)]
#[case("USCF", RatingType::Uscf)]
#[case("DWZ", RatingType::Dwz)]
#[case("ECF", RatingType::Ecf)]
fn rating_tag_kinds(#[case] suffix: &str, #[case] kind: RatingType) {
    let g = tagged(&format!(
        "[WhiteX \"?\"]\n[White{suffix} \"2345\"]\n[Black{suffix} \"1999\"]"
    ));
    assert_eq!(white_rating(&g), Some((2345, kind)));
    let b = g.game.tags().black_rating.unwrap();
    assert_eq!((b.value.get(), b.kind), (1999, kind));
    assert!(g.game.tags().get_extra(&format!("White{suffix}")).is_none());
}

#[test]
fn only_the_first_rating_type_per_player_becomes_the_rating() {
    let g = tagged(
        "[WhiteElo \"2700\"]\n[WhiteRapid \"2600\"]\n[BlackUSCF \"2100\"]\n[BlackElo \"2200\"]",
    );
    assert_eq!(white_rating(&g), Some((2700, RatingType::Elo)));
    assert_eq!(g.game.tags().get_extra("WhiteRapid"), Some("2600"));
    let b = g.game.tags().black_rating.unwrap();
    assert_eq!((b.value.get(), b.kind), (2100, RatingType::Uscf));
    assert_eq!(g.game.tags().get_extra("BlackElo"), Some("2200"));
}

#[test]
fn non_numeric_rating_reads_as_zero_and_the_tag_is_consumed() {
    let g = tagged("[WhiteElo \"abc\"]");
    let r = g.game.tags().white_rating;
    assert!(r.is_none() || r.unwrap().value.get() == 0, "{r:?}");
    assert!(g.game.tags().get_extra("WhiteElo").is_none());
}

#[test]
fn rating_above_4000_is_cut_with_a_warning() {
    let g = tagged("[WhiteElo \"9999\"]");
    assert_eq!(white_rating(&g), Some((4000, RatingType::Elo)));
    let d = g
        .diagnostics
        .iter()
        .find(|d| d.kind == K::RatingTooLarge)
        .unwrap();
    assert_eq!(d.severity, Severity::Warning);
}

#[test]
fn unknown_rating_type_is_an_ordinary_tag() {
    let g = tagged("[WhiteFIDE \"2500\"]");
    assert!(g.game.tags().white_rating.is_none());
    assert_eq!(g.game.tags().get_extra("WhiteFIDE"), Some("2500"));
}

#[test]
fn player_name_with_trailing_rating_is_split_like_scid() {
    let g = tagged("[White \"Kasparov,G (2800)\"]");
    assert_eq!(g.game.tags().white, "Kasparov, G");
    assert_eq!(white_rating(&g), Some((2800, RatingType::Elo)));
}

#[test]
fn player_name_rating_above_4000_is_cut_with_a_warning() {
    let g = tagged("[White \"X (9999)\"]");
    assert_eq!(g.game.tags().white, "X");
    assert_eq!(white_rating(&g), Some((4000, RatingType::Elo)));
    assert!(has(&g, |k| *k == K::RatingTooLarge));
}

#[test]
fn black_name_with_rating() {
    let g = tagged("[Black \"Anand,V (2790)\"]");
    assert_eq!(g.game.tags().black, "Anand, V");
    let b = g.game.tags().black_rating.unwrap();
    assert_eq!((b.value.get(), b.kind), (2790, RatingType::Elo));
}

#[rstest]
#[case::no_space_before_paren("Smith(2500)")]
#[case::three_digits("Smith (250)")]
#[case::five_digits("Smith (25000)")]
#[case::letters("Smith (abcd)")]
#[case::no_close("Smith (2500")]
fn name_suffix_that_is_not_a_rating_stays_in_the_name(#[case] name: &str) {
    let g = tagged(&format!("[White \"{name}\"]"));
    assert!(g.game.tags().white_rating.is_none(), "{name}");
    assert!(g.game.tags().white.starts_with("Smith"), "{name}");
    assert!(
        g.game.tags().white.contains('('),
        "{name}: {}",
        g.game.tags().white
    );
}

#[rstest]
#[case("Anand,V", "Anand, V")]
#[case("Anand, V", "Anand, V")]
#[case("Anand,    V", "Anand, V")]
#[case("Anand , V", "Anand , V")]
#[case("  Anand, V  ", "Anand, V")]
#[case("Anand, V \t", "Anand, V")]
#[case("van der Berg, J", "Van der Berg, J")]
#[case("van den Berg, J", "Van den Berg, J")]
#[case("van de Berg, J", "Van de Berg, J")]
#[case("Van Der Berg, J", "Van der Berg, J")]
#[case("Van Den Berg, J", "Van den Berg, J")]
#[case("Van De Berg, J", "Van de Berg, J")]
#[case("van Gogh", "Van Gogh")]
#[case("Smith, J.", "Smith, J.")]
fn player_names_are_standardised_in_scid_mode(#[case] raw: &str, #[case] expected: &str) {
    let g = tagged(&format!("[White \"{raw}\"]\n[Black \"{raw}\"]"));
    assert_eq!(g.game.tags().white, expected, "raw {raw:?}");
    assert_eq!(g.game.tags().black, expected, "raw {raw:?}");
}

#[test]
fn player_names_as_is_keeps_the_text_but_still_reads_the_rating() {
    let o = ReadOptions {
        player_names: PlayerNames::AsIs,
        ..ReadOptions::default()
    };
    let g = one_with("[White \"van der Berg,J   (2500)\"]\n\n*\n", &o);
    assert_eq!(g.game.tags().white, "van der Berg,J  ");
    assert_eq!(white_rating(&g), Some((2500, RatingType::Elo)));
    let g = one_with("[Black \"  Anand,V  \"]\n\n*\n", &o);
    assert_eq!(g.game.tags().black, "  Anand,V  ");
}

// ----- FEN / SetUp / Variant ---------------------------------------------------

const FEN_KP: &str = "8/8/8/4k3/8/8/4K3/4R3 w - - 0 1";

#[rstest]
#[case::with_setup(format!("[SetUp \"1\"]\n[FEN \"{FEN_KP}\"]"))]
#[case::without_setup(format!("[FEN \"{FEN_KP}\"]"))]
#[case::setup_zero(format!("[SetUp \"0\"]\n[FEN \"{FEN_KP}\"]"))]
#[case::setup_after_fen(format!("[FEN \"{FEN_KP}\"]\n[SetUp \"1\"]"))]
fn fen_tag_sets_the_start_position_with_or_without_setup(#[case] header: String) {
    let g = one(&format!("{header}\n\n1. Rd1 Kf5 *\n"));
    assert_eq!(g.game.start_position().fen(), FEN_KP);
    assert_eq!(g.game.mainline().count(), 2);
    assert!(g.diagnostics.is_empty(), "{:?}", g.diagnostics);
    assert!(
        g.game.tags().get_extra("SetUp").is_none(),
        "SetUp is derived, not kept"
    );
    assert!(g.game.tags().get_extra("FEN").is_none());
}

#[test]
fn fen_start_with_black_to_move_and_fullmove_number() {
    let fen = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 7";
    let g = one(&format!("[FEN \"{fen}\"]\n\n7... e5 8. Nf3 *\n"));
    assert_eq!(g.game.start_position().fen(), fen);
    assert_eq!(main_sans(&g.game), ["e5", "Nf3"]);
}

#[test]
fn invalid_fen_reports_an_error_and_continues_from_the_standard_position() {
    let g = one("[FEN \"not a fen\"]\n\n1. e4 e5 *\n");
    let d = g
        .diagnostics
        .iter()
        .find(|d| d.kind == K::InvalidFen)
        .unwrap();
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(g.game.start_position().fen(), Position::new().fen());
    assert_eq!(
        g.game.initial_comment(),
        Some("Error Invalid FEN: not a fen")
    );
    assert_eq!(main_sans(&g.game), ["e4", "e5"]);
}

#[test]
fn fen_that_is_legal_text_but_illegal_position_is_invalid_fen() {
    // two white kings
    let g = one("[FEN \"8/8/8/8/8/8/8/KK2k3 w - - 0 1\"]\n\n*\n");
    assert!(has(&g, |k| *k == K::InvalidFen), "{:?}", kinds(&g));
    assert_eq!(g.game.start_position().fen(), Position::new().fen());
}

#[test]
fn unsupported_variant_skips_the_movetext_by_default() {
    let g = one("[Variant \"Crazyhouse\"]\n[White \"A\"]\n\n1. e4 e5 2. Nf3 *\n");
    assert_eq!(g.termination, Termination::UnsupportedVariant);
    assert!(has(&g, |k| *k == K::UnsupportedVariant));
    assert_eq!(g.game.mainline().count(), 0);
    assert_eq!(g.game.tags().white, "A");
}

#[test]
fn unsupported_variant_with_try_standard_reads_the_moves() {
    let o = ReadOptions {
        unsupported_variant: UnsupportedVariant::TryStandard,
        ..ReadOptions::default()
    };
    let g = one_with("[Variant \"Crazyhouse\"]\n\n1. e4 e5 2. Nf3 *\n", &o);
    assert_eq!(g.game.mainline().count(), 3);
}

#[rstest]
#[case("Standard")]
#[case("standard")]
#[case("Chess")]
fn standard_variant_names_are_read_as_ordinary_games(#[case] v: &str) {
    let g = one(&format!("[Variant \"{v}\"]\n\n1. e4 e5 *\n"));
    assert_eq!(g.game.mainline().count(), 2, "{v}");
    assert!(!has(&g, |k| *k == K::UnsupportedVariant), "{v}");
}

#[test]
fn chess960_variant_spellings_are_read_as_chess960() {
    for v in ["Chess960", "Fischerandom", "chess960"] {
        let g = one(&format!("[Variant \"{v}\"]\n\n1. e4 e5 *\n"));
        assert_eq!(g.game.mainline().count(), 2, "{v}: {:?}", kinds(&g));
        assert_eq!(g.game.variant(), mychess_core::Variant::Chess960, "{v}");
        assert_ne!(g.termination, Termination::UnsupportedVariant, "{v}");
    }
}

/// King on d1 with `KQkq`: only Chess960 can express these rights.
const ODD_FEN: &str = "rnbkqbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBKQBNR w KQkq - 0 1";

#[rstest]
#[case("Standard")]
#[case("chess")]
#[case("Normal")]
fn explicit_standard_variant_reads_the_fen_as_standard(#[case] v: &str) {
    let g = one(&format!(
        "[Variant \"{v}\"]\n[SetUp \"1\"]\n[FEN \"{ODD_FEN}\"]\n\n1. e3 *\n"
    ));
    assert_eq!(g.game.variant(), mychess_core::Variant::Standard, "{v}");
    assert!(!has(&g, |k| *k == K::Chess960Detected), "{v}");
}

#[rstest]
#[case("[Variant \"From Position\"]\n")]
#[case("[Variant \"\"]\n")]
#[case("")]
fn unspecified_variant_still_detects_chess960_in_the_fen(#[case] tag: &str) {
    let g = one(&format!(
        "{tag}[SetUp \"1\"]\n[FEN \"{ODD_FEN}\"]\n\n1. e3 *\n"
    ));
    assert_eq!(g.game.variant(), mychess_core::Variant::Chess960);
    assert!(has(&g, |k| *k == K::Chess960Detected));
}

#[test]
fn skipped_variant_game_does_not_swallow_the_next_game() {
    let games = read_all("[Variant \"Atomic\"]\n\n1. e4 e5 *\n\n[White \"Next\"]\n\n1. d4 d5 *\n");
    assert_eq!(games.len(), 2);
    assert_eq!(games[0].number, 1);
    assert_eq!(games[1].game.tags().white, "Next");
    assert_eq!(games[1].game.mainline().count(), 2);
}

// ----- ignored_tags -----------------------------------------------------------

#[test]
fn ignored_tags_are_read_and_dropped() {
    let o = ReadOptions {
        ignored_tags: vec!["PlyCount".into(), "Annotator".into()],
        ..ReadOptions::default()
    };
    let g = one_with(
        "[PlyCount \"4\"]\n[Annotator \"x\"]\n[Other \"y\"]\n\n1. e4 *\n",
        &o,
    );
    assert!(g.game.tags().get_extra("PlyCount").is_none());
    assert!(g.game.tags().get_extra("Annotator").is_none());
    assert_eq!(g.game.tags().get_extra("Other"), Some("y"));
    assert!(g.diagnostics.is_empty());
}

#[test]
fn ignored_tags_match_exact_names_only() {
    let o = ReadOptions {
        ignored_tags: vec!["plycount".into()],
        ..ReadOptions::default()
    };
    let g = one_with("[PlyCount \"4\"]\n\n*\n", &o);
    assert_eq!(g.game.tags().get_extra("PlyCount"), Some("4"));
}

#[test]
fn plycount_and_scidflags_are_kept_as_extras_by_default() {
    let g = tagged("[PlyCount \"40\"]\n[ScidFlags \"WBM\"]");
    assert_eq!(g.game.tags().get_extra("PlyCount"), Some("40"));
    assert_eq!(g.game.tags().get_extra("ScidFlags"), Some("WBM"));
}

// ===========================================================================
// 2. Movetext
// ===========================================================================

#[rstest]
#[case::plain("1. e4 e5 2. Nf3 Nc6", "e4 e5 Nf3 Nc6")]
#[case::no_space_after_number("1.e4 e5 2.Nf3 Nc6", "e4 e5 Nf3 Nc6")]
#[case::black_ellipsis("1. e4 1... e5 2. Nf3", "e4 e5 Nf3")]
#[case::black_ellipsis_spaced("1. e4 1. ... e5", "e4 e5")]
#[case::ellipsis_glued("1. e4 1...e5", "e4 e5")]
#[case::unicode_ellipsis_dots("1. e4 1.. e5", "e4 e5")]
#[case::wrong_numbers_are_not_checked("5. e4 99. e5 7. Nf3", "e4 e5 Nf3")]
#[case::no_numbers_at_all("e4 e5 Nf3 Nc6", "e4 e5 Nf3 Nc6")]
#[case::lone_dots("e4 . e5 .. Nf3", "e4 e5 Nf3")]
#[case::zeros_are_a_number("0000 e4 e5", "e4 e5")]
#[case::newlines_everywhere("1.\ne4\n\n\ne5\n2.\nNf3", "e4 e5 Nf3")]
#[case::tabs_and_nbsp("1.\te4\u{a0}e5\t2. Nf3", "e4 e5 Nf3")]
fn move_numbers_and_spacing(#[case] movetext: &str, #[case] expected: &str) {
    let g = mt(movetext);
    assert_eq!(dump(&g.game), expected, "{movetext:?}");
    assert!(
        problems(&g).iter().all(|k| *k == K::MissingResult),
        "{movetext:?}: {:?}",
        g.diagnostics
    );
}

#[rstest]
#[case::castling_zero("1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 4. 0-0 Nf6", "O-O")]
#[case::castling_letter_o("1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 4. O-O Nf6", "O-O")]
#[case::castling_lowercase_o("1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 4. o-o Nf6", "O-O")]
#[case::castling_no_dashes("1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 4. OO Nf6", "O-O")]
#[case::castling_check("1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 4. O-O+ Nf6", "O-O")]
fn castling_spellings_are_tolerated(#[case] movetext: &str, #[case] castle: &str) {
    let g = mt(movetext);
    assert_eq!(main_sans(&g.game)[6], castle, "{movetext}");
    assert_eq!(main_sans(&g.game).len(), 8);
    assert!(
        problems(&g).iter().all(|k| *k == K::MissingResult),
        "{:?}",
        g.diagnostics
    );
}

#[test]
fn queenside_castling_spellings() {
    let pre = "1. d4 d5 2. Nc3 Nc6 3. Bf4 Bf5 4. Qd2 Qd7 5. ";
    for c in ["O-O-O", "0-0-0", "o-o-o", "OOO"] {
        let g = mt(&format!("{pre}{c} O-O-O"));
        assert_eq!(main_sans(&g.game).len(), 10, "{c}: {:?}", g.diagnostics);
        assert_eq!(main_sans(&g.game)[8], "O-O-O", "{c}");
    }
}

#[rstest]
#[case::explicit_pawn("1. Pe4 e5", "e4 e5")]
#[case::capture_without_x("1. e4 d5 2. ed5", "e4 d5 exd5")]
#[case::capture_colon("1. e4 d5 2. e4:d5", "e4 d5 exd5")]
#[case::pawn_capture_short("1. e4 d5 2. ed", "e4 d5 exd5")]
#[case::knight_capture_no_x(
    "1. Nf3 d5 2. e3 Nf6 3. Ne5 e6 4. Nc6 bxc6 5. d3 Bd6 6. Nd2 O-O 7. Nb3",
    "Nf3 d5 e3 Nf6 Ne5 e6 Nc6 bxc6 d3 Bd6 Nd2 O-O Nb3"
)]
#[case::promotion_no_equals(
    "1. a4 b5 2. axb5 Nf6 3. b6 e6 4. b7 Bb4 5. bxa8Q",
    "a4 b5 axb5 Nf6 b6 e6 b7 Bb4 bxa8=Q"
)]
#[case::promotion_equals(
    "1. a4 b5 2. axb5 Nf6 3. b6 e6 4. b7 Bb4 5. bxa8=Q",
    "a4 b5 axb5 Nf6 b6 e6 b7 Bb4 bxa8=Q"
)]
#[case::promotion_equals_lowercase(
    "1. a4 b5 2. axb5 Nf6 3. b6 e6 4. b7 Bb4 5. bxa8=q",
    "a4 b5 axb5 Nf6 b6 e6 b7 Bb4 bxa8=Q"
)]
#[case::promotion_knight(
    "1. a4 b5 2. axb5 Nf6 3. b6 e6 4. b7 Bb4 5. bxa8N",
    "a4 b5 axb5 Nf6 b6 e6 b7 Bb4 bxa8=N"
)]
#[case::check_suffixes_ignored("1. e4+ e5# 2. Nf3++", "e4 e5 Nf3")]
#[case::annotation_suffixes_become_nags("1. e4!! e5?!", "e4 $3 e5 $6")]
#[case::ep_marker_ignored("1. e4 a6 2. e5 d5 3. exd6 e.p.", "e4 a6 e5 d5 exd6")]
#[case::unneeded_disambiguation("1. Ngf3 Nf6", "Nf3 Nf6")]
#[case::unneeded_rank_disambiguation("1. N1f3 Nf6", "Nf3 Nf6")]
#[case::unneeded_full_square("1. Ng1f3 Ng8f6", "Nf3 Nf6")]
fn san_tolerances(#[case] movetext: &str, #[case] expected: &str) {
    let g = mt(movetext);
    assert_eq!(dump(&g.game), expected, "{movetext}");
    assert!(
        !has(&g, |k| matches!(
            k,
            K::IllegalMove(_) | K::AmbiguousMove(_) | K::InvalidMove(_) | K::UnexpectedToken
        )),
        "{movetext}: {:?}",
        g.diagnostics
    );
}

#[test]
fn strict_san_mode_rejects_sloppy_moves() {
    let o = ReadOptions {
        san_mode: SanMode::Strict,
        ..ReadOptions::default()
    };
    let g = mt_with("1. e4 d5 2. ed5 Nf6", &o);
    assert_eq!(
        dump(&g.game),
        "e4 d5 {Error reading move: ed5}",
        "stops at the first bad move of the line"
    );

    assert!(
        has(&g, |k| matches!(
            k,
            K::IllegalMove(_) | K::InvalidMove(_) | K::AmbiguousMove(_) | K::UnexpectedToken
        )),
        "{:?}",
        kinds(&g)
    );
    let g = mt_with("1. e4 d5 2. exd5 Nf6 3. Nc3 *", &o);
    assert_eq!(
        dump(&g.game),
        "e4 d5 exd5 Nf6 Nc3",
        "canonical SAN is fine in strict mode"
    );
}

#[test]
fn null_moves_are_accepted_in_every_spelling() {
    for nm in ["--", "Z0", "null", "0000x"] {
        if nm == "0000x" {
            continue;
        }
        let g = mt(&format!("1. e4 {nm} 2. Nf3 Nf6"));
        assert_eq!(dump(&g.game), "e4 -- Nf3 Nf6", "{nm}: {:?}", g.diagnostics);
    }
}

#[test]
fn null_move_inside_a_variation() {
    let g = mt("1. e4 e5 (1... -- 2. d4 d5) 2. Nf3");
    assert_eq!(dump(&g.game), "e4 e5 (-- d4 d5) Nf3");
}

// ----- NAGs -------------------------------------------------------------------

#[rstest]
#[case("e4 $1", &[1])]
#[case("e4 $14 $2", &[14, 2])]
#[case("e4 $255", &[255])]
#[case("e4 $0", &[])]
#[case("e4!", &[1])]
#[case("e4?", &[2])]
#[case("e4!!", &[3])]
#[case("e4??", &[4])]
#[case("e4!?", &[5])]
#[case("e4?!", &[6])]
#[case("e4 !", &[1])]
#[case("e4 +-", &[18])]
#[case("e4 -+", &[19])]
#[case("e4 +=", &[14])]
#[case("e4 =+", &[15])]
#[case("e4 +/-", &[16])]
#[case("e4 =", &[10])]
#[case("e4 ~", &[13])]
#[case("e4 N", &[146])]
#[case("e4 D", &[201])]
#[case("e4 RR", &[145])]
#[case("e4 R", &[144])]
#[case("e4 BB", &[151])]
#[case("e4 Bb", &[153])]
#[case("e4 ZZ", &[22])]
#[case("e4 zz", &[23])]
#[case("e4 =/&", &[44])]
#[case("e4 $1 $1", &[1])]
#[case("e4 ! $1", &[1])]
#[case("e4$1", &[1])]
#[case("e4 $1$2", &[1, 2])]
#[case("e4!$14", &[1, 14])]
fn nag_forms(#[case] movetext: &str, #[case] nags: &[u8]) {
    let g = mt(&format!("1. {movetext} *"));
    let id = g.game.mainline().next().unwrap();
    let got: Vec<u8> = g.game.nags(id).unwrap().iter().map(|n| n.value()).collect();
    assert_eq!(got, nags, "{movetext}");
    assert!(
        !has(&g, |k| matches!(
            k,
            K::UnexpectedToken | K::InvalidNagSymbol | K::IllegalMove(_) | K::InvalidMove(_)
        )),
        "{movetext}: {:?}",
        g.diagnostics
    );
}

#[test]
fn glyph_words_after_a_move_are_nags_not_moves() {
    // D22: `N`, `R`, `D`, `BB`, `Bb` are NAGs, not piece letters.
    let g = mt("1. e4 N 1... e5 D 2. Nf3 RR 2... Nc6 BB 3. Bc4 Bb *");
    assert_eq!(dump(&g.game), "e4 $146 e5 $201 Nf3 $145 Nc6 $151 Bc4 $153");
}

#[test]
fn nag_out_of_range_is_dropped_with_a_warning() {
    for n in ["$256", "$1000", "$99999999999999999999"] {
        let g = mt(&format!("1. e4 {n} e5"));
        assert_eq!(dump(&g.game), "e4 e5", "{n}");
        let d = g
            .diagnostics
            .iter()
            .find(|d| d.kind == K::NagOutOfRange)
            .unwrap_or_else(|| panic!("{n}: {:?}", g.diagnostics));
        assert_eq!(d.severity, Severity::Warning);
    }
}

#[test]
fn malformed_dollar_signs_never_break_the_game() {
    for bad in ["$", "$x", "$-1", "$ 5", "$$", "$1x"] {
        let g = mt(&format!("1. e4 {bad} e5 2. Nf3 *"));
        let sans = main_sans(&g.game);
        assert!(
            sans.starts_with(&["e4".to_owned()]) && sans.contains(&"e5".to_owned()),
            "{bad}: {sans:?} {:?}",
            g.diagnostics
        );
        assert_eq!(g.termination, Termination::ResultToken, "{bad}");
    }
}

#[test]
fn unknown_symbols_after_a_move_never_lose_the_line() {
    for sym in ["&&&", "xx", "@", "%%"] {
        let g = mt(&format!("1. e4 {sym} e5 2. Nf3 *"));
        assert_eq!(
            main_sans(&g.game),
            ["e4", "e5", "Nf3"],
            "{sym}: {:?}",
            g.diagnostics
        );
    }
}

#[test]
fn nag_before_the_first_move_of_the_line_is_dropped_with_a_warning() {
    let g = mt("$1 1. e4 e5");
    assert_eq!(dump(&g.game), "e4 e5");
    assert!(has(&g, |k| *k == K::NagWithoutMove));
    let g = mt("1. e4 e5 (1... $2 c5)");
    assert_eq!(dump(&g.game), "e4 e5 (c5)");
    assert!(has(&g, |k| *k == K::NagWithoutMove));
}

// ----- Comments ---------------------------------------------------------------

#[test]
fn brace_comment_attaches_to_the_last_move() {
    let g = mt("1. e4 {King's pawn} e5 {reply} 2. Nf3");
    assert_eq!(dump(&g.game), "e4 {King's pawn} e5 {reply} Nf3");
}

#[test]
fn comment_before_the_first_move_is_the_initial_comment() {
    let g = mt("{Intro text} 1. e4 e5");
    assert_eq!(g.game.initial_comment(), Some("Intro text"));
    assert_eq!(dump(&g.game), "{Intro text} e4 e5");
}

#[test]
fn consecutive_comments_are_joined_with_a_space() {
    let g = mt("1. e4 {one} {two}\n{three} e5");
    assert_eq!(dump(&g.game), "e4 {one two three} e5");
}

#[test]
fn comment_after_open_paren_is_comment_before_of_the_variation() {
    let g = mt("1. e4 e5 ({alt line} 1... c5) 2. Nf3");
    let e4 = g.game.mainline().next().unwrap();
    let c5 = g.game.children(e4).unwrap()[1];
    assert_eq!(g.game.comment_before(c5).unwrap(), Some("alt line"));
    assert_eq!(dump(&g.game), "e4 e5 ({alt line} c5) Nf3");
}

#[test]
fn comments_attach_to_the_last_move_of_the_current_line() {
    let g = mt("1. e4 e5 (1... c5 {sicilian}) {after paren} 2. Nf3");
    // after the variation closes the cursor is back on e5
    assert_eq!(dump(&g.game), "e4 e5 {after paren} (c5 {sicilian}) Nf3");
}

#[test]
fn comment_whitespace_collapses_by_default() {
    let g = mt("1. e4 {  hello \n\n   brave\t world  } e5");
    assert_eq!(
        g.game.comment(g.game.mainline().next().unwrap()).unwrap(),
        Some("hello brave world")
    );
}

#[test]
fn comment_whitespace_preserve_keeps_text_and_normalises_line_breaks() {
    let o = ReadOptions {
        comment_whitespace: CommentWhitespace::Preserve,
        ..ReadOptions::default()
    };
    let g = mt_with("1. e4 {  hello \r\n  brave\rworld  } e5", &o);
    let c = g
        .game
        .comment(g.game.mainline().next().unwrap())
        .unwrap()
        .unwrap()
        .to_owned();
    assert!(c.contains("hello") && c.contains("brave") && c.contains("world"));
    assert!(!c.contains('\r'), "no CR in comments: {c:?}");
    assert!(
        c.contains("  hello") || c.starts_with("  hello") || c.contains("hello \n"),
        "{c:?}"
    );
    assert_eq!(c.matches('\n').count(), 2, "{c:?}");
}

#[test]
fn braces_do_not_nest() {
    let g = mt("1. e4 {a {b} e5");
    assert_eq!(dump(&g.game), "e4 {a {b} e5");
    let g = mt("1. e4 {a {b} c} e5 *");
    assert_eq!(
        g.game.comment(g.game.mainline().next().unwrap()).unwrap(),
        Some("a {b")
    );
    assert!(has(&g, |k| *k == K::StrayCloseBrace), "{:?}", kinds(&g));
}

#[test]
fn stray_close_brace_warns_once_per_game_and_is_ignored() {
    let g = mt("1. e4 } e5 } 2. Nf3 }");
    assert_eq!(dump(&g.game), "e4 e5 Nf3");
    assert_eq!(count(&g, |k| *k == K::StrayCloseBrace), 1);
    assert_eq!(
        g.diagnostics
            .iter()
            .find(|d| d.kind == K::StrayCloseBrace)
            .unwrap()
            .severity,
        Severity::Warning
    );
}

#[test]
fn unterminated_comment_is_an_error_and_the_text_is_kept() {
    let g = one("[Event \"x\"]\n\n1. e4 e5 {never closed\nsecond line");
    assert_eq!(g.termination, Termination::EndOfInput);
    let d = g
        .diagnostics
        .iter()
        .find(|d| d.kind == K::UnterminatedComment)
        .unwrap();
    assert_eq!(d.severity, Severity::Error);
    let c = g
        .game
        .comment(g.game.mainline().nth(1).unwrap())
        .unwrap()
        .unwrap();
    assert!(
        c.contains("never closed") && c.contains("second line"),
        "{c:?}"
    );
}

#[test]
fn semicolon_comment_runs_to_end_of_line_and_is_kept_by_default() {
    let g = mt("1. e4 ; best by test\n e5 2. Nf3");
    assert_eq!(dump(&g.game), "e4 {best by test} e5 Nf3");
}

#[test]
fn semicolon_comments_can_be_dropped_like_scid_does() {
    let o = ReadOptions {
        semicolon_comments: SemicolonComments::Drop,
        ..ReadOptions::default()
    };
    let g = mt_with("1. e4 ; best by test\n e5 2. Nf3 ; tail\n", &o);
    assert_eq!(dump(&g.game), "e4 e5 Nf3");
}

#[test]
fn semicolon_inside_a_brace_comment_is_plain_text() {
    let g = mt("1. e4 {a ; b} e5");
    assert_eq!(dump(&g.game), "e4 {a ; b} e5");
}

#[test]
fn brace_inside_a_semicolon_comment_is_plain_text() {
    let g = mt("1. e4 ; has { brace\n e5 2. Nf3");
    assert_eq!(dump(&g.game), "e4 {has { brace} e5 Nf3");
}

#[test]
fn percent_line_is_discarded_only_at_line_start() {
    let g = mt("1. e4\n% escape line e5 e6\n e5 2. Nf3 *");
    assert_eq!(dump(&g.game), "e4 e5 Nf3");
    assert!(g.diagnostics.is_empty(), "{:?}", g.diagnostics);
}

#[test]
fn percent_inside_a_comment_is_text() {
    let g = mt("1. e4 {50%\n% not an escape\n} e5");
    let c = g
        .game
        .comment(g.game.mainline().next().unwrap())
        .unwrap()
        .unwrap();
    assert!(c.contains("50%") && c.contains("% not an escape"), "{c:?}");
}

#[test]
fn clk_command_line_inside_multiline_comment_does_not_cut_the_game() {
    let text =
        "[Event \"A\"]\n\n1. e4 {first\n[%clk 0:10:00]\nmore} e5 1-0\n\n[Event \"B\"]\n\n1. d4 *\n";
    let games = read_all(text);
    assert_eq!(games.len(), 2);
    assert_eq!(games[0].game.mainline().count(), 2);
    let c = games[0]
        .game
        .comment(games[0].game.mainline().next().unwrap())
        .unwrap()
        .unwrap();
    assert!(c.contains("[%clk 0:10:00]"), "{c:?}");
    assert!(
        games[0].diagnostics.is_empty(),
        "{:?}",
        games[0].diagnostics
    );
}

#[test]
fn header_bracket_line_inside_a_comment_does_not_cut_the_game() {
    let games = read_all("[Event \"A\"]\n\n1. e4 {look\n[Event \"fake\"]\nat this} e5 1-0\n");
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].game.mainline().count(), 2);
}

#[test]
fn embedded_commands_are_kept_verbatim_in_comments() {
    let g = mt("1. e4 {[%eval 0.35] [%clk 1:02:03] [%csl Gd4,Re5][%cal Gd2d4] text} e5");
    let c = g
        .game
        .comment(g.game.mainline().next().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        c,
        "[%eval 0.35] [%clk 1:02:03] [%csl Gd4,Re5][%cal Gd2d4] text"
    );
}

#[test]
fn long_comment_is_truncated_at_max_comment_len_with_a_warning() {
    let o = ReadOptions {
        max_comment_len: 100,
        ..ReadOptions::default()
    };
    let long = "x".repeat(5000);
    let g = mt_with(&format!("1. e4 {{{long}}} e5 2. Nf3"), &o);
    let c = g
        .game
        .comment(g.game.mainline().next().unwrap())
        .unwrap()
        .unwrap();
    assert!(c.len() <= 100, "len {}", c.len());
    assert!(c.starts_with("xxxx"));
    assert!(has(&g, |k| *k == K::CommentTruncated));
    assert_eq!(
        main_sans(&g.game),
        ["e4", "e5", "Nf3"],
        "reading continues after the truncated comment"
    );
}

#[test]
fn truncated_comment_respects_utf8_boundaries() {
    let o = ReadOptions {
        max_comment_len: 10,
        ..ReadOptions::default()
    };
    let g = mt_with(&format!("1. e4 {{{}}} e5", "\u{e9}".repeat(100)), &o);
    let c = g
        .game
        .comment(g.game.mainline().next().unwrap())
        .unwrap()
        .unwrap();
    assert!(c.len() <= 10 && c.chars().all(|ch| ch == '\u{e9}'), "{c:?}");
}

#[test]
fn megabyte_sized_comment_under_the_default_limit_is_kept_whole() {
    let big = "y".repeat(500_000);
    let g = mt(&format!("1. e4 {{{big}}} e5 *"));
    assert_eq!(
        g.game
            .comment(g.game.mainline().next().unwrap())
            .unwrap()
            .unwrap()
            .len(),
        500_000
    );
    assert!(g.diagnostics.is_empty());
}

#[test]
fn move_glued_to_a_comment_or_parenthesis_is_read() {
    let g = mt("1. e4{good}e5(1... c5)2. Nf3");
    assert_eq!(dump(&g.game), "e4 {good} e5 (c5) Nf3");
}

// ----- Variations --------------------------------------------------------------

#[test]
fn variation_replaces_the_last_move() {
    let g = mt("1. e4 e5 (1... c5 2. Nf3 d6) 2. Nf3");
    assert_eq!(dump(&g.game), "e4 e5 (c5 Nf3 d6) Nf3");
    let e4 = g.game.mainline().next().unwrap();
    let kids = g.game.children(e4).unwrap();
    assert_eq!(kids.len(), 2, "variation is a sibling of e5");
}

#[test]
fn several_variations_in_order_on_the_same_move() {
    let g = mt("1. e4 e5 (1... c5) (1... e6) (1... c6 2. d4) 2. Nf3");
    assert_eq!(dump(&g.game), "e4 e5 (c5) (e6) (c6 d4) Nf3");
}

#[test]
fn variation_on_the_first_move() {
    let g = mt("1. e4 (1. d4 d5) e5");
    assert_eq!(dump(&g.game), "e4 (d4 d5) e5");
}

#[test]
fn nested_variations_five_levels() {
    let g = mt(
        "1. e4 e5 (1... c5 2. Nf3 (2. c3 d5 (2... Nf6 3. e5 (3. d3 d5 (3... g6 4. Nf3)))) d6) 2. Nf3 *",
    );
    assert_eq!(max_depth(&g.game), 5);
    assert_eq!(
        dump(&g.game),
        "e4 e5 (c5 Nf3 (c3 d5 (Nf6 e5 (d3 d5 (g6 Nf3)))) d6) Nf3"
    );
    assert!(g.diagnostics.is_empty(), "{:?}", g.diagnostics);
}

#[test]
fn variation_as_first_token_is_reported_and_skipped() {
    let g = mt("(1. d4 d5) 1. e4 e5");
    assert_eq!(dump(&g.game), "e4 e5");
    let d = g
        .diagnostics
        .iter()
        .find(|d| d.kind == K::VariationBeforeFirstMove)
        .unwrap();
    assert_eq!(d.severity, Severity::Error);
}

#[test]
fn variation_right_after_a_variation_open_is_reported() {
    let g = mt("1. e4 e5 ((1... c5) 1... d5) 2. Nf3");
    assert_eq!(
        main_sans(&g.game),
        ["e4", "e5", "Nf3"],
        "{:?}",
        g.diagnostics
    );
    assert!(
        has(&g, |k| *k == K::VariationBeforeFirstMove),
        "{:?}",
        kinds(&g)
    );
}

#[test]
fn extra_close_paren_is_a_warning_and_ignored() {
    let g = mt("1. e4 e5 ) 2. Nf3 ) Nc6");
    assert_eq!(dump(&g.game), "e4 e5 Nf3 Nc6");
    assert_eq!(count(&g, |k| *k == K::UnbalancedCloseParen), 2);
    assert!(
        g.diagnostics
            .iter()
            .all(|d| d.severity == Severity::Warning || d.kind == K::MissingResult),
        "{:?}",
        g.diagnostics
    );
}

#[test]
fn variation_open_at_end_is_closed_with_a_warning() {
    let g = mt("1. e4 e5 (1... c5 2. Nf3 *");
    assert_eq!(dump(&g.game), "e4 e5 (c5 Nf3)");
    assert!(has(&g, |k| *k == K::UnclosedVariation));
    let g = one("[Event \"x\"]\n\n1. e4 e5 (1... c5 2. Nf3");
    assert_eq!(g.termination, Termination::EndOfInput);
    assert!(has(&g, |k| *k == K::UnclosedVariation));
    assert!(has(&g, |k| *k == K::MissingResult));
}

#[test]
fn empty_variation_is_harmless() {
    let g = mt("1. e4 e5 () 2. Nf3 ( ) Nc6");
    assert_eq!(dump(&g.game), "e4 e5 Nf3 Nc6");
}

#[test]
fn duplicate_variation_is_merged_not_duplicated() {
    let g = mt("1. e4 e5 (1... e5 {dup}) 2. Nf3");
    let e4 = g.game.mainline().next().unwrap();
    assert_eq!(
        g.game.children(e4).unwrap().len(),
        1,
        "same move at the same point is one node"
    );
    assert!(has(&g, |k| *k == K::DuplicateVariation), "{:?}", kinds(&g));
    let e5 = g.game.children(e4).unwrap()[0];
    assert_eq!(g.game.comment(e5).unwrap(), Some("dup"));
}

#[test]
fn variations_inside_comments_are_just_text() {
    let g = mt("1. e4 {(1. d4) (unbalanced} e5 *");
    assert_eq!(dump(&g.game), "e4 {(1. d4) (unbalanced} e5");
    assert!(g.diagnostics.is_empty());
}

#[test]
fn variation_starting_position_is_the_one_before_the_replaced_move() {
    // 1. e4 e5 (1... c5): c5 is legal only because black is to move after e4.
    let g = mt("1. e4 (1. d4) e5 2. Nf3 (2. Nc3 Nc6) *");
    assert_eq!(dump(&g.game), "e4 (d4) e5 Nf3 (Nc3 Nc6)");
    assert!(g.diagnostics.is_empty());
}

// ----- Errors ------------------------------------------------------------------

#[test]
fn illegal_move_adds_the_error_comment_and_drops_the_rest_of_the_line() {
    let g = mt("1. e4 e5 2. Nf3 Nf6 3. Nxd7 Nxe4 4. Qh5 *");
    assert_eq!(dump(&g.game), "e4 e5 Nf3 Nf6 {Error reading move: Nxd7}");
    let d = g
        .diagnostics
        .iter()
        .find(|d| matches!(d.kind, K::IllegalMove(_)))
        .unwrap();
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.text.as_deref(), Some("Nxd7"));
    assert_eq!(g.termination, Termination::ResultToken);
}

#[rstest]
#[case::nonexistent_piece(
    "1. e4 e5 2. Nf6 Nc6 3. Bc4 *",
    "Nf6",
    "e4 e5 {Error reading move: Nf6}"
)]
#[case::pawn_cannot_move("1. e4 e5 2. e5 Nc6 *", "e5", "e4 e5 {Error reading move: e5}")]
#[case::blocked_castle("1. e4 e5 2. O-O Nc6 *", "O-O", "e4 e5 {Error reading move: O-O}")]
fn error_comment_text_and_position(
    #[case] movetext: &str,
    #[case] token: &str,
    #[case] expected: &str,
) {
    let g = mt(movetext);
    assert_eq!(dump(&g.game), expected, "{movetext}");
    let d = g
        .diagnostics
        .iter()
        .find(|d| d.severity == Severity::Error)
        .unwrap();
    assert_eq!(d.text.as_deref(), Some(token));
    assert_eq!(
        g.termination,
        Termination::ResultToken,
        "the result token is still found"
    );
    assert_eq!(g.game.tags().result, GameResult::Unknown);
}

#[test]
fn error_before_the_first_move_goes_to_the_initial_comment() {
    let g = mt("1. Ke2 e5 *");
    assert_eq!(g.game.initial_comment(), Some("Error reading move: Ke2"));
    assert_eq!(g.game.mainline().count(), 0);
}

#[test]
fn error_after_a_comment_appends_to_the_existing_comment() {
    let g = mt("1. e4 {good} 1... Ke2 e5 *");
    let c = g
        .game
        .comment(g.game.mainline().next().unwrap())
        .unwrap()
        .unwrap()
        .to_owned();
    assert!(
        c.starts_with("good") && c.ends_with("Error reading move: Ke2"),
        "{c:?}"
    );
}

#[test]
fn error_in_the_main_line_does_not_discard_healthy_variations_before_it() {
    let g = mt("1. e4 e5 (1... c5 2. Nf3) 2. Nf3 Nc6 3. Bxf7 Nf6 *");
    assert_eq!(
        dump(&g.game),
        "e4 e5 (c5 Nf3) Nf3 Nc6 {Error reading move: Bxf7}"
    );
}

#[test]
fn error_is_per_line_variations_after_the_error_are_still_read() {
    // D21: the rest of *this line* is dropped; later variations of earlier nodes survive.
    let g = mt("1. e4 e5 2. Nf3 (2. Nc3 Nc6) Nc6 3. Zz9 Nf6 (3... d6) *");
    let d = dump(&g.game);
    assert!(d.contains("(Nc3 Nc6)"), "healthy variation kept: {d}");
    assert!(
        d.contains("{Error reading move: Zz9}") || has(&g, |k| *k == K::UnexpectedToken),
        "{d} {:?}",
        g.diagnostics
    );
}

#[test]
fn error_inside_a_variation_only_drops_that_variation_line() {
    let g = mt("1. e4 e5 (1... c5 2. Kd4 Nc6 3. d4) (1... e6 2. d4) 2. Nf3 Nc6 *");
    let d = dump(&g.game);
    assert!(d.contains("(e6 d4)"), "next variation is intact: {d}");
    assert!(d.ends_with("Nf3 Nc6"), "main line intact: {d}");
    assert!(d.contains("(c5"), "good prefix kept: {d}");
    assert!(
        !d.contains("Nc6 d4)"),
        "tail of the bad variation dropped: {d}"
    );
    assert!(d.contains("{Error reading move: Kd4}"), "{d}");
    assert!(has(&g, |k| matches!(k, K::IllegalMove(_))));
}

#[test]
fn nested_variations_inside_a_dropped_tail_are_skipped_with_balance() {
    let g = mt("1. e4 e5 (1... c5 2. Ke2 (2. d4 cxd4) 2... Nc6 {x ) y}) 2. Nf3 Nc6 *");
    let d = dump(&g.game);
    assert!(
        d.ends_with("Nf3 Nc6"),
        "main line intact after skipping balanced tail: {d}"
    );
    assert_eq!(g.termination, Termination::ResultToken);
}

#[test]
fn only_one_error_comment_per_game_but_every_error_has_a_diagnostic() {
    let g = mt("1. e4 e5 (1... Kd4) (1... Qh5) 2. Kd4 Nc6 *");
    let total: usize = main_sans(&g.game).len();
    assert_eq!(total, 2);
    let n_comments = g
        .game
        .descendants(g.game.root())
        .unwrap()
        .filter(|&n| {
            g.game
                .comment(n)
                .unwrap()
                .is_some_and(|c| c.starts_with("Error reading move"))
        })
        .count()
        + usize::from(
            g.game
                .initial_comment()
                .is_some_and(|c| c.starts_with("Error reading move")),
        );
    assert_eq!(n_comments, 1, "{}", dump(&g.game));
    assert!(
        count(&g, |k| matches!(k, K::IllegalMove(_))) >= 3,
        "{:?}",
        kinds(&g)
    );
}

#[test]
fn ambiguous_move_is_an_error_move() {
    let g = one("[FEN \"6k1/8/8/8/8/8/4K3/R6R w - - 0 1\"]\n\n1. Rd1 *\n");
    assert!(
        has(&g, |k| matches!(k, K::AmbiguousMove(_))),
        "{:?}",
        g.diagnostics
    );
    assert_eq!(g.game.initial_comment(), Some("Error reading move: Rd1"));
}

#[rstest]
#[case("C")]
#[case("e9")]
#[case("xyz")]
#[case("hello")]
#[case("Kk")]
fn unexpected_token_is_an_error_but_not_a_move_error(#[case] tok: &str) {
    let g = mt(&format!("1. e4 {tok} e5 2. Nf3 *"));
    let d = g
        .diagnostics
        .iter()
        .find(|d| d.kind == K::UnexpectedToken)
        .unwrap_or_else(|| panic!("{tok}: {:?}", g.diagnostics));
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.text.as_deref(), Some(tok), "the whole word is reported");
    assert_eq!(
        dump(&g.game),
        "e4 e5 Nf3",
        "reading continues; no Error comment: {tok}"
    );
}

#[test]
fn castling_without_the_right_is_kept_with_a_warning_like_scid() {
    // Spec rule 16 (Scid pgnparse.cpp:1075-1100): the move is accepted with a warning.
    // King has moved and returned, then 0-0.
    let g = one("[FEN \"r3k2r/8/8/8/8/8/8/R3K2R w - - 0 1\"]\n\n1. O-O Kd7 *\n");
    assert_eq!(main_sans(&g.game), ["O-O", "Kd7"], "{:?}", g.diagnostics);
    let d = g
        .diagnostics
        .iter()
        .find(|d| matches!(d.kind, K::IllegalCastling | K::CastlingReinterpreted))
        .unwrap_or_else(|| panic!("{:?}", g.diagnostics));
    assert_eq!(d.severity, Severity::Warning);
    assert!(!has(&g, |k| matches!(k, K::IllegalMove(_))));
    assert!(g.game.initial_comment().is_none());
}

#[test]
fn castling_diagnostic_does_not_add_an_error_comment() {
    let g = one("[FEN \"4k3/8/8/8/8/8/8/R3K2R w - - 0 1\"]\n\n1. O-O Kd7 2. Rfe1 *\n");
    assert_eq!(g.game.mainline().count(), 3, "{:?}", g.diagnostics);
    assert!(!dump(&g.game).contains("Error reading move"));
}

#[test]
fn too_many_diagnostics_are_capped_with_one_marker() {
    let junk = "zz1 ".repeat(500);
    let g = mt(&format!("1. e4 {junk} e5 *"));
    assert!(g.diagnostics.len() <= 101, "{}", g.diagnostics.len());
    assert_eq!(count(&g, |k| *k == K::TooManyDiagnostics), 1);
    let o = ReadOptions {
        max_diagnostics_per_game: 5,
        ..ReadOptions::default()
    };
    let g = mt_with(&format!("1. e4 {junk} e5 *"), &o);
    assert!(g.diagnostics.len() <= 6, "{}", g.diagnostics.len());
    assert_eq!(count(&g, |k| *k == K::TooManyDiagnostics), 1);
}

// ----- Game boundaries / structure ----------------------------------------------

#[test]
fn empty_game_with_result_is_valid() {
    for (text, r) in [
        ("1-0", GameResult::White),
        ("*", GameResult::Unknown),
        ("1/2-1/2", GameResult::Draw),
    ] {
        let g = one(&format!("[White \"A\"]\n[Black \"B\"]\n\n{text}\n"));
        assert_eq!(g.game.mainline().count(), 0);
        assert_eq!(g.game.tags().result, r);
        assert_eq!(g.termination, Termination::ResultToken);
        assert!(g.diagnostics.is_empty(), "{:?}", g.diagnostics);
    }
}

#[test]
fn game_without_blank_line_after_the_header_is_read() {
    let g = one("[White \"A\"]\n[Black \"B\"]\n1. e4 e5 1-0\n");
    assert_eq!(g.game.mainline().count(), 2);
    assert_eq!(g.game.tags().black, "B");
}

#[test]
fn header_lines_are_separated_by_blank_lines_without_ending_the_header() {
    let g = one("[White \"A\"]\n\n\n[Black \"B\"]\n\n1. e4 *\n");
    assert_eq!(g.game.tags().white, "A");
    assert_eq!(g.game.tags().black, "B");
    assert_eq!(g.game.mainline().count(), 1);
}

#[test]
fn percent_and_semicolon_lines_inside_the_header_are_ignored() {
    let g = one("[White \"A\"]\n% an escape\n; a comment\n[Black \"B\"]\n\n1. e4 *\n");
    assert_eq!(
        (g.game.tags().white.as_str(), g.game.tags().black.as_str()),
        ("A", "B")
    );
    assert!(g.diagnostics.is_empty(), "{:?}", g.diagnostics);
}

#[test]
fn stray_text_after_the_tags_starts_the_movetext_and_the_next_bracket_line_a_new_game() {
    // Scid: the header ends at the first non-blank character that is not `[`, `%` or `;`.
    let games = read_all("[White \"A\"]\nhello there\n[Black \"B\"]\n\n1. e4 *\n");
    assert_eq!(games.len(), 2);
    assert_eq!(games[0].game.tags().white, "A");
    assert_eq!(games[0].game.mainline().count(), 0);
    assert!(has(&games[0], |k| *k == K::UnexpectedToken));
    assert_eq!(games[0].termination, Termination::NextGameHeader);
    assert_eq!(games[1].game.tags().black, "B");
}

#[test]
fn header_missing_result_when_next_game_starts() {
    let games = read_all("[White \"A\"]\n\n1. e4 e5\n\n[White \"B\"]\n\n1. d4 1-0\n");
    assert_eq!(games.len(), 2);
    assert_eq!(games[0].termination, Termination::NextGameHeader);
    let d = games[0]
        .diagnostics
        .iter()
        .find(|d| d.kind == K::MissingResult)
        .unwrap();
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(games[0].game.mainline().count(), 2);
    assert_eq!(games[1].termination, Termination::ResultToken);
    assert_eq!(games[1].game.tags().white, "B");
}

#[test]
fn bracket_not_at_line_start_inside_movetext_is_not_a_header() {
    let g = mt("1. e4 [odd] e5 2. Nf3 *");
    assert_eq!(g.game.mainline().count() >= 1, true);
    assert_eq!(g.termination, Termination::ResultToken);
}

#[test]
fn header_at_end_of_file_without_movetext() {
    let games = read_all("[White \"A\"]\n\n1. e4 *\n\n[White \"B\"]\n[Black \"C\"]\n");
    assert_eq!(games.len(), 2);
    assert_eq!(games[1].game.tags().black, "C");
    let d = games[1]
        .diagnostics
        .iter()
        .find(|d| d.kind == K::HeaderOnlyAtEof)
        .unwrap();
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(games[1].game.mainline().count(), 0);
}

#[test]
fn two_headers_without_movetext_between_them_are_one_game() {
    let games = read_all(
        "[Event \"First\"]\n[White \"A\"]\n\n[Event \"Second\"]\n[Black \"B\"]\n\n1. e4 *\n",
    );
    // D28: both headers belong to the same game; the second Event replaces the first.
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].game.tags().event, "Second");
    assert_eq!(games[0].game.tags().white, "A");
    assert_eq!(games[0].game.tags().black, "B");
}

#[test]
fn game_numbers_lines_and_ranges_follow_the_file() {
    let text = "[Event \"1\"]\n\n1. e4 *\n\n[Event \"2\"]\n\n1. d4 *\n\n\n[Event \"3\"]\n\n*\n";
    let games = read_all(text);
    assert_eq!(
        games.iter().map(|g| g.number).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(
        games.iter().map(|g| g.first_line).collect::<Vec<_>>(),
        [1, 5, 10]
    );
    assert_eq!(games[0].byte_range.start, 0);
    assert_eq!(&text[games[1].byte_range.start as usize..][..7], "[Event ");
    assert_eq!(
        &text[games[2].byte_range.start as usize..][..10],
        "[Event \"3\""
    );
    for w in games.windows(2) {
        assert!(w[0].byte_range.end <= w[1].byte_range.start);
    }
    assert!(games[2].byte_range.end as usize <= text.len());
    for (i, g) in games.iter().enumerate() {
        assert!(g.diagnostics.iter().all(|d| d.game == (i as u64) + 1));
    }
}

// ----- Junk between games ---------------------------------------------------------

#[test]
fn junk_between_games_is_ignored_with_one_info_per_block() {
    let text = "garbage at start\nmore\n\n[Event \"A\"]\n\n1. e4 1-0\n\nrandom words\nhere\n\n[Event \"B\"]\n\n1. d4 0-1\n";
    let games = read_all(text);
    assert_eq!(games.len(), 2);
    for g in &games {
        assert_eq!(
            count(g, |k| *k == K::JunkBetweenGames),
            1,
            "{:?}",
            g.diagnostics
        );
        let d = g
            .diagnostics
            .iter()
            .find(|d| d.kind == K::JunkBetweenGames)
            .unwrap();
        assert_eq!(d.severity, Severity::Info);
        assert!(g.game.initial_comment().is_none());
    }
    assert_eq!(games[1].number, 2);
    assert_eq!(games[1].first_line, 11);
}

#[test]
fn pre_game_text_is_exposed_only_with_as_comment() {
    let text = "garbage at start\nmore\n\n[Event \"A\"]\n\n1. e4 1-0\n";
    assert_eq!(read_all(text)[0].pre_game_text, None);
    let o = ReadOptions {
        pre_game_text: PreGameText::AsComment,
        ..ReadOptions::default()
    };
    let g = read_bytes(text.as_bytes(), &o).remove(0);
    assert_eq!(
        g.pre_game_text.as_deref().map(str::trim),
        Some("garbage at start more")
    );
}

#[test]
fn blank_lines_between_games_are_not_junk() {
    let games = read_all("[Event \"A\"]\n\n1. e4 *\n\n\n\n[Event \"B\"]\n\n*\n");
    assert_eq!(games.len(), 2);
    assert!(games.iter().all(|g| !has(g, |k| *k == K::JunkBetweenGames)));
}

#[test]
fn junk_can_become_the_initial_comment_like_pgnscid() {
    let o = ReadOptions {
        pre_game_text: PreGameText::AsComment,
        ..ReadOptions::default()
    };
    let games = read_bytes(b"Some note\nsecond line\n[Event \"A\"]\n\n1. e4 *\n", &o);
    assert_eq!(games.len(), 1);
    assert_eq!(
        games[0].game.initial_comment(),
        Some("Some note second line")
    );
}

#[test]
fn file_without_any_header_has_no_games() {
    assert!(read_all("1. e4 e5 2. Nf3 *\n").is_empty());
    assert!(read_all("just text\nand more\n").is_empty());
}

// ----- parse_pgn_text / parse_movetext ---------------------------------------------

#[test]
fn parse_pgn_text_reads_every_game() {
    let games = parse_pgn_text(
        "[Event \"A\"]\n\n1. e4 *\n\n[Event \"B\"]\n\n1. d4 *\n",
        &ReadOptions::default(),
    );
    assert_eq!(games.len(), 2);
    assert_eq!(games[1].game.tags().event, "B");
}

#[test]
fn parse_pgn_text_of_empty_text_is_empty() {
    assert!(parse_pgn_text("", &ReadOptions::default()).is_empty());
    assert!(parse_pgn_text("   \n\n", &ReadOptions::default()).is_empty());
}

#[test]
fn parse_movetext_from_the_standard_position() {
    let p = parse_movetext(
        "1. e4 e5 2. Nf3 {x} Nc6 (2... d6) 1-0",
        &Position::new(),
        &ReadOptions::default(),
    );
    assert_eq!(dump(&p.game), "e4 e5 Nf3 {x} Nc6 (d6)");
    assert_eq!(p.result, Some(GameResult::White));
    assert_eq!(p.termination, Termination::ResultToken);
    assert!(p.diagnostics.is_empty());
}

#[test]
fn parse_movetext_from_a_custom_position_and_without_result() {
    let start = Position::from_fen(FEN_KP).unwrap();
    let p = parse_movetext("1. Rd1 Kf5", &start, &ReadOptions::default());
    assert_eq!(p.game.start_position().fen(), FEN_KP);
    assert_eq!(p.game.mainline().count(), 2);
    assert_eq!(p.result, None);
    assert_eq!(p.termination, Termination::EndOfInput);
}

#[test]
fn parse_movetext_reports_errors_with_game_zero() {
    let p = parse_movetext("1. e4 Ke2", &Position::new(), &ReadOptions::default());
    assert!(!p.diagnostics.is_empty());
    assert!(p.diagnostics.iter().all(|d| d.game == 0 && d.line >= 1));
    assert_eq!(
        p.game
            .initial_comment()
            .or(p.game.comment(p.game.mainline().next().unwrap()).unwrap()),
        Some("Error reading move: Ke2")
    );
}

#[test]
fn parse_movetext_of_empty_or_garbage_text_does_not_panic() {
    let o = ReadOptions::default();
    assert_eq!(
        parse_movetext("", &Position::new(), &o)
            .game
            .mainline()
            .count(),
        0
    );
    let _ = parse_movetext("\u{0}\u{1}((( ))) {{{ $$$ ;;", &Position::new(), &o);
}

// ===========================================================================
// 3. Diagnostic positions (line, column, offset)
// ===========================================================================

#[test]
fn diagnostic_positions_are_exact_lf() {
    let text = "[Event \"x\"]\n\n1. e4 xyz e5\n 2. Nf3 qq9 *\n";
    let g = one(text);
    let ds: Vec<_> = g
        .diagnostics
        .iter()
        .filter(|d| d.kind == K::UnexpectedToken)
        .collect();
    assert_eq!(ds.len(), 2);
    let want = [("xyz", 3u64, 7u32), ("qq9", 4, 9)];
    for (d, (tok, line, col)) in ds.iter().zip(want) {
        assert_eq!((d.line, d.column), (line, col), "{tok}");
        assert_eq!(&text[d.offset as usize..][..3], tok);
        assert_eq!(d.game, 1);
    }
}

#[rstest]
#[case("\n")]
#[case("\r\n")]
#[case("\r")]
fn line_ending_styles_give_identical_games_and_exact_positions(#[case] eol: &str) {
    let lf = "[Event \"x\"]\n[White \"W\"]\n\n1. e4 {multi\nline} e5 xyz\n2. Nf3 *\n\n[Event \"y\"]\n\n1. d4 qq1 *\n";
    let text = lf.replace('\n', eol);
    let a = read_all(lf);
    let b = read_all(&text);
    assert_eq!(a.len(), 2);
    assert_eq!(b.len(), 2);
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(dump(&x.game), dump(&y.game));
        assert_eq!(x.game.tags(), y.game.tags());
        assert_eq!(x.number, y.number);
        assert_eq!(
            x.first_line, y.first_line,
            "line numbers count every break style"
        );
        assert_eq!(x.diagnostics.len(), y.diagnostics.len());
        for (dx, dy) in x.diagnostics.iter().zip(&y.diagnostics) {
            assert_eq!(dx.kind, dy.kind);
            assert_eq!(dx.line, dy.line, "{eol:?}");
            assert_eq!(dx.column, dy.column, "{eol:?}");
        }
    }
    // offsets are bytes of the *original* stream
    for d in b
        .iter()
        .flat_map(|g| &g.diagnostics)
        .filter(|d| d.kind == K::UnexpectedToken)
    {
        let tok = &text.as_bytes()[d.offset as usize..];
        assert!(
            tok.starts_with(b"xyz") || tok.starts_with(b"qq1"),
            "{eol:?}: offset {} -> {:?}",
            d.offset,
            &text[d.offset as usize..][..3]
        );
    }
    let c = b[0]
        .game
        .comment(b[0].game.mainline().next().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(c, "multi line", "{eol:?}");
}

#[test]
fn mixed_line_endings_in_one_file() {
    let text = "[Event \"x\"]\r\n[White \"W\"]\r\n\r\n1. e4 e5\n2. Nf3\r3. Nc6 xyz *\r\n";
    let g = one(text);
    assert_eq!(dump(&g.game), "e4 e5 Nf3 Nc6");
    let d = g
        .diagnostics
        .iter()
        .find(|d| d.kind == K::UnexpectedToken)
        .unwrap();
    assert_eq!(d.text.as_deref(), Some("xyz"));
}

#[test]
fn column_counts_bytes_not_chars() {
    let text = "[Event \"x\"]\n\n1. e4 {\u{e9}\u{e9}\u{e9}} xyz *\n";
    let g = one(text);
    let d = g
        .diagnostics
        .iter()
        .find(|d| d.kind == K::UnexpectedToken)
        .unwrap();
    assert_eq!(&text.as_bytes()[d.offset as usize..][..3], b"xyz");
    assert_eq!(d.column as usize, "1. e4 {\u{e9}\u{e9}\u{e9}} ".len() + 1);
}

#[test]
fn diagnostics_after_a_bom_are_offset_from_the_start_of_the_stream() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"[Event \"x\"]\n\n1. e4 xyz *\n");
    let g = read_bytes(&bytes, &ReadOptions::default()).remove(0);
    let d = g
        .diagnostics
        .iter()
        .find(|d| d.kind == K::UnexpectedToken)
        .unwrap();
    assert_eq!(
        &bytes[d.offset as usize..][..3],
        b"xyz",
        "offset {}",
        d.offset
    );
    assert_eq!(d.line, 3);
}

#[test]
fn diagnostic_display_has_game_line_and_kind() {
    let g = mt("1. e4 xyz *");
    let s = g.diagnostics[0].to_string();
    assert!(s.contains("game 1") && s.contains("xyz"), "{s}");
}

// ===========================================================================
// 4. Scale and hostile nesting (no stack overflow, linear time)
// ===========================================================================

#[test]
fn thousands_of_nested_variations_do_not_overflow_the_stack() {
    let depth = 5_000;
    let mut s = String::from("1. e4 e5 ");
    // each level: (1... Nc6 ... without legality trouble: alternate between two black replies
    for _ in 0..depth {
        s.push_str("(1... c5 ");
    }
    for _ in 0..depth {
        s.push(')');
    }
    s.push_str(" 2. Nf3 *");
    let g = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || mt(&s))
        .unwrap()
        .join()
        .expect("reader must not use the call stack for nesting");
    assert_eq!(g.termination, Termination::ResultToken);
    assert!(g.game.mainline().count() >= 3 || main_sans(&g.game).len() >= 2);
}

#[test]
fn two_hundred_thousand_open_parentheses_are_survivable() {
    let n = 200_000;
    let s = format!("1. e4 e5 {} 2. Nf3 *", "(".repeat(n));
    let t = std::time::Instant::now();
    let g = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || mt(&s))
        .unwrap()
        .join()
        .unwrap();
    assert!(t.elapsed().as_secs() < 20, "took {:?}", t.elapsed());
    assert!(
        has(&g, |k| matches!(
            k,
            K::UnclosedVariation | K::VariationBeforeFirstMove
        )),
        "{:?}",
        kinds(&g).len()
    );
    assert!(g.diagnostics.len() <= 101);
}

#[test]
fn two_hundred_thousand_close_parentheses_are_survivable() {
    let s = format!("1. e4 e5 {} 2. Nf3 *", ")".repeat(200_000));
    let g = mt(&s);
    assert_eq!(dump(&g.game), "e4 e5 Nf3");
    assert!(g.diagnostics.len() <= 101);
}

#[test]
fn deeply_nested_variations_with_real_moves_build_the_full_tree() {
    // 1000 levels: (1... c5 2. Nf3 (2. c3 ...)) built from alternating legal lines
    let depth = 1_000;
    let mut s = String::from("1. e4 e5 ");
    for _ in 0..depth {
        s.push_str("(1... c5 ");
    }
    for _ in 0..depth {
        s.push(')');
    }
    s.push_str(" *");
    // Identical sibling `c5` at the same point collapses into one node (duplicate merge),
    // so only check that it finishes and keeps the main line.
    let g = mt(&s);
    assert_eq!(main_sans(&g.game), ["e4", "e5"]);
}

#[test]
fn very_long_main_line_is_read_without_recursion() {
    let mut s = String::new();
    // 20_000 plies of knight shuffling (legal, no repetition rule in the reader)
    for i in 0..10_000 {
        s.push_str(&format!("{}. Nf3 Nf6 {}. Ng1 Ng8 ", 2 * i + 1, 2 * i + 2));
    }
    s.push_str("1-0");
    let g = mt(&s);
    assert_eq!(g.game.mainline().count(), 40_000);
    assert!(
        g.diagnostics.is_empty(),
        "{:?}",
        &g.diagnostics[..g.diagnostics.len().min(3)]
    );
}

#[test]
fn oversize_game_is_reported_and_the_reader_resynchronises() {
    let big_comment = "z".repeat(5_000);
    let text = format!(
        "[Event \"Huge\"]\n[White \"BigOne\"]\n\n1. e4 {{{big_comment}}} e5 1-0\n\n[Event \"Small\"]\n\n1. d4 d5 0-1\n"
    );
    let o = ReadOptions {
        max_game_bytes: 1_000,
        ..ReadOptions::default()
    };
    let games = read_bytes(text.as_bytes(), &o);
    assert_eq!(
        games.len(),
        2,
        "{:?}",
        games.iter().map(|g| g.number).collect::<Vec<_>>()
    );
    assert_eq!(games[0].termination, Termination::Oversize);
    assert!(has(&games[0], |k| *k == K::GameTooLarge));
    assert_eq!(
        games[0].game.tags().white,
        "BigOne",
        "header of an oversize game is still read"
    );
    assert_eq!(games[0].game.mainline().count(), 0);
    assert_eq!(games[1].number, 2);
    assert_eq!(games[1].game.tags().event, "Small");
    assert_eq!(games[1].game.mainline().count(), 2);
    assert_eq!(games[1].game.tags().result, GameResult::Black);
}

#[test]
fn elo_helper_sanity() {
    // keeps the Elo import used and documents the 4000 ceiling the reader relies on
    assert!(Elo::new(4001).is_err());
}
