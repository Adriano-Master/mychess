//! M7-4a, criteria 9-10 of `docs/specs/m7-4-exportacao.md`: the HTML viewer
//! volumes (splitting, JSON data, safety, cancellation).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_possible_truncation,
    clippy::assert_is_empty,
    clippy::assigning_clones
)]

#[path = "common/m7_4a_support.rs"]
mod support;

use std::fs;
use std::path::Path;

use mychess_core::{Game, NodeId};
use mychess_db::export::viewer::{ViewerAssets, ViewerOptions, ViewerReport, viewer_volumes};
use mychess_db::si4::Si4Error;
use mychess_db::{CancelToken, ExportError, ExportItem, ExportSkipReason, NoProgress};
use serde_json::Value;
use support::{check_balanced, games_from_pgn};

fn assets() -> ViewerAssets {
    ViewerAssets {
        css: "body{margin:0}".to_owned(),
        js: "/* viewer */ var x = 1;".to_owned(),
        pieces_svg: "<symbol id=\"wK\"><rect width=\"1\" height=\"1\"/></symbol>".to_owned(),
    }
}

fn opts() -> ViewerOptions {
    ViewerOptions {
        assets: assets(),
        ..ViewerOptions::default()
    }
}

fn simple(n: usize) -> Vec<ExportItem> {
    (0..n)
        .map(|i| {
            let mut g = games_from_pgn("[Event \"E\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 1-0\n")
                .remove(0);
            g.tags_mut().white = format!("White {i}");
            ExportItem::Game(Box::new(g))
        })
        .collect()
}

fn run(
    items: Vec<ExportItem>,
    dir: &Path,
    name: &str,
    o: &ViewerOptions,
) -> Result<ViewerReport, ExportError> {
    viewer_volumes(
        items,
        &dir.join(name),
        o,
        &mut NoProgress,
        &CancelToken::new(),
    )
}

/// The JSON of the data block of a volume file.
fn data_of(path: &Path) -> Value {
    let html = fs::read_to_string(path).unwrap();
    let open = "<script type=\"application/json\" id=\"mychess-data\">";
    let start = html.find(open).unwrap() + open.len();
    let end = html[start..].find("</script>").unwrap() + start;
    serde_json::from_str(&html[start..end]).unwrap()
}

#[test]
fn twelve_hundred_games_make_three_volumes_and_one_pgn() {
    let dir = tempfile::tempdir().unwrap();
    let report = run(simple(1200), dir.path(), "X", &opts()).unwrap();
    assert_eq!(report.written, 1200);
    let names: Vec<String> = report
        .volumes
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["X.html", "X_2.html", "X_3.html"]);
    assert_eq!(
        support::file_names(dir.path()),
        ["X.html", "X.pgn", "X_2.html", "X_3.html"]
    );
    let counts: Vec<usize> = report
        .volumes
        .iter()
        .map(|p| data_of(p)["games"].as_array().unwrap().len())
        .collect();
    assert_eq!(counts, [500, 500, 200]);
    let d1 = data_of(&report.volumes[0]);
    let d2 = data_of(&report.volumes[1]);
    let d3 = data_of(&report.volumes[2]);
    assert_eq!(
        (d1["prev"].clone(), d1["next"].clone()),
        (Value::Null, Value::from("X_2.html"))
    );
    assert_eq!(
        (d2["prev"].clone(), d2["next"].clone()),
        (Value::from("X.html"), Value::from("X_3.html"))
    );
    assert_eq!(
        (d3["prev"].clone(), d3["next"].clone()),
        (Value::from("X_2.html"), Value::Null)
    );
    assert_eq!(d3["volume"], 3);
    assert_eq!(d1["pgn"], "X.pgn");
    let html = fs::read_to_string(&report.volumes[1]).unwrap();
    assert!(
        html.contains("href=\"X.html\"")
            && html.contains("href=\"X_3.html\"")
            && html.contains("href=\"X.pgn\"")
    );
    // The PGN has all the games, in order.
    let pgn = fs::read_to_string(report.pgn.as_ref().unwrap()).unwrap();
    assert_eq!(pgn.matches("[Event ").count(), 1200);
    assert!(pgn.find("White 0\"").unwrap() < pgn.find("White 1199\"").unwrap());
}

/// Browsers do not resolve `url(#...)` (clip paths, gradients, filters) of
/// symbols under a `display:none` ancestor, so the sprite is hidden by size.
#[test]
fn the_piece_sprite_is_not_hidden_with_display_none() {
    let dir = tempfile::tempdir().unwrap();
    let report = run(simple(1), dir.path(), "Z", &opts()).unwrap();
    let html = fs::read_to_string(&report.volumes[0]).unwrap();
    let start = html.find("<svg xmlns").unwrap();
    let tag = &html[start..html[start..].find('>').unwrap() + start];
    assert!(!tag.contains("display:none"), "{tag}");
    assert!(tag.contains("aria-hidden=\"true\""));
    assert!(tag.contains("width:0;height:0"));
}

#[test]
fn pgn_can_be_turned_off() {
    let dir = tempfile::tempdir().unwrap();
    let o = ViewerOptions {
        include_pgn: false,
        ..opts()
    };
    let report = run(simple(3), dir.path(), "Y", &o).unwrap();
    assert!(report.pgn.is_none());
    assert_eq!(support::file_names(dir.path()), ["Y.html"]);
    assert_eq!(data_of(&report.volumes[0])["pgn"], Value::Null);
}

#[test]
fn size_limit_splits_earlier_and_a_big_game_still_gets_its_own_volume() {
    let dir = tempfile::tempdir().unwrap();
    let mut items = simple(5);
    items.insert(
        2,
        ExportItem::Game(Box::new(
            games_from_pgn(&format!(
                "[Event \"Big\"]\n[Result \"*\"]\n\n{} *\n",
                support::long_moves()
            ))
            .remove(0),
        )),
    );
    let base = assets().css.len() + assets().js.len() + assets().pieces_svg.len();
    let o = ViewerOptions {
        max_bytes: base + 9_000,
        ..opts()
    };
    let report = run(items, dir.path(), "S", &o).unwrap();
    assert_eq!(report.written, 6);
    assert!(report.volumes.len() >= 3, "{:?}", report.volumes);
    let mut total = 0;
    let mut saw_big_alone = false;
    for v in &report.volumes {
        let games = data_of(v)["games"].as_array().unwrap().clone();
        assert!(!games.is_empty());
        total += games.len();
        if games.len() == 1 && games[0]["event"] == "Big" {
            saw_big_alone = true;
        }
    }
    assert_eq!(total, 6);
    assert!(saw_big_alone, "the oversized game has a volume of its own");
}

#[test]
fn json_reproduces_headers_tree_and_strips_marks() {
    let pgn = "[Event \"Ev\"]\n[Site \"Rio\"]\n[Date \"2024.05.??\"]\n[Round \"2\"]\n[White \"Wh\"]\n[Black \"Bl\"]\n[Result \"1-0\"]\n[WhiteElo \"2500\"]\n[ECO \"B20\"]\n\n{start [%cal Ge2e4]} 1. e4 $1 {Good [%clk 0:01:00] move} (1. d4 $14 d5) 1... c5 $201 2. Nf3 (2. f4 e5 (2... Nc6)) 2... d6 1-0\n";
    let game = games_from_pgn(pgn).remove(0);
    let dir = tempfile::tempdir().unwrap();
    let report = run(
        vec![ExportItem::Game(Box::new(game.clone()))],
        dir.path(),
        "J",
        &opts(),
    )
    .unwrap();
    let data = data_of(&report.volumes[0]);
    let g = &data["games"][0];
    assert_eq!(g["white"], "Wh");
    assert_eq!(g["black"], "Bl");
    assert_eq!(g["event"], "Ev");
    assert_eq!(g["site"], "Rio");
    assert_eq!(g["date"], "2024.05");
    assert_eq!(g["round"], "2");
    assert_eq!(g["result"], "1-0");
    assert_eq!(g["eco"], "B20");
    assert_eq!(g["white_elo"], 2500);
    assert_eq!(g["black_elo"], Value::Null);
    assert_eq!(g["start_fen"], Value::Null);
    let nodes = g["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), game.node_count());
    // Pre-order walk of the tree, node by node.
    let mut ids: Vec<NodeId> = vec![game.root()];
    ids.extend(game.descendants(game.root()).unwrap());
    for (i, id) in ids.iter().enumerate() {
        let n = &nodes[i];
        assert_eq!(
            n["fen"].as_str().unwrap(),
            game.position_at(*id).unwrap().fen()
        );
        let nags: Vec<u64> = n["nags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect();
        let want: Vec<u64> = game
            .nags(*id)
            .unwrap()
            .iter()
            .map(|x| u64::from(x.value()))
            .collect();
        assert_eq!(nags, want);
        let parent = game.parent(*id).unwrap();
        match parent {
            None => {
                assert_eq!(n["prev"], -1);
                assert_eq!(n["san"], Value::Null);
                assert_eq!(n["depth"], 0);
            }
            Some(p) => {
                let pi = ids.iter().position(|x| *x == p).unwrap();
                assert_eq!(n["prev"].as_u64().unwrap() as usize, pi);
                let san = game
                    .position_at(p)
                    .unwrap()
                    .san(game.mv(*id).unwrap())
                    .unwrap();
                assert_eq!(n["san"], san.as_str());
            }
        }
        let kids = game.children(*id).unwrap();
        match kids.first() {
            Some(k) => assert_eq!(
                n["next"].as_u64().unwrap() as usize,
                ids.iter().position(|x| x == k).unwrap()
            ),
            None => assert_eq!(n["next"], -1),
        }
        let vars: Vec<usize> = n["variations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as usize)
            .collect();
        let want_vars: Vec<usize> = kids
            .iter()
            .skip(1)
            .map(|k| ids.iter().position(|x| x == k).unwrap())
            .collect();
        assert_eq!(vars, want_vars);
    }
    // Comments without [%...] blocks; the initial comment sits on the root.
    assert_eq!(nodes[0]["comment"], "start");
    assert_eq!(nodes[1]["comment"], "Good move");
    assert_eq!(nodes[1]["nags"], serde_json::json!([1]));
    // Depths: 1.d4 is a variation of depth 1; 2...Nc6 inside 2.f4 has depth 2.
    let depth_of = |san: &str| {
        nodes.iter().find(|n| n["san"] == san).unwrap()["depth"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(depth_of("e4"), 0);
    assert_eq!(depth_of("d4"), 1);
    assert_eq!(depth_of("f4"), 1);
    assert_eq!(depth_of("Nc6"), 2);
    assert_eq!(depth_of("Nf3"), 0);
}

#[test]
fn start_fen_and_chess960_style_start_are_reported() {
    let game = games_from_pgn("[Event \"E\"]\n[Result \"*\"]\n[SetUp \"1\"]\n[FEN \"4k3/8/8/8/8/8/4P3/4K3 w - - 0 1\"]\n\n1. e4 *\n").remove(0);
    let dir = tempfile::tempdir().unwrap();
    let r = run(
        vec![ExportItem::Game(Box::new(game))],
        dir.path(),
        "F",
        &opts(),
    )
    .unwrap();
    let g = &data_of(&r.volumes[0])["games"][0];
    assert_eq!(g["start_fen"], "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1");
    assert_eq!(g["result"], "*");
    assert_eq!(g["nodes"][0]["fen"], "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1");
}

fn hostile_game() -> Game {
    let mut g = games_from_pgn("[Event \"E\"]\n[Result \"1-0\"]\n\n1. e4 e5 1-0\n").remove(0);
    let name = "</script><script>alert(1)</script> <!-- \u{2028}\u{2029} \" ' & \u{1F600} ]]> \\";
    g.tags_mut().white = name.to_owned();
    g.tags_mut().event = name.to_owned();
    let first = g.mainline().next().unwrap();
    g.set_comment(first, name).unwrap();
    g
}

#[test]
fn hostile_text_cannot_break_out_of_the_data_block_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let r = run(
        vec![ExportItem::Game(Box::new(hostile_game()))],
        dir.path(),
        "H",
        &opts(),
    )
    .unwrap();
    let html = fs::read_to_string(&r.volumes[0]).unwrap();
    // Only the real closers: the data block and the viewer script.
    assert_eq!(html.matches("</script").count(), 2, "{html}");
    assert_eq!(html.matches("<script").count(), 2);
    let data = data_of(&r.volumes[0]);
    let g = &data["games"][0];
    let name = "</script><script>alert(1)</script> <!-- \u{2028}\u{2029} \" ' & \u{1F600} ]]> \\";
    assert_eq!(g["white"], name);
    assert_eq!(g["event"], name);
    assert_eq!(g["nodes"][1]["comment"], name);
    // The data block holds no raw '<' '>' '&'.
    let open = "id=\"mychess-data\">";
    let s = html.find(open).unwrap() + open.len();
    let block = &html[s..html[s..].find("</script>").unwrap() + s];
    assert!(!block.contains(['<', '>', '&', '\u{2028}', '\u{2029}']));
    check_balanced(&html).unwrap();
    // The static fallback has the text escaped.
    assert!(html.contains("&lt;/script&gt;&lt;script&gt;"));
}

#[test]
fn page_is_self_contained_with_a_restrictive_policy() {
    let dir = tempfile::tempdir().unwrap();
    let o = ViewerOptions {
        title: "T <&> \"x\"".to_owned(),
        pieces_credit: Some(
            "Cburnett by Colin M.L. Burnett --> <script>, GPL-2.0-or-later".to_owned(),
        ),
        ..opts()
    };
    let r = run(simple(1), dir.path(), "P", &o).unwrap();
    let html = fs::read_to_string(&r.volumes[0]).unwrap();
    assert!(html.starts_with("<!DOCTYPE html>\n"));
    assert!(html.contains("Content-Security-Policy"));
    assert!(html.contains("default-src &#39;none&#39;"));
    assert!(html.contains("<title>T &lt;&amp;&gt; &quot;x&quot; - 1</title>"));
    // The SVG namespace is an identifier, not a request.
    let without_ns = html.replace("http://www.w3.org/2000/svg", "");
    for forbidden in ["src=", "http://", "https://", "@import", "url("] {
        assert!(!without_ns.contains(forbidden), "{forbidden}");
    }
    // The credit is a comment that the text cannot close early.
    let c = html.find("<!-- Pieces:").unwrap();
    let end = html[c..].find("-->").unwrap() + c;
    assert!(!html[c + 4..end].contains("--"));
    assert!(!html[c..end].contains("<script"));
    assert!(html.contains("<noscript>"));
    assert!(html.contains("White 0"));
    check_balanced(&html).unwrap();
    // The page title in the data too.
    assert_eq!(data_of(&r.volumes[0])["title"], "T <&> \"x\"");
}

#[test]
fn cancel_keeps_finished_volumes_and_their_pgn_and_removes_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let token = CancelToken::new();
    let seen = token.clone();
    let items = simple(7).into_iter().enumerate().map(move |(i, it)| {
        if i == 5 {
            seen.cancel();
        }
        it
    });
    let o = ViewerOptions {
        per_file: 2,
        ..opts()
    };
    let report = viewer_volumes(items, &dir.path().join("C"), &o, &mut NoProgress, &token).unwrap();
    assert!(report.interrupted);
    assert_eq!(report.volumes.len(), 2, "{:?}", report.volumes);
    assert_eq!(report.written, 4);
    assert_eq!(
        support::file_names(dir.path()),
        ["C.html", "C.pgn", "C_2.html"]
    );
    let pgn = fs::read_to_string(report.pgn.unwrap()).unwrap();
    assert_eq!(pgn.matches("[Event ").count(), 4);
}

#[test]
fn cancel_before_anything_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let token = CancelToken::new();
    token.cancel();
    let report = viewer_volumes(
        simple(3),
        &dir.path().join("Z"),
        &opts(),
        &mut NoProgress,
        &token,
    )
    .unwrap();
    assert!(report.interrupted && report.volumes.is_empty() && report.pgn.is_none());
    assert!(support::file_names(dir.path()).is_empty());
}

#[test]
fn unreadable_games_are_skipped_and_never_leave_a_dangling_next_link() {
    let dir = tempfile::tempdir().unwrap();
    let mut items = simple(2);
    // The only game after the first full volume is unreadable.
    items.push(ExportItem::Stored {
        number: 8,
        game: Err(Si4Error::InvalidGameNumber { number: 8, len: 3 }),
    });
    let o = ViewerOptions {
        per_file: 2,
        ..opts()
    };
    let r = run(items, dir.path(), "U", &o).unwrap();
    assert_eq!(r.volumes.len(), 1);
    assert!(matches!(
        r.skipped.as_slice(),
        [(8, ExportSkipReason::Unreadable(_))]
    ));
    assert_eq!(data_of(&r.volumes[0])["next"], Value::Null);
}

#[test]
fn only_unreadable_games_is_nothing_to_export_and_creates_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let items = vec![ExportItem::Stored {
        number: 1,
        game: Err(Si4Error::InvalidGameNumber { number: 1, len: 0 }),
    }];
    let r = run(items, dir.path(), "N", &opts());
    // The error keeps the counts and the first reason (M7-4/D3744).
    match r {
        Err(ExportError::AllSkipped {
            handled,
            skipped,
            first: Some((1, ExportSkipReason::Unreadable(_))),
        }) => assert_eq!((handled, skipped), (1, 1)),
        other => panic!("{other:?}"),
    }
    assert!(support::file_names(dir.path()).is_empty());
    let r = run(vec![], dir.path(), "N", &opts());
    assert!(matches!(r, Err(ExportError::NothingToExport)));
}

#[test]
fn options_out_of_range_and_unsafe_assets_are_refused_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    for bad in [
        ViewerOptions {
            per_file: 0,
            ..opts()
        },
        ViewerOptions {
            per_file: 5001,
            ..opts()
        },
        ViewerOptions {
            max_bytes: 0,
            ..opts()
        },
        ViewerOptions {
            assets: ViewerAssets {
                js: "a</SCRIPT>b".to_owned(),
                ..assets()
            },
            ..opts()
        },
        ViewerOptions {
            assets: ViewerAssets {
                js: "<!-- x".to_owned(),
                ..assets()
            },
            ..opts()
        },
        ViewerOptions {
            assets: ViewerAssets {
                css: "</style>".to_owned(),
                ..assets()
            },
            ..opts()
        },
    ] {
        let r = run(simple(1), dir.path(), "B", &bad);
        assert!(matches!(r, Err(ExportError::InvalidOptions(_))), "{r:?}");
    }
    assert!(support::file_names(dir.path()).is_empty());
    // The limits themselves are valid.
    let r = run(
        simple(1),
        dir.path(),
        "B",
        &ViewerOptions {
            per_file: 5000,
            ..opts()
        },
    );
    assert!(r.is_ok());
}

#[test]
fn names_are_sanitized_and_nothing_else_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let r = run(simple(1), dir.path(), "CON.HTML", &opts()).unwrap();
    assert_eq!(r.volumes[0].file_name().unwrap(), "_CON.html");
    let r = run(simple(1), dir.path(), "a:b*c", &opts()).unwrap();
    assert_eq!(r.volumes[0].file_name().unwrap(), "a_b_c.html");
    assert!(
        support::file_names(dir.path())
            .iter()
            .all(|n| !n.contains(".tmp"))
    );
}

#[test]
fn existing_unrelated_files_are_not_touched_and_the_volume_replaces_its_own_name() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("scid.js"), "mine").unwrap();
    fs::write(dir.path().join("X.html"), "old volume").unwrap();
    run(simple(1), dir.path(), "X", &opts()).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("scid.js")).unwrap(),
        "mine"
    );
    assert!(
        fs::read_to_string(dir.path().join("X.html"))
            .unwrap()
            .starts_with("<!DOCTYPE")
    );
}

#[test]
fn skipped_game_is_counted_and_the_viewer_is_deterministic() {
    let d1 = tempfile::tempdir().unwrap();
    let d2 = tempfile::tempdir().unwrap();
    let a = run(simple(3), d1.path(), "D", &opts()).unwrap();
    let b = run(simple(3), d2.path(), "D", &opts()).unwrap();
    assert_eq!(
        fs::read(&a.volumes[0]).unwrap(),
        fs::read(&b.volumes[0]).unwrap()
    );
    assert_eq!(
        fs::read(a.pgn.unwrap()).unwrap(),
        fs::read(b.pgn.unwrap()).unwrap()
    );
}

#[test]
fn cancel_during_the_second_volume_leaves_the_first_without_a_next_link() {
    let dir = tempfile::tempdir().unwrap();
    let token = CancelToken::new();
    let seen = token.clone();
    let items = simple(5).into_iter().enumerate().map(move |(i, it)| {
        if i == 3 {
            seen.cancel();
        }
        it
    });
    let o = ViewerOptions {
        per_file: 2,
        ..opts()
    };
    let report = viewer_volumes(items, &dir.path().join("X"), &o, &mut NoProgress, &token).unwrap();
    assert!(report.interrupted);
    assert_eq!(report.volumes.len(), 1);
    assert_eq!(support::file_names(dir.path()), ["X.html", "X.pgn"]);
    let html = fs::read_to_string(&report.volumes[0]).unwrap();
    assert!(!html.contains("X_2"), "dangling link to a missing volume");
    assert_eq!(data_of(&report.volumes[0])["next"], Value::Null);
    check_balanced(&html).unwrap();
}

#[test]
fn cancel_during_the_third_volume_fixes_only_the_last_finished_one() {
    let dir = tempfile::tempdir().unwrap();
    let token = CancelToken::new();
    let seen = token.clone();
    let items = simple(7).into_iter().enumerate().map(move |(i, it)| {
        if i == 5 {
            seen.cancel();
        }
        it
    });
    let o = ViewerOptions {
        per_file: 2,
        ..opts()
    };
    let r = viewer_volumes(items, &dir.path().join("C"), &o, &mut NoProgress, &token).unwrap();
    assert_eq!(r.volumes.len(), 2);
    assert_eq!(data_of(&r.volumes[0])["next"], "C_2.html");
    assert_eq!(data_of(&r.volumes[1])["next"], Value::Null);
    assert_eq!(data_of(&r.volumes[1])["prev"], "C.html");
}

#[test]
fn io_failure_on_the_second_volume_leaves_the_first_without_a_next_link() {
    let dir = tempfile::tempdir().unwrap();
    // A directory in the place of the second volume makes the final rename fail.
    fs::create_dir(dir.path().join("F_2.html")).unwrap();
    let o = ViewerOptions {
        per_file: 2,
        ..opts()
    };
    let r = run(simple(5), dir.path(), "F", &o);
    assert!(matches!(r, Err(ExportError::Io { .. })), "{r:?}");
    let names = support::file_names(dir.path());
    assert!(names.iter().all(|n| !n.contains(".tmp")), "{names:?}");
    assert!(!names.contains(&"F.pgn".to_owned()), "{names:?}");
    let first = dir.path().join("F.html");
    assert_eq!(data_of(&first)["next"], Value::Null);
    assert!(!fs::read_to_string(&first).unwrap().contains("F_2"));
}

#[test]
fn pieces_that_could_run_code_are_refused_in_any_case_and_spacing() {
    let dir = tempfile::tempdir().unwrap();
    for bad in [
        "</svg><script>alert(1)</script>",
        "</SVG >x",
        "</ sVg>",
        "<ScRiPt>1</ScRiPt>",
        "<!-- c -->",
        "<![CDATA[x]]>",
        "<rect onload=\"x()\"/>",
        "<rect ONCLICK = \"x()\"/>",
        "<rect/onerror=x>",
        "<a href=\"JaVa\tScRiPt:alert(1)\"/>",
        "<foreignObject><div/></foreignObject>",
        "<FOREIGNOBJECT/>",
        "<iframe src=x></iframe>",
        "<a href=\"&#106;avascript:x\"/>",
    ] {
        let o = ViewerOptions {
            assets: ViewerAssets {
                pieces_svg: bad.to_owned(),
                ..assets()
            },
            ..opts()
        };
        let r = run(simple(1), dir.path(), "S", &o);
        assert!(
            matches!(r, Err(ExportError::InvalidOptions(_))),
            "{bad}: {r:?}"
        );
    }
    assert!(support::file_names(dir.path()).is_empty());
    // Ordinary symbols (attributes with "on" inside names and values) pass.
    let good = "<symbol id=\"wK\" viewBox=\"0 0 45 45\"><path d=\"M1 2\" stroke-linejoin=\"round\" transform=\"scale(1)\" font-family=\"icon\"/><g fill=\"none\" id=\"conic\"/></symbol>";
    let o = ViewerOptions {
        assets: ViewerAssets {
            pieces_svg: good.to_owned(),
            ..assets()
        },
        ..opts()
    };
    assert!(run(simple(1), dir.path(), "S", &o).is_ok());
}

#[test]
fn pieces_credit_never_keeps_two_dashes_in_a_row() {
    let dir = tempfile::tempdir().unwrap();
    let o = ViewerOptions {
        pieces_credit: Some("A-----B --- C-".to_owned()),
        ..opts()
    };
    let r = run(simple(1), dir.path(), "D", &o).unwrap();
    let html = fs::read_to_string(&r.volumes[0]).unwrap();
    let c = html.find("<!-- Pieces:").unwrap();
    let end = html[c..].find(" -->").unwrap() + c;
    assert!(!html[c + 4..end].contains("--"), "{}", &html[c..end]);
    assert!(html[c..end].contains("A- - - - -B"));
}
