//! Performance rule and edge cases of the analysis panel (spec M4-4, regra
//! 16, D814, D816): `view` draws the texts the application already
//! formatted, huge PVs and histories, huge games and engine names, many
//! engines and many events. Headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

mod m4_4_gui_common;

use std::time::{Duration, Instant};

use iced::Settings;
use iced_test::Simulator;
use m4_4_gui_common::common::{self, fingerprint};
use m4_4_gui_common::*;
use mychess_app::engines::{EngineRunState, Protocol};
use mychess_app::{AnalysisPrefs, HistoryLine, PvLine};
use mychess_core::Game;
use mychess_engine::{Bound, Score};
use mychess_gui::i18n::I18n;
use mychess_gui::windows::analysis::{Ctx, RunInfo, Section, section};

fn pv_line(multipv: u8, text: &str) -> PvLine {
    PvLine {
        multipv,
        depth: 18,
        seldepth: None,
        score: Score::Cp(35),
        bound: Bound::Exact,
        wdl: None,
        pv: Vec::new(),
        time_ms: 1000,
        text: text.to_owned(),
    }
}

fn history_line(depth: u32, text: &str) -> HistoryLine {
    HistoryLine {
        depth,
        score: Score::Cp(10),
        bound: Bound::Exact,
        pv: Vec::new(),
        time_ms: 1000,
        text: text.to_owned(),
    }
}

/// The data of a running, analysing engine with eight lines (the rest of
/// the fields are the defaults of a fresh execution).
fn run_info(state: &EngineRunState) -> RunInfo<'_> {
    RunInfo {
        id: mychess_app::engines::RunId(1),
        state,
        analyzing: true,
        locked: false,
        low_priority: false,
        max_lines: Some(8),
        search_moves: true,
        arrows: false,
    }
}

/// A section built straight from a hand-made view (the PV moves are empty on
/// purpose: whatever is drawn must come from `text`).
fn sim_of<'a>(
    view: &'a AnalysisView,
    state: &'a EngineRunState,
    ui: &'a mychess_gui::windows::analysis::AnalysisUi,
    i18n: &'a I18n,
    name: &'a str,
) -> Simulator<'a, Message> {
    let ctx = Ctx {
        i18n,
        ui,
        log_enabled: true,
    };
    let element = section(
        &Section {
            run: run_info(state),
            name,
            protocol: Protocol::Uci,
            view,
        },
        &ctx,
    );
    Simulator::with_size(Settings::default(), SIZE, element)
}

fn fresh_view(multipv: u8) -> AnalysisView {
    AnalysisView::new(
        mychess_app::engines::RunId(1),
        multipv,
        &AnalysisPrefs::default(),
    )
}

#[test]
fn the_pv_text_comes_from_the_formatted_field_not_from_the_moves() {
    let state = EngineRunState::Running;
    let ui = mychess_gui::windows::analysis::AnalysisUi::default();
    let i18n = en();
    let mut a = fresh_view(1);
    a.lines = vec![pv_line(1, "1.e4 e5 2.Nf3 Nc6")];
    let mut b = fresh_view(1);
    b.lines = vec![pv_line(1, "1.d4 d5 2.c4 e6")];
    let mut c = fresh_view(1);
    c.lines = vec![pv_line(1, "1.e4 e5 2.Nf3 Nc6")];
    let fa = fingerprint(&mut sim_of(&a, &state, &ui, &i18n, "E"));
    let fb = fingerprint(&mut sim_of(&b, &state, &ui, &i18n, "E"));
    let fc = fingerprint(&mut sim_of(&c, &state, &ui, &i18n, "E"));
    assert_ne!(
        fa, fb,
        "the drawn moves follow `PvLine::text` (moves are empty)"
    );
    assert_eq!(fa, fc, "same text, same picture");
}

#[test]
fn the_history_text_comes_from_the_formatted_field() {
    let state = EngineRunState::Running;
    let ui = mychess_gui::windows::analysis::AnalysisUi::default();
    let i18n = en();
    let mut view = fresh_view(1);
    view.lines = vec![pv_line(1, "1.e4")];
    view.history = vec![history_line(10, "SENTINEL-TEXT")];
    let mut s = sim_of(&view, &state, &ui, &i18n, "E");
    assert!(s.find("10 [+0.10]  SENTINEL-TEXT (1.00 s)").is_ok());
}

#[test]
fn the_gui_source_formats_no_san_in_the_view() {
    // Static check of rule 16 / D814: the panel only reads what the
    // application formatted.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/windows/analysis.rs");
    let text = std::fs::read_to_string(path).unwrap();
    let production = text.split("#[cfg(test)]").next().unwrap();
    for forbidden in [
        "format_pv",
        ".san(",
        "Position::new",
        "Position::from",
        ".play(",
        "parse_san",
    ] {
        assert!(
            !production.contains(forbidden),
            "windows/analysis.rs formats or plays moves in the view: {forbidden}"
        );
    }
}

#[test]
fn an_enormous_pv_text_and_five_lines_draw_without_trouble() {
    let state = EngineRunState::Running;
    let ui = mychess_gui::windows::analysis::AnalysisUi::default();
    let i18n = en();
    let long: String = (1..=2000)
        .map(|n| format!("{n}.Nf3 Nf6 {n}.Ng1 Ng8"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut view = fresh_view(5);
    view.lines = (1..=5).map(|n| pv_line(n, &long)).collect();
    let started = Instant::now();
    let mut s = sim_of(&view, &state, &ui, &i18n, "E");
    assert!(s.find("+0.35").is_ok());
    drop(fingerprint(&mut s));
    assert!(
        started.elapsed() < Duration::from_secs(60),
        "layout and draw of 5 lines of 8000 moves took {:?}",
        started.elapsed()
    );
}

#[test]
fn five_lines_with_the_whole_history_draw_without_trouble() {
    let state = EngineRunState::Running;
    let ui = mychess_gui::windows::analysis::AnalysisUi::default();
    let i18n = en();
    let mut view = fresh_view(1);
    view.lines = vec![pv_line(1, "1.e4")];
    view.history = (0..200)
        .map(|n| history_line(100 + n, "1.e4 e5 2.Nf3"))
        .collect();
    let mut s = sim_of(&view, &state, &ui, &i18n, "E");
    // The newest 60 (depth 240..=299) are drawn; 239 is not.
    assert!(s.find("299 [+0.10]  1.e4 e5 2.Nf3 (1.00 s)").is_ok());
    assert!(s.find("240 [+0.10]  1.e4 e5 2.Nf3 (1.00 s)").is_ok());
    assert!(s.find("239 [+0.10]  1.e4 e5 2.Nf3 (1.00 s)").is_err());
    assert!(s.find("100 [+0.10]  1.e4 e5 2.Nf3 (1.00 s)").is_err());
}

#[test]
fn an_enormous_engine_name_and_unicode_names_draw_and_do_not_panic() {
    for name in [
        "x".repeat(5000),
        "Stockfish \u{202e}evil\u{202c} \u{1F40D} \u{200d}".to_owned(),
        "{ $name } -term = x".to_owned(),
        "\u{0}\u{1}".to_owned(),
        "a\nb\r\nc".to_owned(),
        String::new(),
        "\u{5f15}\u{64ce} \u{30a8}\u{30f3}\u{30b8}\u{30f3}".to_owned(),
    ] {
        let mut rig = Rig::new();
        let id = add_engine(&mut rig, "List", Protocol::Uci);
        let run = start_with(&mut rig, id, identity(&name, Some(1..=5), true));
        started(&mut rig, run, 1);
        info(
            &mut rig,
            run,
            report(&Position::new(), 1, 1, 10, 20, &["e2e4"]),
        );
        let mut sim = rig.sim(SIZE);
        // The buttons are still there whatever the name is.
        assert!(sim.find("+ Move").is_ok(), "name {name:?}");
        drop(sim);
        rig.app_msg(AppMessage::Analysis(AnalysisMessage::ToggleLock(run)));
        drop(rig.sim(SIZE).find("Lock"));
    }
}

#[test]
fn eight_engines_with_five_lines_each_keep_every_section_working() {
    let mut rig = Rig::new();
    let mut runs = Vec::new();
    for n in 0..8 {
        let name = format!("Engine{n}");
        let id = add_engine(&mut rig, &name, Protocol::Uci);
        let run = start_with(&mut rig, id, identity(&name, Some(1..=5), true));
        rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetMultiPv(run, 5)));
        started(&mut rig, run, 1);
        let start = Position::new();
        for (m, uci) in [
            (1u8, "e2e4"),
            (2, "d2d4"),
            (3, "g1f3"),
            (4, "c2c4"),
            (5, "b1c3"),
        ] {
            info(
                &mut rig,
                run,
                report(&start, 1, m, 15, 30 - i32::from(m), &[uci]),
            );
        }
        runs.push(run);
    }
    for run in &runs {
        assert_eq!(view(&rig, *run).lines.len(), 5);
    }
    assert!(has(&rig, "Engine0"));
    // A key acts on one section only.
    hover(&mut rig, true);
    rig.shell.ui.analysis.focus = Some(runs[3]);
    rig.press(ch('2'));
    for (i, run) in runs.iter().enumerate() {
        let expected = if i == 3 { 2 } else { 5 };
        assert_eq!(view(&rig, *run).multipv, expected, "section {i}");
    }
}

#[test]
fn two_thousand_info_events_in_a_row_leave_a_bounded_consistent_view() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetMultiPv(run, 3)));
    started(&mut rig, run, 1);
    let start = Position::new();
    let pvs = ["e2e4", "d2d4", "g1f3", "c2c4", "b1c3"];
    let begin = Instant::now();
    for i in 0..2000u32 {
        let k = (i % 5) as usize;
        let mut r = report(
            &start,
            1,
            u8::try_from(i % 3 + 1).unwrap(),
            10 + i / 10,
            20,
            &[pvs[k]],
        );
        r.nps = Some(u64::from(i));
        info(&mut rig, run, r);
    }
    let v = view(&rig, run);
    assert!(v.lines.len() <= 3, "never more than the lines asked for");
    assert!(v.history.len() <= mychess_app::MAX_HISTORY);
    let numbers: Vec<u8> = v.lines.iter().map(|l| l.multipv).collect();
    let mut sorted = numbers.clone();
    sorted.sort_unstable();
    assert_eq!(numbers, sorted, "ordered by multipv");
    assert!(has(&rig, "Fake"));
    assert!(
        begin.elapsed() < Duration::from_secs(60),
        "2000 events took {:?}",
        begin.elapsed()
    );
}

#[test]
fn a_two_hundred_fifty_six_move_pv_is_cut_to_what_the_application_formats() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    started(&mut rig, run, 1);
    let start = Position::new();
    let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
    let pv: Vec<&str> = (0..256).map(|i| cycle[i % 4]).collect();
    info(&mut rig, run, report(&start, 1, 1, 30, 10, &pv));
    let line = &view(&rig, run).lines[0];
    assert!(!line.text.is_empty());
    let words = line.text.split(' ').filter(|w| !w.is_empty()).count();
    assert!(
        words >= 256,
        "the text has every move and the numbers: {words}"
    );
    // The window draws it and a click on it does not panic.
    drop(rig.sim(SIZE).find("+0.10"));
    rig.msg(aui(AnalysisUiMessage::PlayPv {
        run,
        line: 1,
        index: 255,
    }));
    assert_eq!(rig.game().node_count(), 257, "256 moves played");
}

#[test]
fn a_huge_game_does_not_make_the_panel_or_a_click_on_a_pv_misbehave() {
    // 800 plies of knight shuffling, then the engine looks at the end.
    let mut game = Game::new();
    let mut at = game.root();
    let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
    for i in 0..800 {
        at = common::play(&mut game, at, cycle[i % 4]);
    }
    let mut rig = Rig::with_game(game);
    rig.app_msg(AppMessage::Game(mychess_app::GameMessage::Navigate(
        mychess_app::Nav::GameEnd,
    )));
    let run = uci_run(&mut rig);
    started(&mut rig, run, 1);
    let position = rig.shell.app.session.position().clone();
    info(
        &mut rig,
        run,
        report(&position, 1, 1, 12, 20, &["g1f3", "g8f6"]),
    );
    assert!(!view(&rig, run).lines.is_empty());
    assert!(has(&rig, "+0.20"));
    rig.msg(aui(AnalysisUiMessage::PlayPv {
        run,
        line: 1,
        index: 1,
    }));
    assert_eq!(rig.game().node_count(), 803);
    let messages = click_msgs(&rig, "+ Move");
    apply(&mut rig, messages);
}

#[test]
fn the_panel_in_a_tiny_window_does_not_panic() {
    let (rig, _run) = uci_run_with_lines();
    for size in [
        iced::Size::new(1.0, 1.0),
        iced::Size::new(120.0, 80.0),
        iced::Size::new(400.0, 300.0),
        iced::Size::new(4000.0, 3000.0),
    ] {
        let mut s = rig.sim(size);
        drop(s.find("Fake"));
    }
}

#[test]
fn messages_for_a_run_that_does_not_exist_are_harmless() {
    let mut rig = Rig::new();
    let ghost = mychess_app::engines::RunId(4242);
    for message in [
        AnalysisMessage::Toggle(ghost),
        AnalysisMessage::AddMove(ghost),
        AnalysisMessage::AddLine(ghost),
        AnalysisMessage::AddLineNumber(ghost, 9),
        AnalysisMessage::SetMultiPv(ghost, 200),
        AnalysisMessage::OpenLog(ghost),
        AnalysisMessage::LogTick(ghost),
        AnalysisMessage::CycleDisplay(ghost),
        AnalysisMessage::Close(ghost),
    ] {
        rig.app_msg(AppMessage::Analysis(message));
    }
    rig.msg(aui(AnalysisUiMessage::PlayPv {
        run: ghost,
        line: 1,
        index: 0,
    }));
    rig.msg(aui(AnalysisUiMessage::LogTickAll));
    rig.msg(aui(AnalysisUiMessage::ExcludeSubmit(ghost)));
    rig.msg(aui(AnalysisUiMessage::OpenExclude(ghost)));
    assert_eq!(rig.shell.app.engine_runs().len(), 0);
}

#[test]
fn a_zero_or_huge_multipv_request_is_clamped_by_the_application() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    for n in [0u8, 1, 5, 6, 255] {
        rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetMultiPv(run, n)));
        let m = view(&rig, run).multipv;
        assert!((1..=5).contains(&m), "{n} -> {m}");
        drop(rig.sim(SIZE).find("Fake"));
    }
}

// ---- why a button is disabled (pure) ----

#[test]
fn each_reason_has_its_own_hint_and_the_order_is_fixed() {
    use mychess_gui::windows::analysis::{Need, disabled_reason as why};
    assert_eq!(why(Protocol::Uci, Need::Nothing, false, false, false), None);
    assert_eq!(
        why(Protocol::XBoard, Need::Uci, false, false, false),
        Some("analysis-unsupported"),
        "XBoard cannot do what needs UCI"
    );
    assert_eq!(
        why(Protocol::XBoard, Need::Nothing, false, false, false),
        None,
        "XBoard is fine for what does not need UCI"
    );
    assert_eq!(
        why(Protocol::Uci, Need::Nothing, true, false, false),
        Some("analysis-unavailable-locked")
    );
    assert_eq!(
        why(Protocol::Uci, Need::Nothing, false, true, false),
        Some("analysis-tip-commandeered")
    );
    assert_eq!(
        why(Protocol::Uci, Need::Lines, false, false, true),
        Some("analysis-unavailable-no-lines")
    );
    // Several at once: unsupported, then locked, then commanded, then lines.
    assert_eq!(
        why(Protocol::XBoard, Need::Uci, true, true, true),
        Some("analysis-unsupported")
    );
    assert_eq!(
        why(Protocol::Uci, Need::Lines, true, true, true),
        Some("analysis-unavailable-locked")
    );
    assert_eq!(
        why(Protocol::Uci, Need::Lines, false, true, true),
        Some("analysis-tip-commandeered")
    );
    assert_eq!(
        why(Protocol::Uci, Need::Nothing, false, false, true),
        None,
        "a button that does not act on lines does not care for them"
    );
}
