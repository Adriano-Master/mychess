//! M7-4a, criterion 11 for the viewer: random games never panic, the JSON is
//! valid, the page is deterministic, balanced and has no raw `</script>`
//! inside the data block. Small games and few cases (memory).
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/m7_4a_support.rs"]
mod support;

use mychess_core::{Game, Nag};
use mychess_db::export::viewer::{ViewerAssets, ViewerOptions, viewer_volumes};
use mychess_db::{CancelToken, ExportItem, NoProgress};
use proptest::prelude::*;
use serde_json::Value;

fn text() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            Just("</script>".to_owned()),
            Just("<!--".to_owned()),
            Just("]]>".to_owned()),
            Just("&amp;".to_owned()),
            Just("\"'".to_owned()),
            Just("\\".to_owned()),
            Just("\0\u{1}".to_owned()),
            Just("\u{2028}\u{2029}".to_owned()),
            Just("[%cal Ge2e4]".to_owned()),
            Just("\u{1F600}".to_owned()),
            "[a-z ]{0,5}",
        ],
        0..5,
    )
    .prop_map(|v| v.concat())
}

type Step = (u8, bool, Option<String>, Vec<u8>);

fn step() -> impl Strategy<Value = Step> {
    (
        any::<u8>(),
        any::<bool>(),
        proptest::option::of(text()),
        proptest::collection::vec(1u8..=255, 0..3),
    )
}

fn build(steps: &[Step], name: &str) -> Game {
    let mut game = Game::new();
    name.clone_into(&mut game.tags_mut().white);
    name.clone_into(&mut game.tags_mut().site);
    let mut cur = game.root();
    for (choice, branch, comment, nags) in steps {
        let from = if *branch {
            game.parent(cur).unwrap().unwrap_or(cur)
        } else {
            cur
        };
        let moves = game.position_at(from).unwrap().legal_moves();
        if moves.is_empty() {
            break;
        }
        let mv = moves[usize::from(*choice) % moves.len()];
        let Ok((node, _)) = game.add_move_new(from, mv) else {
            break;
        };
        if let Some(c) = comment {
            game.set_comment(node, c).unwrap();
        }
        for n in nags {
            game.add_nag(node, Nag::new(u16::from(*n)).unwrap())
                .unwrap();
        }
        cur = node;
    }
    game
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, ..ProptestConfig::default() })]

    #[test]
    fn viewer_pages_are_valid_balanced_and_deterministic(
        games in proptest::collection::vec((proptest::collection::vec(step(), 0..25), text()), 1..4),
        per_file in 1usize..3,
    ) {
        let opts = ViewerOptions {
            per_file,
            assets: ViewerAssets { css: "a{}".to_owned(), js: "var a=1;".to_owned(), pieces_svg: String::new() },
            ..ViewerOptions::default()
        };
        let build_items = || -> Vec<ExportItem> {
            games.iter().map(|(steps, name)| ExportItem::Game(Box::new(build(steps, name)))).collect()
        };
        let d1 = tempfile::tempdir().unwrap();
        let d2 = tempfile::tempdir().unwrap();
        let a = viewer_volumes(build_items(), &d1.path().join("P"), &opts, &mut NoProgress, &CancelToken::new()).unwrap();
        let b = viewer_volumes(build_items(), &d2.path().join("P"), &opts, &mut NoProgress, &CancelToken::new()).unwrap();
        prop_assert_eq!(a.volumes.len(), b.volumes.len());
        let mut total = 0usize;
        for (pa, pb) in a.volumes.iter().zip(&b.volumes) {
            let ha = std::fs::read_to_string(pa).unwrap();
            prop_assert_eq!(&ha, &std::fs::read_to_string(pb).unwrap());
            prop_assert!(!ha.contains('\0'));
            prop_assert_eq!(ha.matches("</script").count(), 2);
            if let Err(e) = support::check_balanced(&ha) {
                prop_assert!(false, "{}", e);
            }
            let open = "id=\"mychess-data\">";
            let s = ha.find(open).unwrap() + open.len();
            let e = ha[s..].find("</script>").unwrap() + s;
            let json: Value = serde_json::from_str(&ha[s..e]).unwrap();
            let n = json["games"].as_array().unwrap().len();
            prop_assert!(n >= 1 && n <= per_file);
            total += n;
        }
        prop_assert_eq!(total, games.len());
    }
}
