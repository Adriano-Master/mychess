//! The UCI options dialog through the real view (spec M4-3, D430, D740-D743;
//! Scid `uci.tcl` 761-835). The pure rules are in
//! `mychess-app/tests/m4_3_options_flow.rs`. Headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

mod m4_3_gui_common;

use m4_3_gui_common::*;
use mychess_app::Effect;
use mychess_app::engines::{
    Capabilities, EngineFailure, EngineId, EngineIdentity, EngineMessage, EngineOption,
    FailureKind, OptionKind, OptionsMessage, Protocol,
};
use mychess_gui::Message;

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

fn kinds() -> EngineIdentity {
    identity(vec![
        opt("Own Flag", OptionKind::Check { default: false }),
        spin("Hash", 16, 1, 1024),
        opt(
            "Style",
            OptionKind::Combo {
                default: "Normal".to_owned(),
                vars: vec!["Solid".to_owned(), "Normal".to_owned()],
            },
        ),
        opt(
            "Syzygy Path",
            OptionKind::String {
                default: "tb".to_owned(),
            },
        ),
        opt("Clear Hash", OptionKind::Button),
        opt("Ponder", OptionKind::Check { default: false }),
    ])
}

/// A rig whose engine 0 has its options dialog open with `identity`.
fn open_with(identity: EngineIdentity) -> (Rig, EngineId) {
    let (mut rig, ids) = rig_with(&[("Fake", "/bin/fake")]);
    let effects = update(&mut rig, EngineMessage::OpenOptions(ids[0]));
    let request = effects
        .iter()
        .find_map(|e| match e {
            Effect::TestEngine { request, .. } => Some(*request),
            _ => None,
        })
        .expect("a probe");
    send(
        &mut rig,
        EngineMessage::Tested {
            request,
            result: Box::new(Ok(identity)),
        },
    );
    (rig, ids[0])
}

fn om(messages: &[Message]) -> Vec<OptionsMessage> {
    engine_msgs(messages)
        .into_iter()
        .filter_map(|m| match m {
            EngineMessage::Options(o) => Some(o),
            _ => None,
        })
        .collect()
}

fn set_msg(name: &str, value: &str) -> OptionsMessage {
    OptionsMessage::Set {
        name: name.to_owned(),
        value: value.to_owned(),
    }
}

fn running(rig: &mut Rig, id: EngineId, identity: &EngineIdentity) {
    send(
        rig,
        EngineMessage::Start(id, mychess_app::engines::StartMode::Analyze),
    );
    let run = rig.shell.app.engine_runs.of_engine(id).unwrap().id;
    send(
        rig,
        EngineMessage::Started {
            run,
            identity: Box::new(identity.clone()),
            at: 1,
        },
    );
}

#[test]
fn each_kind_gets_its_control_with_the_default_beside_the_label() {
    let (rig, _) = open_with(kinds());
    for text in [
        "Own Flag",
        "Hash memory (MB)",
        "Style",
        "Syzygy Path",
        "Clear Hash",
        "default: false",
        "default: 16",
        "default: Normal",
        "-",
        "+",
        "Restore defaults",
        "Save",
        "Cancel",
        "UCI options: Fake",
    ] {
        assert!(has(&rig, text), "{text}");
    }
    assert!(!has(&rig, "Ponder"), "Ponder is managed by the interface");
}

#[test]
fn a_string_option_with_a_non_empty_default_shows_it_beside_the_label() {
    // D430 / Scid `uci.tcl:768-772`: the default goes beside the label of
    // every type when it is not empty.
    let (rig, _) = open_with(kinds());
    assert!(has(&rig, "default: tb"));
}

#[test]
fn only_the_five_options_of_the_scid_have_a_translated_label() {
    let (rig, _) = open_with(identity(vec![
        spin("MultiPV", 1, 1, 500),
        spin("Hash", 16, 1, 1024),
        opt("OwnBook", OptionKind::Check { default: false }),
        opt(
            "BookFile",
            OptionKind::String {
                default: String::new(),
            },
        ),
        opt("UCI_LimitStrength", OptionKind::Check { default: false }),
        spin("UCI_Elo", 1320, 1320, 3190),
        spin("Skill Level", 20, 0, 20),
    ]));
    for text in [
        "Number of lines (MultiPV)",
        "Hash memory (MB)",
        "Use the engine's own book",
        "Book file",
        "Limit strength",
        "UCI_Elo",
        "Skill Level",
    ] {
        assert!(has(&rig, text), "{text}");
    }
    for key in [
        "engines-option-multipv",
        "engines-option-hash",
        "engines-option-ownbook",
        "engines-option-bookfile",
        "engines-option-limitstrength",
    ] {
        assert_ne!(pt().tr(key), key, "{key} in pt-BR");
    }
}

#[test]
fn the_form_lists_the_important_options_first() {
    let (rig, _) = open_with(identity(vec![
        spin("Zed", 1, 0, 9),
        spin("Hash", 16, 1, 1024),
        spin("MultiPV", 1, 1, 500),
    ]));
    let names: Vec<String> = rig
        .shell
        .app
        .engine_dialog
        .as_ref()
        .unwrap()
        .options
        .as_ref()
        .unwrap()
        .visible_options()
        .iter()
        .map(|o| o.name.clone())
        .collect();
    assert_eq!(names, ["MultiPV", "Hash", "Zed"]);
    // Drawn in that order: the bounds of the labels grow downwards.
    let mut s = sim(&rig);
    let y = |s: &mut iced_test::Simulator<'_, Message>, t: &str| {
        s.find(t).unwrap().visible_bounds().unwrap().y
    };
    let rows = [
        y(&mut s, "Number of lines (MultiPV)"),
        y(&mut s, "Hash memory (MB)"),
        y(&mut s, "Zed"),
    ];
    assert!(rows[0] < rows[1] && rows[1] < rows[2], "{rows:?}");
}

#[test]
fn the_plus_and_minus_buttons_move_a_spin_by_its_step_inside_the_range() {
    for (name, min, max, default, step) in [
        ("Hash", 1, 1024, 16, 50),
        ("Small", 0, 10, 5, 1),
        ("Mid", 0, 200, 100, 10),
        ("MultiPV", 1, 500, 1, 1),
        ("Threads", 1, 1024, 8, 1),
        ("Neg", -100, 100, 0, 10),
    ] {
        let (rig, _) = open_with(identity(vec![spin(name, default, min, max)]));
        assert_eq!(
            om(&clicks(&rig, "+")),
            [set_msg(name, &(default + step).min(max).to_string())],
            "{name} +"
        );
        assert_eq!(
            om(&clicks(&rig, "-")),
            [set_msg(name, &(default - step).max(min).to_string())],
            "{name} -"
        );
    }
}

#[test]
fn stepping_never_leaves_the_range() {
    let (rig, _) = open_with(identity(vec![spin("Hash", 1020, 1, 1024)]));
    assert_eq!(om(&clicks(&rig, "+")), [set_msg("Hash", "1024")]);
    let (rig, _) = open_with(identity(vec![spin("Hash", 1, 1, 1024)]));
    assert_eq!(om(&clicks(&rig, "-")), [set_msg("Hash", "1")]);
    // A refused text steps from the default.
    let (mut rig, _) = open_with(identity(vec![spin("Hash", 16, 1, 1024)]));
    send(&mut rig, EngineMessage::Options(set_msg("Hash", "junk")));
    assert_eq!(om(&clicks(&rig, "+")), [set_msg("Hash", "66")]);
}

#[test]
fn the_check_box_toggles_between_true_and_false() {
    let (mut rig, _) = open_with(kinds());
    assert_eq!(om(&clicks(&rig, "Own Flag")), [set_msg("Own Flag", "true")]);
    send(
        &mut rig,
        EngineMessage::Options(set_msg("Own Flag", "true")),
    );
    assert_eq!(
        om(&clicks(&rig, "Own Flag")),
        [set_msg("Own Flag", "false")]
    );
}

#[test]
fn a_range_error_appears_beside_its_field_and_keeps_the_dialog_open() {
    let (mut rig, _) = open_with(kinds());
    send(&mut rig, EngineMessage::Options(set_msg("Hash", "0")));
    click(&mut rig, "Save");
    assert!(has(&rig, "The value must be between 1 and 1024."));
    assert!(
        rig.shell
            .app
            .engine_dialog
            .as_ref()
            .unwrap()
            .options
            .is_some()
    );
    // Typing again clears it.
    send(&mut rig, EngineMessage::Options(set_msg("Hash", "64")));
    assert!(!has(&rig, "The value must be between 1 and 1024."));
}

#[test]
fn other_field_errors_have_their_own_messages() {
    let (mut rig, _) = open_with(kinds());
    send(&mut rig, EngineMessage::Options(set_msg("Style", "Nope")));
    send(
        &mut rig,
        EngineMessage::Options(set_msg("Syzygy Path", "a\nb")),
    );
    click(&mut rig, "Save");
    assert!(has(&rig, "The value is not one of the choices."));
    assert!(has(&rig, "The text must not have line breaks."));
    send(
        &mut rig,
        EngineMessage::Options(set_msg("Own Flag", "maybe")),
    );
    click(&mut rig, "Save");
    assert!(has(&rig, "The value has the wrong type."));
}

#[test]
fn a_button_is_off_without_a_running_engine_and_says_why() {
    let (rig, _) = open_with(kinds());
    assert!(clicks(&rig, "Clear Hash").is_empty());
    assert!(has(&rig, "Buttons act only while the engine is running."));
}

#[test]
fn a_button_works_with_the_engine_running_and_is_not_saved() {
    let (mut rig, id) = open_with(kinds());
    send(&mut rig, EngineMessage::Options(OptionsMessage::Close));
    running(&mut rig, id, &kinds());
    send(&mut rig, EngineMessage::OpenOptions(id));
    assert!(!has(&rig, "Buttons act only while the engine is running."));
    assert_eq!(
        om(&clicks(&rig, "Clear Hash")),
        [OptionsMessage::Press("Clear Hash".to_owned())]
    );
    click(&mut rig, "Save");
    assert!(rig.shell.app.engines.get(id).unwrap().options.is_empty());
}

#[test]
fn restore_defaults_changes_the_form_and_save_then_stores_nothing() {
    let (mut rig, id) = open_with(kinds());
    send(&mut rig, EngineMessage::Options(set_msg("Hash", "300")));
    send(&mut rig, EngineMessage::Options(set_msg("Style", "Solid")));
    click(&mut rig, "Restore defaults");
    let d = rig
        .shell
        .app
        .engine_dialog
        .as_ref()
        .unwrap()
        .options
        .as_ref()
        .unwrap();
    assert_eq!(d.value_of("Hash"), Some("16"));
    assert_eq!(d.value_of("Style"), Some("Normal"));
    click(&mut rig, "Save");
    assert!(rig.shell.app.engines.get(id).unwrap().options.is_empty());
}

#[test]
fn save_writes_only_what_differs_and_keeps_saved_values_of_unannounced_options() {
    let (mut rig, ids) = rig_with(&[("Fake", "/bin/fake")]);
    let mut draft = rig.shell.app.engines.get(ids[0]).unwrap().to_draft();
    draft.options = vec![("Removed In New Version".to_owned(), "x".to_owned())];
    rig.shell.app.engines.edit(ids[0], draft).unwrap();
    let request = match &update(&mut rig, EngineMessage::OpenOptions(ids[0]))[..] {
        [Effect::TestEngine { request, .. }] => *request,
        other => panic!("{other:?}"),
    };
    send(
        &mut rig,
        EngineMessage::Tested {
            request,
            result: Box::new(Ok(kinds())),
        },
    );
    send(&mut rig, EngineMessage::Options(set_msg("Hash", "128")));
    click(&mut rig, "Save");
    assert_eq!(
        rig.shell.app.engines.get(ids[0]).unwrap().options,
        [
            ("Hash".to_owned(), "128".to_owned()),
            ("Removed In New Version".to_owned(), "x".to_owned())
        ]
    );
    assert!(
        rig.shell
            .app
            .engine_dialog
            .as_ref()
            .unwrap()
            .options
            .is_none()
    );
}

#[test]
fn escape_closes_only_the_options_and_enter_saves_them() {
    let (mut rig, id) = open_with(kinds());
    send(&mut rig, EngineMessage::Edit(id));
    // The options are over the editor, which is over the list.
    send(&mut rig, EngineMessage::OpenOptions(id));
    rig.press(nk(NamedKey::Escape));
    assert!(
        rig.shell
            .app
            .engine_dialog
            .as_ref()
            .unwrap()
            .options
            .is_none()
    );
    assert!(
        rig.shell.app.engine_editor.is_some(),
        "the editor is still there"
    );
    assert!(rig.shell.app.engine_dialog.is_some());
}

#[test]
fn enter_saves_the_options() {
    let (mut rig, id) = open_with(kinds());
    send(&mut rig, EngineMessage::Options(set_msg("Hash", "128")));
    rig.press(nk(NamedKey::Enter));
    assert_eq!(
        rig.shell.app.engines.get(id).unwrap().options,
        [("Hash".to_owned(), "128".to_owned())]
    );
}

#[test]
fn enter_while_the_options_are_still_being_read_neither_closes_nor_saves() {
    // D754: the engine is not running, so the dialog waits for the probe.
    let (mut rig, ids) = rig_with(&[("Fish", "/fish")]);
    send(&mut rig, EngineMessage::OpenOptions(ids[0]));
    assert_eq!(
        rig.shell
            .app
            .engine_dialog
            .as_ref()
            .unwrap()
            .options
            .as_ref()
            .unwrap()
            .status,
        mychess_app::engines::OptionsStatus::Probing
    );
    rig.press(nk(NamedKey::Enter));
    assert!(
        rig.shell
            .app
            .engine_dialog
            .as_ref()
            .unwrap()
            .options
            .is_some(),
        "the dialog stays open"
    );
}

#[test]
fn xboard_engines_say_they_have_no_options_and_the_list_button_is_off() {
    let (mut rig, ids) = rig_with(&[("Xb", "/xb")]);
    let mut draft = rig.shell.app.engines.get(ids[0]).unwrap().to_draft();
    draft.protocol = Protocol::XBoard;
    rig.shell.app.engines.edit(ids[0], draft).unwrap();
    assert!(clicks(&rig, "UCI options...").is_empty());
    send(&mut rig, EngineMessage::OpenOptions(ids[0]));
    assert!(has(&rig, "Only UCI engines have options."));
    assert!(clicks(&rig, "Save").is_empty());
}

#[test]
fn probing_failing_and_empty_states_have_their_own_text_and_no_save() {
    let (mut rig, ids) = rig_with(&[("Fake", "/bin/fake")]);
    let effects = update(&mut rig, EngineMessage::OpenOptions(ids[0]));
    assert!(has(&rig, "Starting the engine to read its options..."));
    assert!(clicks(&rig, "Save").is_empty());
    let Some(Effect::TestEngine { request, .. }) = effects.into_iter().next() else {
        panic!()
    };
    let f = EngineFailure {
        kind: FailureKind::Timeout(mychess_app::engines::Phase::Uciok),
        exit_code: None,
        stderr_tail: vec!["boom".to_owned()],
    };
    send(
        &mut rig,
        EngineMessage::Tested {
            request,
            result: Box::new(Err(f)),
        },
    );
    assert!(has(
        &rig,
        "The engine could not be started: The engine did not answer in time. The engine said: boom"
    ));
    assert!(clicks(&rig, "Save").is_empty());
    let (rig, _) = open_with(identity(vec![]));
    assert!(has(&rig, "The engine has no options."));
}

#[test]
fn hostile_option_names_and_a_long_list_draw_without_trouble() {
    let mut options: Vec<_> = (0..300)
        .map(|n| spin(&format!("Opt {n}"), 1, 0, 9))
        .collect();
    options.push(spin("{ $x } -term \u{1F600}", 1, 0, 9));
    options.push(spin(&"L".repeat(3000), 1, 0, 9));
    let (rig, _) = open_with(identity(options));
    assert!(has(&rig, "Opt 0"));
    assert!(has(&rig, "{ $x } -term \u{1F600}"));
}

#[test]
fn the_options_opened_from_the_editor_are_for_that_engine() {
    let (mut rig, ids) = rig_with(&[("A", "/a"), ("B", "/b")]);
    send(&mut rig, EngineMessage::Edit(ids[1]));
    let sent = engine_msgs(&clicks(&rig, "UCI options..."));
    assert_eq!(sent, [EngineMessage::OpenOptions(ids[1])]);
}
