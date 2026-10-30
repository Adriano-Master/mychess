//! The keys of the analysis panel (spec M4-4, D448, D811): they act only
//! with the pointer over the panel and no text field focused; `Enter` adds the
//! move, `Ctrl/Cmd+Enter` the line, `Space` starts or stops, `1`-`5` fix the
//! number of lines (limited to the engine's maximum). Headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

mod m4_4_gui_common;

use iced::keyboard;
use m4_4_gui_common::*;
use mychess_app::engines::{EngineMessage, Protocol, RunId};
use mychess_gui::keymap::key_input;

fn enter() -> KeyInput {
    named(NamedKey::Enter)
}

fn space() -> KeyInput {
    named(NamedKey::Space)
}

fn mainline_len(rig: &Rig) -> usize {
    rig.game()
        .mainline()
        .filter(|id| *id != rig.game().root())
        .count()
}

fn analyzing(rig: &Rig, run: RunId) -> bool {
    rig.shell.app.engine_runs.get(run).expect("run").analyzing
}

/// A rig with lines (two moves each), the panel open and the pointer on it.
fn hovered() -> (Rig, RunId) {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    three_long_lines(&mut rig, run);
    hover(&mut rig, true);
    (rig, run)
}

// ---- the keys ----

#[test]
fn enter_over_the_panel_adds_the_first_move_of_the_best_line() {
    let (mut rig, _run) = hovered();
    rig.press(enter());
    assert_eq!(mainline_len(&rig), 1);
    assert_eq!(root_children_ucis(&rig), ["e2e4"]);
}

#[test]
fn ctrl_or_cmd_enter_over_the_panel_adds_the_whole_line() {
    let (mut rig, _run) = hovered();
    rig.press(command(Key::Named(NamedKey::Enter)));
    // D793: an empty game takes the line as the main line.
    assert_eq!(mainline_len(&rig), 2, "1.e4 e5");
}

#[test]
fn shift_or_alt_with_enter_is_not_the_panels() {
    let (mut rig, _run) = hovered();
    for modifiers in [
        Modifiers {
            command: false,
            shift: true,
            alt: false,
        },
        Modifiers {
            command: false,
            shift: false,
            alt: true,
        },
        Modifiers {
            command: true,
            shift: true,
            alt: false,
        },
        Modifiers {
            command: true,
            shift: false,
            alt: true,
        },
    ] {
        rig.press(KeyInput {
            key: Key::Named(NamedKey::Enter),
            modifiers,
        });
        assert_eq!(mainline_len(&rig), 0, "{modifiers:?}");
    }
}

#[test]
fn space_over_the_panel_starts_and_stops_the_analysis() {
    let (mut rig, run) = hovered();
    assert!(analyzing(&rig, run));
    rig.press(space());
    assert!(!analyzing(&rig, run));
    rig.press(space());
    assert!(analyzing(&rig, run));
}

#[test]
fn digits_one_to_five_fix_the_number_of_lines() {
    let (mut rig, run) = hovered();
    for n in 1..=5u8 {
        rig.press(ch(char::from(b'0' + n)));
        assert_eq!(view(&rig, run).multipv, n, "key {n}");
    }
}

#[test]
fn other_digits_are_not_the_panels() {
    let (mut rig, run) = hovered();
    rig.press(ch('3'));
    assert_eq!(view(&rig, run).multipv, 3);
    for c in ['0', '6', '7', '8', '9'] {
        rig.press(ch(c));
        assert_eq!(view(&rig, run).multipv, 3, "key {c}");
    }
}

#[test]
fn a_digit_above_the_engines_maximum_is_limited_to_it() {
    let mut rig = Rig::new();
    let id = add_engine(&mut rig, "Small", Protocol::Uci);
    let run = start_with(&mut rig, id, identity("Small", Some(1..=2), true));
    hover(&mut rig, true);
    rig.press(ch('5'));
    assert_eq!(view(&rig, run).multipv, 2);
    rig.press(ch('4'));
    assert_eq!(view(&rig, run).multipv, 2);
    rig.press(ch('1'));
    assert_eq!(view(&rig, run).multipv, 1);
}

#[test]
fn a_digit_for_an_engine_without_multipv_changes_nothing_and_does_not_panic() {
    let mut rig = Rig::new();
    let id = add_engine(&mut rig, "Plain", Protocol::Uci);
    let run = start_with(&mut rig, id, identity("Plain", None, true));
    hover(&mut rig, true);
    for c in ['1', '2', '3', '4', '5'] {
        rig.press(ch(c));
    }
    assert_eq!(view(&rig, run).multipv, 1);
}

#[test]
fn digits_with_a_modifier_are_not_the_panels() {
    let (mut rig, run) = hovered();
    rig.press(command(Key::Char('2')));
    assert_ne!(view(&rig, run).multipv, 2);
}

// ---- when the keys do not act ----

#[test]
fn nothing_happens_when_the_pointer_is_not_over_the_panel() {
    let (mut rig, run) = hovered();
    hover(&mut rig, false);
    rig.press(enter());
    rig.press(command(Key::Named(NamedKey::Enter)));
    rig.press(ch('4'));
    assert_eq!(mainline_len(&rig), 0);
    assert_eq!(view(&rig, run).multipv, 3);
}

#[test]
fn space_outside_the_panel_follows_the_central_map_not_the_section() {
    // D811: the pointer outside gives the global `engine-toggle` action,
    // which toggles the main analysis: with a single engine it is its run.
    let (mut rig, run) = hovered();
    hover(&mut rig, false);
    rig.press(space());
    assert!(!analyzing(&rig, run), "the global Space also toggles it");
}

#[test]
fn a_focused_text_field_keeps_every_key() {
    let (mut rig, run) = hovered();
    for input in [
        enter(),
        space(),
        ch('4'),
        command(Key::Named(NamedKey::Enter)),
    ] {
        rig.key(input, true, None);
    }
    assert_eq!(mainline_len(&rig), 0);
    assert_eq!(view(&rig, run).multipv, 3);
    assert!(analyzing(&rig, run));
}

#[test]
fn typing_in_the_exclusion_box_does_not_trigger_the_panels_keys() {
    let (mut rig, run) = hovered();
    rig.msg(aui(AnalysisUiMessage::OpenExclude(run)));
    // The widget captures the keys it uses for text: the subscription then
    // reports `text_focused`.
    for c in ['1', '2', 'e', '4'] {
        rig.key(ch(c), true, Some(c));
    }
    assert_eq!(view(&rig, run).multipv, 3, "digits typed in the box");
    assert!(analyzing(&rig, run));
}

#[test]
fn without_a_running_engine_enter_and_digits_stay_with_the_move_entry() {
    let mut rig = Rig::new();
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Analysis)));
    hover(&mut rig, true);
    // M2-1: typing a move and Enter plays it. The panel must not take the
    // `4` (it would be "4 lines") nor the Enter.
    rig.press(ch('e'));
    rig.press(ch('4'));
    rig.press(enter());
    assert_eq!(
        root_children_ucis(&rig),
        ["e2e4"],
        "the move entry got both keys"
    );
}

#[test]
fn a_closed_panel_never_takes_keys() {
    let (mut rig, run) = hovered();
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Analysis)));
    assert!(!rig.shell.ui.is_open(PaneKind::Analysis));
    rig.press(enter());
    rig.press(ch('2'));
    assert_eq!(mainline_len(&rig), 0);
    assert_ne!(view(&rig, run).multipv, 2);
}

#[test]
fn the_keys_act_on_the_section_used_last_and_on_the_first_otherwise() {
    let mut rig = Rig::new();
    let a = add_engine(&mut rig, "Alpha", Protocol::Uci);
    let b = add_engine(&mut rig, "Beta", Protocol::Uci);
    let run_a = start_with(&mut rig, a, identity("Alpha", Some(1..=5), true));
    let run_b = start_with(&mut rig, b, identity("Beta", Some(1..=5), true));
    hover(&mut rig, true);
    // The first section by default... (a button of Beta's would focus it).
    rig.shell.ui.analysis.focus = None;
    rig.press(ch('3'));
    assert_eq!(view(&rig, run_a).multipv, 3);
    assert_eq!(view(&rig, run_b).multipv, 1);
    // A message of Beta's section makes it the target.
    rig.msg(am(AnalysisMessage::ToggleInfo(run_b)));
    rig.press(ch('2'));
    assert_eq!(view(&rig, run_b).multipv, 2);
    assert_eq!(view(&rig, run_a).multipv, 3);
}

// ---- modals ----

#[test]
fn the_keys_do_not_leak_through_the_about_box() {
    let (mut rig, run) = hovered();
    rig.msg(Message::Ui(UiMessage::ShowAbout(true)));
    rig.press(enter());
    rig.press(ch('4'));
    rig.press(space());
    assert_eq!(
        mainline_len(&rig),
        0,
        "Enter behind the About box added a move"
    );
    assert_eq!(view(&rig, run).multipv, 3, "a digit behind the About box");
    assert!(analyzing(&rig, run), "Space behind the About box");
}

#[test]
fn the_keys_do_not_leak_through_the_engines_dialog() {
    let (mut rig, run) = hovered();
    rig.app_msg(AppMessage::Engine(EngineMessage::OpenDialog));
    assert!(rig.shell.app.engine_dialog.is_some());
    rig.press(ch('4'));
    rig.press(space());
    assert_eq!(view(&rig, run).multipv, 3);
    assert!(analyzing(&rig, run));
}

#[test]
fn the_keys_do_not_leak_through_the_replace_question() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    // 1.e4 e5 on the board, back to the start, lines say 1.d4.
    for u in ["e2e4", "e7e5"] {
        let mv = rig.shell.app.session.position().parse_uci(u).unwrap();
        rig.app_msg(AppMessage::Game(mychess_app::GameMessage::PlayMove {
            mv,
            options: mychess_app::PlayOptions::default(),
        }));
    }
    rig.app_msg(AppMessage::Game(mychess_app::GameMessage::Navigate(
        mychess_app::Nav::Start,
    )));
    started(&mut rig, run, 3);
    info(
        &mut rig,
        run,
        report(&Position::new(), 3, 1, 12, 20, &["d2d4"]),
    );
    hover(&mut rig, true);
    rig.press(enter());
    assert!(
        rig.shell.app.session.prompt().is_some(),
        "Enter on the panel asks whether to replace"
    );
    // A second Enter now belongs to the question, not to a second AddMove.
    rig.press(ch('4'));
    assert_eq!(view(&rig, run).multipv, 1, "digit while a question is open");
}

// ---- Modifiers::command() (multiplatform) ----

#[test]
fn the_command_modifier_is_ctrl_on_linux_and_windows_and_cmd_on_macos() {
    let enter_key = keyboard::Key::Named(keyboard::key::Named::Enter);
    let with = key_input(&enter_key, keyboard::Modifiers::COMMAND).expect("key");
    assert!(with.modifiers.command, "iced's COMMAND is the platform's");
    #[cfg(not(target_os = "macos"))]
    {
        let ctrl = key_input(&enter_key, keyboard::Modifiers::CTRL).expect("key");
        assert!(ctrl.modifiers.command, "Ctrl is the command key here");
        let logo = key_input(&enter_key, keyboard::Modifiers::LOGO).expect("key");
        assert!(!logo.modifiers.command, "the Windows/Super key is not");
    }
    #[cfg(target_os = "macos")]
    {
        let logo = key_input(&enter_key, keyboard::Modifiers::LOGO).expect("key");
        assert!(logo.modifiers.command, "Cmd is the command key on macOS");
        let ctrl = key_input(&enter_key, keyboard::Modifiers::CTRL).expect("key");
        assert!(!ctrl.modifiers.command, "Ctrl alone is not");
    }
    let plain = key_input(&enter_key, keyboard::Modifiers::empty()).expect("key");
    assert_eq!(plain.modifiers, Modifiers::NONE);
}

#[test]
fn the_key_function_follows_the_spec_table_for_every_digit_and_enter() {
    use mychess_gui::windows::analysis::key_message;
    let run = RunId(1);
    let k = |key, modifiers| KeyInput { key, modifiers };
    assert_eq!(
        key_message(&k(Key::Named(NamedKey::Enter), Modifiers::NONE), run, None),
        Some(AnalysisMessage::AddMove(run))
    );
    assert_eq!(
        key_message(
            &k(Key::Named(NamedKey::Enter), Modifiers::COMMAND),
            run,
            None
        ),
        Some(AnalysisMessage::AddLine(run))
    );
    assert_eq!(
        key_message(&k(Key::Named(NamedKey::Space), Modifiers::NONE), run, None),
        Some(AnalysisMessage::Toggle(run))
    );
    for (c, n) in [('1', 1u8), ('2', 2), ('3', 3), ('4', 4), ('5', 5)] {
        assert_eq!(
            key_message(&k(Key::Char(c), Modifiers::NONE), run, Some(8)),
            Some(AnalysisMessage::SetMultiPv(run, n))
        );
    }
    // Escape does not close the panel (D448): not a key of the panel at all.
    assert_eq!(
        key_message(&k(Key::Named(NamedKey::Escape), Modifiers::NONE), run, None),
        None
    );
}

#[test]
fn escape_does_not_close_the_panel() {
    let (mut rig, _run) = hovered();
    rig.press(named(NamedKey::Escape));
    assert!(rig.shell.ui.is_open(PaneKind::Analysis));
    assert_eq!(
        rig.shell.app.engine_runs().len(),
        1,
        "the engine keeps running"
    );
}
