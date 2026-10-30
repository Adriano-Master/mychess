//! Robustness properties of the PGN reader: no panic for any bytes, sane
//! invariants of the results, independence from how the input is chopped,
//! and linear time on pathological input (spec rule 44, D17, D21, D29).

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

use std::io::{self, Read};
use std::time::{Duration, Instant};

use common::*;
use mychess_core::{Position, SanMode};
use mychess_pgn::read::{
    CommentWhitespace, GameChunker, ParsedGame, PgnReader, PlayerNames, PreGameText, ReadOptions,
    SemicolonComments, UnsupportedVariant, parse_movetext, parse_pgn_text,
};
use proptest::prelude::*;

fn cfg() -> ProptestConfig {
    ProptestConfig {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(256),
        max_shrink_iters: 500,
        ..ProptestConfig::default()
    }
}

fn options(bits: u16) -> ReadOptions {
    ReadOptions {
        pre_game_text: if bits & 1 != 0 {
            PreGameText::AsComment
        } else {
            PreGameText::Ignore
        },
        player_names: if bits & 2 != 0 {
            PlayerNames::AsIs
        } else {
            PlayerNames::Scid
        },
        comment_whitespace: if bits & 4 != 0 {
            CommentWhitespace::Preserve
        } else {
            CommentWhitespace::Collapse
        },
        semicolon_comments: if bits & 8 != 0 {
            SemicolonComments::Drop
        } else {
            SemicolonComments::Keep
        },
        san_mode: if bits & 16 != 0 {
            SanMode::Strict
        } else {
            SanMode::Tolerant
        },
        ignored_tags: if bits & 32 != 0 {
            vec!["Annotator".into(), "White".into()]
        } else {
            Vec::new()
        },
        max_comment_len: if bits & 64 != 0 { 7 } else { 1 << 20 },
        max_game_bytes: if bits & 128 != 0 { 300 } else { 64 << 20 },
        max_diagnostics_per_game: if bits & 256 != 0 { 3 } else { 100 },
        unsupported_variant: if bits & 512 != 0 {
            UnsupportedVariant::TryStandard
        } else {
            UnsupportedVariant::Skip
        },
        ..ReadOptions::default()
    }
}

/// Fragments that exercise every state of the lexer when glued at random.
const FRAGMENTS: &[&str] = &[
    "[Event \"x\"]\n",
    "[White \"A, B (2500)\"]\n",
    "[Result \"1-0\"]\n",
    "[FEN \"8/8/8/4k3/8/8/4K3/4R3 w - - 0 1\"]\n",
    "[Variant \"Crazyhouse\"]\n",
    "[Bad \"unterminated]\n",
    "[",
    "]",
    "\"",
    "\\",
    "\n",
    "\r\n",
    "\r",
    "\n\n",
    " ",
    "\t",
    "\u{a0}",
    "1. ",
    "1... ",
    "2. ",
    "e4 ",
    "e5 ",
    "Nf3 ",
    "Nc6 ",
    "Bb5 ",
    "O-O ",
    "0-0-0 ",
    "exd5 ",
    "e8=Q ",
    "e8Q+ ",
    "--",
    "Z0 ",
    "Nbd7 ",
    "ed ",
    "Pe4 ",
    "xyz ",
    "Kk ",
    "$1 ",
    "$ ",
    "$300 ",
    "$0",
    "!! ",
    "?! ",
    "+- ",
    "RR ",
    "D ",
    "N ",
    "1-0",
    "0-1",
    "1/2-1/2",
    "*",
    "1/2",
    "½-½",
    "{",
    "}",
    "{comment}",
    "{ multi\nline [%clk 0:00:01]\n}",
    "(",
    ")",
    "( ",
    " )",
    ";",
    "; semi\n",
    "%",
    "% esc\n",
    "[%clk 1:00]",
    "\u{e9}",
    "\u{4e2d}",
    "\u{feff}",
    "\0",
    "\x1b",
    "((((",
    "))))",
    "{{{{",
    "}}}}",
];

fn soup() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(0..FRAGMENTS.len(), 0..80).prop_map(|ix| {
        let mut b = Vec::new();
        for i in ix {
            b.extend_from_slice(FRAGMENTS[i].as_bytes());
        }
        b
    })
}

const VALID: &str = "[Event \"Test\"]\n[White \"Anand, V\"]\n[Black \"Kramnik, V\"]\n[Result \"1-0\"]\n\n1. e4 {best} e5 $1 2. Nf3 (2. f4 exf4 {gambit}) 2... Nc6 3. Bb5 a6 ; tail\n4. Ba4 Nf6 5. O-O Be7 1-0\n\n[Event \"Two\"]\n[FEN \"8/8/8/4k3/8/8/4K3/4R3 w - - 0 1\"]\n\n1. Rd1 Kf5 *\n";

fn mutated() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec((any::<u16>(), any::<u8>(), any::<u8>()), 0..25).prop_map(|edits| {
        let mut b = VALID.as_bytes().to_vec();
        for (pos, kind, byte) in edits {
            if b.is_empty() {
                break;
            }
            let at = pos as usize % b.len();
            match kind % 4 {
                0 => {
                    b.remove(at);
                }
                1 => b.insert(at, byte),
                2 => b[at] = byte,
                _ => {
                    let end = (at + 1 + byte as usize % 20).min(b.len());
                    let chunk = b[at..end].to_vec();
                    b.splice(at..at, chunk);
                }
            }
        }
        b
    })
}

fn check_invariants(bytes: &[u8], opts: &ReadOptions, games: &[ParsedGame]) -> Result<(), String> {
    let len = bytes.len() as u64;
    let mut prev_end = 0u64;
    for (i, g) in games.iter().enumerate() {
        if g.number != i as u64 + 1 {
            return Err(format!("game {i} has number {}", g.number));
        }
        if g.byte_range.start > g.byte_range.end || g.byte_range.end > len {
            return Err(format!("bad range {:?} for len {len}", g.byte_range));
        }
        if g.byte_range.start < prev_end {
            return Err(format!(
                "overlapping ranges: {prev_end} then {:?}",
                g.byte_range
            ));
        }
        prev_end = g.byte_range.end;
        if g.first_line == 0 {
            return Err("line 0".into());
        }
        if g.diagnostics.len() > opts.max_diagnostics_per_game + 2 {
            return Err(format!("{} diagnostics", g.diagnostics.len()));
        }
        for d in &g.diagnostics {
            if d.game != g.number
                || d.line == 0
                || d.column == 0
                || d.offset > len
                || d.severity != d.kind.severity()
            {
                return Err(format!("bad diagnostic {d:?}"));
            }
            let _ = d.to_string();
        }
        // the tree must be internally consistent: every stored move is legal where stored
        for n in g
            .game
            .descendants(g.game.root())
            .map_err(|e| e.to_string())?
        {
            let p = g
                .game
                .parent(n)
                .map_err(|e| e.to_string())?
                .ok_or("orphan")?;
            let mv = g.game.mv(n).map_err(|e| e.to_string())?;
            let pos = g.game.position_at(p).map_err(|e| e.to_string())?;
            if pos.san(mv).is_err() {
                return Err("stored move cannot be written as SAN".into());
            }
        }
    }
    Ok(())
}

fn run(bytes: &[u8], opts: &ReadOptions) -> Vec<ParsedGame> {
    PgnReader::new(bytes, opts.clone())
        .map(|r| r.expect("slice input never fails"))
        .collect()
}

proptest! {
    #![proptest_config(cfg())]

    #[test]
    fn arbitrary_bytes_never_panic_and_results_are_sane(bytes in prop::collection::vec(any::<u8>(), 0..2000), bits in any::<u16>()) {
        let o = options(bits);
        let games = run(&bytes, &o);
        check_invariants(&bytes, &o, &games).map_err(TestCaseError::fail)?;
    }

    #[test]
    fn token_soup_never_panics_and_results_are_sane(bytes in soup(), bits in any::<u16>()) {
        let o = options(bits);
        let games = run(&bytes, &o);
        check_invariants(&bytes, &o, &games).map_err(TestCaseError::fail)?;
    }

    #[test]
    fn mutated_valid_pgn_never_panics_and_results_are_sane(bytes in mutated(), bits in any::<u16>()) {
        let o = options(bits);
        let games = run(&bytes, &o);
        check_invariants(&bytes, &o, &games).map_err(TestCaseError::fail)?;
    }

    #[test]
    fn movetext_entry_points_never_panic(bytes in soup(), bits in any::<u16>()) {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let o = options(bits);
        let _ = parse_movetext(&text, &Position::new(), &o);
        let _ = parse_pgn_text(&text, &o);
    }

    /// The way the input is cut into `read` calls must not change anything.
    #[test]
    fn result_does_not_depend_on_read_sizes(bytes in prop_oneof![soup(), mutated()], sizes in prop::collection::vec(1usize..64, 1..8)) {
        struct Chopped<'a> { data: &'a [u8], sizes: Vec<usize>, i: usize }
        impl Read for Chopped<'_> {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                let n = self.sizes[self.i % self.sizes.len()].min(buf.len()).min(self.data.len());
                self.i += 1;
                buf[..n].copy_from_slice(&self.data[..n]);
                self.data = &self.data[n..];
                Ok(n)
            }
        }
        // Text that is not valid UTF-8 goes through encoding detection, which currently
        // depends on read sizes: see `encoding_guess_does_not_depend_on_read_sizes` below.
        prop_assume!(std::str::from_utf8(&bytes).is_ok());
        let o = ReadOptions::default();
        let whole = run(&bytes, &o);
        let chopped: Vec<ParsedGame> = PgnReader::new(Chopped { data: &bytes, sizes, i: 0 }, o.clone())
            .map(|r| r.unwrap())
            .collect();
        prop_assert_eq!(whole.len(), chopped.len());
        for (a, b) in whole.iter().zip(&chopped) {
            prop_assert_eq!(a.diagnostics.clone(), b.diagnostics.clone());
            prop_assert_eq!(a.byte_range.clone(), b.byte_range.clone());
            prop_assert!(a.game.same_content(&b.game));
            prop_assert_eq!(a.termination, b.termination);
        }
    }

    /// The low-level chunker + `RawGame::parse` path is equivalent to the iterator.
    #[test]
    fn chunker_path_equals_reader_path(bytes in prop_oneof![soup(), mutated()], bits in any::<u16>()) {
        let o = options(bits);
        let a = run(&bytes, &o);
        let b: Vec<ParsedGame> = GameChunker::new(&bytes[..], o.max_game_bytes)
            .map(|r| r.unwrap().parse(&o))
            .collect();
        prop_assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            prop_assert_eq!(x.diagnostics.clone(), y.diagnostics.clone());
            prop_assert!(x.game.same_content(&y.game));
        }
    }

    /// Line endings are interchangeable (rule 2 / D19): LF, CRLF and CR give the same games.
    #[test]
    fn line_ending_style_never_changes_the_games(bytes in prop_oneof![soup(), mutated()]) {
        let lf: Vec<u8> = {
            // normalise to LF in the generated text first, then re-express it
            let s = String::from_utf8_lossy(&bytes).replace("\r\n", "\n").replace('\r', "\n");
            s.into_bytes()
        };
        let text = String::from_utf8_lossy(&lf).into_owned();
        let o = ReadOptions::default();
        let base = run(text.as_bytes(), &o);
        for eol in ["\r\n", "\r"] {
            let other = run(text.replace('\n', eol).as_bytes(), &o);
            prop_assert_eq!(base.len(), other.len(), "{:?}", eol);
            for (a, b) in base.iter().zip(&other) {
                prop_assert!(a.game.same_content(&b.game), "{:?}: {} vs {}", eol, dump(&a.game), dump(&b.game));
                prop_assert_eq!(a.first_line, b.first_line);
                let ka: Vec<_> = a.diagnostics.iter().map(|d| (d.kind.clone(), d.line, d.column)).collect();
                let kb: Vec<_> = b.diagnostics.iter().map(|d| (d.kind.clone(), d.line, d.column)).collect();
                prop_assert_eq!(ka, kb, "{:?}", eol);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Pathological input: linear time, no stack use
// ---------------------------------------------------------------------------

fn time_it(f: impl Fn()) -> Duration {
    (0..3)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed()
        })
        .min()
        .unwrap()
}

/// Reads `make(n)` and `make(8n)`; the second must take far less than 64x the first.
fn assert_roughly_linear(name: &str, make: impl Fn(usize) -> Vec<u8>) {
    let small = make(25_000);
    let big = make(200_000);
    let o = ReadOptions::default();
    let t_small = time_it(|| {
        let _ = run(&small, &o);
    });
    let t_big = time_it(|| {
        let _ = run(&big, &o);
    });
    // linear = 8x; quadratic = 64x. Allow generous noise (and a floor for tiny timings).
    let floor = Duration::from_millis(20);
    let ratio = t_big.as_secs_f64() / t_small.max(floor).as_secs_f64();
    assert!(
        ratio < 28.0,
        "{name}: 8x input took {ratio:.1}x the time ({t_small:?} -> {t_big:?})"
    );
}

#[test]
fn pathological_inputs_take_linear_time() {
    let wrap = |body: String| format!("[Event \"x\"]\n\n1. e4 e5 {body}").into_bytes();
    assert_roughly_linear("open parens", |n| wrap("(".repeat(n)));
    assert_roughly_linear("close parens", |n| wrap(")".repeat(n)));
    assert_roughly_linear("nested real variations", |n| {
        wrap("(1... c5 ".repeat(n / 8))
    });
    assert_roughly_linear("open braces", |n| wrap("{".repeat(n)));
    assert_roughly_linear("unterminated comment", |n| {
        wrap(format!("{{{}", "a b ".repeat(n / 4)))
    });
    assert_roughly_linear("stray close braces", |n| wrap("} ".repeat(n / 2)));
    assert_roughly_linear("semicolons", |n| wrap("; x\n".repeat(n / 4)));
    assert_roughly_linear("dollars", |n| wrap("$ ".repeat(n / 2)));
    assert_roughly_linear("bad tokens", |n| wrap("zz9 ".repeat(n / 4)));
    assert_roughly_linear("one giant token", |n| wrap("a".repeat(n)));
    assert_roughly_linear("giant digit run", |n| wrap("1".repeat(n)));
    assert_roughly_linear("many illegal moves", |n| wrap("Qh5 ".repeat(n / 4)));
    assert_roughly_linear("many headers", |n| {
        "[Event \"x\"]\n".repeat(n / 12).into_bytes()
    });
    assert_roughly_linear("many one-line games", |n| {
        "[Event \"x\"]\n\n*\n".repeat(n / 15).into_bytes()
    });
    assert_roughly_linear("giant tag value", |n| {
        format!("[Event \"{}\"]\n\n*\n", "v".repeat(n)).into_bytes()
    });
    assert_roughly_linear("giant unterminated tag", |n| {
        format!("[Event \"{}", "v".repeat(n)).into_bytes()
    });
    assert_roughly_linear("many brackets", |n| "[".repeat(n).into_bytes());
    assert_roughly_linear("many bare CR", |n| {
        format!("[Event \"x\"]\r\r{}", "\r".repeat(n)).into_bytes()
    });
    assert_roughly_linear("junk lines", |n| {
        "junk junk junk\n".repeat(n / 15).into_bytes()
    });
    assert_roughly_linear("percent lines", |n| wrap("\n% x".repeat(n / 4)));
    assert_roughly_linear("many nags", |n| wrap("$1 ".repeat(n / 3)));
    assert_roughly_linear("long main line", |n| {
        let mut s = String::from("[Event \"x\"]\n\n");
        for i in 0..n / 20 {
            s.push_str(&format!("{}. Nf3 Nf6 {}. Ng1 Ng8 ", 2 * i + 1, 2 * i + 2));
        }
        s.into_bytes()
    });
}

#[test]
fn many_sibling_variations_on_one_move_are_read_in_linear_time() {
    // 20 distinct legal replies at the root, repeated: duplicates merge into the same node.
    let variants = ["e4", "d4", "c4", "Nf3", "Nc3", "g3", "b3", "f4", "a3", "h3"];
    let make = |n: usize| {
        let mut s = String::from("[Event \"x\"]\n\n1. a4 ");
        for i in 0..n / 6 {
            s.push_str(&format!("({}) ", variants[i % variants.len()]));
        }
        s.into_bytes()
    };
    let small = make(20_000);
    let big = make(160_000);
    let o = ReadOptions::default();
    let t1 = time_it(|| drop(run(&small, &o))).max(Duration::from_millis(20));
    let t2 = time_it(|| drop(run(&big, &o)));
    assert!(
        t2.as_secs_f64() / t1.as_secs_f64() < 28.0,
        "{t1:?} -> {t2:?}"
    );
}

#[test]
fn hostile_inputs_complete_within_a_generous_wall_clock_budget() {
    // Absolute guard in addition to the ratios: 1 MB of nasty text in well under 30 s even in debug.
    let nasty = "(((([%clk] {{ }} ;; $$ %%\n[Event ".repeat(30_000);
    let t = Instant::now();
    let games = run(nasty.as_bytes(), &ReadOptions::default());
    assert!(
        t.elapsed() < Duration::from_secs(30),
        "took {:?}",
        t.elapsed()
    );
    assert!(!games.is_empty());
}

#[test]
fn memory_for_diagnostics_stays_bounded_on_a_hostile_game() {
    let body = "zz9 ) ( } $999 ".repeat(200_000);
    let g = mt(&body);
    assert!(g.diagnostics.len() <= 101, "{}", g.diagnostics.len());
}

/// Reads `data` one byte per `read` call.
struct OneByte<'a>(&'a [u8]);

impl Read for OneByte<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.0.is_empty() {
            return Ok(0);
        }
        buf[0] = self.0[0];
        self.0 = &self.0[1..];
        Ok(1)
    }
}

#[test]
fn encoding_guess_does_not_depend_on_read_sizes() {
    let mut data = b"[White \"Anand\xa3\"]\n\n*\n\n[Event \"".to_vec();
    data.extend(std::iter::repeat_n(0xA5u8, 36));
    data.extend_from_slice(b"\"]\n\n*\n");
    let whole: Vec<String> = run(&data, &ReadOptions::default())
        .iter()
        .map(|g| g.game.tags().white.clone())
        .collect();
    let bytewise: Vec<String> = PgnReader::new(OneByte(&data), ReadOptions::default())
        .map(|g| g.unwrap().game.tags().white.clone())
        .collect();
    assert_eq!(whole, bytewise);
}
