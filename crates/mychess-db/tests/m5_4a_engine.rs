//! M5-4a: the mask engine through its public API: key of a position, move
//! equivalence (D1805), editing (D1821), filling from games (D1823-D1826),
//! search (D1832), outline (D1833) and the overlay over tree rows (D1828).

#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::assert_is_empty,
    clippy::field_reassign_with_default,
    clippy::too_many_lines
)]

// The module name lets `cargo test -p mychess-db m5_4a` select these tests.
mod m5_4a {

    use mychess_core::{Color, Game, Nag, NodeId, Position, Role, SanMode, SanSuffix, Square};
    use mychess_db::filter::Filter;
    use mychess_db::progress::{CancelToken, NoProgress};
    use mychess_db::tree::mask::fill::{FillError, FillSide, feed_game, feed_line};
    use mychess_db::tree::mask::outline::{self, OutlineBudget};
    use mychess_db::tree::mask::search::{self, MaskQuery};
    use mychess_db::tree::mask::{
        Marker, MarkerSlot, MaskColor, MaskKey, MaskNag, MaskOp, MoveMatcher, TreeMask, mask_san,
    };

    fn play(game: &mut Game, parent: NodeId, san: &str) -> NodeId {
        let pos = game.position_at(parent).unwrap().clone();
        let mv = pos.parse_san(san, SanMode::Strict).unwrap().mv;
        game.add_move(parent, mv).unwrap()
    }

    fn play_root(game: &mut Game, san: &str) -> NodeId {
        let root = game.root();
        play(game, root, san)
    }

    fn line(game: &mut Game, sans: &[&str]) -> Vec<NodeId> {
        let mut parent = game.root();
        sans.iter()
            .map(|s| {
                parent = play(game, parent, s);
                parent
            })
            .collect()
    }

    fn after(sans: &[&str]) -> Position {
        let mut g = Game::new();
        let ids = line(&mut g, sans);
        ids.last()
            .map_or_else(Position::new, |&id| g.position_at(id).unwrap().clone())
    }

    fn add(san: &str) -> MaskOp {
        MaskOp::AddMove { san: san.into() }
    }

    // ---------------------------------------------------------------------------
    // Key (D1804)
    // ---------------------------------------------------------------------------

    #[test]
    fn key_is_the_short_fen_and_shared_by_transpositions() {
        let a = after(&["e4", "c5", "Nf3"]);
        let b = after(&["Nf3", "c5", "e4"]);
        assert_eq!(MaskKey::from_position(&a), MaskKey::from_position(&b));
        assert_eq!(
            MaskKey::from_position(&a).as_str(),
            "rnbqkbnr/pp1ppppp/8/2p5/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq -"
        );
        let mut mask = TreeMask::new();
        mask.apply(&a, &add("d6"));
        assert!(mask.position(&b).is_some());
    }

    /// The en passant field is the square only when an enemy pawn stands next to
    /// the pawn that advanced two squares (`position.cpp` 1919-1936), even if the
    /// capture would be illegal; checked on 1000 positions of pseudo-random games.
    #[test]
    fn en_passant_field_follows_the_scid_rule_on_1000_positions() {
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let (mut checked, mut with_ep) = (0, 0);
        while checked < 1000 {
            let mut pos = Position::new();
            let mut previous: Option<(Square, Square)>;
            for _ in 0..80 {
                let moves = pos.legal_moves();
                if moves.is_empty() {
                    break;
                }
                let mv = moves[(next() % moves.len() as u64) as usize];
                let double_push = match (mv.role(), mv.from(), mv.to()) {
                    (Some(Role::Pawn), Some(from), Some(to)) => {
                        (i32::from(from.rank() as u8) - i32::from(to.rank() as u8)).abs() == 2
                    }
                    _ => false,
                };
                previous = if double_push {
                    mv.from().zip(mv.to())
                } else {
                    None
                };
                pos.play(mv).unwrap();
                let key = MaskKey::from_position(&pos);
                let fields: Vec<&str> = key.as_str().split(' ').collect();
                assert_eq!(fields.len(), 4, "{key}");
                // Independent expectation of the en passant field.
                let expected = previous.and_then(|(_, to)| {
                    let adjacent = [-1i32, 1].iter().any(|d| {
                        let file = i32::from(to.file() as u8) + d;
                        u8::try_from(file)
                            .ok()
                            .and_then(|f| {
                                Square::try_from(u32::from(to.rank() as u8) * 8 + u32::from(f)).ok()
                            })
                            .filter(|_| (0..8).contains(&file))
                            .and_then(|sq| pos.piece_at(sq))
                            .is_some_and(|p| p.role == Role::Pawn && p.color == pos.turn())
                    });
                    adjacent.then(|| {
                        let rank = if pos.turn() == Color::White { '6' } else { '3' };
                        format!("{}{rank}", char::from(b'a' + to.file() as u8))
                    })
                });
                assert_eq!(
                    fields[3],
                    expected.as_deref().unwrap_or("-"),
                    "ep field of {}",
                    pos.fen()
                );
                if expected.is_some() {
                    with_ep += 1;
                }
                checked += 1;
                // The key round-trips to the same position.
                assert_eq!(MaskKey::from_text(key.as_str()), key);
            }
        }
        assert!(with_ep > 0, "the sample must include en passant squares");
    }

    // ---------------------------------------------------------------------------
    // Move equivalence (D1805)
    // ---------------------------------------------------------------------------

    #[test]
    fn move_texts_are_equivalent_as_documented() {
        let pos = after(&["e4", "e5", "Qh5", "Nc6", "Bc4", "Nf6"]);
        let m = MoveMatcher::new(Some(&pos));
        assert!(m.same("Qh5", "Qh5+"));
        assert!(m.same("Qf7", "Qxf7"), "bare entry equals the capture");
        assert!(!m.same("Nf7", "Nxf7"), "no knight can capture there");
        assert!(m.same("Qf7", "Qxf7#"));
        assert!(!m.same("Qf7", "Qxf6"));
        // Regex metacharacters are plain text: `+` is not a quantifier.
        assert!(!m.same("Qh5++", "Qh6"));
        assert!(m.same("Qh5++", "Qh5"));

        let promo = Position::from_fen("4k3/P7/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
        let m = MoveMatcher::new(Some(&promo));
        assert!(m.same("a8Q", "a8=Q"));
        assert!(m.same("a8=Q+", "a8Q"));
        assert!(!m.same("a8Q", "a8=N"));
        for short in ["O-O", "0-0", "OK", "OO"] {
            assert!(m.same(short, "O-O"), "{short}");
        }
        for long in ["O-O-O", "0-0-0", "OQ"] {
            assert!(m.same(long, "O-O-O"), "{long}");
        }
        assert!(!m.same("O-O", "O-O-O"));
    }

    #[test]
    fn mate_is_stored_with_plus_never_hash() {
        let pos = after(&["f3", "e5", "g4"]);
        let mate = pos.parse_san("Qh4", SanMode::Strict).unwrap().mv;
        assert_eq!(pos.san_with(mate, SanSuffix::CheckOrMate).unwrap(), "Qh4#");
        assert_eq!(mask_san(&pos, mate).unwrap(), "Qh4+");
    }

    // ---------------------------------------------------------------------------
    // Editing (D1821)
    // ---------------------------------------------------------------------------

    #[test]
    fn editing_follows_the_rules_of_d1821() {
        let pos = Position::new();
        let mut mask = TreeMask::new();
        // Adding twice (also in an equivalent spelling) changes nothing.
        assert!(mask.apply(&pos, &add("e4")));
        assert!(!mask.apply(&pos, &add("e4")));
        // A value on a missing move adds the move first.
        let color = MaskOp::SetColor {
            san: "d4".into(),
            color: MaskColor::Blue,
        };
        assert!(mask.apply(&pos, &color));
        assert!(!mask.apply(&pos, &color), "same value is not a change");
        let nag = MaskOp::SetNag {
            san: "Nf3".into(),
            nag: Some(MaskNag::Interesting),
        };
        assert!(mask.apply(&pos, &nag));
        let marker = MaskOp::SetMarker {
            san: "e4".into(),
            slot: MarkerSlot::Second,
            marker: Some(Marker::Bookmark),
        };
        assert!(mask.apply(&pos, &marker));
        let entry = mask.position(&pos).unwrap();
        let sans: Vec<_> = entry.moves.iter().map(|m| m.san.as_str()).collect();
        assert_eq!(sans, ["e4", "d4", "Nf3"], "insertion order");
        assert_eq!(entry.moves[0].markers, [None, Some(Marker::Bookmark)]);
        assert_eq!(entry.moves[1].color, MaskColor::Blue);

        // Removing: a missing move is not a change; the last one removes the
        // position unless it has a comment.
        assert!(!mask.apply(&pos, &MaskOp::RemoveMove { san: "c4".into() }));
        for s in ["e4", "d4", "Nf3"] {
            assert!(mask.apply(&pos, &MaskOp::RemoveMove { san: s.into() }));
        }
        assert!(mask.is_empty());
        let comment = |t: &str| MaskOp::SetPositionComment { text: t.into() };
        assert!(mask.apply(&pos, &comment("repertoire")));
        mask.apply(&pos, &add("e4"));
        mask.apply(&pos, &MaskOp::RemoveMove { san: "e4".into() });
        assert_eq!(mask.len(), 1, "a comment keeps the position");
        assert!(mask.apply(&pos, &comment("  ")));
        assert!(
            mask.is_empty(),
            "an empty comment removes a move-less position"
        );
        assert!(!mask.apply(&pos, &comment("")), "nothing to do");
    }

    #[test]
    fn equivalent_spellings_edit_the_same_entry() {
        let pos = after(&["e4", "e5", "Qh5", "Nc6", "Bc4", "Nf6"]);
        let mut mask = TreeMask::new();
        assert!(mask.apply(&pos, &add("Qf7")));
        assert!(!mask.apply(&pos, &add("Qxf7+")));
        assert!(mask.apply(
            &pos,
            &MaskOp::SetNag {
                san: "Qxf7#".into(),
                nag: Some(MaskNag::Brilliant)
            }
        ));
        let entry = mask.position(&pos).unwrap();
        assert_eq!(entry.moves.len(), 1);
        assert_eq!(entry.moves[0].san, "Qf7", "text as written is kept");
        assert_eq!(
            mask.lookup_move(&pos, "Qxf7+").and_then(|m| m.nag.clone()),
            Some(MaskNag::Brilliant)
        );
    }

    // ---------------------------------------------------------------------------
    // Filling (D1823-D1826)
    // ---------------------------------------------------------------------------

    #[test]
    fn feed_game_follows_d1823_to_d1825() {
        let mut g = Game::new();
        g.set_initial_comment("Opening notes [%csl Ge4]");
        let ids = line(&mut g, &["e4", "e5", "Nf3", "Nc6"]);
        g.add_nag(ids[0], Nag::new(2).unwrap()).unwrap(); // ?
        g.set_comment(ids[0], "[%eval 0.20] [%clk 0:05:00]")
            .unwrap();
        g.add_nag(ids[1], Nag::new(3).unwrap()).unwrap(); // !!
        g.add_nag(ids[1], Nag::new(14).unwrap()).unwrap(); // only the first counts
        g.set_comment(ids[1], "good").unwrap();
        g.add_nag(ids[2], Nag::new(14).unwrap()).unwrap(); // +=, not standard
        g.add_nag(ids[3], Nag::new(201).unwrap()).unwrap(); // diagram: ignored
        g.set_comment(ids[3], "+0.35").unwrap();
        // A variation from the start position.
        play_root(&mut g, "d4");

        let mut mask = TreeMask::new();
        let report = feed_game(&mut mask, &g, FillSide::All).unwrap();
        assert!(report.changed);
        assert_eq!(report.moves, 5);
        let start = mask.position(&Position::new()).unwrap();
        assert_eq!(start.comment, "Opening notes");
        let sans: Vec<_> = start.moves.iter().map(|m| m.san.as_str()).collect();
        assert_eq!(sans, ["e4", "d4"]);
        assert!(start.moves.iter().all(|m| !m.is_null()));
        assert_eq!(start.moves[0].nag, Some(MaskNag::Poor), "? is standard");
        assert_eq!(start.moves[0].comment, "", "only commands: nothing left");
        let m = &mask.position(g.position_at(ids[0]).unwrap()).unwrap().moves[0];
        assert_eq!(
            (m.nag.clone(), m.comment.as_str()),
            (Some(MaskNag::Brilliant), "good")
        );
        let m = &mask.position(g.position_at(ids[1]).unwrap()).unwrap().moves[0];
        assert_eq!((m.nag.clone(), m.comment.as_str()), (None, "+="));
        let m = &mask.position(g.position_at(ids[2]).unwrap()).unwrap().moves[0];
        assert_eq!((m.nag.clone(), m.comment.as_str()), (None, ""));

        // Running it again changes nothing at all.
        let snapshot = mask.clone();
        let again = feed_game(&mut mask, &g, FillSide::All).unwrap();
        assert!(!again.changed);
        assert_eq!(mask, snapshot);
    }

    #[test]
    fn feed_game_merges_comments_without_repeating_lines() {
        let mut g = Game::new();
        let e4 = play_root(&mut g, "e4");
        let mut mask = TreeMask::new();
        for (text, expected) in [
            ("first", "first"),
            ("second", "first\nsecond"),
            ("first", "first\nsecond"),
            ("second", "first\nsecond"),
            ("third", "first\nsecond\nthird"),
        ] {
            g.set_comment(e4, text).unwrap();
            feed_game(&mut mask, &g, FillSide::All).unwrap();
            let m = &mask.position(&Position::new()).unwrap().moves[0];
            assert_eq!(m.comment, expected);
        }
    }

    #[test]
    fn feed_game_with_a_start_fen_and_variations() {
        let start = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
        let mut g = Game::from_position(start.clone());
        g.set_initial_comment("pawn ending");
        let e4 = play_root(&mut g, "e4");
        play_root(&mut g, "e3");
        play(&mut g, e4, "Kd7");
        let mut mask = TreeMask::new();
        feed_game(&mut mask, &g, FillSide::All).unwrap();
        let entry = mask.position(&start).unwrap();
        assert_eq!(entry.comment, "pawn ending");
        assert_eq!(entry.moves.len(), 2);
        assert!(mask.position(&Position::new()).is_none());
        assert_eq!(mask.len(), 2, "the variation of the first move is included");
    }

    #[test]
    fn feed_line_uses_the_path_to_the_cursor_and_the_side() {
        let mut g = Game::new();
        let main = line(&mut g, &["e4", "e5", "Nf3", "Nc6", "Bb5"]);
        // A variation inside the game: 2.Nc3 instead of 2.Nf3, cursor on it.
        let nc3 = play(&mut g, main[1], "Nc3");

        let mut all = TreeMask::new();
        let r = feed_line(&mut all, &g, nc3, FillSide::All).unwrap();
        assert_eq!(r.moves, 3);
        assert!(all.position(g.position_at(main[2]).unwrap()).is_none());
        let after_e5 = all.position(g.position_at(main[1]).unwrap()).unwrap();
        assert_eq!(after_e5.moves[0].san, "Nc3");

        let mut white = TreeMask::new();
        assert_eq!(
            feed_line(&mut white, &g, nc3, FillSide::White)
                .unwrap()
                .moves,
            2
        );
        assert!(white.position(g.position_at(main[0]).unwrap()).is_none());
        let mut black = TreeMask::new();
        assert_eq!(
            feed_line(&mut black, &g, nc3, FillSide::Black)
                .unwrap()
                .moves,
            1
        );
        assert!(black.position(&Position::new()).is_none());

        // At the start of the game there is nothing to do.
        let mut none = TreeMask::new();
        let r = feed_line(&mut none, &g, g.root(), FillSide::All).unwrap();
        assert!(!r.changed && none.is_empty());
    }

    #[test]
    fn chess960_games_are_refused() {
        let g = Game::from_position(Position::chess960_start(100).unwrap());
        let mut mask = TreeMask::new();
        assert!(matches!(
            feed_game(&mut mask, &g, FillSide::All),
            Err(FillError::NotStandard)
        ));
        assert!(mask.is_empty());
    }

    #[test]
    fn feed_filter_checks_the_filter_size() {
        use mychess_db::pgn_db::{OpenOptions, PgnDatabase};
        let pgn = "[Event \"?\"]\n[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n\
               [Event \"?\"]\n[Result \"0-1\"]\n\n1. d4 d5 0-1\n\n";
        let (mut db, _) = PgnDatabase::from_reader(
            pgn.as_bytes(),
            std::path::Path::new("t.pgn"),
            &OpenOptions::default(),
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
        let n = mychess_db::source::GameSource::len(&db);
        let mut filter = Filter::new(n);
        filter.fill(1);
        let mut mask = TreeMask::new();
        let mut ticks = Vec::new();
        let mut sink = |done: u64, total: u64| ticks.push((done, total));
        let report = mychess_db::tree::mask::fill::feed_filter(
            &mut mask,
            &mut db,
            &filter,
            FillSide::All,
            &CancelToken::new(),
            &mut sink,
        )
        .unwrap();
        assert_eq!(
            (report.games, report.skipped, report.cancelled),
            (2, 0, false)
        );
        assert_eq!(ticks, [(1, 2), (2, 2)]);
        assert_eq!(mask.position(&Position::new()).unwrap().moves.len(), 2);

        // Cancelled before it starts: nothing done, flagged.
        let cancel = CancelToken::new();
        cancel.cancel();
        let mut empty = TreeMask::new();
        let r = mychess_db::tree::mask::fill::feed_filter(
            &mut empty,
            &mut db,
            &filter,
            FillSide::All,
            &cancel,
            &mut NoProgress,
        )
        .unwrap();
        assert!(r.cancelled && empty.is_empty());

        // Wrong size.
        let wrong = Filter::new(n + 1);
        assert!(matches!(
            mychess_db::tree::mask::fill::feed_filter(
                &mut empty,
                &mut db,
                &wrong,
                FillSide::All,
                &CancelToken::new(),
                &mut NoProgress
            ),
            Err(FillError::FilterSize { .. })
        ));
    }

    // ---------------------------------------------------------------------------
    // Search (D1832)
    // ---------------------------------------------------------------------------

    fn searchable() -> TreeMask {
        let start = Position::new();
        let e4pos = after(&["e4"]);
        let mut mask = TreeMask::new();
        let set = |mask: &mut TreeMask, pos: &Position, san: &str, nag, color, text: &str| {
            mask.apply(
                pos,
                &MaskOp::SetNag {
                    san: san.into(),
                    nag,
                },
            );
            mask.apply(
                pos,
                &MaskOp::SetColor {
                    san: san.into(),
                    color,
                },
            );
            mask.apply(
                pos,
                &MaskOp::SetMoveComment {
                    san: san.into(),
                    text: text.into(),
                },
            );
        };
        set(
            &mut mask,
            &start,
            "e4",
            Some(MaskNag::Good),
            MaskColor::Green,
            "King pawn *main*",
        );
        set(&mut mask, &start, "d4", None, MaskColor::White, "");
        set(&mut mask, &start, "null", None, MaskColor::White, "hidden");
        set(
            &mut mask,
            &e4pos,
            "c5",
            Some(MaskNag::Good),
            MaskColor::Red,
            "Sicilian",
        );
        mask.apply(
            &start,
            &MaskOp::SetMarker {
                san: "d4".into(),
                slot: MarkerSlot::First,
                marker: Some(Marker::ToTrain),
            },
        );
        mask.apply(
            &e4pos,
            &MaskOp::SetPositionComment {
                text: "After e4".into(),
            },
        );
        mask
    }

    fn found(mask: &TreeMask, f: impl FnOnce(&mut MaskQuery)) -> Vec<String> {
        let mut q = MaskQuery::default();
        f(&mut q);
        search::run(mask, &q)
            .rows
            .into_iter()
            .map(|r| r.san.unwrap_or_else(|| "<position>".into()))
            .collect()
    }

    #[test]
    fn search_filters_and_their_combinations() {
        let m = searchable();
        assert_eq!(
            found(&m, |_| {}),
            ["e4", "d4", "c5"],
            "null is never listed"
        );
        assert_eq!(
            found(&m, |q| q.nag = Some(Some(MaskNag::Good))),
            ["e4", "c5"]
        );
        assert_eq!(found(&m, |q| q.nag = Some(None)), ["d4"]);
        assert_eq!(
            found(&m, |q| q.marker1 = Some(Some(Marker::ToTrain))),
            ["d4"]
        );
        assert_eq!(found(&m, |q| q.marker2 = Some(None)).len(), 3);
        assert_eq!(found(&m, |q| q.color = Some(MaskColor::Red)), ["c5"]);
        assert_eq!(found(&m, |q| q.move_text = Some("SICIL".into())), ["c5"]);
        assert_eq!(
            found(&m, |q| {
                q.nag = Some(Some(MaskNag::Good));
                q.color = Some(MaskColor::Green);
            }),
            ["e4"]
        );
        // The text is a literal substring: `*` is not a wildcard.
        assert_eq!(found(&m, |q| q.move_text = Some("*main*".into())), ["e4"]);
        assert!(found(&m, |q| q.move_text = Some("K*n".into())).is_empty());
    }

    #[test]
    fn position_comment_filter_excludes_positions_and_adds_a_row() {
        let m = searchable();
        let mut q = MaskQuery::default();
        q.position_text = Some("after".into());
        let hits = search::run(&m, &q);
        assert_eq!((hits.positions_found, hits.positions_total), (1, 2));
        let rows: Vec<_> = hits.rows.iter().map(|r| r.san.as_deref()).collect();
        assert_eq!(rows, [None, Some("c5")]);
        assert_eq!(hits.rows[0].comment, "After e4");
        assert!(hits.rows[0].key.full_fen().unwrap().ends_with(" 0 1"));
    }

    #[test]
    fn search_result_is_limited_to_10000_rows() {
        let mut mask = TreeMask::new();
        let mut fens = 0;
        for i in 0..30u32 {
            let key = MaskKey::from_text(&format!("8/8/8/8/8/8/8/K6k w - - {i}"));
            // Not a valid short FEN: opaque keys are fine for this count.
            let mut p = mychess_db::tree::mask::MaskPosition::default();
            for j in 0..400 {
                p.moves
                    .push(mychess_db::tree::mask::MaskMove::new(format!("m{j}")));
            }
            mask.insert(key, p);
            fens += 1;
        }
        let hits = search::run(&mask, &MaskQuery::default());
        assert_eq!(hits.rows.len(), search::MAX_HITS);
        assert!(hits.truncated);
        assert_eq!(hits.moves_found, fens * 400);
    }

    // ---------------------------------------------------------------------------
    // Overlay (D1828)
    // ---------------------------------------------------------------------------

    #[test]
    fn overlay_pairs_tree_rows_with_entries_and_lists_the_rest() {
        let start = Position::new();
        let mut mask = TreeMask::new();
        for s in ["e4", "null", "Nf3", "Qd4"] {
            mask.apply(&start, &add(s));
        }
        mask.apply(
            &start,
            &MaskOp::SetPositionComment {
                text: "hello".into(),
            },
        );
        let o = mask.overlay(&start, &["d4", "Nf3+", "e4"]);
        assert_eq!(o.comment, "hello");
        assert!(o.rows[0].is_none());
        assert_eq!(o.rows[1].unwrap().san, "Nf3");
        assert_eq!(o.rows[2].unwrap().san, "e4");
        assert_eq!(o.only_mask.len(), 1, "null is hidden");
        assert_eq!(o.only_mask[0].entry.san, "Qd4");
        assert!(o.only_mask[0].legal.is_none(), "illegal entry is flagged");

        let legal_only = {
            let mut m = TreeMask::new();
            m.apply(&start, &add("Nc3"));
            m
        };
        let o = legal_only.overlay(&start, &[]);
        assert!(o.only_mask[0].legal.is_some());
        // Empty tree: only the mask's moves are there.
        assert!(TreeMask::new().overlay(&start, &["e4"]).rows[0].is_none());
    }

    // ---------------------------------------------------------------------------
    // Outline (D1833)
    // ---------------------------------------------------------------------------

    fn add_line(mask: &mut TreeMask, sans: &[&str]) {
        let mut pos = Position::new();
        for s in sans {
            let mv = pos.parse_san(s, SanMode::Strict).unwrap().mv;
            mask.apply(&pos, &add(&mask_san(&pos, mv).unwrap()));
            pos.play(mv).unwrap();
        }
    }

    fn steps(n: &outline::OutlineNode) -> Vec<&str> {
        n.steps.iter().map(|s| s.san.as_str()).collect()
    }

    #[test]
    fn outline_merges_runs_and_branches_at_forks() {
        let mut mask = TreeMask::new();
        add_line(&mut mask, &["e4", "e5", "Nf3", "Nc6", "Bb5"]);
        add_line(&mut mask, &["e4", "e5", "Nf3", "Nc6", "Bc4"]);
        add_line(&mut mask, &["d4"]);
        let o = outline::build(&mask, &Position::new(), OutlineBudget::default());
        assert_eq!(o.roots.len(), 2);
        assert_eq!(steps(&o.roots[0]), ["e4", "e5", "Nf3", "Nc6"]);
        assert_eq!(
            o.roots[0].children.iter().map(steps).collect::<Vec<_>>(),
            [vec!["Bb5"], vec!["Bc4"]]
        );
        assert_eq!(steps(&o.roots[1]), ["d4"]);
        assert_eq!((o.nodes, o.truncated), (7, false));
    }

    #[test]
    fn outline_does_not_expand_a_cycle_and_keeps_the_siblings() {
        let mut mask = TreeMask::new();
        add_line(&mut mask, &["Nf3", "Nf6", "Ng1", "Ng8"]);
        add_line(&mut mask, &["e4"]);
        let o = outline::build(&mask, &Position::new(), OutlineBudget::default());
        assert_eq!(o.roots.len(), 2, "e4 survives the cycle of the first line");
        assert_eq!(steps(&o.roots[0]), ["Nf3", "Nf6", "Ng1", "Ng8"]);
        assert!(o.roots[0].children.is_empty());
    }

    #[test]
    fn outline_budget_is_respected_and_reported() {
        let mut mask = TreeMask::new();
        add_line(&mut mask, &["e4", "e5", "Nf3", "Nc6", "Bb5", "a6"]);
        let o = outline::build(
            &mask,
            &Position::new(),
            OutlineBudget {
                max_nodes: 3,
                max_depth: 200,
            },
        );
        assert!(o.truncated && o.nodes <= 3);
        let o = outline::build(
            &mask,
            &Position::new(),
            OutlineBudget {
                max_nodes: 20_000,
                max_depth: 2,
            },
        );
        assert!(o.truncated);
        assert_eq!(steps(&o.roots[0]).len(), 2);
    }

    #[test]
    fn outline_of_a_wide_mask_with_transpositions_stays_within_the_budget() {
        // Moves of both knights back and forth create many transpositions.
        let mut mask = TreeMask::new();
        for l in [
            ["Nf3", "Nf6", "Nc3", "Nc6", "Ng1", "Ng8", "Nb1", "Nb8"],
            ["Nc3", "Nc6", "Nf3", "Nf6", "Nb1", "Nb8", "Ng1", "Ng8"],
            ["Nf3", "Nc6", "Nc3", "Nf6", "Ng1", "Nb8", "Nb1", "Ng8"],
        ] {
            add_line(&mut mask, &l);
        }
        let budget = OutlineBudget {
            max_nodes: 50,
            max_depth: 200,
        };
        let o = outline::build(&mask, &Position::new(), budget);
        assert!(o.nodes <= 50);
    }
}
