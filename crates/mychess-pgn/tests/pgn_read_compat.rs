//! Compatibility tests that read data shipped with the Scid vs PC reference
//! tree (never copied into the repository). Run with `cargo xtask compat`.
//!
//! The reference tree has no PGN files (docs/referencia-scid.md), so the
//! corpus is `scid.eco`/`scid_fr.eco` converted to PGN in memory: every
//! opening line becomes a game. Spec: `docs/specs/m1-3-pgn.md`, "Fixtures".

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

use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use common::*;
use flate2::{Compression, write::GzEncoder};
use mychess_pgn::read::{PgnReader, ReadOptions, Severity, Termination};

fn scid_reference() -> PathBuf {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/scid_vs_pc-4.27");
    assert!(
        dir.is_dir(),
        "Scid vs PC reference not found at {} (see docs/referencia-scid.md)",
        dir.display()
    );
    dir
}

struct Line {
    code: String,
    name: String,
    moves: String,
}

/// Parses the `.eco` file format: `CODE "name" moves... *`, continued on the next lines.
fn eco_lines(bytes: &[u8]) -> Vec<Line> {
    let text = String::from_utf8_lossy(bytes);
    let mut out: Vec<Line> = Vec::new();
    let mut open: Option<Line> = None;
    for raw in text.lines() {
        let l = raw.trim_end();
        if l.starts_with('#') || l.trim().is_empty() {
            continue;
        }
        let starts_entry = open.is_none()
            && l.as_bytes()
                .first()
                .is_some_and(|c| (b'A'..=b'E').contains(c))
            && l.as_bytes().get(1).is_some_and(u8::is_ascii_digit);
        if starts_entry {
            let (code, rest) = l.split_once(' ').unwrap();
            let rest = rest.trim_start();
            let rest = rest.strip_prefix('"').unwrap();
            let (name, moves) = rest.split_once('"').unwrap();
            open = Some(Line {
                code: code.to_owned(),
                name: name.to_owned(),
                moves: moves.trim().to_owned(),
            });
        } else if let Some(o) = open.as_mut() {
            o.moves.push(' ');
            o.moves.push_str(l.trim());
        }
        if open.as_ref().is_some_and(|o| o.moves.ends_with('*')) {
            out.push(open.take().unwrap());
        }
    }
    out
}

fn to_pgn(lines: &[Line]) -> String {
    let mut s = String::new();
    for l in lines {
        s.push_str(&format!(
            "[Event \"{}\"]\n[ECO \"{}\"]\n\n{}\n\n",
            l.name.replace('\\', "\\\\").replace('"', "\\\""),
            l.code,
            l.moves
        ));
    }
    s
}

fn plies(moves: &str) -> usize {
    moves
        .split_whitespace()
        .map(|w| w.rsplit('.').next().unwrap_or(w))
        .filter(|w| !w.is_empty() && *w != "*")
        .count()
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_reads_every_opening_line_of_scid_eco_as_a_game() {
    let bytes = std::fs::read(scid_reference().join("scid.eco")).unwrap();
    let lines = eco_lines(&bytes);
    assert!(lines.len() > 10_000, "parsed only {} entries", lines.len());
    let pgn = to_pgn(&lines);
    let t = Instant::now();
    let games = read_bytes(pgn.as_bytes(), &ReadOptions::default());
    let secs = t.elapsed().as_secs_f64();
    eprintln!(
        "scid.eco: {} games, {:.1} MB in {secs:.2}s ({:.1} MB/s)",
        games.len(),
        pgn.len() as f64 / 1e6,
        pgn.len() as f64 / 1e6 / secs
    );
    assert_eq!(games.len(), lines.len());
    for (i, (g, l)) in games.iter().zip(&lines).enumerate() {
        assert_eq!(g.number, i as u64 + 1);
        let bad: Vec<_> = g
            .diagnostics
            .iter()
            .filter(|d| d.severity > Severity::Info)
            .collect();
        assert!(
            bad.is_empty(),
            "{} {:?} ({}): {bad:?}",
            l.code,
            l.name,
            l.moves
        );
        assert_eq!(g.termination, Termination::ResultToken, "{}", l.moves);
        assert_eq!(
            g.game.mainline().count(),
            plies(&l.moves),
            "{} {:?}: {}",
            l.code,
            l.name,
            l.moves
        );
        assert_eq!(
            g.game.tags().eco.map(|e| e.to_string()),
            Some(l.code.clone()),
            "{}",
            l.name
        );
        assert_eq!(g.game.tags().event, l.name);
    }
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_scid_eco_as_pgn_is_identical_when_gzipped_and_when_crlf() {
    let bytes = std::fs::read(scid_reference().join("scid.eco")).unwrap();
    let pgn = to_pgn(&eco_lines(&bytes));
    let plain = read_bytes(pgn.as_bytes(), &ReadOptions::default());
    let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
    enc.write_all(pgn.as_bytes()).unwrap();
    let gz = enc.finish().unwrap();
    let zipped = read_bytes(&gz, &ReadOptions::default());
    let crlf = read_bytes(
        pgn.replace('\n', "\r\n").as_bytes(),
        &ReadOptions::default(),
    );
    assert_eq!(plain.len(), zipped.len());
    assert_eq!(plain.len(), crlf.len());
    for ((a, b), c) in plain.iter().zip(&zipped).zip(&crlf) {
        assert!(a.game.same_content(&b.game));
        assert!(a.game.same_content(&c.game));
        assert_eq!(a.byte_range, b.byte_range);
    }
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_scid_fr_eco_is_latin1_and_reads_without_panic() {
    let bytes = std::fs::read(scid_reference().join("scid_fr.eco")).unwrap();
    assert!(
        std::str::from_utf8(&bytes).is_err(),
        "scid_fr.eco is not UTF-8"
    );
    // Build the PGN byte-wise so the Latin-1 names reach the reader undecoded.
    let mut pgn: Vec<u8> = Vec::new();
    let mut open = false;
    let mut count = 0usize;
    for raw in bytes.split(|&b| b == b'\n') {
        if raw.starts_with(b"#") || raw.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if !open && raw.first().is_some_and(|c| (b'A'..=b'E').contains(c)) {
            let q1 = raw.iter().position(|&b| b == b'"').unwrap();
            let q2 = raw.iter().rposition(|&b| b == b'"').unwrap();
            pgn.extend_from_slice(b"[Event \"");
            pgn.extend_from_slice(&raw[q1 + 1..q2]);
            pgn.extend_from_slice(b"\"]\n\n");
            pgn.extend_from_slice(&raw[q2 + 1..]);
            open = true;
            count += 1;
        } else {
            pgn.extend_from_slice(raw);
        }
        pgn.push(b'\n');
        if raw.trim_ascii_end().ends_with(b"*") {
            pgn.push(b'\n');
            open = false;
        }
    }
    let games = read_bytes(&pgn, &ReadOptions::default());
    assert_eq!(games.len(), count);
    let events: Vec<&str> = games.iter().map(|g| g.game.tags().event.as_str()).collect();
    assert!(
        events.iter().any(|e| e.contains("Benk\u{f6}")),
        "Latin-1 accents decoded (\u{f6}): {:?}",
        &events[..3]
    );
    assert!(
        events.iter().all(|e| !e.contains('\u{fffd}')),
        "no replacement characters"
    );
    let mut it = PgnReader::new(&pgn[..], ReadOptions::default());
    let _ = it.next();
    assert_eq!(it.fallback_encoding().name(), "windows-1252");
}
