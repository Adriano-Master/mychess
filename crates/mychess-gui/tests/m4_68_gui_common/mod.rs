//! Shared helpers of the `m4_6_gui_*` and `m4_8_gui_*` contract tests (specs
//! `docs/specs/m4-6-anotacao-automatica.md`, "Comportamento observavel",
//! "Textos de interface", D880-D900 and D910-D913, and
//! `docs/specs/m4-8-ameaca-shootout-automove.md`, D840-D861 and D914-D918),
//! headless.
//!
//! Engines are never spawned: the run is created through the real `AppState`
//! and the engine's answers are synthetic `EngineEvent`s, applied with the
//! application's own `update` so that the `Effect`s it asks for can be read.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

#[path = "../m4_4_gui_common/mod.rs"]
pub mod base;

pub use base::common::fingerprint;
pub use base::*;
use iced::widget::Id;
use iced::{Element, Point, Rectangle, Settings, Size};
use iced_test::Simulator;
pub use iced_test::selector::Bounded;
use iced_test::selector::Candidate;
pub use mychess_app::engines::EngineMessage;
pub use mychess_app::engines::{Protocol, RunId, StartMode};
pub use mychess_app::{
    AnnotateMessage, AnnotateNotice, AnnotateOptions, AppNotice, AppState, Control, Effect,
    EngineMode, GameMessage, Informant, ModeMessage, ModeNotice, StopReason, WithScore, WithVars,
};
use mychess_core::{Game, NodeId};
use mychess_engine::{BestMove, EngineCommand, SearchRequest};
pub use mychess_gui::i18n::I18n;
pub use mychess_gui::windows::annotate_dialog;
pub use mychess_gui::windows::dialog_ui::ActiveDialog;

/// The application's `update` on the rig's state, effects returned (the
/// shell would hand them to the runtime).
pub fn up(rig: &mut Rig, message: impl Into<AppMessage>) -> Vec<Effect> {
    let effects = mychess_app::update(&mut rig.shell.app, message.into());
    rig.sync();
    effects
}

pub fn ann(rig: &mut Rig, message: AnnotateMessage) -> Vec<Effect> {
    up(rig, AppMessage::Annotate(message))
}

pub fn mode_msg(rig: &mut Rig, message: ModeMessage) -> Vec<Effect> {
    up(rig, AppMessage::Mode(message))
}

pub fn annotate_msg(message: AnnotateMessage) -> Message {
    Message::App(AppMessage::Annotate(message))
}

pub fn mode_message(message: ModeMessage) -> Message {
    Message::App(AppMessage::Mode(message))
}

/// The annotation messages among `messages`.
pub fn annotates(messages: &[Message]) -> Vec<AnnotateMessage> {
    messages
        .iter()
        .filter_map(|m| match m {
            Message::App(AppMessage::Annotate(a)) => Some(a.clone()),
            _ => None,
        })
        .collect()
}

/// The mode messages among `messages`.
pub fn modes_of(messages: &[Message]) -> Vec<ModeMessage> {
    messages
        .iter()
        .filter_map(|m| match m {
            Message::App(AppMessage::Mode(a)) => Some(*a),
            _ => None,
        })
        .collect()
}

pub fn active(rig: &Rig) -> Option<ActiveDialog> {
    ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs)
}

pub fn esc(rig: &mut Rig) {
    rig.press(named(NamedKey::Escape));
}

pub fn enter(rig: &mut Rig) {
    rig.press(named(NamedKey::Enter));
}

/// A game from the standard position with the moves `ucis`.
pub fn game_of(ucis: &[&str]) -> Game {
    let mut game = Game::new();
    let mut node = game.root();
    for uci in ucis {
        let mv = game
            .position_at(node)
            .expect("position")
            .parse_uci(uci)
            .expect("uci");
        node = game.add_move(node, mv).expect("legal");
    }
    game
}

pub const SCHOLAR: [&str; 7] = ["e2e4", "e7e5", "d1h5", "b8c6", "f1c4", "g8f6", "h5f7"];

/// A rig with a game and a UCI engine "Fake" that finished its handshake.
pub fn rig_with_engine(ucis: &[&str]) -> (Rig, RunId) {
    let mut rig = Rig::new();
    rig.set_game(game_of(ucis));
    let run = uci_run(&mut rig);
    (rig, run)
}

/// An XBoard engine that finished its handshake.
pub fn xboard_run(rig: &mut Rig) -> RunId {
    let id = add_engine(rig, "Xb", Protocol::XBoard);
    start_with(rig, id, identity("Xb", None, false))
}

/// The dialog box in a window of its own (the box is the same one the
/// window stacks above everything). The informant block follows the state
/// of the shell.
pub fn dialog_sim<'a>(rig: &'a Rig, informant: bool, i18n: &I18n) -> Simulator<'a, Message> {
    let open = informant || rig.shell.ui.dialogs.annotate_informant_open;
    let element: Element<'a, Message> =
        annotate_dialog::view(&rig.shell.app, open, i18n).expect("the dialog is open");
    Simulator::with_size(Settings::default(), Size::new(900.0, 900.0), element)
}

/// Opens the informant block (visual state of the dialog).
pub fn open_informant(rig: &mut Rig) {
    rig.shell.ui.dialogs.annotate_informant_open = true;
}

/// The centre of the visible part of the `n`-th (tree order) text with this
/// content; the body of the dialog scrolls, so it is scrolled down when the
/// text is below the fold.
pub fn nth_text_point(s: &mut Simulator<'_, Message>, label: &str, n: usize) -> Point {
    for _ in 0..4 {
        let mut seen = 0usize;
        let target = s
            .find(move |c: Candidate<'_>| match c {
                Candidate::Text {
                    content,
                    visible_bounds,
                    ..
                } if content == label => {
                    seen += 1;
                    (seen == n + 1).then_some(visible_bounds)
                }
                _ => None,
            })
            .unwrap_or_else(|e| panic!("text #{n} {label:?}: {e:?}"));
        if let Some(visible) = target {
            return visible.center();
        }
        s.point_at(Point::new(450.0, 300.0));
        drop(
            s.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -6.0 },
            })]),
        );
    }
    panic!("text #{n} {label:?} never became visible");
}

/// Clicks the `n`-th text with this content in the dialog.
pub fn dialog_click_nth(rig: &Rig, label: &str, n: usize) -> Vec<Message> {
    let mut s = dialog_sim(rig, false, &en());
    let at = nth_text_point(&mut s, label, n);
    s.point_at(at);
    drop(s.simulate([
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
            iced::mouse::Button::Left,
        )),
    ]));
    s.into_messages().collect()
}

/// What a click on `label` in the dialog sends.
pub fn dialog_click(rig: &Rig, label: &str) -> Vec<Message> {
    dialog_click_nth(rig, label, 0)
}

/// Whether the dialog box shows a text.
pub fn dialog_has(rig: &Rig, label: &str) -> bool {
    dialog_sim(rig, false, &en()).find(label).is_ok()
}

/// The options the dialog holds.
pub fn dialog_options(rig: &Rig) -> AnnotateOptions {
    rig.shell
        .app
        .annotate_dialog
        .as_ref()
        .expect("the dialog is open")
        .options
}

// ---- searches ----

pub fn commands(effects: &[Effect]) -> Vec<EngineCommand> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::EngineCommand { command, .. } => Some((**command).clone()),
            _ => None,
        })
        .collect()
}

pub fn last_search(effects: &[Effect]) -> Option<SearchRequest> {
    commands(effects).into_iter().rev().find_map(|c| match c {
        EngineCommand::Search(r) => Some(r),
        _ => None,
    })
}

pub fn stops(effects: &[Effect]) -> usize {
    commands(effects)
        .iter()
        .filter(|c| **c == EngineCommand::Stop)
        .count()
}

pub fn position_of(request: &SearchRequest) -> Position {
    let mut pos = request.position.start.clone();
    for m in &request.position.moves {
        pos.play(*m).expect("legal");
    }
    pos
}

/// Counter of the ids of synthetic searches.
pub struct Searches(pub u64);

impl Searches {
    pub fn new() -> Searches {
        Searches(500)
    }

    /// The engine answers the last search of `effects` with a score (white's
    /// point of view) and a line, then its best move. Returns the effects of
    /// the best move.
    pub fn answer(
        &mut self,
        rig: &mut Rig,
        run: RunId,
        effects: &[Effect],
        cp: i32,
        pv: &[&str],
    ) -> Option<Vec<Effect>> {
        let request = last_search(effects)?;
        self.0 += 1;
        let search = mychess_engine::SearchId(self.0);
        let at = position_of(&request);
        let ev = |rig: &mut Rig, e: mychess_engine::EngineEvent| {
            up(
                rig,
                AppMessage::Engine(EngineMessage::Event {
                    run,
                    event: Box::new(e),
                }),
            )
        };
        ev(
            rig,
            mychess_engine::EngineEvent::SearchTagged {
                search,
                tag: request.tag,
            },
        );
        ev(rig, mychess_engine::EngineEvent::SearchStarted(search));
        ev(
            rig,
            mychess_engine::EngineEvent::Info(report(&at, search.0, 1, 13, cp, pv)),
        );
        let mv = pv_of(&at, pv).first().copied();
        Some(ev(
            rig,
            mychess_engine::EngineEvent::BestMove(BestMove {
                search,
                mv,
                ponder: None,
            }),
        ))
    }
}

/// The engine dies.
pub fn exit_engine(rig: &mut Rig, run: RunId) {
    up(
        rig,
        AppMessage::Engine(EngineMessage::Event {
            run,
            event: Box::new(mychess_engine::EngineEvent::Exited(
                mychess_engine::ExitInfo {
                    reason: mychess_engine::ExitReason::Crashed,
                    status: None,
                    stderr_tail: Vec::new(),
                },
            )),
        }),
    );
}

pub fn app_notice(rig: &Rig) -> Option<AppNotice> {
    rig.shell.app.notice.clone()
}

pub fn annotate_notice(rig: &Rig) -> Option<AnnotateNotice> {
    match notice(rig).as_ref() {
        Some(AppNotice::Annotate(n)) => Some(*n),
        _ => None,
    }
}

pub fn mode_notice(rig: &Rig) -> Option<ModeNotice> {
    match notice(rig).as_ref() {
        Some(AppNotice::Mode(n)) => Some(n.clone()),
        _ => None,
    }
}

/// Whether the full window shows a text.
pub fn window_has(rig: &Rig, label: &str) -> bool {
    rig.sim(SIZE).find(label).is_ok()
}

/// A window of another size.
pub fn window_has_at(rig: &Rig, size: Size, label: &str) -> bool {
    rig.sim(size).find(label).is_ok()
}

pub fn ids(source: &str) -> Vec<String> {
    source
        .lines()
        .filter(|l| !l.starts_with(['#', ' ', '\t']) && l.contains('='))
        .filter_map(|l| l.split('=').next())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Lets the status bar notice the remark of the application (the shell's
/// clock starts on a tick; nothing here uses the real time to decide).
pub fn tick_status(rig: &mut Rig) {
    // The shell moves the remark of the application to the status bar after
    // each message it dispatches: a harmless one does it for the remarks
    // that `up` produced behind the shell's back.
    rig.app_msg(AppMessage::Annotate(AnnotateMessage::Tick {
        run: RunId(u64::MAX),
        generation: 0,
    }));
    rig.msg(Message::Dialog(
        mychess_gui::windows::dialog_ui::DialogMessage::StatusTick(std::time::Instant::now()),
    ));
}
