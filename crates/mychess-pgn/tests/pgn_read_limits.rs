//! Resource limits and hostile input of the PGN reader: node and tag limits,
//! truncation at every byte, gigantic constructs, UTF-16, cp1251 files.

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

use std::time::{Duration, Instant};

use common::*;
use mychess_pgn::read::{DiagnosticKind as K, PgnError, PgnReader, ReadOptions, Termination};

fn count_kind(g: &mychess_pgn::read::ParsedGame, k: &K) -> usize {
    g.diagnostics.iter().filter(|d| &d.kind == k).count()
}

#[test]
fn three_hundred_thousand_distinct_tags_finish_fast_and_are_capped() {
    let mut s = String::from("[Event \"x\"]\n");
    for i in 0..300_000 {
        s.push_str(&format!("[T{i} \"v\"]\n"));
    }
    s.push_str("\n1. e4 *\n");
    let t = Instant::now();
    let g = one(&s);
    assert!(
        t.elapsed() < Duration::from_secs(10),
        "took {:?}",
        t.elapsed()
    );
    assert_eq!(
        g.game.tags().extras().len(),
        256,
        "default max_tags_per_game"
    );
    assert_eq!(count_kind(&g, &K::TooManyTags), 1);
    assert_eq!(g.game.mainline().count(), 1);
}

#[test]
fn max_tags_per_game_is_configurable() {
    let o = ReadOptions::default().with_max_tags_per_game(3);
    let g = one_with(
        "[A \"1\"]\n[B \"2\"]\n[C \"3\"]\n[D \"4\"]\n[E \"5\"]\n\n*\n",
        &o,
    );
    assert_eq!(g.game.tags().extras().len(), 3);
    assert_eq!(count_kind(&g, &K::TooManyTags), 1);
}

#[test]
fn node_limit_stops_the_moves_with_a_diagnostic_and_termination() {
    let mut s = String::from("[Event \"x\"]\n\n");
    for i in 0..500 {
        s.push_str(&format!("{}. Nf3 Nf6 {}. Ng1 Ng8 ", 2 * i + 1, 2 * i + 2));
    }
    s.push_str("1-0\n\n[Event \"next\"]\n\n1. e4 *\n");
    let o = ReadOptions::default().with_max_nodes_per_game(100);
    let games = read_bytes(s.as_bytes(), &o);
    assert_eq!(games.len(), 2, "the next game is still found");
    assert_eq!(games[0].termination, Termination::NodeLimit);
    assert!(has(&games[0], |k| *k == K::TooManyNodes));
    assert!(
        games[0].game.node_count() <= 101,
        "{}",
        games[0].game.node_count()
    );
    assert!(games[0].game.mainline().count() >= 99);
    assert_eq!(games[1].game.tags().event, "next");
    assert_eq!(games[1].game.mainline().count(), 1);
}

#[test]
fn sixty_four_megabytes_of_moves_are_bounded_by_the_node_limit() {
    // ~64 MB of distinct-looking moves in one game; the limit keeps memory and time bounded.
    let unit = "Nf3 Nf6 Ng1 Ng8 ";
    let mut s = String::from("[Event \"x\"]\n\n");
    s.push_str(&unit.repeat(64 * 1024 * 1024 / unit.len()));
    s.push_str("*\n");
    let o = ReadOptions::default()
        .with_max_game_bytes(128 << 20)
        .with_max_nodes_per_game(50_000);
    let t = Instant::now();
    let g = one_with(&s, &o);
    assert_eq!(g.termination, Termination::NodeLimit);
    assert!(g.game.node_count() <= 50_001);
    assert!(
        t.elapsed() < Duration::from_secs(60),
        "took {:?}",
        t.elapsed()
    );
}

#[test]
fn game_over_max_game_bytes_is_oversize_with_the_new_default_of_16_mib() {
    assert_eq!(ReadOptions::default().max_game_bytes, 16 << 20);
    let mut s = String::from("[Event \"big\"]\n\n1. e4 {");
    s.push_str(&"x".repeat(17 << 20));
    s.push_str("} *\n\n[Event \"ok\"]\n\n1. d4 *\n");
    let games = read_bytes(s.as_bytes(), &ReadOptions::default());
    assert_eq!(games.len(), 2);
    assert_eq!(games[0].termination, Termination::Oversize);
    assert_eq!(games[1].game.tags().event, "ok");
}

#[test]
fn one_million_open_parentheses_are_survivable_in_a_small_stack() {
    let s = format!(
        "[Event \"x\"]\n\n1. e4 e5 {} 2. Nf3 *\n",
        "(".repeat(1_000_000)
    );
    let t = Instant::now();
    let g = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || one(&s))
        .unwrap()
        .join()
        .unwrap();
    assert!(t.elapsed() < Duration::from_secs(20));
    assert!(g.diagnostics.len() <= 101);
}

#[test]
fn huge_comment_and_huge_line_are_bounded() {
    let big = "a".repeat(40 << 20);
    let s = format!("[Event \"x\"]\n\n1. e4 {{{big}}} e5 *\n");
    let o = ReadOptions::default().with_max_game_bytes(64 << 20);
    let t = Instant::now();
    let g = one_with(&s, &o);
    assert!(
        g.game
            .comment(g.game.mainline().next().unwrap())
            .unwrap()
            .unwrap()
            .len()
            <= 1 << 20
    );
    assert!(has(&g, |k| *k == K::CommentTruncated));
    assert!(t.elapsed() < Duration::from_secs(30));
    let line = format!("[Event \"{}\"]\n\n*\n", "z".repeat(30 << 20));
    let g = one_with(&line, &o);
    assert!(g.game.tags().event.len() <= 64 * 1024);
}

#[test]
fn truncating_a_valid_file_at_every_byte_never_panics_and_keeps_complete_games() {
    let full = "\u{feff}[Event \"A\"]\n[White \"M\u{fc}ller (2500)\"]\n\n1. e4 {c\nd} e5 (1... c5 $1) 2. Nf3 1-0\n\n[Event \"B\"]\n[FEN \"8/8/8/4k3/8/8/4K3/4R3 w - - 0 1\"]\n\n1. Rd1 ; x\n Kf5 *\n\n[Event \"C\"]\n";
    let bytes = full.as_bytes();
    let reference = read_bytes(bytes, &ReadOptions::default());
    for cut in 0..=bytes.len() {
        let games = read_bytes(&bytes[..cut], &ReadOptions::default());
        assert!(games.len() <= reference.len());
        for (i, g) in games.iter().enumerate() {
            assert_eq!(g.number, i as u64 + 1);
            assert!(g.byte_range.end <= cut as u64);
        }
    }
}

#[test]
fn utf16_bom_is_an_unsupported_encoding_error() {
    for bom in [[0xFFu8, 0xFE], [0xFE, 0xFF]] {
        let mut data = bom.to_vec();
        for c in "[Event \"x\"]\n\n*\n".encode_utf16() {
            data.extend_from_slice(&if bom[0] == 0xFF {
                c.to_le_bytes()
            } else {
                c.to_be_bytes()
            });
        }
        let mut r = PgnReader::new(&data[..], ReadOptions::default());
        match r.next() {
            Some(Err(PgnError::UnsupportedEncoding { encoding })) => {
                assert!(encoding.contains("UTF-16"), "{encoding}")
            }
            other => panic!(
                "expected UnsupportedEncoding, got {:?}",
                other.map(|r| r.map(|g| g.number))
            ),
        }
        assert!(r.next().is_none());
    }
}

#[test]
fn windows_1251_file_decodes_every_game_correctly() {
    fn cp1251(s: &str) -> Vec<u8> {
        s.chars()
            .map(|c| match c {
                '\u{410}'..='\u{44f}' => (c as u32 - 0x410 + 0xC0) as u8,
                c => c as u8,
            })
            .collect()
    }
    let names = [
        "\u{41a}\u{430}\u{441}\u{43f}\u{430}\u{440}\u{43e}\u{432}, \u{413}\u{430}\u{440}\u{440}\u{438}",
        "\u{41a}\u{430}\u{440}\u{43f}\u{43e}\u{432}, \u{410}\u{43d}\u{430}\u{442}\u{43e}\u{43b}\u{438}\u{439}",
        "\u{422}\u{430}\u{43b}\u{44c}, \u{41c}\u{438}\u{445}\u{430}\u{438}\u{43b}",
    ];
    let mut b = Vec::new();
    for i in 0..30 {
        let n = names[i % 3];
        b.extend_from_slice(b"[White \"");
        b.extend(cp1251(n));
        b.extend_from_slice(b"\"]\n[Black \"");
        b.extend(cp1251(names[(i + 1) % 3]));
        b.extend_from_slice(b"\"]\n\n1. e4 {");
        b.extend(cp1251(
            "\u{445}\u{43e}\u{440}\u{43e}\u{448}\u{438}\u{439} \u{445}\u{43e}\u{434}",
        ));
        b.extend_from_slice(b"} e5 *\n\n");
    }
    let games = read_bytes(&b, &ReadOptions::default());
    assert_eq!(games.len(), 30);
    for (i, g) in games.iter().enumerate() {
        assert_eq!(g.game.tags().white, names[i % 3], "game {i}");
        assert_eq!(
            g.game.comment(g.game.mainline().next().unwrap()).unwrap(),
            Some("\u{445}\u{43e}\u{440}\u{43e}\u{448}\u{438}\u{439} \u{445}\u{43e}\u{434}"),
            "game {i}"
        );
    }
}

#[test]
fn skipped_bytes_counts_text_outside_games() {
    let mut r = PgnReader::new(
        &b"hello world\n[Event \"x\"]\n\n*\ntrailing text\n"[..],
        ReadOptions::default(),
    );
    while r.next().is_some() {}
    assert!(r.skipped_bytes() >= "hello world".len() as u64);
    let mut r = PgnReader::new(&b"no headers here\n"[..], ReadOptions::default());
    assert!(r.next().is_none());
    assert!(r.skipped_bytes() >= 15);
}

#[test]
fn partial_game_is_emitted_before_a_gzip_error() {
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;
    let mut enc = GzEncoder::new(Vec::new(), Compression::none());
    enc.write_all(b"[Event \"A\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 6. Re1 b5 7. Bb3 d6 8. c3 O-O 9. h3 Nb8 10. d4 Nbd7 11. c4 c6 12. cxb5 axb5").unwrap();
    let full = enc.finish().unwrap();
    let cut = &full[..full.len() - 12];
    let items: Vec<_> = PgnReader::new(cut, ReadOptions::default()).collect();
    assert!(
        matches!(
            items.last(),
            Some(Err(PgnError::Gzip { .. } | PgnError::Io { .. }))
        ),
        "{:?}",
        items.len()
    );
    if let Some(Ok(g)) = items.first() {
        assert!(
            g.game.mainline().count() > 5,
            "the partial game keeps what was read"
        );
    } else {
        panic!("a partial game should precede the error");
    }
}
