//! Contract tests for the Polyglot Zobrist key (spec D1103, vectors table).

mod common;

use common::{key_of, pos_after};
use mychess_book::polyglot_key;
use mychess_core::Position;

fn key(fen: &str) -> u64 {
    polyglot_key(&Position::from_fen(fen).expect("valid FEN"))
}

/// Last entry of `Random64` (the turn key), fixed by the spec.
const TURN_KEY: u64 = 0xF8D6_26AA_AF27_8509;

#[test]
fn the_nine_spec_vectors_from_fen() {
    let cases = [
        (
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            0x463b_9618_1691_fc9c_u64,
        ),
        (
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
            0x823c_9b50_fd11_4196,
        ),
        (
            "rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 2",
            0x0756_b944_61c5_0fb0,
        ),
        (
            "rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR b KQkq - 0 2",
            0x662f_afb9_65db_29d4,
        ),
        (
            "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
            0x22a4_8b5a_8e47_ff78,
        ),
        (
            "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPPKPPP/RNBQ1BNR b kq - 0 3",
            0x652a_607c_a3f2_42c1,
        ),
        (
            "rnbq1bnr/ppp1pkpp/8/3pPp2/8/8/PPPPKPPP/RNBQ1BNR w - - 0 4",
            0x00fd_d303_c946_bdd9,
        ),
        (
            "rnbqkbnr/p1pppppp/8/8/PpP4P/8/1P1PPPP1/RNBQKBNR b KQkq c3 0 3",
            0x3c81_23ea_7b06_7637,
        ),
        (
            "rnbqkbnr/p1pppppp/8/8/P6P/R1p5/1P1PPPP1/1NBQKBNR b Kkq - 0 4",
            0x5c3f_9b82_9b27_9560,
        ),
    ];
    for (fen, expected) in cases {
        assert_eq!(key(fen), expected, "{fen}");
    }
}

#[test]
fn the_nine_spec_vectors_from_played_moves() {
    // The key is computed from the Position reached by play, not from text.
    let cases: [(&[&str], u64); 9] = [
        (&[], 0x463b_9618_1691_fc9c),
        (&["e2e4"], 0x823c_9b50_fd11_4196),
        (&["e2e4", "d7d5"], 0x0756_b944_61c5_0fb0),
        (&["e2e4", "d7d5", "e4e5"], 0x662f_afb9_65db_29d4),
        (&["e2e4", "d7d5", "e4e5", "f7f5"], 0x22a4_8b5a_8e47_ff78),
        (
            &["e2e4", "d7d5", "e4e5", "f7f5", "e1e2"],
            0x652a_607c_a3f2_42c1,
        ),
        (
            &["e2e4", "d7d5", "e4e5", "f7f5", "e1e2", "e8f7"],
            0x00fd_d303_c946_bdd9,
        ),
        (
            &["a2a4", "b7b5", "h2h4", "b5b4", "c2c4"],
            0x3c81_23ea_7b06_7637,
        ),
        (
            &["a2a4", "b7b5", "h2h4", "b5b4", "c2c4", "b4c3", "a1a3"],
            0x5c3f_9b82_9b27_9560,
        ),
    ];
    for (ucis, expected) in cases {
        assert_eq!(key_of(ucis), expected, "{ucis:?}");
    }
}

#[test]
fn negative_vector_wrong_rule_value_is_not_produced() {
    // 1.e4 has no black pawn next to e4: the e3 square must NOT be hashed.
    // Hashing it anyway would give 4d0dde8ef7cc031f (spec, negative test).
    let k = key("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1");
    assert_ne!(k, 0x4d0d_de8e_f7cc_031f);
    assert_eq!(k, 0x823c_9b50_fd11_4196);
}

#[test]
fn turn_contributes_exactly_the_last_table_value_for_white() {
    let white = key("4k3/8/8/8/8/8/8/4K3 w - - 0 1");
    let black = key("4k3/8/8/8/8/8/8/4K3 b - - 0 1");
    assert_eq!(white ^ black, TURN_KEY);
}

#[test]
fn clocks_and_move_numbers_do_not_change_the_key() {
    let a = key("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
    let b = key("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 37 90");
    assert_eq!(a, b);
}

/// XOR contributed by an en passant square, as `with ^ without`.
fn ep_delta(with: &str, without: &str) -> u64 {
    key(with) ^ key(without)
}

#[test]
fn en_passant_with_adjacent_capturer_adds_a_file_dependent_constant() {
    // White pawn c5 next to the black pawn that just reached d5: d-file.
    let d = ep_delta(
        "4k3/8/8/2Pp4/8/8/8/4K3 w - d6 0 1",
        "4k3/8/8/2Pp4/8/8/8/4K3 w - - 0 1",
    );
    // a2-a4 next to a black pawn on b4: a-file constant.
    let a = ep_delta(
        "4k3/8/8/8/Pp6/8/8/4K3 b - a3 0 1",
        "4k3/8/8/8/Pp6/8/8/4K3 b - - 0 1",
    );
    // h2-h4 next to a black pawn on g4: h-file constant.
    let h = ep_delta(
        "4k3/8/8/8/6pP/8/8/4K3 b - h3 0 1",
        "4k3/8/8/8/6pP/8/8/4K3 b - - 0 1",
    );
    // Vector 5 (1.e4 d5 2.e5 f5): f-file.
    let f = ep_delta(
        "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
        "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq - 0 3",
    );
    let all = [a, d, f, h];
    for (i, x) in all.iter().enumerate() {
        assert_ne!(*x, 0, "file {i} must be hashed");
        for y in &all[i + 1..] {
            assert_ne!(x, y, "one constant per file");
        }
    }
    // The constant depends on the file only, not on the rest of the position.
    let d_other = ep_delta(
        "r3k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
        "r3k3/8/8/3pP3/8/8/8/4K3 w - - 0 1",
    );
    assert_eq!(d, d_other);
}

#[test]
fn vector_three_en_passant_square_is_not_hashed() {
    // After 1.e4 d5 the white pawn stands on e4, not next to d5 on the 5th
    // rank: the d6 of the FEN must not enter the key.
    assert_eq!(
        key("rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 2"),
        key("rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2"),
    );
}

#[test]
fn en_passant_for_black_capturer_also_counts() {
    // White just played e2-e4 with a black pawn on d4: black may capture.
    let delta = ep_delta(
        "4k3/8/8/8/3pP3/8/8/4K3 b - e3 0 1",
        "4k3/8/8/8/3pP3/8/8/4K3 b - - 0 1",
    );
    assert_ne!(delta, 0);
}

#[test]
fn en_passant_without_adjacent_capturer_is_not_hashed() {
    // After 1.e4 nothing stands next to e4.
    assert_eq!(
        key("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1"),
        key("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1"),
    );
    // A pawn of the wrong colour next to it does not count (white d4 pawn,
    // black to move): only the side to move can capture.
    assert_eq!(
        key("4k3/8/8/8/3PP3/8/8/4K3 b - e3 0 1"),
        key("4k3/8/8/8/3PP3/8/8/4K3 b - - 0 1"),
    );
}

#[test]
fn pinned_capturer_still_counts_for_en_passant() {
    // exd6 e.p. would expose the white king on a5 to the rook on h5: the
    // capture is illegal, but the Polyglot rule ignores pins (D1103).
    let pinned = ep_delta(
        "4k3/8/8/K2pP2r/8/8/8/8 w - d6 0 1",
        "4k3/8/8/K2pP2r/8/8/8/8 w - - 0 1",
    );
    let normal = ep_delta(
        "4k3/8/8/2Pp4/8/8/8/4K3 w - d6 0 1",
        "4k3/8/8/2Pp4/8/8/8/4K3 w - - 0 1",
    );
    assert_ne!(pinned, 0, "the file of a pinned capturer is hashed");
    assert_eq!(pinned, normal);
}

#[test]
fn each_castling_right_contributes_its_own_nonzero_constant() {
    let base = |rights: &str| key(&format!("r3k2r/8/8/8/8/8/8/R3K2R w {rights} - 0 1"));
    let none = base("-");
    let singles: Vec<u64> = ["K", "Q", "k", "q"]
        .iter()
        .map(|r| base(r) ^ none)
        .collect();
    for (i, s) in singles.iter().enumerate() {
        assert_ne!(*s, 0, "right {i}");
        for (j, t) in singles.iter().enumerate() {
            if i != j {
                assert_ne!(s, t, "rights {i} and {j} must differ");
            }
        }
    }
    // Rights combine by XOR.
    assert_eq!(base("KQkq") ^ none, singles.iter().fold(0, |a, b| a ^ b));
    assert_eq!(base("Kq") ^ none, singles[0] ^ singles[3]);
    assert_eq!(base("Qk") ^ none, singles[1] ^ singles[2]);
}

#[test]
fn castling_rights_are_identified_by_colour_and_side() {
    // Cross-check against vectors: lacking Q+K but keeping kq (vector 6) and
    // lacking everything (vector 7) pin the black/white split.
    let kq = key("rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPPKPPP/RNBQ1BNR b kq - 0 3");
    assert_eq!(kq, 0x652a_607c_a3f2_42c1);
    // K only for white, kq for black (vector 9).
    let k_kq = key("rnbqkbnr/p1pppppp/8/8/P6P/R1p5/1P1PPPP1/1NBQKBNR b Kkq - 0 4");
    assert_eq!(k_kq, 0x5c3f_9b82_9b27_9560);
}

#[test]
fn losing_a_right_by_moving_the_rook_changes_the_key() {
    let start = "r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R w KQkq - 0 1";
    let there_and_back = Position::from_fen(start).expect("fen");
    let mut p = there_and_back.clone();
    for u in ["a1b1", "a8b8", "b1a1", "b8a8"] {
        let m = p.parse_uci(u).expect("legal");
        p.play(m).expect("play");
    }
    // Same pieces, white to move again, but Q and q are gone.
    assert_ne!(polyglot_key(&p), polyglot_key(&there_and_back));
    assert_eq!(
        polyglot_key(&p),
        key("r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R w Kk - 4 3")
    );
}

#[test]
fn transpositions_give_the_same_key() {
    let a = key_of(&["g1f3", "g8f6", "b1c3", "b8c6"]);
    let b = key_of(&["b1c3", "b8c6", "g1f3", "g8f6"]);
    let c = key_of(&["g1f3", "b8c6", "b1c3", "g8f6"]);
    assert_eq!(a, b);
    assert_eq!(a, c);
    // 1.e4 e5 2.Nf3 versus 1.Nf3 e5 2.e4.
    assert_eq!(
        key_of(&["e2e4", "e7e5", "g1f3"]),
        key_of(&["g1f3", "e7e5", "e2e4"])
    );
    // A different position differs.
    assert_ne!(a, key_of(&["g1f3", "g8f6", "b1c3"]));
}

#[test]
fn key_comes_from_the_position_not_from_how_it_was_built() {
    let played = pos_after(&["e2e4", "e7e5", "g1f3", "b8c6"]);
    let parsed = Position::from_fen(&played.fen()).expect("fen round trip");
    assert_eq!(polyglot_key(&played), polyglot_key(&parsed));
}
