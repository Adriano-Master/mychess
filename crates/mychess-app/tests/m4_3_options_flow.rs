//! The UCI options dialog as pure state and flow (spec M4-3, D430,
//! D740-D743; Scid `uci.tcl` 23, 761-801, 829-835, 878-926). Only the
//! public API of `mychess-app`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

use mychess_app::engines::{
    Capabilities, EngineDraft, EngineFailure, EngineId, EngineIdentity, EngineMessage,
    EngineOption, EngineRunState, FailureKind, OptionError, OptionKind, OptionValue,
    OptionsMessage, OptionsStatus, Protocol, StartMode, default_text, ordered_options, spin_step,
};
use mychess_app::{AppState, Effect, Message};
use mychess_engine::EngineCommand;
use rstest::rstest;

fn opt(name: &str, kind: OptionKind) -> EngineOption {
    EngineOption {
        name: name.to_owned(),
        kind,
    }
}

fn spin(name: &str, default: i64, min: i64, max: i64) -> EngineOption {
    opt(name, OptionKind::Spin { default, min, max })
}

fn identity(options: Vec<EngineOption>) -> EngineIdentity {
    EngineIdentity {
        name: "Fake".to_owned(),
        author: None,
        options,
        capabilities: Capabilities::default(),
    }
}

fn names(identity: &EngineIdentity) -> Vec<String> {
    ordered_options(identity)
        .iter()
        .map(|o| o.name.clone())
        .collect()
}

fn full() -> EngineIdentity {
    identity(vec![
        opt(
            "Style",
            OptionKind::Combo {
                default: "Normal".to_owned(),
                vars: vec!["Solid".to_owned(), "Normal".to_owned()],
            },
        ),
        spin("Skill Level", 20, 0, 20),
        spin("Threads", 1, 1, 1024),
        spin("Hash", 16, 1, 33_554_432),
        opt("Ponder", OptionKind::Check { default: false }),
        spin("MultiPV", 1, 1, 500),
        opt(
            "BookFile",
            OptionKind::String {
                default: String::new(),
            },
        ),
        opt("OwnBook", OptionKind::Check { default: false }),
        opt(
            "Syzygy Path",
            OptionKind::String {
                default: String::new(),
            },
        ),
        opt("UCI_LimitStrength", OptionKind::Check { default: false }),
        spin("UCI_Elo", 1320, 1320, 3190),
        opt("Clear Hash", OptionKind::Button),
        opt("UCI_Chess960", OptionKind::Check { default: false }),
    ])
}

// ---- order ----

#[test]
fn the_important_options_come_first_in_the_order_of_the_scid_then_the_rest_as_announced() {
    assert_eq!(
        names(&full()),
        [
            "MultiPV",
            "Hash",
            "OwnBook",
            "BookFile",
            "UCI_LimitStrength",
            "UCI_Elo",
            "Threads",
            "Skill Level",
            "Style",
            "Syzygy Path",
            "Clear Hash"
        ]
    );
}

#[test]
fn options_managed_by_the_interface_are_never_shown() {
    let shown = names(&full());
    for hidden in ["Ponder", "UCI_Chess960"] {
        assert!(!shown.iter().any(|n| n == hidden), "{hidden}");
    }
}

#[test]
fn an_engine_with_no_options_has_an_empty_form() {
    assert!(ordered_options(&identity(vec![])).is_empty());
}

#[test]
fn the_rest_keeps_the_announced_order_even_with_names_that_look_alike() {
    let id = identity(vec![
        spin("hash", 1, 0, 5),
        spin("Zeta", 1, 0, 5),
        spin("Hash", 1, 0, 5),
        spin("Alpha", 1, 0, 5),
    ]);
    // Names are matched exactly (case included), as UCI names are case-sensitive in the Scid's list.
    assert_eq!(names(&id), ["Hash", "hash", "Zeta", "Alpha"]);
}

// ---- spin step ----

#[rstest]
#[case("Hash", 1, 1024, 50)]
#[case("Small", 0, 10, 1)]
#[case("Edge100", 0, 100, 1)]
#[case("Edge101", 0, 101, 10)]
#[case("Edge1000", 0, 1000, 10)]
#[case("Edge1001", 0, 1001, 50)]
#[case("Negative", -100, 100, 10)]
#[case("Huge", i64::MIN, i64::MAX, 50)]
#[case("MultiPV", 1, 500, 1)]
#[case("Threads", 1, 1024, 1)]
#[case("MultiPV", 1, 100_000, 1)]
fn the_spin_step_follows_the_width_of_the_range(
    #[case] name: &str,
    #[case] min: i64,
    #[case] max: i64,
    #[case] step: i64,
) {
    assert_eq!(spin_step(name, min, max), step);
}

#[test]
fn the_step_is_never_zero_or_negative_even_for_odd_ranges() {
    for (min, max) in [(5, 5), (10, 1), (i64::MAX, i64::MIN), (0, 0)] {
        assert!(spin_step("X", min, max) >= 1, "{min}..{max}");
    }
}

// ---- the flow ----

struct World {
    app: AppState,
    id: EngineId,
}

fn world(options: Vec<(String, String)>, protocol: Protocol) -> World {
    let mut app = AppState::default();
    let mut draft = EngineDraft::new("Fake", "/bin/fake");
    draft.options = options;
    draft.protocol = protocol;
    let id = app.engines.add(draft).unwrap();
    app_send(&mut app, EngineMessage::OpenDialog);
    World { app, id }
}

fn app_send(app: &mut AppState, message: EngineMessage) -> Vec<Effect> {
    mychess_app::update(app, Message::Engine(message))
}

fn opts(app: &mut AppState, m: OptionsMessage) -> Vec<Effect> {
    app_send(app, EngineMessage::Options(m))
}

fn set(app: &mut AppState, name: &str, value: &str) {
    opts(
        app,
        OptionsMessage::Set {
            name: name.to_owned(),
            value: value.to_owned(),
        },
    );
}

/// Opens the options of the engine, answering the probe with `identity`.
fn open(w: &mut World, id: EngineIdentity) -> Vec<Effect> {
    let effects = app_send(&mut w.app, EngineMessage::OpenOptions(w.id));
    let request = effects.iter().find_map(|e| match e {
        Effect::TestEngine { request, .. } => Some(*request),
        _ => None,
    });
    if let Some(request) = request {
        app_send(
            &mut w.app,
            EngineMessage::Tested {
                request,
                result: Box::new(Ok(id)),
            },
        );
    }
    effects
}

fn dialog(w: &World) -> &mychess_app::engines::OptionsDialog {
    w.app
        .engine_dialog
        .as_ref()
        .unwrap()
        .options
        .as_ref()
        .expect("options dialog")
}

fn saved(w: &World) -> Vec<(String, String)> {
    w.app.engines.get(w.id).unwrap().options.clone()
}

fn run_it(w: &mut World, id: &EngineIdentity) -> mychess_app::engines::RunId {
    app_send(&mut w.app, EngineMessage::Start(w.id, StartMode::Analyze));
    let run = w.app.engine_runs.of_engine(w.id).unwrap().id;
    app_send(
        &mut w.app,
        EngineMessage::Started {
            run,
            identity: Box::new(id.clone()),
            at: 1,
        },
    );
    assert_eq!(
        w.app.engine_runs.get(run).unwrap().state,
        EngineRunState::Running
    );
    run
}

fn commands(effects: &[Effect]) -> Vec<(String, OptionValue)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::EngineCommand { command, .. } => match &**command {
                EngineCommand::SetOption { name, value } => Some((name.clone(), value.clone())),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

#[test]
fn opening_the_options_of_an_idle_uci_engine_probes_it_and_fills_the_form() {
    let mut w = world(vec![], Protocol::Uci);
    let effects = open(&mut w, full());
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::TestEngine { .. }))
    );
    assert_eq!(dialog(&w).status, OptionsStatus::Ready);
    assert_eq!(dialog(&w).value_of("Hash"), Some("16"));
    assert_eq!(dialog(&w).value_of("OwnBook"), Some("false"));
    assert_eq!(dialog(&w).value_of("Style"), Some("Normal"));
    assert_eq!(
        dialog(&w).value_of("Clear Hash"),
        None,
        "buttons have no value"
    );
}

#[test]
fn the_form_starts_from_the_saved_values() {
    let mut w = world(vec![("Hash".to_owned(), "128".to_owned())], Protocol::Uci);
    open(&mut w, full());
    assert_eq!(dialog(&w).value_of("Hash"), Some("128"));
}

#[test]
fn a_running_engine_gives_its_options_without_a_probe() {
    let mut w = world(vec![], Protocol::Uci);
    run_it(&mut w, &full());
    let effects = open(&mut w, full());
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::TestEngine { .. })),
        "{effects:?}"
    );
    assert_eq!(dialog(&w).status, OptionsStatus::Ready);
}

#[test]
fn xboard_engines_have_no_options_and_start_no_probe() {
    let mut w = world(vec![], Protocol::XBoard);
    let effects = app_send(&mut w.app, EngineMessage::OpenOptions(w.id));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(dialog(&w).status, OptionsStatus::NotUci);
}

#[test]
fn a_failed_probe_leaves_the_dialog_with_the_failure_and_nothing_to_save() {
    let mut w = world(vec![("Hash".to_owned(), "77".to_owned())], Protocol::Uci);
    let effects = app_send(&mut w.app, EngineMessage::OpenOptions(w.id));
    let Some(Effect::TestEngine { request, .. }) = effects.into_iter().next() else {
        panic!("a probe");
    };
    let failure = EngineFailure {
        kind: FailureKind::CannotStart,
        exit_code: None,
        stderr_tail: vec![],
    };
    app_send(
        &mut w.app,
        EngineMessage::Tested {
            request,
            result: Box::new(Err(failure.clone())),
        },
    );
    assert_eq!(dialog(&w).status, OptionsStatus::Failed(failure));
    opts(&mut w.app, OptionsMessage::Save);
    // Whether Save closes the dialog here is not specified (its button is
    // off); what matters is that the saved options are not lost.
    assert_eq!(saved(&w), [("Hash".to_owned(), "77".to_owned())]);
}

#[test]
fn save_without_a_ready_form_does_nothing_and_keeps_the_dialog_open() {
    // D754: Enter while probing or after a failure.
    let mut w = world(vec![("Hash".to_owned(), "77".to_owned())], Protocol::Uci);
    app_send(&mut w.app, EngineMessage::OpenOptions(w.id));
    assert_eq!(dialog(&w).status, OptionsStatus::Probing);
    let effects = app_send(&mut w.app, EngineMessage::Options(OptionsMessage::Save));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(dialog(&w).status, OptionsStatus::Probing, "still open");
    assert_eq!(saved(&w), [("Hash".to_owned(), "77".to_owned())]);
}

#[test]
fn an_answer_to_another_request_does_not_fill_the_options_dialog() {
    let mut w = world(vec![], Protocol::Uci);
    app_send(&mut w.app, EngineMessage::OpenOptions(w.id));
    app_send(
        &mut w.app,
        EngineMessage::Tested {
            request: mychess_app::RequestId::default(),
            result: Box::new(Ok(full())),
        },
    );
    assert_eq!(dialog(&w).status, OptionsStatus::Probing);
}

#[test]
fn save_keeps_only_what_differs_from_the_default() {
    let mut w = world(vec![], Protocol::Uci);
    open(&mut w, full());
    set(&mut w.app, "Hash", "128");
    set(&mut w.app, "OwnBook", "true");
    set(&mut w.app, "Threads", "1"); // the default
    set(&mut w.app, "Style", "Normal"); // the default
    opts(&mut w.app, OptionsMessage::Save);
    assert_eq!(
        saved(&w),
        [
            ("Hash".to_owned(), "128".to_owned()),
            ("OwnBook".to_owned(), "true".to_owned())
        ]
    );
    assert!(
        w.app.engine_dialog.as_ref().unwrap().options.is_none(),
        "Save closes the dialog"
    );
}

#[test]
fn going_back_to_the_default_removes_the_saved_value() {
    let mut w = world(vec![("Hash".to_owned(), "128".to_owned())], Protocol::Uci);
    open(&mut w, full());
    set(&mut w.app, "Hash", "16");
    opts(&mut w.app, OptionsMessage::Save);
    assert!(saved(&w).is_empty(), "{:?}", saved(&w));
}

#[test]
fn values_are_saved_normalized() {
    let mut w = world(vec![], Protocol::Uci);
    open(&mut w, full());
    set(&mut w.app, "Hash", " 0064 ");
    set(&mut w.app, "OwnBook", "TRUE");
    opts(&mut w.app, OptionsMessage::Save);
    assert_eq!(
        saved(&w),
        [
            ("Hash".to_owned(), "64".to_owned()),
            ("OwnBook".to_owned(), "true".to_owned())
        ]
    );
}

#[test]
fn saved_options_the_engine_did_not_announce_this_time_are_kept() {
    let mut w = world(
        vec![
            ("Gone".to_owned(), "x".to_owned()),
            ("Hash".to_owned(), "32".to_owned()),
        ],
        Protocol::Uci,
    );
    open(&mut w, full());
    opts(&mut w.app, OptionsMessage::Save);
    let kept = saved(&w);
    assert!(
        kept.contains(&("Gone".to_owned(), "x".to_owned())),
        "{kept:?}"
    );
    assert!(
        kept.contains(&("Hash".to_owned(), "32".to_owned())),
        "{kept:?}"
    );
}

#[test]
fn a_button_is_never_saved() {
    let mut w = world(vec![], Protocol::Uci);
    open(&mut w, full());
    opts(&mut w.app, OptionsMessage::Save);
    assert!(saved(&w).iter().all(|(n, _)| n != "Clear Hash"));
    // Not even if a stale value is pushed into the form.
    open(&mut w, full());
    set(&mut w.app, "Clear Hash", "true");
    opts(&mut w.app, OptionsMessage::Save);
    assert!(
        saved(&w).iter().all(|(n, _)| n != "Clear Hash"),
        "{:?}",
        saved(&w)
    );
}

#[test]
fn save_is_all_or_nothing_and_the_errors_are_per_field() {
    let mut w = world(vec![], Protocol::Uci);
    open(&mut w, full());
    set(&mut w.app, "Hash", "0");
    set(&mut w.app, "UCI_Elo", "abc");
    set(&mut w.app, "Style", "Nope");
    set(&mut w.app, "Syzygy Path", "a\nb");
    set(&mut w.app, "MultiPV", "3"); // valid, but must not be saved either
    opts(&mut w.app, OptionsMessage::Save);
    assert!(
        saved(&w).is_empty(),
        "nothing is written when any field is refused"
    );
    let d = dialog(&w);
    assert!(matches!(
        d.error_of("Hash"),
        Some(OptionError::OutOfRange { min: 1, .. })
    ));
    assert!(matches!(
        d.error_of("UCI_Elo"),
        Some(OptionError::WrongType { .. })
    ));
    assert!(matches!(
        d.error_of("Style"),
        Some(OptionError::NotInList { .. })
    ));
    assert!(matches!(
        d.error_of("Syzygy Path"),
        Some(OptionError::ControlCharacter { .. })
    ));
    assert!(d.error_of("MultiPV").is_none());
    // Fixing a field clears only its own error.
    set(&mut w.app, "Hash", "64");
    assert!(dialog(&w).error_of("Hash").is_none());
    assert!(dialog(&w).error_of("UCI_Elo").is_some());
}

#[rstest]
#[case("Hash", "1", true)]
#[case("Hash", "33554432", true)]
#[case("Hash", "0", false)]
#[case("Hash", "33554433", false)]
#[case("Hash", "-1", false)]
#[case("Hash", "", false)]
#[case("Hash", "1e3", false)]
#[case("Hash", "99999999999999999999999", false)]
#[case("Hash", "١٢٣", false)]
#[case("UCI_Elo", "1319", false)]
#[case("UCI_Elo", "3190", true)]
fn spin_bounds_are_inclusive_and_anything_else_is_refused(
    #[case] name: &str,
    #[case] text: &str,
    #[case] ok: bool,
) {
    let mut w = world(vec![], Protocol::Uci);
    open(&mut w, full());
    set(&mut w.app, name, text);
    opts(&mut w.app, OptionsMessage::Save);
    assert_eq!(dialog_closed(&w), ok, "{name}={text:?}");
}

fn dialog_closed(w: &World) -> bool {
    w.app.engine_dialog.as_ref().unwrap().options.is_none()
}

#[test]
fn reset_puts_every_default_back_in_the_form_but_saves_nothing() {
    let mut w = world(vec![("Hash".to_owned(), "128".to_owned())], Protocol::Uci);
    open(&mut w, full());
    set(&mut w.app, "OwnBook", "true");
    set(&mut w.app, "Hash", "0");
    opts(&mut w.app, OptionsMessage::Save); // refused: leaves an error
    opts(&mut w.app, OptionsMessage::Reset);
    assert_eq!(dialog(&w).value_of("Hash"), Some("16"));
    assert_eq!(dialog(&w).value_of("OwnBook"), Some("false"));
    assert!(dialog(&w).errors.is_empty(), "reset clears the errors");
    assert_eq!(
        saved(&w),
        [("Hash".to_owned(), "128".to_owned())],
        "nothing written yet"
    );
    opts(&mut w.app, OptionsMessage::Save);
    assert!(saved(&w).is_empty(), "Save after Reset stores no override");
}

#[test]
fn close_discards_the_edits() {
    let mut w = world(vec![], Protocol::Uci);
    open(&mut w, full());
    set(&mut w.app, "Hash", "256");
    opts(&mut w.app, OptionsMessage::Close);
    assert!(saved(&w).is_empty());
    assert!(dialog_closed(&w));
}

#[test]
fn a_button_acts_only_on_a_running_engine() {
    let mut w = world(vec![], Protocol::Uci);
    open(&mut w, full());
    let effects = opts(&mut w.app, OptionsMessage::Press("Clear Hash".to_owned()));
    assert!(effects.is_empty(), "no engine running: {effects:?}");
    run_it(&mut w, &full());
    let effects = opts(&mut w.app, OptionsMessage::Press("Clear Hash".to_owned()));
    assert_eq!(commands(&effects).len(), 1, "{effects:?}");
    assert_eq!(commands(&effects)[0].0, "Clear Hash");
}

#[test]
fn pressing_something_that_is_not_a_button_sends_nothing() {
    let mut w = world(vec![], Protocol::Uci);
    run_it(&mut w, &full());
    open(&mut w, full());
    for name in ["Hash", "Unknown option", ""] {
        let effects = opts(&mut w.app, OptionsMessage::Press(name.to_owned()));
        assert!(commands(&effects).is_empty(), "{name:?}");
    }
}

#[test]
fn saving_with_the_engine_running_tells_it_only_what_changed() {
    let mut w = world(vec![], Protocol::Uci);
    run_it(&mut w, &full());
    open(&mut w, full());
    set(&mut w.app, "Hash", "128");
    set(&mut w.app, "Threads", "1"); // unchanged
    let effects = opts(&mut w.app, OptionsMessage::Save);
    assert_eq!(
        commands(&effects),
        [("Hash".to_owned(), OptionValue::Int(128))]
    );
}

#[test]
fn saving_without_changes_sends_nothing_to_the_engine() {
    let mut w = world(vec![], Protocol::Uci);
    run_it(&mut w, &full());
    open(&mut w, full());
    let effects = opts(&mut w.app, OptionsMessage::Save);
    assert!(commands(&effects).is_empty(), "{effects:?}");
}

#[test]
fn going_back_to_the_default_with_the_engine_running_sends_the_default() {
    let mut w = world(vec![("Hash".to_owned(), "128".to_owned())], Protocol::Uci);
    run_it(&mut w, &full());
    open(&mut w, full());
    set(&mut w.app, "Hash", "16");
    let effects = opts(&mut w.app, OptionsMessage::Save);
    assert_eq!(
        commands(&effects),
        [("Hash".to_owned(), OptionValue::Int(16))]
    );
}

#[test]
fn an_idle_engine_is_told_nothing_when_saving() {
    let mut w = world(vec![], Protocol::Uci);
    open(&mut w, full());
    set(&mut w.app, "Hash", "128");
    let effects = opts(&mut w.app, OptionsMessage::Save);
    assert!(commands(&effects).is_empty());
}

#[test]
fn defaults_are_shown_as_text_per_kind() {
    let id = full();
    let get = |n: &str| id.options.iter().find(|o| o.name == n).unwrap();
    assert_eq!(default_text(get("Hash")), "16");
    assert_eq!(default_text(get("OwnBook")), "false");
    assert_eq!(default_text(get("Style")), "Normal");
    assert_eq!(default_text(get("BookFile")), "");
}

#[test]
fn options_with_hostile_names_do_not_break_the_form() {
    let mut w = world(vec![], Protocol::Uci);
    let id = identity(vec![
        spin("{ $x } \u{1F600}", 1, 0, 9),
        spin("", 1, 0, 9),
        spin(&"N".repeat(5000), 1, 0, 9),
    ]);
    open(&mut w, id);
    set(&mut w.app, "{ $x } \u{1F600}", "5");
    opts(&mut w.app, OptionsMessage::Save);
    assert_eq!(
        saved(&w).first().map(|(n, v)| (n.as_str(), v.as_str())),
        Some(("{ $x } \u{1F600}", "5"))
    );
}

#[test]
fn opening_the_options_of_a_deleted_engine_gives_a_notice_and_no_dialog() {
    let mut w = world(vec![], Protocol::Uci);
    let gone = EngineId(999_999);
    app_send(&mut w.app, EngineMessage::OpenOptions(gone));
    assert!(w.app.engine_dialog.as_ref().unwrap().options.is_none());
    assert!(w.app.notice.is_some());
}

#[test]
fn options_messages_without_the_dialog_are_ignored() {
    let mut w = world(vec![], Protocol::Uci);
    for m in [
        OptionsMessage::Save,
        OptionsMessage::Reset,
        OptionsMessage::Close,
        OptionsMessage::Press("x".into()),
    ] {
        assert!(opts(&mut w.app, m).is_empty());
    }
    set(&mut w.app, "Hash", "1");
}
