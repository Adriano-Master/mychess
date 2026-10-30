//! Integration and property tests of `Game::merge_game` (spec M7-6a).

mod common;

use mychess_core::{Game, MergeError, MergeOptions, Position, SanMode, merge_comment_parts};
use proptest::prelude::*;

fn game(sans: &str) -> Game {
    let mut g = Game::new();
    let mut at = g.root();
    for san in sans.split_whitespace() {
        let mv = g
            .position_at(at)
            .expect("node")
            .parse_san(san, SanMode::Strict)
            .expect("san")
            .mv;
        at = g.add_move(at, mv).expect("add");
    }
    g
}

/// A game that follows the random playout of `choices` from `start`.
fn playout(start: &Position, choices: &[u16]) -> Game {
    let mut g = Game::from_position(start.clone());
    let mut at = g.root();
    for (_, mv) in common::random_game(start, choices) {
        at = g.add_move(at, mv).expect("legal");
    }
    g
}

fn mainline_uci(g: &Game) -> Vec<String> {
    let mut out = Vec::new();
    let mut pos = g.start_position().clone();
    for id in g.mainline() {
        let mv = g.mv(id).expect("move");
        out.push(pos.uci(mv));
        pos = g.position_at(id).expect("node").clone();
    }
    out
}

#[test]
fn spec_scenarios_end_to_end() {
    // A <- B, then B again: second merge is a no-op.
    let b = game("e4 e5 Nf3 Nc6 Bc4 Bc5 c3 Nf6");
    let mut a = game("e4 e5 Nf3 Nc6 Bb5 a6");
    let opts = MergeOptions {
        end_ply: None,
        comment: Some(merge_comment_parts(&b).english(false)),
    };
    let first = a.merge_game(&b, &opts).expect("merge");
    assert_eq!(first.added, 4);
    let snapshot = a.clone();
    let second = a.merge_game(&b, &opts).expect("again");
    assert_eq!((second.added, second.reused), (0, 4));
    assert!(a.same_content(&snapshot));
}

#[test]
fn empty_target_is_refused_and_unchanged() {
    let mut g = Game::new();
    let before = g.clone();
    assert_eq!(
        g.merge_game(&game("e4"), &MergeOptions::default()),
        Err(MergeError::NoMoves)
    );
    assert!(g.same_content(&before));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn merge_never_panics_and_keeps_invariants(
        start in 0usize..common::START_COUNT,
        shared in prop::collection::vec(any::<u16>(), 0..12),
        tail_a in prop::collection::vec(any::<u16>(), 0..30),
        tail_b in prop::collection::vec(any::<u16>(), 0..30),
        end in prop::option::of(0u32..60),
    ) {
        let start = common::start_position(start);
        let cat = |t: &[u16]| shared.iter().chain(t).copied().collect::<Vec<_>>();
        let mut a = playout(&start, &cat(&tail_a));
        let b = playout(&start, &cat(&tail_b));
        let opts = MergeOptions { end_ply: end, comment: Some("c".into()) };

        let before = a.clone();
        let main_before = mainline_uci(&a);
        match a.merge_game(&b, &opts) {
            Err(_) => prop_assert!(a.same_content(&before), "error must not change the game"),
            Ok(r) => {
                for id in a.descendants(a.root()).expect("root") {
                    let parent = a.parent(id).expect("node").expect("parent");
                    let mv = a.mv(id).expect("move");
                    let expected = a.position_at(parent).expect("node").after(mv).expect("legal");
                    prop_assert_eq!(a.position_at(id).expect("node"), &expected);
                }
                if !r.extended_mainline {
                    prop_assert_eq!(mainline_uci(&a), main_before);
                }
                prop_assert_eq!(a.node_count(), before.node_count() + usize::try_from(r.added).expect("fits"));
                // Merging again is a no-op, except when a cut-off merge extended
                // the main line: the longer main line then matches deeper in
                // `b`, so the next call continues where the first stopped.
                if !(r.extended_mainline && r.truncated) {
                    let once = a.clone();
                    match a.merge_game(&b, &opts) {
                        Ok(r2) => {
                            prop_assert_eq!(r2.added, 0);
                            prop_assert!(a.same_content(&once));
                        }
                        Err(e) => {
                            prop_assert!(matches!(e, MergeError::NothingToAdd), "unexpected {:?}", e);
                            prop_assert!(a.same_content(&once));
                        }
                    }
                }
            }
        }
    }
}
