//! Contract tests of the `.sg4` record decoder (spec M3-1; `docs/formatos/si4.md`
//! section 9): hand-built records, with the legality-checked results compared
//! against SAN played by the core (an oracle independent of the piece list).
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::decimal_bitwise_operands
)]

#[path = "common/si4_builder.rs"]
mod si4_builder;

use mychess_core::{Game, Move, Role};
use mychess_db::si4::{Si4Database, Si4Error, bytes_to_string};
use mychess_pgn::write::{WriteOptions, game_to_string};
use pretty_assertions::assert_eq;
use rstest::rstest;
use si4_builder::{
    MemDb, final_fen, mainline_moves, mb, one, rec, rec_fen, rec_full, san_line, tag_code,
    tag_event_date, tag_literal,
};

fn movetext(game: &Game) -> String {
    let text = game_to_string(game, &WriteOptions::default()).unwrap();
    let body = text.rsplit("\n\n").next().unwrap();
    body.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn check_line(record: Vec<u8>, fen: Option<&str>, sans: &[&str]) {
    let mut db = one(record);
    let game = db.game(1).unwrap();
    let (moves, fen_after) = san_line(fen, sans);
    assert_eq!(mainline_moves(&game), moves);
    assert_eq!(final_fen(&game), fen_after);
    assert_eq!(db.decode_mainline(1).unwrap().len(), moves.len());
}

const STD: Option<&str> = None;

#[test]
fn every_piece_kind_decodes_through_the_piece_list() {
    let stream = [
        0xCF, 0xCF, 0x67, 0x22, 0x5A, 0x52, 0x0A, 0x61, 0xB1, 0xB1, 0x36, 0xF1, 0x3D, 0x43, 0x6D,
        0x31, 0x46, 0x74, 0x61, 0x7A, 0x3C, 0x49, 0x81, 0x81, 0x1E, 0x0F,
    ];
    check_line(
        rec(&stream, &[]),
        STD,
        &[
            "e4", "e5", "Nf3", "Nc6", "Bc4", "Bc5", "O-O", "Nf6", "d3", "d6", "Bg5", "h6", "Bxf6",
            "Qxf6", "h3", "Qg6", "Re1", "h5", "Re3", "Be6", "Qd2", "a6", "a3", "Ra7",
        ],
    );
}

#[rstest]
#[case::queen(0x14, 0x17, "a8=Q", "Qh8")]
#[case::rook(0x17, 0x17, "a8=R", "Rh8")]
#[case::bishop(0x1A, 0x1F, "a8=B", "Bh1")]
#[case::knight(0x1D, 0x12, "a8=N", "Nb6")]
fn push_promotion_then_a_move_of_the_promoted_piece(
    #[case] promo: u8,
    #[case] next: u8,
    #[case] s1: &str,
    #[case] s3: &str,
) {
    let fen = "8/P6k/8/8/8/8/8/K7 w - - 0 1";
    let r = rec_fen(fen, &[promo, 0x01, next, 0x0F], &[]);
    check_line(r, Some(fen), &[s1, "Kg6", s3]);
}

#[rstest]
#[case::queen(0x15, 0x18, "axb8=Q", "Qb1")]
#[case::rook(0x18, 0x18, "axb8=R", "Rb1")]
#[case::bishop(0x1B, 0x1F, "axb8=B", "Bh2")]
#[case::knight(0x1E, 0x12, "axb8=N", "Nc6")]
fn capture_promotion_then_a_move_of_the_promoted_piece(
    #[case] promo: u8,
    #[case] next: u8,
    #[case] s1: &str,
    #[case] s3: &str,
) {
    let fen = "1r5k/P7/8/8/8/8/8/K7 w - - 0 1";
    let r = rec_fen(fen, &[promo, 0x01, next, 0x0F], &[]);
    check_line(r, Some(fen), &[s1, "Kg7", s3]);
}

#[test]
fn promotion_roles_are_reported_by_raw_game() {
    for (byte, role) in [
        (0x14, Role::Queen),
        (0x17, Role::Rook),
        (0x1A, Role::Bishop),
        (0x1D, Role::Knight),
    ] {
        let mut db = one(rec_fen("8/P6k/8/8/8/8/8/K7 w - - 0 1", &[byte, 0x0F], &[]));
        let raw = db.raw_game(1).unwrap();
        assert_eq!(raw.mainline()[0].mv.promotion, Some(role));
    }
}

#[test]
fn captured_piece_is_swapped_with_the_last_one_of_the_list() {
    // 1.b7xc8=Q Rb8xc8: after the capture the black rook takes index 1.
    let fen = "1rb4k/1P6/8/8/8/8/8/K7 w - - 0 1";
    check_line(
        rec_fen(fen, &[0x15, 0x12, 0x0F], &[]),
        Some(fen),
        &["bxc8=Q", "Rxc8"],
    );
}

#[test]
fn after_a_capture_the_last_index_is_stale_and_the_swapped_piece_has_the_slot() {
    // 1.e4 d5 2.exd5: the black h pawn (index 15) now has index 11.
    let ok = rec(&[0xCF, 0xBF, 0xC0, 0xB1, 0x0F], &[]);
    check_line(ok, STD, &["e4", "d5", "exd5", "h6"]);
    let stale = rec(&[0xCF, 0xBF, 0xC0, 0xF1, 0x0F], &[]);
    let mut db = one(stale);
    assert!(db.raw_game(1).is_err(), "index 15 no longer exists");
    assert!(db.decode_mainline(1).is_err());
    assert!(db.game(1).is_err());
}

#[test]
fn en_passant_captures_the_pawn_beside_for_both_colors() {
    check_line(
        rec(&[0xCF, 0x81, 0xC1, 0xBF, 0xC0, 0xB1, 0x0F], &[]),
        STD,
        &["e4", "a6", "e5", "d5", "exd6", "h6"],
    );
    check_line(
        rec(&[0xF1, 0x8F, 0xF1, 0x81, 0x9F, 0x80, 0x91, 0x0F], &[]),
        STD,
        &["h3", "a5", "h4", "a4", "b4", "axb3", "h5"],
    );
}

#[test]
fn castling_both_sides_for_both_colors() {
    let fen = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
    check_line(
        rec_fen(fen, &[0x09, 0x0A, 0x1F, 0x23, 0x13, 0x0F], &[]),
        Some(fen),
        &["O-O-O", "O-O", "Rd8", "Rfxd8", "Rd1"],
    );
    let fen_b = "r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1";
    check_line(
        rec_fen(fen_b, &[0x09, 0x0A, 0x0F], &[]),
        Some(fen_b),
        &["O-O-O", "O-O"],
    );
}

#[test]
fn null_move_is_byte_zero_of_the_king() {
    let mut db = one(rec(&[0xCF, 0x00, 0xBF, 0x0F], &[]));
    let raw = db.raw_game(1).unwrap();
    assert!(raw.mainline()[1].mv.is_null());
    assert!(!raw.mainline()[0].mv.is_null());
    let game = db.game(1).unwrap();
    let (moves, _) = san_line(STD, &["e4", "--", "d4"]);
    assert_eq!(mainline_moves(&game), moves);
    assert!(moves.contains(&Move::Null));
}

#[rstest]
#[case::rook(vec![0x10])]
#[case::bishop(vec![0x32])]
#[case::queen_two_bytes(vec![0x43, 0x43])]
#[case::knight_v0(vec![0x20])]
fn a_non_king_piece_moving_to_its_own_square_is_an_error(#[case] mv: Vec<u8>) {
    let mut stream = mv;
    stream.push(0x0F);
    let mut db = one(rec(&stream, &[]));
    assert!(db.raw_game(1).is_err());
    assert!(db.decode_mainline(1).is_err());
}

#[test]
fn queen_diagonal_needs_a_second_byte_between_64_and_127() {
    // Qd1 is piece 4 (column 3); 0x43 starts the diagonal form.
    for n in [0u8, 63, 128, 255] {
        assert!(
            one(rec(&[0x43, n, 0x0F], &[])).raw_game(1).is_err(),
            "n {n}"
        );
    }
    assert!(one(rec(&[0x43], &[])).raw_game(1).is_err());
    let ok = one(rec(&[0x43, 64 + 16, 0x0F], &[])).raw_game(1).unwrap();
    assert_eq!(ok.mainline()[0].mv.to.to_string(), "a3");
}

#[test]
fn fen_start_with_king_after_other_pieces_reorders_the_list() {
    let fen = "6k1/8/8/8/8/8/PPP5/RNBQKBNR w - - 0 1";
    let stream = [
        0x11, 0x04, 0x97, 0x04, 0xA6, 0x03, 0x59, 0x04, 0x63, 0x4C, 0x0F,
    ];
    check_line(
        rec_fen(fen, &stream, &[]),
        Some(fen),
        &["b3", "Kf8", "Nf3", "Ke8", "Rg1", "Kf7", "Bb2", "Ke7", "Qe2"],
    );
    let fen_b = "rnbqkbnr/8/8/8/8/8/8/4K3 b - - 0 1";
    check_line(
        rec_fen(fen_b, &[0x12, 0x08, 0x2C, 0x08, 0x3A, 0x0F], &[]),
        Some(fen_b),
        &["Nc6", "Kf2", "Be6", "Kg3", "Qd3"],
    );
}

#[rstest]
#[case::two_white_kings("4k3/8/8/8/8/8/8/K3K3 w - - 0 1")]
#[case::no_black_king("8/8/8/8/8/8/8/4K3 w - - 0 1")]
#[case::seventeen_white("4k3/8/8/8/8/PPPPPPPP/PPPPPPPP/4K3 w - - 0 1")]
#[case::pawn_on_rank_one("4k3/8/8/8/8/8/8/P3K3 w - - 0 1")]
#[case::pawn_on_rank_eight("P3k3/8/8/8/8/8/8/4K3 w - - 0 1")]
#[case::bad_letter("4x3/8/8/8/8/8/8/4K3 w - - 0 1")]
#[case::nine_squares("4k4/8/8/8/8/8/8/4K3 w - - 0 1")]
#[case::seven_ranks("4k3/8/8/8/8/8/4K3 w - - 0 1")]
#[case::bad_side("4k3/8/8/8/8/8/8/4K3 x - - 0 1")]
fn invalid_start_fen_is_an_error(#[case] fen: &str) {
    let mut db = one(rec_fen(fen, &[0x0F], &[]));
    assert!(db.raw_game(1).is_err(), "{fen}");
    assert!(db.game(1).is_err());
}

#[test]
fn sixteen_pieces_per_side_are_accepted_and_core_illegal_starts_fail_only_in_game() {
    let mut db = one(rec_fen(
        "4k3/8/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1",
        &[0x0F],
        &[],
    ));
    assert!(db.raw_game(1).is_ok());
    assert!(db.game(1).is_ok());
    // Adjacent kings: structurally fine (D259), refused by the core.
    let mut db = one(rec_fen("8/8/8/8/8/8/3kK3/8 w - - 0 1", &[0x0F], &[]));
    assert!(db.raw_game(1).is_ok());
    assert!(matches!(
        db.game(1),
        Err(Si4Error::IllegalStart { game: 1, .. })
    ));
}

#[test]
fn fen_flag_without_a_terminated_fen_is_an_error() {
    let mut r = vec![0u8, 1];
    r.extend_from_slice(b"4k3/8/8/8/8/8/8/4K3 w");
    assert!(one(r).raw_game(1).is_err());
}

// ---------------------------------------------------- variations and NAGs

#[test]
fn multiple_and_nested_variations_build_the_tree() {
    // 1.e4 (1.d4 d5 (1...Nf6) 2.c4) (1.Nf3) e5
    let stream = [
        0xCF, 13, 0xBF, 0xBF, 13, 0x61, 14, 0xAF, 14, 13, 0x67, 14, 0xCF, 0x0F,
    ];
    let mut db = one(rec(&stream, &[]));
    let raw = db.raw_game(1).unwrap();
    assert_eq!(raw.lines.len(), 4);
    assert_eq!(raw.mainline().len(), 2);
    assert_eq!(raw.mainline()[0].variations.len(), 2);
    let game = db.game(1).unwrap();
    let text = movetext(&game);
    assert_eq!(text, "1. e4 (1. d4 d5 (1... Nf6) 2. c4) (1. Nf3) 1... e5 *");
}

#[test]
fn variation_after_a_capture_restores_the_piece_list() {
    // 1.e4 d5 2.exd5 (2.Nc3 d4) 2...Qxd5 3.Nc3 Qa5 4.h3 h6
    let stream = [
        0xCF, 0xBF, 0xC0, 13, 0x28, 0xB1, 14, 0x4C, 0x28, 0x40, 0xC1, 0xB1, 0x0F,
    ];
    let mut db = one(rec(&stream, &[]));
    let game = db.game(1).unwrap();
    let (moves, fen) = san_line(STD, &["e4", "d5", "exd5", "Qxd5", "Nc3", "Qa5", "h3", "h6"]);
    assert_eq!(mainline_moves(&game), moves);
    assert_eq!(final_fen(&game), fen);
    assert_eq!(
        movetext(&game),
        "1. e4 d5 2. exd5 (2. Nc3 d4) 2... Qxd5 3. Nc3 Qa5 4. h3 h6 *"
    );
}

#[test]
fn variation_after_a_promotion_restores_the_pawn() {
    // 1.a8=N (1.a8=Q Kg6) 1...Kg6 2.Nb6
    let fen = "8/P6k/8/8/8/8/8/K7 w - - 0 1";
    let r = rec_fen(fen, &[0x1D, 13, 0x14, 0x01, 14, 0x01, 0x12, 0x0F], &[]);
    let mut db = one(r);
    let game = db.game(1).unwrap();
    let (moves, _) = san_line(Some(fen), &["a8=N", "Kg6", "Nb6"]);
    assert_eq!(mainline_moves(&game), moves);
    assert!(movetext(&game).contains("(1. a8=Q Kg6)"));
}

#[test]
fn nag_zero_is_ignored_and_at_most_seven_nags_are_kept() {
    let mut s = vec![0xCF];
    for n in [0u8, 1, 0, 2, 3, 4, 5, 6, 7, 8, 9] {
        s.extend_from_slice(&[11, n]);
    }
    s.extend_from_slice(&[0xCF, 0x0F]);
    let mut db = one(rec(&s, &[]));
    let raw = db.raw_game(1).unwrap();
    let nags: Vec<u8> = raw.mainline()[0].nags.iter().map(|n| n.value()).collect();
    assert_eq!(nags, vec![1, 2, 3, 4, 5, 6, 7]);
    let game = db.game(1).unwrap();
    let id = game.mainline().next().unwrap();
    assert_eq!(game.nags(id).unwrap().len(), 7);
}

#[test]
fn nag_value_bytes_equal_to_stream_tokens_are_values() {
    // 11 0F is NAG 15, not END_GAME; 11 0E is NAG 14.
    let mut db = one(rec(&[0xCF, 11, 0x0F, 11, 0x0E, 0xCF, 0x0F], &[]));
    let raw = db.raw_game(1).unwrap();
    let nags: Vec<u8> = raw.mainline()[0].nags.iter().map(|n| n.value()).collect();
    assert_eq!(nags, vec![15, 14]);
    assert_eq!(db.decode_mainline(1).unwrap().len(), 2);
}

#[test]
fn nag_before_the_first_move_is_ignored() {
    let mut db = one(rec(&[11, 5, 0xCF, 0x0F], &[]));
    let raw = db.raw_game(1).unwrap();
    assert_eq!(raw.mainline()[0].nags.len(), 0);
    assert!(db.game(1).is_ok());
}

#[test]
fn a_nag_token_at_the_end_of_the_record_is_an_error() {
    assert!(one(rec(&[0xCF, 11], &[])).raw_game(1).is_err());
}

// ----------------------------------------------------------- comments

#[test]
fn comments_are_read_in_pre_order() {
    // {c0} 1.e4 {c1} ({c2} 1.d4 {c3} d5 {c4} (1...Nf6 {c5}) 2.c4 {c6}) e5 {c7}
    let stream = [
        12, 0xCF, 12, 13, 12, 0xBF, 12, 0xBF, 12, 13, 0x61, 12, 14, 0xAF, 12, 14, 0xCF, 12, 0x0F,
    ];
    let texts: Vec<Vec<u8>> = (0..8).map(|i| format!("c{i}").into_bytes()).collect();
    let refs: Vec<&[u8]> = texts.iter().map(Vec::as_slice).collect();
    let mut db = one(rec(&stream, &refs));
    let raw = db.raw_game(1).unwrap();
    assert_eq!(raw.lines[0].start_comment.as_deref(), Some(&b"c0"[..]));
    assert_eq!(raw.lines[0].moves[0].comment.as_deref(), Some(&b"c1"[..]));
    assert_eq!(raw.lines[1].start_comment.as_deref(), Some(&b"c2"[..]));
    assert_eq!(raw.lines[1].moves[0].comment.as_deref(), Some(&b"c3"[..]));
    assert_eq!(raw.lines[1].moves[1].comment.as_deref(), Some(&b"c4"[..]));
    assert_eq!(raw.lines[2].moves[0].comment.as_deref(), Some(&b"c5"[..]));
    assert_eq!(raw.lines[1].moves[2].comment.as_deref(), Some(&b"c6"[..]));
    assert_eq!(raw.lines[0].moves[1].comment.as_deref(), Some(&b"c7"[..]));
    let game = db.game(1).unwrap();
    assert_eq!(game.initial_comment(), Some("c0"));
    assert_eq!(
        movetext(&game),
        "{c0} 1. e4 {c1} ({c2} 1. d4 {c3} 1... d5 {c4} (1... Nf6 {c5}) 2. c4 {c6}) 1... e5 {c7} *"
    );
}

#[test]
fn several_comment_marks_on_one_move_count_as_one_comment() {
    let mut db = one(rec(&[0xCF, 12, 12, 0xCF, 0x0F], &[b"x"]));
    let raw = db.raw_game(1).unwrap();
    assert_eq!(raw.mainline()[0].comment.as_deref(), Some(&b"x"[..]));
}

#[test]
fn missing_or_unterminated_comment_strings_are_errors() {
    // one mark, no string
    assert!(one(rec(&[0xCF, 12, 0x0F], &[])).raw_game(1).is_err());
    // two marks, one string
    assert!(
        one(rec(&[0xCF, 12, 0xCF, 12, 0x0F], &[b"a"]))
            .raw_game(1)
            .is_err()
    );
    // no final NUL
    let mut r = rec(&[0xCF, 12, 0x0F], &[]);
    r.extend_from_slice(b"abc");
    assert!(one(r).raw_game(1).is_err());
}

#[test]
fn extra_bytes_after_the_last_comment_are_tolerated() {
    let mut r = rec(&[0xCF, 12, 0x0F], &[b"x"]);
    r.extend_from_slice(b"junk\0more");
    let mut db = one(r);
    let raw = db.raw_game(1).unwrap();
    assert_eq!(raw.mainline()[0].comment.as_deref(), Some(&b"x"[..]));
    let mut db = one(rec(&[0xCF, 0x0F], &[b"stray"]));
    assert!(db.game(1).is_ok());
}

#[test]
fn an_empty_comment_string_is_a_present_empty_comment() {
    let mut db = one(rec(&[0xCF, 12, 0x0F], &[b""]));
    let raw = db.raw_game(1).unwrap();
    assert_eq!(raw.mainline()[0].comment.as_deref(), Some(&b""[..]));
}

#[test]
fn comment_text_is_utf8_when_valid_and_windows_1252_otherwise() {
    assert_eq!(bytes_to_string(b""), "");
    assert_eq!(bytes_to_string("é€".as_bytes()), "é€");
    assert_eq!(bytes_to_string(b"caf\xE9"), "café");
    assert_eq!(bytes_to_string(b"\x80"), "€");
    assert_eq!(bytes_to_string(b"\xC3"), "Ã");
    let mut db = one(rec(&[0xCF, 12, 0x0F], &[b"caf\xE9"]));
    let game = db.game(1).unwrap();
    let id = game.mainline().next().unwrap();
    assert_eq!(game.comment(id).unwrap(), Some("café"));
}

// ----------------------------------------------------- variation merging

#[test]
fn a_variation_repeating_the_main_move_is_merged_and_reported() {
    // 1.e4 (1.e4 e5) 1...c5 : the duplicate is merged, its continuation becomes a variation.
    let stream = [0xCF, 13, 0xCF, 0xCF, 14, 0xAF, 0x0F];
    let mut db = one(rec(&stream, &[]));
    let raw = db.raw_game(1).unwrap();
    assert_eq!(raw.lines.len(), 2);
    let (game, report) = db.game_with_report(1).unwrap();
    assert_eq!(report.merged_variations, 1);
    assert_eq!(report.discarded_variations, 0);
    assert_eq!(movetext(&game), "1. e4 c5 (1... e5) *");
}

// ----------------------------------------------------------------- tags

fn tags_of(db: &mut MemDb) -> Vec<(Vec<u8>, Vec<u8>)> {
    db.raw_game(1).unwrap().extra_tags
}

#[test]
fn extra_tags_literal_common_and_repeated() {
    let mut tags = tag_literal(b"Note", b"x");
    tags.extend(tag_code(241, b"ESP"));
    tags.extend(tag_code(242, b"RUS"));
    tags.extend(tag_code(243, b"Anno"));
    tags.extend(tag_code(244, b"21"));
    tags.extend(tag_code(246, b"Sicilian"));
    tags.extend(tag_code(247, b"Najdorf"));
    tags.extend(tag_code(248, b"setup"));
    tags.extend(tag_code(249, b"src"));
    tags.extend(tag_code(250, b"1"));
    tags.extend(tag_literal(b"Note", b"y")); // replaces
    tags.extend(tag_literal(b"Annotator", b"Other")); // same name as 243: replaces
    let mut db = one(rec_full(&tags, 0, None, &[0x0F], &[]));
    let got = tags_of(&mut db);
    let find = |n: &str| {
        got.iter()
            .find(|(k, _)| k == n.as_bytes())
            .map(|(_, v)| v.clone())
    };
    assert_eq!(find("Note"), Some(b"y".to_vec()));
    assert_eq!(find("WhiteCountry"), Some(b"ESP".to_vec()));
    assert_eq!(find("BlackCountry"), Some(b"RUS".to_vec()));
    assert_eq!(find("Annotator"), Some(b"Other".to_vec()));
    assert_eq!(find("PlyCount"), Some(b"21".to_vec()));
    assert_eq!(find("Opening"), Some(b"Sicilian".to_vec()));
    assert_eq!(find("Variation"), Some(b"Najdorf".to_vec()));
    assert_eq!(find("Setup"), Some(b"setup".to_vec()));
    assert_eq!(find("Source"), Some(b"src".to_vec()));
    assert_eq!(find("SetUp"), Some(b"1".to_vec()));
    assert_eq!(got.len(), 10, "repeated names do not add entries");
    let game = db.game(1).unwrap();
    assert_eq!(game.tags().get_extra("Note"), Some("y"));
    assert_eq!(game.tags().get_extra("Annotator"), Some("Other"));
}

#[test]
fn event_date_tags_245_and_255() {
    let bits = (2003u32 << 9) | (7 << 5) | 15;
    let mut db = one(rec_full(&tag_event_date(bits), 0, None, &[0x0F], &[]));
    let raw = db.raw_game(1).unwrap();
    assert_eq!(raw.event_date.unwrap().to_string(), "2003.07.15");
    assert_eq!(
        db.game(1).unwrap().tags().event_date.to_string(),
        "2003.07.15"
    );
    let mut db = one(rec_full(
        &tag_code(245, b"2001.02.03"),
        0,
        None,
        &[0x0F],
        &[],
    ));
    assert_eq!(
        db.game(1).unwrap().tags().event_date.to_string(),
        "2001.02.03"
    );
}

#[test]
fn tag_codes_251_to_254_are_errors() {
    for code in 251..=254u8 {
        let mut db = one(rec_full(&tag_code(code, b"v"), 0, None, &[0x0F], &[]));
        assert!(
            matches!(db.raw_game(1), Err(Si4Error::CorruptRecord { game: 1, .. })),
            "code {code}"
        );
    }
}

#[test]
fn at_most_255_distinct_tags() {
    let build = |n: usize| {
        let mut t = Vec::new();
        for i in 0..n {
            t.extend(tag_literal(format!("T{i:04}").as_bytes(), b"v"));
        }
        t
    };
    let mut ok = one(rec_full(&build(255), 0, None, &[0x0F], &[]));
    assert_eq!(ok.raw_game(1).unwrap().extra_tags.len(), 255);
    assert!(ok.game(1).is_ok());
    let mut bad = one(rec_full(&build(256), 0, None, &[0x0F], &[]));
    assert!(bad.raw_game(1).is_err());
}

#[test]
fn tag_name_of_240_bytes_and_value_of_255_bytes_and_empty_value() {
    let name = vec![b'N'; 240];
    let value = vec![b'v'; 255];
    let mut t = tag_literal(&name, &value);
    t.extend(tag_literal(b"Empty", b""));
    let mut db = one(rec_full(&t, 0, None, &[0x0F], &[]));
    let tags = tags_of(&mut db);
    assert_eq!(tags[0], (name, value));
    assert_eq!(tags[1], (b"Empty".to_vec(), Vec::new()));
}

#[test]
fn tags_the_core_cannot_hold_are_dropped_from_the_game_but_kept_raw() {
    let mut t = tag_literal(b"Bad Name", b"x");
    t.extend(tag_literal(b"Event", b"Shadow"));
    t.extend(tag_literal(b"Keep", b"a\nb\rc"));
    let mut db = one(rec_full(&t, 0, None, &[0x0F], &[]));
    assert_eq!(tags_of(&mut db).len(), 3);
    let game = db.game(1).unwrap();
    assert_eq!(game.tags().get_extra("Bad Name"), None);
    assert_eq!(game.tags().event, "Wijk");
    assert_eq!(game.tags().get_extra("Keep"), Some("a b c"));
}

#[test]
fn every_strict_prefix_of_a_valid_record_is_an_error() {
    let mut tags = tag_literal(b"Note", b"x");
    tags.extend(tag_code(243, b"AB"));
    let records = vec![
        rec(&[0xCF, 0xCF, 0x0F], &[]),
        rec_full(
            &tags,
            0,
            None,
            &[0xCF, 12, 13, 0xBF, 12, 14, 0xCF, 11, 3, 0x0F],
            &[b"a", b"b"],
        ),
        rec_fen("4k3/8/8/8/8/8/8/R3K3 w - - 0 1", &[0x1C, 0x0F], &[]),
    ];
    for r in records {
        assert!(one(r.clone()).raw_game(1).is_ok());
        for len in 1..r.len() {
            let cut = r[..len].to_vec();
            let mut db = one(cut);
            assert!(db.raw_game(1).is_err(), "prefix {len} of {}", r.len());
            assert!(db.game(1).is_err());
        }
    }
}

// ------------------------------------------------------ structure errors

#[rstest]
#[case::no_end_game(vec![0xCF, 0xCF])]
#[case::end_marker_at_level_zero(vec![0xCF, 14, 0x0F])]
#[case::start_before_first_move(vec![13, 0xCF, 14, 0x0F])]
#[case::start_before_first_move_of_variation(vec![0xCF, 13, 13, 0xBF, 14, 14, 0x0F])]
#[case::end_game_inside_variation(vec![0xCF, 13, 0xBF, 0x0F])]
#[case::unclosed_variation(vec![0xCF, 13, 0xBF])]
fn structural_errors_are_reported_not_panics(#[case] stream: Vec<u8>) {
    let mut db = one(rec(&stream, &[]));
    assert!(db.raw_game(1).is_err());
    assert!(db.game(1).is_err());
}

#[test]
fn piece_index_beyond_the_list_and_squares_off_the_board_are_errors() {
    let two_kings = "4k3/8/8/8/8/8/8/4K3 w - - 0 1";
    assert!(
        one(rec_fen(two_kings, &[mb(2, 1), 0x0F], &[]))
            .raw_game(1)
            .is_err()
    );
    // Knight a1, offset -17: off the board.
    let knight = "4k3/8/8/8/8/8/8/N3K3 w - - 0 1";
    assert!(
        one(rec_fen(knight, &[mb(1, 1), 0x0F], &[]))
            .raw_game(1)
            .is_err()
    );
    // King e1 with v = 1 (-9).
    assert!(one(rec(&[0x01, 0x0F], &[])).raw_game(1).is_err());
    // Pawn h7: right capture +9 and double step +16 leave the board.
    let pawn = "4k3/7P/8/8/8/8/8/4K3 w - - 0 1";
    assert!(
        one(rec_fen(pawn, &[mb(1, 5), 0x0F], &[]))
            .raw_game(1)
            .is_err()
    );
    assert!(
        one(rec_fen(pawn, &[mb(1, 15), 0x0F], &[]))
            .raw_game(1)
            .is_err()
    );
    // Knight b1 (piece 2) with v = 9..: invalid.
    for v in [0u8, 9, 10] {
        assert!(
            one(rec(&[mb(2, v), 0x0F], &[])).raw_game(1).is_err(),
            "v {v}"
        );
    }
}

#[test]
fn illegal_but_well_formed_moves_are_illegal_move_errors_in_game_only() {
    // Rook a1 to a4 through its own pawn.
    let mut db = one(rec(&[mb(1, 11), 0x0F], &[]));
    assert!(db.raw_game(1).is_ok());
    assert!(matches!(
        db.game(1),
        Err(Si4Error::IllegalMove { game: 1, ply: 1 })
    ));
    // Castling from the start position.
    let mut db = one(rec(&[0x0A, 0x0F], &[]));
    assert!(db.game(1).is_err());
}

#[test]
fn variation_depth_is_limited_without_overflowing_the_stack() {
    let nested = |n: usize| {
        let mut s = Vec::new();
        for _ in 0..n {
            s.extend_from_slice(&[mb(6, 7), 13]);
        }
        s.push(mb(6, 7));
        s.extend(std::iter::repeat_n(14, n));
        s.push(0x0F);
        s
    };
    let mut db = one(rec(&nested(5_000), &[]));
    let raw = db.raw_game(1).unwrap();
    assert_eq!(raw.lines.len(), 5_001);
    drop(raw);
    let mut db = one(rec(&nested(30_000), &[]));
    assert!(db.raw_game(1).is_err());
    // Never closed: also an error.
    let mut s = Vec::new();
    for _ in 0..40_000 {
        s.extend_from_slice(&[mb(6, 7), 13]);
    }
    let mut db = one(rec(&s, &[]));
    assert!(db.raw_game(1).is_err());
}

#[test]
fn decode_mainline_skips_variations_nags_and_comments() {
    let stream = [0xCF, 11, 0x0F, 12, 13, 0xBF, 14, 0xCF, 0x0F];
    let mut db = one(rec(&stream, &[b"c"]));
    let m = db.decode_mainline(1).unwrap();
    let raw = db.raw_game(1).unwrap();
    assert_eq!(m, raw.mainline().iter().map(|n| n.mv).collect::<Vec<_>>());
    assert_eq!(m.len(), 2);
}

// ------------------------------------------------------ index to Game tags

#[test]
fn standard_tags_come_from_the_index_and_the_name_base() {
    use si4_builder::{EntrySpec, IndexHeader, Parts, default_names, index_file, open_parts};
    let mut e = EntrySpec::new(0, 5);
    e.white = 1;
    e.black = 0;
    e.result = 2;
    e.game_date = (1985 << 9) | (7 << 5) | 9;
    e.event_date = (5 << 9) | (3 << 5) | 0x1C;
    e.eco = 1 + 131 * 242;
    e.white_elo = 2810;
    e.white_type = 5;
    e.black_elo = 0;
    let p = Parts {
        index: index_file(&IndexHeader::default(), &[e.encode()]),
        names: default_names(),
        sg4: rec(&[0xCF, 0xCF, 0x0F], &[]),
    };
    let mut db = open_parts(&p).unwrap();
    let t = db.game(1).unwrap().tags().clone();
    assert_eq!((t.white.as_str(), t.black.as_str()), ("Carlsen", "Anand"));
    assert_eq!(
        (t.event.as_str(), t.site.as_str(), t.round.as_str()),
        ("Wijk", "Wijk aan Zee", "1")
    );
    assert_eq!(t.date.to_string(), "1985.07.09");
    assert_eq!(t.event_date.to_string(), "1986.03.28");
    assert_eq!(t.result.as_str(), "0-1");
    assert_eq!(t.eco.map(|x| x.to_string()).as_deref(), Some("C42"));
    let w = t.white_rating.unwrap();
    assert_eq!((w.value.get(), w.kind.name()), (2810, "DWZ"));
    assert!(t.black_rating.is_none());
}

#[test]
fn name_bytes_are_utf8_or_windows_1252() {
    use si4_builder::{EntrySpec, IndexHeader, NameRec, index_file, names_file_with, section};
    let players: Vec<NameRec> = vec![
        (0, 1, "Élan".as_bytes().to_vec()),
        (1, 1, b"Mu\xF1oz".to_vec()),
    ];
    let names = names_file_with(
        &[players, section(&["E"]), section(&["S"]), section(&["1"])],
        [1; 4],
        None,
    );
    let index = index_file(&IndexHeader::default(), &[EntrySpec::new(0, 3).encode()]);
    let mut db = Si4Database::from_parts(
        index,
        &names,
        std::io::Cursor::new(vec![0, 0, 15]),
        "x.sg4".into(),
    )
    .unwrap();
    let g = db.game(1).unwrap();
    assert_eq!(
        (g.tags().white.as_str(), g.tags().black.as_str()),
        ("Élan", "Muñoz")
    );
}
