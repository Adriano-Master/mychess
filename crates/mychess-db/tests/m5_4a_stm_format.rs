//! M5-4a: the `.stm` file (`docs/formatos/stm.md`): the documented examples,
//! refusals, tolerant reading, encodings, truncation and the file operations.

#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::assert_is_empty,
    clippy::field_reassign_with_default,
    clippy::too_many_lines
)]

// The module name lets `cargo test -p mychess-db m5_4a` select these tests.
mod m5_4a {

    use std::fmt::Write as _;
    use std::fs;

    use mychess_db::tree::mask::stm::{self, MAX_STM_BYTES, MAX_WARNINGS};
    use mychess_db::tree::mask::{
        Marker, MaskColor, MaskKey, MaskMove, MaskNag, MaskPosition, StmError, StmWarningKind,
        TreeMask,
    };

    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -";

    /// Example 6.A of `stm.md` (hand made): the real line break is inside the
    /// comment `Line one` / `second line`.
    const EXAMPLE_A: &str = "set ::tree::mask::maskSerialized {{rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -} {{{null {} white {} {} {}} {e4 ! Green {King's pawn} ::rep::_tb_include {}} {d4 {} white {} {} {}}} {My repertoire}} {rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq -} {{{e5 {} white {} {} {}} {c5 ?! Red {Sicilian: see line 2} ::rep::_tb_exclude ::tree::mask::imageMainLine}} {}} {rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq -} {{{Nf3 !? Yellow {Line one\nsecond line} tb_bkm {}}} {}}}\n";

    fn one_position(moves: Vec<MaskMove>, comment: &str) -> TreeMask {
        let mut mask = TreeMask::new();
        mask.insert(
            MaskKey::from_text(START),
            MaskPosition {
                moves,
                comment: comment.into(),
            },
        );
        mask
    }

    fn commented(san: &str, comment: &str) -> MaskMove {
        let mut m = MaskMove::new(san);
        m.comment = comment.into();
        m
    }

    #[test]
    fn example_a_is_read_and_written_back_with_the_same_bytes() {
        let (mask, report) = stm::parse(EXAMPLE_A.as_bytes()).unwrap();
        assert_eq!(report.total_warnings(), 0);
        assert_eq!((report.positions, report.moves), (3, 6));
        let entries: Vec<_> = mask.iter().collect();
        // The order of the file is kept.
        assert_eq!(entries[0].0.as_str(), START);
        assert_eq!(entries[0].1.comment, "My repertoire");
        let e4 = &entries[0].1.moves[1];
        assert_eq!(e4.san, "e4");
        assert_eq!(e4.nag, Some(MaskNag::Good));
        assert_eq!(e4.color, MaskColor::Green);
        assert_eq!(e4.markers, [Some(Marker::Include), None]);
        let c5 = &entries[1].1.moves[1];
        assert_eq!(c5.nag, Some(MaskNag::Dubious));
        assert_eq!(c5.markers, [Some(Marker::Exclude), Some(Marker::MainLine)]);
        assert_eq!(entries[2].1.moves[0].comment, "Line one\nsecond line");
        assert_eq!(stm::serialize(&mask), EXAMPLE_A);
    }

    #[test]
    fn example_b_unbalanced_brace_and_trailing_backslash() {
        let mask = one_position(
            vec![
                commented("e4", "a { b"),
                commented("d4", "x\\"),
                commented("c4", "a\\\nb"),
            ],
            "",
        );
        let text = stm::serialize(&mask);
        assert!(text.contains("{e4 {} white a\\ \\{\\ b {} {}}"), "{text}");
        assert!(text.contains("{d4 {} white x\\\\ {} {}}"), "{text}");
        assert!(text.contains("{c4 {} white a\\\\\\nb {} {}}"), "{text}");
        let (back, report) = stm::parse(text.as_bytes()).unwrap();
        assert_eq!(report.total_warnings(), 0);
        assert_eq!(back, mask);
    }

    #[test]
    fn awkward_comments_round_trip() {
        let comments = [
            "{",
            "}",
            "{}",
            "}{",
            "\\",
            "\\{",
            "a\\}b",
            "\"quoted\"",
            "[brackets] and $dollar; semi",
            "# hash first",
            "line\nbreak\n\nand more",
            "tab\there",
            "caf\u{e9} \u{4e2d}\u{6587} \u{1f600}",
            "'single'",
            "a\\\nb",
            "\\\\",
            "x\\",
        ];
        for c in comments {
            let mask = one_position(vec![commented("e4", c)], c);
            let text = stm::serialize(&mask);
            let (back, report) =
                stm::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{c:?}: {e}"));
            assert_eq!(report.total_warnings(), 0, "{c:?}");
            assert_eq!(back, mask, "{c:?}\n{text}");
        }
    }

    #[test]
    fn a_huge_comment_is_cut_at_the_limit_with_a_warning() {
        let big = "x\u{e9}".repeat(200_000);
        let text = format!(
            "set ::tree::mask::maskSerialized {{{{{START}}} {{{{{{e4 {{}} white {{{big}}} {{}} {{}}}}}} {{}}}}}}\n"
        );
        let (mask, report) = stm::parse(text.as_bytes()).unwrap();
        let (_, p) = mask.iter().next().unwrap();
        assert!(p.moves[0].comment.len() <= 64 * 1024);
        assert!(p.moves[0].comment.starts_with("x\u{e9}x"));
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.kind == StmWarningKind::CommentTruncated)
        );
    }

    #[test]
    fn files_that_are_not_masks_or_hold_other_commands_are_refused() {
        for empty in ["", "   \n\t", "# only a comment\n", "\u{feff}"] {
            assert!(
                matches!(stm::parse(empty.as_bytes()), Err(StmError::NotAMask)),
                "{empty:?}"
            );
        }
        assert!(matches!(
            stm::parse(&[0x00, 0x01, 0xff, 0xfe, 0x80]),
            Err(StmError::NotAMask)
        ));
        assert!(matches!(
            stm::parse(b"This is a plain text file.\n"),
            Err(StmError::NotAMask)
        ));
        assert!(matches!(
            stm::parse(b"source evil.tcl\n"),
            Err(StmError::NotAMask)
        ));
        for hostile in [
            "set ::tree::mask::maskSerialized {}\nexec rm -rf ~\n",
            "set ::tree::mask::maskSerialized {}\nsource other.stm\n",
            "set ::tree::mask::maskSerialized {}; exec ls\n",
            "set ::tree::mask::maskSerialized [exec ls]\n",
            "set ::tree::mask::maskSerialized \"$env(HOME)\"\n",
            "set ::tree::mask::maskSerialized {} extra\n",
            "set ::auto_path {}\n",
            "set ::tree::mask::maskSerialized {}\n# fine\nputs hello\n",
        ] {
            assert!(
                matches!(
                    stm::parse(hostile.as_bytes()),
                    Err(StmError::ForeignCommand { .. })
                ),
                "{hostile:?}"
            );
        }
        for broken in [
            "set ::tree::mask::maskSerialized",
            "set ::tree::mask::maskSerialized {{a b}",
            "set ::tree::mask::maskSerialized {a}b\n",
            "set ::tree::mask::maskSerialized \"abc\n",
        ] {
            assert!(
                matches!(stm::parse(broken.as_bytes()), Err(StmError::Syntax { .. })),
                "{broken:?}"
            );
        }
        // A list-level syntax error (unbalanced inside the word) loads nothing.
        let bad_list = "set ::tree::mask::maskSerialized {{a} b} c}\n";
        assert!(stm::parse(bad_list.as_bytes()).is_err());
    }

    #[test]
    fn comments_and_blank_lines_around_the_command_are_accepted() {
        let text = "# saved by hand\n\n  set tree::mask::maskSerialized  {}  \n# end\n";
        let (mask, _) = stm::parse(text.as_bytes()).unwrap();
        assert!(mask.is_empty());
        // A bare, backslash-escaped word (the form the Scid writes when braces do
        // not fit) and a quoted one are accepted.
        let (mask, _) = stm::parse(b"set ::tree::mask::maskSerialized \"\"\n").unwrap();
        assert!(mask.is_empty());
    }

    #[test]
    fn tolerant_reading_loads_what_is_good_and_reports_the_rest() {
        use StmWarningKind::*;
        let text = format!(
            "set ::tree::mask::maskSerialized {{\
         {{{START}}} {{{{{{e4 +- purple x {{}} weird {{}}}} {{e4+ ! white {{}} {{}} {{}}}} \
         {{{{}} {{}} {{}} {{}} {{}} {{}}}} {{d4 !}} {{c4 {{}} white c {{}} {{}} extra}}}} one}} \
         {{{START}}} {{{{}} two}} \
         bad {{not a fen}} \
         {{rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq -}} {{{{{{null {{}} white {{}} {{}} {{}}}}}} keep}} \
         dangling}}\n"
        );
        let (mask, report) = stm::parse(text.as_bytes()).unwrap();
        let kinds: Vec<_> = report.warnings.iter().map(|w| w.kind).collect();
        for k in [
            UnknownNag,
            UnknownColor,
            UnknownMarker,
            DuplicateMove,
            EmptySan,
            WrongMoveLength,
            DuplicateKey,
            KeyNotFen,
            WrongValueLength,
            OddElementCount,
        ] {
            assert!(kinds.contains(&k), "{k:?} missing in {kinds:?}");
        }
        // Repeated key: the last value wins, in the place of the first.
        let entries: Vec<_> = mask.iter().collect();
        assert_eq!(entries[0].0.as_str(), START);
        assert_eq!(entries[0].1.comment, "two");
        assert!(entries[0].1.moves.is_empty());
        assert!(
            entries
                .iter()
                .any(|(k, _)| matches!(k, MaskKey::Opaque(s) if s == "bad"))
        );
        // `null` is preserved.
        let null_pos = entries
            .iter()
            .find(|(k, _)| k.as_str().contains("4P3"))
            .unwrap();
        assert!(null_pos.1.moves[0].is_null());
        assert_eq!(null_pos.1.comment, "keep");
    }

    #[test]
    fn moves_with_the_wrong_length_are_completed_or_trimmed() {
        let text = format!(
            "set ::tree::mask::maskSerialized {{{{{START}}} {{{{{{e4 !}} {{d4 {{}} Red c {{}} {{}} junk junk}}}} {{}}}}}}\n"
        );
        let (mask, report) = stm::parse(text.as_bytes()).unwrap();
        let (_, p) = mask.iter().next().unwrap();
        assert_eq!(p.moves[0].nag, Some(MaskNag::Good));
        assert_eq!(p.moves[0].color, MaskColor::White);
        assert_eq!(p.moves[1].color, MaskColor::Red);
        assert_eq!(p.moves[1].comment, "c");
        assert_eq!(report.total_warnings(), 2);
    }

    #[test]
    fn unknown_values_survive_a_round_trip() {
        let text = format!(
            "set ::tree::mask::maskSerialized {{{{{START}}} {{{{{{e4 +- #00ff00 {{}} tb_zzz {{}}}}}} {{}}}} {{opaque key}} {{{{{{x {{}} white {{}} {{}} {{}}}}}} {{}}}}}}\n"
        );
        let (mask, report) = stm::parse(text.as_bytes()).unwrap();
        // `#rrggbb` is a valid Tk color: no warning for it.
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.kind == StmWarningKind::UnknownColor)
        );
        assert_eq!(stm::serialize(&mask), text);
    }

    #[test]
    fn old_and_spaced_values_are_normalized_on_write() {
        let text = format!(
            "set ::tree::mask::maskSerialized {{{{{START}}} {{{{{{e4 {{ !}} green {{}} tb_help {{}}}}}} {{}}}}}}\n"
        );
        let (mask, _) = stm::parse(text.as_bytes()).unwrap();
        let out = stm::serialize(&mask);
        assert!(out.contains("{e4 ! Green {} tb_help_small {}}"), "{out}");
    }

    #[test]
    fn warnings_are_listed_up_to_100_and_the_rest_is_counted() {
        let mut text = String::from("set ::tree::mask::maskSerialized {");
        for i in 0..350 {
            write!(text, "k{i} {{{{}} {{}}}} ").unwrap();
        }
        text.push_str("}\n");
        let (mask, report) = stm::parse(text.as_bytes()).unwrap();
        assert_eq!(mask.len(), 350);
        assert_eq!(report.warnings.len(), MAX_WARNINGS);
        assert_eq!(report.dropped_warnings, 250);
        assert_eq!(report.total_warnings(), 350);
    }

    #[test]
    fn more_than_500_moves_in_a_position_are_dropped_with_a_warning() {
        let mut moves = String::new();
        for i in 0..520 {
            write!(moves, "{{m{i} {{}} white {{}} {{}} {{}}}} ").unwrap();
        }
        let text =
            format!("set ::tree::mask::maskSerialized {{{{{START}}} {{{{{moves}}} {{}}}}}}\n");
        let (mask, report) = stm::parse(text.as_bytes()).unwrap();
        assert_eq!(mask.iter().next().unwrap().1.moves.len(), 500);
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.kind == StmWarningKind::TooManyMoves)
        );
    }

    #[test]
    fn truncating_the_file_anywhere_is_an_error_or_a_complete_file() {
        let mut mask = one_position(
            vec![
                commented("e4", "a { b \u{e9}\nline"),
                commented("d4", "x\\"),
            ],
            "pos [c]",
        );
        mask.insert(MaskKey::Opaque("odd key".into()), {
            let mut p = MaskPosition::default();
            p.moves.push(MaskMove::new("Nf3"));
            p
        });
        let text = stm::serialize(&mask);
        let closing = text.rfind('}').unwrap();
        for cut in 0..=text.len() {
            let result = stm::parse(&text.as_bytes()[..cut]);
            if cut <= closing {
                assert!(result.is_err(), "cut at {cut} must fail");
            } else {
                assert_eq!(result.unwrap().0, mask, "cut at {cut}");
            }
        }
    }

    #[test]
    fn utf8_with_and_without_bom_utf16_and_windows_1252() {
        let body = format!(
            "set ::tree::mask::maskSerialized {{{{{START}}} {{{{{{e4 {{}} white {{caf\u{e9} \u{201c}x\u{201d}}} {{}} {{}}}}}} {{}}}}}}\n"
        );
        let expected = "caf\u{e9} \u{201c}x\u{201d}";
        let comment_of = |bytes: &[u8]| {
            let (mask, _) = stm::parse(bytes).unwrap_or_else(|e| panic!("{e}: {bytes:?}"));
            mask.iter().next().unwrap().1.moves[0].comment.clone()
        };
        assert_eq!(comment_of(body.as_bytes()), expected);
        let mut bom = vec![0xef, 0xbb, 0xbf];
        bom.extend_from_slice(body.as_bytes());
        assert_eq!(comment_of(&bom), expected);
        let mut utf16 = vec![0xff, 0xfe];
        for u in body.encode_utf16() {
            utf16.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(comment_of(&utf16), expected);
        // Windows-1252: curly quotes are 0x93 and 0x94, the e acute 0xE9.
        let cp1252: Vec<u8> = body
            .chars()
            .map(|c| match c {
                '\u{e9}' => 0xe9,
                '\u{201c}' => 0x93,
                '\u{201d}' => 0x94,
                other => other as u8,
            })
            .collect();
        assert_eq!(comment_of(&cp1252), expected);
        // Latin-1 only.
        let latin1: Vec<u8> = "set ::tree::mask::maskSerialized {}\n# caf\u{e9}\n"
            .chars()
            .map(|c| c as u8)
            .collect();
        assert!(stm::parse(&latin1).unwrap().0.is_empty());
    }

    #[test]
    fn line_ends_crlf_and_cr_are_read_and_lf_is_written() {
        let body = format!(
            "set ::tree::mask::maskSerialized {{{{{START}}} {{{{{{e4 {{}} white {{one\ntwo}} {{}} {{}}}}}} {{}}}}}}\n"
        );
        for text in [
            body.clone(),
            body.replace('\n', "\r\n"),
            body.replace('\n', "\r"),
        ] {
            let (mask, _) = stm::parse(text.as_bytes()).unwrap();
            assert_eq!(mask.iter().next().unwrap().1.moves[0].comment, "one\ntwo");
            let out = stm::serialize(&mask);
            assert_eq!(out, body);
            assert!(!out.contains('\r'));
            assert!(!out.as_bytes().starts_with(&[0xef, 0xbb, 0xbf]));
        }
    }

    #[test]
    fn a_comment_with_carriage_returns_is_normalized_on_write() {
        let mask = one_position(vec![commented("e4", "a\r\nb\rc")], "  padded \n");
        let text = stm::serialize(&mask);
        assert!(!text.contains('\r'));
        let (back, _) = stm::parse(text.as_bytes()).unwrap();
        let (_, p) = back.iter().next().unwrap();
        assert_eq!(p.moves[0].comment, "a\nb\nc");
        assert_eq!(p.comment, "padded");
    }

    #[test]
    fn dead_positions_are_not_written() {
        let mut mask = one_position(vec![MaskMove::new("null")], "");
        mask.insert(MaskKey::Opaque("empty".into()), MaskPosition::default());
        assert_eq!(
            stm::serialize(&mask),
            "set ::tree::mask::maskSerialized {}\n"
        );
        let mask = one_position(vec![MaskMove::new("null")], "kept for its comment");
        assert!(stm::serialize(&mask).contains("null"));
    }

    #[test]
    fn a_file_over_the_limit_is_refused_before_it_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.stm");
        let file = fs::File::create(&path).unwrap();
        // A sparse file: no real data is written or read.
        file.set_len(MAX_STM_BYTES + 1).unwrap();
        drop(file);
        assert!(matches!(
            stm::read(&path),
            Err(StmError::TooLarge { len }) if len == MAX_STM_BYTES + 1
        ));
    }

    #[test]
    fn read_and_write_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rep.stm");
        let mask = one_position(vec![commented("e4", "hello")], "root");
        let size = stm::write(&path, &mask).unwrap();
        assert_eq!(size, fs::metadata(&path).unwrap().len());
        assert_eq!(fs::read(&path).unwrap(), stm::serialize(&mask).into_bytes());
        let (back, report) = stm::read(&path).unwrap();
        assert_eq!(back, mask);
        assert_eq!(report.total_warnings(), 0);
        // Reading the written bytes and writing again changes nothing.
        stm::write(&path, &back).unwrap();
        assert_eq!(fs::read(&path).unwrap(), stm::serialize(&mask).into_bytes());
        // No temporary file stays behind.
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        // A missing file is an i/o error with the path.
        match stm::read(&dir.path().join("missing.stm")) {
            Err(StmError::Io { path, .. }) => assert!(path.ends_with("missing.stm")),
            other => panic!("{other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn write_keeps_permissions_and_follows_symlinks() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.stm");
        let link = dir.path().join("link.stm");
        stm::write(&real, &TreeMask::new()).unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o640)).unwrap();
        symlink(&real, &link).unwrap();
        let mask = one_position(vec![MaskMove::new("e4")], "");
        stm::write(&link, &mask).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(stm::read(&real).unwrap().0, mask);
        let mode = fs::metadata(&real).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_write_keeps_the_old_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("ro");
        fs::create_dir(&sub).unwrap();
        let path = sub.join("m.stm");
        let first = one_position(vec![MaskMove::new("e4")], "");
        stm::write(&path, &first).unwrap();
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o500)).unwrap();
        // Root ignores directory permissions; only check when the write fails.
        let result = stm::write(&path, &TreeMask::new());
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o700)).unwrap();
        if result.is_err() {
            assert_eq!(stm::read(&path).unwrap().0, first);
            assert_eq!(fs::read_dir(&sub).unwrap().count(), 1);
        }
    }
}
