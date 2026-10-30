//! The "Engines" dialog from the specification of M4-3 ("Comportamento
//! observavel" 1, 3, 5; rules 4-6; D433, D743; Scid `analysis.tcl`
//! `listEngines`/`choose`/`findEngine`). Headless, through the real view.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

mod m4_3_gui_common;

use iced::{Event, mouse};
use m4_3_gui_common::*;
use mychess_app::engines::{
    EngineDraft, EngineMessage, EngineNotice, EngineRunState, FailureKind, FoundEngine, Hotkey,
    MANY_RUNS, StartMode, parse_scid_engines_dat,
};
use mychess_app::{AppNotice, NamedKey};
use mychess_gui::{Message, UiMessage};

// ---- empty list ----

#[test]
fn an_empty_list_invites_to_add_or_to_look_for_installed_engines() {
    let (rig, _) = rig_with(&[]);
    assert!(has(
        &rig,
        "No engines yet. Add one, or look for the engines installed on this computer."
    ));
    assert_eq!(engine_msgs(&clicks(&rig, "New")), [EngineMessage::New]);
    assert_eq!(
        engine_msgs(&clicks(&rig, "Find installed engines")),
        [EngineMessage::FindInstalled]
    );
}

#[test]
fn with_an_empty_list_the_row_buttons_do_nothing() {
    let (rig, _) = rig_with(&[]);
    for label in [
        "Edit",
        "Copy",
        "Delete",
        "Up",
        "Down",
        "UCI options...",
        "Start",
    ] {
        assert!(clicks(&rig, label).is_empty(), "{label}");
    }
}

// ---- columns ----

#[test]
fn columns_show_name_key_type_elo_and_the_utc_date_of_the_last_use() {
    let mut rig = Rig::new();
    let mut a = EngineDraft::new("Alpha", "/bin/alpha");
    a.elo = Some(3500);
    a.last_used = Some(1_700_000_000);
    let mut b = EngineDraft::new("Beta", "/bin/beta");
    b.protocol = mychess_app::engines::Protocol::XBoard;
    let ida = rig.shell.app.engines.add(a).unwrap();
    rig.shell.app.engines.add(b).unwrap();
    rig.shell
        .app
        .engines
        .set_hotkey(ida, Some(Hotkey::F3))
        .unwrap();
    send(&mut rig, EngineMessage::OpenDialog);
    for text in [
        "Name",
        "Key",
        "Type",
        "Elo",
        "Last used",
        "Alpha",
        "Beta",
        "F3",
        "3500",
        "uci",
        "xboard",
        "2023-11-14",
    ] {
        assert!(has(&rig, text), "{text}");
    }
    assert!(!has(&rig, "F2"), "only F3 is assigned");
}

#[test]
fn the_last_use_date_does_not_depend_on_the_time_zone() {
    // 2023-11-14T23:59:59Z and 2023-11-15T00:00:00Z.
    let mut rig = Rig::new();
    for (name, at) in [("Late", 1_700_006_399_i64), ("Next", 1_700_006_400)] {
        let mut d = EngineDraft::new(name, "/bin/x");
        d.last_used = Some(at);
        rig.shell.app.engines.add(d).unwrap();
    }
    send(&mut rig, EngineMessage::OpenDialog);
    assert!(has(&rig, "2023-11-14"));
    assert!(has(&rig, "2023-11-15"));
}

// ---- selection ----

#[test]
fn opening_selects_the_first_engine_and_a_click_selects_a_row() {
    let (mut rig, ids) = trio();
    assert_eq!(selected(&rig), Some(ids[0]));
    let sent = click(&mut rig, "Lc0");
    assert!(sent.contains(&EngineMessage::Select(Some(ids[1]))));
    assert_eq!(selected(&rig), Some(ids[1]));
}

#[test]
fn typing_a_letter_jumps_to_the_next_engine_that_starts_with_it() {
    let (mut rig, ids) = rig_with(&[
        ("Stockfish", "/s"),
        ("Crafty", "/c"),
        ("Lc0", "/l"),
        ("Komodo", "/k"),
        ("Critter", "/ct"),
    ]);
    rig.press(ch('l'));
    assert_eq!(selected(&rig), Some(ids[2]));
    rig.press(ch('c'));
    assert_eq!(
        selected(&rig),
        Some(ids[4]),
        "next 'c' after Lc0 is Critter"
    );
    rig.press(ch('C'));
    assert_eq!(
        selected(&rig),
        Some(ids[1]),
        "wraps around and ignores case"
    );
    rig.press(ch('z'));
    assert_eq!(selected(&rig), Some(ids[1]), "no match keeps the selection");
}

#[test]
fn a_letter_typed_into_a_text_field_does_not_jump() {
    let (mut rig, ids) = trio();
    rig.key(ch('l'), true, Some('l'));
    assert_eq!(selected(&rig), Some(ids[0]));
}

#[test]
fn arrows_move_the_selection_and_stop_at_the_ends() {
    let (mut rig, ids) = trio();
    rig.press(nk(NamedKey::Up));
    assert_eq!(selected(&rig), Some(ids[0]), "already at the top");
    rig.press(nk(NamedKey::Down));
    rig.press(nk(NamedKey::Down));
    rig.press(nk(NamedKey::Down));
    assert_eq!(selected(&rig), Some(ids[2]));
}

// ---- New / Edit / Copy ----

#[test]
fn new_and_edit_open_the_editor() {
    let (mut rig, ids) = trio();
    click(&mut rig, "New");
    let editor = rig.shell.app.engine_editor.as_ref().expect("editor");
    assert_eq!(editor.id, None);
    assert!(has(&rig, "New engine"));
    editor_cancel(&mut rig);
    click(&mut rig, "Edit");
    let editor = rig.shell.app.engine_editor.as_ref().expect("editor");
    assert_eq!(editor.id, Some(ids[0]));
    assert_eq!(editor.name, "Stockfish");
    assert!(has(&rig, "Edit engine"));
}

fn editor_cancel(rig: &mut Rig) {
    editor(rig, mychess_app::engines::EditorMessage::Cancel);
    assert!(rig.shell.app.engine_editor.is_none());
}

#[test]
fn copy_appends_the_suffix_new_selects_the_copy_and_gives_it_no_key() {
    let (mut rig, ids) = trio();
    rig.shell
        .app
        .engines
        .set_hotkey(ids[0], Some(Hotkey::F2))
        .unwrap();
    click(&mut rig, "Copy");
    assert_eq!(names(&rig), ["Stockfish", "Lc0", "Crafty", "Stockfish-new"]);
    let copy = rig.shell.app.engines.engines()[3].id;
    assert_eq!(selected(&rig), Some(copy));
    assert_eq!(rig.shell.app.engines.hotkey_of(copy), None);
    assert_eq!(rig.shell.app.engines.hotkey_of(ids[0]), Some(Hotkey::F2));
}

// ---- Delete ----

#[test]
fn delete_asks_with_the_name_and_the_command_and_cancel_keeps_the_engine() {
    let (mut rig, ids) = rig_with(&[("Fish Eye", "/opt/my engines/fish eye")]);
    click(&mut rig, "Delete");
    assert_eq!(
        rig.shell.app.engine_dialog.as_ref().unwrap().confirm_delete,
        Some(ids[0])
    );
    assert!(has(
        &rig,
        "Delete the engine Fish Eye (/opt/my engines/fish eye)?"
    ));
    click(&mut rig, "Cancel");
    assert_eq!(names(&rig), ["Fish Eye"]);
    assert!(
        rig.shell
            .app
            .engine_dialog
            .as_ref()
            .unwrap()
            .confirm_delete
            .is_none()
    );
}

#[test]
fn confirming_a_deletion_removes_the_engine_frees_its_key_and_selects_a_neighbour() {
    let (mut rig, ids) = trio();
    rig.shell
        .app
        .engines
        .set_hotkey(ids[1], Some(Hotkey::F2))
        .unwrap();
    click(&mut rig, "Lc0");
    click(&mut rig, "Delete");
    assert_eq!(names(&rig), ["Stockfish", "Lc0", "Crafty"], "nothing yet");
    // Both buttons are named "Delete" now: the confirmation is the top box.
    let sent = click(&mut rig, "Delete");
    assert_eq!(sent, [EngineMessage::ConfirmDelete(ids[1])]);
    assert_eq!(names(&rig), ["Stockfish", "Crafty"]);
    assert_eq!(rig.shell.app.engines.hotkey_owner(Hotkey::F2), None);
    assert_eq!(selected(&rig), Some(ids[2]));
}

#[test]
fn enter_on_the_confirmation_is_the_safe_answer_and_escape_cancels() {
    let (mut rig, _) = trio();
    click(&mut rig, "Delete");
    rig.press(nk(NamedKey::Enter));
    assert_eq!(names(&rig).len(), 3);
    assert!(
        rig.shell
            .app
            .engine_dialog
            .as_ref()
            .unwrap()
            .confirm_delete
            .is_none()
    );
    click(&mut rig, "Delete");
    rig.press(nk(NamedKey::Escape));
    assert_eq!(names(&rig).len(), 3);
    assert!(
        rig.shell
            .app
            .engine_dialog
            .as_ref()
            .unwrap()
            .confirm_delete
            .is_none()
    );
    assert!(rig.shell.app.engine_dialog.is_some(), "the list stays open");
}

#[test]
fn deleting_a_running_engine_stops_the_run_first() {
    let (mut rig, ids) = trio();
    let run = start(&mut rig, ids[0]);
    click(&mut rig, "Delete");
    let effects = update(&mut rig, EngineMessage::ConfirmDelete(ids[0]));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, mychess_app::Effect::StopEngine { run: r, .. } if *r == run)),
        "{effects:?}"
    );
    assert_eq!(rig.shell.app.engines.get(ids[0]), None);
}

// ---- Up / Down ----

#[test]
fn up_and_down_swap_with_the_neighbour_and_do_nothing_at_the_limits() {
    let (mut rig, _) = trio();
    click(&mut rig, "Up");
    assert_eq!(
        names(&rig),
        ["Stockfish", "Lc0", "Crafty"],
        "first row cannot go up"
    );
    click(&mut rig, "Down");
    assert_eq!(names(&rig), ["Lc0", "Stockfish", "Crafty"]);
    click(&mut rig, "Down");
    assert_eq!(names(&rig), ["Lc0", "Crafty", "Stockfish"]);
    click(&mut rig, "Down");
    assert_eq!(
        names(&rig),
        ["Lc0", "Crafty", "Stockfish"],
        "last row cannot go down"
    );
    click(&mut rig, "Up");
    assert_eq!(names(&rig), ["Lc0", "Stockfish", "Crafty"]);
}

#[test]
fn moving_an_engine_does_not_renumber_the_keys() {
    let (mut rig, ids) = trio();
    rig.shell
        .app
        .engines
        .set_hotkey(ids[0], Some(Hotkey::F2))
        .unwrap();
    rig.shell
        .app
        .engines
        .set_hotkey(ids[2], Some(Hotkey::F4))
        .unwrap();
    click(&mut rig, "Down");
    assert_eq!(rig.shell.app.engines.hotkey_owner(Hotkey::F2), Some(ids[0]));
    assert_eq!(rig.shell.app.engines.hotkey_owner(Hotkey::F4), Some(ids[2]));
}

#[test]
fn a_move_at_the_limit_asks_for_no_save_and_a_real_move_does() {
    let dir = tempfile::tempdir().unwrap();
    let mut app =
        mychess_app::AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
    let ids: Vec<_> = ["A", "B"]
        .iter()
        .map(|n| app.engines.add(EngineDraft::new(*n, "/x")).unwrap())
        .collect();
    let mut msg = |m| mychess_app::update(&mut app, em(m));
    assert!(msg(EngineMessage::MoveUp(ids[0])).is_empty());
    let effects = msg(EngineMessage::MoveDown(ids[0]));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, mychess_app::Effect::SaveEngines { .. })),
        "{effects:?}"
    );
}

// ---- keys ----

#[test]
fn a_key_taken_by_another_engine_leaves_the_previous_owner() {
    let (mut rig, ids) = trio();
    send(&mut rig, EngineMessage::SetHotkey(ids[0], Some(Hotkey::F2)));
    send(&mut rig, EngineMessage::SetHotkey(ids[1], Some(Hotkey::F2)));
    assert_eq!(rig.shell.app.engines.hotkey_owner(Hotkey::F2), Some(ids[1]));
    assert_eq!(rig.shell.app.engines.hotkey_of(ids[0]), None);
    // One row shows F2, and it is the new owner's.
    assert!(has(&rig, "F2"));
}

#[test]
fn each_engine_has_at_most_one_key() {
    let (mut rig, ids) = trio();
    send(&mut rig, EngineMessage::SetHotkey(ids[0], Some(Hotkey::F2)));
    send(&mut rig, EngineMessage::SetHotkey(ids[0], Some(Hotkey::F4)));
    assert_eq!(rig.shell.app.engines.hotkey_of(ids[0]), Some(Hotkey::F4));
    assert_eq!(rig.shell.app.engines.hotkey_owner(Hotkey::F2), None);
}

// ---- Start / Stop / Restart ----

#[test]
fn the_row_button_reads_start_stop_or_restart_after_the_state_of_the_run() {
    let (mut rig, ids) = trio();
    assert!(has(&rig, "Start"));
    let run = start(&mut rig, ids[0]);
    assert!(has(&rig, "Stop"));
    assert!(has(&rig, "Stockfish  [starting...]"));
    send(
        &mut rig,
        EngineMessage::StartFailed {
            run,
            failure: failure(FailureKind::CannotStart),
        },
    );
    assert!(has(&rig, "Restart"));
    assert!(has(&rig, "Stockfish  [failed]"));
    assert!(!has(&rig, "Stop"));
}

#[test]
fn start_with_the_left_button_analyses_and_enter_does_the_same() {
    let (mut rig, ids) = trio();
    assert_eq!(
        click(&mut rig, "Start"),
        [EngineMessage::Start(ids[0], StartMode::Analyze)]
    );
    let (mut rig, ids) = trio();
    rig.press(nk(NamedKey::Enter));
    assert!(rig.shell.app.engine_runs.of_engine(ids[0]).is_some());
    assert!(rig.shell.app.engine_runs.of_engine(ids[1]).is_none());
}

#[test]
fn a_double_click_on_a_row_starts_that_engine() {
    let (rig, ids) = trio();
    let mut s = sim(&rig);
    let bounds = s.find("Lc0").unwrap().visible_bounds().unwrap();
    s.point_at(bounds.center());
    for _ in 0..2 {
        drop(s.simulate(iced_test::simulator::click()));
    }
    let sent = engine_msgs(&s.into_messages().collect::<Vec<_>>());
    assert!(
        sent.contains(&EngineMessage::Start(ids[1], StartMode::Analyze)),
        "{sent:?}"
    );
}

#[test]
fn the_right_button_on_start_starts_without_analysing() {
    let (rig, ids) = trio();
    let mut s = sim(&rig);
    let bounds = s.find("Start").unwrap().visible_bounds().unwrap();
    s.point_at(bounds.center());
    drop(s.simulate([
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right)),
    ]));
    let sent = engine_msgs(&s.into_messages().collect::<Vec<_>>());
    assert_eq!(sent, [EngineMessage::Start(ids[0], StartMode::Idle)]);
}

#[test]
fn starting_a_running_engine_ends_it_instead_of_starting_it_twice() {
    let (mut rig, ids) = trio();
    let run = start(&mut rig, ids[0]);
    let effects = update(&mut rig, EngineMessage::Start(ids[0], StartMode::Analyze));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, mychess_app::Effect::StopEngine { run: r, .. } if *r == run)),
        "{effects:?}"
    );
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, mychess_app::Effect::StartEngine { .. }))
    );
    assert_eq!(
        rig.shell.app.engine_runs.len(),
        1,
        "never two runs of one engine"
    );
}

#[test]
fn a_stopping_run_offers_a_disabled_stop() {
    let (mut rig, ids) = trio();
    let run = start(&mut rig, ids[0]);
    send(&mut rig, EngineMessage::Stop(run));
    assert_eq!(
        rig.shell.app.engine_runs.get(run).unwrap().state,
        EngineRunState::Stopping
    );
    assert!(has(&rig, "Stockfish  [stopping...]"));
    assert!(clicks(&rig, "Stop").is_empty());
}

#[test]
fn from_four_runs_on_the_dialog_warns_about_cpu() {
    let (mut rig, ids) = rig_with(&[
        ("A", "/a"),
        ("B", "/b"),
        ("C", "/c"),
        ("D", "/d"),
        ("E", "/e"),
    ]);
    assert_eq!(MANY_RUNS, 4);
    for id in &ids[..3] {
        start(&mut rig, *id);
    }
    assert!(!has(&rig, "3 engines are running; this uses a lot of CPU."));
    start(&mut rig, ids[3]);
    assert!(has(&rig, "4 engines are running; this uses a lot of CPU."));
    start(&mut rig, ids[4]);
    assert!(has(&rig, "5 engines are running; this uses a lot of CPU."));
}

#[test]
fn a_fifth_start_also_raises_the_status_bar_remark() {
    let (mut rig, ids) = rig_with(&[("A", "/a"), ("B", "/b"), ("C", "/c"), ("D", "/d")]);
    for id in &ids {
        start(&mut rig, *id);
    }
    let notice = rig
        .shell
        .ui
        .status_notice
        .clone()
        .or_else(|| rig.shell.app.notice.clone());
    assert_eq!(
        notice,
        Some(AppNotice::Engine(EngineNotice::ManyRunning { count: 4 }))
    );
}

// ---- reports ----

fn answer_import(rig: &mut Rig, text: &str) {
    let effects = update(
        rig,
        EngineMessage::ImportScid(std::path::PathBuf::from("engines.dat")),
    );
    let request = effects
        .iter()
        .find_map(|e| match e {
            mychess_app::Effect::ImportScidEngines { request, .. } => Some(*request),
            _ => None,
        })
        .expect("an import effect");
    send(
        rig,
        EngineMessage::ScidRead {
            request,
            result: Box::new(Ok(parse_scid_engines_dat(text))),
        },
    );
}

const DAT: &str = "engine {\n Name Toga\n Cmd /usr/games/toga\n Dir .\n UCI 1\n Elo 2800\n}\nengine {\n Name Toga\n Cmd /usr/games/toga\n Dir .\n}\nengine {\n Cmd /x\n}\n";

#[test]
fn an_import_report_shows_the_counts_and_dismiss_clears_it() {
    let (mut rig, _) = trio();
    answer_import(&mut rig, DAT);
    // One added, one duplicate of the first, one without name/dir.
    assert!(has(
        &rig,
        "Imported 1; skipped 1 already in the list and 1 unusable."
    ));
    assert!(has(
        &rig,
        "Toga: the command or folder is relative to the Scid folder; check it."
    ));
    assert!(has(&rig, "Dismiss"));
    click(&mut rig, "Dismiss");
    assert!(!has(
        &rig,
        "Imported 1; skipped 1 already in the list and 1 unusable."
    ));
    assert!(!has(&rig, "Dismiss"));
    assert!(names(&rig).contains(&"Toga".to_owned()));
}

#[test]
fn a_file_without_engines_gives_the_nothing_to_import_remark() {
    let (mut rig, _) = trio();
    answer_import(&mut rig, "# nothing here\nset x 1\n");
    assert_eq!(names(&rig).len(), 3);
    let notice = rig
        .shell
        .ui
        .status_notice
        .clone()
        .or_else(|| rig.shell.app.notice.clone());
    assert_eq!(
        notice,
        Some(AppNotice::Engine(EngineNotice::NothingToImport))
    );
}

#[test]
fn found_engines_are_listed_to_click_and_never_added_by_themselves() {
    let (mut rig, _) = trio();
    let effects = update(&mut rig, EngineMessage::FindInstalled);
    let request = effects
        .iter()
        .find_map(|e| match e {
            mychess_app::Effect::FindInstalledEngines { request } => Some(*request),
            _ => None,
        })
        .expect("a search effect");
    assert!(has(&rig, "Looking for engines..."));
    assert!(
        clicks(&rig, "Find installed engines").is_empty(),
        "disabled while searching"
    );
    send(
        &mut rig,
        EngineMessage::Found {
            request,
            engines: vec![FoundEngine {
                name: "komodo".to_owned(),
                path: std::path::PathBuf::from("/opt/my engines/komodo"),
            }],
        },
    );
    assert!(has(&rig, "Engines found (click to add)"));
    assert_eq!(names(&rig).len(), 3, "nothing added yet");
    let sent = click(&mut rig, "komodo  (/opt/my engines/komodo)");
    assert_eq!(
        sent,
        [EngineMessage::AddFound(std::path::PathBuf::from(
            "/opt/my engines/komodo"
        ))]
    );
    click(&mut rig, "Dismiss");
    assert!(!has(&rig, "Engines found (click to add)"));
}

#[test]
fn problems_reading_engines_toml_are_listed_in_the_dialog() {
    let (mut rig, _) = rig_with(&[]);
    let loaded = mychess_app::engines::load_engine_list(
        "version = 1\n[[engine]]\nname = \"A\"\ncommand = \"/a\"\nelo = \"high\"\nhotkey = \"F2\"\n[[engine]]\nname = \"B\"\ncommand = \"/b\"\nhotkey = \"F2\"\n[[engine]]\nname = \"\"\ncommand = \"/c\"\n",
    );
    let loaded = loaded.expect("readable");
    assert!(!loaded.issues.is_empty());
    send(&mut rig, EngineMessage::Loaded(Box::new(Ok(loaded))));
    assert!(has(&rig, "Remarks about the engine list"));
    assert!(has(&rig, "Dismiss"));
    click(&mut rig, "Dismiss");
    assert!(!has(&rig, "Remarks about the engine list"));
}

// ---- analysis options ----

#[test]
fn the_analysis_options_unfold_and_send_the_changed_preferences() {
    let (mut rig, _) = trio();
    assert!(!has(&rig, "Low CPU priority"));
    let sent = clicks(&rig, "Analysis options");
    assert!(matches!(
        sent[..],
        [Message::Ui(UiMessage::ToggleEnginePrefs)]
    ));
    rig.feed(sent);
    for text in [
        "Log size (lines, 0 = off)",
        "Max ply (0 = unlimited)",
        "Add the engine name to comments",
        "Low CPU priority",
        "Wrap long lines",
        "Show extra information",
    ] {
        assert!(has(&rig, text), "{text}");
    }
    let before = *rig.shell.app.prefs.analysis;
    let sent = engine_msgs(&clicks(&rig, "Wrap long lines"));
    match &sent[..] {
        [EngineMessage::SetAnalysisPrefs(p)] => {
            assert_eq!(p.wrap_pv, !before.wrap_pv);
            assert_eq!(p.log_lines, before.log_lines);
        }
        other => panic!("{other:?}"),
    }
}

// ---- edges ----

#[test]
fn the_longest_list_of_256_engines_draws_and_navigates_to_both_ends() {
    let mut rig = Rig::new();
    for n in 0..256 {
        rig.shell
            .app
            .engines
            .add(EngineDraft::new(
                format!("Engine {n:03}"),
                format!("/bin/e{n}"),
            ))
            .unwrap();
    }
    send(&mut rig, EngineMessage::OpenDialog);
    assert!(has(&rig, "Engine 000"));
    for _ in 0..300 {
        rig.press(nk(NamedKey::Down));
    }
    let last = rig.shell.app.engines.engines()[255].id;
    assert_eq!(selected(&rig), Some(last));
    // The list is full: copying says so and adds nothing.
    send(&mut rig, EngineMessage::Copy(last));
    assert_eq!(rig.shell.app.engines.len(), 256);
    let notice = rig
        .shell
        .ui
        .status_notice
        .clone()
        .or_else(|| rig.shell.app.notice.clone());
    assert_eq!(notice, Some(AppNotice::Engine(EngineNotice::ListFull)));
}

#[test]
fn huge_unicode_and_fluent_looking_names_draw_and_show_up_verbatim() {
    let hostile = "{ $name } { -term } { NUMBER(1) } \u{202e}\u{1F600}\u{0301}";
    let (mut rig, _) = rig_with(&[
        (hostile, "/bin/{ $command }"),
        ("日本語エンジン", "/ビン/エンジン"),
    ]);
    assert!(has(&rig, hostile));
    assert!(has(&rig, "日本語エンジン"));
    click(&mut rig, "Delete");
    assert!(has(
        &rig,
        &format!("Delete the engine {hostile} (/bin/{{ $command }})?")
    ));
}

#[test]
fn a_name_of_4096_characters_draws_without_panic() {
    let name = "W".repeat(4096);
    let (rig, ids) = rig_with(&[(name.as_str(), "/bin/w")]);
    assert_eq!(
        rig.shell
            .app
            .engines
            .get(ids[0])
            .unwrap()
            .name
            .chars()
            .count(),
        4096
    );
    drop(sim(&rig).find("Start").unwrap());
}

#[test]
fn commands_with_spaces_show_in_the_confirmation_untouched() {
    let (mut rig, _) = rig_with(&[("Fish", "C:\\Program Files\\Fish Chess\\fish.exe")]);
    click(&mut rig, "Delete");
    assert!(has(
        &rig,
        "Delete the engine Fish (C:\\Program Files\\Fish Chess\\fish.exe)?"
    ));
}
