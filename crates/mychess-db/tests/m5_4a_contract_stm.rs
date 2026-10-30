//! M5-4a contract tests for the `.stm` file, derived from
//! `docs/specs/m5-4-mascaras-de-arvore.md` (D1801, D1813-D1816, D1840-D1846)
//! and `docs/formatos/stm.md`: hostile input, encodings, ordering and the
//! atomic write.

#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::assert_is_empty,
    clippy::too_many_lines,
    clippy::manual_assert_eq,
    clippy::format_push_string,
    clippy::naive_bytecount
)]

// The module name lets `cargo test -p mychess-db m5_4a` select these tests.
mod m5_4a_contract {

    use std::fs;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use mychess_db::tree::mask::stm::{self, MAX_WARNINGS};
    use mychess_db::tree::mask::{
        MaskKey, MaskMove, MaskPosition, StmError, StmWarningKind, TreeMask, clean_comment,
    };
    use proptest::prelude::*;

    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -";
    const HEAD: &str = "set ::tree::mask::maskSerialized ";

    /// Example 6.A of `stm.md`.
    const EXAMPLE_A: &str = "set ::tree::mask::maskSerialized {{rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -} {{{null {} white {} {} {}} {e4 ! Green {King's pawn} ::rep::_tb_include {}} {d4 {} white {} {} {}}} {My repertoire}} {rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq -} {{{e5 {} white {} {} {}} {c5 ?! Red {Sicilian: see line 2} ::rep::_tb_exclude ::tree::mask::imageMainLine}} {}} {rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq -} {{{Nf3 !? Yellow {Line one\nsecond line} tb_bkm {}}} {}}}\n";

    fn script(word: &str) -> String {
        format!("{HEAD}{word}\n")
    }

    fn one(key: &str, san: &str, comment: &str) -> TreeMask {
        let mut mask = TreeMask::new();
        let mut m = MaskMove::new(san);
        m.comment = comment.into();
        mask.insert(
            MaskKey::from_text(key),
            MaskPosition {
                moves: vec![m],
                comment: String::new(),
            },
        );
        mask
    }

    // -----------------------------------------------------------------------
    // Truncation, unbalanced delimiters
    // -----------------------------------------------------------------------

    #[test]
    fn example_a_cut_at_every_byte_is_an_error_until_the_last_brace() {
        let bytes = EXAMPLE_A.as_bytes();
        let closing = EXAMPLE_A.rfind('}').unwrap();
        for cut in 0..=bytes.len() {
            let result = stm::parse(&bytes[..cut]);
            if cut <= closing {
                assert!(result.is_err(), "cut at {cut} must be an error");
            } else {
                let (mask, report) = result.unwrap();
                assert_eq!(mask.len(), 3, "cut at {cut}");
                assert_eq!(report.total_warnings(), 0);
            }
        }
        // An empty file is "not a mask", not a syntax error.
        assert!(matches!(stm::parse(b""), Err(StmError::NotAMask)));
    }

    #[test]
    fn multibyte_comment_cut_inside_a_character_never_panics() {
        let text = stm::serialize(&one(START, "e4", "caf\u{e9} \u{4e2d} \u{1f600}"));
        let closing = text.rfind('}').unwrap();
        for cut in 0..=text.len() {
            let result = stm::parse(&text.as_bytes()[..cut]);
            if cut <= closing {
                assert!(result.is_err(), "cut at {cut}");
            }
        }
    }

    #[test]
    fn unbalanced_or_foreign_words_are_errors_and_never_a_mask() {
        let cases: Vec<(&str, String)> = vec![
            ("open brace", script("{{a}")),
            ("extra close brace", script("{a}}")),
            ("only open brace", script("{")),
            ("unterminated quote", script("\"abc")),
            ("quote then garbage", script("\"a\"b")),
            ("brace then garbage", script("{a}b")),
            ("command substitution", script("[exec ls]")),
            ("bare command substitution", script("a[b]")),
            ("variable", script("$x")),
            ("quoted variable", script("\"$x\"")),
            ("quoted bracket", script("\"[puts x]\"")),
            ("second word", script("{} extra")),
            (
                "second command by newline",
                format!("{HEAD}{{}}\nputs hi\n"),
            ),
            (
                "second command by semicolon",
                format!("{HEAD}{{}}; exec rm\n"),
            ),
            ("command before", format!("puts hi\n{HEAD}{{}}\n")),
            ("source before", format!("source /etc/passwd\n{HEAD}{{}}\n")),
            ("other variable", "set other {}\n".to_string()),
            ("no value", "set ::tree::mask::maskSerialized".to_string()),
            ("no set", "::tree::mask::maskSerialized {}\n".to_string()),
            ("double set", format!("{HEAD}{{}}\n{HEAD}{{}}\n")),
            ("exec", "exec rm -rf /\n".to_string()),
            ("binary", "\u{7f}ELF\u{1}\u{2}\u{3}".to_string()),
        ];
        for (name, text) in cases {
            assert!(
                stm::parse(text.as_bytes()).is_err(),
                "{name}: must be refused, got Ok"
            );
        }
    }

    #[test]
    fn dollar_and_bracket_are_data_inside_braces_and_escaped_elsewhere() {
        // Inside braces nothing is substituted (stm.md 3, rule 2).
        let mask = one(START, "e4", "price $5 [x] \"q\" ; end");
        let text = stm::serialize(&mask);
        assert_eq!(stm::parse(text.as_bytes()).unwrap().0, mask);
        // Escaped `$` and `[` in a bare word are plain characters (rule 4).
        let (_, report) = stm::parse(script("\\$a\\[b").as_bytes()).unwrap();
        assert!(report.total_warnings() > 0, "one element is an odd list");
    }

    #[test]
    fn syntax_error_in_an_inner_level_is_a_warning_or_an_error_never_a_panic() {
        let key = format!("{{{START}}}");
        let broken_values = [
            "{{{e4 {} white {x {} {}}} {}}",
            "{{{e4 {} white x}} {}}",
            "{{e4}} {}",
            "{{{e4 \" white}} {}}",
            "{{} {}}",
            "{{{}} {}}",
            "{x}",
            "{}",
        ];
        for value in broken_values {
            let text = script(&format!("{{{key} {value}}}"));
            match stm::parse(text.as_bytes()) {
                Ok((mask, report)) => {
                    for (_, p) in mask.iter() {
                        assert!(p.moves.len() <= 500);
                    }
                    let _ = report.total_warnings();
                }
                Err(e) => assert!(
                    matches!(e, StmError::Syntax { .. } | StmError::NotAMask),
                    "{value}: {e:?}"
                ),
            }
        }
    }

    // -----------------------------------------------------------------------
    // Size and nesting
    // -----------------------------------------------------------------------

    #[test]
    fn deep_nesting_does_not_overflow_a_small_stack() {
        // No recursion: the structure has four fixed levels (D1815). Run on a
        // thread with a tiny stack so a recursive parser would crash here.
        let handle = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let n = 1_000_000;
                let balanced = script(&format!("{{{}{}}}", "{".repeat(n), "}".repeat(n)));
                let r1 = stm::parse(balanced.as_bytes());
                let unbalanced = script(&format!("{{{}}}", "{".repeat(n)));
                let r2 = stm::parse(unbalanced.as_bytes());
                let backslashes = script(&format!("{{{}}}", "\\".repeat(n)));
                let r3 = stm::parse(backslashes.as_bytes());
                let quotes = script(&format!("\"{}\"", "{\"".repeat(n)));
                let r4 = stm::parse(quotes.as_bytes());
                (
                    r1.is_ok(),
                    r2.is_err(),
                    r3.is_ok() || r3.is_err(),
                    r4.is_ok(),
                )
            })
            .unwrap();
        let (_, unbalanced_is_err, _, _) = handle.join().expect("no stack overflow, no panic");
        assert!(unbalanced_is_err, "an open brace never closed is an error");
    }

    #[test]
    fn nested_value_inside_a_valid_position_is_not_expanded_recursively() {
        let n = 200_000;
        let value = format!("{{{}{}}}", "{".repeat(n), "}".repeat(n));
        let text = script(&format!("{{{{{START}}} {value}}}"));
        let result = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || stm::parse(text.as_bytes()).map(|(m, r)| (m.len(), r.total_warnings())))
            .unwrap()
            .join()
            .expect("no panic");
        drop(result);
    }

    #[test]
    fn a_million_short_elements_are_read_with_bounded_warnings() {
        // `k {}` repeated: one key, a million times. The last wins; the
        // warnings are listed up to 100 and the rest counted (D1814).
        let n = 1_000_000;
        let text = script(&format!("{{{}}}", "k {} ".repeat(n)));
        let (mask, report) = stm::parse(text.as_bytes()).unwrap();
        assert!(mask.len() <= 1);
        assert_eq!(report.warnings.len(), MAX_WARNINGS);
        assert!(report.total_warnings() >= n - 1);
        assert!(report.warnings.len() == MAX_WARNINGS);
    }

    #[test]
    fn many_distinct_positions_keep_their_count_and_order() {
        let n = 100_000;
        let mut body = String::new();
        for i in 0..n {
            body.push_str(&format!(
                "{{k{i}}} {{{{{{m{i} {{}} white {{}} {{}} {{}}}}}} {{}}}} "
            ));
        }
        let (mask, report) = stm::parse(script(&format!("{{{body}}}")).as_bytes()).unwrap();
        assert_eq!(mask.len(), n);
        assert_eq!(report.positions, n);
        // Opaque keys: warned (KeyNotFen), kept in file order.
        assert_eq!(report.total_warnings(), n);
        let keys: Vec<String> = mask.iter().map(|(k, _)| k.as_str().to_string()).collect();
        assert!(keys.iter().enumerate().all(|(i, k)| *k == format!("k{i}")));
    }

    #[test]
    fn a_list_of_a_million_moves_is_cut_at_500_with_one_warning_kind() {
        let n = 1_000_000;
        let mut moves = String::with_capacity(n * 8);
        for i in 0..n {
            moves.push_str(&format!("m{i} "));
        }
        let text = script(&format!("{{{{{START}}} {{{{{moves}}} {{}}}}}}"));
        let (mask, report) = stm::parse(text.as_bytes()).unwrap();
        // One-element "moves" are completed with empties (WrongMoveLength),
        // never more than 500 are kept (D1815).
        let (_, p) = mask.iter().next().unwrap();
        assert!(p.moves.len() <= 500);
        assert!(report.total_warnings() > 0);
    }

    // -----------------------------------------------------------------------
    // Encodings, NUL
    // -----------------------------------------------------------------------

    fn comment_body(comment: &str) -> String {
        format!("{HEAD}{{{{{START}}} {{{{{{e4 {{}} white {{{comment}}} {{}} {{}}}}}} {{}}}}}}\n")
    }

    fn first_comment(bytes: &[u8]) -> String {
        stm::parse(bytes).unwrap().0.iter().next().unwrap().1.moves[0]
            .comment
            .clone()
    }

    #[test]
    fn utf16_big_endian_with_bom_is_read() {
        let body = comment_body("caf\u{e9} \u{1f600}");
        let mut bytes = vec![0xfe, 0xff];
        for u in body.encode_utf16() {
            bytes.extend_from_slice(&u.to_be_bytes());
        }
        assert_eq!(first_comment(&bytes), "caf\u{e9} \u{1f600}");
    }

    #[test]
    fn utf16_with_odd_length_or_lone_surrogate_is_not_a_panic() {
        let body = comment_body("abc");
        let mut le = vec![0xff, 0xfe];
        for u in body.encode_utf16() {
            le.extend_from_slice(&u.to_le_bytes());
        }
        le.push(0x41); // dangling byte
        drop(stm::parse(&le));
        let mut lone = vec![0xff, 0xfe];
        for u in HEAD.encode_utf16() {
            lone.extend_from_slice(&u.to_le_bytes());
        }
        lone.extend_from_slice(&0xd800u16.to_le_bytes());
        lone.extend_from_slice(&[b'{', 0, b'}', 0]);
        drop(stm::parse(&lone));
        // Only a BOM.
        assert!(stm::parse(&[0xff, 0xfe]).is_err());
        assert!(stm::parse(&[0xfe, 0xff]).is_err());
        assert!(stm::parse(&[0xef, 0xbb, 0xbf]).is_err());
    }

    #[test]
    fn windows_1252_special_range_is_decoded() {
        // 0x80 is the euro sign, 0x99 the trade mark in Windows-1252.
        let mut bytes = comment_body("X").into_bytes();
        let pos = bytes.iter().position(|&b| b == b'X').unwrap();
        bytes.splice(pos..=pos, [0x80, b' ', 0x99, b' ', 0xe9]);
        assert_eq!(first_comment(&bytes), "\u{20ac} \u{2122} \u{e9}");
    }

    #[test]
    fn every_byte_value_inside_a_comment_never_panics() {
        let mut bytes = comment_body("X").into_bytes();
        let pos = bytes.iter().position(|&b| b == b'X').unwrap();
        // Skip the braces, which would change the structure.
        let all: Vec<u8> = (0u8..=255).filter(|b| *b != b'{' && *b != b'}').collect();
        bytes.splice(pos..=pos, all);
        let (mask, _) = stm::parse(&bytes).unwrap();
        let text = stm::serialize(&mask);
        // Whatever was read can be written and read back identically.
        assert_eq!(stm::parse(text.as_bytes()).unwrap().0, mask);
    }

    #[test]
    fn nul_bytes_are_handled_without_panic() {
        assert!(stm::parse(&[0u8; 4096]).is_err());
        let mut bytes = comment_body("a\0b").into_bytes();
        bytes.push(0);
        drop(stm::parse(&bytes));
        let mut before = vec![0u8];
        before.extend_from_slice(comment_body("a").as_bytes());
        drop(stm::parse(&before));
        // A NUL in a comment that is accepted must survive a round trip.
        if let Ok((mask, _)) = stm::parse(comment_body("a\0b").as_bytes()) {
            let text = stm::serialize(&mask);
            assert_eq!(stm::parse(text.as_bytes()).unwrap().0, mask);
        }
    }

    #[test]
    fn writing_is_utf8_without_bom_and_lf_only() {
        let mask = one(START, "e4", "caf\u{e9}\r\nx\ry");
        let text = stm::serialize(&mask);
        let bytes = text.as_bytes();
        assert!(!bytes.starts_with(&[0xef, 0xbb, 0xbf]));
        assert!(!bytes.contains(&b'\r'));
        assert!(std::str::from_utf8(bytes).is_ok());
        assert!(bytes.ends_with(b"\n"));
        assert_eq!(bytes.iter().filter(|&&b| b == b'\n').count(), 3);
    }

    // -----------------------------------------------------------------------
    // Order, opaque keys, `#` protection
    // -----------------------------------------------------------------------

    #[test]
    fn file_order_is_kept_and_new_positions_go_last() {
        // Reverse alphabetical on purpose: no sorting may happen.
        let fens = [
            "4k3/8/8/8/8/8/4P3/4K3 w - -",
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq -",
            START,
            "4k3/8/8/8/8/8/8/4K2R w K -",
        ];
        let mut body = String::new();
        for (i, f) in fens.iter().enumerate() {
            body.push_str(&format!(
                "{{{f}}} {{{{{{m{i} {{}} white {{}} {{}} {{}}}}}} {{}}}} "
            ));
        }
        let (mut mask, _) = stm::parse(script(&format!("{{{body}}}")).as_bytes()).unwrap();
        let order = |m: &TreeMask| -> Vec<String> {
            m.iter().map(|(k, _)| k.as_str().to_string()).collect()
        };
        let canon: Vec<String> = fens
            .iter()
            .map(|f| MaskKey::from_text(f).as_str().to_string())
            .collect();
        assert_eq!(order(&mask), canon);
        let text = stm::serialize(&mask);
        let (back, _) = stm::parse(text.as_bytes()).unwrap();
        assert_eq!(order(&back), canon);
        assert_eq!(stm::serialize(&back), text, "stable bytes");
        // A new position is appended; editing an old one keeps its slot.
        mask.insert(
            MaskKey::Opaque("zzz".into()),
            MaskPosition {
                moves: vec![MaskMove::new("e4")],
                comment: String::new(),
            },
        );
        mask.insert(
            MaskKey::from_text(fens[0]),
            MaskPosition {
                moves: vec![MaskMove::new("Kd2")],
                comment: String::new(),
            },
        );
        let after = order(&mask);
        assert_eq!(after[0], canon[0], "replaced in place");
        assert_eq!(after.last().unwrap(), "zzz");
        assert_eq!(after.len(), 5);
    }

    #[test]
    fn hash_first_element_is_protected_in_every_list() {
        // The first key of V, the first SAN of a list and a first comment.
        let mut mask = TreeMask::new();
        mask.insert(
            MaskKey::Opaque("#first".into()),
            MaskPosition {
                moves: vec![MaskMove::new("#san"), MaskMove::new("# x")],
                comment: "#comment".into(),
            },
        );
        let text = stm::serialize(&mask);
        assert!(
            text.starts_with("set ::tree::mask::maskSerialized {{#first}"),
            "{text}"
        );
        assert_eq!(stm::parse(text.as_bytes()).unwrap().0, mask);
    }

    #[test]
    fn opaque_keys_with_special_characters_survive() {
        let mut mask = TreeMask::new();
        for (i, key) in ["a b", "{", "}", "a\\", "x\"y", "$v", "[c]", "a;b", "", "  "]
            .iter()
            .enumerate()
        {
            mask.insert(
                MaskKey::Opaque((*key).to_string()),
                MaskPosition {
                    moves: vec![MaskMove::new(format!("m{i}"))],
                    comment: String::new(),
                },
            );
        }
        let text = stm::serialize(&mask);
        let (back, report) = stm::parse(text.as_bytes()).unwrap();
        assert_eq!(back, mask);
        assert_eq!(report.positions, 10);
    }

    #[test]
    fn two_spellings_of_the_same_position_are_one_key_and_the_last_wins() {
        // D1842: the en passant field is canonicalized; same position twice.
        let a = "4k3/8/8/8/8/8/4P3/4K3 w - -";
        let b = "4k3/8/8/8/8/8/4P3/4K3 w - e3"; // no black pawn beside e4: canonical `-`
        let body = format!(
            "{{{a}}} {{{{{{x {{}} white {{}} {{}} {{}}}}}} {{}}}} {{{b}}} {{{{{{y {{}} white {{}} {{}} {{}}}}}} {{}}}}"
        );
        let (mask, report) = stm::parse(script(&format!("{{{body}}}")).as_bytes()).unwrap();
        assert_eq!(mask.len(), 1);
        assert_eq!(mask.iter().next().unwrap().1.moves[0].san, "y");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.kind == StmWarningKind::DuplicateKey)
        );
    }

    // -----------------------------------------------------------------------
    // Files
    // -----------------------------------------------------------------------

    #[test]
    fn reading_a_directory_or_a_missing_path_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(stm::read(dir.path()), Err(StmError::Io { .. })));
        assert!(matches!(
            stm::read(&dir.path().join("nope").join("x.stm")),
            Err(StmError::Io { .. })
        ));
    }

    #[test]
    fn reading_an_empty_or_binary_file_is_not_a_mask() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty.stm");
        fs::write(&empty, b"").unwrap();
        assert!(matches!(stm::read(&empty), Err(StmError::NotAMask)));
        let bin = dir.path().join("bin.stm");
        fs::write(&bin, (0u8..=255).cycle().take(10_000).collect::<Vec<u8>>()).unwrap();
        assert!(stm::read(&bin).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn reading_an_unreadable_file_is_an_io_error() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.stm");
        fs::write(&path, EXAMPLE_A).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        let result = stm::read(&path);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        // Root can read anything; otherwise the error carries the path.
        if let Err(e) = result {
            assert!(matches!(e, StmError::Io { ref path, .. } if path.ends_with("secret.stm")));
        }
    }

    #[test]
    fn writing_into_a_missing_directory_fails_and_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing").join("m.stm");
        assert!(matches!(
            stm::write(&path, &one(START, "e4", "")),
            Err(StmError::Io { .. })
        ));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn writing_over_a_directory_fails_and_leaves_it_alone() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("m.stm");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("inner.txt"), b"keep").unwrap();
        assert!(stm::write(&target, &one(START, "e4", "")).is_err());
        assert!(target.is_dir());
        assert_eq!(fs::read(target.join("inner.txt")).unwrap(), b"keep");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1, "no temp left");
    }

    #[test]
    fn a_read_only_destination_is_replaced_whole_or_left_intact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.stm");
        let old = one(START, "e4", "old");
        stm::write(&path, &old).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&path, perms).unwrap();
        let new = one(START, "d4", "new");
        let result = stm::write(&path, &new);
        let on_disk = stm::read(&path).unwrap().0;
        match result {
            Ok(_) => assert_eq!(on_disk, new),
            Err(_) => assert_eq!(on_disk, old),
        }
        assert!(
            fs::metadata(&path).unwrap().permissions().readonly(),
            "permissions of the destination are kept"
        );
        let mut perms = fs::metadata(&path).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        fs::set_permissions(&path, perms).unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1, "no temp left");
    }

    #[test]
    fn concurrent_writers_never_leave_a_torn_file_for_a_reader() {
        let dir = tempfile::tempdir().unwrap();
        let path = Arc::new(dir.path().join("m.stm"));
        let masks: Vec<TreeMask> = (0..4)
            .map(|i| {
                one(
                    START,
                    "e4",
                    &format!("writer {i} {}", "x".repeat(10_000 * (i + 1))),
                )
            })
            .collect();
        stm::write(&path, &masks[0]).unwrap();
        let done = Arc::new(AtomicBool::new(false));
        let reader = {
            let (path, done, masks) = (path.clone(), done.clone(), masks.clone());
            std::thread::spawn(move || {
                let mut reads = 0usize;
                while !done.load(Ordering::Relaxed) {
                    // On Windows a rename can briefly fail a read; only
                    // content, not availability, is the contract.
                    if let Ok((m, r)) = stm::read(&path) {
                        assert_eq!(r.total_warnings(), 0);
                        assert!(masks.contains(&m), "torn or foreign content");
                        reads += 1;
                    }
                }
                reads
            })
        };
        let writers: Vec<_> = masks
            .iter()
            .cloned()
            .map(|m| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for _ in 0..10 {
                        drop(stm::write(&path, &m));
                    }
                })
            })
            .collect();
        for w in writers {
            w.join().unwrap();
        }
        done.store(true, Ordering::Relaxed);
        let _ = reader.join().unwrap();
        let (last, _) = stm::read(&path).unwrap();
        assert!(masks.contains(&last));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1, "no temp left");
    }

    #[test]
    fn opaque_key_with_carriage_return_round_trips() {
        let mut mask = TreeMask::new();
        mask.insert(
            MaskKey::Opaque("a\r\nb".into()),
            MaskPosition {
                moves: vec![MaskMove::new("e4")],
                comment: String::new(),
            },
        );
        let text = stm::serialize(&mask);
        assert_eq!(stm::parse(text.as_bytes()).unwrap().0, mask);
    }

    // -----------------------------------------------------------------------
    // Properties
    // -----------------------------------------------------------------------

    fn hostile() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            prop_oneof![
                3 => proptest::sample::select(vec![
                    "a", "b", " ", "\n", "\r\n", "\t", "{", "}", "{}", "}{", "\\", "\\\n", "\\\\",
                    "\\{", "\\}", "\"", "[", "]", "$", ";", "#", "'", "%", "\u{e9}", "\u{1f600}",
                    "{ }", "[%eval 1]", "$x", "\"\"", "\\n",
                ]),
            ],
            0..20,
        )
        .prop_map(|v| v.concat())
    }

    fn mask_strategy() -> impl Strategy<Value = TreeMask> {
        proptest::collection::vec(
            (
                hostile(),
                hostile(),
                proptest::collection::vec(hostile(), 0..4),
            ),
            0..6,
        )
        .prop_map(|entries| {
            let mut mask = TreeMask::new();
            for (i, (key_text, pos_comment, comments)) in entries.into_iter().enumerate() {
                let moves = comments
                    .into_iter()
                    .enumerate()
                    .map(|(j, c)| {
                        let mut m = MaskMove::new(format!("m{j}"));
                        m.comment = clean_comment(&c).0;
                        m
                    })
                    .collect();
                mask.insert(
                    MaskKey::Opaque(format!("{}{i}", key_text.replace('\r', ""))),
                    MaskPosition {
                        moves,
                        comment: clean_comment(&pos_comment).0,
                    },
                );
            }
            mask
        })
    }

    proptest! {
        #[test]
        fn hostile_text_round_trips_and_the_bytes_are_stable(mask in mask_strategy()) {
            let text = stm::serialize(&mask);
            prop_assert!(!text.contains('\r'));
            prop_assert!(text.ends_with('\n'));
            let (back, _) = stm::parse(text.as_bytes()).unwrap();
            // Positions with no moves and no comment are not written (D1846).
            let live: Vec<_> = mask.iter().filter(|(_, p)| !p.is_dead()).collect();
            let back_all: Vec<_> = back.iter().collect();
            prop_assert_eq!(live.len(), back_all.len());
            for (a, b) in live.iter().zip(back_all.iter()) {
                prop_assert_eq!(a.0, b.0, "order and keys");
                prop_assert_eq!(a.1, b.1);
            }
            prop_assert_eq!(stm::serialize(&back), text);
        }

        #[test]
        fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
            drop(stm::parse(&bytes));
        }

        #[test]
        fn mutated_valid_files_never_panic(
            at in 0usize..EXAMPLE_A.len(),
            byte in any::<u8>(),
            drop_len in 0usize..8,
        ) {
            let mut bytes = EXAMPLE_A.as_bytes().to_vec();
            bytes[at] = byte;
            let end = (at + drop_len).min(bytes.len());
            bytes.drain(at..end);
            if let Ok((mask, _)) = stm::parse(&bytes) {
                let text = stm::serialize(&mask);
                prop_assert!(stm::parse(text.as_bytes()).is_ok());
            }
        }
    }
}
