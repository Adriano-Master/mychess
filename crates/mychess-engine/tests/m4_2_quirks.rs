//! M4-2 contract tests: the engine quirk table (D421), one row per entry of
//! `tcl/tools/wbdetect.tcl` (the expectations come from the Tcl *code*, not
//! from its log messages), Crafty, and "announced features win the table".

#[path = "m4_1_common/helpers.rs"]
mod common;
#[path = "m4_2_common/xb.rs"]
mod xb;

use common::*;
use mychess_engine::{EngineCommand, Score};
use xb::*;

struct Row {
    name: &'static str,
    text: &'static str,
    analyze: bool,
    setboard: bool,
    seconds: bool,
}

const fn row(
    name: &'static str,
    text: &'static str,
    analyze: bool,
    setboard: bool,
    seconds: bool,
) -> Row {
    Row {
        name,
        text,
        analyze,
        setboard,
        seconds,
    }
}

/// `wbdetect.tcl`: `has_analyze`, `has_setboard` and `wholeSeconds` set by
/// each `if` block (read from the code, which differs from some messages).
const ROWS: &[Row] = &[
    row("Crafty", "Crafty v23.4 JA (1 cpus)", true, true, false),
    row("Amy", "Amy version 0.7", true, false, false),
    row("Baron", "Baron 0.28a", true, true, true),
    row("Duke", "D U K E 1.1", true, false, false),
    row("ESC", "ESCbook.bin loaded", true, false, false),
    row("Fortress", "FORTRESS 1.62", true, false, false),
    row("GNU Chess 5", "GNU Chess v5.02", false, true, false),
    row("Gromit", "Gromit3.00", true, false, false),
    row("Gromit exact", "GROMIT", true, false, false),
    row("Jester", "Jester 0.82", true, false, false),
    row("Leila", "(c) Calzerano", true, false, false),
    row("LordKing", "LordKing 3.2", true, false, false),
    row("Nejmet 2", "NEJMET 2.6.0", true, true, false),
    row("Nejmet 3", "Nejmet 3.0.1", true, true, false),
    row("Pharaon", "Pharaon 2.61", true, true, true),
    row("Skaki", "Skaki 1.19", true, false, false),
    row("TCB", "EngineControl-TCB 0045", true, false, false),
    row("ZChess", "ZChess 2.22", true, false, true),
    row("King of Kings", "King of Kings 2.02", true, true, false),
    row("EXchess", "EXchess 4.03", true, true, false),
    row("WildCat", "WildCat version 2.61", true, false, false),
    row("Phalanx", "Phalanx XXIII", true, true, false),
    row("Scorpio", "Scorpio 2.7", true, false, false),
];

struct Seen {
    analyze: bool,
    setboard: bool,
    seconds: bool,
    init: Vec<String>,
}

/// Starts an infinite search from the standard position on a legacy engine
/// (no `feature`), after the given start-up text, and reports what the
/// session did and understood.
async fn observe(banner: &[&str], features: &[&str]) -> Seen {
    let mut r = rig_with(xconfig(), banner, features).await;
    let (_, lines) = r.start_raw(infinite(START_FEN)).await;
    let analyze = lines.last().is_some_and(|l| l == "analyze");
    let setboard = lines.iter().any(|l| l.starts_with("setboard"));
    let info = r.think("1 +5 3 100 e2e4").await;
    Seen {
        analyze,
        setboard,
        seconds: info.time_ms == Some(3000),
        init: r.init.clone(),
    }
}

#[tokio::test(start_paused = true)]
async fn the_table_has_the_22_entries_of_wbdetect_and_each_is_recognised_from_its_text() {
    // 22 blocks in the Tcl file; Gromit has two patterns, so 23 rows here.
    assert_eq!(ROWS.len(), 23);
    for row in ROWS {
        let seen = observe(&[row.text], &[]).await;
        assert_eq!(seen.analyze, row.analyze, "{}: analyze", row.name);
        assert_eq!(seen.setboard, row.setboard, "{}: setboard", row.name);
        assert_eq!(seen.seconds, row.seconds, "{}: whole seconds", row.name);
    }
}

#[tokio::test(start_paused = true)]
async fn the_same_texts_work_when_they_arrive_in_myname() {
    for row in ROWS {
        let feature = format!("feature myname=\"{}\" done=1", row.text);
        let seen = observe(&[], &[&feature]).await;
        assert_eq!(
            (seen.analyze, seen.setboard, seen.seconds),
            (row.analyze, row.setboard, row.seconds),
            "{}",
            row.name
        );
    }
}

#[tokio::test(start_paused = true)]
async fn the_pattern_may_be_embedded_in_a_longer_line() {
    let seen = observe(&["Welcome!", "This is Baron 0.28a, enjoy."], &[]).await;
    assert!(seen.analyze && seen.setboard && seen.seconds);
}

#[tokio::test(start_paused = true)]
async fn unknown_engines_get_no_adjustments() {
    for text in [
        "Stockfish 17",
        "GNU Chess v6",
        "gromit",
        "GROMIT 2",
        "crafty lowercase",
        "Amy",
    ] {
        let seen = observe(&[text], &[]).await;
        assert_eq!(
            (seen.analyze, seen.setboard, seen.seconds),
            (false, false, false),
            "{text}"
        );
        assert_eq!(strs(&seen.init), ["easy", "post", "force"], "{text}");
    }
}

#[tokio::test(start_paused = true)]
async fn crafty_gets_its_four_opening_commands_before_the_standard_ones() {
    for (banner, features) in [
        (Some("Crafty v23.4 JA (1 cpus)"), None),
        (
            None,
            Some(r#"feature myname="Crafty-23.4 JA" name=1 done=1"#),
        ),
    ] {
        let b: Vec<&str> = banner.into_iter().collect();
        let f: Vec<&str> = features.into_iter().collect();
        let seen = observe(&b, &f).await;
        let init = strs(&seen.init);
        let at = init
            .iter()
            .position(|l| *l == "log off")
            .unwrap_or_else(|| panic!("no `log off` in {init:?}"));
        assert_eq!(
            init[at..at + 4],
            ["log off", "noise 1000", "egtb off", "resign 0"],
            "{init:?}"
        );
        assert_eq!(init[init.len() - 3..], ["easy", "post", "force"]);
    }
}

#[tokio::test(start_paused = true)]
async fn only_crafty_has_opening_commands() {
    for row in ROWS.iter().filter(|r| r.name != "Crafty") {
        let seen = observe(&[row.text], &[]).await;
        assert_eq!(strs(&seen.init), ["easy", "post", "force"], "{}", row.name);
    }
}

#[tokio::test(start_paused = true)]
async fn announced_features_win_over_the_table() {
    // Baron would be analyze + setboard; the engine says it has neither.
    let seen = observe(&["Baron 0.28a"], &["feature analyze=0 setboard=0 done=1"]).await;
    assert!(!seen.analyze && !seen.setboard);
    assert!(
        seen.seconds,
        "the time unit is not a feature: the table still applies"
    );
    // Crafty banner with analyze=0 and setboard=1 announced.
    let seen = observe(&["Crafty v23.4"], &["feature analyze=0 setboard=1 done=1"]).await;
    assert!(!seen.analyze && seen.setboard);
    // An engine the table does not know announcing analyze=1 only.
    let seen = observe(&["Unknown 1.0"], &["feature analyze=1 done=1"]).await;
    assert!(seen.analyze && !seen.setboard);
}

#[tokio::test(start_paused = true)]
async fn the_table_fills_in_what_the_engine_did_not_announce() {
    let seen = observe(&["Phalanx XXIII"], &["feature ping=0 done=1"]).await;
    assert!(seen.analyze && seen.setboard);
}

#[tokio::test(start_paused = true)]
async fn the_first_matching_entry_wins() {
    // `Crafty` is listed before `Amy`: both appear, Crafty's adjustments apply.
    let seen = observe(&["Amy version 1", "Crafty v1.0"], &[]).await;
    assert!(seen.analyze);
    // Amy is first in the table *and* in the text: setboard stays off.
    assert!(!seen.setboard, "Amy has no setboard");
}

#[tokio::test(start_paused = true)]
async fn crafty_version_boundary_17_9_vs_18_0_vs_23_4() {
    for (banner, white_pov) in [
        ("Crafty v17.9 JA", false),
        ("Crafty v18.0", true),
        ("Crafty v23.4 JA (1 cpus)", true),
    ] {
        let mut r = rig_with(xconfig(), &[banner], &[FULL]).await;
        r.engine
            .send(EngineCommand::Search(infinite(BLACK_WINS_QUEEN)))
            .unwrap();
        let lines = r.read_search_lines().await;
        r.pong(&lines).await;
        let info = r.think("6 +300 10 100 a3a1").await;
        let want = if white_pov { 300 } else { -300 };
        assert_eq!(info.score.map(|s| s.0), Some(Score::Cp(want)), "{banner}");
    }
}
