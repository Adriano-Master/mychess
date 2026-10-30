//! M5-4a review fixes (D1850 to D1859): bounded memory when reading, previews
//! of comments, validated values, truncation, UTF-16 and the write checks.

#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::format_push_string,
    clippy::field_reassign_with_default
)]

mod m5_4a_fixes {
    use std::fs;

    use mychess_core::{Position, SanMode};
    use mychess_db::tree::mask::outline::{self, MAX_OUTLINE_DEPTH};
    use mychess_db::tree::mask::search::{self, MaskQuery};
    use mychess_db::tree::mask::stm;
    use mychess_db::tree::mask::{
        MAX_FIELD_BYTES, MAX_POSITIONS, MAX_PREVIEW_BYTES, Marker, MarkerSlot, MaskColor,
        MaskInsertError, MaskKey, MaskMove, MaskNag, MaskOp, MaskPosition, MaskValueError,
        StmError, StmWarningKind, TreeMask,
    };
    use proptest::prelude::*;

    const HEAD: &str = "set ::tree::mask::maskSerialized {";

    #[test]
    fn two_megabytes_of_minimal_pairs_keep_a_bounded_ratio() {
        let mut text = String::from(HEAD);
        let mut n = 0usize;
        while text.len() < 2 * 1024 * 1024 {
            text.push_str(&format!("k{n} {{{{}} {{}}}} "));
            n += 1;
        }
        text.push_str("}\n");
        let (mask, report) = stm::parse(text.as_bytes()).unwrap();
        assert_eq!(mask.len(), n);
        assert_eq!(report.total_warnings(), n, "each key is not a FEN");
        let ratio = mask.approx_bytes() / text.len();
        assert!(ratio <= 40, "{ratio} bytes of structure per byte of file");
    }

    #[test]
    fn a_file_with_too_many_positions_is_cut_with_a_warning() {
        let mut text = String::from(HEAD);
        for i in 0..=MAX_POSITIONS {
            text.push_str(&format!("{i} {{{{}} x}} "));
        }
        text.push_str("}\n");
        let (mask, report) = stm::parse(text.as_bytes()).unwrap();
        assert_eq!(mask.len(), MAX_POSITIONS);
        assert!(mask.is_full());
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.kind == StmWarningKind::TooManyPositions)
        );
        let mut mask = mask;
        let key = MaskKey::Opaque("new".into());
        assert_eq!(
            mask.try_insert(key.clone(), MaskPosition::default()),
            Err(MaskInsertError::Full)
        );
        assert!(!mask.insert(key, MaskPosition::default()));
        assert_eq!(mask.len(), MAX_POSITIONS);
    }

    #[test]
    fn every_strict_prefix_of_a_file_is_an_error() {
        let text = "set ::tree::mask::maskSerialized {{rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -} {{{e4 ! Green {a b} tb_bkm {}}} {root}}}\n";
        for end in 0..text.len() - 1 {
            assert!(stm::parse(&text.as_bytes()[..end]).is_err(), "prefix {end}");
        }
        assert!(stm::parse(text.as_bytes()).is_ok());
    }

    #[test]
    fn utf16_with_bom_is_read_in_both_orders() {
        let text = "set ::tree::mask::maskSerialized {{rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -} {{{e4 {} white caf\u{e9} {} {}}} {}}}\n";
        let units: Vec<u16> = text.encode_utf16().collect();
        let le: Vec<u8> = [0xff, 0xfe]
            .into_iter()
            .chain(units.iter().flat_map(|u| u.to_le_bytes()))
            .collect();
        let be: Vec<u8> = [0xfe, 0xff]
            .into_iter()
            .chain(units.iter().flat_map(|u| u.to_be_bytes()))
            .collect();
        for bytes in [le, be] {
            let (mask, _) = stm::parse(&bytes).unwrap();
            let (_, p) = mask.iter().next().unwrap();
            assert_eq!(p.moves[0].comment, "caf\u{e9}");
        }
    }

    #[test]
    fn escaped_dollar_and_bracket_are_data_but_a_bare_one_is_refused() {
        let ok = |w: &str| stm::parse(format!("set ::tree::mask::maskSerialized {w}\n").as_bytes());
        // `\$` and `\[` are escaped: no substitution, so no foreign command.
        assert!(ok("{}").is_ok());
        assert!(!matches!(
            ok("\\{\\}\\$\\["),
            Err(StmError::ForeignCommand { .. })
        ));
        // An escaped backslash followed by `$` is a substitution.
        assert!(matches!(
            ok("a\\\\$b"),
            Err(StmError::ForeignCommand { .. })
        ));
        assert!(matches!(ok("[x]"), Err(StmError::ForeignCommand { .. })));
    }

    #[test]
    fn constructors_validate() {
        assert_eq!(MaskNag::other("!"), Err(MaskValueError::Reserved));
        assert_eq!(MaskNag::other("  "), Err(MaskValueError::Empty));
        assert_eq!(
            MaskNag::other(&"x".repeat(MAX_FIELD_BYTES + 1)),
            Err(MaskValueError::TooLong)
        );
        assert_eq!(MaskNag::other(" +- "), Ok(MaskNag::Other("+-".into())));
        assert_eq!(MaskColor::other("GREEN"), Err(MaskValueError::Reserved));
        assert_eq!(MaskColor::other(""), Err(MaskValueError::Empty));
        assert_eq!(
            MaskColor::other("#00ff00"),
            Ok(MaskColor::Other("#00ff00".into()))
        );
        assert_eq!(Marker::other("tb_help"), Err(MaskValueError::Reserved));
        assert_eq!(Marker::other("tb_cut"), Err(MaskValueError::Reserved));
        assert_eq!(Marker::other("zz"), Ok(Marker::Other("zz".into())));
        assert_eq!(MaskMove::try_new(" e4\r\n").unwrap().san, "e4");
        assert_eq!(MaskMove::try_new(" \n"), Err(MaskValueError::Empty));
        assert_eq!(MaskMove::new("  e4 ").san, "e4");
        assert_eq!(
            MaskNag::Other("!".into()).normalized(),
            Ok(Some(MaskNag::Good))
        );
        assert_eq!(MaskNag::Other(" ".into()).normalized(), Ok(None));
        assert_eq!(
            MaskColor::Other("red".into()).normalized(),
            Ok(MaskColor::Red)
        );
    }

    #[test]
    fn insert_normalizes_what_the_file_would_give_back() {
        let key = MaskKey::from_position(&Position::new());
        let mut m = MaskMove::new("e4");
        m.nag = Some(MaskNag::Other("!".into()));
        m.color = MaskColor::Other("blue".into());
        m.markers = [Some(Marker::Other("tb_cut".into())), None];
        let mut dup = MaskMove::new("e4+");
        dup.comment = "dropped".into();
        let mut p = MaskPosition::default();
        p.moves = vec![m, dup, MaskMove::new("  ")];
        let mut mask = TreeMask::new();
        mask.insert(key.clone(), p);
        let got = mask.get(&key).unwrap();
        assert_eq!(got.moves.len(), 1);
        assert_eq!(got.moves[0].nag, Some(MaskNag::Good));
        assert_eq!(got.moves[0].color, MaskColor::Blue);
        assert_eq!(got.moves[0].markers[0], Some(Marker::ToRemove));
        let (back, report) = stm::parse(stm::serialize(&mask).as_bytes()).unwrap();
        assert_eq!(report.total_warnings(), 0);
        assert_eq!(back, mask);
    }

    #[test]
    fn dead_positions_do_not_count_for_equality() {
        let mut a = TreeMask::new();
        a.insert(MaskKey::Opaque("k".into()), MaskPosition::default());
        assert_eq!(a, TreeMask::new());
    }

    #[test]
    fn outline_and_search_keep_only_a_preview() {
        let start = Position::new();
        let long = format!("first line \u{e9}{}\nsecond", "\u{e9}".repeat(40_000));
        let mut mask = TreeMask::new();
        for op in [
            MaskOp::SetMoveComment {
                san: "e4".into(),
                text: long.clone(),
            },
            MaskOp::SetPositionComment { text: long },
        ] {
            mask.apply(&start, &op);
        }
        let o = outline::build(&mask, &start, outline::OutlineBudget::default());
        let step = &o.roots[0].steps[0];
        for c in [&o.comment, &step.comment, &step.position_comment] {
            assert!(c.len() <= MAX_PREVIEW_BYTES && c.starts_with("first line"));
            assert!(!c.contains('\n'));
        }
        let hits = search::run(&mask, &MaskQuery::default());
        assert!(
            hits.rows
                .iter()
                .all(|r| r.comment.len() <= MAX_PREVIEW_BYTES)
        );
        // The full comment stays in the mask.
        assert!(mask.position(&start).unwrap().moves[0].comment.len() > 60_000);
        const { assert!(MAX_OUTLINE_DEPTH >= 100) };
        let deep = outline::build(
            &mask,
            &start,
            outline::OutlineBudget {
                max_nodes: usize::MAX,
                max_depth: usize::MAX,
            },
        );
        assert_eq!(deep.roots.len(), 1);
    }

    #[test]
    fn a_read_only_destination_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.stm");
        let mask = TreeMask::new();
        stm::write(&path, &mask).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&path, perms).unwrap();
        let result = stm::write(&path, &mask);
        let mut perms = fs::metadata(&path).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        fs::set_permissions(&path, perms).unwrap();
        assert!(
            matches!(result, Err(StmError::ReadOnly { .. })),
            "{result:?}"
        );
        assert!(stm::write_report(&path, &mask).unwrap().permissions_kept);
    }

    fn special() -> impl Strategy<Value = String> {
        prop::collection::vec(
            prop::sample::select(vec![
                '{',
                '}',
                '\\',
                '[',
                ']',
                '$',
                ';',
                '"',
                '#',
                ' ',
                '\n',
                '\r',
                '\t',
                '\u{e9}',
                '\u{1f600}',
                'a',
                'x',
                '!',
                '?',
                '+',
                '=',
                '-',
                'O',
                '0',
            ]),
            0..8,
        )
        .prop_map(|v| v.into_iter().collect())
    }

    fn op() -> impl Strategy<Value = (usize, MaskOp)> {
        (0usize..2, special(), special(), 0usize..6).prop_map(|(pos, a, b, kind)| {
            let op = match kind {
                0 => MaskOp::AddMove { san: a },
                1 => MaskOp::SetMoveComment { san: a, text: b },
                2 => MaskOp::SetPositionComment { text: b },
                3 => MaskOp::SetNag {
                    san: a,
                    nag: Some(MaskNag::Other(b)),
                },
                4 => MaskOp::SetColor {
                    san: a,
                    color: MaskColor::Other(b),
                },
                _ => MaskOp::SetMarker {
                    san: a,
                    slot: MarkerSlot::Second,
                    marker: Some(Marker::Other(b)),
                },
            };
            (pos, op)
        })
    }

    proptest! {
        #[test]
        fn masks_built_with_awkward_values_round_trip(ops in prop::collection::vec(op(), 0..12)) {
            let start = Position::new();
            let mut after = start.clone();
            let mv = start.parse_san("e4", SanMode::Strict).unwrap().mv;
            after.play(mv).unwrap();
            let mut mask = TreeMask::new();
            for (pos, op) in &ops {
                mask.apply(if *pos == 0 { &start } else { &after }, op);
            }
            let text = stm::serialize(&mask);
            let (back, _) = stm::parse(text.as_bytes()).unwrap();
            prop_assert_eq!(&back, &mask);
            prop_assert_eq!(stm::serialize(&back), text);
        }

        #[test]
        fn opaque_keys_and_inserted_values_round_trip(
            key in special(), san in special(), nag in special(), color in special(), c in special()
        ) {
            let mut m = MaskMove::new(san);
            m.nag = Some(MaskNag::Other(nag));
            m.color = MaskColor::Other(color);
            m.comment = c.clone();
            let mut p = MaskPosition::default();
            p.moves.push(m);
            p.comment = c;
            let mut mask = TreeMask::new();
            mask.insert(MaskKey::Opaque(key), p);
            let (back, _) = stm::parse(stm::serialize(&mask).as_bytes()).unwrap();
            prop_assert_eq!(back, mask);
        }
    }
}
