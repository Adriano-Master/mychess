//! Shared helpers of the `m4_3_gui_*` contract tests (spec
//! `docs/specs/m4-3-gerencia-de-engines.md`, phase 3: the GUI), headless.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

#[path = "../m3_6_common/mod.rs"]
pub mod c;

pub use c::{Rig, ctrl, key_char};
use iced::{Element, Settings, Size};
use iced_test::Simulator;
use mychess_app::engines::{
    EditorMessage, EngineDraft, EngineFailure, EngineId, EngineMessage, EngineRunState,
    FailureKind, Hotkey, RunId, StartMode,
};
use mychess_app::{AppState, Effect, Message as AppMessage};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::{engine_editor, engine_options, engines_dialog};
use mychess_gui::{Message, UiMessage};

pub const SIZE: Size = Size::new(1000.0, 900.0);

pub fn en() -> I18n {
    I18n::new(Language::EnUs)
}

pub fn pt() -> I18n {
    I18n::new(Language::PtBr)
}

/// A message to the engine management.
pub fn em(message: EngineMessage) -> AppMessage {
    AppMessage::Engine(message)
}

/// Sends an engine message through the shell (effects are not run).
pub fn send(rig: &mut Rig, message: EngineMessage) {
    rig.app_msg(em(message));
}

/// A rig with these engines `(name, command)` in the list and the dialog
/// open (the first engine is selected, as the application does).
pub fn rig_with(engines: &[(&str, &str)]) -> (Rig, Vec<EngineId>) {
    let mut rig = Rig::new();
    let ids = engines
        .iter()
        .map(|(name, command)| {
            rig.shell
                .app
                .engines
                .add(EngineDraft::new(*name, *command))
                .expect("add")
        })
        .collect();
    send(&mut rig, EngineMessage::OpenDialog);
    (rig, ids)
}

/// The three engines of the usual scenario.
pub fn trio() -> (Rig, Vec<EngineId>) {
    rig_with(&[
        ("Stockfish", "/bin/stockfish"),
        ("Lc0", "/bin/lc0"),
        ("Crafty", "/bin/crafty"),
    ])
}

/// The box on top, drawn the way the dialogs module chooses it.
pub fn top_box<'a>(app: &'a AppState, prefs_open: bool, i18n: &I18n) -> Element<'a, Message> {
    let dialog = app.engine_dialog.as_ref().expect("the dialog is open");
    if dialog.options.is_some() {
        engine_options::view(app, i18n).expect("options box")
    } else if dialog.confirm_delete.is_some() {
        engines_dialog::delete_view(app, i18n).expect("delete box")
    } else if app.engine_editor.is_some() {
        engine_editor::view(app, i18n).expect("editor box")
    } else {
        engines_dialog::view(app, prefs_open, i18n).expect("list box")
    }
}

pub fn sim_of<'a>(app: &'a AppState, prefs_open: bool, i18n: &I18n) -> Simulator<'a, Message> {
    Simulator::with_size(Settings::default(), SIZE, top_box(app, prefs_open, i18n))
}

pub fn sim(rig: &Rig) -> Simulator<'_, Message> {
    sim_of(
        &rig.shell.app,
        rig.shell.ui.dialogs.engines_prefs_open,
        &en(),
    )
}

pub fn has(rig: &Rig, text: &str) -> bool {
    sim(rig).find(text).is_ok()
}

/// The messages that a click on `label` produces (not applied).
pub fn clicks(rig: &Rig, label: &str) -> Vec<Message> {
    let mut s = sim(rig);
    s.click(label)
        .unwrap_or_else(|e| panic!("a clickable {label:?}: {e:?}"));
    s.into_messages().collect()
}

/// The engine messages among `messages`.
pub fn engine_msgs(messages: &[Message]) -> Vec<EngineMessage> {
    messages
        .iter()
        .filter_map(|m| match m {
            Message::App(AppMessage::Engine(e)) => Some(e.clone()),
            _ => None,
        })
        .collect()
}

/// Clicks `label` and applies what it sent (effects are not run).
pub fn click(rig: &mut Rig, label: &str) -> Vec<EngineMessage> {
    let messages = clicks(rig, label);
    let engine = engine_msgs(&messages);
    rig.feed(messages);
    engine
}

/// The names of the engines in the list, in order.
pub fn names(rig: &Rig) -> Vec<String> {
    rig.shell
        .app
        .engines
        .engines()
        .iter()
        .map(|e| e.name.clone())
        .collect()
}

pub fn selected(rig: &Rig) -> Option<EngineId> {
    rig.shell
        .app
        .engine_dialog
        .as_ref()
        .and_then(|d| d.selected)
}

pub fn editor(rig: &mut Rig, message: EditorMessage) {
    send(rig, EngineMessage::Editor(message));
}

/// Starts `id` (the process is not spawned: the effect is dropped) and
/// returns its execution.
pub fn start(rig: &mut Rig, id: EngineId) -> RunId {
    send(rig, EngineMessage::Start(id, StartMode::Analyze));
    rig.shell.app.engine_runs.of_engine(id).expect("a run").id
}

pub fn failure(kind: FailureKind) -> EngineFailure {
    EngineFailure {
        kind,
        exit_code: None,
        stderr_tail: Vec::new(),
    }
}

/// `mychess_app::update` straight on the state of the rig (to see effects).
pub fn update(rig: &mut Rig, message: EngineMessage) -> Vec<Effect> {
    mychess_app::update(&mut rig.shell.app, em(message))
}

pub fn ftl_ids(language: &str, file: &str) -> std::collections::BTreeSet<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join(file);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .filter_map(|l| {
            let (id, _) = l.split_once(" = ")?;
            (!id.starts_with(' ') && !id.starts_with('#') && !id.starts_with('-'))
                .then(|| id.to_owned())
        })
        .collect()
}

pub use mychess_app::{Key, KeyInput, Modifiers, NamedKey};

pub fn ch(c: char) -> KeyInput {
    c::b::common::ch(c)
}

pub fn nk(key: NamedKey) -> KeyInput {
    c::b::named(key)
}
