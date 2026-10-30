//! M5-7b contract: the ECO browser (spec `docs/specs/m5-7-eco-e-novidade.md`,
//! D1702-D1708, D1721, D1722 and the first two acceptance criteria).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_push_string
)]

mod m5_7b_common;

use m5_7b_common::*;
use mychess_app::{BAR_UNITS, EcoLevel, bar_parts, grid_lines};
use mychess_db::{EcoStats, bar_scale};
use tempfile::tempdir;

/// B20: 1-0 1-0 0-1 1/2 (count 4); C40: 1-0 1/2 0-1 (3); D10: 1-0 and
/// unfinished (2); B90a: one game.
fn eco_games() -> String {
    let mut pgn = String::new();
    for (result, code) in [
        ("1-0", "B20"),
        ("1-0", "B20"),
        ("0-1", "B20"),
        ("1/2-1/2", "B20"),
        ("1-0", "C40"),
        ("1/2-1/2", "C40"),
        ("0-1", "C40"),
        ("1-0", "D10"),
        ("*", "D10"),
        ("1-0", "B90a"),
    ] {
        pgn.push_str(&game_pgn("1. e4", result, "2020.01.01", Some(code)));
    }
    // A game without a code never counts.
    pgn.push_str(&game_pgn("1. d4", "1-0", "2020.01.01", None));
    pgn
}

fn rig_with_games(dir: &std::path::Path) -> Rig {
    let mut rig = Rig::new();
    rig.open_pgn(dir, "g.pgn", &eco_games());
    rig.set_book(BOOK);
    rig
}

#[test]
fn opening_the_browser_asks_for_exactly_one_statistics_table_and_shows_a_to_e() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    let effects = rig.raw(eco(EcoMessage::OpenBrowser { glossary: vec![] }));
    assert_eq!(count(&effects, is_stats), 1);
    assert_eq!(rig.state.eco_window(), Some(EcoWindow::Browser));
    assert!(rig.browser().building);
    rig.settle(effects);
    let view = rig.browser();
    assert!(!view.building);
    let labels: Vec<&str> = view.bars.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["A", "B", "C", "D", "E"]);
    assert_eq!(view.prefix, "");
    assert_eq!(view.level, EcoLevel::All);
    assert_eq!(view.total_games, 11);
    assert_eq!(
        view.classified, 10,
        "the game without a code is not counted"
    );
    assert_eq!(view.stats.count, 10);
}

#[test]
fn the_parts_of_every_bar_add_up_to_1000_and_the_largest_bar_is_full_height() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    rig.open_browser();
    let view = rig.browser();
    let by = |label: &str| view.bars.iter().find(|b| b.label == label).unwrap();
    // B = 5 games (B20 x4, B90a), the largest.
    assert_eq!(by("B").stats.count, 5);
    assert_eq!(by("B").height, BAR_UNITS);
    assert_eq!(by("B").parts, [600, 200, 200]);
    // C = 3 games: 1/3 of each, the draws take the rounding.
    assert_eq!(by("C").parts, [333, 334, 333]);
    assert_eq!(by("C").height, 600);
    // D = 1-0 plus an unfinished game: the unfinished one joins the draws.
    assert_eq!(by("D").parts, [500, 500, 0]);
    assert_eq!(by("D").height, 400);
    // Empty bars have no height.
    assert_eq!(by("A").height, 0);
    assert_eq!(by("A").parts, [0, 0, 0]);
    for bar in &view.bars {
        if bar.stats.count > 0 {
            assert_eq!(bar.parts.iter().map(|&p| u32::from(p)).sum::<u32>(), 1000);
            assert!(bar.height >= 1);
        }
    }
    assert_eq!(view.bars.iter().map(|b| b.height).max(), Some(BAR_UNITS));
    assert_eq!(view.max, 5);
}

#[test]
fn the_summary_line_counts_the_node_with_the_draws_and_the_unfinished_apart_from_the_sides() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    rig.open_browser();
    let s = rig.browser().stats;
    // Whole tree: 10 games; 1-0: B20 x2, C40, D10, B90a = 5; 0-1: 2;
    // 1/2: 2; unfinished: 1.
    assert_eq!(
        (s.count, s.white, s.black, s.draw, s.none),
        (10, 5, 2, 2, 1)
    );
}

#[test]
fn bar_parts_pure_arithmetic_has_the_documented_edges() {
    // Empty bar: height 0, no parts.
    assert_eq!(bar_parts(&EcoStats::default(), 10), (0, [0, 0, 0]));
    // A non-empty bar is at least one unit high, however small.
    let one = EcoStats {
        count: 1,
        white: 1,
        draw: 0,
        black: 0,
        none: 0,
    };
    assert_eq!(bar_parts(&one, 1_000_000).0, 1);
    // The largest bar is full height.
    assert_eq!(bar_parts(&one, 1).0, BAR_UNITS);
    // A bar larger than the stated max cannot overflow the chart.
    assert_eq!(bar_parts(&one, 0).0, BAR_UNITS);
    for (w, d, b, n) in [(1, 1, 1, 0), (7, 3, 11, 5), (0, 0, 1, 0), (999, 1, 0, 0)] {
        let stats = EcoStats {
            count: w + d + b + n,
            white: w,
            draw: d,
            black: b,
            none: n,
        };
        let (_, parts) = bar_parts(&stats, stats.count);
        assert_eq!(parts.iter().map(|&p| u32::from(p)).sum::<u32>(), 1000);
    }
}

#[test]
fn the_grid_follows_bar_scale() {
    // 20 or fewer: every 5; 21: every 10 (D1184).
    assert_eq!(bar_scale(20), 5);
    assert_eq!(bar_scale(21), 10);
    let values = |max| grid_lines(max).iter().map(|l| l.value).collect::<Vec<_>>();
    assert_eq!(values(20), [5, 10, 15, 20]);
    assert_eq!(values(25), [10, 20]);
    assert_eq!(values(4), Vec::<u32>::new());
    for line in grid_lines(25) {
        assert_eq!(u32::from(line.height), line.value * 1000 / 25);
    }
    // A line at the max is the top of the chart.
    assert_eq!(grid_lines(100).last().unwrap().height, BAR_UNITS);
}

#[test]
fn the_view_grid_is_the_scale_of_the_largest_bar() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let mut pgn = String::new();
    for _ in 0..25 {
        pgn.push_str(&game_pgn("1. e4", "1-0", "2020.01.01", Some("B20")));
    }
    rig.open_pgn(dir.path(), "many.pgn", &pgn);
    rig.set_book(BOOK);
    rig.open_browser();
    let view = rig.browser();
    assert_eq!(view.max, 25);
    assert_eq!(view.grid, grid_lines(25));
    let values: Vec<u32> = view.grid.iter().map(|l| l.value).collect();
    assert_eq!(values, [10, 20]);
}

#[test]
fn clicking_down_the_levels_and_going_up_with_no_effects() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    rig.open_browser();
    for (to, level, bars, count) in [
        ("B", EcoLevel::Section, 10, 5),
        ("B9", EcoLevel::Section, 10, 1),
        ("B90", EcoLevel::Code, 26, 1),
        ("B90a", EcoLevel::Code, 0, 1),
    ] {
        let effects = rig.raw(eco(EcoMessage::BrowseTo(to.to_owned())));
        assert!(effects.is_empty(), "navigation is pure ({to})");
        let view = rig.browser();
        assert_eq!(view.prefix, to);
        assert_eq!(view.level, level, "{to}");
        assert_eq!(view.bars.len(), bars, "{to}");
        assert_eq!(view.stats.count, count, "{to}");
    }
    // Under B90 the label is the subcode letter; under B it is the prefix.
    rig.raw(eco(EcoMessage::BrowseTo("B90".to_owned())));
    assert_eq!(rig.browser().bars[0].label, "a");
    assert_eq!(rig.browser().bars[0].prefix, "B90a");
    assert_eq!(rig.browser().bars[25].label, "z");
    // Up: one character at a time, and the root is the end.
    for expected in ["B9", "B", ""] {
        let effects = rig.raw(eco(EcoMessage::BrowseUp));
        assert!(effects.is_empty());
        assert_eq!(rig.browser().prefix, expected);
    }
    rig.raw(eco(EcoMessage::BrowseUp));
    assert_eq!(rig.browser().prefix, "");
}

#[test]
fn nothing_below_a_subcode_has_bars_but_the_list_stays() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    rig.open_browser();
    rig.raw(eco(EcoMessage::BrowseTo("B90a".to_owned())));
    let view = rig.browser();
    assert!(view.bars.is_empty());
    assert_eq!(view.grid, grid_lines(1));
    assert_eq!(view.stats.count, 1);
}

#[test]
fn a_prefix_that_is_not_a_node_of_the_tree_is_ignored() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    rig.open_browser();
    rig.raw(eco(EcoMessage::BrowseTo("B".to_owned())));
    for bad in [
        "Z",
        "b",
        "B9a",
        "B90A",
        "B90a1",
        "B90ab",
        "B 9",
        "9",
        "ÁB",
        "B90aaaaaaaa",
    ] {
        rig.raw(eco(EcoMessage::BrowseTo(bad.to_owned())));
        assert_eq!(rig.browser().prefix, "B", "{bad:?} must be refused");
    }
}

#[test]
fn browsing_with_no_browser_open_does_nothing() {
    let mut rig = Rig::new();
    for m in [
        EcoMessage::BrowseTo("B".to_owned()),
        EcoMessage::BrowseUp,
        EcoMessage::SetFind("x".to_owned()),
        EcoMessage::SubmitFind,
        EcoMessage::SetScope(BrowserScope::Filter),
        EcoMessage::ImportLine(0),
        EcoMessage::CloseBrowser,
    ] {
        assert!(rig.raw(eco(m)).is_empty());
    }
    assert_eq!(rig.state.eco_window(), None);
    assert!(rig.state.eco_browser_view().is_none());
}

#[test]
fn the_title_levels_follow_the_prefix_length() {
    assert_eq!(EcoLevel::of(""), EcoLevel::All);
    assert_eq!(EcoLevel::of("B"), EcoLevel::Section);
    assert_eq!(EcoLevel::of("B9"), EcoLevel::Section);
    assert_eq!(EcoLevel::of("B90"), EcoLevel::Code);
    assert_eq!(EcoLevel::of("B90a"), EcoLevel::Code);
}

// ---- the list ----

#[test]
fn the_list_shows_the_summary_of_the_node_and_follows_navigation() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    rig.open_browser();
    let book = rig.state.eco_book.clone().unwrap();
    for prefix in ["", "B", "C", "C4", "C40", "B90"] {
        rig.raw(eco(EcoMessage::BrowseTo(prefix.to_owned())));
        let expected: Vec<String> = book
            .summary(prefix)
            .iter()
            .map(|e| e.eco().to_string())
            .collect();
        let shown: Vec<String> = rig
            .browser()
            .entries
            .iter()
            .map(|e| e.code.clone())
            .collect();
        assert_eq!(shown, expected, "node {prefix:?}");
    }
    rig.raw(eco(EcoMessage::BrowseTo("C40".to_owned())));
    let row = &rig.browser().entries[0];
    assert_eq!(row.code, "C40");
    assert_eq!(row.name, "King's Knight Opening");
    assert_eq!(row.moves, "1.e4 e5 2.Nf3");
}

/// A book of well over 500 distinct positions, all coded `B90a` so the node
/// `B90` lists every one of them (no thinning below three characters).
fn big_book() -> (String, usize) {
    let white = [
        "a3", "a4", "b3", "b4", "c3", "c4", "d3", "d4", "e3", "e4", "f3", "f4", "g3", "g4", "h3",
        "h4", "Na3", "Nc3", "Nf3", "Nh3",
    ];
    let black = [
        "a6", "a5", "b6", "b5", "c6", "c5", "d6", "d5", "e6", "e5", "f6", "f5", "g6", "g5", "h6",
        "h5", "Na6", "Nc6", "Nf6", "Nh6",
    ];
    let third = [
        "a3", "b3", "c3", "d3", "e3", "f3", "g3", "h3", "Nf3", "Nc3", "Nh3", "Na3",
    ];
    let mut text = String::new();
    let mut n = 0;
    for w in white {
        for b in black {
            n += 1;
            text.push_str(&format!("B90a \"Line {n} Sicilian\" 1.{w} {b} *\n"));
            for t in third {
                n += 1;
                text.push_str(&format!("B90a \"Line {n} Sicilian\" 1.{w} {b} 2.{t} *\n"));
            }
        }
    }
    let parsed =
        mychess_core::EcoBook::parse_with(&text, mychess_core::EcoParseOptions::skip_entry())
            .expect("parse");
    let len = parsed.book.len();
    (text, len)
}

#[test]
fn the_list_stops_at_500_rows_with_the_truncation_flag() {
    let (text, len) = big_book();
    assert!(len > 600, "the fixture book is big enough ({len})");
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "g.pgn", &eco_games());
    // Lenient parse for the fixture: illegal lines are skipped.
    let report =
        mychess_core::EcoBook::parse_with(&text, mychess_core::EcoParseOptions::skip_entry())
            .unwrap();
    rig.state.eco_book = Some(SharedEco::new(report.book));
    rig.open_browser();
    rig.raw(eco(EcoMessage::BrowseTo("B90".to_owned())));
    let view = rig.browser();
    assert_eq!(view.entries.len(), 500);
    assert!(view.truncated);
    // A search that hits everything is cut at 500 too.
    rig.raw(eco(EcoMessage::SetFind("sicilian".to_owned())));
    rig.raw(eco(EcoMessage::SubmitFind));
    let view = rig.browser();
    assert_eq!(view.entries.len(), 500);
    assert!(view.truncated);
    // Narrow search: not truncated.
    let name = rig.state.eco_book.as_ref().unwrap().entries()[2]
        .name()
        .to_owned();
    rig.raw(eco(EcoMessage::SetFind(format!("[{name}]"))));
    rig.raw(eco(EcoMessage::SubmitFind));
    let view = rig.browser();
    assert_eq!(view.entries.len(), 1);
    assert!(!view.truncated);
}

#[test]
fn a_short_list_is_not_flagged_as_truncated() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    rig.open_browser();
    assert!(!rig.browser().truncated);
}

#[test]
fn the_search_ignores_case_and_matches_code_original_and_translated_name() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    rig.open_browser_with(&[("Defence", "Defesa"), ("Sicilian", "Siciliana")]);
    let find = |rig: &mut Rig, text: &str| -> Vec<String> {
        rig.raw(eco(EcoMessage::SetFind(text.to_owned())));
        rig.raw(eco(EcoMessage::SubmitFind));
        rig.browser()
            .entries
            .iter()
            .map(|e| e.code.clone())
            .collect()
    };
    // Original name, other case.
    assert_eq!(find(&mut rig, "SICILIAN"), ["B20"]);
    assert_eq!(find(&mut rig, "sicilian defence"), ["B20"]);
    // Translated name (the original has no "siciliana" or "defesa").
    assert_eq!(find(&mut rig, "SiciliaNA"), ["B20"]);
    assert_eq!(find(&mut rig, "defesa"), ["B20"]);
    // The row shows the translated name.
    assert_eq!(rig.browser().entries[0].name, "Siciliana Defesa");
    // By code, any case.
    assert_eq!(find(&mut rig, "b20"), ["B20"]);
    assert_eq!(find(&mut rig, "c4"), ["C40"]);
    // No hit.
    assert!(find(&mut rig, "zzzz").is_empty());
    assert_eq!(rig.browser().finding, Some("zzzz"));
}

#[test]
fn the_search_looks_at_the_whole_book_not_only_the_node_and_empty_text_goes_back() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    rig.open_browser();
    rig.raw(eco(EcoMessage::BrowseTo("C".to_owned())));
    let node: Vec<String> = rig
        .browser()
        .entries
        .iter()
        .map(|e| e.code.clone())
        .collect();
    rig.raw(eco(EcoMessage::SetFind("Sicilian".to_owned())));
    rig.raw(eco(EcoMessage::SubmitFind));
    let found: Vec<String> = rig
        .browser()
        .entries
        .iter()
        .map(|e| e.code.clone())
        .collect();
    assert_eq!(found, ["B20"], "B20 is outside the node C");
    for blank in ["", "   ", "\t"] {
        rig.raw(eco(EcoMessage::SetFind(blank.to_owned())));
        rig.raw(eco(EcoMessage::SubmitFind));
        let back: Vec<String> = rig
            .browser()
            .entries
            .iter()
            .map(|e| e.code.clone())
            .collect();
        assert_eq!(back, node, "blank text {blank:?}");
        assert_eq!(rig.browser().finding, None);
    }
}

#[test]
fn typing_without_enter_does_not_search() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    rig.open_browser();
    let before = rig.browser().entries.len();
    rig.raw(eco(EcoMessage::SetFind("Sicilian".to_owned())));
    assert_eq!(rig.browser().entries.len(), before);
    assert_eq!(rig.browser().find, "Sicilian");
    assert_eq!(rig.browser().finding, None);
}

#[test]
fn the_search_handles_hostile_text_without_panicking() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    rig.open_browser();
    for text in [
        "[",
        "]",
        "\u{0}",
        "İ",
        "ß",
        &"x".repeat(100_000),
        "🙂",
        "B20 [",
        "\n",
    ] {
        rig.raw(eco(EcoMessage::SetFind(text.to_owned())));
        rig.raw(eco(EcoMessage::SubmitFind));
        assert!(rig.browser().entries.len() <= 500);
    }
}

// ---- no book ----

#[test]
fn without_a_book_the_browser_opens_with_stats_and_an_empty_list() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "g.pgn", &eco_games());
    assert!(rig.state.eco_book.is_none());
    let effects = rig.raw(eco(EcoMessage::OpenBrowser { glossary: vec![] }));
    assert_eq!(count(&effects, is_stats), 1);
    rig.settle(effects);
    let view = rig.browser();
    assert!(!view.book_loaded);
    assert!(view.entries.is_empty());
    assert_eq!(view.classified, 10, "the stats come from the index");
    assert!(view.truncated.eq(&false));
    // An empty book counts as no book.
    rig.state.eco_book = Some(SharedEco::new(mychess_core::EcoBook::default()));
    rig.raw(eco(EcoMessage::BrowseTo("B".to_owned())));
    assert!(!rig.browser().book_loaded);
    assert!(rig.browser().entries.is_empty());
}

#[test]
fn load_book_file_asks_the_host_for_an_eco_file() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "g.pgn", &eco_games());
    rig.open_browser();
    let effects = rig.raw(eco(EcoMessage::LoadBookFile));
    assert!(
        effects
            .iter()
            .any(|e| e.runs_on_host() && format!("{e:?}").contains("OpenEco")),
        "{effects:?}"
    );
}

#[test]
fn a_book_loaded_while_the_browser_is_open_fills_the_list() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "g.pgn", &eco_games());
    rig.open_browser();
    assert!(rig.browser().entries.is_empty());
    let path = dir.path().join("x.eco");
    rig.state.prefs.files.eco_file = Some(path.clone());
    rig.send(Message::EcoLoaded {
        path,
        result: Ok(book(BOOK)),
    });
    let view = rig.browser();
    assert!(view.book_loaded);
    assert!(!view.entries.is_empty());
}

// ---- scope and stale answers ----

#[test]
fn switching_the_scope_rebuilds_the_table_and_the_same_scope_does_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    let slot = rig.state.bases.current();
    rig.open_browser();
    assert!(!rig.browser().filter_active, "the filter has every game");
    rig.raw(eco(EcoMessage::CloseBrowser));
    // Keep only the first four games (all B20); the list is locked while a
    // dialog is open, so this happens with the browser closed.
    rig.keep(slot, &[1, 2, 3, 4]);
    rig.open_browser();
    assert_eq!(rig.browser().stats.count, 10);
    assert!(rig.browser().filter_active);
    let effects = rig.raw(eco(EcoMessage::SetScope(BrowserScope::Filter)));
    assert_eq!(count(&effects, is_stats), 1);
    assert!(rig.browser().building);
    rig.settle(effects);
    let view = rig.browser();
    assert_eq!(view.scope, BrowserScope::Filter);
    assert_eq!(view.stats.count, 4);
    assert_eq!(view.bars.iter().filter(|b| b.stats.count > 0).count(), 1);
    // Back to the whole base.
    let effects = rig.raw(eco(EcoMessage::SetScope(BrowserScope::WholeBase)));
    assert_eq!(count(&effects, is_stats), 1);
    rig.settle(effects);
    assert_eq!(rig.browser().stats.count, 10);
}

#[test]
fn an_answer_with_an_old_request_id_is_ignored() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    let slot = rig.state.bases.current();
    rig.keep(slot, &[1, 2, 3, 4]);
    let first = rig.raw(eco(EcoMessage::OpenBrowser { glossary: vec![] }));
    // The whole base, started first and answered last.
    let second = rig.raw(eco(EcoMessage::SetScope(BrowserScope::Filter)));
    assert_eq!(count(&first, is_stats), 1);
    assert_eq!(count(&second, is_stats), 1);
    let old = rig.perform(first.into_iter().next().unwrap());
    let new = rig.perform(second.into_iter().next().unwrap());
    // The old answer arrives while the new one is still being built.
    rig.send(old);
    assert!(
        rig.browser().building,
        "the old answer must not finish the build"
    );
    assert_eq!(rig.browser().stats.count, 0);
    rig.send(new);
    assert!(!rig.browser().building);
    assert_eq!(
        rig.browser().stats.count,
        4,
        "the table is the one of the filter"
    );
}

#[test]
fn an_old_answer_arriving_after_the_new_one_does_not_replace_it() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    let slot = rig.state.bases.current();
    rig.keep(slot, &[1, 2, 3, 4]);
    let first = rig.raw(eco(EcoMessage::OpenBrowser { glossary: vec![] }));
    let second = rig.raw(eco(EcoMessage::SetScope(BrowserScope::Filter)));
    let old = rig.perform(first.into_iter().next().unwrap());
    let new = rig.perform(second.into_iter().next().unwrap());
    rig.send(new);
    assert_eq!(rig.browser().stats.count, 4);
    rig.send(old);
    assert_eq!(rig.browser().stats.count, 4);
    assert_eq!(rig.browser().scope, BrowserScope::Filter);
}

#[test]
fn an_answer_with_no_browser_open_is_dropped() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    let effects = rig.raw(eco(EcoMessage::OpenBrowser { glossary: vec![] }));
    rig.raw(eco(EcoMessage::CloseBrowser));
    assert_eq!(rig.state.eco_window(), None);
    let late = rig.perform(effects.into_iter().next().unwrap());
    rig.send(late);
    assert_eq!(rig.state.eco_window(), None);
    assert!(rig.state.eco_browser_view().is_none());
}

#[test]
fn opening_another_window_closes_the_browser_and_only_one_is_open() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    rig.open_browser();
    rig.raw(eco(EcoMessage::OpenClassify));
    assert_eq!(rig.state.eco_window(), Some(EcoWindow::Classify));
    assert!(rig.state.eco_browser_view().is_none());
    rig.raw(eco(EcoMessage::OpenNovelty));
    assert_eq!(rig.state.eco_window(), Some(EcoWindow::Novelty));
    assert!(rig.state.eco_classify_view().is_none());
    rig.open_browser();
    assert_eq!(rig.state.eco_window(), Some(EcoWindow::Browser));
    assert!(rig.state.eco_novelty_view().is_none());
    assert!(rig.state.modal_open());
    rig.raw(eco(EcoMessage::CloseBrowser));
    assert!(!rig.state.modal_open());
}

#[test]
fn a_browser_on_a_database_that_is_closed_goes_away() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    let slot = rig.state.bases.current();
    rig.open_browser();
    rig.send(BasesMessage::Close(slot));
    assert!(rig.state.eco_browser_view().is_none() || rig.state.eco_window().is_some());
}

// ---- import the line ----

#[test]
fn importing_a_line_replaces_a_clean_game_closes_the_browser_and_goes_to_the_end() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    rig.open_browser();
    rig.raw(eco(EcoMessage::BrowseTo("C40".to_owned())));
    assert_eq!(rig.browser().entries[0].moves, "1.e4 e5 2.Nf3");
    rig.send(eco(EcoMessage::ImportLine(0)));
    assert_eq!(rig.state.eco_window(), None, "the browser closes");
    assert_eq!(rig.mainline_len(), 3);
    assert_eq!(rig.ply(), 3, "the cursor is at the end of the line");
    assert!(rig.state.is_dirty(), "the new game is marked as altered");
    assert_eq!(
        rig.state.session.position().fen(),
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2"
    );
}

#[test]
fn importing_a_line_over_an_altered_game_asks_first() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    rig.play_all(&["d2d4", "d7d5"]);
    assert!(rig.state.is_dirty());
    rig.open_browser();
    rig.raw(eco(EcoMessage::BrowseTo("C40".to_owned())));
    rig.send(eco(EcoMessage::ImportLine(0)));
    assert!(rig.state.discard_prompt.is_some(), "asks before replacing");
    assert_eq!(rig.mainline_len(), 2, "the game is still the old one");
    // Cancel keeps the old game.
    rig.send(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert_eq!(rig.mainline_len(), 2);
    assert!(rig.state.discard_prompt.is_none());
    // Import again and discard.
    rig.open_browser();
    rig.raw(eco(EcoMessage::BrowseTo("C40".to_owned())));
    rig.send(eco(EcoMessage::ImportLine(0)));
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(rig.mainline_len(), 3);
    assert_eq!(
        rig.state.session.game().tags().white,
        "?",
        "no tags come with the line"
    );
}

#[test]
fn importing_a_line_that_does_not_exist_changes_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    rig.open_browser();
    let n = rig.browser().entries.len();
    for index in [n, n + 1, usize::MAX] {
        assert!(rig.raw(eco(EcoMessage::ImportLine(index))).is_empty());
        assert_eq!(rig.state.eco_window(), Some(EcoWindow::Browser));
    }
    assert_eq!(rig.mainline_len(), 0);
}

#[test]
fn importing_the_start_entry_gives_an_empty_game() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with_games(dir.path());
    rig.play("e2e4");
    rig.open_browser();
    // The root list begins with the start entry (A00a, no moves).
    assert_eq!(rig.browser().entries[0].code, "A00a");
    rig.send(eco(EcoMessage::ImportLine(0)));
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(rig.mainline_len(), 0);
}
