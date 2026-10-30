//! Set-up-position dialog (spec M2-2, rules 1-12 and D105-D113).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::default_trait_access,
    clippy::format_push_string,
    clippy::items_after_statements,
    clippy::assigning_clones,
    clippy::field_reassign_with_default,
    clippy::type_complexity,
    let_underscore_drop
)]

mod m2_2_common;

use m2_2_common::*;
use mychess_app::{
    CastlingFlags, Direction, SetupAnalysis, SetupBoard, SetupMessage as S, SetupProblem,
    SetupWarning, analyze_setup,
};
use mychess_core::{CastlingSide, Color, FenMode, File};
use proptest::prelude::*;
use rstest::rstest;

fn start() -> SetupBoard {
    SetupBoard::from_position(&Position::new())
}

fn cleared() -> SetupBoard {
    let mut b = start();
    b.apply(S::Clear);
    b
}

/// Puts `p` on `at` (the square must be empty).
fn put(b: &mut SetupBoard, p: char, at: &str) {
    b.apply(S::SelectBrush(piece(p)));
    b.apply(S::ClickSquare(sq(at)));
}

fn problem(b: &SetupBoard) -> SetupProblem {
    match &b.analysis {
        SetupAnalysis::Invalid(p) => p.clone(),
        other => panic!("expected an invalid analysis, got {other:?}"),
    }
}

fn warnings(b: &SetupBoard) -> Vec<SetupWarning> {
    match &b.analysis {
        SetupAnalysis::Valid { warnings, .. } => warnings.clone(),
        other => panic!("expected a valid analysis, got {other:?}"),
    }
}

// ------------------------------------------------------------------ rule 1

proptest! {
    #[test]
    fn rule1_from_position_then_analyze_round_trips(p in mychess_core::test_support::arb_position()) {
        // The dialog's counters are limited to 0..=999 (D111).
        prop_assume!(p.fullmove_number() <= 999 && p.halfmove_clock() <= 999);
        let board = SetupBoard::from_position(&p);
        match analyze_setup(&board) {
            SetupAnalysis::Valid { position, fen, .. } => {
                prop_assert_eq!(&position, &p);
                // The generator may set an en passant square whose capture is
                // illegal (pinned pawn); the dialog drops it (D107), so the
                // FEN is the one of the analysed position.
                prop_assert_eq!(fen, position.fen());
            }
            other => prop_assert!(false, "not valid: {:?}", other),
        }
    }
}

#[test]
fn rule1_standard_start_round_trips() {
    let b = start();
    match &b.analysis {
        SetupAnalysis::Valid {
            position,
            fen,
            warnings,
        } => {
            assert_eq!(position, &Position::new());
            assert_eq!(fen, START_FEN);
            assert!(warnings.is_empty());
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        b.fen_text, START_FEN,
        "the displayed text is the canonical FEN"
    );
}

// ------------------------------------------------------------------ rule 2

#[test]
fn rule2_click_puts_then_erases_then_replaces() {
    let mut b = cleared();
    put(&mut b, 'N', "c3");
    assert_eq!(b.piece_at(sq("c3")), Some(piece('N')));
    b.apply(S::ClickSquare(sq("c3")));
    assert_eq!(b.piece_at(sq("c3")), None, "same piece again erases");
    put(&mut b, 'N', "c3");
    b.apply(S::SelectBrush(piece('q')));
    b.apply(S::ClickSquare(sq("c3")));
    assert_eq!(
        b.piece_at(sq("c3")),
        Some(piece('q')),
        "another piece replaces"
    );
}

#[rstest]
#[case('P', "a1")]
#[case('P', "h8")]
#[case('p', "e1")]
#[case('p', "e8")]
fn rule2_pawns_are_refused_on_back_ranks(#[case] p: char, #[case] at: &str) {
    let mut b = cleared();
    let before = b.clone();
    put(&mut b, p, at);
    assert_eq!(b.piece_at(sq(at)), None);
    assert_eq!(b.squares, before.squares);
}

#[test]
fn rule2_pawn_on_back_rank_does_not_erase_an_existing_piece() {
    let mut b = cleared();
    put(&mut b, 'N', "a1");
    b.apply(S::SelectBrush(piece('P')));
    b.apply(S::ClickSquare(sq("a1")));
    assert_eq!(
        b.piece_at(sq("a1")),
        Some(piece('N')),
        "gesture ignored in silence"
    );
}

// ------------------------------------------------------------------ rule 3

#[test]
fn rule3_right_click_erases_and_takes_the_piece_as_brush() {
    let mut b = start();
    b.apply(S::RightClickSquare(sq("e2")));
    assert_eq!(b.piece_at(sq("e2")), None);
    assert_eq!(b.brush, piece('P'));
    b.apply(S::RightClickSquare(sq("g8")));
    assert_eq!(b.brush, piece('n'));
}

#[test]
fn rule3_right_click_on_empty_square_changes_nothing() {
    let mut b = start();
    b.apply(S::SelectBrush(piece('q')));
    let before = b.clone();
    b.apply(S::RightClickSquare(sq("e4")));
    assert_eq!(b, before);
}

#[test]
fn rule3_middle_click_copies_the_piece_without_changing_the_board() {
    let mut b = start();
    let squares = b.squares;
    b.apply(S::MiddleClickSquare(sq("d8")));
    assert_eq!(b.brush, piece('q'));
    assert_eq!(b.squares, squares);
    b.apply(S::MiddleClickSquare(sq("d5")));
    assert_eq!(b.brush, piece('q'), "empty square: nothing");
}

// ------------------------------------------------------------------ rule 4

#[test]
fn rule4_cycle_order_is_kqrbnp_then_kqrbnp_black() {
    let mut b = start();
    b.apply(S::SelectBrush(piece('K')));
    let mut seen = String::new();
    for _ in 0..12 {
        seen.push(b.brush.char());
        b.apply(S::CycleBrush(Direction::Next));
    }
    assert_eq!(seen, "KQRBNPkqrbnp");
    assert_eq!(b.brush, piece('K'), "12 steps return to the start");
}

#[test]
fn rule4_prev_is_the_inverse_of_next() {
    let mut b = start();
    for start_char in "KQRBNPkqrbnp".chars() {
        b.apply(S::SelectBrush(piece(start_char)));
        b.apply(S::CycleBrush(Direction::Next));
        b.apply(S::CycleBrush(Direction::Prev));
        assert_eq!(b.brush, piece(start_char));
    }
    b.apply(S::SelectBrush(piece('K')));
    b.apply(S::CycleBrush(Direction::Prev));
    assert_eq!(b.brush, piece('p'), "wraps around");
}

#[test]
fn rule4_default_brush_is_the_white_pawn() {
    assert_eq!(start().brush, piece('P'));
}

// ------------------------------------------------------------------ rule 5

#[test]
fn rule5_drop_moves_and_replaces_the_target() {
    let mut b = start();
    b.apply(S::Drop {
        from: sq("g1"),
        to: Some(sq("f3")),
    });
    assert_eq!(b.piece_at(sq("g1")), None);
    assert_eq!(b.piece_at(sq("f3")), Some(piece('N')));
    assert_eq!(b.brush, piece('N'));
    // Onto an occupied square: the target is replaced.
    b.apply(S::Drop {
        from: sq("f3"),
        to: Some(sq("e2")),
    });
    assert_eq!(b.piece_at(sq("e2")), Some(piece('N')));
    assert_eq!(b.piece_at(sq("f3")), None);
}

#[test]
fn rule5_drop_onto_the_same_piece_still_replaces_never_erases() {
    // D109: the Scid would erase when the target holds the brush piece.
    let mut b = start();
    b.apply(S::Drop {
        from: sq("a2"),
        to: Some(sq("b2")),
    });
    assert_eq!(b.piece_at(sq("b2")), Some(piece('P')));
    assert_eq!(b.piece_at(sq("a2")), None);
}

#[test]
fn rule5_drop_outside_or_from_empty_does_nothing() {
    let mut b = start();
    let before = b.clone();
    b.apply(S::Drop {
        from: sq("a2"),
        to: None,
    });
    assert_eq!(b, before);
    b.apply(S::Drop {
        from: sq("e4"),
        to: Some(sq("e5")),
    });
    assert_eq!(b, before);
}

#[test]
fn rule5_pawn_dropped_on_back_rank_does_nothing() {
    let mut b = start();
    let before = b.clone();
    b.apply(S::Drop {
        from: sq("a2"),
        to: Some(sq("a8")),
    });
    assert_eq!(b.squares, before.squares);
    b.apply(S::Drop {
        from: sq("a7"),
        to: Some(sq("h1")),
    });
    assert_eq!(b.squares, before.squares);
}

// ------------------------------------------------------------------ rule 6

#[test]
fn rule6_clear_leaves_only_kings_on_g2_and_b7() {
    let mut b = start();
    b.apply(S::Clear);
    let pieces: Vec<_> = b
        .squares
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.map(|p| (i, p)))
        .collect();
    assert_eq!(pieces.len(), 2);
    assert_eq!(b.piece_at(sq("g2")), Some(piece('K')));
    assert_eq!(b.piece_at(sq("b7")), Some(piece('k')));
    assert_eq!(b.castling, CastlingFlags::default());
    assert_eq!(b.ep_file, None);
    assert!(
        b.analysis.is_valid(),
        "the cleared board is a legal position"
    );
}

#[test]
fn rule6_initial_gives_the_start_fen_and_counters() {
    let mut b = cleared();
    b.apply(S::SetFullmove(40));
    b.apply(S::SetHalfmove(7));
    b.apply(S::SetTurn(Color::Black));
    b.apply(S::Initial);
    assert_eq!(b.fen_text, START_FEN);
    assert_eq!(b.castling, CastlingFlags::ALL);
}

// ------------------------------------------------------------------ rule 7

#[test]
fn rule7_swap_colours_twice_is_identity_and_swaps_turn_and_castling() {
    let p = fen("r3k2r/pppq1ppp/2n5/8/8/5N2/PPP2PPP/R3K2B w Qkq - 3 9");
    let mut b = SetupBoard::from_position(&p);
    let original = b.clone();
    b.apply(S::SwapColours);
    assert_eq!(b.turn, Color::Black);
    assert_eq!(
        b.piece_at(sq("a1")),
        Some(piece('r')),
        "colours inverted in place"
    );
    assert_eq!(b.piece_at(sq("a8")), Some(piece('R')), "NOT mirrored");
    assert_eq!(b.piece_at(sq("f3")), Some(piece('n')));
    assert!(b.castling.get(Color::Black, CastlingSide::QueenSide));
    assert!(b.castling.get(Color::White, CastlingSide::KingSide));
    assert!(b.castling.get(Color::White, CastlingSide::QueenSide));
    assert!(!b.castling.get(Color::Black, CastlingSide::KingSide));
    b.apply(S::SwapColours);
    assert_eq!(b.squares, original.squares);
    assert_eq!(b.turn, original.turn);
    assert_eq!(b.castling, original.castling);
}

#[test]
fn rule7_mirrors_twice_restore_squares_but_clear_castling_and_ep() {
    let p = fen("rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 3");
    for msg in [S::MirrorRanks, S::MirrorFiles] {
        let mut b = SetupBoard::from_position(&p);
        let squares = b.squares;
        assert_eq!(b.ep_file, Some(File::D));
        b.apply(msg.clone());
        assert_ne!(b.squares, squares, "{msg:?} changes the board");
        assert_eq!(
            b.castling,
            CastlingFlags::default(),
            "{msg:?} clears castling"
        );
        assert_eq!(b.ep_file, None, "{msg:?} clears en passant");
        b.apply(msg);
        assert_eq!(b.squares, squares);
    }
}

#[test]
fn rule7_mirror_ranks_reverses_rank_order_and_mirror_files_reverses_files() {
    let mut b = start();
    b.apply(S::MirrorRanks);
    assert_eq!(b.piece_at(sq("e1")), Some(piece('k')));
    assert_eq!(b.piece_at(sq("d8")), Some(piece('Q')));
    let mut b = start();
    b.apply(S::MirrorFiles);
    assert_eq!(
        b.piece_at(sq("d1")),
        Some(piece('K')),
        "king and queen swap files"
    );
    assert_eq!(b.piece_at(sq("e1")), Some(piece('Q')));
}

#[test]
fn flip_is_visual_only() {
    let mut b = start();
    let squares = b.squares;
    let fen_before = b.fen_text.clone();
    b.apply(S::Flip);
    assert!(b.flipped);
    assert_eq!(b.squares, squares);
    assert_eq!(b.fen_text, fen_before);
}

// ------------------------------------------------------------------ rule 8

#[test]
fn rule8_missing_white_king_is_reported_before_the_black_king() {
    let mut b = cleared();
    b.apply(S::RightClickSquare(sq("g2")));
    b.apply(S::RightClickSquare(sq("b7")));
    assert_eq!(problem(&b), SetupProblem::MissingKing(Color::White));
    let mut b = cleared();
    b.apply(S::RightClickSquare(sq("b7")));
    assert_eq!(problem(&b), SetupProblem::MissingKing(Color::Black));
}

#[test]
fn rule8_two_black_kings() {
    let mut b = cleared();
    put(&mut b, 'k', "a5");
    assert_eq!(problem(&b), SetupProblem::TooManyKings(Color::Black));
    let mut b = cleared();
    put(&mut b, 'K', "a5");
    assert_eq!(problem(&b), SetupProblem::TooManyKings(Color::White));
}

#[test]
fn rule8_seventeen_white_pieces() {
    let mut b = start();
    put(&mut b, 'N', "e4");
    assert_eq!(problem(&b), SetupProblem::TooManyPieces(Color::White));
    // 16 is accepted.
    b.apply(S::ClickSquare(sq("e4")));
    assert!(b.analysis.is_valid());
}

#[test]
fn rule8_nine_black_pawns() {
    let mut b = start();
    // Remove a black rook and add a ninth pawn: still 16 pieces, 9 pawns.
    b.apply(S::RightClickSquare(sq("a8")));
    put(&mut b, 'p', "a6");
    assert_eq!(problem(&b), SetupProblem::TooManyPawns(Color::Black));
}

#[test]
fn rule8_adjacent_kings() {
    let mut b = cleared();
    b.apply(S::RightClickSquare(sq("b7")));
    put(&mut b, 'k', "g3");
    assert_eq!(problem(&b), SetupProblem::AdjacentKings);
}

#[test]
fn rule8_opponent_in_check_black_king_attacked_with_white_to_move() {
    let mut b = cleared();
    put(&mut b, 'R', "b1");
    assert_eq!(b.turn, Color::White);
    assert_eq!(problem(&b), SetupProblem::OpponentInCheck);
    b.apply(S::SetTurn(Color::Black));
    assert!(
        b.analysis.is_valid(),
        "side to move in check is accepted (as in the Scid)"
    );
}

#[test]
fn rule8_checkmate_and_stalemate_are_accepted() {
    // Fool's mate final position, white to move.
    let mate = fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3");
    assert!(SetupBoard::from_position(&mate).analysis.is_valid());
    let stalemate = fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
    assert!(SetupBoard::from_position(&stalemate).analysis.is_valid());
}

#[test]
fn rule8_pawn_on_back_rank_via_fen_text_is_reported() {
    let mut b = start();
    b.apply(S::FenEdited("P3k3/8/8/8/8/8/8/4K3 w - - 0 1".to_owned()));
    b.apply(S::ApplyFenText);
    // The text loader either refuses the FEN or flags the back-rank pawn;
    // in both cases the dialog cannot be confirmed.
    assert!(!b.analysis.is_valid(), "{:?}", b.analysis);
}

#[test]
fn problems_and_warnings_have_message_keys() {
    let mut b = cleared();
    b.apply(S::RightClickSquare(sq("g2")));
    let SetupAnalysis::Invalid(p) = &b.analysis else {
        panic!()
    };
    assert!(p.message_key().starts_with("setup-problem-"));
    for w in [
        SetupWarning::EnPassantDiscarded,
        SetupWarning::CastlingDiscarded(Color::White, CastlingSide::KingSide),
    ] {
        assert!(w.message_key().starts_with("setup-warning-"));
    }
}

// ------------------------------------------------------------------ rule 9

#[test]
fn rule9_castling_without_rook_is_discarded_with_a_warning() {
    let mut b = start();
    b.apply(S::RightClickSquare(sq("h1")));
    assert!(
        b.castling.get(Color::White, CastlingSide::KingSide),
        "the checkbox stays"
    );
    assert_eq!(
        warnings(&b),
        vec![SetupWarning::CastlingDiscarded(
            Color::White,
            CastlingSide::KingSide
        )]
    );
    let SetupAnalysis::Valid { fen, .. } = &b.analysis else {
        panic!()
    };
    assert_eq!(
        fen,
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBN1 w Qkq - 0 1"
    );
}

#[test]
fn rule9_castling_without_king_on_home_square_is_discarded() {
    let mut b = start();
    b.apply(S::Drop {
        from: sq("e1"),
        to: Some(sq("e3")),
    });
    let w = warnings(&b);
    assert!(w.contains(&SetupWarning::CastlingDiscarded(
        Color::White,
        CastlingSide::KingSide
    )));
    assert!(w.contains(&SetupWarning::CastlingDiscarded(
        Color::White,
        CastlingSide::QueenSide
    )));
}

#[test]
fn rule9_en_passant_without_a_pawn_is_discarded_with_a_warning() {
    let mut b = cleared();
    b.apply(S::SetEpFile(Some(File::E)));
    assert_eq!(warnings(&b), vec![SetupWarning::EnPassantDiscarded]);
    let SetupAnalysis::Valid { fen, .. } = &b.analysis else {
        panic!()
    };
    assert!(fen.contains(" - 0 1"), "{fen}");
}

#[test]
fn rule9_en_passant_rank_is_deduced_from_the_side_to_move() {
    // White to move: e6. Black to move: e3.
    let mut b = SetupBoard::from_position(&fen("4k3/8/8/4Pp2/8/8/8/4K3 w - f6 0 2"));
    b.apply(S::SetEpFile(Some(File::F)));
    let SetupAnalysis::Valid {
        fen: text,
        warnings,
        ..
    } = &b.analysis
    else {
        panic!()
    };
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(text.contains(" f6 "), "{text}");

    let mut b = SetupBoard::from_position(&fen("4k3/8/8/8/4pP2/8/8/4K3 b - f3 0 2"));
    b.apply(S::SetEpFile(Some(File::F)));
    let SetupAnalysis::Valid { fen: text, .. } = &b.analysis else {
        panic!()
    };
    assert!(text.contains(" f3 "), "{text}");
}

// ------------------------------------------------------------------ rule 10

#[test]
fn rule10_invalid_fen_text_keeps_the_draft_and_reports_a_fen_problem() {
    let mut b = start();
    let squares = b.squares;
    b.apply(S::FenEdited("this is not a fen".to_owned()));
    assert_eq!(b.squares, squares, "typing alone changes nothing");
    b.apply(S::ApplyFenText);
    assert_eq!(b.squares, squares, "the board of the draft is untouched");
    assert!(
        matches!(problem(&b), SetupProblem::Fen(_)),
        "{:?}",
        b.analysis
    );
}

#[test]
fn rule10_valid_fen_text_replaces_every_field_of_the_draft() {
    let mut b = start();
    b.apply(S::FenEdited(
        "  FEN: r3k2r/8/8/8/8/8/8/R3K2R b Kq - 12 30 ".to_owned(),
    ));
    b.apply(S::ApplyFenText);
    assert_eq!(b.turn, Color::Black);
    assert_eq!(b.halfmove_clock, 12);
    assert_eq!(b.fullmove_number, 30);
    assert!(b.castling.get(Color::White, CastlingSide::KingSide));
    assert!(!b.castling.get(Color::White, CastlingSide::QueenSide));
    assert!(b.castling.get(Color::Black, CastlingSide::QueenSide));
    assert_eq!(b.piece_at(sq("h1")), Some(piece('R')));
    assert_eq!(b.piece_at(sq("e2")), None);
    assert_eq!(b.fen_text, "r3k2r/8/8/8/8/8/8/R3K2R b Kq - 12 30");
}

#[test]
fn clear_fen_text_empties_the_field_only() {
    let mut b = start();
    b.apply(S::ClearFenText);
    assert!(b.fen_text.is_empty());
    assert!(b.analysis.is_valid(), "the board did not change");
}

// ------------------------------------------------------------------ rule 12

#[rstest]
#[case(0, 1)]
#[case(1, 1)]
#[case(5000, 999)]
#[case(999, 999)]
#[case(u16::MAX, 999)]
fn rule12_fullmove_is_clamped_to_1_to_999(#[case] input: u16, #[case] expected: u16) {
    let mut b = start();
    b.apply(S::SetFullmove(input));
    assert_eq!(b.fullmove_number, expected);
}

#[rstest]
#[case(0, 0)]
#[case(5000, 999)]
#[case(u16::MAX, 999)]
fn rule12_halfmove_is_clamped_to_0_to_999(#[case] input: u16, #[case] expected: u16) {
    let mut b = start();
    b.apply(S::SetHalfmove(input));
    assert_eq!(b.halfmove_clock, expected);
}

// ------------------------------------------------------------------ D112

#[test]
fn fen_history_is_session_state_without_duplicates_newest_first_max_20() {
    let mut h = Harness::new();
    for i in 1..=25u16 {
        h.send(Command::SetupBoard);
        h.send(SetupMessage::SetFullmove(i));
        h.send(SetupMessage::Ok);
        assert!(h.state.setup.is_none(), "Ok closes the dialog (clean game)");
    }
    assert_eq!(h.state.setup_history.len(), 20);
    assert!(h.state.setup_history[0].ends_with(" 25"));
    // Re-adding an existing entry moves it to the front, without duplicate.
    h.send(Command::SetupBoard);
    h.send(SetupMessage::SetFullmove(10));
    h.send(SetupMessage::Ok);
    assert_eq!(h.state.setup_history.len(), 20);
    assert!(h.state.setup_history[0].ends_with(" 10"));
    let count = h
        .state
        .setup_history
        .iter()
        .filter(|f| f.ends_with(" 10"))
        .count();
    assert_eq!(count, 1);
}

#[test]
fn paste_fen_in_the_dialog_is_a_host_effect_and_the_answer_loads_the_draft_only() {
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(Command::SetupBoard);
    let eff = h.send(SetupMessage::PasteFen);
    assert!(eff.iter().any(|e| matches!(e, Effect::ReadClipboard(_))));
    let game_before = h.state.session.game().clone();
    h.clipboard(
        mychess_app::ClipboardPurpose::SetupFen,
        "8/8/8/8/8/8/8/K6k w - - 0 1",
    );
    let board = h.state.setup.as_ref().unwrap();
    assert_eq!(board.fen_text, "8/8/8/8/8/8/8/K6k w - - 0 1");
    assert!(h.state.session.game().same_content(&game_before));
}

// ------------------------------------------------------------------ rule 11 + D105/D113

#[test]
fn rule11_setup_starts_from_the_position_at_the_cursor() {
    let mut h = Harness::new();
    h.play_all(&["e2e4", "e7e5"]);
    let expected = h.state.session.position().fen();
    h.send(Command::SetupBoard);
    assert_eq!(h.state.setup.as_ref().unwrap().fen_text, expected);
}

#[test]
fn rule11_cancel_is_neutral_for_game_cursor_dirty_and_origin() {
    let mut h = Harness::new();
    h.play_all(&["e2e4", "e7e5"]);
    let game = h.state.session.game().clone();
    let cursor = h.state.session.cursor();
    let dirty = h.state.is_dirty();
    h.send(Command::SetupBoard);
    h.send(SetupMessage::Clear);
    h.send(SetupMessage::Cancel);
    assert!(h.state.setup.is_none());
    assert!(h.state.session.game().same_content(&game));
    assert_eq!(h.state.session.cursor(), cursor);
    assert_eq!(h.state.is_dirty(), dirty);
    assert_eq!(h.state.origin, GameOrigin::New);
}

#[test]
fn d105_editing_the_draft_does_not_touch_the_game_until_ok() {
    let mut h = Harness::new();
    h.play("e2e4");
    let game = h.state.session.game().clone();
    h.send(Command::SetupBoard);
    h.send(SetupMessage::Clear);
    h.send(SetupMessage::ClickSquare(sq("a4")));
    assert!(h.state.session.game().same_content(&game));
}

#[test]
fn rule11_ok_on_a_clean_game_replaces_it_with_a_new_game_from_the_position() {
    let mut h = Harness::new();
    h.send(Command::SetupBoard);
    h.send(SetupMessage::Clear);
    h.send(SetupMessage::Ok);
    assert!(h.state.setup.is_none());
    let game = h.state.session.game();
    assert_eq!(game.start_position().fen(), "8/1k6/8/8/8/8/6K1/8 w - - 0 1");
    assert_eq!(h.state.session.cursor(), game.root());
    assert_eq!(h.state.origin, GameOrigin::New);
    assert!(!h.state.is_dirty());
    assert_eq!(*game.tags(), mychess_core::Tags::default());
    assert!(pgn_of(game).contains("[FEN \"8/1k6/8/8/8/8/6K1/8 w - - 0 1\"]"));
}

#[test]
fn rule11_ok_with_the_standard_position_gives_a_game_without_fen_tag() {
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(Command::SetupBoard);
    h.send(SetupMessage::Initial);
    // The game is dirty: a confirmation comes first.
    h.send(SetupMessage::Ok);
    assert!(h.state.discard_prompt.is_some());
    h.send(Message::AnswerDiscard(mychess_app::DiscardChoice::DontSave));
    let pgn = pgn_of(h.state.session.game());
    assert!(!pgn.contains("[FEN"), "{pgn}");
    assert!(!pgn.contains("[SetUp"), "{pgn}");
    assert_eq!(h.plies(), 0);
}

#[test]
fn rule11_ok_with_invalid_analysis_does_nothing() {
    let mut h = Harness::new();
    h.play("e2e4");
    let game = h.state.session.game().clone();
    h.send(Command::SetupBoard);
    h.send(SetupMessage::RightClickSquare(sq("e1")));
    h.send(SetupMessage::Ok);
    assert!(h.state.setup.is_some(), "the dialog stays open");
    assert!(h.state.discard_prompt.is_none());
    assert!(h.state.session.game().same_content(&game));
}

#[test]
fn d103_ok_on_a_dirty_game_asks_first_and_cancel_returns_to_the_dialog() {
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(Command::SetupBoard);
    h.send(SetupMessage::Clear);
    h.send(SetupMessage::Ok);
    assert!(h.state.discard_prompt.is_some());
    assert_eq!(h.plies(), 1, "nothing replaced yet");
    h.send(Message::AnswerDiscard(mychess_app::DiscardChoice::Cancel));
    assert!(h.state.discard_prompt.is_none());
    assert!(h.state.setup.is_some(), "Cancel closes only the question");
    assert_eq!(h.plies(), 1);
    h.send(SetupMessage::Ok);
    h.send(Message::AnswerDiscard(mychess_app::DiscardChoice::DontSave));
    assert_eq!(h.plies(), 0);
    assert!(h.state.setup.is_none());
}

#[test]
fn setup_messages_without_an_open_dialog_are_ignored() {
    let mut h = Harness::new();
    let eff = h.raw(SetupMessage::Ok);
    assert!(eff.is_empty());
    h.raw(SetupMessage::ClickSquare(sq("e4")));
    assert!(h.state.setup.is_none());
}

#[test]
fn chess960_game_opens_the_dialog_in_chess960_mode() {
    let mut h = Harness::new();
    let pos = Position::chess960_start(518 + 1).unwrap();
    assert_ne!(pos.castling_mode(), mychess_core::CastlingMode::Standard);
    h.state.session.replace_game(Game::from_position(pos));
    h.send(Command::SetupBoard);
    let setup = h.state.setup.as_ref().expect("the editor opens (M9-6)");
    assert!(setup.is_chess960());
    assert_eq!(setup.chess960_number(), Some(519));
    assert!(setup.analysis.is_valid());
    assert!(h.state.notice.is_none());
}

#[test]
fn m9_6_the_chess960_switch_changes_how_castling_is_read() {
    let mut b = start();
    assert!(!b.is_chess960());
    assert_eq!(b.chess960_number(), None, "only shown in Chess960 mode");
    b.apply(S::SetChess960(true));
    assert!(b.is_chess960());
    assert_eq!(b.chess960_number(), Some(518));
    // The initial position keeps the mode.
    b.apply(S::Clear);
    assert_eq!(b.chess960_number(), None);
    b.apply(S::Initial);
    assert!(b.is_chess960());
    assert_eq!(b.chess960_number(), Some(518));
    b.apply(S::SetChess960(false));
    assert!(!b.is_chess960());
}

#[test]
fn m9_6_chess960_rights_need_the_king_between_the_rooks() {
    // King on b1 with a rook on a1 only: O-O-O needs a rook on the queen
    // side; the right asked for the king side has no rook and is dropped
    // with a warning, in Chess960 mode too.
    let mut b = start();
    b.apply(S::SetChess960(true));
    b.apply(S::FenEdited("4k3/8/8/8/8/8/8/RK6 w KQ - 0 1".to_owned()));
    b.apply(S::ApplyFenText);
    assert!(b.is_chess960(), "the box stays on when a text is applied");
    assert!(b.castling.white_queen && !b.castling.white_king);
    // Asking for the king-side right without a rook there warns.
    b.apply(S::ToggleCastling(Color::White, CastlingSide::KingSide));
    let SetupAnalysis::Valid { warnings, .. } = &b.analysis else {
        panic!("valid: {:?}", b.analysis);
    };
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            SetupWarning::CastlingDiscarded(Color::White, CastlingSide::KingSide)
        )),
        "{warnings:?}"
    );
}

#[test]
fn m9_6_accepting_a_chess960_draft_starts_a_chess960_game() {
    let mut h = Harness::new();
    h.send(Command::SetupBoard);
    h.send(S::SetChess960(true));
    h.send(S::Ok);
    // The standard start in Chess960 mode is position 518 and keeps the
    // variant (D4502).
    assert!(h.state.setup.is_none());
    let game = h.state.session.game();
    assert_eq!(
        game.start_position().castling_mode(),
        mychess_core::CastlingMode::Chess960
    );
    assert_eq!(game.start_position().chess960_number(), Some(518));
}

#[test]
fn m9_6_an_altered_chess960_game_can_go_through_the_editor_to_a_new_game() {
    // The "current position" of the play dialog passes through the editor
    // (Setup, then OK): it opens on a Chess960 game, after a move, and gives
    // a Chess960 game back with the same position.
    let mut h = Harness::new();
    let pos = Position::chess960_start(100).unwrap();
    h.state.session.replace_game(Game::from_position(pos));
    let mv = h.state.session.position().parse_uci("e2e4").expect("legal");
    h.send(GameMessage::PlayMove {
        mv,
        options: mychess_app::PlayOptions::default(),
    });
    let before = h.state.session.position().fen();
    h.send(Command::SetupBoard);
    assert!(h.state.setup.as_ref().is_some_and(SetupBoard::is_chess960));
    h.send(S::Ok);
    // The game was altered: the question comes first, then the replacement.
    if h.state.discard_prompt.is_some() {
        h.send(Message::AnswerDiscard(mychess_app::DiscardChoice::DontSave));
    }
    let game = h.state.session.game();
    assert_eq!(game.variant(), mychess_core::Variant::Chess960);
    assert_eq!(game.start_position().fen(), before);
}

#[test]
fn d108_fen_mode_is_standard_for_a_standard_game() {
    assert_eq!(start().mode, FenMode::Standard);
}

proptest! {
    // Rule 66: arbitrary edits never panic and `analysis` is always coherent
    // with the fields.
    #[test]
    fn arbitrary_edit_sequences_never_panic(
        ops in proptest::collection::vec((0u8..14, 0u8..64, 0u8..12, any::<u16>()), 0..60)
    ) {
        let mut b = start();
        for (op, s, pc, n) in ops {
            let square = Square::new(u32::from(s));
            let pieces = "KQRBNPkqrbnp";
            let p = piece(pieces.chars().nth(usize::from(pc)).unwrap());
            let msg = match op {
                0 => S::ClickSquare(square),
                1 => S::RightClickSquare(square),
                2 => S::MiddleClickSquare(square),
                3 => S::Drop { from: square, to: Some(Square::new(u32::from(n % 64))) },
                4 => S::Drop { from: square, to: None },
                5 => S::SelectBrush(p),
                6 => S::CycleBrush(Direction::Next),
                7 => S::SwapColours,
                8 => S::MirrorRanks,
                9 => S::MirrorFiles,
                10 => S::SetFullmove(n),
                11 => S::SetHalfmove(n),
                12 => S::SetTurn(if n % 2 == 0 { Color::White } else { Color::Black }),
                _ => S::ToggleCastling(Color::White, CastlingSide::KingSide),
            };
            b.apply(msg);
            prop_assert!(b.fullmove_number >= 1 && b.fullmove_number <= 999);
            prop_assert!(b.halfmove_clock <= 999);
            prop_assert!(b.fen_history.len() <= 20);
            // No pawn ever sits on a back rank through the gestures.
            for (i, slot) in b.squares.iter().enumerate() {
                if slot.is_some_and(|p| p.role == Role::Pawn) {
                    prop_assert!((8..56).contains(&i), "pawn on a back rank at {i}");
                }
            }
            if let SetupAnalysis::Valid { position, fen, .. } = &b.analysis {
                prop_assert_eq!(&position.fen(), fen);
                prop_assert_eq!(&b.fen_text, fen);
            }
        }
    }

    // Rule 66: hostile text in the FEN field never panics.
    #[test]
    fn arbitrary_fen_text_never_panics(text in ".{0,300}") {
        let mut b = start();
        b.apply(S::FenEdited(text));
        b.apply(S::ApplyFenText);
        prop_assert!(b.fullmove_number >= 1);
    }
}
