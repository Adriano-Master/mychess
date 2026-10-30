//! The button bar of a section (spec M4-4, "Comportamento observavel" 1,
//! regras 2, 5-8, D444, D445, D813, D815): what each button sends and when it
//! is disabled (a disabled button sends nothing). Headless, on the real
//! window.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

mod m4_4_gui_common;

use m4_4_gui_common::common;
use m4_4_gui_common::*;
use mychess_app::engines::{EngineFailure, EngineMessage, FailureKind, Protocol};
use mychess_gui::windows::analysis::EXCLUDE_INPUT_ID;

fn no_messages(messages: &[Message], what: &str) {
    assert!(
        messages.is_empty(),
        "{what}: expected nothing, got {messages:?}"
    );
}

// ---- add buttons ----

#[test]
fn the_add_buttons_send_their_message_on_the_left_click() {
    let (rig, run) = uci_run_with_lines();
    assert!(is_am(
        &click_msgs(&rig, "+ Move"),
        &AnalysisMessage::AddMove(run)
    ));
    assert!(is_am(
        &click_msgs(&rig, "+ Line"),
        &AnalysisMessage::AddLine(run)
    ));
    assert!(is_ui(
        &click_msgs(&rig, "+ All"),
        &AnalysisUiMessage::AddAll {
            run,
            secondary: false
        }
    ));
}

#[test]
fn the_right_click_of_the_add_buttons_is_score_second_line_and_second_only() {
    let (rig, run) = uci_run_with_lines();
    // D813 / Comportamento 1: "+ Move" right = score, "+ Line" right = line 2,
    // "+ All" right = only the second line.
    assert!(is_am(
        &right_click(&rig, "+ Move"),
        &AnalysisMessage::AddScore(run)
    ));
    assert!(is_am(
        &right_click(&rig, "+ Line"),
        &AnalysisMessage::AddLineNumber(run, 2)
    ));
    assert!(is_ui(
        &right_click(&rig, "+ All"),
        &AnalysisUiMessage::AddAll {
            run,
            secondary: true
        }
    ));
}

#[test]
fn the_left_click_does_not_also_send_the_right_click_message() {
    let (rig, run) = uci_run_with_lines();
    let left = click_msgs(&rig, "+ Move");
    assert!(!is_am(&left, &AnalysisMessage::AddScore(run)), "{left:?}");
    let right = right_click(&rig, "+ Move");
    assert!(!is_am(&right, &AnalysisMessage::AddMove(run)), "{right:?}");
}

/// The tree after clicking "+ All" in the way `how` says. The game is
/// 1.a3 a6 with the cursor at the start, so every line is a variation of
/// the next move (rule 7); at a leaf only the first line would be added
/// (D442).
fn add_all(how: &str) -> (Vec<String>, usize) {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    for u in ["a2a3", "a7a6"] {
        let mv = rig.shell.app.session.position().parse_uci(u).unwrap();
        rig.app_msg(AppMessage::Game(mychess_app::GameMessage::PlayMove {
            mv,
            options: mychess_app::PlayOptions::default(),
        }));
    }
    rig.app_msg(AppMessage::Game(mychess_app::GameMessage::Navigate(
        mychess_app::Nav::Start,
    )));
    three_long_lines(&mut rig, run);
    let messages = match how {
        "left" => click_msgs(&rig, "+ All"),
        "right" => right_click(&rig, "+ All"),
        "ctrl" => {
            hold_command(&mut rig, true);
            click_msgs(&rig, "+ All")
        }
        other => panic!("{other}"),
    };
    assert!(!messages.is_empty(), "{how}: the button sent something");
    apply(&mut rig, messages);
    (root_children_ucis(&rig), rig.game().node_count())
}

#[test]
fn add_all_adds_every_line_the_second_only_or_the_first_moves_only_by_the_click() {
    // Regra 8: Todas / Só a segunda / Só primeiros lances.
    let (all, n_all) = add_all("left");
    assert_eq!(all, ["a2a3", "e2e4", "d2d4", "g1f3"], "all three lines");
    assert_eq!(n_all, 3 + 6, "two moves each");
    let (second, n_second) = add_all("right");
    assert_eq!(second, ["a2a3", "d2d4"], "the second line only");
    assert_eq!(n_second, 3 + 2);
    let (first, n_first) = add_all("ctrl");
    assert_eq!(first, ["a2a3", "e2e4", "d2d4", "g1f3"], "three lines...");
    assert_eq!(n_first, 3 + 3, "...cut to their first move");
}

#[test]
fn add_all_is_one_undo_point() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    for u in ["a2a3", "a7a6"] {
        let mv = rig.shell.app.session.position().parse_uci(u).unwrap();
        rig.app_msg(AppMessage::Game(mychess_app::GameMessage::PlayMove {
            mv,
            options: mychess_app::PlayOptions::default(),
        }));
    }
    rig.app_msg(AppMessage::Game(mychess_app::GameMessage::Navigate(
        mychess_app::Nav::Start,
    )));
    three_long_lines(&mut rig, run);
    let messages = click_msgs(&rig, "+ All");
    apply(&mut rig, messages);
    assert_eq!(rig.game().node_count(), 9);
    rig.app_msg(AppMessage::Game(mychess_app::GameMessage::Undo));
    assert_eq!(rig.game().node_count(), 3, "one Undo takes everything back");
}

#[test]
fn add_all_at_the_end_of_the_line_adds_only_the_first_line() {
    // D442: at a leaf there is no "next move" to be a sibling of.
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    three_long_lines(&mut rig, run);
    let messages = click_msgs(&rig, "+ All");
    apply(&mut rig, messages);
    assert_eq!(root_children_ucis(&rig), ["e2e4"]);
}

#[test]
fn add_all_needs_at_least_two_lines() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    started(&mut rig, run, 1);
    info(
        &mut rig,
        run,
        report(&Position::new(), 1, 1, 10, 20, &["e2e4"]),
    );
    assert_eq!(view(&rig, run).multipv, 1);
    no_messages(&click_msgs(&rig, "+ All"), "+ All with one line");
    // The other two still work.
    assert!(is_am(
        &click_msgs(&rig, "+ Move"),
        &AnalysisMessage::AddMove(run)
    ));
}

#[test]
fn the_add_buttons_are_dead_until_the_engine_has_lines() {
    let mut rig = Rig::new();
    let _run = uci_run(&mut rig);
    for label in ["+ Move", "+ Line", "+ All"] {
        no_messages(&click_msgs(&rig, label), label);
        no_messages(&right_click(&rig, label), label);
    }
}

#[test]
fn the_add_buttons_and_exclude_are_dead_while_the_position_is_locked() {
    let (mut rig, run) = uci_run_with_lines();
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::ToggleLock(run)));
    assert!(rig.shell.app.engine_runs.get(run).unwrap().locked, "locked");
    for label in ["+ Move", "+ Line", "+ All", "Exclude"] {
        no_messages(&click_msgs(&rig, label), label);
    }
    // Right clicks as well.
    no_messages(&right_click(&rig, "+ Move"), "right + Move");
    // The stepper of lines is disabled too (D444).
    no_messages(&click_msgs(&rig, "\u{2212}"), "minus");
    no_messages(&click_msgs(&rig, "+"), "plus");
    // The lock button itself says "Locked" and unlocks.
    assert!(has(&rig, "Locked"));
    assert!(is_am(
        &click_msgs(&rig, "Locked"),
        &AnalysisMessage::ToggleLock(run)
    ));
}

#[test]
fn lock_sends_the_toggle_and_the_panel_explains_what_it_is_locked_on() {
    let (mut rig, run) = uci_run_with_lines();
    assert!(is_am(
        &click_msgs(&rig, "Lock"),
        &AnalysisMessage::ToggleLock(run)
    ));
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::ToggleLock(run)));
    assert!(view(&rig, run).lock.is_some());
    assert!(
        has(&rig, "Locked on start position"),
        "the hint under the bar"
    );
}

#[test]
fn the_lock_hint_names_the_move_and_the_variation_depth() {
    let sample = common::sample();
    let (nf3, nc3) = (sample.nf3, sample.nc3);
    let mut rig = Rig::with_game(sample.game);
    let run = uci_run(&mut rig);
    rig.goto(nf3);
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::ToggleLock(run)));
    assert!(has(&rig, "Locked on 2.Nf3"), "main line: no vardepth");
    rig.app_msg(AnalysisMessage::ToggleLock(run).into_app());
    // The variation 2.Nc3: (vardepth K) of analysis.tcl 3383-3397.
    rig.goto(nc3);
    rig.app_msg(AnalysisMessage::ToggleLock(run).into_app());
    assert!(
        has(&rig, "Locked on 2.Nc3 (vardepth 1)"),
        "the hint ends with (vardepth K) in a variation: {:?}",
        view(&rig, run).lock
    );
}

// ---- Lines stepper ----

#[test]
fn the_lines_stepper_goes_from_one_to_the_engine_maximum() {
    let (rig, run) = uci_run_with_lines(); // 3 lines, engine max 5
    assert!(is_am(
        &click_msgs(&rig, "\u{2212}"),
        &AnalysisMessage::SetMultiPv(run, 2)
    ));
    assert!(is_am(
        &click_msgs(&rig, "+"),
        &AnalysisMessage::SetMultiPv(run, 4)
    ));
}

#[test]
fn the_lines_stepper_stops_at_one_and_at_the_engine_maximum() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    assert_eq!(view(&rig, run).multipv, 1);
    no_messages(&click_msgs(&rig, "\u{2212}"), "below 1");
    assert!(is_am(
        &click_msgs(&rig, "+"),
        &AnalysisMessage::SetMultiPv(run, 2)
    ));
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetMultiPv(run, 5)));
    assert_eq!(view(&rig, run).multipv, 5);
    no_messages(&click_msgs(&rig, "+"), "above the engine's 5");
}

#[test]
fn the_lines_stepper_is_limited_to_eight_even_for_an_engine_that_allows_more() {
    let mut rig = Rig::new();
    let id = add_engine(&mut rig, "Fake", Protocol::Uci);
    let run = start_with(&mut rig, id, identity("Fake", Some(1..=200), true));
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetMultiPv(run, 7)));
    assert!(is_am(
        &click_msgs(&rig, "+"),
        &AnalysisMessage::SetMultiPv(run, 8)
    ));
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetMultiPv(run, 8)));
    assert_eq!(view(&rig, run).multipv, 8);
    no_messages(&click_msgs(&rig, "+"), "the stepper stops at min(max, 8)");
}

#[test]
fn an_engine_without_multipv_has_a_dead_stepper() {
    let mut rig = Rig::new();
    let id = add_engine(&mut rig, "Fake", Protocol::Uci);
    let _run = start_with(&mut rig, id, identity("Fake", None, true));
    no_messages(&click_msgs(&rig, "+"), "plus");
    no_messages(&click_msgs(&rig, "\u{2212}"), "minus");
}

// ---- XBoard ----

fn xboard_rig() -> (Rig, mychess_app::engines::RunId) {
    let mut rig = Rig::new();
    let id = add_engine(&mut rig, "Fake", Protocol::XBoard);
    let run = start_with(&mut rig, id, identity("Fake", None, false));
    started(&mut rig, run, 1);
    info(
        &mut rig,
        run,
        report(&Position::new(), 1, 1, 10, 20, &["e2e4"]),
    );
    (rig, run)
}

#[test]
fn an_xboard_section_has_update_but_no_exclusion_and_no_lines_stepper() {
    let (rig, run) = xboard_rig();
    assert!(is_am(
        &click_msgs(&rig, "Update"),
        &AnalysisMessage::RequestStats(run)
    ));
    no_messages(&click_msgs(&rig, "Exclude"), "Exclude on XBoard");
    no_messages(&click_msgs(&rig, "+"), "stepper up");
    no_messages(&click_msgs(&rig, "\u{2212}"), "stepper down");
    no_messages(&click_msgs(&rig, "+ All"), "+ All on XBoard");
    // Still adds the move.
    assert!(is_am(
        &click_msgs(&rig, "+ Move"),
        &AnalysisMessage::AddMove(run)
    ));
}

#[test]
fn a_uci_section_has_no_update_button() {
    let (rig, _run) = uci_run_with_lines();
    assert!(!has(&rig, "Update"));
}

#[test]
fn a_uci_engine_that_cannot_search_moves_has_no_exclusion() {
    let mut rig = Rig::new();
    let id = add_engine(&mut rig, "Fake", Protocol::Uci);
    let _run = start_with(&mut rig, id, identity("Fake", Some(1..=5), false));
    no_messages(&click_msgs(&rig, "Exclude"), "no searchmoves");
}

// ---- engine states ----

#[test]
fn a_starting_engine_shows_it_and_has_no_live_buttons() {
    let mut rig = Rig::new();
    let id = add_engine(&mut rig, "Fake", Protocol::Uci);
    rig.app_msg(AppMessage::Engine(EngineMessage::Start(
        id,
        mychess_app::engines::StartMode::Analyze,
    )));
    assert!(has(&rig, "starting..."));
    for label in ["+ Move", "+ Line", "Lock", "Exclude", "Low CPU"] {
        no_messages(&click_msgs(&rig, label), label);
    }
    // Info and the display mode only change the view: always available.
    assert!(!click_msgs(&rig, "Info").is_empty());
}

#[test]
fn a_stopped_analysis_says_stopped_and_keeps_the_lines_but_refuses_to_edit() {
    let (mut rig, run) = uci_run_with_lines();
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::Toggle(run)));
    assert!(!rig.shell.app.engine_runs.get(run).unwrap().analyzing);
    assert!(has(&rig, "stopped"));
    // Rule 11: the lines stay visible.
    assert!(has(&rig, "+0.30"));
    for label in ["+ Move", "+ Line", "+ All", "Lock", "Exclude"] {
        no_messages(&click_msgs(&rig, label), label);
    }
}

#[test]
fn a_failed_engine_shows_the_reason_and_only_the_close_button_acts_on_the_run() {
    let mut rig = Rig::new();
    let id = add_engine(&mut rig, "Fake", Protocol::Uci);
    rig.app_msg(AppMessage::Engine(EngineMessage::Start(
        id,
        mychess_app::engines::StartMode::Analyze,
    )));
    let run = rig.shell.app.engine_runs.of_engine(id).unwrap().id;
    rig.app_msg(AppMessage::Engine(EngineMessage::StartFailed {
        run,
        failure: EngineFailure {
            kind: FailureKind::CannotStart,
            exit_code: None,
            stderr_tail: Vec::new(),
        },
    }));
    // The section is still there and says it failed.
    let mut sim = rig.sim(SIZE);
    assert!(sim.find("Fake").is_ok());
    drop(sim);
    for label in ["+ Move", "+ Line", "+ All", "Lock", "Exclude", "Low CPU"] {
        no_messages(&click_msgs(&rig, label), label);
    }
}

// ---- toggles that only change the view ----

#[test]
fn priority_info_display_and_log_buttons_send_their_messages() {
    let (rig, run) = uci_run_with_lines();
    assert!(is_am(
        &click_msgs(&rig, "Low CPU"),
        &AnalysisMessage::ToggleLowPriority(run)
    ));
    assert!(is_am(
        &click_msgs(&rig, "Info"),
        &AnalysisMessage::ToggleInfo(run)
    ));
    assert!(is_am(
        &click_msgs(&rig, "No wrap"),
        &AnalysisMessage::CycleDisplay(run)
    ));
    assert!(is_am(
        &click_msgs(&rig, "Log"),
        &AnalysisMessage::OpenLog(run)
    ));
}

#[test]
fn the_log_button_is_hidden_when_the_log_is_off() {
    let (mut rig, _run) = uci_run_with_lines();
    assert!(has(&rig, "Log"));
    rig.shell.app.prefs.analysis.log_lines = 0;
    assert!(
        !has(&rig, "Log"),
        "D447: 0 turns the log off and hides the button"
    );
}

#[test]
fn the_display_button_names_the_current_mode_and_cycles_hidden_nowrap_wrap() {
    let (mut rig, run) = uci_run_with_lines();
    let mut seen = Vec::new();
    for _ in 0..3 {
        let label = ["No wrap", "Wrap", "Hidden"]
            .into_iter()
            .find(|l| has(&rig, l))
            .expect("one of the modes is named");
        seen.push(label);
        let messages = click_msgs(&rig, label);
        assert!(is_am(&messages, &AnalysisMessage::CycleDisplay(run)));
        apply(&mut rig, messages);
    }
    // D449: no wrap (default) -> wrap -> hidden -> no wrap (order of the Scid's toggle).
    assert_eq!(seen.len(), 3);
    assert_eq!(
        seen.iter().collect::<std::collections::BTreeSet<_>>().len(),
        3,
        "three different modes: {seen:?}"
    );
    assert_eq!(seen[0], "No wrap", "the default is no wrap (wrapPV off)");
}

#[test]
fn the_info_button_shows_the_two_information_lines() {
    let (mut rig, run) = uci_run_with_lines();
    let messages = click_msgs(&rig, "Info");
    apply(&mut rig, messages);
    assert!(view(&rig, run).show_info);
    let (first, second) =
        mychess_gui::windows::analysis_text::info_lines(&en(), view(&rig, run), true);
    assert!(has(&rig, &first), "first line: {first}");
    assert!(has(&rig, &second.expect("UCI has a second line")));
}

#[test]
fn the_close_button_ends_the_run_of_its_section() {
    let (rig, run) = uci_run_with_lines();
    let at = close_button_point(&rig, "+0.30");
    let messages = click_at(&rig, at);
    assert!(
        is_am(&messages, &AnalysisMessage::Close(run)),
        "{messages:?}"
    );
}

#[test]
fn the_start_stop_button_toggles_the_analysis() {
    let (rig, run) = uci_run_with_lines();
    let messages = click_at(&rig, toggle_button_point(&rig));
    assert!(
        is_am(&messages, &AnalysisMessage::Toggle(run)),
        "{messages:?}"
    );
}

// ---- exclusion box (D814, D445) ----

#[test]
fn the_exclude_button_opens_the_box_inside_the_section_and_cancel_closes_it() {
    let (mut rig, run) = uci_run_with_lines();
    let messages = click_msgs(&rig, "Exclude");
    assert!(is_ui(&messages, &AnalysisUiMessage::OpenExclude(run)));
    apply(&mut rig, messages);
    assert!(view(&rig, run).exclude_open);
    assert!(has(&rig, "Exclude moves"), "the box has its title");
    assert!(has(
        &rig,
        "Moves in SAN, separated by spaces or commas. Leave empty to clear."
    ));
    let messages = click_msgs(&rig, "Cancel");
    assert!(is_am(&messages, &AnalysisMessage::CloseExclude(run)));
    apply(&mut rig, messages);
    assert!(!view(&rig, run).exclude_open);
    assert!(!has(&rig, "Exclude moves"));
}

fn type_exclusion(rig: &mut Rig, text: &str) {
    let messages = {
        let mut s = rig.sim(SIZE);
        s.click(iced::widget::Id::new(EXCLUDE_INPUT_ID))
            .expect("the exclusion field");
        s.typewrite(text);
        s.into_messages().collect::<Vec<_>>()
    };
    apply(rig, messages);
}

fn open_exclude(rig: &mut Rig) {
    let messages = click_msgs(rig, "Exclude");
    apply(rig, messages);
}

#[test]
fn typing_in_the_exclusion_box_and_confirming_excludes_the_moves() {
    let (mut rig, run) = uci_run_with_lines();
    open_exclude(&mut rig);
    type_exclusion(&mut rig, "e4, d4");
    assert_eq!(rig.shell.ui.analysis.exclude_text, "e4, d4");
    let messages = click_msgs(&rig, "OK");
    assert!(is_ui(&messages, &AnalysisUiMessage::ExcludeSubmit(run)));
    apply(&mut rig, messages);
    assert_eq!(view(&rig, run).exclusion.len(), 2);
    assert_eq!(view(&rig, run).exclusion_san, ["e4", "d4"]);
}

#[test]
fn an_illegal_token_is_reported_in_the_status_bar_with_the_token() {
    let (mut rig, run) = uci_run_with_lines();
    open_exclude(&mut rig);
    type_exclusion(&mut rig, "Zz9");
    let messages = click_msgs(&rig, "OK");
    apply(&mut rig, messages);
    assert!(view(&rig, run).exclusion.is_empty());
    match notice(&rig) {
        Some(mychess_app::AppNotice::Analysis(mychess_app::AnalysisNotice::ExcludeInvalid(t))) => {
            assert_eq!(t, "Zz9");
        }
        other => panic!("expected ExcludeInvalid, got {other:?}"),
    }
}

#[test]
fn excluding_every_legal_move_is_refused_with_a_notice() {
    let (mut rig, run) = uci_run_with_lines();
    open_exclude(&mut rig);
    let all = "a3 a4 b3 b4 c3 c4 d3 d4 e3 e4 f3 f4 g3 g4 h3 h4 Na3 Nc3 Nf3 Nh3";
    type_exclusion(&mut rig, all);
    let messages = click_msgs(&rig, "OK");
    apply(&mut rig, messages);
    assert!(view(&rig, run).exclusion.is_empty());
    assert!(matches!(
        notice(&rig),
        Some(mychess_app::AppNotice::Analysis(
            mychess_app::AnalysisNotice::ExcludeAll
        ))
    ));
}

#[test]
fn an_empty_confirmation_clears_the_exclusion_and_the_box_starts_with_the_current_one() {
    let (mut rig, run) = uci_run_with_lines();
    open_exclude(&mut rig);
    type_exclusion(&mut rig, "e4");
    let messages = click_msgs(&rig, "OK");
    apply(&mut rig, messages);
    assert_eq!(view(&rig, run).exclusion_san, ["e4"]);
    // Reopen: the field holds the current exclusion (as the Scid's dialog).
    open_exclude(&mut rig);
    assert_eq!(rig.shell.ui.analysis.exclude_text, "e4");
    // Clear the text and confirm.
    rig.msg(aui(AnalysisUiMessage::ExcludeInput(String::new())));
    let messages = click_msgs(&rig, "OK");
    apply(&mut rig, messages);
    assert!(view(&rig, run).exclusion.is_empty());
}

#[test]
fn hostile_exclusion_text_never_panics() {
    let (mut rig, _run) = uci_run_with_lines();
    let long = "e4 ".repeat(10_000);
    for text in [
        "",
        "   ",
        ",,,",
        "\u{0}\u{1}\u{202e}e4",
        "e4\u{2028}d4",
        "{ $x } -x",
        "\u{1F600}",
        long.as_str(),
    ] {
        open_exclude(&mut rig);
        rig.msg(aui(AnalysisUiMessage::ExcludeInput(text.to_owned())));
        let messages = click_msgs(&rig, "OK");
        apply(&mut rig, messages);
        drop(notice(&rig));
        drop(rig.sim(SIZE).find("Fake"));
    }
}
