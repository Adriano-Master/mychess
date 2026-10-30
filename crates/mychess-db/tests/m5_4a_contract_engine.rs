//! M5-4a contract tests for the mask engine (D1805, D1821, D1823-D1826,
//! D1828, D1832, D1833, D1843, D1847-D1849): move equivalence, overlay,
//! outline budgets and cycles, search limits and cancellable filling.

#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

// The module name lets `cargo test -p mychess-db m5_4a` select these tests.
mod m5_4a_contract {

    use std::fmt::Write as _;

    use mychess_core::{Game, NodeId, Position, SanMode};
    use mychess_db::filter::Filter;
    use mychess_db::pgn_db::{OpenOptions, PgnDatabase};
    use mychess_db::progress::{CancelToken, NoProgress};
    use mychess_db::source::GameSource;
    use mychess_db::tree::mask::fill::{FillError, FillSide, feed_filter, feed_game, feed_line};
    use mychess_db::tree::mask::outline::{self, OutlineBudget, OutlineNode};
    use mychess_db::tree::mask::search::{self, MAX_HITS, MaskQuery};
    use mychess_db::tree::mask::{
        MAX_COMMENT_BYTES, MaskKey, MaskMove, MaskNag, MaskOp, MaskPosition, MoveMatcher, TreeMask,
        mask_san, resolve_move,
    };
    use proptest::prelude::*;

    fn pos(fen: &str) -> Position {
        Position::from_fen(fen).unwrap()
    }

    fn after(sans: &[&str]) -> Position {
        let mut p = Position::new();
        for s in sans {
            let mv = p.parse_san(s, SanMode::Strict).unwrap().mv;
            p.play(mv).unwrap();
        }
        p
    }

    fn add(san: &str) -> MaskOp {
        MaskOp::AddMove { san: san.into() }
    }

    fn add_line(mask: &mut TreeMask, sans: &[&str]) {
        let mut p = Position::new();
        for s in sans {
            let mv = p.parse_san(s, SanMode::Strict).unwrap().mv;
            mask.apply(&p, &add(&mask_san(&p, mv).unwrap()));
            p.play(mv).unwrap();
        }
    }

    fn sans_of(n: &OutlineNode) -> Vec<&str> {
        n.steps.iter().map(|s| s.san.as_str()).collect()
    }

    fn count_steps(nodes: &[OutlineNode]) -> usize {
        nodes
            .iter()
            .map(|n| n.steps.len() + count_steps(&n.children))
            .sum()
    }

    // -----------------------------------------------------------------------
    // D1805 / D1843: equivalence of move texts
    // -----------------------------------------------------------------------

    #[test]
    fn castling_spellings_are_one_move_and_the_two_sides_differ() {
        let m = MoveMatcher::new(None);
        for short in ["O-O", "0-0", "OO", "OK", "O-O+", "O-O#", "0-0+", "O-O!?"] {
            for other in ["O-O", "0-0", "OO", "OK"] {
                assert!(m.same(short, other), "{short} vs {other}");
            }
            for long in ["O-O-O", "0-0-0", "OQ"] {
                assert!(!m.same(short, long), "{short} vs {long}");
            }
        }
        for long in ["O-O-O", "0-0-0", "OQ", "O-O-O+", "0-0-0#"] {
            for other in ["O-O-O", "0-0-0", "OQ"] {
                assert!(m.same(long, other), "{long} vs {other}");
            }
        }
    }

    #[test]
    fn check_mate_and_annotation_suffixes_do_not_matter() {
        let m = MoveMatcher::new(None);
        for (a, b) in [
            ("Qh5", "Qh5+"),
            ("Qh5+", "Qh5#"),
            ("Qxf7+", "Qxf7#"),
            ("Qxf7", "Qxf7#"),
            ("e4", "e4!"),
            ("e4", "e4?!"),
            ("e4", " e4 "),
        ] {
            assert!(m.same(a, b), "{a} vs {b}");
            assert!(m.same(b, a), "{b} vs {a}");
        }
    }

    #[test]
    fn move_texts_are_compared_literally_never_as_a_regex() {
        let m = MoveMatcher::new(None);
        for (a, b) in [
            ("Q.5", "Qh5"),
            (".*", "e4"),
            ("e4", "e4+e4"),
            ("O.O", "O-O"),
            ("[a-h]4", "e4"),
            ("e4", "e5"),
            ("", "e4"),
        ] {
            assert!(!m.same(a, b), "{a:?} vs {b:?}");
        }
        // Via the mask: a pattern is a different move, never a hit.
        let mut mask = TreeMask::new();
        let start = Position::new();
        mask.apply(&start, &add("e4"));
        assert!(mask.lookup_move(&start, ".*").is_none());
        assert!(mask.lookup_move(&start, "e.").is_none());
        assert!(mask.apply(&start, &add("e.")), "a different text is added");
    }

    #[test]
    fn missing_capture_and_promotion_marks_resolve_through_the_position() {
        // Nf7 (Scid "add move" menu) is Nxf7.
        let p = pos("4k3/5p2/8/4N3/8/8/8/4K3 w - - 0 1");
        let m = MoveMatcher::new(Some(&p));
        assert!(m.same("Nf7", "Nxf7"));
        assert!(m.same("Nxf7", "Nf7"));
        assert!(m.same("Nf7+", "Nxf7"));
        assert!(!m.same("Ng6", "Nxf7"));
        assert!(
            MoveMatcher::new(None).same("Nf7", "Nxf7"),
            "without a position the legality check is skipped (D1843)"
        );
        // e8Q is e8=Q.
        let p = pos("k7/4P3/8/8/8/8/8/4K3 w - - 0 1");
        let m = MoveMatcher::new(Some(&p));
        for other in ["e8=Q", "e8=Q+", "e8Q", "e8=Q#"] {
            assert!(m.same("e8Q", other), "{other}");
        }
        assert!(!m.same("e8Q", "e8=R"));
        assert!(!m.same("e8Q", "e8=N"));
        assert!(!m.same("e8Q", "e7"));
        // axb8Q is axb8=Q.
        let p = pos("1n5k/P7/8/8/8/8/8/4K3 w - - 0 1");
        let m = MoveMatcher::new(Some(&p));
        assert!(m.same("axb8Q", "axb8=Q"));
        assert!(m.same("ab8Q", "axb8=Q"));
        assert!(!m.same("axb8Q", "axb8=N"));
    }

    #[test]
    fn resolve_move_follows_legality_for_castling() {
        let both = pos("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        for t in ["O-O", "0-0", "OO", "OK", "O-O+"] {
            assert!(resolve_move(&both, t).is_some(), "{t}");
        }
        for t in ["O-O-O", "0-0-0", "OQ"] {
            assert!(resolve_move(&both, t).is_some(), "{t}");
        }
        assert_eq!(resolve_move(&both, "O-O"), resolve_move(&both, "OK"));
        assert_ne!(resolve_move(&both, "O-O"), resolve_move(&both, "OQ"));
        // No right: not legal.
        let no_long = pos("r3k2r/8/8/8/8/8/8/R3K2R w Kkq - 0 1");
        assert!(resolve_move(&no_long, "O-O-O").is_none());
        assert!(resolve_move(&no_long, "OQ").is_none());
        // Through an attacked square: not legal.
        let attacked = pos("5r2/1k6/8/8/8/8/8/R3K2R w KQ - 0 1");
        assert!(resolve_move(&attacked, "O-O").is_none());
        assert!(resolve_move(&attacked, "OK").is_none());
        assert!(resolve_move(&attacked, "O-O-O").is_some());
        // Garbage and the null move are never a move.
        for t in ["", " ", "null", "Z9", "e9", "O-O-O-O", "\0", "e4\u{e9}"] {
            assert!(resolve_move(&both, t).is_none(), "{t:?}");
        }
    }

    #[test]
    fn a_pgn_null_move_text_is_not_a_legal_move_of_the_mask() {
        let p = Position::new();
        for t in ["--", "Z0", "@@@@"] {
            let resolved = resolve_move(&p, t);
            assert!(
                resolved.is_none(),
                "{t:?} must not resolve to a legal move: {resolved:?}"
            );
            let mut mask = TreeMask::new();
            mask.apply(&p, &add(t));
            let o = mask.overlay(&p, &[]);
            assert!(o.only_mask.iter().all(|m| m.legal.is_none()), "{t:?}");
        }
    }

    #[test]
    fn castling_entries_keep_their_spelling_and_are_not_duplicated() {
        let p = pos("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        let mut mask = TreeMask::new();
        assert!(mask.apply(&p, &add("0-0")));
        for same in ["O-O", "OK", "OO", "O-O+", "0-0"] {
            assert!(!mask.apply(&p, &add(same)), "{same} is the same entry");
        }
        let entry = mask.lookup_move(&p, "O-O").unwrap();
        assert_eq!(entry.san, "0-0", "an entry read as is stays as it came");
        assert!(
            mask.apply(&p, &add("O-O-O")),
            "the long side is another move"
        );
        assert_eq!(mask.position(&p).unwrap().moves.len(), 2);
        // Editing through another spelling edits the same entry.
        assert!(mask.apply(
            &p,
            &MaskOp::SetNag {
                san: "OK".into(),
                nag: Some(MaskNag::Good)
            }
        ));
        assert_eq!(mask.position(&p).unwrap().moves.len(), 2);
        assert_eq!(mask.position(&p).unwrap().moves[0].nag, Some(MaskNag::Good));
        // Removing through another spelling removes it.
        assert!(mask.apply(&p, &MaskOp::RemoveMove { san: "O-O".into() }));
        assert_eq!(mask.position(&p).unwrap().moves.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Illegal entries, hostile edits
    // -----------------------------------------------------------------------

    #[test]
    fn an_illegal_entry_is_kept_edited_and_removed_like_any_other() {
        let start = Position::new();
        let mut mask = TreeMask::new();
        assert!(mask.apply(&start, &add("Qd4")));
        assert!(mask.lookup_move(&start, "Qd4").is_some());
        assert!(mask.apply(
            &start,
            &MaskOp::SetNag {
                san: "Qd4".into(),
                nag: Some(MaskNag::Blunder)
            }
        ));
        assert!(!mask.apply(
            &start,
            &MaskOp::SetNag {
                san: "Qd4".into(),
                nag: Some(MaskNag::Blunder)
            }
        ));
        assert!(mask.apply(&start, &MaskOp::RemoveMove { san: "Qd4".into() }));
        assert!(mask.is_empty(), "the position goes away with its last move");
        assert!(!mask.apply(&start, &MaskOp::RemoveMove { san: "Qd4".into() }));
    }

    #[test]
    fn overlay_lists_an_illegal_entry_as_only_mask_with_no_move() {
        let start = Position::new();
        let mut mask = TreeMask::new();
        for s in ["e4", "Qd4", "Nf3", "Ke2"] {
            mask.apply(&start, &add(s));
        }
        let o = mask.overlay(&start, &["e4"]);
        assert_eq!(o.rows.len(), 1);
        assert_eq!(o.rows[0].unwrap().san, "e4");
        let only: Vec<_> = o
            .only_mask
            .iter()
            .map(|m| (m.entry.san.as_str(), m.legal.is_some()))
            .collect();
        assert_eq!(
            only,
            [("Qd4", false), ("Nf3", true), ("Ke2", false)],
            "mask order"
        );
    }

    #[test]
    fn overlay_matches_tree_rows_by_equivalence_and_never_repeats_them() {
        // Mask has "Ne5" (no x), the tree row says "Nxe5".
        let p = after(&["e4", "e5", "Nf3", "d6"]);
        let mut mask = TreeMask::new();
        mask.apply(&p, &add("Ne5"));
        mask.apply(&p, &add("d4"));
        let o = mask.overlay(&p, &["Nxe5", "Bc4+", "d4"]);
        assert_eq!(o.rows[0].unwrap().san, "Ne5");
        assert!(o.rows[1].is_none());
        assert_eq!(o.rows[2].unwrap().san, "d4");
        assert!(o.only_mask.is_empty(), "{:?}", o.only_mask);
        // Duplicated tree rows both get the entry.
        let o = mask.overlay(&p, &["d4", "d4"]);
        assert!(o.rows.iter().all(Option::is_some));
        // A position the mask does not know: nothing, no panic.
        let o = mask.overlay(&Position::new(), &["e4", "", "x"]);
        assert_eq!(o.comment, "");
        assert!(o.rows.iter().all(Option::is_none));
        assert!(o.only_mask.is_empty());
        // An empty or hostile tree text matches nothing.
        let o = mask.overlay(&p, &["", ".*", "null"]);
        assert!(o.rows.iter().all(Option::is_none));
        assert_eq!(o.only_mask.len(), 2);
    }

    #[test]
    fn hostile_edit_texts_never_panic_and_comments_are_limited() {
        let start = Position::new();
        let mut mask = TreeMask::new();
        for san in [
            "",
            " ",
            "\0",
            "{",
            "e4\n",
            "\u{1f600}",
            &"x".repeat(100_000),
        ] {
            let _ = mask.apply(&start, &add(san));
        }
        for (_, p) in mask.iter() {
            assert!(
                p.moves.iter().all(|m| !m.san.trim().is_empty()),
                "no empty SAN"
            );
        }
        let huge = "\u{e9}".repeat(MAX_COMMENT_BYTES);
        mask.apply(&start, &add("e4"));
        mask.apply(
            &start,
            &MaskOp::SetMoveComment {
                san: "e4".into(),
                text: huge.clone(),
            },
        );
        mask.apply(&start, &MaskOp::SetPositionComment { text: huge });
        let p = mask.position(&start).unwrap();
        assert!(p.comment.len() <= MAX_COMMENT_BYTES);
        assert!(!p.comment.is_empty() && p.comment.chars().all(|c| c == '\u{e9}'));
        let m = p.moves.iter().find(|m| m.san == "e4").unwrap();
        assert!(m.comment.len() <= MAX_COMMENT_BYTES);
        // A comment of only blanks is empty, and removes an empty position.
        let mut only = TreeMask::new();
        only.apply(
            &start,
            &MaskOp::SetPositionComment {
                text: " \n\t ".into(),
            },
        );
        assert!(only.is_empty());
        only.apply(&start, &MaskOp::SetPositionComment { text: "x".into() });
        assert_eq!(only.len(), 1);
        only.apply(&start, &MaskOp::SetPositionComment { text: "  ".into() });
        assert!(
            only.is_empty(),
            "empty comment, no moves: the position goes away"
        );
    }

    // -----------------------------------------------------------------------
    // Outline
    // -----------------------------------------------------------------------

    #[test]
    fn outline_expands_both_orders_of_a_transposition() {
        let mut mask = TreeMask::new();
        add_line(&mut mask, &["Nf3", "Nf6", "c4", "e6"]);
        add_line(&mut mask, &["c4", "Nf6", "Nf3", "e6"]);
        let o = outline::build(&mask, &Position::new(), OutlineBudget::default());
        assert_eq!(o.roots.len(), 2);
        assert_eq!(sans_of(&o.roots[0]), ["Nf3", "Nf6", "c4", "e6"]);
        assert_eq!(sans_of(&o.roots[1]), ["c4", "Nf6", "Nf3", "e6"]);
        assert_eq!((o.nodes, o.truncated), (8, false));
        assert_eq!(count_steps(&o.roots), o.nodes);
    }

    #[test]
    fn outline_keeps_siblings_around_a_cycle_at_any_depth() {
        let mut mask = TreeMask::new();
        add_line(&mut mask, &["e4", "Nf6", "Nc3", "Ng8", "Nb1", "Nf6"]); // back to a seen position
        add_line(&mut mask, &["e4", "e5"]);
        add_line(&mut mask, &["d4"]);
        let o = outline::build(&mask, &Position::new(), OutlineBudget::default());
        assert!(!o.truncated);
        assert_eq!(count_steps(&o.roots), o.nodes);
        assert_eq!(o.roots.len(), 2, "e4 and d4");
        let e4 = &o.roots[0];
        assert_eq!(sans_of(e4), ["e4"]);
        let kids: Vec<_> = e4.children.iter().map(|c| sans_of(c)[0]).collect();
        assert_eq!(kids, ["Nf6", "e5"], "the sibling e5 is kept");
    }

    #[test]
    fn outline_budget_boundaries() {
        let mut mask = TreeMask::new();
        add_line(&mut mask, &["Nf3", "Nf6", "Ng1", "Ng8"]); // 4 steps, then a cycle
        for max in 0..8usize {
            let o = outline::build(
                &mask,
                &Position::new(),
                OutlineBudget {
                    max_nodes: max,
                    max_depth: 200,
                },
            );
            assert!(o.nodes <= max, "max {max}: {} nodes", o.nodes);
            assert_eq!(count_steps(&o.roots), o.nodes, "max {max}");
            assert!(o.roots.iter().all(|r| !r.steps.is_empty()), "no empty node");
            assert_eq!(o.truncated, max < 4, "max {max}");
        }
    }

    #[test]
    fn outline_depth_boundaries() {
        let line = [
            "e4", "e5", "Nf3", "Nc6", "Bb5", "a6", "Ba4", "Nf6", "O-O", "Be7",
        ];
        let mut mask = TreeMask::new();
        add_line(&mut mask, &line);
        for depth in [0usize, 1, 5, 9, 10, 11, 200] {
            let o = outline::build(
                &mask,
                &Position::new(),
                OutlineBudget {
                    max_nodes: 20_000,
                    max_depth: depth,
                },
            );
            assert!(o.roots.iter().all(|r| !r.steps.is_empty()), "depth {depth}");
            assert_eq!(o.nodes, depth.min(10), "depth {depth}");
            assert_eq!(count_steps(&o.roots), o.nodes);
            assert_eq!(o.truncated, depth < 10, "depth {depth}");
        }
    }

    #[test]
    fn outline_of_everything_respects_a_small_budget_through_transpositions() {
        // Every legal move for the first three plies: many transpositions.
        let mut mask = TreeMask::new();
        let mut layer = vec![Position::new()];
        for _ in 0..3 {
            let mut next = Vec::new();
            for p in &layer {
                for mv in p.legal_moves() {
                    mask.apply(p, &add(&mask_san(p, mv).unwrap()));
                    let mut q = p.clone();
                    q.play(mv).unwrap();
                    next.push(q);
                }
            }
            layer = next;
            layer.sort_by_key(Position::fen);
            layer.dedup_by_key(|p| p.fen());
        }
        for max in [1usize, 25, 777, 5_000] {
            let o = outline::build(
                &mask,
                &Position::new(),
                OutlineBudget {
                    max_nodes: max,
                    max_depth: 200,
                },
            );
            assert!(o.nodes <= max);
            assert_eq!(count_steps(&o.roots), o.nodes);
            assert!(o.truncated, "max {max}: the full outline is bigger");
        }
    }

    #[test]
    fn outline_edge_cases() {
        let mask = TreeMask::new();
        let o = outline::build(&mask, &Position::new(), OutlineBudget::default());
        assert!(o.roots.is_empty() && o.nodes == 0 && !o.truncated && o.comment.is_empty());
        // Illegal entry and null: null is hidden, the illegal one has no `after`.
        let start = Position::new();
        let mut mask = TreeMask::new();
        mask.apply(&start, &add("null"));
        mask.apply(&start, &add("Qd4"));
        mask.apply(
            &start,
            &MaskOp::SetPositionComment {
                text: "root".into(),
            },
        );
        let o = outline::build(&mask, &start, OutlineBudget::default());
        assert_eq!(o.comment, "root");
        assert_eq!(o.roots.len(), 1);
        assert_eq!(o.roots[0].steps[0].san, "Qd4");
        assert!(o.roots[0].steps[0].after.is_none());
        // A position that is not in the mask.
        let o = outline::build(&mask, &after(&["e4"]), OutlineBudget::default());
        assert!(o.roots.is_empty());
        // The step records the key of the position reached.
        let mut mask = TreeMask::new();
        add_line(&mut mask, &["e4"]);
        let o = outline::build(&mask, &start, OutlineBudget::default());
        assert_eq!(
            o.roots[0].steps[0].after,
            Some(MaskKey::from_position(&after(&["e4"])))
        );
    }

    // -----------------------------------------------------------------------
    // Search
    // -----------------------------------------------------------------------

    fn mask_with_moves(total: usize) -> TreeMask {
        let mut mask = TreeMask::new();
        let mut left = total;
        let mut i = 0;
        while left > 0 {
            let n = left.min(400);
            mask.insert(
                MaskKey::Opaque(format!("k{i}")),
                MaskPosition {
                    moves: (0..n).map(|j| MaskMove::new(format!("m{j}"))).collect(),
                    comment: String::new(),
                },
            );
            left -= n;
            i += 1;
        }
        mask
    }

    #[test]
    fn search_limit_boundaries_count_everything_and_flag_the_cut() {
        for (total, truncated) in [
            (MAX_HITS - 1, false),
            (MAX_HITS, false),
            (MAX_HITS + 1, true),
        ] {
            let hits = search::run(&mask_with_moves(total), &MaskQuery::default());
            assert_eq!(hits.truncated, truncated, "{total}");
            assert_eq!(hits.rows.len(), total.min(MAX_HITS));
            assert_eq!(hits.moves_found, total, "the count is complete");
            assert_eq!(hits.moves_total, total);
        }
    }

    #[test]
    fn search_rows_keep_file_order_and_skip_null() {
        let mut mask = TreeMask::new();
        for (i, k) in ["zz", "aa", "mm"].iter().enumerate() {
            mask.insert(
                MaskKey::Opaque((*k).into()),
                MaskPosition {
                    moves: vec![MaskMove::new("null"), MaskMove::new(format!("m{i}"))],
                    comment: String::new(),
                },
            );
        }
        let hits = search::run(&mask, &MaskQuery::default());
        let rows: Vec<_> = hits
            .rows
            .iter()
            .map(|r| (r.key.as_str().to_string(), r.san.clone().unwrap()))
            .collect();
        assert_eq!(
            rows,
            [
                ("zz".into(), "m0".into()),
                ("aa".into(), "m1".into()),
                ("mm".into(), "m2".into())
            ]
        );
        assert_eq!(hits.moves_total, 3, "null is not counted");
        // A query that names `null`'s comment finds nothing.
        let q = MaskQuery {
            move_text: Some(String::new()),
            ..MaskQuery::default()
        };
        assert_eq!(search::run(&mask, &q).moves_found, 3);
    }

    #[test]
    fn search_text_is_literal_and_case_insensitive() {
        let mut mask = TreeMask::new();
        let mut m = MaskMove::new("e4");
        m.comment = "Note: a*b? [x] \u{c9}COLE".into();
        mask.insert(
            MaskKey::Opaque("k".into()),
            MaskPosition {
                moves: vec![m],
                comment: "Pos [A]".into(),
            },
        );
        let find = |t: &str| {
            let q = MaskQuery {
                move_text: Some(t.into()),
                ..MaskQuery::default()
            };
            search::run(&mask, &q).moves_found
        };
        assert_eq!(find("a*b?"), 1);
        assert_eq!(find("A*B?"), 1);
        assert_eq!(find("[x]"), 1);
        assert_eq!(find("\u{e9}cole"), 1, "non-ASCII case folding");
        for wildcard in ["a*", "*", "?", "[a-z]", "a?b", ".*", "a.b"] {
            let expect = usize::from(wildcard == "*" || wildcard == "?" || wildcard == "a*");
            assert_eq!(find(wildcard), expect, "{wildcard} is not a pattern");
        }
        let q = MaskQuery {
            position_text: Some("[a]".into()),
            ..MaskQuery::default()
        };
        let hits = search::run(&mask, &q);
        assert_eq!(hits.positions_found, 1);
        assert_eq!(hits.rows[0].san, None);
        let q = MaskQuery {
            position_text: Some("nothing".into()),
            ..MaskQuery::default()
        };
        let hits = search::run(&mask, &q);
        assert!(hits.rows.is_empty() && hits.positions_found == 0);
        assert_eq!(hits.positions_total, 1);
    }

    // -----------------------------------------------------------------------
    // Filling
    // -----------------------------------------------------------------------

    fn five_games() -> PgnDatabase {
        let mut pgn = String::new();
        for (white, mv) in [
            ("A", "e4"),
            ("B", "d4"),
            ("C", "c4"),
            ("D", "Nf3"),
            ("E", "g3"),
        ] {
            write!(pgn,
                "[Event \"?\"]\n[White \"{white}\"]\n[Result \"*\"]\n\n1. {mv} {{ played by {white} }} *\n\n"
            ).unwrap();
        }
        let (db, _) = PgnDatabase::from_reader(
            pgn.as_bytes(),
            std::path::Path::new("five.pgn"),
            &OpenOptions::default(),
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
        db
    }

    fn first_moves(mask: &TreeMask) -> Vec<String> {
        mask.position(&Position::new())
            .map(|p| p.moves.iter().map(|m| m.san.clone()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn feed_filter_cancelled_in_the_middle_keeps_the_partial_result() {
        let mut db = five_games();
        let n = db.len();
        assert_eq!(n, 5);
        let mut filter = Filter::new(n);
        filter.fill(1);
        let cancel = CancelToken::new();
        let mut ticks = Vec::new();
        let mut sink = |done: u64, total: u64| {
            ticks.push((done, total));
            if done == 2 {
                cancel.cancel();
            }
        };
        let mut mask = TreeMask::new();
        let r = feed_filter(
            &mut mask,
            &mut db,
            &filter,
            FillSide::All,
            &cancel,
            &mut sink,
        )
        .unwrap();
        assert!(r.cancelled);
        assert!(r.changed, "the partial result counts as a change");
        assert_eq!((r.games, r.skipped), (2, 0));
        assert_eq!(ticks, [(1, 5), (2, 5)], "no progress after the cancel");
        assert_eq!(first_moves(&mask), ["e4", "d4"], "the two first games stay");
        // Running again without the cancel completes and keeps everything.
        let r = feed_filter(
            &mut mask,
            &mut db,
            &filter,
            FillSide::All,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
        assert!(!r.cancelled);
        assert_eq!(first_moves(&mask), ["e4", "d4", "c4", "Nf3", "g3"]);
    }

    #[test]
    fn feed_filter_cancelled_on_the_last_game_is_not_flagged() {
        let mut db = five_games();
        let mut filter = Filter::new(db.len());
        filter.fill(1);
        let cancel = CancelToken::new();
        let mut sink = |done: u64, _total: u64| {
            if done == 5 {
                cancel.cancel();
            }
        };
        let mut mask = TreeMask::new();
        let r = feed_filter(
            &mut mask,
            &mut db,
            &filter,
            FillSide::All,
            &cancel,
            &mut sink,
        )
        .unwrap();
        assert_eq!(r.games, 5);
        assert!(!r.cancelled, "nothing was left to cancel");
    }

    #[test]
    fn feed_filter_uses_only_the_games_of_the_filter_and_the_side() {
        let mut db = five_games();
        let mut filter = Filter::new(db.len());
        filter.fill(0);
        filter.set(1, 1);
        filter.set(3, 1);
        let mut mask = TreeMask::new();
        let mut ticks = Vec::new();
        let mut sink = |d: u64, t: u64| ticks.push((d, t));
        let r = feed_filter(
            &mut mask,
            &mut db,
            &filter,
            FillSide::All,
            &CancelToken::new(),
            &mut sink,
        )
        .unwrap();
        assert_eq!(r.games, 2);
        assert_eq!(ticks, [(1, 2), (2, 2)], "total is the size of the filter");
        assert_eq!(first_moves(&mask), ["e4", "c4"], "game numbers are 1-based");
        // Black has no move in these one-move games: nothing is added.
        let mut black = TreeMask::new();
        let r = feed_filter(
            &mut black,
            &mut db,
            &filter,
            FillSide::Black,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
        assert_eq!(r.games, 2);
        assert!(!r.changed);
        assert!(black.is_empty());
        // An empty filter: no tick, no change, Ok.
        let mut empty = Filter::new(db.len());
        empty.fill(0);
        let mut m = TreeMask::new();
        let mut called = false;
        let mut sink = |_: u64, _: u64| called = true;
        let r = feed_filter(
            &mut m,
            &mut db,
            &empty,
            FillSide::All,
            &CancelToken::new(),
            &mut sink,
        )
        .unwrap();
        assert!(!called && !r.changed && !r.cancelled && m.is_empty());
    }

    #[test]
    fn feed_filter_skips_games_that_are_not_standard_and_counts_them() {
        let pgn = "[Event \"?\"]\n[Result \"*\"]\n\n1. e4 *\n\n\
                   [Event \"?\"]\n[Variant \"Chess960\"]\n[SetUp \"1\"]\n[FEN \"bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq - 0 1\"]\n[Result \"*\"]\n\n1. e4 *\n\n\
                   [Event \"?\"]\n[Result \"*\"]\n\n1. d4 *\n\n";
        let (mut db, _) = PgnDatabase::from_reader(
            pgn.as_bytes(),
            std::path::Path::new("mixed.pgn"),
            &OpenOptions::default(),
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
        let mut filter = Filter::new(db.len());
        filter.fill(1);
        let mut mask = TreeMask::new();
        let r = feed_filter(
            &mut mask,
            &mut db,
            &filter,
            FillSide::All,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
        assert_eq!(
            r.games + r.skipped,
            u64::from(db.len()),
            "every game is accounted for"
        );
        // Whether the importer drops the variant game or the filler skips it,
        // the standard games are in the mask.
        assert!(first_moves(&mask).contains(&"d4".to_string()), "{r:?}");
    }

    #[test]
    fn feed_filter_with_a_filter_of_the_wrong_size_changes_nothing() {
        let mut db = five_games();
        for len in [0, 4, 6] {
            let filter = Filter::new(len);
            let mut mask = TreeMask::new();
            let r = feed_filter(
                &mut mask,
                &mut db,
                &filter,
                FillSide::All,
                &CancelToken::new(),
                &mut NoProgress,
            );
            assert!(matches!(r, Err(FillError::FilterSize { .. })), "{len}");
            assert!(mask.is_empty());
        }
    }

    #[test]
    fn feed_line_with_a_node_of_another_game_is_an_error() {
        let mut other = Game::new();
        let root = other.root();
        let mv = Position::new().parse_san("e4", SanMode::Strict).unwrap().mv;
        let foreign: NodeId = other.add_move(root, mv).unwrap();
        let game = Game::new();
        let mut mask = TreeMask::new();
        assert!(matches!(
            feed_line(&mut mask, &game, foreign, FillSide::All),
            Err(FillError::Node(_))
        ));
        assert!(mask.is_empty());
        // The cursor at the start: nothing to do, no error (case 7).
        let r = feed_line(&mut mask, &game, game.root(), FillSide::All).unwrap();
        assert!(!r.changed && mask.is_empty());
    }

    fn random_game(choices: &[u8]) -> Game {
        let mut game = Game::new();
        let mut parent = game.root();
        let mut p = Position::new();
        for &c in choices {
            let moves = p.legal_moves();
            if moves.is_empty() {
                break;
            }
            let mv = moves[usize::from(c) % moves.len()];
            parent = game.add_move(parent, mv).unwrap();
            p.play(mv).unwrap();
        }
        game
    }

    proptest! {
        #[test]
        fn feeding_the_same_game_twice_changes_nothing_the_second_time(
            choices in proptest::collection::vec(any::<u8>(), 0..80),
            side in prop_oneof![Just(FillSide::All), Just(FillSide::White), Just(FillSide::Black)],
        ) {
            let game = random_game(&choices);
            let mut mask = TreeMask::new();
            let first = feed_game(&mut mask, &game, side).unwrap();
            let snapshot = mask.clone();
            let second = feed_game(&mut mask, &game, side).unwrap();
            prop_assert!(!second.changed, "second run changed the mask");
            prop_assert_eq!(&mask, &snapshot);
            prop_assert_eq!(first.moves, second.moves);
            // The mask never holds more moves than the game has.
            let held: usize = mask.iter().map(|(_, p)| p.moves.len()).sum();
            prop_assert!(held as u64 <= first.moves.max(1));
        }

        #[test]
        fn a_line_is_a_prefix_of_the_whole_game(
            choices in proptest::collection::vec(any::<u8>(), 1..60),
            cut in 0usize..60,
        ) {
            let game = random_game(&choices);
            let nodes = game.descendants(game.root()).unwrap().collect::<Vec<_>>();
            let upto = nodes.get(cut.min(nodes.len().saturating_sub(1))).copied();
            let mut line = TreeMask::new();
            if let Some(until) = upto {
                feed_line(&mut line, &game, until, FillSide::All).unwrap();
            }
            let mut whole = TreeMask::new();
            feed_game(&mut whole, &game, FillSide::All).unwrap();
            for (key, p) in line.iter() {
                let w = whole.get(key);
                prop_assert!(w.is_some(), "line position missing in the game's mask");
                for m in &p.moves {
                    prop_assert!(w.unwrap().moves.iter().any(|x| x.san == m.san));
                }
            }
        }
    }
}
