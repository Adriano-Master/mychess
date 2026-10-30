//! Shared helpers of the `m4_4_gui_*` contract tests (spec
//! `docs/specs/m4-4-janela-de-analise.md`, "Comportamento observavel",
//! "Textos de interface" and D810-D818: the analysis panel), headless.
//!
//! Engines are never spawned: the run is created through the real
//! `AppState` and the engine's answers are synthetic `EngineEvent`s, the way
//! the application's own `m4_4_common` does it.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

#[path = "../m2_4_common/mod.rs"]
pub mod common;

use std::ops::RangeInclusive;

pub use common::{Rig, ch, command, command_shift, named, plain};
use iced::{Settings, Size};
use iced_test::Simulator;
use mychess_app::engines::{
    EngineDraft, EngineId, EngineIdentity, EngineMessage, Protocol, RunId, StartMode,
};
pub use mychess_app::{
    AnalysisMessage, AnalysisView, Key, KeyInput, Message as AppMessage, Modifiers, NamedKey,
};
pub use mychess_core::{Move, Position};
use mychess_engine::{
    Bound, Capabilities, EngineEvent, LogEntry, LogKind, Score, SearchId, SearchInfo,
};
use mychess_gui::i18n::{I18n, Language};
pub use mychess_gui::windows::analysis::AnalysisUiMessage;
pub use mychess_gui::windows::layout::PaneKind;
pub use mychess_gui::{Message, UiMessage};

pub const SIZE: Size = Size::new(1400.0, 900.0);

pub fn en() -> I18n {
    I18n::new(Language::EnUs)
}

pub fn pt() -> I18n {
    I18n::new(Language::PtBr)
}

pub fn identity(
    name: &str,
    multipv: Option<RangeInclusive<u8>>,
    search_moves: bool,
) -> EngineIdentity {
    EngineIdentity {
        name: name.to_owned(),
        author: None,
        options: Vec::new(),
        capabilities: Capabilities {
            multipv,
            search_moves,
            ..Capabilities::default()
        },
    }
}

/// Adds an engine to the list (not started).
pub fn add_engine(rig: &mut Rig, name: &str, protocol: Protocol) -> EngineId {
    let mut draft = EngineDraft::new(name, "/fake/engine");
    draft.protocol = protocol;
    rig.shell.app.engines.add(draft).expect("add")
}

/// Starts `id` in analysis mode and answers its handshake with `identity`.
pub fn start_with(rig: &mut Rig, id: EngineId, identity: EngineIdentity) -> RunId {
    rig.app_msg(AppMessage::Engine(EngineMessage::Start(
        id,
        StartMode::Analyze,
    )));
    let run = rig
        .shell
        .app
        .engine_runs
        .of_engine(id)
        .expect("a run was created")
        .id;
    rig.app_msg(AppMessage::Engine(EngineMessage::Started {
        run,
        identity: Box::new(identity),
        at: 1_700_000_000,
    }));
    run
}

/// A UCI engine "Fake" with MultiPV 1..=5 and `searchmoves`.
pub fn uci_run(rig: &mut Rig) -> RunId {
    let id = add_engine(rig, "Fake", Protocol::Uci);
    start_with(rig, id, identity("Fake", Some(1..=5), true))
}

pub fn event(rig: &mut Rig, run: RunId, event: EngineEvent) {
    rig.app_msg(AppMessage::Engine(EngineMessage::Event {
        run,
        event: Box::new(event),
    }));
}

pub fn started(rig: &mut Rig, run: RunId, search: u64) {
    event(rig, run, EngineEvent::SearchStarted(SearchId(search)));
}

pub fn pv_of(position: &Position, ucis: &[&str]) -> Vec<Move> {
    let mut pos = position.clone();
    ucis.iter()
        .map(|u| {
            let m = pos.parse_uci(u).expect("uci");
            pos.play(m).expect("play");
            m
        })
        .collect()
}

pub fn report(
    position: &Position,
    search: u64,
    multipv: u8,
    depth: u32,
    cp: i32,
    pv: &[&str],
) -> SearchInfo {
    SearchInfo {
        search: SearchId(search),
        multipv,
        depth: Some(depth),
        score: Some((Score::Cp(cp), Bound::Exact)),
        pv: pv_of(position, pv),
        time_ms: Some(1000),
        nodes: Some(1_000_000),
        ..SearchInfo::default()
    }
}

pub fn info(rig: &mut Rig, run: RunId, info: SearchInfo) {
    event(rig, run, EngineEvent::Info(info));
}

/// The start position's three best lines, 1.e4 / 1.d4 / 1.Nf3, search 1.
pub fn three_lines(rig: &mut Rig, run: RunId) {
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetMultiPv(run, 3)));
    let start = Position::new();
    started(rig, run, 1);
    for (n, uci) in [(1u8, "e2e4"), (2, "d2d4"), (3, "g1f3")] {
        info(
            rig,
            run,
            report(&start, 1, n, 12, 40 - i32::from(n) * 10, &[uci]),
        );
    }
}

/// A UCI run with three lines.
pub fn uci_run_with_lines() -> (Rig, RunId) {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    three_lines(&mut rig, run);
    (rig, run)
}

pub fn am(message: AnalysisMessage) -> Message {
    Message::App(AppMessage::Analysis(message))
}

pub fn aui(message: AnalysisUiMessage) -> Message {
    Message::Ui(UiMessage::Analysis(message))
}

pub fn view(rig: &Rig, run: RunId) -> &AnalysisView {
    rig.shell.app.analysis_view(run).expect("a view")
}

/// The messages a click on `label` produces in the real window.
pub fn click_msgs(rig: &Rig, label: &str) -> Vec<Message> {
    let mut s = rig.sim(SIZE);
    s.click(label)
        .unwrap_or_else(|e| panic!("a clickable {label:?}: {e:?}"));
    strip_hover(s.into_messages().collect())
}

/// The `mouse_area` of the panel reports the pointer entering it on every
/// click of a simulator that starts with the pointer outside: not what the
/// tests look at.
pub fn strip_hover(messages: Vec<Message>) -> Vec<Message> {
    messages
        .into_iter()
        .filter(|m| {
            !matches!(
                m,
                Message::Ui(UiMessage::Analysis(AnalysisUiMessage::Hover(_)))
            )
        })
        .collect()
}

/// Whether a click on `label` finds a widget at all.
pub fn has(rig: &Rig, label: &str) -> bool {
    rig.sim(SIZE).find(label).is_ok()
}

pub fn is_am(messages: &[Message], wanted: &AnalysisMessage) -> bool {
    messages
        .iter()
        .any(|m| matches!(m, Message::App(AppMessage::Analysis(a)) if a == wanted))
}

pub fn is_ui(messages: &[Message], wanted: &AnalysisUiMessage) -> bool {
    messages
        .iter()
        .any(|m| matches!(m, Message::Ui(UiMessage::Analysis(a)) if a == wanted))
}

/// Applies the messages the way the runtime would (application messages
/// with their effects dropped, the rest to the shell).
pub fn apply(rig: &mut Rig, messages: Vec<Message>) {
    for m in messages {
        rig.msg(m);
    }
}

/// The hover of the pointer over the panel, as the `mouse_area` reports it.
pub fn hover(rig: &mut Rig, on: bool) {
    rig.msg(aui(AnalysisUiMessage::Hover(on)));
}

pub fn notice(rig: &Rig) -> Option<mychess_app::AppNotice> {
    rig.shell
        .ui
        .status_notice
        .clone()
        .or_else(|| rig.shell.app.notice.clone())
}

pub fn log_entry(seq: u64, kind: LogKind, text: &str) -> LogEntry {
    LogEntry {
        seq,
        kind,
        text: text.to_owned(),
    }
}

/// Opens the log of `run` and answers the first read with `entries`.
pub fn open_log_with(rig: &mut Rig, run: RunId, entries: Vec<LogEntry>) {
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::OpenLog(run)));
    let next = entries.last().map_or(0, |e| e.seq + 1);
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::LogRead {
        run,
        since: 0,
        entries,
        next,
    }));
}

pub fn ftl_text(language: &str, file: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

// ---- pointer helpers ----

use iced::{Point, Rectangle};
use iced_test::selector::Bounded;

/// The bounds of the first text with this content in the real window.
pub fn text_bounds(rig: &Rig, label: &str) -> Rectangle {
    let mut s = rig.sim(SIZE);
    let t = s
        .find(label)
        .unwrap_or_else(|e| panic!("a text {label:?}: {e:?}"));
    t.bounds()
}

/// Points at `at` and clicks with `button`, returning what the view sent.
pub fn press_at(rig: &Rig, at: Point, button: iced::mouse::Button) -> Vec<Message> {
    let mut s = rig.sim(SIZE);
    s.point_at(at);
    let events = [
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(button)),
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(button)),
    ];
    drop(s.simulate(events));
    strip_hover(s.into_messages().collect())
}

pub fn click_at(rig: &Rig, at: Point) -> Vec<Message> {
    press_at(rig, at, iced::mouse::Button::Left)
}

/// A right click on the text `label`.
pub fn right_click(rig: &Rig, label: &str) -> Vec<Message> {
    let b = text_bounds(rig, label);
    press_at(rig, b.center(), iced::mouse::Button::Right)
}

/// A middle click on the text `label`.
pub fn middle_click(rig: &Rig, label: &str) -> Vec<Message> {
    let b = text_bounds(rig, label);
    press_at(rig, b.center(), iced::mouse::Button::Middle)
}

/// The first icon-only button of the bar (Start/Stop): left of "+ Move".
pub fn toggle_button_point(rig: &Rig) -> Point {
    let add = text_bounds(rig, "+ Move");
    let name = text_bounds(rig, "Fake");
    Point::new(name.x + 10.0, add.center_y())
}

/// The close button of the first section: right of its score.
pub fn close_button_point(rig: &Rig, score: &str) -> Point {
    let s = text_bounds(rig, score);
    Point::new(s.x + s.width + 8.0 + 14.0, s.center_y())
}

/// Holds Ctrl/Cmd the way the subscription reports it.
pub fn hold_command(rig: &mut Rig, on: bool) {
    rig.msg(Message::Games(
        mychess_gui::windows::game_list::GameListMessage::Modifiers {
            command: on,
            shift: false,
        },
    ));
}

/// The first moves of the root's children, in order.
pub fn root_children_ucis(rig: &Rig) -> Vec<String> {
    let game = rig.game();
    let root = game.root();
    let pos = game.start_position().clone();
    game.children(root)
        .expect("children")
        .iter()
        .map(|id| pos.uci(game.mv(*id).expect("mv")))
        .collect()
}

/// Three lines of two moves each from the start position (1.e4 e5 / 1.d4
/// d5 / 1.Nf3 Nf6), search 1, depth 14.
pub fn three_long_lines(rig: &mut Rig, run: RunId) {
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetMultiPv(run, 3)));
    let start = Position::new();
    started(rig, run, 1);
    for (n, pv) in [
        (1u8, ["e2e4", "e7e5"]),
        (2, ["d2d4", "d7d5"]),
        (3, ["g1f3", "g8f6"]),
    ] {
        info(
            rig,
            run,
            report(&start, 1, n, 14, 40 - i32::from(n) * 10, &pv),
        );
    }
}

/// The bounds of the `n`-th (0-based, tree order) text with this content.
pub fn nth_text_bounds(rig: &Rig, label: &str, n: usize) -> Rectangle {
    use iced_test::selector::Candidate;
    let mut seen = 0usize;
    let mut s = rig.sim(SIZE);
    s.find(move |c: Candidate<'_>| match c {
        Candidate::Text {
            content, bounds, ..
        } if content == label => {
            seen += 1;
            (seen == n + 1).then_some(bounds)
        }
        _ => None,
    })
    .unwrap_or_else(|e| panic!("text #{n} {label:?}: {e:?}"))
}

/// How many texts with this content are in the tree.
pub fn count_text(rig: &Rig, label: &str) -> usize {
    use iced_test::selector::Candidate;
    let mut s = rig.sim(SIZE);
    let mut n = 0usize;
    // `find` stops at the first output: ask for ever later ones.
    loop {
        let mut seen = 0usize;
        let want = n;
        let found = s
            .find(move |c: Candidate<'_>| match c {
                Candidate::Text { content, .. } if content == label => {
                    seen += 1;
                    (seen == want + 1).then_some(())
                }
                _ => None,
            })
            .is_ok();
        if !found {
            return n;
        }
        n += 1;
    }
}

/// `AnalysisMessage` as an application message.
pub trait IntoApp {
    fn into_app(self) -> AppMessage;
}

impl IntoApp for AnalysisMessage {
    fn into_app(self) -> AppMessage {
        AppMessage::Analysis(self)
    }
}
