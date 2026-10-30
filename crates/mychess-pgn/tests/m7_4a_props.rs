//! M7-4a, criterion 11: the document writers never panic, are deterministic
//! and keep their output well formed for random games. Games are small and
//! the case count is low on purpose (memory).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m7_4a_support;

use m7_4a_support::{FakeDiagrams, check_balanced};
use mychess_core::{Game, Nag, NodeId};
use mychess_pgn::export::{HtmlOptions, LatexOptions, game_to_html_string, game_to_latex_string};
use proptest::prelude::*;

#[derive(Debug, Clone)]
struct Step {
    choice: u8,
    branch: bool,
    comment: Option<String>,
    nags: Vec<u8>,
}

fn text() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            Just("<script>".to_owned()),
            Just("</script>".to_owned()),
            Just("]]>".to_owned()),
            Just("&".to_owned()),
            Just("\"".to_owned()),
            Just("'".to_owned()),
            Just("{".to_owned()),
            Just("}".to_owned()),
            Just("\\".to_owned()),
            Just("$#%_~^".to_owned()),
            Just("\0".to_owned()),
            Just("\n\n".to_owned()),
            Just("[%cal Ge2e4]".to_owned()),
            Just("[%csl Rd4,Gd5]".to_owned()),
            Just("\u{1F600}\u{2028}".to_owned()),
            "[a-z ]{0,6}",
        ],
        0..5,
    )
    .prop_map(|parts| parts.concat())
}

fn step() -> impl Strategy<Value = Step> {
    (
        any::<u8>(),
        any::<bool>(),
        proptest::option::of(text()),
        proptest::collection::vec(prop_oneof![1u8..=6, Just(201u8), 8u8..=215], 0..3),
    )
        .prop_map(|(choice, branch, comment, nags)| Step {
            choice,
            branch,
            comment,
            nags,
        })
}

fn build(steps: &[Step], names: &str) -> Game {
    let mut game = Game::new();
    names.clone_into(&mut game.tags_mut().white);
    names.clone_into(&mut game.tags_mut().event);
    let mut cur: NodeId = game.root();
    for s in steps {
        let from = if s.branch {
            game.parent(cur).unwrap().unwrap_or(cur)
        } else {
            cur
        };
        let moves = game.position_at(from).unwrap().legal_moves();
        if moves.is_empty() {
            break;
        }
        let mv = moves[usize::from(s.choice) % moves.len()];
        let Ok((node, _)) = game.add_move_new(from, mv) else {
            break;
        };
        if let Some(c) = &s.comment {
            game.set_comment(node, c).unwrap();
        }
        for n in &s.nags {
            game.add_nag(node, Nag::new(u16::from(*n)).unwrap())
                .unwrap();
        }
        cur = node;
    }
    game
}

fn html_opts(variant: u8) -> HtmlOptions {
    HtmlOptions {
        comments: variant & 1 == 0,
        variations: variant & 2 == 0,
        symbols: variant & 4 != 0,
        diagram_px: if variant & 8 == 0 { 120 } else { 0 },
        ..HtmlOptions::default()
    }
}

fn environments_pair(tex: &str) -> bool {
    [
        "document",
        "addmargin",
        "longfbox",
        "tabularx",
        "center",
        "@twocolumnfalse",
    ]
    .iter()
    .all(|e| {
        tex.matches(&format!("\\begin{{{e}}}")).count()
            == tex.matches(&format!("\\end{{{e}}}")).count()
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    fn html_is_balanced_deterministic_and_safe(
        steps in proptest::collection::vec(step(), 0..40),
        names in text(),
        variant in 0u8..16,
    ) {
        let game = build(&steps, &names);
        let opts = html_opts(variant);
        let a = game_to_html_string(&game, &opts, &mut FakeDiagrams::default()).unwrap();
        let b = game_to_html_string(&game, &opts, &mut FakeDiagrams::default()).unwrap();
        prop_assert_eq!(&a, &b);
        prop_assert!(!a.contains('\0'));
        prop_assert!(!a.to_lowercase().contains("</script"));
        prop_assert!(!a.contains("<script"));
        if let Err(e) = check_balanced(&a) {
            prop_assert!(false, "{}\n{}", e, a);
        }
    }

    #[test]
    fn latex_is_paired_deterministic_and_has_no_nul(
        steps in proptest::collection::vec(step(), 0..40),
        names in text(),
        flip in any::<bool>(),
        comments in any::<bool>(),
        variations in any::<bool>(),
    ) {
        let game = build(&steps, &names);
        let opts = LatexOptions { flip, comments, variations, ..LatexOptions::default() };
        let a = game_to_latex_string(&game, &opts).unwrap();
        let b = game_to_latex_string(&game, &opts).unwrap();
        prop_assert_eq!(&a, &b);
        prop_assert!(!a.contains('\0'));
        prop_assert!(environments_pair(&a), "{}", a);
        prop_assert!(!a.contains("[%"), "marks must be stripped");
    }
}

#[test]
fn deeply_nested_variations_do_not_overflow_the_stack() {
    // 3000 nested variations: alternating moves from the start position.
    let mut game = Game::new();
    let mut cur = game.root();
    let first = game.position_at(cur).unwrap().legal_moves();
    let a = game.add_move(cur, first[0]).unwrap();
    cur = a;
    for _ in 0..3000 {
        let parent = game.parent(cur).unwrap().unwrap();
        let moves = game.position_at(parent).unwrap().legal_moves();
        let Some(mv) = moves
            .iter()
            .copied()
            .find(|m| game.find_child(parent, *m).unwrap().is_none())
        else {
            break;
        };
        let (n, _) = game.add_move_new(parent, mv).unwrap();
        cur = n;
        // Descend by one move so the next alternative nests deeper.
        let next = game.position_at(cur).unwrap().legal_moves();
        cur = game.add_move(cur, next[0]).unwrap();
    }
    let html =
        game_to_html_string(&game, &HtmlOptions::default(), &mut FakeDiagrams::default()).unwrap();
    assert!(html.contains("<article"));
    assert!(
        html.matches("class=\"var").count() > 20,
        "the tree really is nested"
    );
    game_to_latex_string(&game, &LatexOptions::default()).unwrap();
}
