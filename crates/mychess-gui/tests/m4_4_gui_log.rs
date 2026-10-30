//! The protocol log of a section (spec M4-4, D447, D802, D814, D816): it opens
//! in place of the lines, draws the last 400 entries that match the filter,
//! has Auto / Update / Find / Save as... / Close, and the 250 ms clock runs
//! only while a log is open, "Auto" is on and the panel is visible. Headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

mod m4_4_gui_common;

use m4_4_gui_common::*;
use mychess_app::engines::Protocol;
use mychess_engine::LogKind;
use mychess_gui::windows::analysis::{LOG_SEARCH_ID, LOG_SHOWN};

fn basic_log() -> Vec<mychess_engine::LogEntry> {
    vec![
        log_entry(0, LogKind::Sent, "uci"),
        log_entry(1, LogKind::Received, "uciok"),
        log_entry(2, LogKind::Stderr, "warning: hash"),
        log_entry(3, LogKind::Note, "session started"),
    ]
}

fn rig_with_log() -> (Rig, RunId) {
    let (mut rig, run) = uci_run_with_lines();
    open_log_with(&mut rig, run, basic_log());
    (rig, run)
}

use mychess_app::engines::RunId;

#[test]
fn the_log_opens_in_place_of_the_lines_with_its_four_prefixes() {
    let (rig, run) = rig_with_log();
    assert!(view(&rig, run).log_open);
    assert!(!has(&rig, "+0.20"), "D814: the log replaces the lines");
    for text in ["> uci", "< uciok", "! warning: hash", "# session started"] {
        assert!(has(&rig, text), "{text}");
    }
    for text in ["Auto", "Update", "Save as...", "Close"] {
        assert!(has(&rig, text), "{text}");
    }
    // The bar of the section is still there.
    assert!(has(&rig, "+ Move"));
}

#[test]
fn closing_the_log_brings_the_lines_back_and_forgets_the_entries() {
    let (mut rig, run) = rig_with_log();
    let messages = click_msgs(&rig, "Close");
    assert!(is_am(&messages, &AnalysisMessage::CloseLog(run)));
    apply(&mut rig, messages);
    assert!(!view(&rig, run).log_open);
    assert!(
        view(&rig, run).log.is_empty(),
        "D802: CloseLog forgets the entries"
    );
    assert!(has(&rig, "+0.20"));
    assert!(!has(&rig, "< uciok"));
}

#[test]
fn update_asks_for_the_new_entries() {
    let (rig, run) = rig_with_log();
    let messages = click_msgs(&rig, "Update");
    assert!(
        is_am(&messages, &AnalysisMessage::LogTick(run)),
        "{messages:?}"
    );
}

#[test]
fn save_as_asks_for_a_file_and_a_cancelled_dialog_changes_nothing() {
    let (mut rig, run) = rig_with_log();
    let messages = click_msgs(&rig, "Save as...");
    assert!(
        is_ui(&messages, &AnalysisUiMessage::PickLogFile(run)),
        "{messages:?}"
    );
    // The dialog is only a task (not run here); answering `None` is a no-op.
    let before = view(&rig, run).log.len();
    rig.msg(aui(AnalysisUiMessage::LogFilePicked(run, None)));
    assert_eq!(view(&rig, run).log.len(), before);
    assert!(notice(&rig).is_none());
}

#[test]
fn picking_a_file_turns_into_the_save_message_of_the_application() {
    let (mut rig, run) = rig_with_log();
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("engine.log");
    // The Task of the shell carries the effect; its work units show that an
    // effect was handed over, and no file is written by the shell itself.
    let task = rig.shell.update(aui(AnalysisUiMessage::LogFilePicked(
        run,
        Some(path.clone()),
    )));
    assert!(task.units() >= 1, "SaveLog became an effect");
    assert!(
        !path.exists(),
        "the write happens in the effect, not in update"
    );
}

#[test]
fn the_auto_checkbox_toggles_the_automatic_refresh() {
    let (mut rig, _run) = rig_with_log();
    assert!(rig.shell.ui.analysis.log_auto);
    let messages = click_msgs(&rig, "Auto");
    assert!(
        is_ui(&messages, &AnalysisUiMessage::LogAuto(false)),
        "{messages:?}"
    );
    apply(&mut rig, messages);
    assert!(!rig.shell.ui.analysis.log_auto);
    let messages = click_msgs(&rig, "Auto");
    assert!(is_ui(&messages, &AnalysisUiMessage::LogAuto(true)));
}

// ---- entries drawn ----

fn numbered_log(count: u64) -> Vec<mychess_engine::LogEntry> {
    (0..count)
        .map(|n| log_entry(n, LogKind::Received, &format!("line {n}")))
        .collect()
}

#[test]
fn only_the_last_four_hundred_entries_are_drawn() {
    let (mut rig, run) = uci_run_with_lines();
    open_log_with(&mut rig, run, numbered_log(1000));
    assert_eq!(view(&rig, run).log.len(), 1000);
    assert!(has(&rig, "< line 999"), "the newest");
    assert!(has(&rig, "< line 600"), "the 400th newest");
    assert!(!has(&rig, "< line 599"), "D814: the 401st is not drawn");
    assert!(!has(&rig, "< line 0"));
    assert_eq!(LOG_SHOWN, 400);
}

#[test]
fn the_filter_keeps_the_entries_that_contain_the_text_without_caring_for_case() {
    let (mut rig, run) = rig_with_log();
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetLogFilter(
        run,
        "UCI".to_owned(),
    )));
    assert!(has(&rig, "> uci"));
    assert!(has(&rig, "< uciok"));
    assert!(!has(&rig, "! warning: hash"));
    assert!(!has(&rig, "# session started"));
}

#[test]
fn a_filter_that_matches_nothing_says_the_log_is_empty() {
    let (mut rig, run) = rig_with_log();
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetLogFilter(
        run,
        "no such text".to_owned(),
    )));
    assert!(has(&rig, "The log is empty."));
    assert!(!has(&rig, "< uciok"));
}

#[test]
fn the_last_four_hundred_of_the_matches_are_drawn_not_of_the_whole_log() {
    let (mut rig, run) = uci_run_with_lines();
    open_log_with(&mut rig, run, numbered_log(3000));
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetLogFilter(
        run,
        "line 1".to_owned(),
    )));
    // Matches: 1, 10-19, 100-199, 1000-1999 = 1111 entries; the newest 400
    // end at 1999 and start at 1999 - 399 = 1600.
    assert!(has(&rig, "< line 1999"));
    assert!(has(&rig, "< line 1600"));
    assert!(!has(&rig, "< line 1599"));
    assert!(!has(&rig, "< line 2999"), "does not match the filter");
}

#[test]
fn the_find_field_sends_the_filter_as_the_user_types() {
    let (mut rig, run) = rig_with_log();
    let messages = {
        let mut s = rig.sim(SIZE);
        s.click(iced::widget::Id::new(LOG_SEARCH_ID))
            .expect("the find field");
        s.typewrite("ok");
        s.into_messages().collect::<Vec<_>>()
    };
    assert!(
        is_am(
            &messages,
            &AnalysisMessage::SetLogFilter(run, "ok".to_owned())
        ),
        "{messages:?}"
    );
    apply(&mut rig, messages);
    assert_eq!(view(&rig, run).log_filter, "ok");
    assert!(has(&rig, "< uciok"));
    assert!(!has(&rig, "> uci"));
}

#[test]
fn hostile_log_text_is_drawn_without_panicking() {
    let (mut rig, run) = uci_run_with_lines();
    let big = "x".repeat(100_000);
    let entries = vec![
        log_entry(0, LogKind::Received, "\u{0}\u{1}\u{7f}"),
        log_entry(1, LogKind::Received, "\u{202e}rtl\u{200b}"),
        log_entry(2, LogKind::Stderr, "{ $arg } -foo = bar"),
        log_entry(3, LogKind::Received, "\u{1F600}\u{1F468}\u{200D}\u{1F469}"),
        log_entry(4, LogKind::Received, &big),
        log_entry(5, LogKind::Note, ""),
        log_entry(6, LogKind::Sent, "line\r\nbreak\rhere\nand"),
    ];
    open_log_with(&mut rig, run, entries);
    assert!(has(&rig, "! { $arg } -foo = bar"));
    drop(rig.sim(SIZE));
}

#[test]
fn the_log_of_a_section_does_not_touch_the_other_sections() {
    let mut rig = Rig::new();
    let a = add_engine(&mut rig, "Alpha", Protocol::Uci);
    let b = add_engine(&mut rig, "Beta", Protocol::Uci);
    let run_a = start_with(&mut rig, a, identity("Alpha", Some(1..=5), true));
    let run_b = start_with(&mut rig, b, identity("Beta", Some(1..=5), true));
    open_log_with(&mut rig, run_a, basic_log());
    assert!(view(&rig, run_a).log_open);
    assert!(!view(&rig, run_b).log_open);
    assert_eq!(
        count_text(&rig, "Close"),
        1,
        "only Alpha's log has its Close"
    );
}

// ---- the clock ----

fn units(rig: &Rig) -> usize {
    rig.shell.subscription().units()
}

#[test]
fn the_log_clock_runs_only_with_an_open_log_auto_on_and_the_panel_visible() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    let idle = units(&rig);
    assert!(!rig.shell.analysis_log_ticking());
    open_log_with(&mut rig, run, basic_log());
    assert!(rig.shell.analysis_log_ticking());
    assert_eq!(units(&rig), idle + 1, "a 250 ms timer");
    // Auto off: the timer goes.
    rig.msg(aui(AnalysisUiMessage::LogAuto(false)));
    assert_eq!(units(&rig), idle);
    rig.msg(aui(AnalysisUiMessage::LogAuto(true)));
    assert_eq!(units(&rig), idle + 1);
    // Panel hidden: the timer goes, the log stays open.
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Analysis)));
    assert!(view(&rig, run).log_open);
    assert_eq!(units(&rig), idle, "an invisible log is not refreshed");
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Analysis)));
    assert_eq!(units(&rig), idle + 1);
    // Closing the log stops it.
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::CloseLog(run)));
    assert_eq!(units(&rig), idle);
}

#[test]
fn two_open_logs_share_one_timer() {
    let mut rig = Rig::new();
    let a = add_engine(&mut rig, "Alpha", Protocol::Uci);
    let b = add_engine(&mut rig, "Beta", Protocol::Uci);
    let run_a = start_with(&mut rig, a, identity("Alpha", Some(1..=5), true));
    let run_b = start_with(&mut rig, b, identity("Beta", Some(1..=5), true));
    let idle = units(&rig);
    open_log_with(&mut rig, run_a, basic_log());
    open_log_with(&mut rig, run_b, basic_log());
    assert_eq!(units(&rig), idle + 1);
}

#[test]
fn a_tick_reads_only_the_open_logs() {
    let mut rig = Rig::new();
    let a = add_engine(&mut rig, "Alpha", Protocol::Uci);
    let b = add_engine(&mut rig, "Beta", Protocol::Uci);
    let run_a = start_with(&mut rig, a, identity("Alpha", Some(1..=5), true));
    let _run_b = start_with(&mut rig, b, identity("Beta", Some(1..=5), true));
    let none = rig.shell.update(aui(AnalysisUiMessage::LogTickAll)).units();
    assert_eq!(none, 0, "no log open: nothing to read");
    open_log_with(&mut rig, run_a, basic_log());
    let one = rig.shell.update(aui(AnalysisUiMessage::LogTickAll)).units();
    assert!(one >= 1, "the open log is read");
    // A second tick while the read is still in flight does not stack up.
    let again = rig.shell.update(aui(AnalysisUiMessage::LogTickAll)).units();
    assert!(again <= one, "one read at a time (D802): {again} vs {one}");
}

#[test]
fn a_log_that_is_off_cannot_be_opened_from_the_view() {
    let (mut rig, _run) = uci_run_with_lines();
    rig.shell.app.prefs.analysis.log_lines = 0;
    assert!(!has(&rig, "Log"));
}
