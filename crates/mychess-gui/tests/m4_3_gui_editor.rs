//! The engine editor from the specification of M4-3 ("Comportamento
//! observavel" 2; rules 4, 7, 8; D429, D725; Scid `analysis.tcl` `edit`
//! 531-764). Headless, through the real view.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

mod m4_3_gui_common;

use m4_3_gui_common::*;
use mychess_app::engines::{
    CommandWarning, EditorMessage as E, EngineDraft, EngineFailure, EngineIdentity, EngineMessage,
    EngineNotice, FailureKind, Hotkey, Protocol,
};
use mychess_app::{AppNotice, Effect};
use mychess_gui::windows::engine_editor::EDITOR_NAME_ID;

/// A rig with the editor open on a new engine.
fn new_editor() -> Rig {
    let (mut rig, _) = rig_with(&[]);
    send(&mut rig, EngineMessage::New);
    rig
}

fn fill(rig: &mut Rig, name: &str, command: &str) {
    editor(rig, E::SetName(name.to_owned()));
    editor(rig, E::SetCommand(command.to_owned()));
}

fn save(rig: &mut Rig) {
    editor(rig, E::Save);
}

fn notice(rig: &Rig) -> Option<AppNotice> {
    rig.shell
        .ui
        .status_notice
        .clone()
        .or_else(|| rig.shell.app.notice.clone())
}

fn identity(options: usize) -> EngineIdentity {
    EngineIdentity {
        name: "Fake 1.0".to_owned(),
        author: None,
        options: (0..options)
            .map(|n| mychess_app::engines::EngineOption {
                name: format!("O{n}"),
                kind: mychess_app::engines::OptionKind::Button,
            })
            .collect(),
        capabilities: mychess_app::engines::Capabilities::default(),
    }
}

// ---- validation ----

#[test]
fn saving_without_a_name_shows_the_error_and_keeps_the_editor_open() {
    let mut rig = new_editor();
    editor(&mut rig, E::SetCommand("/bin/x".to_owned()));
    click(&mut rig, "Save");
    assert!(has(&rig, "The name must not be empty."));
    assert!(rig.shell.app.engine_editor.is_some());
    assert!(rig.shell.app.engines.is_empty());
}

#[test]
fn saving_without_a_command_shows_the_error() {
    let mut rig = new_editor();
    editor(&mut rig, E::SetName("Fish".to_owned()));
    click(&mut rig, "Save");
    assert!(has(&rig, "The command must not be empty."));
    assert!(rig.shell.app.engines.is_empty());
}

#[test]
fn a_name_of_only_spaces_counts_as_empty_and_names_are_trimmed() {
    let mut rig = new_editor();
    fill(&mut rig, "   \t ", "/bin/x");
    save(&mut rig);
    assert!(has(&rig, "The name must not be empty."));
    editor(&mut rig, E::SetName("  Fish  ".to_owned()));
    save(&mut rig);
    assert_eq!(names(&rig), ["Fish"]);
}

#[test]
fn an_error_goes_away_when_the_user_edits_a_field() {
    let mut rig = new_editor();
    save(&mut rig);
    assert!(has(&rig, "The name must not be empty."));
    editor(&mut rig, E::SetName("F".to_owned()));
    assert!(!has(&rig, "The name must not be empty."));
}

#[test]
fn elo_is_accepted_from_0_to_4000_and_zero_or_empty_means_unknown() {
    for (text, expected) in [
        ("0", None),
        ("", None),
        ("1", Some(1)),
        ("4000", Some(4000)),
        ("  2850 ", Some(2850)),
    ] {
        let mut rig = new_editor();
        fill(&mut rig, "Fish", "/bin/x");
        editor(&mut rig, E::SetElo(text.to_owned()));
        save(&mut rig);
        assert_eq!(
            rig.shell.app.engines.engines().first().map(|e| e.elo),
            Some(expected),
            "Elo {text:?}"
        );
    }
}

#[test]
fn elo_out_of_range_or_not_a_number_is_refused_with_a_message() {
    for (text, message) in [
        ("4001", "The Elo must be between 0 and 4000."),
        ("99999", "The Elo must be a number."),
        ("abc", "The Elo must be a number."),
        ("-1", "The Elo must be a number."),
        ("12.5", "The Elo must be a number."),
        ("３０００", "The Elo must be a number."),
    ] {
        let mut rig = new_editor();
        fill(&mut rig, "Fish", "/bin/x");
        editor(&mut rig, E::SetElo(text.to_owned()));
        save(&mut rig);
        assert!(rig.shell.app.engines.is_empty(), "Elo {text:?} saved");
        assert!(has(&rig, message), "Elo {text:?}: {message}");
    }
}

#[test]
fn arguments_are_split_with_quotes_and_never_by_a_shell() {
    let mut rig = new_editor();
    fill(&mut rig, "Fish", "/bin/x");
    editor(
        &mut rig,
        E::SetArgs("-a \"b c\" 'd e' $HOME `id` ; rm".to_owned()),
    );
    save(&mut rig);
    let args = &rig.shell.app.engines.engines()[0].args;
    assert_eq!(args, &["-a", "b c", "d e", "$HOME", "`id`", ";", "rm"]);
}

#[test]
fn an_unclosed_quote_is_a_field_error_not_a_panic() {
    for text in ["\"abc", "'abc", "a \"b c", "\\"] {
        let mut rig = new_editor();
        fill(&mut rig, "Fish", "/bin/x");
        editor(&mut rig, E::SetArgs(text.to_owned()));
        save(&mut rig);
        if rig.shell.app.engines.is_empty() {
            assert!(
                has(&rig, "The parameters have a quote that is not closed."),
                "{text:?}"
            );
        }
    }
    let mut rig = new_editor();
    fill(&mut rig, "Fish", "/bin/x");
    editor(&mut rig, E::SetArgs("\"abc".to_owned()));
    save(&mut rig);
    assert!(rig.shell.app.engines.is_empty());
    assert!(has(&rig, "The parameters have a quote that is not closed."));
}

#[test]
fn empty_arguments_are_an_empty_list() {
    let mut rig = new_editor();
    fill(&mut rig, "Fish", "/bin/x");
    editor(&mut rig, E::SetArgs("   ".to_owned()));
    save(&mut rig);
    assert!(rig.shell.app.engines.engines()[0].args.is_empty());
}

#[test]
fn an_argument_with_spaces_comes_back_quoted_in_the_form_and_survives_a_save() {
    let (mut rig, ids) = rig_with(&[("Fish", "/bin/fish")]);
    let mut draft = rig.shell.app.engines.get(ids[0]).unwrap().to_draft();
    draft.args = vec![
        "--book".to_owned(),
        "my book.bin".to_owned(),
        "-x".to_owned(),
    ];
    rig.shell.app.engines.edit(ids[0], draft).unwrap();
    send(&mut rig, EngineMessage::Edit(ids[0]));
    let shown = rig.shell.app.engine_editor.as_ref().unwrap().args.clone();
    assert!(shown.contains('"') || shown.contains('\''), "{shown}");
    save(&mut rig);
    assert_eq!(
        rig.shell.app.engines.get(ids[0]).unwrap().args,
        ["--book", "my book.bin", "-x"]
    );
}

#[test]
fn a_command_and_folder_with_spaces_and_unicode_are_stored_as_typed() {
    let mut rig = new_editor();
    fill(&mut rig, "Fish", "/opt/Meus Programas/xadrez ♞/fish");
    editor(
        &mut rig,
        E::SetWorkingDir("/opt/Meus Programas/xadrez ♞".to_owned()),
    );
    save(&mut rig);
    let e = &rig.shell.app.engines.engines()[0];
    assert_eq!(
        e.command,
        std::path::PathBuf::from("/opt/Meus Programas/xadrez ♞/fish")
    );
    assert_eq!(
        e.working_dir,
        Some(std::path::PathBuf::from("/opt/Meus Programas/xadrez ♞"))
    );
}

#[test]
fn a_name_over_the_limit_is_refused_with_a_message() {
    let mut rig = new_editor();
    fill(&mut rig, &"n".repeat(5000), "/bin/x");
    save(&mut rig);
    assert!(rig.shell.app.engines.is_empty());
    assert!(has(&rig, "A field is too long."));
}

// ---- save, cancel, keys ----

#[test]
fn save_closes_the_editor_adds_the_engine_and_selects_it() {
    let (mut rig, _) = trio();
    send(&mut rig, EngineMessage::New);
    fill(&mut rig, "Fish", "/bin/fish");
    click(&mut rig, "Save");
    assert!(rig.shell.app.engine_editor.is_none());
    assert_eq!(names(&rig).last().map(String::as_str), Some("Fish"));
    assert_eq!(selected(&rig), Some(rig.shell.app.engines.engines()[3].id));
}

#[test]
fn cancel_and_escape_discard_the_changes() {
    let (mut rig, ids) = trio();
    send(&mut rig, EngineMessage::Edit(ids[0]));
    editor(&mut rig, E::SetName("Changed".to_owned()));
    click(&mut rig, "Cancel");
    assert!(rig.shell.app.engine_editor.is_none());
    assert_eq!(names(&rig)[0], "Stockfish");
    send(&mut rig, EngineMessage::Edit(ids[0]));
    editor(&mut rig, E::SetName("Changed".to_owned()));
    rig.press(nk(NamedKey::Escape));
    assert!(rig.shell.app.engine_editor.is_none());
    assert_eq!(names(&rig)[0], "Stockfish");
    assert!(rig.shell.app.engine_dialog.is_some(), "the list stays");
}

#[test]
fn enter_saves_the_form_unless_a_text_field_used_it() {
    let mut rig = new_editor();
    fill(&mut rig, "Fish", "/bin/fish");
    rig.key(nk(NamedKey::Enter), true, None);
    assert!(
        rig.shell.app.engines.is_empty(),
        "a focused field keeps Enter"
    );
    rig.press(nk(NamedKey::Enter));
    assert_eq!(names(&rig), ["Fish"]);
}

#[test]
fn the_editor_is_the_dialog_on_top_of_the_list() {
    let mut rig = new_editor();
    let top =
        mychess_gui::windows::dialog_ui::ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs);
    assert_eq!(
        top,
        Some(mychess_gui::windows::dialog_ui::ActiveDialog::EngineEditor)
    );
    editor(&mut rig, E::Cancel);
    let top =
        mychess_gui::windows::dialog_ui::ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs);
    assert_eq!(
        top,
        Some(mychess_gui::windows::dialog_ui::ActiveDialog::Engines)
    );
}

#[test]
fn typing_in_the_name_field_sends_the_text() {
    let rig = new_editor();
    let mut s = sim(&rig);
    s.click(iced::widget::Id::new(EDITOR_NAME_ID))
        .expect("name field");
    s.typewrite("Fish");
    let sent = engine_msgs(&s.into_messages().collect::<Vec<_>>());
    assert!(
        sent.iter()
            .any(|m| matches!(m, EngineMessage::Editor(E::SetName(t)) if t.ends_with('h'))),
        "{sent:?}"
    );
}

fn click_id(rig: &Rig, id: &'static str) -> Vec<EngineMessage> {
    let mut s = sim(rig);
    s.click(iced::widget::Id::new(id))
        .unwrap_or_else(|e| panic!("{id}: {e:?}"));
    engine_msgs(&s.into_messages().collect::<Vec<_>>())
}

#[test]
fn radio_buttons_send_the_protocol_and_the_key() {
    let rig = new_editor();
    assert_eq!(
        click_id(&rig, "engine-protocol-xboard"),
        [EngineMessage::Editor(E::SetProtocol(Protocol::XBoard))]
    );
    for (id, key) in [
        ("engine-key-f2", Hotkey::F2),
        ("engine-key-f3", Hotkey::F3),
        ("engine-key-f4", Hotkey::F4),
    ] {
        assert_eq!(
            click_id(&rig, id),
            [EngineMessage::Editor(E::SetHotkey(Some(key)))]
        );
    }
    // "None" is only sent when another key is selected (a radio already on
    // does not publish again).
    let mut rig = new_editor();
    editor(&mut rig, E::SetHotkey(Some(Hotkey::F2)));
    assert_eq!(
        click_id(&rig, "engine-key-none"),
        [EngineMessage::Editor(E::SetHotkey(None))]
    );
}

#[test]
fn a_new_engine_on_a_list_without_keys_gets_f2_then_the_second_gets_f3() {
    let mut rig = new_editor();
    fill(&mut rig, "A", "/a");
    save(&mut rig);
    let a = rig.shell.app.engines.engines()[0].id;
    assert_eq!(rig.shell.app.engines.hotkey_of(a), Some(Hotkey::F2));
    send(&mut rig, EngineMessage::New);
    fill(&mut rig, "B", "/b");
    save(&mut rig);
    let b = rig.shell.app.engines.engines()[1].id;
    assert_eq!(rig.shell.app.engines.hotkey_of(b), Some(Hotkey::F3));
}

#[test]
fn choosing_a_key_that_another_engine_has_takes_it_from_that_engine() {
    let (mut rig, ids) = trio();
    send(&mut rig, EngineMessage::SetHotkey(ids[0], Some(Hotkey::F2)));
    send(&mut rig, EngineMessage::Edit(ids[1]));
    editor(&mut rig, E::SetHotkey(Some(Hotkey::F2)));
    save(&mut rig);
    assert_eq!(rig.shell.app.engines.hotkey_owner(Hotkey::F2), Some(ids[1]));
    assert_eq!(rig.shell.app.engines.hotkey_of(ids[0]), None);
}

#[test]
fn choosing_none_frees_the_key() {
    let (mut rig, ids) = trio();
    send(&mut rig, EngineMessage::SetHotkey(ids[0], Some(Hotkey::F2)));
    send(&mut rig, EngineMessage::Edit(ids[0]));
    assert_eq!(
        rig.shell.app.engine_editor.as_ref().unwrap().hotkey,
        Some(Hotkey::F2)
    );
    editor(&mut rig, E::SetHotkey(None));
    save(&mut rig);
    assert_eq!(rig.shell.app.engines.hotkey_of(ids[0]), None);
}

#[test]
fn fields_the_form_does_not_show_are_kept_when_saving_an_edit() {
    let (mut rig, ids) = rig_with(&[("Fish", "/bin/fish")]);
    let mut draft = EngineDraft::new("Fish", "/bin/fish");
    draft.options = vec![
        ("Hash".to_owned(), "256".to_owned()),
        ("Style".to_owned(), "Risky".to_owned()),
    ];
    draft.multipv = 4;
    draft.low_priority = Some(true);
    draft.last_used = Some(1_700_000_000);
    rig.shell.app.engines.edit(ids[0], draft).unwrap();
    send(&mut rig, EngineMessage::Edit(ids[0]));
    editor(&mut rig, E::SetName("Fish 2".to_owned()));
    editor(&mut rig, E::SetElo("3000".to_owned()));
    save(&mut rig);
    let e = rig.shell.app.engines.get(ids[0]).unwrap();
    assert_eq!(e.name, "Fish 2");
    assert_eq!(e.elo, Some(3000));
    assert_eq!(
        e.options,
        [
            ("Hash".to_owned(), "256".to_owned()),
            ("Style".to_owned(), "Risky".to_owned())
        ]
    );
    assert_eq!(e.multipv, 4);
    assert_eq!(e.low_priority, Some(true));
    assert_eq!(e.last_used, Some(1_700_000_000));
}

#[test]
fn editing_the_command_of_a_running_engine_says_to_restart() {
    let (mut rig, ids) = trio();
    start(&mut rig, ids[0]);
    send(&mut rig, EngineMessage::Edit(ids[0]));
    editor(&mut rig, E::SetCommand("/bin/other".to_owned()));
    save(&mut rig);
    assert_eq!(
        notice(&rig),
        Some(AppNotice::Engine(EngineNotice::RestartToApply))
    );
    assert_eq!(
        rig.shell.app.engines.get(ids[0]).unwrap().command,
        std::path::PathBuf::from("/bin/other")
    );
    // Only a name change does not need a restart.
    let (mut rig, ids) = trio();
    start(&mut rig, ids[0]);
    send(&mut rig, EngineMessage::Edit(ids[0]));
    editor(&mut rig, E::SetName("Renamed".to_owned()));
    save(&mut rig);
    assert_ne!(
        notice(&rig),
        Some(AppNotice::Engine(EngineNotice::RestartToApply))
    );
}

#[test]
fn the_options_button_exists_only_when_editing_an_existing_engine() {
    let rig = new_editor();
    assert!(!has(&rig, "UCI options..."));
    let (mut rig, ids) = trio();
    send(&mut rig, EngineMessage::Edit(ids[0]));
    assert_eq!(
        engine_msgs(&clicks(&rig, "UCI options...")),
        [EngineMessage::OpenOptions(ids[0])]
    );
}

// ---- Test engine / Detect protocol ----

fn request_of(effects: &[Effect]) -> mychess_app::RequestId {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::TestEngine { request, .. } | Effect::DetectEngine { request, .. } => {
                Some(*request)
            }
            _ => None,
        })
        .expect("a probe effect")
}

#[test]
fn test_engine_shows_the_name_the_protocol_and_the_option_count() {
    let mut rig = new_editor();
    fill(&mut rig, "Fish", "/bin/fish");
    let effects = update(&mut rig, EngineMessage::Editor(E::Test));
    assert!(has(&rig, "Testing..."));
    let request = request_of(&effects);
    send(
        &mut rig,
        EngineMessage::Tested {
            request,
            result: Box::new(Ok(identity(3))),
        },
    );
    assert!(has(&rig, "OK: Fake 1.0 (UCI, 3 options)"));
}

#[test]
fn a_failed_test_shows_the_translated_reason_with_exit_code_and_stderr() {
    let mut rig = new_editor();
    fill(&mut rig, "Fish", "/bin/fish");
    let request = request_of(&update(&mut rig, EngineMessage::Editor(E::Test)));
    let failure = EngineFailure {
        kind: FailureKind::ExitedEarly,
        exit_code: Some(3),
        stderr_tail: vec!["no such file".to_owned()],
    };
    send(
        &mut rig,
        EngineMessage::Tested {
            request,
            result: Box::new(Err(failure)),
        },
    );
    assert!(has(
        &rig,
        "Test failed: The engine ended before it was ready (exit code 3). The engine said: no such file"
    ));
    // Editing the command forgets a result that no longer describes the form.
    editor(&mut rig, E::SetCommand("/bin/other".to_owned()));
    assert!(!has(
        &rig,
        "Test failed: The engine ended before it was ready (exit code 3). The engine said: no such file"
    ));
}

#[test]
fn a_late_answer_for_an_old_request_is_ignored() {
    let mut rig = new_editor();
    fill(&mut rig, "Fish", "/bin/fish");
    let first = request_of(&update(&mut rig, EngineMessage::Editor(E::Test)));
    let _second = request_of(&update(&mut rig, EngineMessage::Editor(E::Test)));
    send(
        &mut rig,
        EngineMessage::Tested {
            request: first,
            result: Box::new(Ok(identity(1))),
        },
    );
    assert!(has(&rig, "Testing..."));
    assert!(!has(&rig, "OK: Fake 1.0 (UCI, 1 options)"));
}

#[test]
fn testing_without_a_command_asks_for_one_and_starts_nothing() {
    let mut rig = new_editor();
    let effects = update(&mut rig, EngineMessage::Editor(E::Test));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(has(&rig, "The command must not be empty."));
}

#[test]
fn detect_protocol_suggests_and_use_it_applies_the_suggestion() {
    let mut rig = new_editor();
    fill(&mut rig, "Fish", "/bin/fish");
    let request = request_of(&update(&mut rig, EngineMessage::Editor(E::Detect)));
    assert!(has(&rig, "Detecting..."));
    send(
        &mut rig,
        EngineMessage::Detected {
            request,
            result: Ok(Protocol::XBoard),
        },
    );
    assert!(has(&rig, "The engine speaks XBOARD."));
    assert_eq!(
        rig.shell.app.engine_editor.as_ref().unwrap().protocol,
        Protocol::Uci,
        "a suggestion does not switch by itself"
    );
    click(&mut rig, "Use it");
    assert_eq!(
        rig.shell.app.engine_editor.as_ref().unwrap().protocol,
        Protocol::XBoard
    );
    assert!(
        !has(&rig, "Use it")
            || rig
                .shell
                .app
                .engine_editor
                .as_ref()
                .unwrap()
                .detect
                .is_none()
    );
}

#[test]
fn detect_offers_nothing_to_apply_when_the_protocol_already_matches() {
    let mut rig = new_editor();
    fill(&mut rig, "Fish", "/bin/fish");
    let request = request_of(&update(&mut rig, EngineMessage::Editor(E::Detect)));
    send(
        &mut rig,
        EngineMessage::Detected {
            request,
            result: Ok(Protocol::Uci),
        },
    );
    assert!(has(&rig, "The engine speaks UCI."));
    assert!(!has(&rig, "Use it"));
}

#[test]
fn a_failed_detection_names_the_reason() {
    let mut rig = new_editor();
    fill(&mut rig, "Fish", "/bin/fish");
    let request = request_of(&update(&mut rig, EngineMessage::Editor(E::Detect)));
    send(
        &mut rig,
        EngineMessage::Detected {
            request,
            result: Err(failure(FailureKind::NotAnEngine)),
        },
    );
    assert!(has(
        &rig,
        "Detection failed: The file is not an engine, or it speaks neither UCI nor XBoard."
    ));
}

// ---- command check (a warning, never a block) ----

fn check(rig: &mut Rig) {
    rig.send(em(EngineMessage::Editor(E::Check)));
}

#[test]
fn a_command_that_does_not_exist_warns_but_does_not_block_saving() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = new_editor();
    fill(
        &mut rig,
        "Ghost",
        dir.path().join("no such engine").to_str().unwrap(),
    );
    check(&mut rig);
    assert_eq!(
        rig.shell.app.engine_editor.as_ref().unwrap().warnings,
        [CommandWarning::CommandNotFound]
    );
    assert!(has(&rig, "The command was not found on this computer."));
    save(&mut rig);
    assert_eq!(names(&rig), ["Ghost"], "saved despite the warning");
}

#[test]
fn a_missing_folder_warns_and_an_existing_program_in_a_folder_with_spaces_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("my engines");
    std::fs::create_dir(&home).unwrap();
    let exe = home.join("fish");
    std::fs::write(&exe, b"#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut rig = new_editor();
    fill(&mut rig, "Fish", exe.to_str().unwrap());
    editor(
        &mut rig,
        E::SetWorkingDir(home.join("gone").to_str().unwrap().to_owned()),
    );
    check(&mut rig);
    assert_eq!(
        rig.shell.app.engine_editor.as_ref().unwrap().warnings,
        [CommandWarning::WorkingDirMissing]
    );
    assert!(has(&rig, "The folder does not exist."));
    editor(
        &mut rig,
        E::SetWorkingDir(home.to_str().unwrap().to_owned()),
    );
    check(&mut rig);
    assert!(
        rig.shell
            .app
            .engine_editor
            .as_ref()
            .unwrap()
            .warnings
            .is_empty()
    );
}

#[cfg(unix)]
#[test]
fn a_file_without_the_execute_bit_warns() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.txt");
    std::fs::write(&file, b"x").unwrap();
    let mut rig = new_editor();
    fill(&mut rig, "Fish", file.to_str().unwrap());
    check(&mut rig);
    assert_eq!(
        rig.shell.app.engine_editor.as_ref().unwrap().warnings,
        [CommandWarning::NotExecutable]
    );
    assert!(has(&rig, "The file cannot be run."));
}

#[test]
fn checking_an_empty_command_asks_for_no_disk_access() {
    let mut rig = new_editor();
    let effects = update(&mut rig, EngineMessage::Editor(E::Check));
    assert!(effects.is_empty(), "{effects:?}");
}

#[test]
fn the_check_is_not_triggered_by_typing_only_by_enter_or_browse() {
    let mut rig = new_editor();
    let effects = update(
        &mut rig,
        EngineMessage::Editor(E::SetCommand("/bin/x".to_owned())),
    );
    assert!(
        effects.is_empty(),
        "typing must not touch the disk: {effects:?}"
    );
}
