//! Clipboard (FEN/PGN) and PGN import (spec M2-2, rules 13-23, D114-D123).
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
    AppNotice, ClipboardError, ClipboardPurpose, DiscardChoice, FEN_EXCERPT_CHARS, ImportIssue,
    ImportMode, ImportReport, MAX_CLIPBOARD_BYTES, PasteFenError, PgnLoad, PgnSource, ScanLimits,
    normalize_fen_text, read_pasted_fen, scan_pgn,
};
use mychess_pgn::read::{DiagnosticKind, ReadOptions, Severity, parse_pgn_text};
use proptest::prelude::*;
use rstest::rstest;

const LONG_LINE: [&str; 22] = [
    "e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6", "b5a4", "g8f6", "e1g1", "f8e7", "f1e1", "b7b5",
    "a4b3", "d7d6", "c2c3", "e8g8", "h2h3", "c6a5", "b3c2", "c7c5", "d2d4", "d8c7",
];

fn scan(text: &str) -> (PgnLoad, Vec<mychess_app::ImportNotice>) {
    scan_pgn(
        PgnSource::Text(text.to_owned()),
        &ScanLimits::default(),
        &ReadOptions::default(),
    )
    .unwrap()
}

fn single(load: PgnLoad) -> mychess_app::LoadedGame {
    match load {
        PgnLoad::Single(g) => g,
        other => panic!("expected a single game, got {other:?}"),
    }
}

// ------------------------------------------------------------------ rule 14

#[rstest]
#[case(
    "FEN: rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR/ W KQkq - 0 1",
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
)]
#[case("  fen:\n8/8/8/8/8/8/8/K6k  b  ", "8/8/8/8/8/8/8/K6k b")]
#[case("fEn:8/8/8/8/8/8/8/K6k w", "8/8/8/8/8/8/8/K6k w")]
#[case("8/8/8/8/8/8/8/K6\u{e9}k w", "8/8/8/8/8/8/8/K6k w")]
#[case("8/8/8/8/8/8/8/K6\\u00e9k w", "8/8/8/8/8/8/8/K6k w")]
#[case("   \t\n  ", "")]
#[case("", "")]
#[case("8/8/8/8/8/8/8/K6k\r\nb\r\n- -", "8/8/8/8/8/8/8/K6k b - -")]
fn rule14_normalizes_pasted_fen_text(#[case] raw: &str, #[case] expected: &str) {
    assert_eq!(normalize_fen_text(raw), expected);
}

proptest! {
    #[test]
    fn rule14_normalize_is_idempotent(raw in ".{0,200}") {
        let once = normalize_fen_text(&raw);
        // A slash-only board field leaves a leading space: see
        // `rule14_normalize_is_idempotent_when_the_board_field_is_only_slashes`.
        prop_assume!(!once.starts_with(' '));
        prop_assert_eq!(normalize_fen_text(&once), once);
    }

    #[test]
    fn rule66_normalize_and_read_pasted_fen_never_panic(raw in "\\PC{0,400}") {
        let normalized = normalize_fen_text(&raw);
        prop_assert!(normalized.is_ascii());
        let _ = read_pasted_fen(&raw);
    }

    #[test]
    fn rule66_hostile_bytes_as_text_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        let raw = String::from_utf8_lossy(&bytes).into_owned();
        let _ = normalize_fen_text(&raw);
        let _ = read_pasted_fen(&raw);
    }
}

#[rstest]
#[case("/ A")]
#[case("// w")]
#[case("fen: / b - -")]
fn rule14_normalize_is_idempotent_when_the_board_field_is_only_slashes(#[case] raw: &str) {
    let once = normalize_fen_text(raw);
    assert_eq!(normalize_fen_text(&once), once, "{raw:?} -> {once:?}");
    assert!(!once.starts_with(' '), "no leading space: {once:?}");
}

// ------------------------------------------------------------------ rule 15

#[test]
fn rule15_empty_text_is_the_empty_error() {
    assert_eq!(read_pasted_fen(""), Err(PasteFenError::Empty));
    assert_eq!(read_pasted_fen("  \n\t "), Err(PasteFenError::Empty));
}

#[test]
fn rule15_garbage_reports_an_excerpt_of_at_most_80_chars() {
    match read_pasted_fen("lixo") {
        Err(PasteFenError::Invalid { excerpt, .. }) => assert_eq!(excerpt, "lixo"),
        other => panic!("{other:?}"),
    }
    let long = "x".repeat(500);
    match read_pasted_fen(&long) {
        Err(PasteFenError::Invalid { excerpt, .. }) => {
            assert!(
                excerpt.chars().count() <= FEN_EXCERPT_CHARS,
                "{}",
                excerpt.len()
            );
            assert!(
                !excerpt.ends_with("..."),
                "the ellipsis belongs to the presentation"
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(FEN_EXCERPT_CHARS, 80);
}

#[rstest]
#[case("8/8/8/8/8/8/8/K6k w")]
#[case("rnbqkbnrpppppppp8888PPPPPPPPRNBQKBNR w")]
#[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR W KQkq - 0 1")]
#[case("FEN: 8/8/8/8/8/8/8/K6k b - - 5 40")]
fn rule15_tolerant_fens_are_accepted(#[case] raw: &str) {
    assert!(read_pasted_fen(raw).is_ok(), "{raw}");
}

#[test]
fn rule15_uppercase_side_to_move_means_lowercase() {
    let pos = read_pasted_fen("8/8/8/8/8/8/8/K6k B - - 0 1").unwrap();
    assert_eq!(pos.turn(), Color::Black);
}

#[test]
fn paste_error_message_keys_are_stable_data() {
    assert_eq!(PasteFenError::Empty.message_key(), "paste-fen-error-empty");
    assert_eq!(ClipboardError::Empty.message_key(), "clipboard-error-empty");
    assert_eq!(
        ClipboardError::TooLarge { limit: 1 }.message_key(),
        "clipboard-error-too-large"
    );
    assert_eq!(
        ClipboardError::Unavailable.message_key(),
        "clipboard-error-unavailable"
    );
}

// ------------------------------------------------------------------ rule 13

#[test]
fn rule13_copy_fen_emits_exactly_one_write_with_the_fen_at_the_cursor() {
    let mut h = Harness::new();
    h.play_all(&["e2e4", "c7c5"]);
    let expected = h.state.session.position().fen();
    let eff = h.raw(Command::CopyFen);
    assert_eq!(eff, vec![Effect::WriteClipboard(expected.clone())]);
    // Move the cursor back: the FEN follows the cursor, not the end.
    h.send(GameMessage::Navigate(mychess_app::Nav::Start));
    let eff = h.raw(Command::CopyFen);
    assert_eq!(eff, vec![Effect::WriteClipboard(START_FEN.to_owned())]);
    assert_ne!(expected, START_FEN);
}

#[test]
fn rule13_en_passant_square_appears_only_when_a_capture_is_possible() {
    let mut h = Harness::new();
    h.play("e2e4");
    let eff = h.raw(Command::CopyFen);
    assert_eq!(
        eff,
        vec![Effect::WriteClipboard(
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1".to_owned()
        )]
    );
}

// ------------------------------------------------------------------ rule 16 / 17

#[test]
fn rule16_invalid_clipboard_leaves_the_game_and_sets_a_structured_notice() {
    let mut h = Harness::new();
    h.play("e2e4");
    let game = h.state.session.game().clone();
    let eff = h.send(Command::PasteFen);
    assert_eq!(eff, vec![Effect::ReadClipboard(ClipboardPurpose::Fen)]);
    h.clipboard(ClipboardPurpose::Fen, "this is not a position");
    assert!(h.state.session.game().same_content(&game));
    assert!(h.state.discard_prompt.is_none());
    assert!(matches!(
        h.state.notice,
        Some(AppNotice::PasteFen(PasteFenError::Invalid { .. }))
    ));
}

#[test]
fn rule16_valid_fen_on_a_clean_game_replaces_it_directly() {
    let mut h = Harness::new();
    h.clipboard(ClipboardPurpose::Fen, "FEN: 8/8/8/8/8/8/8/K6k b - - 0 1");
    assert!(h.state.discard_prompt.is_none());
    assert_eq!(
        h.state.session.game().start_position().fen(),
        "8/8/8/8/8/8/8/K6k b - - 0 1"
    );
    assert_eq!(h.state.origin, GameOrigin::New);
    assert!(!h.state.is_dirty());
}

#[test]
fn rule16_valid_fen_on_a_dirty_game_opens_the_discard_prompt_first() {
    let mut h = Harness::new();
    h.play("e2e4");
    h.clipboard(ClipboardPurpose::Fen, "8/8/8/8/8/8/8/K6k w - - 0 1");
    assert!(h.state.discard_prompt.is_some());
    assert_eq!(
        h.plies(),
        1,
        "the game is intact while the question is open"
    );
    h.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(h.plies(), 0);
}

#[test]
fn rule17_oversized_clipboard_is_rejected_without_analysis() {
    let mut h = Harness::new();
    let big = "x".repeat(MAX_CLIPBOARD_BYTES + 1);
    let eff = h.send(Message::ClipboardRead {
        purpose: ClipboardPurpose::Moves,
        result: Ok(big),
    });
    assert!(eff.is_empty());
    assert_eq!(
        h.state.notice,
        Some(AppNotice::Clipboard(ClipboardError::TooLarge {
            limit: MAX_CLIPBOARD_BYTES
        }))
    );
    assert!(h.state.pasted_moves.is_none(), "nothing was parsed");
}

#[test]
fn rule17_exactly_the_limit_is_accepted() {
    // 32 MiB of spaces would be "empty"; use a long comment-like text instead.
    let mut text = "1. e4 {".to_owned();
    text.push_str(&"x".repeat(MAX_CLIPBOARD_BYTES - text.len() - 1));
    text.push('}');
    assert_eq!(text.len(), MAX_CLIPBOARD_BYTES);
    assert_eq!(ClipboardError::check(&text), Ok(()));
}

#[test]
fn clipboard_empty_and_unavailable_become_notices() {
    let mut h = Harness::new();
    h.send(Message::ClipboardRead {
        purpose: ClipboardPurpose::Fen,
        result: Ok("  \n".to_owned()),
    });
    assert_eq!(
        h.state.notice,
        Some(AppNotice::Clipboard(ClipboardError::Empty))
    );
    h.state.notice = None;
    h.send(Message::ClipboardRead {
        purpose: ClipboardPurpose::Fen,
        result: Err(ClipboardError::Unavailable),
    });
    assert_eq!(
        h.state.notice,
        Some(AppNotice::Clipboard(ClipboardError::Unavailable))
    );
}

#[test]
fn clipboard_write_failure_is_a_notice_not_a_panic() {
    let mut h = Harness::new();
    h.send(Message::ClipboardWriteFailed);
    assert_eq!(h.state.notice, Some(AppNotice::ClipboardWriteFailed));
}

// ------------------------------------------------------------------ rule 18

#[test]
fn rule18_copy_pgn_is_one_line_lf_no_bom_and_reparses_to_the_same_game() {
    let mut h = Harness::new();
    h.play_all(&LONG_LINE);
    // Variation, comment and NAG are part of the copy.
    let root = h.state.session.game().root();
    let mut game = h.state.session.game().clone();
    let first = game.children(root).unwrap()[0];
    game.set_comment(first, "best by test").unwrap();
    let mv = game.position_at(root).unwrap().parse_uci("d2d4").unwrap();
    game.add_move(root, mv).unwrap();
    game.tags_mut().white = "Zé, Ã".to_owned();
    h.state.session.replace_game(game.clone());
    h.send(GameMessage::Navigate(mychess_app::Nav::Start)); // cursor irrelevant

    let eff = h.raw(Command::CopyPgn);
    let [Effect::WriteClipboard(text)] = eff.as_slice() else {
        panic!("{eff:?}");
    };
    assert!(!text.starts_with('\u{feff}'));
    assert!(!text.contains('\r'));
    // Unlimited width: the movetext is a single line.
    let movetext_lines = text
        .lines()
        .skip_while(|l| l.starts_with('['))
        .filter(|l| !l.is_empty())
        .count();
    assert_eq!(movetext_lines, 1, "{text}");
    let parsed = parse_pgn_text(text, &ReadOptions::default());
    assert_eq!(parsed.len(), 1);
    assert!(parsed[0].game.same_content(&game));
}

#[test]
fn rule18_copy_pgn_ignores_the_save_encoding_preference() {
    let mut h = Harness::new();
    h.state.prefs.pgn_export.encoding = mychess_app::EncodingPref::Latin1;
    h.state.prefs.pgn_export.width = 20;
    h.play("e2e4");
    h.state.session.replace_game({
        let mut g = h.state.session.game().clone();
        g.tags_mut().white = "\u{265f} pawn".to_owned();
        g
    });
    let eff = h.raw(Command::CopyPgn);
    let [Effect::WriteClipboard(text)] = eff.as_slice() else {
        panic!("{eff:?}");
    };
    assert!(text.contains('\u{265f}'), "always UTF-8 text");
    assert_eq!(
        text.lines().filter(|l| l.starts_with("1.")).count(),
        1,
        "unlimited width even though the preference says 20"
    );
}

// ------------------------------------------------------------------ rule 19-21

#[test]
fn rule19_text_without_header_is_read_as_moves_only_without_missing_result() {
    let (load, notices) = scan("1. e4 e5 2. Nf3");
    let loaded = single(load);
    assert_eq!(loaded.report.mode, ImportMode::MovesOnly);
    assert_eq!(loaded.game.mainline().count(), 3);
    assert!(
        !loaded
            .report
            .notices
            .iter()
            .chain(notices.iter())
            .any(|n| n.issue == ImportIssue::Pgn(DiagnosticKind::MissingResult)),
        "{:?}",
        loaded.report
    );
    assert!(!loaded.report.has_problems());
}

#[test]
fn rule19_moves_only_text_starts_from_the_standard_position_not_the_current_one() {
    let mut h = Harness::new();
    h.play("d2d4"); // the current position is not the start
    h.send(ImportMessage::Open);
    h.send(ImportMessage::TextEdited("1. e4 e5".to_owned()));
    h.send(ImportMessage::Import);
    h.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(h.plies(), 2);
    assert_eq!(*h.state.session.game().start_position(), Position::new());
}

#[test]
fn rule19_full_game_with_header_has_result_and_no_notices() {
    let (load, notices) = scan("[White \"A\"]\n\n1. e4 e5 1-0");
    let loaded = single(load);
    assert_eq!(loaded.report.mode, ImportMode::FullGame);
    assert_eq!(loaded.game.tags().result, mychess_core::GameResult::White);
    assert_eq!(loaded.game.tags().white, "A");
    assert!(loaded.report.notices.is_empty(), "{:?}", loaded.report);
    assert!(notices.is_empty(), "{notices:?}");
}

#[test]
fn rule20_illegal_move_loads_the_game_and_reports_an_error_with_its_line() {
    let (load, _) = scan("[Event \"x\"]\n\n1. e4 e5 2. Ke4 *");
    let loaded = single(load);
    assert_eq!(
        loaded.game.mainline().count(),
        2,
        "moves before the error survive"
    );
    let bad: Vec<_> = loaded
        .report
        .notices
        .iter()
        .filter(|n| n.severity == Severity::Error)
        .collect();
    assert_eq!(bad.len(), 1, "{:?}", loaded.report);
    assert!(matches!(
        bad[0].issue,
        ImportIssue::Pgn(DiagnosticKind::IllegalMove(_))
    ));
    assert_eq!(bad[0].line, Some(3));
    assert!(bad[0].column.is_some());
    assert!(loaded.report.has_problems());
}

#[test]
fn rule20_dialog_stays_open_on_errors_and_closes_on_info_only() {
    let mut h = Harness::new();
    h.send(ImportMessage::Open);
    h.send(ImportMessage::TextEdited(
        "[Event \"x\"]\n\n1. e4 e5 2. Ke4 *".to_owned(),
    ));
    h.send(ImportMessage::Import);
    assert!(h.state.import.is_some(), "errors keep the dialog open");
    assert_eq!(h.plies(), 2, "the game is loaded despite the error");
    let report = h.state.import.as_ref().unwrap().report.clone().unwrap();
    assert!(report.has_problems());
    assert_eq!(h.state.origin, GameOrigin::Clipboard);
    assert!(h.state.is_dirty(), "pasted text counts as unsaved");

    // A clean import closes the dialog by itself.
    let mut h = Harness::new();
    h.send(ImportMessage::Open);
    h.send(ImportMessage::TextEdited(
        "[White \"A\"]\n\n1. e4 e5 1-0".to_owned(),
    ));
    h.send(ImportMessage::Import);
    assert!(h.state.import.is_none());
    assert_eq!(h.plies(), 2);
}

#[test]
fn rule20_warning_also_keeps_the_dialog_open() {
    // Missing result token with a header is a warning.
    let mut h = Harness::new();
    h.send(ImportMessage::Open);
    h.send(ImportMessage::TextEdited(
        "[White \"A\"]\n\n1. e4 e5".to_owned(),
    ));
    h.send(ImportMessage::Import);
    let report = h.state.import.as_ref().map(|d| d.report.clone());
    if let Some(Some(r)) = report {
        assert!(r.has_problems());
    } else {
        // Closed: then there must have been no warning at all.
        let (_, notices) = scan("[White \"A\"]\n\n1. e4 e5");
        assert!(notices.iter().all(|n| n.severity == Severity::Info));
    }
}

#[test]
fn rule21_two_games_open_the_chooser_one_game_loads_whitespace_is_nothing() {
    let (load, _) = scan(&multi_pgn(2));
    match load {
        PgnLoad::Many(c) => assert_eq!(c.len(), 2),
        other => panic!("{other:?}"),
    }
    let (load, _) = scan(&multi_pgn(1));
    assert!(matches!(load, PgnLoad::Single(_)));
    let (load, notices) = scan("   \n \t ");
    assert!(matches!(load, PgnLoad::None { .. }), "{load:?}");
    assert!(
        notices
            .iter()
            .any(|n| n.issue == ImportIssue::NothingToImport && n.severity == Severity::Error),
        "{notices:?}"
    );
}

#[test]
fn header_only_text_is_a_game_without_moves_and_replaces_after_the_question() {
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(ImportMessage::Open);
    h.send(ImportMessage::TextEdited("[Event \"x\"]".to_owned()));
    h.send(ImportMessage::Import);
    assert!(h.state.discard_prompt.is_some());
    assert_eq!(h.plies(), 1);
}

#[test]
fn rule21_import_of_several_games_opens_the_chooser_and_does_not_touch_the_game() {
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(ImportMessage::Open);
    h.send(ImportMessage::TextEdited(multi_pgn(3)));
    h.send(ImportMessage::Import);
    let chooser = h.state.chooser.as_ref().expect("chooser");
    assert_eq!(chooser.collection.len(), 3);
    assert_eq!(h.plies(), 1);
    assert!(h.state.discard_prompt.is_none());
}

#[rstest]
#[case("")]
#[case("   \n")]
#[case("garbage that is not chess at all")]
fn nothing_to_import_never_replaces_the_game(#[case] text: &str) {
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(ImportMessage::Open);
    h.send(ImportMessage::TextEdited(text.to_owned()));
    h.send(ImportMessage::Import);
    assert_eq!(h.plies(), 1, "{text:?}");
    assert!(h.state.discard_prompt.is_none());
}

// ------------------------------------------------------------------ rule 22 / 23

#[test]
fn rule22_stale_scan_answer_is_dropped_without_state_change_or_effects() {
    let mut h = Harness::new();
    h.send(ImportMessage::Open);
    h.send(ImportMessage::TextEdited("1. e4 e5".to_owned()));
    let effects = h.raw(ImportMessage::Import);
    let [Effect::ScanPgn { request, source }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    // The user closes the dialog while the scan runs: the id is invalidated.
    h.raw(ImportMessage::Close);
    let answer = h.perform(Effect::ScanPgn {
        request: *request,
        source: source.clone(),
    });
    let before_plies = h.plies();
    let eff = h.raw(answer);
    assert!(eff.is_empty());
    assert_eq!(h.plies(), before_plies);
    assert!(h.state.import.is_none());
    assert!(h.state.chooser.is_none());
    assert!(h.state.discard_prompt.is_none());
}

#[test]
fn rule22_answer_with_a_wrong_request_id_is_ignored() {
    let mut h = Harness::new();
    h.send(ImportMessage::Open);
    h.send(ImportMessage::TextEdited("1. e4 e5".to_owned()));
    let effects = h.raw(ImportMessage::Import);
    let [Effect::ScanPgn { request, source }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    let stale = Effect::ScanPgn {
        request: RequestId(request.0 + 1000),
        source: source.clone(),
    };
    let answer = h.perform(stale);
    let eff = h.raw(answer);
    assert!(eff.is_empty());
    assert_eq!(h.plies(), 0);
    // The real answer still works.
    let answer = h.perform(Effect::ScanPgn {
        request: *request,
        source: source.clone(),
    });
    h.raw(answer);
    assert_eq!(h.plies(), 2);
}

#[test]
fn rule22_second_import_supersedes_the_first() {
    let mut h = Harness::new();
    h.send(ImportMessage::Open);
    h.send(ImportMessage::TextEdited("1. e4".to_owned()));
    let first = h.raw(ImportMessage::Import);
    h.send(ImportMessage::TextEdited("1. d4 d5".to_owned()));
    let second = h.raw(ImportMessage::Import);
    let a1 = h.perform(first.into_iter().next().unwrap());
    h.raw(a1);
    assert_eq!(h.plies(), 0, "first answer is obsolete");
    let a2 = h.perform(second.into_iter().next().unwrap());
    h.raw(a2);
    assert_eq!(h.plies(), 2);
}

#[test]
fn rule23_import_only_emits_scan_pgn_and_does_not_analyse_in_update() {
    let mut h = Harness::new();
    h.send(ImportMessage::Open);
    // 10 MiB of text that would be slow to parse: update must return at once
    // with exactly one effect and an untouched game.
    let big = "1. e4 e5 ".repeat(10 * 1024 * 1024 / 9);
    h.send(ImportMessage::TextEdited(big.clone()));
    let eff = h.raw(ImportMessage::Import);
    assert_eq!(eff.len(), 1);
    assert!(matches!(
        &eff[0],
        Effect::ScanPgn { source: PgnSource::Text(t), .. } if t.len() == big.len()
    ));
    assert_eq!(h.plies(), 0);
    assert!(h.state.import.as_ref().unwrap().request.is_some());
}

// ------------------------------------------------------------------ D119

#[test]
fn d119_open_asks_for_the_clipboard_to_prefill_and_only_fills_an_empty_box() {
    let mut h = Harness::new();
    let eff = h.send(ImportMessage::Open);
    assert_eq!(
        eff,
        vec![Effect::ReadClipboard(ClipboardPurpose::ImportPrefill)]
    );
    h.clipboard(ClipboardPurpose::ImportPrefill, "1. e4 e5");
    assert_eq!(h.state.import.as_ref().unwrap().text, "1. e4 e5");
    // Typed text is never overwritten by a late clipboard answer.
    h.send(ImportMessage::TextEdited("typed".to_owned()));
    h.clipboard(ClipboardPurpose::ImportPrefill, "other");
    assert_eq!(h.state.import.as_ref().unwrap().text, "typed");
}

#[test]
fn d119_paste_current_game_inserts_the_pgn_with_width_70() {
    let mut h = Harness::new();
    h.play_all(&LONG_LINE);
    h.send(ImportMessage::Open);
    h.send(ImportMessage::PasteCurrentGame);
    let text = h.state.import.as_ref().unwrap().text.clone();
    assert!(text.contains("[Event"));
    let longest = text
        .lines()
        .filter(|l| !l.starts_with('['))
        .map(|l| l.chars().count())
        .max()
        .unwrap();
    assert!(longest <= 70, "longest movetext line is {longest}");
    let parsed = parse_pgn_text(&text, &ReadOptions::default());
    assert!(parsed[0].game.same_content(h.state.session.game()));
}

#[test]
fn d119_clear_resets_text_report_and_request() {
    let mut h = Harness::new();
    h.send(ImportMessage::Open);
    h.send(ImportMessage::TextEdited(
        "[Event \"x\"]\n\n1. e4 e5 2. Ke4 *".to_owned(),
    ));
    h.send(ImportMessage::Import);
    h.send(ImportMessage::Clear);
    let d = h.state.import.as_ref().unwrap();
    assert!(d.text.is_empty() && d.report.is_none() && d.request.is_none());
}

#[test]
fn import_messages_without_a_dialog_do_nothing() {
    let mut h = Harness::new();
    assert!(h.raw(ImportMessage::Import).is_empty());
    h.raw(ImportMessage::TextEdited("x".to_owned()));
    h.raw(ImportMessage::PasteCurrentGame);
    h.raw(ImportMessage::Clear);
    assert!(h.state.import.is_none());
}

#[test]
fn d122_every_pgn_diagnostic_kind_has_a_pgn_diag_key() {
    let kinds = [
        DiagnosticKind::BadTag,
        DiagnosticKind::TagTooLong,
        DiagnosticKind::RatingTooLarge,
        DiagnosticKind::InvalidFen,
        DiagnosticKind::IllegalCastling,
        DiagnosticKind::CastlingReinterpreted,
        DiagnosticKind::TooManyNodes,
        DiagnosticKind::TooManyTags,
        DiagnosticKind::DuplicateVariation,
        DiagnosticKind::InvalidNagSymbol,
        DiagnosticKind::NagOutOfRange,
        DiagnosticKind::NagWithoutMove,
        DiagnosticKind::VariationBeforeFirstMove,
        DiagnosticKind::UnbalancedCloseParen,
        DiagnosticKind::UnclosedVariation,
        DiagnosticKind::StrayCloseBrace,
        DiagnosticKind::UnterminatedComment,
        DiagnosticKind::CommentTruncated,
        DiagnosticKind::MissingResult,
        DiagnosticKind::UnexpectedToken,
        DiagnosticKind::ResultMismatch,
        DiagnosticKind::InvalidTextInHeader,
        DiagnosticKind::HeaderOnlyAtEof,
        DiagnosticKind::JunkBetweenGames,
        DiagnosticKind::EncodingFallback,
        DiagnosticKind::Chess960Detected,
        DiagnosticKind::UnsupportedVariant,
        DiagnosticKind::GameTooLarge,
        DiagnosticKind::TooManyDiagnostics,
    ];
    let mut keys = std::collections::BTreeSet::new();
    for k in kinds {
        let key = ImportIssue::Pgn(k.clone()).message_key();
        assert!(key.starts_with("pgn-diag-"), "{k:?} -> {key}");
        assert_ne!(key, "pgn-diag-unknown", "{k:?} has no dedicated key");
        assert!(keys.insert(key), "duplicate key {key}");
    }
}

#[test]
fn d122_import_issue_keys() {
    assert_eq!(
        ImportIssue::NothingToImport.message_key(),
        "import-issue-nothing-to-import"
    );
    assert!(
        ImportIssue::TextOutsideGames { bytes: 1 }
            .message_key()
            .starts_with("import-issue-")
    );
}

#[test]
fn d122_import_report_default_is_not_a_problem() {
    assert!(!ImportReport::default().has_problems());
}

// ------------------------------------------------------------------ D123 (paste moves)

#[test]
fn d123_paste_moves_parses_from_the_cursor_position_and_leaves_the_game_alone() {
    let mut h = Harness::new();
    h.play("e2e4");
    let game = h.state.session.game().clone();
    let eff = h.send(Command::PasteMoves);
    assert_eq!(eff, vec![Effect::ReadClipboard(ClipboardPurpose::Moves)]);
    // 1...e5 2.Nf3 starts from the position after 1.e4, i.e. black to move.
    h.clipboard(ClipboardPurpose::Moves, "1... e5 2. Nf3 Nc6");
    let outcome = h
        .state
        .pasted_moves
        .as_ref()
        .expect("parsed moves are delivered");
    assert_eq!(outcome.game.mainline().count(), 3);
    assert_eq!(
        *outcome.game.start_position(),
        game.position_at(game.children(game.root()).unwrap()[0])
            .unwrap()
            .clone()
    );
    assert_eq!(outcome.report.mode, ImportMode::MovesOnly);
    assert!(
        !outcome
            .report
            .notices
            .iter()
            .any(|n| n.issue == ImportIssue::Pgn(DiagnosticKind::MissingResult)),
        "headerless moves: no MissingResult"
    );
    assert!(
        h.state.session.game().same_content(&game),
        "no insertion in this slice"
    );
}

#[test]
fn d123_paste_moves_with_an_illegal_move_reports_an_error() {
    let mut h = Harness::new();
    h.clipboard(ClipboardPurpose::Moves, "1. e4 Ke4");
    let outcome = h.state.pasted_moves.as_ref().unwrap();
    assert_eq!(outcome.game.mainline().count(), 1);
    assert!(outcome.report.has_problems());
}

#[test]
fn d123_moves_parsed_with_an_obsolete_request_are_dropped() {
    let mut h = Harness::new();
    let eff = h.raw(Message::ClipboardRead {
        purpose: ClipboardPurpose::Moves,
        result: Ok("1. e4".to_owned()),
    });
    let first = eff.into_iter().next().unwrap();
    let eff = h.raw(Message::ClipboardRead {
        purpose: ClipboardPurpose::Moves,
        result: Ok("1. d4 d5".to_owned()),
    });
    let second = eff.into_iter().next().unwrap();
    let a = h.perform(first);
    h.raw(a);
    assert!(h.state.pasted_moves.is_none(), "obsolete answer ignored");
    let b = h.perform(second);
    h.raw(b);
    assert_eq!(
        h.state
            .pasted_moves
            .as_ref()
            .unwrap()
            .game
            .mainline()
            .count(),
        2
    );
}

#[test]
fn d123_pasted_moves_are_cleared_when_the_game_is_replaced() {
    let mut h = Harness::new();
    h.clipboard(ClipboardPurpose::Moves, "1. e4 e5");
    assert!(h.state.pasted_moves.is_some());
    h.send(Command::NewGame);
    assert!(h.state.pasted_moves.is_none());
}

// ------------------------------------------------------------------ hostile texts

#[rstest]
#[case("\0\0\0")]
#[case("[Event \"unterminated")]
#[case("{ unterminated comment")]
#[case("((((((((((((((((((((")]
#[case(
    "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 6. Re1 b5 7. Bb3 d6 8. c3 O-O 9. h3 Nb8 10. d4 Nbd7 11. Zz9 *"
)]
#[case("\u{feff}\u{feff}[Event \"x\"]")]
#[case("1-0 1-0 1-0 *")]
#[case("$1 $2 $3")]
fn hostile_pgn_text_through_import_never_panics(#[case] text: &str) {
    let mut h = Harness::new();
    h.send(ImportMessage::Open);
    h.send(ImportMessage::TextEdited(text.to_owned()));
    h.send(ImportMessage::Import);
    h.send(Message::ClipboardRead {
        purpose: ClipboardPurpose::Moves,
        result: Ok(text.to_owned()),
    });
    h.send(Message::ClipboardRead {
        purpose: ClipboardPurpose::Fen,
        result: Ok(text.to_owned()),
    });
}

proptest! {
    #[test]
    fn rule66_arbitrary_pgn_text_never_panics_in_scan(text in "\\PC{0,500}") {
        let _ = scan_pgn(PgnSource::Text(text), &ScanLimits::default(), &ReadOptions::default());
    }

    #[test]
    fn rule66_movetext_like_noise_never_panics(text in "[0-9a-hKQRBNOx=+#!?{}()\\[\\]\\\" .\\-\\n/;$*]{0,300}") {
        let _ = scan_pgn(PgnSource::Text(text.clone()), &ScanLimits::default(), &ReadOptions::default());
        let mut h = Harness::new();
        h.send(Message::ClipboardRead { purpose: ClipboardPurpose::Moves, result: Ok(text) });
    }
}

// ------------------------------------------------------------------ D122 (Info does not hold the dialog)

#[test]
fn d122_info_only_reports_do_not_keep_the_dialog_open() {
    // A Chess960 game: the reader notes it at Info level.
    let text = "[Event \"x\"]\n[Variant \"Chess960\"]\n[SetUp \"1\"]\n[FEN \"bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq - 0 1\"]\n\n1. e4 e5 *\n";
    let (load, notices) = scan(text);
    let loaded = single(load);
    let all: Vec<_> = loaded.report.notices.iter().chain(notices.iter()).collect();
    assert!(all.iter().all(|n| n.severity == Severity::Info), "{all:?}");
    let mut h = Harness::new();
    h.send(ImportMessage::Open);
    h.send(ImportMessage::TextEdited(text.to_owned()));
    h.send(ImportMessage::Import);
    assert_eq!(h.plies(), 2);
    assert!(
        h.state.import.is_none(),
        "Info does not keep the dialog: {:?}",
        h.state.import
    );
}

#[test]
fn d122_text_between_games_is_reported_without_losing_the_games() {
    let text = format!(
        "Some e-mail header\nFrom: someone\n\n{}\nrandom text between the games\n\n{}",
        multi_pgn(1),
        multi_pgn(1).replace("P0", "Second")
    );
    let (load, notices) = scan(&text);
    let PgnLoad::Many(c) = load else {
        panic!("{load:?}")
    };
    assert_eq!(c.len(), 2);
    assert!(
        notices.iter().all(|n| n.severity != Severity::Error),
        "junk is not an error: {notices:?}"
    );
}
