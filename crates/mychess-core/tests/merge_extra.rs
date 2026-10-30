//! Extra tests of `Game::merge_game` derived from the specification
//! (`docs/specs/m7-6-mesclar-partida.md`) and the Scid's `sc_game merge`:
//! nested variations and annotations in the other game, start positions,
//! null moves, Chess960, cut-off values, repetitions, deep trees, atomicity,
//! comments and a brute-force oracle for the matching rule.

mod common;

use std::time::{Duration, Instant};

use mychess_core::{
    CastlingMode, Game, GameResult, MergeError, MergeOptions, Move, Nag, NodeId, PartialDate,
    Position, SanMode, merge_comment_parts,
};
use proptest::prelude::*;

// ---------------------------------------------------------------- helpers

fn game_from(start: Position, sans: &str) -> Game {
    let mut g = Game::from_position(start);
    let mut at = g.root();
    for san in sans.split_whitespace() {
        let mv = g
            .position_at(at)
            .expect("node")
            .parse_san(san, SanMode::Strict)
            .unwrap_or_else(|e| panic!("bad san {san}: {e:?}"))
            .mv;
        at = g.add_move(at, mv).expect("add");
    }
    g
}

fn game(sans: &str) -> Game {
    game_from(Position::new(), sans)
}

/// Plays the knight shuffle `Nf3 Nf6 Ng1 Ng8` for `plies` plies.
fn shuffle(plies: usize) -> Game {
    let mut g = Game::new();
    let mut at = g.root();
    let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
    for i in 0..plies {
        let mv = g
            .position_at(at)
            .expect("node")
            .parse_uci(cycle[i % 4])
            .expect("uci");
        at = g.add_move(at, mv).expect("add");
    }
    g
}

fn last_of(g: &Game) -> NodeId {
    g.mainline().last().expect("has moves")
}

fn main_ucis(g: &Game) -> Vec<String> {
    let mut out = Vec::new();
    let mut pos = g.start_position().clone();
    for id in g.mainline() {
        let mv = g.mv(id).expect("move");
        out.push(pos.uci(mv));
        pos = g.position_at(id).expect("node").clone();
    }
    out
}

fn san_of(g: &Game, id: NodeId) -> String {
    let parent = g.parent(id).expect("node").expect("not root");
    g.position_at(parent)
        .expect("node")
        .san(g.mv(id).expect("move"))
        .expect("san")
}

fn opts(end: Option<u32>) -> MergeOptions {
    MergeOptions {
        end_ply: end,
        comment: None,
    }
}

fn with_comment(text: &str) -> MergeOptions {
    MergeOptions {
        end_ply: None,
        comment: Some(text.to_owned()),
    }
}

fn a() -> Game {
    game("e4 e5 Nf3 Nc6 Bb5 a6")
}

fn b() -> Game {
    game("e4 e5 Nf3 Nc6 Bc4 Bc5 c3 Nf6")
}

// ------------------------------------------- only the other main line counts

#[test]
fn other_game_variations_nags_and_comments_are_not_copied() {
    // B has a nested variation (depth 2), NAGs and comments everywhere.
    let mut other = b();
    let nodes: Vec<NodeId> = other.mainline().collect();
    let pos4 = other.position_at(nodes[3]).expect("node").clone();
    let alt = pos4.parse_san("Bb5", SanMode::Strict).expect("san").mv;
    let var = other.add_move(nodes[3], alt).expect("var");
    let after_var = other.position_at(var).expect("node").clone();
    let deeper = after_var.parse_san("a6", SanMode::Strict).expect("san").mv;
    let var2 = other.add_move(var, deeper).expect("var2");
    let after_var2 = other.position_at(var2).expect("node").clone();
    let deepest = after_var2
        .parse_san("Ba4", SanMode::Strict)
        .expect("san")
        .mv;
    other.add_move(var2, deepest).expect("var3");
    for &n in &nodes {
        other.set_comment(n, "{weird} ; text").expect("comment");
        other.set_comment_before(n, "before").expect("before");
        other.add_nag(n, Nag::new(1).expect("nag")).expect("nag");
    }
    other.set_initial_comment("initial");

    let mut g = a();
    let before_nodes = g.node_count();
    let r = g.merge_game(&other, &opts(None)).expect("merge");
    assert_eq!((r.match_ply, r.added), (4, 4));
    // Exactly the 4 mainline moves, nothing from the variations.
    assert_eq!(g.node_count(), before_nodes + 4);
    // None of the annotations came along.
    for id in g.descendants(g.root()).expect("root") {
        assert_eq!(g.comment(id).expect("node"), None);
        assert_eq!(g.comment_before(id).expect("node"), None);
        assert_eq!(g.nags(id).expect("node").len(), 0);
    }
    assert_eq!(g.initial_comment(), None);
}

#[test]
fn position_reachable_only_through_other_variation_does_not_match() {
    // Target mainline passes through 1.e4 e5 2.f4; the other game only has
    // 2.f4 inside a variation, so the deepest match is ply 2.
    let mut other = game("e4 e5 Nf3 Nc6");
    let nodes: Vec<NodeId> = other.mainline().collect();
    let p2 = other.position_at(nodes[1]).expect("node").clone();
    let f4 = p2.parse_san("f4", SanMode::Strict).expect("san").mv;
    other.add_move(nodes[1], f4).expect("var");
    let mut g = game("e4 e5 f4 exf4");
    let r = g.merge_game(&other, &opts(None)).expect("merge");
    assert_eq!((r.match_ply, r.other_ply), (2, 2));
    assert_eq!(san_of(&g, r.first), "Nf3");
}

#[test]
fn target_variations_do_not_take_part_in_matching_but_fuse_when_equal() {
    // Target: main 1.e4 e5 2.Nf3, variation 2.Bc4 (already there).
    let mut g = game("e4 e5 Nf3");
    let nodes: Vec<NodeId> = g.mainline().collect();
    let p2 = g.position_at(nodes[1]).expect("node").clone();
    let bc4 = p2.parse_san("Bc4", SanMode::Strict).expect("san").mv;
    let existing = g.add_move(nodes[1], bc4).expect("var");
    let other = game("e4 e5 Bc4 Nf6");
    let r = g.merge_game(&other, &opts(None)).expect("merge");
    assert_eq!((r.match_ply, r.other_ply), (2, 2));
    assert_eq!((r.added, r.reused), (1, 1));
    assert_eq!(
        r.first, existing,
        "existing variation is reused, not cloned"
    );
    assert!(!r.extended_mainline);
    assert_eq!(g.children(existing).expect("node").len(), 1);
}

// ----------------------------------------------------- start positions

#[test]
fn clocks_are_ignored_when_matching_start_positions() {
    // After 1.e4 e5 with different halfmove/fullmove counters.
    let base = game("e4 e5");
    let fen = base.position_at(last_of(&base)).expect("node").fen();
    let fields: Vec<&str> = fen.split_whitespace().collect();
    let other_fen = format!(
        "{} {} {} {} 17 40",
        fields[0], fields[1], fields[2], fields[3]
    );
    let other_start = Position::from_fen(&other_fen).expect("fen");
    let other = game_from(other_start, "Bc4 Nf6");

    let mut g = game("e4 e5 Nf3 Nc6");
    let r = g.merge_game(&other, &opts(None)).expect("merge");
    assert_eq!((r.match_ply, r.other_ply, r.added), (2, 0, 2));
    assert_eq!(san_of(&g, r.first), "Bc4");
    assert!(g.is_variation(r.first).expect("node"));
}

#[test]
fn fen_target_merges_standard_source_that_reaches_its_position() {
    let base = game("e4 e5");
    let start = base.position_at(last_of(&base)).expect("node").clone();
    let mut g = game_from(start, "Nf3 Nc6");
    let r = g
        .merge_game(&game("e4 e5 Nf3 Nc6 Bb5"), &opts(None))
        .expect("merge");
    // The end of the target matches ply 4 of the source: main line extended.
    assert_eq!((r.match_ply, r.other_ply, r.added), (2, 4, 1));
    assert!(r.extended_mainline);
    assert_eq!(g.mainline().count(), 3);
}

#[test]
fn en_passant_square_matters_only_when_the_capture_is_legal() {
    // Capturable en passant: different position than the same board without.
    let with_ep = Position::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").expect("fen");
    let without = Position::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - - 0 1").expect("fen");
    assert_ne!(with_ep, without, "legal ep must distinguish positions");
    let mut x = game_from(with_ep, "exd6");
    let y = game_from(without, "Kd2");
    assert_eq!(x.merge_game(&y, &opts(None)), Err(MergeError::NoMatch));

    // Not capturable: the FEN square is normalised away, positions are equal.
    let not_capt = Position::from_fen("4k3/8/8/3p4/8/8/8/4K3 w - d6 0 1").expect("fen");
    let plain = Position::from_fen("4k3/8/8/3p4/8/8/8/4K3 w - - 0 1").expect("fen");
    let mut x = game_from(not_capt, "Kd2");
    let y = game_from(plain, "Ke2");
    let r = x.merge_game(&y, &opts(None)).expect("merge");
    assert_eq!((r.match_ply, r.other_ply, r.added), (0, 0, 1));
}

#[test]
fn castling_rights_distinguish_otherwise_equal_positions() {
    let rights = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").expect("fen");
    let none = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w - - 0 1").expect("fen");
    let mut x = game_from(rights, "Ra2");
    let y = game_from(none, "Rb1");
    assert_eq!(x.merge_game(&y, &opts(None)), Err(MergeError::NoMatch));
}

// -------------------------------------------------------- empty games

#[test]
fn other_game_without_moves_is_nothing_to_add_and_changes_nothing() {
    let mut g = a();
    let before = g.clone();
    assert_eq!(
        g.merge_game(&Game::new(), &opts(None)),
        Err(MergeError::NothingToAdd)
    );
    assert!(g.same_content(&before));
}

#[test]
fn both_games_empty_report_no_moves() {
    let mut g = Game::new();
    assert_eq!(
        g.merge_game(&Game::new(), &opts(None)),
        Err(MergeError::NoMoves)
    );
}

#[test]
fn empty_other_with_other_start_position_is_no_match_or_nothing_to_add() {
    let mut g = a();
    let other =
        Game::from_position(Position::from_fen("4k3/8/8/8/8/8/8/R3K3 w Q - 0 1").expect("fen"));
    let before = g.clone();
    let err = g.merge_game(&other, &opts(None)).expect_err("must refuse");
    assert!(matches!(
        err,
        MergeError::NoMatch | MergeError::NothingToAdd
    ));
    assert!(g.same_content(&before));
}

// ------------------------------------------------------ end_ply values

#[test]
fn end_ply_values_zero_one_exact_and_huge() {
    // (end_ply, added, truncated) for A <- B (match at ply 4, B has 8 plies).
    let cases = [
        (Some(0), 1, true),
        (Some(1), 1, true),
        (Some(4), 1, true),
        (Some(5), 1, true),
        (Some(6), 2, true),
        (Some(7), 3, true),
        (Some(8), 4, false),
        (Some(9), 4, false),
        (Some(u32::MAX), 4, false),
        (None, 4, false),
    ];
    for (end, added, truncated) in cases {
        let mut g = a();
        let r = g.merge_game(&b(), &opts(end)).expect("merge");
        assert_eq!(
            (r.added, r.truncated),
            (added, truncated),
            "end_ply = {end:?}"
        );
        assert_eq!(g.mainline().count(), 6, "main line intact for {end:?}");
    }
}

#[test]
fn end_ply_zero_at_start_match_copies_one_move() {
    let mut g = a();
    let r = g
        .merge_game(&game("d4 d5 c4 e6"), &opts(Some(0)))
        .expect("merge");
    assert_eq!((r.match_ply, r.added, r.truncated), (0, 1, true));
    assert_eq!(san_of(&g, r.first), "d4");
}

#[test]
fn end_ply_with_extended_main_line_copies_the_prefix_only() {
    let mut e = game("e4 e5");
    let r = e.merge_game(&b(), &opts(Some(0))).expect("merge");
    assert!(r.extended_mainline && r.truncated);
    assert_eq!(r.added, 1);
    assert_eq!(e.mainline().count(), 3);
}

// -------------------------------------------- transpositions, repetitions

#[test]
fn repeated_positions_pick_deepest_own_ply_and_latest_other_ply() {
    // Own: start position recurs at plies 0, 4, 8.
    let own = "Nf3 Nf6 Ng1 Ng8 Nf3 Nf6 Ng1 Ng8 e4";
    let mut g = game(own);
    let r = g
        .merge_game(&game("Nf3 Nf6 Ng1 Ng8 d4"), &opts(None))
        .expect("merge");
    assert_eq!((r.match_ply, r.other_ply, r.added), (8, 4, 1));
    assert!(!r.extended_mainline);
    assert_eq!(san_of(&g, r.first), "d4");
    assert!(g.is_variation(r.first).expect("node"));
}

#[test]
fn other_ending_in_a_repeated_position_has_nothing_to_add() {
    // The other game ends exactly where the repetition is: the latest ply
    // (4) wins and leaves no moves to copy.
    let mut g = game("Nf3 Nf6 Ng1 Ng8 e4");
    let before = g.clone();
    assert_eq!(
        g.merge_game(&game("Nf3 Nf6 Ng1 Ng8"), &opts(None)),
        Err(MergeError::NothingToAdd)
    );
    assert!(g.same_content(&before));
}

#[test]
fn long_repetition_matches_the_latest_occurrence_in_the_other_game() {
    let mut g = shuffle(40);
    // Other: 40 plies of shuffle then a different continuation.
    let mut other = shuffle(40);
    let pos = other.position_at(last_of(&other)).expect("node").clone();
    let mv = pos.parse_san("d4", SanMode::Strict).expect("san").mv;
    other.add_move(last_of(&other), mv).expect("add");
    let r = g.merge_game(&other, &opts(None)).expect("merge");
    // Target end (ply 40) == other ply 40; main line is extended with d4.
    assert_eq!((r.match_ply, r.other_ply, r.added), (40, 40, 1));
    assert!(r.extended_mainline);
}

// --------------------------------------------------- null moves

#[test]
fn null_moves_in_the_other_game_are_copied_when_legal() {
    let mut other = game("e4 e5 Nf3");
    let last = last_of(&other);
    // Black passes, then White plays again.
    let n1 = other.add_move(last, Move::Null).expect("null");
    let p = other.position_at(n1).expect("node").clone();
    let bc4 = p.parse_san("Bc4", SanMode::Strict).expect("san").mv;
    other.add_move(n1, bc4).expect("add");

    let mut g = game("e4 e5");
    let r = g.merge_game(&other, &opts(None)).expect("merge");
    assert_eq!((r.added, r.extended_mainline), (3, true));
    assert_eq!(g.mainline().count(), 5);
    assert_eq!(
        main_ucis(&g),
        main_ucis(&other),
        "null move is written as 0000 in both"
    );
}

#[test]
fn null_move_in_the_target_main_line_matches_by_position() {
    let mut with_null = game("e4");
    let null_node = with_null
        .add_move(last_of(&with_null), Move::Null)
        .expect("null");
    let after_null = with_null.position_at(null_node).expect("node").clone();
    let mv = after_null.parse_san("d4", SanMode::Strict).expect("san").mv;
    with_null.add_move(null_node, mv).expect("add");
    // The other game reaches the same position without the null move?
    // Not possible (side to move differs); it must instead share 1.e4.
    let mut y = with_null.clone();
    let r = y
        .merge_game(&game("e4 e5 Nf3"), &opts(None))
        .expect("merge");
    // Deepest own ply in other: ply 1 (after 1.e4).
    assert_eq!((r.match_ply, r.other_ply), (1, 1));
    assert!(y.is_variation(r.first).expect("node"));
}

// ----------------------------------------------------- Chess960

#[test]
fn chess960_games_merge_with_castling() {
    for n in [518u16, 0, 959, 123] {
        let start = Position::chess960_start(n).expect("start");
        let moves = start.legal_moves();
        // Pick two different first moves.
        let m1 = moves[0];
        let m2 = *moves.last().expect("moves");
        let mut x = Game::from_position(start.clone());
        let n1 = x.add_move(x.root(), m1).expect("add");
        let after1 = x.position_at(n1).expect("node").clone();
        let reply = after1.legal_moves()[0];
        x.add_move(n1, reply).expect("add");
        let mut y = Game::from_position(start.clone());
        y.add_move(y.root(), m2).expect("add");
        let before = x.clone();
        match x.merge_game(&y, &opts(None)) {
            Ok(r) => {
                assert_eq!(r.match_ply, 0, "start 960#{n}");
                for id in x.descendants(x.root()).expect("root") {
                    let p = x.parent(id).expect("node").expect("parent");
                    let expected = x
                        .position_at(p)
                        .expect("node")
                        .after(x.mv(id).expect("move"))
                        .expect("legal");
                    assert_eq!(x.position_at(id).expect("node"), &expected);
                }
            }
            Err(MergeError::NothingToAdd) => assert!(x.same_content(&before)),
            Err(e) => panic!("960#{n}: unexpected {e:?}"),
        }
    }
}

#[test]
fn chess960_game_accepts_a_standard_source_atomically() {
    // Same board, different castling mode. Either the merge works and every
    // node is legal, or it fails with `Game(..)` and nothing changed.
    let start960 = Position::chess960_start(518).expect("start");
    let line = "e4 e5 Nf3 Nc6 Bb5 a6 Ba4 Nf6 O-O Be7";
    let mut target = game_from(start960, "e4 e5 Nf3 Nc6 Bb5 a6");
    let source = game(line);
    let before = target.clone();
    match target.merge_game(&source, &opts(None)) {
        Ok(r) => {
            assert_eq!(r.match_ply, 6);
            for id in target.descendants(target.root()).expect("root") {
                assert!(target.position_at(id).is_ok());
            }
        }
        Err(MergeError::Game(_)) => {
            assert!(target.same_content(&before), "error must roll back");
            assert_eq!(target.node_count(), before.node_count());
        }
        Err(e) => panic!("unexpected {e:?}"),
    }
}

#[test]
fn standard_game_accepts_a_chess960_source_atomically() {
    let start960 = Position::chess960_start(518).expect("start");
    let source = game_from(start960, "e4 e5 Nf3 Nc6 Bb5 a6 Ba4 Nf6 O-O Be7");
    let mut target = game("e4 e5 Nf3 Nc6 Bb5 a6");
    let before = target.clone();
    match target.merge_game(&source, &opts(None)) {
        Ok(_) => {
            for id in target.descendants(target.root()).expect("root") {
                assert!(target.position_at(id).is_ok());
            }
        }
        Err(MergeError::Game(_)) => assert!(target.same_content(&before)),
        Err(e) => panic!("unexpected {e:?}"),
    }
}

#[test]
fn castling_mode_of_position_is_visible_to_the_tests() {
    // Sanity for the two tests above: the 960 start really is 960 mode.
    let p = Position::chess960_start(518).expect("start");
    assert_eq!(p.variant(), mychess_core::Variant::Chess960);
    let _ = CastlingMode::Chess960;
}

// ---------------------------------------------------------- atomicity

#[test]
fn a_failing_copy_leaves_no_new_nodes_and_no_comment() {
    // Force a failure mid-copy: a standard-mode target and a source whose
    // castling move is only valid in the source's mode. Search the 960 start
    // positions for a pair where the merge fails; whatever happens, the
    // target must be intact on `Err`.
    let mut failures = 0;
    for n in [0u16, 100, 200, 300, 518, 700, 959] {
        let start = Position::chess960_start(n).expect("start");
        // Walk a short random line in 960 mode as source.
        let ch: Vec<u16> = (0..14).map(|i| 3 * i + 1).collect();
        let mut source = Game::from_position(start.clone());
        let mut at = source.root();
        for (_, mv) in common::random_game(&start, &ch) {
            at = source.add_move(at, mv).expect("legal");
        }
        // Target: same start, as standard-mode game built from the FEN.
        let std_start = Position::from_fen(&start.fen()).unwrap_or_else(|_| start.clone());
        let mut target = Game::from_position(std_start.clone());
        let first = std_start.legal_moves()[0];
        if target.add_move(target.root(), first).is_err() {
            continue;
        }
        let before = target.clone();
        let o = with_comment("should not survive an error");
        if target.merge_game(&source, &o).is_err() {
            failures += 1;
            assert!(target.same_content(&before), "960#{n}: not atomic");
            assert_eq!(target.node_count(), before.node_count());
            assert_eq!(
                target
                    .comment_before(target.mainline().next().expect("node"))
                    .expect("node"),
                None
            );
        }
    }
    let _ = failures; // zero is fine: the guarantee is "unchanged on Err".
}

#[test]
fn self_merge_of_a_clone_is_nothing_to_add_even_with_variations() {
    let mut g = a();
    let nodes: Vec<NodeId> = g.mainline().collect();
    let p = g.position_at(nodes[3]).expect("node").clone();
    let alt = p.parse_san("Bc4", SanMode::Strict).expect("san").mv;
    g.add_move(nodes[3], alt).expect("var");
    g.set_comment(nodes[0], "c").expect("c");
    let copy = g.clone();
    let before = g.clone();
    assert_eq!(
        g.merge_game(&copy, &with_comment("x")),
        Err(MergeError::NothingToAdd)
    );
    assert!(g.same_content(&before));
}

// ---------------------------------------------------------- comments

#[test]
fn comment_none_empty_and_blank_add_nothing() {
    for c in [None, Some(String::new()), Some("   \n\t ".to_owned())] {
        let mut g = a();
        let r = g
            .merge_game(
                &b(),
                &MergeOptions {
                    end_ply: None,
                    comment: c.clone(),
                },
            )
            .expect("merge");
        assert_eq!(g.comment_before(r.first).expect("node"), None, "{c:?}");
    }
}

#[test]
fn comment_with_unicode_and_lines_is_stored_verbatim_and_idempotent() {
    let text = "Müller, Åsa -- 李 1/2-1/2 (4 moves)\nÖffnung ☃, 2020\r\nthird line";
    let mut g = a();
    let r = g.merge_game(&b(), &with_comment(text)).expect("first");
    assert_eq!(g.comment_before(r.first).expect("node"), Some(text));
    let again = g.merge_game(&b(), &with_comment(text)).expect("second");
    assert_eq!(again.first, r.first);
    assert_eq!(g.comment_before(r.first).expect("node"), Some(text));
}

#[test]
fn comment_is_appended_when_the_old_one_does_not_contain_it_and_skipped_when_it_does() {
    let mut g = a();
    let r = g.merge_game(&b(), &with_comment("one")).expect("first");
    g.merge_game(&b(), &with_comment("two")).expect("second");
    assert_eq!(g.comment_before(r.first).expect("node"), Some("one\ntwo"));
    // Contained (substring of the whole old text): no change.
    g.merge_game(&b(), &with_comment("one\ntwo"))
        .expect("third");
    assert_eq!(g.comment_before(r.first).expect("node"), Some("one\ntwo"));
    g.merge_game(&b(), &with_comment("two")).expect("fourth");
    assert_eq!(g.comment_before(r.first).expect("node"), Some("one\ntwo"));
}

#[test]
fn comment_goes_before_the_first_move_of_an_extended_main_line() {
    let mut e = game("e4 e5");
    let r = e.merge_game(&b(), &with_comment("note")).expect("merge");
    assert!(r.extended_mainline);
    assert_eq!(san_of(&e, r.first), "Nf3");
    assert_eq!(e.comment_before(r.first).expect("node"), Some("note"));
    let others: Vec<NodeId> = e.mainline().skip(3).collect();
    for id in others {
        assert_eq!(e.comment_before(id).expect("node"), None);
    }
}

// ------------------------------------------- merge_comment_parts

#[test]
fn comment_parts_cover_every_result_and_unknown_date() {
    let cases = [
        (GameResult::White, "1-0"),
        (GameResult::Black, "0-1"),
        (GameResult::Draw, "1/2-1/2"),
        (GameResult::Unknown, "*"),
    ];
    for (res, text) in cases {
        let mut g = game("e4 e5");
        g.tags_mut().result = res;
        assert_eq!(merge_comment_parts(&g).result, text);
    }
    let mut g = game("e4 e5");
    g.tags_mut().date = PartialDate::new(None, Some(5), Some(3)).unwrap_or_default();
    assert_eq!(merge_comment_parts(&g).year, 0, "month/day without year");
}

#[test]
fn comment_parts_count_only_main_line_plies_and_round_up() {
    let mut g = game("e4 e5 Nf3");
    let nodes: Vec<NodeId> = g.mainline().collect();
    let p = g.position_at(nodes[0]).expect("node").clone();
    let alt = p.parse_san("c5", SanMode::Strict).expect("san").mv;
    g.add_move(nodes[0], alt).expect("var");
    let parts = merge_comment_parts(&g);
    assert_eq!((parts.half_moves, parts.full_moves()), (3, 2));
    assert_eq!(merge_comment_parts(&Game::new()).full_moves(), 0);
    assert_eq!(merge_comment_parts(&game("e4")).full_moves(), 1);
}

#[test]
fn comment_parts_english_keeps_unicode_names_and_year_zero() {
    let mut g = game("e4 e5 Nf3 Nc6");
    let t = g.tags_mut();
    t.white = "Łukasz Ñandú".into();
    t.black = "李小龍".into();
    t.event = "Café Open".into();
    let parts = merge_comment_parts(&g);
    assert_eq!(
        parts.english(true),
        "Łukasz Ñandú -- 李小龍 * (2 moves)\nCafé Open, 0"
    );
    assert_eq!(
        parts.english(false),
        "Łukasz Ñandú -- 李小龍 *\nCafé Open, 0"
    );
}

// ------------------------------------------ deep trees and performance

#[test]
fn deep_main_lines_do_not_overflow_the_stack() {
    let plies = 20_000;
    let mut g = shuffle(plies);
    let other = shuffle(plies);
    // Same game: nothing to add, but all structures are traversed.
    let before = g.clone();
    assert_eq!(
        g.merge_game(&other, &opts(None)),
        Err(MergeError::NothingToAdd)
    );
    assert!(g.same_content(&before));
    // A shorter target merges the tail of a longer source.
    let mut short = shuffle(plies - 3);
    let r = short.merge_game(&other, &opts(None)).expect("merge");
    assert!(r.extended_mainline);
    assert_eq!(r.added, 3);
    assert_eq!(short.mainline().count(), plies);
    drop(short);
    drop(g);
}

#[test]
fn deeply_nested_variations_in_the_target_are_handled() {
    // Each step adds a main move and a sibling variation, then descends into
    // the variation: nesting depth ~ 1500.
    let mut g = Game::new();
    let mut cur = g.root();
    for i in 0..1_500 {
        let pos = g.position_at(cur).expect("node").clone();
        let moves = pos.legal_moves();
        if moves.len() < 2 {
            break;
        }
        // Avoid always taking the first move: cycle through a few.
        let main = moves[i % moves.len()];
        let var = moves[(i + 1) % moves.len()];
        if g.add_move(cur, main).is_err() {
            break;
        }
        match g.add_move(cur, var) {
            Ok(id) => cur = id,
            Err(_) => break,
        }
    }
    assert!(g.node_count() > 100);
    let copy = g.clone();
    assert!(g.same_content(&copy));
    drop(g.merge_game(&game("e4 e5"), &opts(None))); // must not blow up
    let _ = g.descendants(g.root()).expect("root").count();
}

#[test]
fn merge_time_grows_roughly_linearly_with_length() {
    // Generous bound: 40 000 plies must merge in well under the time a
    // quadratic scan would take (and in a debug-friendly few seconds).
    let long = 40_000;
    let mut g = shuffle(long - 3);
    let other = shuffle(long);
    let t = Instant::now();
    let r = g.merge_game(&other, &opts(None)).expect("merge");
    let elapsed = t.elapsed();
    assert_eq!(r.added, 3);
    assert!(
        elapsed < Duration::from_secs(10),
        "merge of {long} plies took {elapsed:?}"
    );
}

// ------------------------------------------------- brute-force oracle

fn naive_match(own: &Game, other: &Game) -> Option<(usize, usize)> {
    let own_pos: Vec<&Position> = std::iter::once(own.start_position())
        .chain(own.mainline().map(|id| own.position_at(id).expect("node")))
        .collect();
    let other_pos: Vec<&Position> = std::iter::once(other.start_position())
        .chain(
            other
                .mainline()
                .map(|id| other.position_at(id).expect("node")),
        )
        .collect();
    let mut best = None;
    for (i, p) in own_pos.iter().enumerate() {
        if let Some(j) = other_pos.iter().rposition(|q| q == p) {
            best = Some((i, j));
        }
    }
    best
}

fn playout(start: &Position, choices: &[u16]) -> Game {
    let mut g = Game::from_position(start.clone());
    let mut at = g.root();
    for (_, mv) in common::random_game(start, choices) {
        at = g.add_move(at, mv).expect("legal");
    }
    g
}

/// Decorates a game with variations, comments and NAGs (never changes its
/// main line).
fn decorate(g: &mut Game, seed: u16) {
    let nodes: Vec<NodeId> = std::iter::once(g.root()).chain(g.mainline()).collect();
    for (i, &n) in nodes.iter().enumerate() {
        let pos = g.position_at(n).expect("node").clone();
        let moves = pos.legal_moves();
        if moves.is_empty() {
            continue;
        }
        if (usize::from(seed) + i).is_multiple_of(3) && !g.children(n).expect("node").is_empty() {
            let alt = moves[(usize::from(seed) + i) % moves.len()];
            drop(g.add_move(n, alt));
        }
        if n != g.root() {
            drop(g.set_comment(n, "c"));
            drop(g.add_nag(n, Nag::new(3).expect("nag")));
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn match_agrees_with_the_brute_force_oracle(
        start in 0usize..common::START_COUNT,
        shared in prop::collection::vec(any::<u16>(), 0..10),
        tail_a in prop::collection::vec(any::<u16>(), 0..25),
        tail_b in prop::collection::vec(any::<u16>(), 0..25),
        end in prop::option::of(0u32..40),
    ) {
        let start = common::start_position(start);
        let cat = |t: &[u16]| shared.iter().chain(t).copied().collect::<Vec<_>>();
        let mut x = playout(&start, &cat(&tail_a));
        let y = playout(&start, &cat(&tail_b));
        let expected = naive_match(&x, &y);
        let x_len = x.mainline().count();
        let y_len = y.mainline().count();
        let before = x.clone();
        let result = x.merge_game(&y, &opts(end));
        if x_len == 0 {
            prop_assert_eq!(result, Err(MergeError::NoMoves));
            return Ok(());
        }
        match (expected, result) {
            (None, r) => prop_assert_eq!(r, Err(MergeError::NoMatch)),
            (Some((i, j)), Err(MergeError::NothingToAdd)) => {
                let wanted = end.map_or(usize::MAX, |e| e as usize);
                let effective = wanted.max(j + 1).min(y_len);
                prop_assert!(effective <= j, "i={i} j={j} len={y_len}");
                prop_assert!(x.same_content(&before));
            }
            (Some((i, j)), Ok(r)) => {
                prop_assert_eq!((r.match_ply as usize, r.other_ply as usize), (i, j));
                let wanted = end.map_or(usize::MAX, |e| e as usize);
                let effective = wanted.max(j + 1).min(y_len);
                prop_assert_eq!((r.added + r.reused) as usize, effective - j);
                prop_assert_eq!(r.truncated, effective < y_len);
                prop_assert_eq!(r.extended_mainline, i == x_len);
                // The copied moves are exactly other[j..effective].
                let ys = main_ucis(&y);
                let mut pos_node = std::iter::once(before.root())
                    .chain(before.mainline())
                    .nth(i)
                    .expect("ply");
                for uci in &ys[j..effective] {
                    let pos = x.position_at(pos_node).expect("node").clone();
                    let mv = pos.parse_uci(uci).expect("uci");
                    let child = x.find_child(pos_node, mv).expect("node").expect("child exists");
                    pos_node = child;
                }
                prop_assert_eq!(x.parent(r.first).expect("node"),
                    Some(std::iter::once(before.root()).chain(before.mainline()).nth(i).expect("ply")));
            }
            (Some(_), Err(e)) => prop_assert!(false, "unexpected error {:?}", e),
        }
    }

    #[test]
    fn annotations_and_variations_of_the_other_game_never_change_the_result(
        start in 0usize..common::START_COUNT,
        shared in prop::collection::vec(any::<u16>(), 0..10),
        tail_a in prop::collection::vec(any::<u16>(), 0..20),
        tail_b in prop::collection::vec(any::<u16>(), 0..20),
        seed in any::<u16>(),
    ) {
        let start = common::start_position(start);
        let cat = |t: &[u16]| shared.iter().chain(t).copied().collect::<Vec<_>>();
        let base = playout(&start, &cat(&tail_a));
        let plain = playout(&start, &cat(&tail_b));
        let mut decorated = plain.clone();
        decorate(&mut decorated, seed);

        let mut x1 = base.clone();
        let mut x2 = base.clone();
        let r1 = x1.merge_game(&plain, &opts(None));
        let r2 = x2.merge_game(&decorated, &opts(None));
        prop_assert_eq!(r1.is_ok(), r2.is_ok());
        if let (Ok(a1), Ok(a2)) = (r1, r2) {
            prop_assert_eq!((a1.match_ply, a1.other_ply, a1.added, a1.reused),
                            (a2.match_ply, a2.other_ply, a2.added, a2.reused));
            prop_assert!(x1.same_content(&x2));
        }
    }

    #[test]
    fn merging_a_decorated_target_keeps_its_annotations(
        start in 0usize..common::START_COUNT,
        shared in prop::collection::vec(any::<u16>(), 0..10),
        tail_a in prop::collection::vec(any::<u16>(), 1..20),
        tail_b in prop::collection::vec(any::<u16>(), 0..20),
        seed in any::<u16>(),
    ) {
        let start = common::start_position(start);
        let cat = |t: &[u16]| shared.iter().chain(t).copied().collect::<Vec<_>>();
        let mut x = playout(&start, &cat(&tail_a));
        decorate(&mut x, seed);
        let y = playout(&start, &cat(&tail_b));
        let before = x.clone();
        if x.merge_game(&y, &opts(None)).is_ok() {
            // Every node that existed keeps its comment, NAGs and moves.
            for id in before.descendants(before.root()).expect("root") {
                prop_assert_eq!(x.comment(id).expect("node"), before.comment(id).expect("node"));
                prop_assert_eq!(x.nags(id).expect("node"), before.nags(id).expect("node"));
                prop_assert_eq!(x.mv(id).expect("node"), before.mv(id).expect("node"));
            }
        }
    }
}

// ---------------------------------------- atomicity (D3872), forced error

/// White lost the king-side right (`Qkq`), yet the other game castles
/// king-side through `add_move_lenient` (as the PGN reader keeps it). The
/// target game only accepts legal castling, so copying that move fails.
const LOST_RIGHT_FEN: &str = "r3k2r/8/8/8/8/8/P7/R3K2R w Qkq - 0 1";

fn lenient_other(sans_before_castle: &str) -> Game {
    let start = Position::from_fen(LOST_RIGHT_FEN).expect("fen");
    let mut g = game_from(start, sans_before_castle);
    let at = last_of(&g);
    let pos = g.position_at(at).expect("node").clone();
    let castle = pos.parse_san("O-O", SanMode::Tolerant).expect("san").mv;
    assert!(g.add_move(at, castle).is_err(), "strict add must refuse");
    g.add_move_lenient(at, castle).expect("lenient add");
    g
}

fn lost_right_target(sans: &str) -> Game {
    game_from(Position::from_fen(LOST_RIGHT_FEN).expect("fen"), sans)
}

#[test]
fn error_on_the_first_created_move_leaves_the_game_untouched() {
    let other = lenient_other("a3 Rh7");
    // Target shares `a3`, then goes elsewhere: match at ply 1.
    let mut g = lost_right_target("a3 Kd7");
    let before = g.clone();
    let err = g
        .merge_game(&other, &with_comment("must not stay"))
        .expect_err("castling is not legal in the target");
    assert!(matches!(err, MergeError::Game(_)), "got {err:?}");
    assert!(g.same_content(&before));
    assert_eq!(g.node_count(), before.node_count());
    for id in g.descendants(g.root()).expect("root") {
        assert_eq!(g.comment_before(id).expect("node"), None);
    }
}

#[test]
fn error_after_created_nodes_rolls_back_only_what_was_created() {
    // The target already holds the variation `Rh7` (reused by the merge), so
    // `first` (reused Rh7) differs from the first created node (`a4`).
    let other = lenient_other("a3 Rh7 a4 Rh6");
    let mut g = lost_right_target("a3 Kd7");
    let nodes: Vec<NodeId> = g.mainline().collect();
    let p1 = g.position_at(nodes[0]).expect("node").clone();
    let rh7 = p1.parse_san("Rh7", SanMode::Strict).expect("san").mv;
    let existing = g.add_move(nodes[0], rh7).expect("existing variation");
    let before = g.clone();
    let count = g.node_count();

    let err = g
        .merge_game(&other, &with_comment("note"))
        .expect_err("castling is not legal in the target");
    assert!(matches!(err, MergeError::Game(_)), "got {err:?}");
    assert!(g.same_content(&before));
    assert_eq!(g.node_count(), count);
    // The reused variation survives and has no children left over.
    assert!(g.contains(existing));
    assert_eq!(g.children(existing).expect("node").len(), 0);
    assert_eq!(g.comment_before(existing).expect("node"), None);
    // The game is still usable: a later successful merge works.
    let ok = g
        .merge_game(
            &game_from(
                Position::from_fen(LOST_RIGHT_FEN).expect("fen"),
                "a3 Rh7 a4",
            ),
            &opts(None),
        )
        .expect("later merge");
    assert_eq!((ok.reused, ok.added), (1, 1));
}

#[test]
fn error_after_only_created_nodes_removes_all_of_them() {
    let other = lenient_other("a3 Rh7 a4 Rh6");
    let mut g = lost_right_target("a3 Kd7");
    let before = g.clone();
    let err = g.merge_game(&other, &opts(None)).expect_err("fails");
    assert!(matches!(err, MergeError::Game(_)));
    assert!(g.same_content(&before));
    assert_eq!(g.node_count(), before.node_count());
    // Same parent has the same children as before (no dangling ids).
    let parent = g.mainline().next().expect("a3");
    assert_eq!(
        g.children(parent).expect("node"),
        before.children(parent).expect("node")
    );
}

#[test]
fn error_in_extended_main_line_mode_is_atomic_too() {
    // Target ends at the matching position: the failing copy would have
    // extended the main line.
    let other = lenient_other("a3 Rh7");
    let mut g = lost_right_target("a3");
    let before = g.clone();
    let err = g.merge_game(&other, &opts(None)).expect_err("fails");
    assert!(matches!(err, MergeError::Game(_)));
    assert!(g.same_content(&before));
    assert_eq!(g.mainline().count(), 1);
}

#[test]
fn end_ply_equal_and_one_less_than_other_length_set_truncated() {
    let other_len = 8;
    let mut g = a();
    let r = g.merge_game(&b(), &opts(Some(other_len))).expect("merge");
    assert!(!r.truncated);
    let mut g = a();
    let r = g
        .merge_game(&b(), &opts(Some(other_len - 1)))
        .expect("merge");
    assert!(r.truncated);
    let mut g = a();
    let r = g.merge_game(&b(), &opts(Some(0))).expect("merge");
    assert_eq!((r.added, r.truncated), (1, true));
}
