//! Integration tests for the game tree (`Game`, `NodeId`, iterators).
//!
//! Contract: `docs/specs/m1-2-arvore-e-cabecalhos.md` (rules 1-8, D8-D12) and
//! Scid's `Game::{AddMove, MainVariation, FirstVariation, DeleteVariation,
//! Truncate, TruncateStart}` in `src/game.cpp`.

mod common;

use mychess_core::{Game, GameError, Move, MoveError, Nag, NodeId, Position};
use proptest::prelude::*;

// ---------------------------------------------------------------- helpers

fn play(g: &mut Game, parent: NodeId, uci: &str) -> NodeId {
    let pos = g.position_at(parent).expect("live node").clone();
    let mv = pos
        .parse_uci(uci)
        .unwrap_or_else(|e| panic!("bad uci {uci}: {e}"));
    g.add_move(parent, mv).expect("legal move")
}

fn play_root(g: &mut Game, uci: &str) -> NodeId {
    let root = g.root();
    play(g, root, uci)
}

fn nag(n: u16) -> Nag {
    Nag::new(n).expect("valid nag")
}

fn kids(g: &Game, id: NodeId) -> Vec<NodeId> {
    g.children(id).expect("live").to_vec()
}

/// Every live node, root first, in pre-order.
fn all_nodes(g: &Game) -> Vec<NodeId> {
    std::iter::once(g.root())
        .chain(g.descendants(g.root()).expect("root"))
        .collect()
}

/// Structural invariants that must hold after any operation.
fn check_invariants(g: &Game) {
    let nodes = all_nodes(g);
    let mut seen = std::collections::HashSet::new();
    for &id in &nodes {
        assert!(seen.insert(id), "node {id} visited twice (cycle or dup)");
        assert!(g.contains(id));
        let pos = g.position_at(id).expect("pos");
        match g.parent(id).expect("parent") {
            None => {
                assert_eq!(id, g.root());
                assert_eq!(g.ply(id), Ok(0));
                assert_eq!(pos, g.start_position());
                assert_eq!(g.mv(id), Err(GameError::IsRoot));
            }
            Some(p) => {
                let mv = g.mv(id).expect("move");
                let expected = g.position_at(p).expect("pp").after(mv).expect("legal");
                assert_eq!(&expected, pos, "position of {id} differs from replay");
                assert!(g.children(p).expect("c").contains(&id));
                assert_eq!(g.ply(id).expect("ply"), g.ply(p).expect("ply") + 1);
                let pk = g.children(p).expect("c");
                assert_eq!(g.is_variation(id), Ok(pk[0] != id));
            }
        }
        // children are distinct moves, each pointing back at `id`.
        let ch = g.children(id).expect("c");
        let mut mvs = Vec::new();
        for &c in ch {
            assert_eq!(g.parent(c), Ok(Some(id)));
            mvs.push(g.mv(c).expect("mv"));
        }
        let before = mvs.len();
        mvs.sort_by_key(|m| format!("{m:?}"));
        mvs.dedup();
        assert_eq!(mvs.len(), before, "duplicate sibling moves under {id}");
        // path_to ends at the node and has `ply` entries.
        let path = g.path_to(id).expect("path");
        assert_eq!(path.len(), g.ply(id).expect("ply") as usize);
        assert_eq!(path.last().copied().or(Some(g.root())), Some(id));
        // ancestors end at the root.
        let anc: Vec<_> = g.ancestors(id).expect("anc").collect();
        assert_eq!(anc.len(), path.len());
        if id != g.root() {
            assert_eq!(anc.last().copied(), Some(g.root()));
        }
    }
    assert_eq!(nodes.len(), g.node_count(), "node_count != reachable nodes");
}

/// 1.e4 e5 (1...c5 2.Nf3 (2.c3) 2...d6) (1...e6) 2.Nf3
fn sample() -> (Game, [NodeId; 7]) {
    let mut g = Game::new();
    let e4 = play_root(&mut g, "e2e4");
    let e5 = play(&mut g, e4, "e7e5");
    let c5 = play(&mut g, e4, "c7c5");
    let nf3v = play(&mut g, c5, "g1f3");
    let c3 = play(&mut g, c5, "c2c3");
    let d6 = play(&mut g, nf3v, "d7d6");
    let e6 = play(&mut g, e4, "e7e6");
    (g, [e4, e5, c5, nf3v, c3, d6, e6])
}

/// A node with a main line and three variations: 1.e4 + e5 | c5 | e6 | c6.
fn four_way() -> (Game, NodeId, [NodeId; 4]) {
    let mut g = Game::new();
    let e4 = play_root(&mut g, "e2e4");
    let m0 = play(&mut g, e4, "e7e5");
    let m1 = play(&mut g, e4, "c7c5");
    let m2 = play(&mut g, e4, "e7e6");
    let m3 = play(&mut g, e4, "c7c6");
    (g, e4, [m0, m1, m2, m3])
}

// ----------------------------------------------------------- construction

#[test]
fn new_game_has_only_the_root() {
    let g = Game::new();
    assert_eq!(g.node_count(), 1);
    assert_eq!(g.mainline().count(), 0);
    assert_eq!(g.descendants(g.root()).expect("root").count(), 0);
    assert_eq!(g.ancestors(g.root()).expect("root").count(), 0);
    assert_eq!(g.path_to(g.root()), Ok(vec![]));
    assert_eq!(g.start_position(), &Position::new());
    check_invariants(&g);
}

#[test]
fn long_mainline_has_consistent_ply_and_positions() {
    let mut g = Game::new();
    let mut cur = g.root();
    let mut pos = Position::new();
    for i in 0..300u32 {
        let moves = pos.legal_moves();
        if moves.is_empty() {
            break;
        }
        let m = moves[(i as usize * 7) % moves.len()];
        cur = g.add_move(cur, m).expect("legal");
        pos = pos.after(m).expect("legal");
        assert_eq!(g.position_at(cur).expect("pos"), &pos);
        assert_eq!(g.ply(cur), Ok(i + 1));
    }
    let main: Vec<_> = g.mainline().collect();
    assert_eq!(main.last(), Some(&cur));
    assert_eq!(main.len(), g.node_count() - 1);
    check_invariants(&g);
}

#[test]
fn nested_variations_several_levels_deep() {
    // 1.e4 e5 (1...c5 2.Nf3 (2.c3 Nf6 (2...d5 3.exd5 (3.e5) 3...Qxd5)) 2...d6)
    let mut g = Game::new();
    let e4 = play_root(&mut g, "e2e4");
    let e5 = play(&mut g, e4, "e7e5");
    let c5 = play(&mut g, e4, "c7c5");
    let nf3 = play(&mut g, c5, "g1f3");
    let c3 = play(&mut g, c5, "c2c3");
    let nf6 = play(&mut g, c3, "g8f6");
    let d5 = play(&mut g, c3, "d7d5");
    let exd5 = play(&mut g, d5, "e4d5");
    let e5v = play(&mut g, d5, "e4e5");
    let qxd5 = play(&mut g, exd5, "d8d5");
    let d6 = play(&mut g, nf3, "d7d6");
    assert_eq!(g.ply(qxd5), Ok(6));
    assert_eq!(g.ply(e5v), Ok(5));
    assert_eq!(kids(&g, d5), vec![exd5, e5v]);
    assert_eq!(kids(&g, c3), vec![nf6, d5]);
    assert_eq!(
        g.path_to(qxd5).expect("p"),
        vec![e4, c5, c3, d5, exd5, qxd5]
    );
    assert_eq!(g.is_variation(d5), Ok(true));
    assert_eq!(g.is_variation(exd5), Ok(false));
    let _ = (e5, d6);
    assert_eq!(g.node_count(), 12);
    check_invariants(&g);
}

#[test]
fn add_move_twice_returns_same_node_without_growth() {
    let mut g = Game::new();
    let a = play_root(&mut g, "e2e4");
    let e5 = play(&mut g, a, "e7e5");
    let c5 = play(&mut g, a, "c7c5");
    let n = g.node_count();
    assert_eq!(play(&mut g, a, "c7c5"), c5);
    assert_eq!(play(&mut g, a, "e7e5"), e5);
    assert_eq!(g.node_count(), n);
    assert_eq!(kids(&g, a), vec![e5, c5]);
}

#[test]
fn illegal_move_is_rejected_and_tree_is_unchanged() {
    let mut g = Game::new();
    let e4 = play_root(&mut g, "e2e4");
    let before_count = g.node_count();
    let before_kids = kids(&g, e4);
    // A white move in black's position (e2e4 is not legal after 1.e4).
    let white_move = Position::new().parse_uci("d2d4").expect("uci");
    let err = g.add_move(e4, white_move).expect_err("illegal");
    assert!(matches!(
        err,
        GameError::IllegalMove(MoveError::Illegal { .. })
    ));
    assert_eq!(g.node_count(), before_count);
    assert_eq!(kids(&g, e4), before_kids);
    // The same move is fine from the root, where it is legal.
    assert!(g.add_move(g.root(), white_move).is_ok());
    check_invariants(&g);
}

#[test]
fn null_move_is_a_node_with_flipped_side() {
    let mut g = Game::new();
    let n = g.add_move(g.root(), Move::Null).expect("null");
    assert_eq!(
        g.position_at(n).expect("p").turn(),
        mychess_core::Color::Black
    );
    assert_eq!(g.ply(n), Ok(1));
    let again = g.add_move(g.root(), Move::Null).expect("null");
    assert_eq!(n, again, "same null move must not duplicate");
    check_invariants(&g);
}

#[test]
fn fen_start_game_uses_relative_ply_and_fen_move_numbers() {
    let pos = common::fen("8/8/8/8/8/5k2/8/R3K3 b - - 0 40");
    let mut g = Game::from_position(pos.clone());
    let n1 = play_root(&mut g, "f3e3");
    let n2 = play(&mut g, n1, "a1a2");
    assert_eq!(g.ply(g.root()), Ok(0));
    assert_eq!(g.ply(n1), Ok(1));
    assert_eq!(g.ply(n2), Ok(2));
    assert_eq!(g.position_at(g.root()).expect("p").fullmove_number(), 40);
    assert_eq!(g.position_at(n1).expect("p").fullmove_number(), 41);
    assert_eq!(g.position_at(n2).expect("p").fullmove_number(), 41);
    assert_eq!(g.start_position(), &pos);
    check_invariants(&g);
}

// ------------------------------------------------------------ query errors

#[test]
fn root_has_no_move_and_no_parent() {
    let g = Game::new();
    assert_eq!(g.mv(g.root()), Err(GameError::IsRoot));
    assert_eq!(g.parent(g.root()), Ok(None));
    assert_eq!(g.is_variation(g.root()), Ok(false));
}

#[test]
fn id_from_another_game_is_invalid() {
    let (big, [e4, _, _, nf3v, ..]) = sample();
    let mut small = Game::new();
    let small_e4 = play_root(&mut small, "e2e4");
    let _ = (e4, small_e4);
    // `nf3v` has an arena index the small game never allocated.
    assert!(!small.contains(nf3v));
    assert_eq!(small.children(nf3v), Err(GameError::InvalidNode(nf3v)));
    assert_eq!(small.parent(nf3v), Err(GameError::InvalidNode(nf3v)));
    assert_eq!(small.mv(nf3v), Err(GameError::InvalidNode(nf3v)));
    assert_eq!(
        small.promote_variation(nf3v),
        Err(GameError::InvalidNode(nf3v))
    );
    assert_eq!(
        small.delete_variation(nf3v),
        Err(GameError::InvalidNode(nf3v))
    );
    assert_eq!(small.truncate_from(nf3v), Err(GameError::InvalidNode(nf3v)));
    assert!(small.truncate_start(nf3v).is_err());
    assert!(small.descendants(nf3v).is_err());
    let m = Position::new().parse_uci("e2e4").expect("uci");
    assert_eq!(small.add_move(nf3v, m), Err(GameError::InvalidNode(nf3v)));
    assert_eq!(small.node_count(), 2);
    drop(big);
}

#[test]
fn deleted_ids_are_invalid_for_every_method() {
    let (mut g, [e4, _, c5, nf3v, c3, d6, _]) = sample();
    g.delete_variation(c5).expect("delete");
    let mv = Position::new().parse_uci("e2e4").expect("uci");
    for id in [c5, nf3v, c3, d6] {
        let bad = GameError::InvalidNode(id);
        assert!(!g.contains(id));
        assert_eq!(g.children(id), Err(bad.clone()));
        assert_eq!(g.variations(id), Err(bad.clone()));
        assert_eq!(g.parent(id), Err(bad.clone()));
        assert_eq!(g.mv(id), Err(bad.clone()));
        assert_eq!(g.ply(id), Err(bad.clone()));
        assert_eq!(g.position_at(id).map(|_| ()), Err(bad.clone()));
        assert_eq!(g.is_variation(id), Err(bad.clone()));
        assert_eq!(g.path_to(id), Err(bad.clone()));
        assert!(g.descendants(id).is_err());
        assert!(g.ancestors(id).is_err());
        assert!(g.mainline_from(id).is_err());
        assert_eq!(g.add_move(id, mv), Err(bad.clone()));
        assert_eq!(g.promote_variation(id), Err(bad.clone()));
        assert_eq!(g.make_first(id), Err(bad.clone()));
        assert_eq!(g.delete_variation(id), Err(bad.clone()));
        assert_eq!(g.truncate_from(id), Err(bad.clone()));
        assert!(g.truncate_start(id).is_err());
        assert_eq!(g.comment(id), Err(bad.clone()));
        assert_eq!(g.set_comment(id, "x"), Err(bad.clone()));
        assert_eq!(g.append_comment(id, "x"), Err(bad.clone()));
        assert_eq!(g.comment_before(id), Err(bad.clone()));
        assert_eq!(g.set_comment_before(id, "x"), Err(bad.clone()));
        assert_eq!(g.nags(id), Err(bad.clone()));
        assert_eq!(g.add_nag(id, nag(1)), Err(bad.clone()));
        assert_eq!(g.remove_nag(id, nag(1)), Err(bad.clone()));
        assert_eq!(g.clear_nags(id), Err(bad));
    }
    assert!(g.contains(e4));
}

#[test]
fn ids_are_not_reused_after_deletion() {
    let (mut g, [e4, _, c5, ..]) = sample();
    g.delete_variation(c5).expect("delete");
    let fresh = play(&mut g, e4, "c7c5");
    assert_ne!(fresh, c5, "a stale id must never alias a new node");
    assert_eq!(g.mv(c5), Err(GameError::InvalidNode(c5)));
    check_invariants(&g);
}

// --------------------------------------------------------------- iterators

#[test]
fn descendants_is_depth_first_preorder_in_pgn_order() {
    let (g, [e4, e5, c5, nf3v, c3, d6, e6]) = sample();
    let order: Vec<_> = g.descendants(g.root()).expect("d").collect();
    assert_eq!(order, vec![e4, e5, c5, nf3v, d6, c3, e6]);
    let sub: Vec<_> = g.descendants(c5).expect("d").collect();
    assert_eq!(sub, vec![nf3v, d6, c3]);
}

#[test]
fn mainline_and_mainline_from_follow_first_children() {
    let (g, [e4, e5, _, _, _, _, _]) = sample();
    assert_eq!(g.mainline().collect::<Vec<_>>(), vec![e4, e5]);
    assert_eq!(
        g.mainline_from(e4).expect("m").collect::<Vec<_>>(),
        vec![e5]
    );
    assert_eq!(g.mainline_from(e5).expect("m").count(), 0);
    assert_eq!(g.mainline_from(g.root()).expect("m").count(), 2);
}

#[test]
fn ancestors_run_from_parent_to_root() {
    let (g, [e4, _, c5, nf3v, ..]) = sample();
    let anc: Vec<_> = g.ancestors(nf3v).expect("a").collect();
    assert_eq!(anc, vec![c5, e4, g.root()]);
    assert_eq!(g.path_to(nf3v).expect("p"), vec![e4, c5, nf3v]);
}

// ------------------------------------------------------------ promotion etc

#[test]
fn promote_variation_makes_node_main_and_old_main_a_variation() {
    let (mut g, [e4, e5, c5, _, _, _, e6]) = sample();
    g.promote_variation(c5).expect("promote");
    let k = kids(&g, e4);
    assert_eq!(k[0], c5, "promoted node is children[0]");
    assert!(k.contains(&e5) && k.contains(&e6));
    assert_eq!(g.is_variation(e5), Ok(true));
    assert_eq!(g.is_variation(c5), Ok(false));
    assert_eq!(g.mainline().collect::<Vec<_>>()[..2], [e4, c5]);
    check_invariants(&g);
}

#[test]
fn promote_variation_keeps_subtrees_and_node_data_attached() {
    let (mut g, [e4, e5, c5, nf3v, c3, d6, _]) = sample();
    g.set_comment(c5, "sicilian").expect("c");
    g.set_comment_before(c5, "before").expect("c");
    g.add_nag(c5, nag(5)).expect("n");
    g.set_comment(e5, "open").expect("c");
    g.promote_variation(c5).expect("promote");
    assert_eq!(kids(&g, c5), vec![nf3v, c3]);
    assert_eq!(kids(&g, nf3v), vec![d6]);
    assert_eq!(g.comment(c5), Ok(Some("sicilian")));
    // D12: comment_before stays attached to the node (no "(text)" conversion).
    assert_eq!(g.comment_before(c5), Ok(Some("before")));
    assert_eq!(g.nags(c5).expect("n"), &[nag(5)]);
    assert_eq!(g.comment(e5), Ok(Some("open")));
    assert_eq!(g.parent(c5), Ok(Some(e4)));
}

#[test]
fn promote_variation_places_old_main_right_after_the_new_main() {
    // Spec rule 2: "a antiga linha principal vira variacao (a posicao
    // imediatamente depois)"; Scid MainVariation (game.cpp:1118) moves the
    // chosen variation to the front of the variation list and then swaps it
    // with the main line, so [main, v1, v2, v3] with v2 promoted gives
    // [v2, main, v1, v3].
    let (mut g, e4, [main, v1, v2, v3]) = four_way();
    g.promote_variation(v2).expect("promote");
    assert_eq!(kids(&g, e4), vec![v2, main, v1, v3]);
}

#[test]
fn promote_first_variation_swaps_with_main() {
    let (mut g, e4, [main, v1, v2, v3]) = four_way();
    g.promote_variation(v1).expect("promote");
    assert_eq!(kids(&g, e4), vec![v1, main, v2, v3]);
}

#[test]
fn promote_variation_errors() {
    let (mut g, [e4, e5, ..]) = sample();
    assert_eq!(g.promote_variation(e5), Err(GameError::NotVariation(e5)));
    assert_eq!(g.promote_variation(e4), Err(GameError::NotVariation(e4)));
    assert_eq!(g.promote_variation(g.root()), Err(GameError::IsRoot));
}

#[test]
fn promote_twice_restores_the_original_order_for_one_variation() {
    let (mut g, e4, [main, v1, ..]) = {
        let mut g = Game::new();
        let e4 = play_root(&mut g, "e2e4");
        let a = play(&mut g, e4, "e7e5");
        let b = play(&mut g, e4, "c7c5");
        (g, e4, [a, b])
    };
    g.promote_variation(v1).expect("p");
    g.promote_variation(main).expect("p");
    assert_eq!(kids(&g, e4), vec![main, v1]);
}

#[test]
fn promote_nested_variation_leaves_outer_levels_alone() {
    let (mut g, [e4, e5, c5, nf3v, c3, _, e6]) = sample();
    g.promote_variation(c3).expect("promote");
    assert_eq!(kids(&g, c5)[0], c3);
    assert_eq!(kids(&g, e4), vec![e5, c5, e6]);
    assert!(g.is_variation(nf3v).expect("v"));
    check_invariants(&g);
}

#[test]
fn make_first_moves_variation_to_slot_one_and_keeps_main() {
    // Scid FirstVariation (game.cpp:1080): variation order only.
    let (mut g, e4, [main, v1, v2, v3]) = four_way();
    g.make_first(v3).expect("first");
    assert_eq!(kids(&g, e4), vec![main, v3, v1, v2]);
    g.make_first(v3).expect("noop");
    assert_eq!(kids(&g, e4), vec![main, v3, v1, v2]);
    g.make_first(v2).expect("first");
    assert_eq!(kids(&g, e4), vec![main, v2, v3, v1]);
    check_invariants(&g);
}

#[test]
fn make_first_errors() {
    let (mut g, [e4, e5, ..]) = sample();
    assert_eq!(g.make_first(e5), Err(GameError::NotVariation(e5)));
    assert_eq!(g.make_first(e4), Err(GameError::NotVariation(e4)));
    assert_eq!(g.make_first(g.root()), Err(GameError::IsRoot));
}

#[test]
fn delete_variation_removes_whole_subtree() {
    let (mut g, [e4, e5, c5, nf3v, c3, d6, e6]) = sample();
    g.delete_variation(c5).expect("delete");
    assert_eq!(kids(&g, e4), vec![e5, e6]);
    assert_eq!(g.node_count(), 4);
    for id in [c5, nf3v, c3, d6] {
        assert!(!g.contains(id));
    }
    check_invariants(&g);
}

#[test]
fn delete_nested_variation_keeps_outer_lines() {
    let (mut g, [_, _, c5, nf3v, c3, d6, _]) = sample();
    g.delete_variation(c3).expect("delete");
    assert_eq!(kids(&g, c5), vec![nf3v]);
    assert!(g.contains(d6));
    assert_eq!(g.node_count(), 7);
    check_invariants(&g);
}

#[test]
fn delete_variation_errors_leave_tree_untouched() {
    let (mut g, [e4, e5, ..]) = sample();
    let n = g.node_count();
    assert_eq!(g.delete_variation(e5), Err(GameError::NotVariation(e5)));
    assert_eq!(g.delete_variation(e4), Err(GameError::NotVariation(e4)));
    assert_eq!(g.delete_variation(g.root()), Err(GameError::IsRoot));
    assert_eq!(g.node_count(), n);
}

#[test]
fn truncate_from_main_node_removes_subtree_and_promotes_first_variation() {
    let (mut g, [e4, e5, c5, nf3v, _, _, e6]) = sample();
    g.truncate_from(e5).expect("truncate");
    assert_eq!(kids(&g, e4), vec![c5, e6]);
    assert!(!g.contains(e5));
    assert!(g.contains(nf3v));
    assert_eq!(g.is_variation(c5), Ok(false));
    check_invariants(&g);
}

#[test]
fn truncate_from_variation_removes_its_subtree_only() {
    let (mut g, [e4, e5, c5, nf3v, c3, d6, e6]) = sample();
    g.truncate_from(c5).expect("truncate");
    assert_eq!(kids(&g, e4), vec![e5, e6]);
    for id in [c5, nf3v, c3, d6] {
        assert!(!g.contains(id));
    }
    assert_eq!(g.node_count(), 4);
    check_invariants(&g);
}

#[test]
fn truncate_from_removes_following_moves_in_a_line() {
    let mut g = Game::new();
    let e4 = play_root(&mut g, "e2e4");
    let e5 = play(&mut g, e4, "e7e5");
    let nf3 = play(&mut g, e5, "g1f3");
    let nc6 = play(&mut g, nf3, "b8c6");
    g.truncate_from(nf3).expect("t");
    assert_eq!(g.mainline().collect::<Vec<_>>(), vec![e4, e5]);
    assert!(!g.contains(nf3) && !g.contains(nc6));
    assert_eq!(g.truncate_from(nf3), Err(GameError::InvalidNode(nf3)));
}

#[test]
fn truncate_from_root_is_is_root_error() {
    let (mut g, _) = sample();
    let n = g.node_count();
    assert_eq!(g.truncate_from(g.root()), Err(GameError::IsRoot));
    assert_eq!(g.node_count(), n);
}

#[test]
fn truncate_start_builds_fen_game_with_same_tags_and_continuation() {
    let (mut g, [e4, e5, c5, _, _, _, e6]) = sample();
    g.tags_mut().white = "Anna".into();
    g.set_comment(e4, "after e4").expect("c");
    g.add_nag(c5, nag(1)).expect("n");
    let t = g.truncate_start(e4).expect("start");
    assert_eq!(t.tags().white, "Anna");
    assert_eq!(t.start_position(), g.position_at(e4).expect("p"));
    assert_ne!(t.start_position(), &Position::new());
    assert_eq!(t.node_count(), g.node_count() - 1, "all but e4 itself");
    let k = t.children(t.root()).expect("k").to_vec();
    assert_eq!(k.len(), 3, "e5, c5, e6 become root children");
    let mvs: Vec<_> = k.iter().map(|&n| t.mv(n).expect("m")).collect();
    let expected: Vec<_> = [e5, c5, e6].iter().map(|&n| g.mv(n).expect("m")).collect();
    assert_eq!(mvs, expected);
    assert_eq!(t.nags(k[1]).expect("n"), &[nag(1)]);
    assert_eq!(t.ply(k[0]), Ok(1));
    check_invariants(&t);
    // The original is untouched.
    assert_eq!(g.node_count(), 8);
    check_invariants(&g);
}

#[test]
fn truncate_start_at_root_is_an_equivalent_copy() {
    let (g, _) = sample();
    let t = g.truncate_start(g.root()).expect("copy");
    assert_eq!(t.start_position(), g.start_position());
    let a: Vec<_> = t
        .descendants(t.root())
        .expect("d")
        .map(|n| t.mv(n))
        .collect();
    let b: Vec<_> = g
        .descendants(g.root())
        .expect("d")
        .map(|n| g.mv(n))
        .collect();
    assert_eq!(a, b);
}

#[test]
fn truncate_start_at_leaf_gives_an_empty_game_from_that_position() {
    let (g, [_, e5, ..]) = sample();
    let t = g.truncate_start(e5).expect("start");
    assert_eq!(t.node_count(), 1);
    assert_eq!(t.start_position(), g.position_at(e5).expect("p"));
}

#[test]
fn truncate_start_inherits_node_comment_as_initial_comment() {
    let (mut g, [e4, ..]) = sample();
    g.set_comment(e4, "after e4").expect("c");
    let t = g.truncate_start(e4).expect("start");
    assert_eq!(t.initial_comment(), Some("after e4"));
}

#[test]
fn strip_variations_keeps_mainline_comments_and_nags() {
    let (mut g, [e4, e5, ..]) = sample();
    let nf3 = play(&mut g, e5, "g1f3");
    g.set_comment(nf3, "c").expect("c");
    g.set_comment_before(nf3, "b").expect("c");
    g.add_nag(e5, nag(2)).expect("n");
    g.strip_variations();
    assert_eq!(g.mainline().collect::<Vec<_>>(), vec![e4, e5, nf3]);
    assert_eq!(g.node_count(), 4);
    assert_eq!(g.comment(nf3), Ok(Some("c")));
    assert_eq!(g.comment_before(nf3), Ok(Some("b")));
    assert_eq!(g.nags(e5).expect("n"), &[nag(2)]);
    assert_eq!(g.variations(e4).expect("v"), []);
    check_invariants(&g);
}

#[test]
fn strip_variations_removes_nested_variations_inside_the_mainline() {
    let mut g = Game::new();
    let e4 = play_root(&mut g, "e2e4");
    let e5 = play(&mut g, e4, "e7e5");
    let nf3 = play(&mut g, e5, "g1f3");
    let _nc3 = play(&mut g, e5, "b1c3");
    let nc6 = play(&mut g, nf3, "b8c6");
    let _nf6 = play(&mut g, nf3, "g8f6");
    g.strip_variations();
    assert_eq!(g.mainline().collect::<Vec<_>>(), vec![e4, e5, nf3, nc6]);
    assert_eq!(g.node_count(), 5);
    check_invariants(&g);
}

#[test]
fn strip_on_game_without_variations_is_a_noop() {
    let mut g = Game::new();
    play_root(&mut g, "e2e4");
    g.strip_variations();
    g.strip_comments();
    g.strip_nags();
    assert_eq!(g.node_count(), 2);
}

#[test]
fn strip_comments_removes_all_comment_kinds_but_keeps_nags() {
    let (mut g, [e4, e5, ..]) = sample();
    g.set_initial_comment("intro");
    g.set_comment(e4, "a").expect("c");
    g.set_comment_before(e5, "b").expect("c");
    g.add_nag(e4, nag(3)).expect("n");
    g.strip_comments();
    assert_eq!(g.initial_comment(), None);
    assert_eq!(g.comment(e4), Ok(None));
    assert_eq!(g.comment_before(e5), Ok(None));
    assert_eq!(g.nags(e4).expect("n"), &[nag(3)], "D9: NAGs survive");
}

#[test]
fn strip_nags_removes_nags_but_keeps_comments() {
    let (mut g, [e4, ..]) = sample();
    g.set_comment(e4, "a").expect("c");
    g.add_nag(e4, nag(3)).expect("n");
    g.strip_nags();
    assert_eq!(g.nags(e4).expect("n"), []);
    assert_eq!(g.comment(e4), Ok(Some("a")));
}

// --------------------------------------------------------- comments & NAGs

#[test]
fn set_comment_with_blank_text_removes_the_comment() {
    let mut g = Game::new();
    let e4 = play_root(&mut g, "e2e4");
    g.set_comment(e4, "x").expect("c");
    for blank in ["", " ", "  \t\n "] {
        g.set_comment(e4, "x").expect("c");
        g.set_comment(e4, blank).expect("c");
        assert_eq!(g.comment(e4), Ok(None), "blank {blank:?}");
    }
}

#[test]
fn set_comment_preserves_text_verbatim() {
    let mut g = Game::new();
    let e4 = play_root(&mut g, "e2e4");
    let text = " spaces {braces} [%eval 0.3]\nnew line é 日本 ";
    g.set_comment(e4, text).expect("c");
    assert_eq!(g.comment(e4), Ok(Some(text)));
}

#[test]
fn append_comment_inserts_one_separator_when_needed() {
    let mut g = Game::new();
    let e4 = play_root(&mut g, "e2e4");
    g.append_comment(e4, "one").expect("c");
    assert_eq!(g.comment(e4), Ok(Some("one")), "append to empty");
    g.append_comment(e4, "two").expect("c");
    assert_eq!(g.comment(e4), Ok(Some("one two")));
    g.append_comment(e4, "  ").expect("c");
    assert_eq!(
        g.comment(e4),
        Ok(Some("one two")),
        "blank append is a no-op"
    );
    g.set_comment(e4, "end ").expect("c");
    g.append_comment(e4, "x").expect("c");
    assert_eq!(g.comment(e4), Ok(Some("end x")), "no double space");
    g.set_comment(e4, "line\n").expect("c");
    g.append_comment(e4, "x").expect("c");
    assert_eq!(g.comment(e4), Ok(Some("line\nx")));
}

#[test]
fn comment_before_is_independent_and_rejected_on_root() {
    let mut g = Game::new();
    let e4 = play_root(&mut g, "e2e4");
    g.set_comment_before(e4, "pre").expect("c");
    g.set_comment(e4, "post").expect("c");
    assert_eq!(g.comment_before(e4), Ok(Some("pre")));
    assert_eq!(g.comment(e4), Ok(Some("post")));
    g.set_comment_before(e4, " ").expect("c");
    assert_eq!(g.comment_before(e4), Ok(None));
    assert_eq!(g.comment(e4), Ok(Some("post")));
    assert_eq!(g.set_comment_before(g.root(), "x"), Err(GameError::IsRoot));
    assert_eq!(g.comment_before(g.root()), Ok(None));
}

#[test]
fn initial_comment_set_get_and_clear() {
    let mut g = Game::new();
    assert_eq!(g.initial_comment(), None);
    g.set_initial_comment("intro");
    assert_eq!(g.initial_comment(), Some("intro"));
    assert_eq!(g.comment(g.root()), Ok(Some("intro")));
    g.set_initial_comment("   ");
    assert_eq!(g.initial_comment(), None);
}

#[test]
fn nags_have_no_duplicates_and_keep_insertion_order() {
    let mut g = Game::new();
    let e4 = play_root(&mut g, "e2e4");
    assert_eq!(g.add_nag(e4, nag(14)), Ok(true));
    assert_eq!(g.add_nag(e4, nag(1)), Ok(true));
    assert_eq!(g.add_nag(e4, nag(200)), Ok(true));
    assert_eq!(g.add_nag(e4, nag(14)), Ok(false));
    assert_eq!(g.add_nag(e4, nag(1)), Ok(false));
    assert_eq!(g.nags(e4).expect("n"), &[nag(14), nag(1), nag(200)]);
    assert_eq!(g.remove_nag(e4, nag(1)), Ok(true));
    assert_eq!(g.remove_nag(e4, nag(1)), Ok(false));
    assert_eq!(g.nags(e4).expect("n"), &[nag(14), nag(200)]);
    g.clear_nags(e4).expect("clear");
    assert_eq!(g.nags(e4).expect("n"), []);
}

#[test]
fn many_distinct_nags_have_no_limit_in_the_core() {
    // D8: no limit in the core (the SI4 codec imposes the format limit).
    let mut g = Game::new();
    let e4 = play_root(&mut g, "e2e4");
    for n in 1..=255u16 {
        assert_eq!(g.add_nag(e4, nag(n)), Ok(true));
    }
    assert_eq!(g.nags(e4).expect("n").len(), 255);
}

#[test]
fn nags_on_root_are_rejected() {
    let mut g = Game::new();
    assert_eq!(g.add_nag(g.root(), nag(1)), Err(GameError::IsRoot));
    assert_eq!(g.remove_nag(g.root(), nag(1)), Err(GameError::IsRoot));
    assert_eq!(g.clear_nags(g.root()), Err(GameError::IsRoot));
    assert_eq!(g.nags(g.root()).expect("n"), []);
}

#[test]
fn clone_is_independent_of_the_original() {
    let (g, [e4, ..]) = sample();
    let mut c = g.clone();
    c.truncate_from(e4).expect("t");
    assert_eq!(c.node_count(), 1);
    assert_eq!(g.node_count(), 8);
    check_invariants(&g);
}

// ------------------------------------------------------------------ scale

#[test]
fn twenty_thousand_plies_traverse_clone_truncate_and_drop() {
    let mut g = Game::new();
    let mut cur = g.root();
    for _ in 0..5000 {
        for m in ["g1f3", "g8f6", "f3g1", "f6g8"] {
            cur = play(&mut g, cur, m);
        }
    }
    assert_eq!(g.node_count(), 20_001);
    assert_eq!(g.mainline().count(), 20_000);
    assert_eq!(g.descendants(g.root()).expect("d").count(), 20_000);
    assert_eq!(g.ancestors(cur).expect("a").count(), 20_000);
    assert_eq!(g.path_to(cur).expect("p").len(), 20_000);
    assert_eq!(g.ply(cur), Ok(20_000));
    let copy = g.clone();
    let t = g.truncate_start(g.root()).expect("copy");
    assert_eq!(t.node_count(), 20_001);
    drop(t);
    let first = g.mainline().next().expect("first");
    g.truncate_from(first).expect("truncate");
    assert_eq!(g.node_count(), 1);
    drop(copy); // deep drop must not overflow the stack
    // Depth with a variation hanging off each node.
    let mut h = Game::new();
    let mut cur = h.root();
    for i in 0..2000 {
        let pos = h.position_at(cur).expect("p").clone();
        let moves = pos.legal_moves();
        if moves.len() < 2 {
            break;
        }
        let m = moves[i % moves.len()];
        let alt = moves[(i + 1) % moves.len()];
        h.add_move(cur, alt).expect("alt");
        cur = h.add_move(cur, m).expect("main");
    }
    let n = h.node_count();
    h.strip_variations();
    assert!(h.node_count() < n);
    check_invariants(&h);
}

#[test]
fn thousands_of_variations_in_breadth_are_handled() {
    // Many moves under one node: use a position with many legal moves.
    let pos = common::fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1");
    let mut g = Game::from_position(pos.clone());
    let mut roots = Vec::new();
    // Breadth at many nodes: each of the 48 moves, then each reply.
    for m in pos.legal_moves() {
        let a = g.add_move(g.root(), m).expect("a");
        roots.push(a);
        let p1 = pos.after(m).expect("after");
        for r in p1.legal_moves() {
            g.add_move(a, r).expect("b");
        }
    }
    let n = g.node_count();
    assert!(n > 1500, "expected a wide tree, got {n}");
    assert_eq!(g.descendants(g.root()).expect("d").count(), n - 1);
    assert_eq!(g.variations(g.root()).expect("v").len(), 47);
    check_invariants(&g);
    // Delete every variation at the root, one by one, then the rest.
    let vars = g.variations(g.root()).expect("v").to_vec();
    for v in vars {
        g.delete_variation(v).expect("delete");
    }
    assert_eq!(g.children(g.root()).expect("c").len(), 1);
    check_invariants(&g);
}

#[test]
fn drop_of_a_wide_and_deep_tree_does_not_overflow() {
    let mut g = Game::new();
    let mut cur = g.root();
    for _ in 0..4000 {
        cur = play(&mut g, cur, "g1f3");
        cur = play(&mut g, cur, "g8f6");
        cur = play(&mut g, cur, "f3g1");
        cur = play(&mut g, cur, "f6g8");
    }
    // Branch at the end with every legal move.
    let pos = g.position_at(cur).expect("p").clone();
    for m in pos.legal_moves() {
        g.add_move(cur, m).expect("m");
    }
    assert!(g.node_count() > 16_000);
    drop(g);
}

// ---------------------------------------------------------- Send + Sync

#[test]
fn game_and_ids_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Game>();
    assert_send_sync::<NodeId>();
    assert_send_sync::<GameError>();
}

// ---------------------------------------------------------- memory report

#[test]
fn reports_memory_per_node_estimate() {
    // Observation only (no assertion on size): one `Position` is stored in
    // every node, so memory per node is about size_of::<Position>() plus the
    // node bookkeeping (Option<Move>, parent, ply, 2 comments, nags, children).
    let pos = size_of::<Position>();
    let game = size_of::<Game>();
    let mut g = Game::new();
    let mut cur = g.root();
    for _ in 0..5000 {
        for m in ["g1f3", "g8f6", "f3g1", "f6g8"] {
            cur = play(&mut g, cur, m);
        }
    }
    let _ = cur;
    eprintln!(
        "MEMORY: size_of::<Position>() = {pos} B, size_of::<Game>() = {game} B; \
         estimated >= {pos} B/node (+ ~120 B bookkeeping) => 20k plies ~ {} KiB",
        20_000 * (pos + 120) / 1024
    );
    assert!(pos > 0);
}

// ---------------------------------------------------------------- proptest

#[derive(Debug, Clone)]
enum Op {
    Add(u16, u16),
    Promote(u16),
    MakeFirst(u16),
    Delete(u16),
    Truncate(u16),
    TruncateStartCheck(u16),
    StripVariations,
    StripComments,
    StripNags,
    SetComment(u16, bool),
    AppendComment(u16),
    SetBefore(u16, bool),
    AddNag(u16, u16),
    RemoveNag(u16, u16),
    ClearNags(u16),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        8 => (any::<u16>(), any::<u16>()).prop_map(|(a, b)| Op::Add(a, b)),
        2 => any::<u16>().prop_map(Op::Promote),
        2 => any::<u16>().prop_map(Op::MakeFirst),
        1 => any::<u16>().prop_map(Op::Delete),
        1 => any::<u16>().prop_map(Op::Truncate),
        1 => any::<u16>().prop_map(Op::TruncateStartCheck),
        1 => Just(Op::StripVariations),
        1 => Just(Op::StripComments),
        1 => Just(Op::StripNags),
        2 => (any::<u16>(), any::<bool>()).prop_map(|(a, b)| Op::SetComment(a, b)),
        1 => any::<u16>().prop_map(Op::AppendComment),
        1 => (any::<u16>(), any::<bool>()).prop_map(|(a, b)| Op::SetBefore(a, b)),
        2 => (any::<u16>(), 1..=255u16).prop_map(|(a, b)| Op::AddNag(a, b)),
        1 => (any::<u16>(), 1..=255u16).prop_map(|(a, b)| Op::RemoveNag(a, b)),
        1 => any::<u16>().prop_map(Op::ClearNags),
    ]
}

fn pick(g: &Game, sel: u16) -> NodeId {
    let nodes = all_nodes(g);
    nodes[usize::from(sel) % nodes.len()]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]

    #[test]
    fn random_edit_sequences_preserve_all_invariants(
        start in 0..common::START_COUNT,
        ops in proptest::collection::vec(op_strategy(), 1..60),
    ) {
        let mut g = Game::from_position(common::start_position(start));
        for op in ops {
            let count_before = g.node_count();
            match op {
                Op::Add(n, m) => {
                    let id = pick(&g, n);
                    let moves = g.position_at(id).expect("p").legal_moves();
                    if !moves.is_empty() {
                        let mv = moves[usize::from(m) % moves.len()];
                        let child = g.add_move(id, mv).expect("legal");
                        prop_assert_eq!(g.parent(child), Ok(Some(id)));
                        prop_assert_eq!(g.mv(child), Ok(mv));
                        // idempotent
                        prop_assert_eq!(g.add_move(id, mv), Ok(child));
                    }
                }
                Op::Promote(n) => {
                    let id = pick(&g, n);
                    let was_var = g.is_variation(id).expect("v");
                    let is_root = id == g.root();
                    let kids_before: std::collections::BTreeSet<_> = match g.parent(id).expect("p") {
                        Some(p) => g.children(p).expect("c").iter().copied().collect(),
                        None => std::collections::BTreeSet::default(),
                    };
                    let r = g.promote_variation(id);
                    if is_root { prop_assert_eq!(r, Err(GameError::IsRoot)); }
                    else if !was_var { prop_assert_eq!(r, Err(GameError::NotVariation(id))); }
                    else {
                        prop_assert_eq!(r, Ok(()));
                        let p = g.parent(id).expect("p").expect("has parent");
                        prop_assert_eq!(g.children(p).expect("c")[0], id);
                        let after: std::collections::BTreeSet<_> = g.children(p).expect("c").iter().copied().collect();
                        prop_assert_eq!(after, kids_before);
                    }
                    prop_assert_eq!(g.node_count(), count_before);
                }
                Op::MakeFirst(n) => {
                    let id = pick(&g, n);
                    let r = g.make_first(id);
                    if r.is_ok() {
                        let p = g.parent(id).expect("p").expect("parent");
                        prop_assert_eq!(g.children(p).expect("c")[1], id);
                    }
                    prop_assert_eq!(g.node_count(), count_before);
                }
                Op::Delete(n) => {
                    let id = pick(&g, n);
                    let size = if g.contains(id) { g.descendants(id).expect("d").count() + 1 } else { 0 };
                    if g.delete_variation(id).is_ok() {
                        prop_assert_eq!(g.node_count(), count_before - size);
                        prop_assert!(!g.contains(id));
                    } else {
                        prop_assert_eq!(g.node_count(), count_before);
                    }
                }
                Op::Truncate(n) => {
                    let id = pick(&g, n);
                    let size = g.descendants(id).expect("d").count() + 1;
                    if g.truncate_from(id).is_ok() {
                        prop_assert_eq!(g.node_count(), count_before - size);
                        prop_assert!(!g.contains(id));
                    } else {
                        prop_assert_eq!(id, g.root());
                    }
                }
                Op::TruncateStartCheck(n) => {
                    let id = pick(&g, n);
                    let t = g.truncate_start(id).expect("start");
                    prop_assert_eq!(t.start_position(), g.position_at(id).expect("p"));
                    prop_assert_eq!(t.node_count(), g.descendants(id).expect("d").count() + 1);
                    check_invariants(&t);
                    prop_assert_eq!(g.node_count(), count_before);
                }
                Op::StripVariations => {
                    let main: Vec<_> = g.mainline().collect();
                    g.strip_variations();
                    prop_assert_eq!(g.node_count(), main.len() + 1);
                    prop_assert_eq!(g.mainline().collect::<Vec<_>>(), main);
                }
                Op::StripComments => {
                    g.strip_comments();
                    for id in all_nodes(&g) {
                        prop_assert_eq!(g.comment(id), Ok(None));
                        prop_assert_eq!(g.comment_before(id), Ok(None));
                    }
                }
                Op::StripNags => {
                    g.strip_nags();
                    for id in all_nodes(&g) {
                        prop_assert_eq!(g.nags(id).expect("n"), []);
                    }
                }
                Op::SetComment(n, blank) => {
                    let id = pick(&g, n);
                    g.set_comment(id, if blank { "  " } else { "text" }).expect("c");
                    prop_assert_eq!(g.comment(id).expect("c"), if blank { None } else { Some("text") });
                }
                Op::AppendComment(n) => {
                    let id = pick(&g, n);
                    g.append_comment(id, "more").expect("c");
                    prop_assert!(g.comment(id).expect("c").expect("some").ends_with("more"));
                }
                Op::SetBefore(n, blank) => {
                    let id = pick(&g, n);
                    let r = g.set_comment_before(id, if blank { "" } else { "pre" });
                    if id == g.root() { prop_assert_eq!(r, Err(GameError::IsRoot)); }
                    else { prop_assert_eq!(r, Ok(())); }
                }
                Op::AddNag(n, v) => {
                    let id = pick(&g, n);
                    if id != g.root() {
                        g.add_nag(id, nag(v)).expect("n");
                        let ns = g.nags(id).expect("n").to_vec();
                        prop_assert_eq!(ns.iter().filter(|&&x| x == nag(v)).count(), 1);
                    }
                }
                Op::RemoveNag(n, v) => {
                    let id = pick(&g, n);
                    if id != g.root() {
                        g.remove_nag(id, nag(v)).expect("n");
                        prop_assert!(!g.nags(id).expect("n").contains(&nag(v)));
                    }
                }
                Op::ClearNags(n) => {
                    let id = pick(&g, n);
                    if id != g.root() {
                        g.clear_nags(id).expect("n");
                        prop_assert_eq!(g.nags(id).expect("n"), []);
                    }
                }
            }
            check_invariants(&g);
        }
    }

    #[test]
    fn descendants_preorder_matches_a_recursive_reference(
        start in 0..common::START_COUNT,
        picks in proptest::collection::vec((any::<u16>(), any::<u16>()), 1..60),
    ) {
        fn walk(g: &Game, id: NodeId, out: &mut Vec<NodeId>) {
            for &c in g.children(id).expect("c") {
                out.push(c);
                walk(g, c, out);
            }
        }
        let mut g = Game::from_position(common::start_position(start));
        for (n, m) in picks {
            let id = pick(&g, n);
            let moves = g.position_at(id).expect("p").legal_moves();
            if !moves.is_empty() {
                g.add_move(id, moves[usize::from(m) % moves.len()]).expect("legal");
            }
        }
        let mut expected = Vec::new();
        walk(&g, g.root(), &mut expected);
        let got: Vec<_> = g.descendants(g.root()).expect("d").collect();
        prop_assert_eq!(got, expected);
    }
}
