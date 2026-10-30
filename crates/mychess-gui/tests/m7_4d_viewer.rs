//! M7-4d (spec M7-4, criteria 10 and the rules of section C): the files
//! `assets/export/viewer.js` and `viewer.css`. There is no JavaScript engine
//! in the test machine, so the contract is checked from the outside: the
//! text cannot close its own element or reach the network, never builds
//! markup from text (the viewer shows game text with `textContent` only),
//! has balanced brackets, and names exactly the fields and the piece ids
//! that the volumes of `mychess-db` really hold.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::assigning_clones,
    clippy::format_collect
)]

use std::collections::BTreeSet;
use std::path::Path;

use mychess_core::{Game, Nag, NodeId};
use mychess_db::export::viewer::{ViewerAssets, ViewerOptions, viewer_volumes};
use mychess_db::{CancelToken, ExportItem, NoProgress};
use serde_json::Value;

fn asset(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/export")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn play(game: &mut Game, parent: NodeId, uci: &str) -> NodeId {
    let mv = game
        .position_at(parent)
        .unwrap()
        .parse_uci(uci)
        .expect("legal move");
    game.add_move(parent, mv).unwrap()
}

/// A game with a comment, a NAG, a variation and a name that tries to break
/// out of the page.
fn game() -> Game {
    let mut game = Game::new();
    let root = game.root();
    game.tags_mut().white = "</script><b>Alfa & \u{2028}Co".to_owned();
    game.tags_mut().black = "Beta".to_owned();
    let e4 = play(&mut game, root, "e2e4");
    game.add_nag(e4, Nag::new(1).unwrap()).unwrap();
    game.set_comment(e4, "best <i>by test</i> [%cal Ge2e4]")
        .unwrap();
    let e5 = play(&mut game, e4, "e7e5");
    play(&mut game, e4, "c7c5");
    let nf3 = play(&mut game, e5, "g1f3");
    play(&mut game, nf3, "b8c6");
    game
}

/// The text of `source` without string literals and comments.
fn code_only(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied().unwrap_or('\0');
        if c == '/' && next == '*' {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
        } else if c == '/' && next == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '\'' || c == '"' {
            i += 1;
            while i < chars.len() && chars[i] != c {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            out.push_str("\"\"");
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

#[test]
fn the_script_and_the_style_cannot_close_their_element_or_hide_markup() {
    let js = asset("viewer.js");
    let css = asset("viewer.css");
    for (name, text) in [("viewer.js", &js), ("viewer.css", &css)] {
        let lower = text.to_ascii_lowercase();
        for bad in ["</script", "</style", "<!--", "-->", "<![cdata["] {
            assert!(!lower.contains(bad), "{name} holds {bad}");
        }
    }
    assert!(
        !css.contains("@import") && !css.contains("url("),
        "the style reaches out"
    );
}

#[test]
fn the_script_never_builds_markup_from_text_or_reaches_the_network() {
    let js = asset("viewer.js");
    let code = code_only(&js);
    for bad in [
        "eval(",
        "Function(",
        "innerHTML",
        "outerHTML",
        "insertAdjacentHTML",
        "document.write",
        "setTimeout(\"",
        "fetch(",
        "XMLHttpRequest",
        "WebSocket",
        "sendBeacon",
        "import(",
        "importScripts",
        "localStorage",
        "document.cookie",
        "window.open",
        "location",
    ] {
        assert!(!code.contains(bad), "the script uses {bad}");
    }
    // The only address in the text is the SVG namespace.
    for (at, _) in js.match_indices("http") {
        let rest = &js[at..];
        assert!(
            rest.starts_with("http://www.w3.org/2000/svg"),
            "an address in the script: {}",
            &rest[..rest.len().min(40)]
        );
    }
}

#[test]
fn the_script_has_balanced_brackets() {
    let code = code_only(&asset("viewer.js"));
    let mut stack = Vec::new();
    for (n, c) in code.chars().enumerate() {
        match c {
            '(' | '[' | '{' => stack.push(c),
            ')' | ']' | '}' => {
                let open = stack.pop().unwrap_or_else(|| panic!("extra {c} at {n}"));
                assert_eq!(
                    (open, c),
                    match c {
                        ')' => ('(', ')'),
                        ']' => ('[', ']'),
                        _ => ('{', '}'),
                    },
                    "mismatch at {n}"
                );
            }
            _ => {}
        }
    }
    assert!(stack.is_empty(), "unclosed {stack:?}");
}

#[test]
fn every_volume_carries_the_script_and_it_reads_the_fields_the_data_has() {
    let dir = tempfile::tempdir().unwrap();
    let options = ViewerOptions {
        assets: ViewerAssets {
            css: asset("viewer.css"),
            js: asset("viewer.js"),
            pieces_svg: ["wK", "wQ", "wR", "wB", "wN", "wP", "bK", "bQ", "bR", "bB", "bN", "bP"]
                .iter()
                .map(|id| {
                    format!("<symbol id=\"{id}\" viewBox=\"0 0 45 45\"><rect width=\"45\" height=\"45\"/></symbol>")
                })
                .collect(),
        },
        ..ViewerOptions::default()
    };
    let dest = dir.path().join("games.html");
    let report = viewer_volumes(
        vec![ExportItem::Game(Box::new(game()))],
        &dest,
        &options,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .expect("the volume is written");
    assert_eq!(report.volumes.len(), 1);
    let html = std::fs::read_to_string(&dest).unwrap();
    // The data block is JSON that cannot break out of the page.
    let open = "<script type=\"application/json\" id=\"mychess-data\">";
    let start = html.find(open).expect("data block") + open.len();
    let end = start + html[start..].find("</script>").expect("end of the block");
    let block = &html[start..end];
    assert!(!block.contains('<') && !block.contains('\u{2028}'));
    let json: Value = serde_json::from_str(block).expect("valid JSON");
    assert!(json["games"].is_array());
    // The script and the style are in the file, once, and the page has the
    // element the script fills.
    assert_eq!(
        html.matches("function ()").count(),
        asset("viewer.js").matches("function ()").count()
    );
    assert!(html.contains("<main id=\"viewer\">"));
    // Every field the script reads exists in the data.
    let js = code_only(&asset("viewer.js"));
    let game0 = &json["games"][0];
    let node0 = &game0["nodes"][0];
    let top: BTreeSet<&str> = ["games", "nodes", "n"].into_iter().collect();
    for field in ["games", "nodes", "n"] {
        assert!(
            js.contains(&format!(".{field}")),
            "the script never reads .{field}"
        );
        assert!(top.contains(field));
    }
    for field in [
        "white",
        "black",
        "white_elo",
        "black_elo",
        "event",
        "round",
        "site",
        "date",
        "result",
        "eco",
    ] {
        assert!(
            js.contains(&format!(".{field}")),
            "the script never reads .{field}"
        );
        assert!(game0.get(field).is_some(), "the data has no {field}");
    }
    for field in [
        "fen",
        "san",
        "nags",
        "comment",
        "prev",
        "next",
        "variations",
    ] {
        assert!(
            js.contains(&format!(".{field}")),
            "the script never reads .{field}"
        );
        assert!(node0.get(field).is_some(), "the nodes have no {field}");
    }
    // The comments have their commands removed, the text stays text.
    let comment = json["games"][0]["nodes"][1]["comment"].as_str().unwrap();
    assert_eq!(comment, "best <i>by test</i>");
    // The pieces the script draws are symbols of the page.
    for id in ["wK", "bP"] {
        assert!(html.contains(&format!("<symbol id=\"{id}\"")), "{id}");
    }
    // The script finds the piece by the letter pair of the placement.
    assert!(
        code_only(&asset("viewer.js")).contains("'#' + now[i]")
            || asset("viewer.js").contains("href: '#' + now[i]")
    );
}

#[test]
fn the_script_and_the_style_have_a_license_line_and_the_registry_lists_them() {
    for name in ["viewer.js", "viewer.css"] {
        let text = asset(name);
        assert!(
            text.contains("GPL-3.0-or-later"),
            "{name} does not state its license"
        );
        assert!(
            !text.to_lowercase().contains("scid.js"),
            "{name} mentions the Scid files"
        );
    }
    let registry = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/LICENSES.md"),
    )
    .unwrap();
    assert!(registry.contains("export/viewer.js") && registry.contains("export/viewer.css"));
}

#[test]
fn the_keys_and_the_buttons_of_the_spec_are_there() {
    let js = asset("viewer.js");
    for key in ["ArrowLeft", "ArrowRight", "Home", "End", "'f'", "'F'"] {
        assert!(js.contains(key), "no key {key}");
    }
    // Flip, first, previous, next, last, and the game picker.
    for label in [
        "Flip the board",
        "First position",
        "Previous move",
        "Next move",
        "Last position",
    ] {
        assert!(js.contains(label), "no button {label}");
    }
    assert!(js.contains("'select'"));
    // In the move list ArrowUp/ArrowDown return before the switch of game:
    // the early return must come before the `switch (event.key)`.
    let early = js
        .find("inMoves && (event.key === 'ArrowUp'")
        .expect("vertical keys of the move list are handled first");
    let switch = js.find("switch (event.key)").expect("key switch");
    assert!(early < switch, "the move-list guard comes after the switch");
    // Printing keeps the moves and drops the controls.
    assert!(asset("viewer.css").contains("@media print"));
}
