//! Contract tests of the `.stc` layout (M5-3a, D1040 to D1045, D1057, D1058
//! of `docs/specs/m5-3-cache-stc.md`): the 22-byte header, the 71-byte node,
//! golden bytes worked out by hand, the whole file and the write/read round
//! trip.
#![allow(
    clippy::needless_pass_by_value,
    clippy::cast_possible_truncation,
    clippy::naive_bytecount,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::assert_is_empty
)]

#[path = "common/m5_3_support.rs"]
mod stc;

use mychess_core::{Position, Role, Square};
use mychess_db::Filter;
use mychess_db::si4::RawMove;
use mychess_db::tree::stc::format::{
    FILE_VERSION, HEADER_LEN, MAGIC, NODE_LEN, NodeExtras, StcHeader, decode_node, encode_node,
};
use mychess_db::tree::stc::{StcCache, StcError, pack_bytemap, unpack_bytemap};
use mychess_db::tree::{CachedTree, TreeCache, TreeNode};
use pretty_assertions::assert_eq;
use rstest::rstest;
use stc::{
    entry, file, header, node, node_full, simple_entry, start_board, stc_file, tmp_base, tree_of,
    write_stc,
};

fn corrupt<T: std::fmt::Debug>(r: Result<T, StcError>) {
    assert!(matches!(r, Err(StcError::Corrupt { .. })), "got {r:?}");
}

// ------------------------------------------------------------ header

#[test]
fn header_golden_bytes() {
    let h = StcHeader {
        version: 400,
        cache_size: 1000,
        num_in_use: 2,
        lowest_total: 7,
        lowest_total_index: 1,
    };
    let expected: [u8; 22] = [
        0x73, 0x74, 0x63, 0x18, // magic "stc" 24
        0x01, 0x90, // version 400
        0x00, 0x00, 0x03, 0xE8, // CacheSize 1000
        0x00, 0x00, 0x00, 0x02, // NumInUse
        0x00, 0x00, 0x00, 0x07, // LowestTotal
        0x00, 0x00, 0x00, 0x01, // LowestTotalIndex
    ];
    assert_eq!(h.to_bytes(), expected);
    assert_eq!(StcHeader::from_bytes(&expected).unwrap(), h);
    assert_eq!(HEADER_LEN, 22);
    assert_eq!(NODE_LEN, 71);
    assert_eq!(MAGIC, [0x73, 0x74, 0x63, 0x18]);
    assert_eq!(FILE_VERSION, 400);
    assert_eq!(header(1000, 2, 7, 1), expected);
}

#[test]
fn header_accepts_any_version_and_checks_only_the_magic() {
    let mut bytes = header(10, 0, 0, 0);
    bytes[4] = 0x12;
    bytes[5] = 0x34;
    assert_eq!(StcHeader::from_bytes(&bytes).unwrap().version, 0x1234);
}

#[rstest]
#[case::first_byte(0)]
#[case::second_byte(1)]
#[case::third_byte(2)]
#[case::fourth_byte(3)]
fn header_with_a_wrong_magic_byte_is_corrupt(#[case] at: usize) {
    let mut bytes = header(10, 0, 0, 0);
    bytes[at] ^= 0x01;
    corrupt(StcHeader::from_bytes(&bytes));
}

#[test]
fn header_shorter_than_22_bytes_is_corrupt() {
    let bytes = header(10, 0, 0, 0);
    for n in 0..22 {
        corrupt(StcHeader::from_bytes(&bytes[..n]));
    }
}

// ------------------------------------------------------------ node: golden

fn extras() -> NodeExtras {
    NodeExtras {
        piece_num: 1,
        moving_piece: 2,
        captured_num: 3,
        captured_piece: 4,
        captured_square: 5,
        castle_flags: 6,
        ep_square: 7,
        old_half_move_clock: 0x0102,
        san: "e4".to_owned(),
        score: 0x0A0B_0C0D,
        eco_code: 0x0E0F,
    }
}

fn e2e4(freq_tree_order: [u32; 4]) -> TreeNode {
    TreeNode {
        mv: Some(RawMove {
            from: Square::E2,
            to: Square::E4,
            promotion: None,
        }),
        total: 100,
        freq: freq_tree_order,
        elo_sum: 7000,
        elo_count: 3,
        perf_sum: 5000,
        perf_count: 2,
        year_sum: 8000,
        year_count: 4,
    }
}

#[test]
fn node_golden_bytes_follow_the_layout_of_d1043() {
    // TreeNode order is [white, black, draw, none]; the file order is
    // [none, white, black, draw] (D1042).
    let n = e2e4([10, 20, 30, 40]);
    let mut out = Vec::new();
    encode_node(&n, &extras(), &mut out);
    #[rustfmt::skip]
    let expected: Vec<u8> = vec![
        0x01,             // pieceNum
        0x02,             // movingPiece
        0x0C,             // from e2 = 12
        0x1C,             // to e4 = 28
        0x03,             // capturedNum
        0x04,             // capturedPiece
        0x07,             // promote: none
        0x00,             // reserved
        0x05,             // capturedSquare
        0x06,             // castleFlags
        0x07,             // epSquare
        0x01, 0x02,       // oldHalfMoveClock
        b'e', b'4', 0, 0, 0, 0, 0, 0, // san, zero padded to 8
        0, 0, 0, 40,      // freq[0] no result
        0, 0, 0, 10,      // freq[1] white
        0, 0, 0, 20,      // freq[2] black
        0, 0, 0, 30,      // freq[3] draw
        0, 0, 0, 100,     // total
        0x0A, 0x0B, 0x0C, 0x0D, // score
        0x0E, 0x0F,       // ecoCode
        0, 0, 0, 3,       // eloCount
        0, 0, 0x1B, 0x58, // eloSum 7000
        0, 0, 0, 2,       // perfCount
        0, 0, 0x13, 0x88, // perfSum 5000
        0, 0, 0, 4,       // yearCount
        0, 0, 0x1F, 0x40, // yearSum 8000
    ];
    assert_eq!(expected.len(), 71);
    assert_eq!(out, expected);
    assert_eq!(decode_node(&expected).unwrap(), n);
}

#[test]
fn end_of_game_node_uses_from_65_and_empty_san_is_all_zeros() {
    let n = TreeNode {
        mv: None,
        total: 1,
        freq: [0, 0, 1, 0],
        elo_sum: 0,
        elo_count: 0,
        perf_sum: 0,
        perf_count: 0,
        year_sum: 0,
        year_count: 0,
    };
    let ex = NodeExtras {
        piece_num: 0,
        moving_piece: 0,
        captured_num: 0,
        captured_piece: 7,
        captured_square: 0,
        castle_flags: 0,
        ep_square: 65,
        old_half_move_clock: 0,
        san: String::new(),
        score: 500,
        eco_code: 0,
    };
    let mut out = Vec::new();
    encode_node(&n, &ex, &mut out);
    assert_eq!(out.len(), 71);
    assert_eq!(&out[..13], &[0, 0, 65, 65, 0, 7, 7, 0, 0, 0, 65, 0, 0]);
    assert_eq!(&out[13..21], &[0u8; 8], "san is all zeros");
    // draw is file slot 3
    assert_eq!(
        &out[21..37],
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]
    );
    assert_eq!(&out[37..41], &[0, 0, 0, 1], "total");
    assert_eq!(&out[41..45], &[0, 0, 1, 0xF4], "score 500");
    assert_eq!(decode_node(&out).unwrap(), n);
}

#[rstest]
#[case::queen(Role::Queen, 2)]
#[case::rook(Role::Rook, 3)]
#[case::bishop(Role::Bishop, 4)]
#[case::knight(Role::Knight, 5)]
fn promotion_codes_are_the_piece_numbers_of_the_scid(#[case] role: Role, #[case] code: u8) {
    let n = TreeNode {
        mv: Some(RawMove {
            from: Square::A7,
            to: Square::A8,
            promotion: Some(role),
        }),
        ..e2e4([1, 0, 0, 0])
    };
    let mut out = Vec::new();
    encode_node(&n, &extras(), &mut out);
    assert_eq!(&out[2..4], &[48, 56], "a7 = 48, a8 = 56");
    assert_eq!(out[6], code);
    assert_eq!(decode_node(&out).unwrap(), n);
}

#[test]
fn year_and_other_sums_are_truncated_to_32_bits_on_write() {
    let n = TreeNode {
        year_sum: 0x1_0000_0005,
        elo_sum: 0x2_0000_0006,
        perf_sum: 0x3_0000_0007,
        ..e2e4([1, 0, 0, 0])
    };
    let mut out = Vec::new();
    encode_node(&n, &extras(), &mut out);
    assert_eq!(&out[51..55], &[0, 0, 0, 6]);
    assert_eq!(&out[59..63], &[0, 0, 0, 7]);
    assert_eq!(&out[67..71], &[0, 0, 0, 5]);
    let back = decode_node(&out).unwrap();
    assert_eq!((back.elo_sum, back.perf_sum, back.year_sum), (6, 7, 5));
}

#[test]
fn a_long_san_cannot_overflow_the_8_byte_field() {
    let mut ex = extras();
    ex.san = "abcdefghijkl".to_owned();
    let mut out = Vec::new();
    encode_node(&e2e4([1, 0, 0, 0]), &ex, &mut out);
    assert_eq!(out.len(), 71);
}

// ------------------------------------------------------------ node: reading

#[test]
fn file_only_fields_are_ignored_when_reading_a_node() {
    let mut garbage = node_full(12, 28, [1, 2, 3, 4], 10, [1, 2, 3, 4, 5, 6]);
    let clean = decode_node(&garbage).unwrap();
    // pieceNum, movingPiece, capturedNum, capturedPiece, reserved,
    // capturedSquare, castleFlags, epSquare, clock, san, score, ecoCode.
    for i in [0, 1, 4, 5, 7, 8, 9, 10, 11, 12] {
        garbage[i] = 0xEE;
    }
    for b in &mut garbage[13..21] {
        *b = b'Z';
    }
    for b in &mut garbage[41..47] {
        *b = 0xFF;
    }
    assert_eq!(decode_node(&garbage).unwrap(), clean);
}

#[test]
fn reading_maps_the_file_result_order_to_the_tree_order() {
    // File: [no result 1, white 2, black 3, draw 4].
    let bytes = node(12, 28, 7, [1, 2, 3, 4], 10);
    assert_eq!(decode_node(&bytes).unwrap().freq, [2, 3, 4, 1]);
}

#[test]
fn reading_keeps_the_counters_and_the_move() {
    let bytes = node_full(12, 28, [0, 5, 0, 0], 5, [3, 7000, 2, 5000, 4, 8000]);
    let n = decode_node(&bytes).unwrap();
    assert_eq!(n, e2e4_counts());
}

fn e2e4_counts() -> TreeNode {
    TreeNode {
        total: 5,
        freq: [5, 0, 0, 0],
        ..e2e4([0; 4])
    }
}

#[test]
fn from_65_is_end_of_game_and_to_and_promote_are_ignored() {
    let mut bytes = node(65, 200, 99, [0, 1, 0, 0], 1);
    bytes[3] = 200;
    bytes[6] = 99;
    let n = decode_node(&bytes).unwrap();
    assert_eq!(n.mv, None);
    assert_eq!(n.freq, [1, 0, 0, 0]);
}

#[rstest]
#[case::from_64(64, 28, 7)]
#[case::from_66(66, 28, 7)]
#[case::from_255(255, 28, 7)]
#[case::to_64(12, 64, 7)]
#[case::to_65(12, 65, 7)]
#[case::promote_0(48, 56, 0)]
#[case::promote_1(48, 56, 1)]
#[case::promote_6(48, 56, 6)]
#[case::promote_8(48, 56, 8)]
#[case::promote_255(48, 56, 255)]
fn invalid_node_fields_are_corrupt(#[case] from: u8, #[case] to: u8, #[case] promote: u8) {
    corrupt(decode_node(&node(from, to, promote, [0; 4], 0)));
}

#[test]
fn promotion_codes_2_to_5_and_7_are_valid() {
    for p in [2u8, 3, 4, 5, 7] {
        assert!(decode_node(&node(48, 56, p, [0; 4], 0)).is_ok(), "{p}");
    }
}

#[test]
fn node_of_the_wrong_length_is_corrupt() {
    let bytes = node(12, 28, 7, [0; 4], 0);
    corrupt(decode_node(&bytes[..70]));
    let mut longer = bytes.clone();
    longer.push(0);
    corrupt(decode_node(&longer));
    corrupt(decode_node(&[]));
}

// ------------------------------------------------------------ whole file

#[test]
fn a_written_file_has_the_layout_of_d1043_byte_by_byte() {
    let (_dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    let mut tree = tree_of(5, 7);
    tree.filter.set(3, 4);
    cache.store(&Position::new(), tree.clone());
    let size = cache.write(None).unwrap();
    let bytes = std::fs::read(stc_file(&base)).unwrap();
    assert_eq!(size as usize, bytes.len());
    // header: version 400, CacheSize 1000, one entry, lowest (7, 0)
    assert_eq!(&bytes[..22], &header(1000, 1, 7, 0)[..]);
    // entry head
    assert_eq!(bytes[22], 0, "white to move");
    assert_eq!(&bytes[23..87], &start_board()[..]);
    assert_eq!(&bytes[87..91], &[0, 0, 0, 1], "moveCount");
    assert_eq!(&bytes[91..95], &[0, 0, 0, 7], "totalCount");
    // the node
    let n = &bytes[95..166];
    assert_eq!((n[2], n[3]), (12, 28), "e2e4");
    assert_eq!(n[5], 7, "no captured piece");
    assert_eq!(n[6], 7, "no promotion");
    assert_eq!(n[7], 0, "reserved");
    assert_eq!(n[10], 65, "no en passant square");
    assert_eq!(&n[11..13], &[0, 0], "oldHalfMoveClock");
    assert_eq!(&n[13..21], &[b'e', b'4', 0, 0, 0, 0, 0, 0], "san");
    // White wins: file slot 1
    assert_eq!(
        &n[21..37],
        &[0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(&n[37..41], &[0, 0, 0, 7], "total");
    // the filter block
    assert_eq!(&bytes[166..170], &[0, 0, 0, 5], "CFilterSize");
    assert_eq!(&bytes[170..174], &[0, 0, 0, 2], "CFilterCount");
    let len = u32::from_be_bytes(bytes[174..178].try_into().unwrap()) as usize;
    assert_eq!(bytes.len(), 178 + len, "CompressedLength covers the rest");
    let raw = unpack_bytemap(&bytes[178..], 5).unwrap();
    assert_eq!(raw, tree.filter.as_bytes());
    assert_eq!(&bytes[178..], &pack_bytemap(tree.filter.as_bytes())[..]);
}

#[test]
fn black_to_move_and_black_pieces_use_the_codes_of_d1043() {
    let (_dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    let pos = Position::from_fen("4k3/8/8/8/8/8/8/4K3 b - - 0 1").unwrap();
    cache.store(&pos, tree_of(1, 1));
    cache.write(None).unwrap();
    let bytes = std::fs::read(stc_file(&base)).unwrap();
    assert_eq!(bytes[22], 1, "black to move");
    let board = &bytes[23..87];
    assert_eq!(board[4], 1, "white king on e1");
    assert_eq!(board[60], 9, "black king on e8");
    assert_eq!(board.iter().filter(|b| **b == 7).count(), 62);
}

#[test]
fn an_empty_cache_is_written_as_a_header_only_file() {
    let (_dir, base) = tmp_base();
    let size = StcCache::new(Some(&base)).write(None).unwrap();
    assert_eq!(size, 22);
    assert_eq!(
        std::fs::read(stc_file(&base)).unwrap(),
        header(1000, 0, 0, 0)
    );
}

// ------------------------------------------------------------ round trip

fn knights() -> Vec<Position> {
    stc::positions(6)
}

#[test]
fn write_then_read_preserves_boards_sides_counters_filters_and_lowest() {
    let (_dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    let ps = knights();
    let totals = [50u32, 10, 30, 70];
    let mut trees = Vec::new();
    for (i, t) in totals.iter().enumerate() {
        let mut tree = tree_of(9, *t);
        tree.nodes[0].elo_sum = 2400 * u64::from(*t);
        tree.nodes[0].elo_count = *t;
        tree.nodes[0].year_sum = 2000;
        tree.nodes[0].year_count = 1;
        tree.nodes[0].freq = [1, 2, 3, *t - 6];
        tree.filter.set(u32::try_from(i).unwrap() + 2, 9);
        cache.store(&ps[i], tree.clone());
        trees.push(tree);
    }
    // And a black-to-move position with the same board as ps[0].
    let black = Position::from_fen(&ps[0].fen().replace(" w ", " b ")).unwrap();
    cache.store(&black, tree_of(9, 200));
    assert_eq!(cache.main_lowest(), Some((10, 1)));
    cache.write(None).unwrap();

    let mut fresh = StcCache::new(Some(&base));
    for (p, t) in ps.iter().zip(&trees) {
        assert_eq!(fresh.lookup(p, 9).as_ref(), Some(t), "{}", p.fen());
    }
    assert_eq!(fresh.lookup(&black, 9), Some(tree_of(9, 200)));
    assert_eq!(fresh.info(), (5, 1000));
    assert_eq!(fresh.main_lowest(), Some((10, 1)));
    assert!(fresh.take_load_error().is_none());
}

#[test]
fn a_tree_with_the_end_of_game_node_and_a_promotion_survives_the_round_trip() {
    let (_dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    let mut tree = tree_of(4, 3);
    tree.nodes.push(TreeNode {
        mv: None,
        total: 2,
        freq: [0, 0, 2, 0],
        elo_sum: 0,
        elo_count: 0,
        perf_sum: 0,
        perf_count: 0,
        year_sum: 0,
        year_count: 0,
    });
    tree.nodes.push(TreeNode {
        mv: Some(RawMove {
            from: Square::A7,
            to: Square::A8,
            promotion: Some(Role::Knight),
        }),
        total: 1,
        freq: [0, 1, 0, 0],
        elo_sum: 0,
        elo_count: 0,
        perf_sum: 0,
        perf_count: 0,
        year_sum: 0,
        year_count: 0,
    });
    let pos = Position::new();
    cache.store(&pos, tree.clone());
    cache.write(None).unwrap();
    let mut fresh = StcCache::new(Some(&base));
    assert_eq!(fresh.lookup(&pos, 4), Some(tree));
}

#[test]
fn a_filter_of_zero_games_round_trips() {
    let (_dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    let tree = CachedTree {
        nodes: Vec::new(),
        filter: Filter::new(0),
    };
    cache.store(&Position::new(), tree.clone());
    cache.write(None).unwrap();
    let mut fresh = StcCache::new(Some(&base));
    assert_eq!(fresh.lookup(&Position::new(), 0), Some(tree));
}

#[test]
fn a_big_filter_with_long_runs_round_trips() {
    let (_dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    let mut tree = tree_of(5000, 1);
    tree.filter.fill(0);
    tree.filter.set(1, 3);
    for g in 1000..1400 {
        tree.filter.set(g, 5);
    }
    tree.filter.set(4999, 255);
    cache.store(&Position::new(), tree.clone());
    cache.write(None).unwrap();
    assert!(std::fs::metadata(stc_file(&base)).unwrap().len() < 400);
    let mut fresh = StcCache::new(Some(&base));
    assert_eq!(fresh.lookup(&Position::new(), 5000), Some(tree));
}

// ------------------------------------------------------------ reading files built by hand

fn read(bytes: &[u8]) -> (StcCache, Result<(), StcError>) {
    let (dir, base) = tmp_base();
    write_stc(&base, bytes);
    let mut cache = StcCache::new(Some(&base));
    let r = cache.read();
    drop(dir);
    (cache, r)
}

#[test]
fn a_hand_built_file_is_read_with_the_file_order_of_results() {
    let (_dir, base) = tmp_base();
    let e = entry(
        0,
        &start_board(),
        1,
        10,
        &[node_full(
            12,
            28,
            [1, 2, 3, 4],
            10,
            [3, 7000, 2, 5000, 4, 8000],
        )],
        (3, 1, &[0, 2, 0, 0]),
    );
    write_stc(&base, &file(header(1000, 1, 10, 0), &[e]));
    let mut cache = StcCache::new(Some(&base));
    let hit = cache.lookup(&Position::new(), 3).unwrap();
    assert_eq!(
        hit.nodes,
        vec![TreeNode {
            total: 10,
            freq: [2, 3, 4, 1],
            ..e2e4([0; 4])
        }]
    );
    assert_eq!(hit.filter.as_bytes(), &[2, 0, 0]);
}

#[test]
fn any_version_number_is_accepted_and_trailing_bytes_are_ignored() {
    let mut h = header(1000, 1, 5, 0);
    h[4] = 0;
    h[5] = 1;
    let mut bytes = file(h, &[simple_entry(3, 5)]);
    bytes.extend_from_slice(b"trailing garbage");
    let (cache, r) = read(&bytes);
    r.unwrap();
    assert_eq!(cache.info(), (1, 1000));
}

#[test]
fn the_cache_size_of_the_file_replaces_the_configured_one() {
    let (cache, r) = read(&file(header(7, 1, 5, 0), &[simple_entry(3, 5)]));
    r.unwrap();
    assert_eq!(cache.info(), (1, 7));
}

#[test]
fn the_cache_size_limits_are_1_to_100000() {
    let (cache, r) = read(&header(100_000, 0, 0, 0));
    r.unwrap();
    assert_eq!(cache.info().1, 100_000);
    let (cache, r) = read(&header(1, 0, 0, 0));
    r.unwrap();
    assert_eq!(cache.info().1, 1);
    for size in [0u32, 100_001, u32::MAX] {
        let (cache, r) = read(&header(size, 0, 0, 0));
        corrupt(r);
        assert_eq!(cache.info(), (0, 1000), "size {size}");
    }
}

/// Two entries of different keys (white and black to move) with the given
/// totals.
fn two_entries(first: u32, second: u32) -> [Vec<u8>; 2] {
    let a = simple_entry(3, first);
    let mut b = simple_entry(3, second);
    b[0] = 1; // black to move: another key
    [a, b]
}

#[rstest]
#[case::minimum_first(5, 9, 5, 0)]
#[case::minimum_second(9, 5, 5, 1)]
fn the_lowest_total_of_the_file_is_kept_when_it_is_coherent(
    #[case] first: u32,
    #[case] second: u32,
    #[case] lowest: u32,
    #[case] index: u32,
) {
    // D1057 / D1065 (a): the index points to an entry with that total and it
    // is the minimum.
    let entries = two_entries(first, second);
    let (cache, r) = read(&file(header(100, 2, lowest, index), &entries));
    r.unwrap();
    assert_eq!(cache.main_lowest(), Some((lowest, index as usize)));
}

#[rstest]
#[case::total_not_the_one_of_the_entry(7, 1)] // totals 5 and 9: 7 is nobody's
#[case::index_out_of_range(5, 99)]
#[case::index_u32_max(5, u32::MAX)]
#[case::total_diverges_from_the_entry(5, 1)] // entry 1 has 9
#[case::entry_is_not_the_minimum(9, 1)] // entry 1 has 9, but 5 is lower
fn an_incoherent_lowest_total_of_the_file_is_recomputed(#[case] lowest: u32, #[case] index: u32) {
    // D1065 (a): the file declares totals 5 and 9; the answer is (5, 0).
    let entries = two_entries(5, 9);
    let (cache, r) = read(&file(header(100, 2, lowest, index), &entries));
    r.unwrap();
    assert_eq!(cache.main_lowest(), Some((5, 0)));
}

#[test]
fn san_score_and_eco_code_stored_in_the_file_do_not_reach_the_tree() {
    let mut n = node(12, 28, 7, [0, 5, 0, 0], 5);
    n[13..21].copy_from_slice(b"garbage!");
    n[41..45].copy_from_slice(&[0xFF; 4]);
    n[45..47].copy_from_slice(&[0xFF; 2]);
    n[0] = 99;
    n[1] = 98;
    let e = entry(0, &start_board(), 1, 5, &[n], (2, 1, &[0, 2, 0]));
    // [0, 2, 0] is a copied filter of 2 games.
    let (mut cache, r) = read(&file(header(100, 1, 5, 0), &[e]));
    r.unwrap();
    let hit = cache.lookup(&Position::new(), 2).unwrap();
    assert_eq!(hit.nodes[0].freq, [5, 0, 0, 0]);
    assert_eq!(hit.nodes[0].total, 5);
}

#[test]
fn an_entry_with_zero_nodes_is_valid() {
    let e = entry(1, &start_board(), 0, 0, &[], (2, 0, &[0, 0, 0]));
    let (cache, r) = read(&file(header(100, 1, 0, 0), &[e]));
    r.unwrap();
    assert_eq!(cache.info(), (1, 100));
}
