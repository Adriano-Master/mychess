//! Spec m2-1, rules 44-54: keyboard move entry (candidates, completion,
//! Enter/Backspace/Escape) and free move text.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m2_1_common;

use std::collections::BTreeSet;

use m2_1_common::*;
use mychess_app::{
    EditSettings, GameMessage, GameSession, MoveEntrySettings, Notice, Prompt, ReplaceChoice,
};
use mychess_core::CastlingSide;
use rstest::rstest;

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const BOTH_CASTLES: &str = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
const LONG_ONLY: &str = "r3k2r/8/8/8/8/8/8/R3K2R w Q - 0 1";
const SHORT_ONLY: &str = "r3k2r/8/8/8/8/8/8/R3K2R w K - 0 1";
const PROMO: &str = "7k/P7/8/8/8/8/8/K7 w - - 0 1";
const PAWN_B_VS_BISHOP: &str = "4k3/8/8/8/2p5/1P1B4/8/4K3 w - - 0 1";
const C960_CASTLES: &str = "4k3/8/8/8/8/8/8/1R2K1R1 w GB - 0 1";

fn with_entry(f: impl FnOnce(&mut MoveEntrySettings)) -> EditSettings {
    let mut e = EditSettings::default();
    f(&mut e.move_entry);
    e
}

fn texts(s: &GameSession) -> BTreeSet<String> {
    s.move_entry()
        .candidates()
        .iter()
        .map(|c| c.text.clone())
        .collect()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn last_move(s: &GameSession) -> Move {
    s.game().mv(s.cursor()).unwrap()
}

// ---- rule 54 and candidate tables (rule 46) ----

#[test]
fn start_position_e_prefix_offers_san_and_coordinates() {
    let mut s = session_fen(START);
    type_text(&mut s, "e");
    assert_eq!(s.move_entry().text(), "e");
    assert_eq!(texts(&s), set(&["e3", "e4", "e2e3", "e2e4"]));
}

#[test]
fn start_position_e_prefix_without_coordinate_entry_offers_only_san() {
    let mut s = session_fen(START);
    s.update(GameMessage::ApplySettings(with_entry(|m| {
        m.coordinate = false;
    })));
    type_text(&mut s, "e");
    assert_eq!(texts(&s), set(&["e3", "e4"]));
}

#[test]
fn start_position_o_has_no_candidates_because_castling_is_illegal() {
    let mut s = session_fen(START);
    type_text(&mut s, "O");
    assert_eq!(s.move_entry().text(), "", "character rejected");
    assert!(s.move_entry().candidates().is_empty());
}

#[test]
fn nf_has_a_single_candidate_and_waits_without_auto_expand() {
    let mut s = session_fen(START);
    type_text(&mut s, "Nf");
    assert_eq!(texts(&s), set(&["Nf3"]));
    assert_eq!(s.game().node_count(), 1, "not played yet");
}

#[test]
fn nf_completes_with_auto_expand() {
    let mut s = session_fen(START);
    s.update(GameMessage::ApplySettings(with_entry(|m| {
        m.auto_expand = true;
    })));
    type_text(&mut s, "Nf");
    assert_eq!(s.game().node_count(), 2);
    assert_eq!(s.game().ply(s.cursor()).unwrap(), 1);
    assert!(s.move_entry().text().is_empty());
    assert!(s.move_entry().candidates().is_empty());
}

#[test]
fn typing_the_full_text_plays_the_move_without_auto_expand() {
    let mut s = session_fen(START);
    type_text(&mut s, "Nf3");
    assert_eq!(s.game().node_count(), 2);
    assert_eq!(s.undo_depth(), 1);
    assert!(s.is_dirty());
}

#[test]
fn typing_full_pawn_move_plays_it() {
    let mut s = session_fen(START);
    type_text(&mut s, "e4");
    assert_eq!(s.game().node_count(), 2);
    assert!(matches!(
        last_move(&s),
        Move::Normal {
            role: Role::Pawn,
            ..
        }
    ));
}

#[test]
fn coordinate_move_is_typed_and_played() {
    let mut s = session_fen(START);
    type_text(&mut s, "e2");
    assert_eq!(texts(&s), set(&["e2e3", "e2e4"]));
    type_text(&mut s, "e4");
    assert_eq!(s.game().node_count(), 2);
}

#[test]
fn coordinate_digits_are_rejected_when_coordinate_entry_is_off() {
    let mut s = session_fen(START);
    s.update(GameMessage::ApplySettings(with_entry(|m| {
        m.coordinate = false;
    })));
    type_text(&mut s, "e2");
    assert_eq!(s.move_entry().text(), "e", "'2' has no candidate");
}

#[test]
fn rejected_character_restores_text_and_candidates() {
    let mut s = session_fen(START);
    type_text(&mut s, "e");
    let before = (s.move_entry().text().to_owned(), texts(&s));
    type_text(&mut s, "5"); // "e5": no legal move
    assert_eq!((s.move_entry().text().to_owned(), texts(&s)), before);
}

#[test]
fn capture_text_omits_the_x() {
    let g = line(&["e2e4", "d7d5"]);
    let last = g.mainline().last().unwrap();
    let mut s = GameSession::new(g);
    goto(&mut s, last);
    type_text(&mut s, "ed");
    assert_eq!(texts(&s), set(&["ed5"]));
    type_text(&mut s, "5");
    assert_eq!(s.game().node_count(), 4);
    assert!(last_move(&s).is_capture());
}

#[test]
fn promotion_has_four_candidates_without_the_equals_sign() {
    let mut s = session_fen(PROMO);
    type_text(&mut s, "a8");
    assert_eq!(texts(&s), set(&["a8Q", "a8R", "a8B", "a8N"]));
}

#[rstest]
#[case('q', Role::Queen)]
#[case('Q', Role::Queen)]
#[case('r', Role::Rook)]
#[case('n', Role::Knight)]
#[case('N', Role::Knight)]
#[case('B', Role::Bishop)]
#[case('b', Role::Bishop)] // D2-82(d): lower-case b falls back to the bishop
fn promotion_piece_letter_completes_the_move(#[case] letter: char, #[case] role: Role) {
    let mut s = session_fen(PROMO);
    type_text(&mut s, "a8");
    s.update(GameMessage::MoveEntryChar(letter));
    match last_move(&s) {
        Move::Normal { promotion, .. } => assert_eq!(promotion, Some(role)),
        other => panic!("not a promotion: {other:?}"),
    }
}

#[test]
fn coordinate_promotion_candidates_use_an_uppercase_piece_letter() {
    let mut s = session_fen(PROMO);
    type_text(&mut s, "a7");
    assert_eq!(texts(&s), set(&["a7a8Q", "a7a8R", "a7a8B", "a7a8N"]));
}

// ---- castling (rules 46, 54) ----

#[test]
fn both_castles_legal_short_has_oo_and_ok_and_long_has_only_oq() {
    let mut s = session_fen(BOTH_CASTLES);
    type_text(&mut s, "O");
    assert_eq!(texts(&s), set(&["OO", "OK", "OQ"]));
}

#[rstest]
#[case("OO", CastlingSide::KingSide)]
#[case("OK", CastlingSide::KingSide)]
#[case("OQ", CastlingSide::QueenSide)]
fn castling_text_plays_the_right_side(#[case] typed: &str, #[case] side: CastlingSide) {
    let mut s = session_fen(BOTH_CASTLES);
    type_text(&mut s, typed);
    assert_eq!(s.game().node_count(), 2);
    assert_eq!(last_move(&s).castling_side(), Some(side));
}

#[test]
fn lower_case_o_and_k_are_normalized_for_castling() {
    let mut s = session_fen(BOTH_CASTLES);
    type_text(&mut s, "oq");
    assert_eq!(last_move(&s).castling_side(), Some(CastlingSide::QueenSide));
}

#[test]
fn only_long_castling_legal_makes_oo_mean_the_long_castle() {
    let mut s = session_fen(LONG_ONLY);
    type_text(&mut s, "O");
    assert_eq!(texts(&s), set(&["OO", "OQ"]));
    type_text(&mut s, "O");
    assert_eq!(last_move(&s).castling_side(), Some(CastlingSide::QueenSide));
}

#[test]
fn only_long_castling_legal_rejects_ok() {
    let mut s = session_fen(LONG_ONLY);
    type_text(&mut s, "OK");
    assert_eq!(s.move_entry().text(), "O");
    assert_eq!(s.game().node_count(), 1);
}

#[test]
fn only_short_castling_legal_rejects_oq() {
    let mut s = session_fen(SHORT_ONLY);
    type_text(&mut s, "OQ");
    assert_eq!(s.move_entry().text(), "O");
    assert_eq!(texts(&s), set(&["OO", "OK"]));
}

#[test]
fn dash_castling_notation_is_never_a_candidate() {
    let mut s = session_fen(BOTH_CASTLES);
    type_text(&mut s, "O");
    s.update(GameMessage::MoveEntryChar('-'));
    assert_eq!(s.move_entry().text(), "O");
    assert!(texts(&s).iter().all(|t| !t.contains('-')));
}

#[test]
fn chess960_castling_candidates_and_moves() {
    let mut s = session_fen(C960_CASTLES);
    type_text(&mut s, "O");
    assert_eq!(texts(&s), set(&["OO", "OK", "OQ"]));
    type_text(&mut s, "Q");
    match last_move(&s) {
        Move::Castle { rook_from, .. } => assert_eq!(rook_from, sq("b1")),
        other => panic!("expected castling, got {other:?}"),
    }
    let mut s = session_fen(C960_CASTLES);
    type_text(&mut s, "OK");
    match last_move(&s) {
        Move::Castle { rook_from, .. } => assert_eq!(rook_from, sq("g1")),
        other => panic!("expected castling, got {other:?}"),
    }
}

#[test]
fn chess960_start_position_has_regular_candidates() {
    let position = Position::chess960_start(518).unwrap();
    let mut s = GameSession::new(Game::from_position(position));
    type_text(&mut s, "e");
    assert_eq!(texts(&s), set(&["e3", "e4", "e2e3", "e2e4"]));
    type_text(&mut s, "O");
    assert_eq!(s.move_entry().text(), "e", "no castling at the start");
}

// ---- pawn b x bishop B (rule 47, D2-82 c/d) ----

#[test]
fn b_prefix_matches_pawn_and_bishop_moves() {
    let mut s = session_fen(PAWN_B_VS_BISHOP);
    type_text(&mut s, "b");
    let t = texts(&s);
    assert!(t.contains("bc4") && t.contains("b4"), "pawn moves: {t:?}");
    assert!(
        t.contains("Bc4") && t.contains("Be2"),
        "bishop moves: {t:?}"
    );
}

#[test]
fn lower_case_bc4_plays_the_pawn_capture() {
    let mut s = session_fen(PAWN_B_VS_BISHOP);
    type_text(&mut s, "bc4");
    assert_eq!(s.game().node_count(), 2);
    assert!(matches!(
        last_move(&s),
        Move::Normal {
            role: Role::Pawn,
            capture: Some(_),
            ..
        }
    ));
}

#[test]
fn upper_case_bc4_plays_the_bishop_capture() {
    let mut s = session_fen(PAWN_B_VS_BISHOP);
    type_text(&mut s, "Bc4");
    assert_eq!(s.game().node_count(), 2);
    assert!(matches!(
        last_move(&s),
        Move::Normal {
            role: Role::Bishop,
            capture: Some(_),
            ..
        }
    ));
}

#[test]
fn partial_b_then_c_waits() {
    let mut s = session_fen(PAWN_B_VS_BISHOP);
    type_text(&mut s, "bc");
    assert_eq!(s.game().node_count(), 1);
    assert_eq!(s.move_entry().text(), "bc");
}

#[test]
fn lower_case_b_is_a_prefix_of_bishop_moves_when_no_pawn_can_move() {
    // Only the bishop on c1 can move; no pawn on the b-file.
    let mut s = session_fen("4k3/8/8/8/8/8/8/2B1K3 w - - 0 1");
    type_text(&mut s, "b");
    assert!(texts(&s).contains("Bb2"), "{:?}", texts(&s));
}

#[test]
fn lower_case_b_after_a_square_falls_back_to_bishop() {
    // "a8b" has no candidate, but "a8B" has: D2-82(d).
    let mut s = session_fen(PROMO);
    type_text(&mut s, "a8");
    s.update(GameMessage::MoveEntryChar('b'));
    assert_eq!(s.game().node_count(), 2);
}

// ---- normalization (rule 45) ----

#[rstest]
#[case::knight_lower('n', "N")]
#[case::knight_upper('N', "N")]
#[case::file_upper('E', "e")]
#[case::file_upper_a('A', "a")]
fn start_position_normalizes_typed_letters(#[case] typed: char, #[case] text: &str) {
    let mut s = session_fen(START);
    s.update(GameMessage::MoveEntryChar(typed));
    assert_eq!(s.move_entry().text(), text);
}

#[rstest]
#[case('q', "Q")]
#[case('Q', "Q")]
#[case('k', "K")]
#[case('K', "K")]
#[case('r', "R")]
fn pieces_with_moves_are_upper_cased(#[case] typed: char, #[case] text: &str) {
    let mut s = session_fen("4k3/8/8/8/8/8/R7/Q3K3 w - - 0 1");
    s.update(GameMessage::MoveEntryChar(typed));
    assert_eq!(s.move_entry().text(), text);
}

#[rstest]
#[case('x')]
#[case('=')]
#[case('+')]
#[case('-')]
#[case('#')]
#[case('0')]
#[case('9')]
#[case(' ')]
#[case('\n')]
#[case('\0')]
#[case('z')]
#[case('é')]
#[case('♘')]
#[case('\u{202e}')]
fn other_characters_are_ignored_without_a_notice(#[case] ch: char) {
    let mut s = session_fen(START);
    s.update(GameMessage::MoveEntryChar(ch));
    assert_eq!(s.move_entry().text(), "");
    assert!(s.move_entry().candidates().is_empty());
    assert!(s.notice().is_none());
    assert_eq!(s.game().node_count(), 1);
}

#[test]
fn ignored_characters_in_the_middle_do_not_disturb_the_text() {
    let mut s = session_fen(START);
    type_text(&mut s, "N-x f=+3");
    assert_eq!(s.game().node_count(), 2, "typed Nf3 with noise");
}

#[test]
fn entry_disabled_ignores_every_key() {
    let mut s = session_fen(START);
    s.update(GameMessage::ApplySettings(with_entry(|m| {
        m.enabled = false;
    })));
    type_text(&mut s, "e4");
    s.update(GameMessage::MoveEntryCommit);
    assert_eq!(s.move_entry().text(), "");
    assert_eq!(s.game().node_count(), 1);
}

// ---- Backspace / Escape / Enter (rules 48-50) ----

#[test]
fn backspace_removes_the_last_character_and_recomputes() {
    let mut s = session_fen(START);
    type_text(&mut s, "e2");
    assert_eq!(texts(&s), set(&["e2e3", "e2e4"]));
    s.update(GameMessage::MoveEntryBackspace);
    assert_eq!(s.move_entry().text(), "e");
    assert_eq!(texts(&s), set(&["e3", "e4", "e2e3", "e2e4"]));
    s.update(GameMessage::MoveEntryBackspace);
    assert_eq!(s.move_entry().text(), "");
    assert!(s.move_entry().candidates().is_empty());
    s.update(GameMessage::MoveEntryBackspace);
    assert_eq!(s.move_entry().text(), "");
}

#[test]
fn escape_clears_text_and_candidates() {
    let mut s = session_fen(START);
    type_text(&mut s, "N");
    s.update(GameMessage::MoveEntryClear);
    assert_eq!(s.move_entry().text(), "");
    assert!(s.move_entry().candidates().is_empty());
}

#[test]
fn enter_with_a_single_candidate_plays_it() {
    let mut s = session_fen(START);
    type_text(&mut s, "Nf");
    s.update(GameMessage::MoveEntryCommit);
    assert_eq!(s.game().node_count(), 2);
    assert!(s.move_entry().text().is_empty());
}

#[test]
fn enter_with_several_candidates_does_nothing() {
    let mut s = session_fen(START);
    type_text(&mut s, "e");
    let before = texts(&s);
    s.update(GameMessage::MoveEntryCommit);
    assert_eq!(s.game().node_count(), 1);
    assert_eq!(s.move_entry().text(), "e");
    assert_eq!(texts(&s), before);
}

#[test]
fn enter_with_empty_text_does_nothing() {
    let mut s = session_fen(START);
    s.update(GameMessage::MoveEntryCommit);
    assert_eq!(s.game().node_count(), 1);
    assert!(s.notice().is_none());
}

// ---- integration with the play flow (rule 51-52) ----

#[test]
fn completing_a_move_at_a_branch_opens_the_replace_prompt_and_clears_the_entry() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    type_text(&mut s, "d5");
    assert!(matches!(s.prompt(), Some(Prompt::ReplaceMove(_))));
    assert!(s.move_entry().text().is_empty());
    assert_eq!(s.game().node_count(), nested_game().0.node_count());
    answer(&mut s, ReplaceChoice::AddVariation);
    assert_eq!(children(&s, n.m1).len(), 3);
}

#[test]
fn completing_an_existing_move_by_keyboard_navigates() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    type_text(&mut s, "e5");
    assert_eq!(s.cursor(), n.m2);
    assert_eq!(s.undo_depth(), 0);
}

#[test]
fn edits_navigation_and_undo_clear_the_entry() {
    let (mut s, n) = nested_session();
    type_text(&mut s, "e");
    s.update(GameMessage::SetComment {
        node: n.m1,
        text: "x".into(),
    });
    assert_eq!(s.move_entry().text(), "");
    type_text(&mut s, "e");
    s.update(GameMessage::Undo);
    assert_eq!(s.move_entry().text(), "");
}

#[test]
fn entry_in_a_position_without_moves_has_no_candidates() {
    // Stalemate: Black to move has no legal moves.
    let mut s = session_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
    type_text(&mut s, "Kg");
    assert_eq!(s.move_entry().text(), "");
}

// ---- EnterMoveText (rule 53) ----

fn text_move(s: &mut GameSession, text: &str) {
    s.update(GameMessage::EnterMoveText(text.to_owned()));
}

#[rstest]
#[case("e4")]
#[case("e2e4")]
#[case("  e4  ")]
fn enter_move_text_accepts_san_and_uci(#[case] text: &str) {
    let mut s = GameSession::new(Game::new());
    text_move(&mut s, text);
    assert_eq!(s.game().node_count(), 2, "{text:?}");
    assert!(s.notice().is_none());
}

#[test]
fn enter_move_text_tolerates_spurious_check_suffix() {
    let mut s = GameSession::new(Game::new());
    text_move(&mut s, "Nf3+");
    assert_eq!(s.game().node_count(), 2);
}

#[rstest]
#[case("O-O")]
#[case("0-0")]
#[case("0-0-0")]
#[case("O-O-O")]
fn enter_move_text_accepts_castling_spellings(#[case] text: &str) {
    let mut s = session_fen(BOTH_CASTLES);
    text_move(&mut s, text);
    assert!(last_move(&s).is_castle(), "{text}");
}

#[test]
fn enter_move_text_castling_without_right_is_illegal() {
    let mut s = GameSession::new(Game::new());
    text_move(&mut s, "O-O");
    assert_eq!(s.notice(), Some(&Notice::IllegalMove));
    assert_eq!(s.game().node_count(), 1);
}

#[test]
fn enter_move_text_null_move() {
    let mut s = GameSession::new(Game::new());
    text_move(&mut s, "--");
    assert!(last_move(&s).is_null());
}

#[test]
fn enter_move_text_null_move_in_check_is_refused() {
    let mut s = session_fen("4k3/8/8/8/8/8/4r3/4K3 w - - 0 1");
    text_move(&mut s, "--");
    assert_eq!(s.notice(), Some(&Notice::NullMoveInCheck));
}

#[test]
fn enter_move_text_promotion_san() {
    let mut s = session_fen(PROMO);
    text_move(&mut s, "a8=N");
    assert!(matches!(
        last_move(&s),
        Move::Normal {
            promotion: Some(Role::Knight),
            ..
        }
    ));
}

#[rstest]
#[case("")]
#[case("   ")]
#[case("\t\n")]
fn enter_move_text_blank_does_nothing(#[case] text: &str) {
    let mut s = GameSession::new(Game::new());
    text_move(&mut s, text);
    assert_eq!(s.game().node_count(), 1);
    assert!(s.notice().is_none());
}

#[rstest]
#[case("zzz")]
#[case("e9")]
#[case("♘f3")]
#[case("\0\0")]
#[case("e4 e5")]
fn enter_move_text_garbage_is_unreadable(#[case] text: &str) {
    let mut s = GameSession::new(Game::new());
    text_move(&mut s, text);
    assert!(
        matches!(s.notice(), Some(Notice::UnreadableMove(_))),
        "{text:?} -> {:?}",
        s.notice()
    );
    assert_eq!(s.game().node_count(), 1);
}

#[test]
fn enter_move_text_of_one_mebibyte_does_not_panic() {
    let mut s = GameSession::new(Game::new());
    text_move(&mut s, &"é".repeat(512 * 1024));
    assert!(matches!(s.notice(), Some(Notice::UnreadableMove(_))));
    text_move(&mut s, &"e4 ".repeat(350_000));
    assert!(matches!(s.notice(), Some(Notice::UnreadableMove(_))));
    assert_eq!(s.game().node_count(), 1);
}

#[test]
fn enter_move_text_follows_the_branch_flow() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    text_move(&mut s, "e5");
    assert_eq!(s.cursor(), n.m2, "existing move navigates");
    goto(&mut s, n.m1);
    text_move(&mut s, "d5");
    assert!(matches!(s.prompt(), Some(Prompt::ReplaceMove(_))));
}

#[test]
fn enter_move_text_works_even_with_entry_disabled() {
    let mut s = GameSession::new(Game::new());
    s.update(GameMessage::ApplySettings(with_entry(|m| {
        m.enabled = false;
    })));
    text_move(&mut s, "e4");
    assert_eq!(s.game().node_count(), 2);
}
