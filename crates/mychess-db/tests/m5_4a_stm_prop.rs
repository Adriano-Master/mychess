//! M5-4a: properties of the `.stm` reader and writer: read(write(m)) == m for
//! random masks with awkward texts, and no panic for random input.

#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::assert_is_empty,
    clippy::field_reassign_with_default,
    clippy::too_many_lines
)]

// The module name lets `cargo test -p mychess-db m5_4a` select these tests.
mod m5_4a {

    use mychess_db::tree::mask::stm;
    use mychess_db::tree::mask::{
        Marker, MaskColor, MaskKey, MaskMove, MaskNag, MaskPosition, StmWarningKind, TreeMask,
        clean_comment,
    };
    use proptest::prelude::*;

    const FENS: [&str; 4] = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -",
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq -",
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq -",
        "4k3/8/8/8/8/8/4P3/4K3 w - -",
    ];

    /// Text with the characters that matter to Tcl lists, newlines, accents and
    /// astral characters.
    fn text() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            prop_oneof![
                4 => proptest::char::range('a', 'f'),
                2 => Just(' '),
                2 => Just('\n'),
                1 => Just('\t'),
                2 => Just('{'),
                2 => Just('}'),
                2 => Just('\\'),
                1 => Just('"'),
                1 => Just('$'),
                1 => Just('['),
                1 => Just(']'),
                1 => Just(';'),
                1 => Just('#'),
                1 => Just('\''),
                1 => Just('\u{e9}'),
                1 => Just('\u{4e2d}'),
                1 => Just('\u{1f600}'),
            ],
            0..24,
        )
        .prop_map(|chars| chars.into_iter().collect())
    }

    fn key() -> impl Strategy<Value = MaskKey> {
        prop_oneof![
            3 => proptest::sample::select(FENS.to_vec()).prop_map(MaskKey::from_text),
            1 => text().prop_map(|t| MaskKey::from_text(&t)),
        ]
    }

    fn mask_move(i: usize) -> impl Strategy<Value = MaskMove> {
        (
            text(),
            proptest::option::of(prop_oneof![
                proptest::sample::select(MaskNag::STANDARD.to_vec()),
                text().prop_filter_map("no nag text", |t| MaskNag::from_text(&t)),
            ]),
            prop_oneof![
                proptest::sample::select(MaskColor::STANDARD.to_vec()),
                text().prop_map(|t| MaskColor::from_text(&t)),
            ],
            text().prop_map(|t| clean_comment(&t).0),
            proptest::array::uniform2(proptest::option::of(prop_oneof![
                proptest::sample::select(Marker::ALL.to_vec()),
                text().prop_filter_map("no marker", |t| Marker::from_text(&t)),
            ])),
        )
            .prop_map(move |(suffix, nag, color, comment, markers)| MaskMove {
                // The prefix keeps the moves of a position distinct (D1805).
                san: format!("m{i}_{}", suffix.trim()),
                nag,
                color,
                comment,
                markers,
            })
    }

    fn position() -> impl Strategy<Value = MaskPosition> {
        (0usize..6, text().prop_map(|t| clean_comment(&t).0)).prop_flat_map(|(n, comment)| {
            let moves: Vec<_> = (0..n).map(mask_move).collect();
            moves.prop_map(move |moves| MaskPosition {
                moves,
                comment: comment.clone(),
            })
        })
    }

    fn mask() -> impl Strategy<Value = TreeMask> {
        proptest::collection::vec((key(), position()), 0..6).prop_map(|entries| {
            let mut mask = TreeMask::new();
            for (key, position) in entries {
                if !position.is_dead() {
                    mask.insert(key, position);
                }
            }
            mask
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(400))]

        #[test]
        fn read_of_write_is_the_same_mask(m in mask()) {
            let text = stm::serialize(&m);
            let (back, report) = stm::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}\n{text}"));
            // Only the "kept as it is" notices are expected.
            let only_notices = report.warnings.iter().all(|w| {
                matches!(
                    w.kind,
                    StmWarningKind::UnknownNag
                        | StmWarningKind::UnknownColor
                        | StmWarningKind::UnknownMarker
                        | StmWarningKind::KeyNotFen
                )
            });
            prop_assert!(only_notices, "{:?}\n{}", report.warnings, text);
            prop_assert_eq!(&back, &m, "\n{}", text);
            // The second write is byte-identical: the canonical form is stable.
            prop_assert_eq!(stm::serialize(&back), text);
        }

        #[test]
        fn crlf_and_bom_do_not_change_the_result(m in mask()) {
            let text = stm::serialize(&m);
            let mut decorated = vec![0xef, 0xbb, 0xbf];
            decorated.extend_from_slice(text.replace('\n', "\r\n").as_bytes());
            let (back, _) = stm::parse(&decorated).unwrap();
            prop_assert_eq!(back, m);
        }

        #[test]
        fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
            drop(stm::parse(&bytes));
        }

        #[test]
        fn random_text_after_the_header_never_panics(body in text(), tail in text()) {
            let text = format!("set ::tree::mask::maskSerialized {{{body}}}{tail}\n");
            drop(stm::parse(text.as_bytes()));
            let text = format!("set ::tree::mask::maskSerialized {body}\n");
            drop(stm::parse(text.as_bytes()));
        }

        #[test]
        fn truncated_files_never_panic(m in mask(), cut in 0usize..2000) {
            let text = stm::serialize(&m);
            let cut = cut.min(text.len());
            drop(stm::parse(&text.as_bytes()[..cut]));
        }
    }
}
