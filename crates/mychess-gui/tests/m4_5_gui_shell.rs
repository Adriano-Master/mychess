//! M4-5 (GUI): the bar, the traffic lights and the arrows inside the real
//! window, driven by synthetic engine events (spec "Comportamento
//! observavel", D455, D821-D826, D850-D857). Headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp,
    clippy::format_push_string,
    clippy::assert_is_empty,
    clippy::too_many_lines,
    clippy::match_same_arms,
    clippy::map_unwrap_or,
    clippy::redundant_closure_for_method_calls
)]

mod m4_4_gui_common;

use iced::widget;
use m4_4_gui_common::*;
use mychess_app::engines::EngineMessage;
use mychess_app::{
    Action, ArrowKind, ArrowMode, ArrowPrefs, EvalBarMessage, EvalBarPrefs, PrefChange,
    PrefsMessage, Shortcuts, TrafficLightPrefs,
};
use mychess_core::Square;
use mychess_engine::{Bound, Score, SearchId, SearchInfo};
use mychess_gui::keymap::{Platform, binding_label, key_input};
use mychess_gui::widgets::board::interaction::BoardEvent;
use mychess_gui::windows::board_column::{EVAL_BAR_ID, LIGHTS_ID};
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};

fn sq(name: &str) -> Square {
    name.parse().expect("square")
}

fn present(rig: &Rig, id: &'static str) -> bool {
    rig.sim(SIZE).find(widget::Id::new(id)).is_ok()
}

fn bounds_of(rig: &Rig, id: &'static str) -> iced::Rectangle {
    rig.sim(SIZE)
        .find(widget::Id::new(id))
        .expect("a widget")
        .bounds()
}

fn shortcut(rig: &mut Rig, letter: char) {
    rig.press(command_shift(Key::Char(letter)));
}

fn eng(rig: &Rig) -> mychess_app::EngineViewPrefs {
    *rig.shell.app.prefs.engine_view
}

fn set(rig: &mut Rig, change: PrefChange) {
    rig.app_msg(AppMessage::Prefs(PrefsMessage::Set(change)));
}

fn arrows_on(rig: &mut Rig, run: mychess_app::engines::RunId) {
    rig.app_msg(AppMessage::EvalBar(EvalBarMessage::ToggleArrows(run)));
}

fn mate_info(search: u64, multipv: u8, mate: i32, uci: &str) -> SearchInfo {
    SearchInfo {
        search: SearchId(search),
        multipv,
        depth: Some(20),
        score: Some((Score::Mate(mate), Bound::Exact)),
        pv: pv_of(&Position::new(), &[uci]),
        ..SearchInfo::default()
    }
}

// ---- defaults and shortcuts ----

#[test]
fn bar_lights_and_arrows_are_off_by_default_like_the_scid() {
    let (rig, _run) = uci_run_with_lines();
    let p = eng(&rig);
    assert!(!p.eval_bar.show, "Scid gameInfo(showEval) = 0");
    assert!(!p.traffic_lights.show, "Scid enableMPVTls = 0");
    assert!(!p.arrows.enabled);
    assert_eq!(p.eval_bar.scale, 5);
    assert!(!p.eval_bar.log && !p.eval_bar.ticks && p.eval_bar.label);
    assert_eq!(p.arrows.mode, ArrowMode::Arrows);
    assert!(p.arrows.number_lines && p.arrows.click_to_play);
    assert_eq!(p.arrows.sequence_len, 3);
    assert!(!present(&rig, EVAL_BAR_ID));
    assert!(!present(&rig, LIGHTS_ID));
    assert!(rig.shell.app.engine_arrows().is_empty());
}

#[test]
fn the_bar_shortcut_shows_and_hides_the_bar_in_the_window() {
    let mut rig = Rig::new();
    shortcut(&mut rig, 's');
    assert!(eng(&rig).eval_bar.show);
    assert!(present(&rig, EVAL_BAR_ID), "shown even without an engine");
    shortcut(&mut rig, 's');
    assert!(!eng(&rig).eval_bar.show);
    assert!(!present(&rig, EVAL_BAR_ID));
}

#[test]
fn the_bar_is_level_with_the_squares_in_the_real_window() {
    let mut rig = Rig::new();
    shortcut(&mut rig, 's');
    let canvas = bounds_of(&rig, "board-canvas");
    let bar = bounds_of(&rig, EVAL_BAR_ID);
    assert!(
        bar.x >= canvas.x + canvas.width,
        "to the right of the board"
    );
    assert!(bar.height > 0.0 && bar.height <= canvas.height);
    assert!(bar.x + bar.width <= SIZE.width + 0.5);
}

#[test]
fn the_lights_shortcut_draws_one_band_per_multipv_line() {
    let (mut rig, _run) = uci_run_with_lines();
    shortcut(&mut rig, 'l');
    assert!(eng(&rig).traffic_lights.show);
    let lights = rig.shell.app.traffic_lights();
    assert_eq!(lights.len(), 3);
    let ranks: Vec<u8> = lights.iter().map(|l| l.rank).collect();
    assert_eq!(ranks, [1, 2, 3], "line 1 at the top");
    assert!(present(&rig, LIGHTS_ID));
    shortcut(&mut rig, 'l');
    assert!(!present(&rig, LIGHTS_ID));
    assert!(rig.shell.app.traffic_lights().is_empty());
}

#[test]
fn the_lights_follow_the_multipv_count_of_the_engine() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    shortcut(&mut rig, 'l');
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetMultiPv(run, 5)));
    started(&mut rig, run, 1);
    let start = Position::new();
    let moves = ["e2e4", "d2d4", "g1f3", "c2c4", "b1c3"];
    for (i, uci) in moves.iter().enumerate() {
        let n = u8::try_from(i + 1).unwrap();
        info(
            &mut rig,
            run,
            report(&start, 1, n, 12, 60 - 10 * i32::from(n), &[uci]),
        );
        assert_eq!(
            rig.shell.app.traffic_lights().len(),
            i + 1,
            "after line {n}"
        );
    }
    // Strictly top to bottom, line 1 first.
    let ranks: Vec<u8> = rig
        .shell
        .app
        .traffic_lights()
        .iter()
        .map(|l| l.rank)
        .collect();
    assert_eq!(ranks, [1, 2, 3, 4, 5]);
}

#[test]
fn the_numbers_shortcut_toggles_the_arrow_numbering() {
    let mut rig = Rig::new();
    assert!(eng(&rig).arrows.number_lines);
    shortcut(&mut rig, 'm');
    assert!(!eng(&rig).arrows.number_lines);
    shortcut(&mut rig, 'm');
    assert!(eng(&rig).arrows.number_lines);
}

#[test]
fn the_new_shortcuts_use_command_so_ctrl_on_linux_and_cmd_on_macos() {
    use iced::keyboard::{Key as IcedKey, Modifiers};
    let shortcuts = Shortcuts::defaults();
    let mods = Modifiers::COMMAND | Modifiers::SHIFT;
    for (letter, action) in [
        ("S", Action::ToggleEvalBar),
        ("L", Action::ToggleTrafficLights),
        ("M", Action::ToggleArrowNumbers),
    ] {
        let input = key_input(&IcedKey::Character(letter.into()), mods).expect("input");
        assert_eq!(shortcuts.lookup_in(&input, false), Some(action), "{letter}");
        let binding = shortcuts.bindings(action).first().copied().expect("bound");
        assert_eq!(
            binding_label(&binding, Platform::Linux),
            format!("Ctrl+Shift+{letter}")
        );
        assert_eq!(
            binding_label(&binding, Platform::Windows),
            format!("Ctrl+Shift+{letter}")
        );
        let mac = binding_label(&binding, Platform::MacOs);
        assert!(mac.contains('⌘') && !mac.contains("Ctrl"), "{mac}");
        // Command is required: Shift alone and Alt do nothing.
        let shift = key_input(&IcedKey::Character(letter.into()), Modifiers::SHIFT).expect("input");
        assert_eq!(shortcuts.lookup_in(&shift, false), None, "{letter} + Shift");
        let alt = key_input(
            &IcedKey::Character(letter.into()),
            Modifiers::ALT | Modifiers::SHIFT,
        )
        .expect("input");
        assert_eq!(
            shortcuts.lookup_in(&alt, false),
            None,
            "{letter} + Alt+Shift"
        );
    }
}

#[test]
fn the_new_shortcuts_do_not_steal_the_old_ones() {
    // The Scid's Ctrl+Shift+C / N stay "copy FEN" and "new game" here.
    let s = Shortcuts::defaults();
    let bound = |key: char| {
        let input = KeyInput {
            key: Key::Char(key),
            modifiers: Modifiers {
                command: true,
                shift: true,
                alt: false,
            },
        };
        s.lookup_in(&input, false)
    };
    assert_eq!(bound('n'), Some(Action::NewGame));
    for k in ['s', 'l', 'm'] {
        assert!(
            matches!(
                bound(k),
                Some(
                    Action::ToggleEvalBar
                        | Action::ToggleTrafficLights
                        | Action::ToggleArrowNumbers
                )
            ),
            "{k}"
        );
    }
}

// ---- View menu ----

fn view_entries(rig: &Rig) -> Vec<MenuEntry> {
    menu_specs(&rig.shell.app, &rig.shell.ui, rig.shell.options)
        .into_iter()
        .find(|s| {
            s.entries.iter().any(|e| {
                matches!(
                    e,
                    MenuEntry::Item {
                        label: Label::Key("menu-view-evalbar"),
                        ..
                    }
                )
            })
        })
        .expect("a menu with the evaluation bar item")
        .entries
}

fn find<'a>(entries: &'a [MenuEntry], key: &str) -> &'a MenuEntry {
    entries
        .iter()
        .find(|e| match e {
            MenuEntry::Item {
                label: Label::Key(k),
                ..
            }
            | MenuEntry::Submenu {
                label: Label::Key(k),
                ..
            } => *k == key,
            _ => false,
        })
        .unwrap_or_else(|| panic!("menu entry {key}"))
}

fn sub<'a>(entries: &'a [MenuEntry], key: &str) -> &'a [MenuEntry] {
    match find(entries, key) {
        MenuEntry::Submenu { entries, .. } => entries,
        other => panic!("{key} is not a submenu: {other:?}"),
    }
}

fn checked(entries: &[MenuEntry], key: &str) -> Option<bool> {
    match find(entries, key) {
        MenuEntry::Item { checked, .. } => *checked,
        MenuEntry::Submenu { .. } => None,
        MenuEntry::Separator | MenuEntry::Group(_) => None,
    }
}

fn action_of(entry: &MenuEntry) -> Message {
    match entry {
        MenuEntry::Item { action, .. } => action.clone().expect("an enabled item"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_view_menu_has_the_three_checkable_items_with_their_shortcuts() {
    let rig = Rig::new();
    let entries = view_entries(&rig);
    for (key, action) in [
        ("menu-view-evalbar", Action::ToggleEvalBar),
        ("menu-view-traffic-lights", Action::ToggleTrafficLights),
        ("menu-view-arrow-numbers", Action::ToggleArrowNumbers),
    ] {
        let MenuEntry::Item {
            shortcut, checked, ..
        } = find(&entries, key)
        else {
            panic!("{key} is not an item");
        };
        assert!(checked.is_some(), "{key} is checkable");
        assert_eq!(
            *shortcut,
            rig.shell.app.shortcuts.bindings(action).first().copied(),
            "{key}"
        );
    }
    assert_eq!(checked(&entries, "menu-view-evalbar"), Some(false));
    assert_eq!(checked(&entries, "menu-view-traffic-lights"), Some(false));
    assert_eq!(checked(&entries, "menu-view-arrow-numbers"), Some(true));
}

#[test]
fn view_menu_items_toggle_the_preferences_and_the_check_follows() {
    let mut rig = Rig::new();
    for (key, read) in [
        (
            "menu-view-evalbar",
            (|p: mychess_app::EngineViewPrefs| p.eval_bar.show) as fn(_) -> bool,
        ),
        ("menu-view-traffic-lights", |p| p.traffic_lights.show),
        ("menu-view-arrow-numbers", |p| p.arrows.number_lines),
    ] {
        let before = read(eng(&rig));
        let message = action_of(find(&view_entries(&rig), key));
        let Message::App(app) = message else {
            panic!("{key} sends an application message");
        };
        rig.send(app);
        assert_eq!(read(eng(&rig)), !before, "{key}");
        assert_eq!(checked(&view_entries(&rig), key), Some(!before), "{key}");
    }
}

#[test]
fn the_scale_submenu_lists_1_to_10_and_marks_the_current_one() {
    let mut rig = Rig::new();
    let scales = |rig: &Rig| {
        sub(
            sub(&view_entries(rig), "menu-options-evalbar"),
            "menu-options-evalbar-scale",
        )
        .to_vec()
    };
    let list = scales(&rig);
    assert_eq!(list.len(), 10);
    let labels: Vec<String> = list
        .iter()
        .map(|e| match e {
            MenuEntry::Item {
                label: Label::Text(t),
                ..
            } => t.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(labels, (1..=10).map(|n| n.to_string()).collect::<Vec<_>>());
    let on: Vec<bool> = list
        .iter()
        .map(|e| {
            matches!(
                e,
                MenuEntry::Item {
                    checked: Some(true),
                    ..
                }
            )
        })
        .collect();
    assert_eq!(on.iter().filter(|b| **b).count(), 1);
    assert!(on[4], "5 is the default");
    // Picking 8 changes the preference and the mark.
    let Message::App(app) = action_of(&list[7]) else {
        panic!("app message");
    };
    rig.send(app);
    assert_eq!(eng(&rig).eval_bar.scale, 8);
    let after = scales(&rig);
    assert!(matches!(
        after[7],
        MenuEntry::Item {
            checked: Some(true),
            ..
        }
    ));
    assert!(matches!(
        after[4],
        MenuEntry::Item {
            checked: Some(false),
            ..
        }
    ));
}

#[test]
fn log_ticks_and_label_items_flip_their_flags() {
    let mut rig = Rig::new();
    for key in [
        "menu-options-evalbar-log",
        "menu-options-evalbar-ticks",
        "menu-options-evalbar-label",
    ] {
        let before = |rig: &Rig| {
            let p = eng(rig).eval_bar;
            match key {
                "menu-options-evalbar-log" => p.log,
                "menu-options-evalbar-ticks" => p.ticks,
                _ => p.label,
            }
        };
        let was = before(&rig);
        let entries = view_entries(&rig);
        let item = find(sub(&entries, "menu-options-evalbar"), key).clone();
        assert_eq!(
            matches!(
                item,
                MenuEntry::Item {
                    checked: Some(true),
                    ..
                }
            ),
            was,
            "{key}: the check shows the state"
        );
        let Message::App(app) = action_of(&item) else {
            panic!("app message");
        };
        rig.send(app);
        assert_eq!(before(&rig), !was, "{key}");
    }
}

#[test]
fn the_arrow_options_cover_mode_side_length_and_click_to_play() {
    let mut rig = Rig::new();
    let opts = |rig: &Rig| sub(&view_entries(rig), "menu-options-arrows").to_vec();
    let pick = |rig: &mut Rig, key: &str| {
        let list = opts(rig);
        let Message::App(app) = action_of(find(&list, key)) else {
            panic!("app message");
        };
        rig.send(app);
    };
    assert_eq!(
        checked(&opts(&rig), "analysis-arrow-mode-arrows"),
        Some(true)
    );
    pick(&mut rig, "analysis-arrow-mode-sequence");
    assert_eq!(eng(&rig).arrows.mode, ArrowMode::Sequence);
    assert_eq!(
        checked(&opts(&rig), "analysis-arrow-mode-sequence"),
        Some(true)
    );
    assert_eq!(
        checked(&opts(&rig), "analysis-arrow-mode-arrows"),
        Some(false)
    );
    pick(&mut rig, "analysis-arrow-seq-white");
    assert_eq!(
        eng(&rig).arrows.sequence_side,
        mychess_app::SequenceSide::White
    );
    pick(&mut rig, "analysis-arrow-seq-black");
    assert_eq!(
        eng(&rig).arrows.sequence_side,
        mychess_app::SequenceSide::Black
    );
    pick(&mut rig, "analysis-arrow-seq-both");
    assert_eq!(
        eng(&rig).arrows.sequence_side,
        mychess_app::SequenceSide::Both
    );
    // The length: 1..=8.
    let lens = sub(&opts(&rig), "analysis-arrow-seq-len").to_vec();
    assert_eq!(lens.len(), 8);
    let Message::App(app) = action_of(&lens[5]) else {
        panic!("app message");
    };
    rig.send(app);
    assert_eq!(eng(&rig).arrows.sequence_len, 6);
    // Click to play flips.
    assert!(eng(&rig).arrows.click_to_play);
    pick(&mut rig, "menu-options-arrows-click");
    assert!(!eng(&rig).arrows.click_to_play);
}

#[test]
fn preference_messages_clamp_bad_values() {
    let mut rig = Rig::new();
    let p = eng(&rig);
    set(
        &mut rig,
        PrefChange::EvalBar(EvalBarPrefs {
            scale: 99,
            ..p.eval_bar
        }),
    );
    assert!((1..=10).contains(&eng(&rig).eval_bar.scale));
    let p = eng(&rig);
    set(
        &mut rig,
        PrefChange::EvalBar(EvalBarPrefs {
            scale: 0,
            ..p.eval_bar
        }),
    );
    assert!((1..=10).contains(&eng(&rig).eval_bar.scale));
    let p = eng(&rig);
    set(
        &mut rig,
        PrefChange::Arrows(ArrowPrefs {
            sequence_len: 200,
            ..p.arrows
        }),
    );
    assert!((1..=8).contains(&eng(&rig).arrows.sequence_len));
    let p = eng(&rig);
    set(
        &mut rig,
        PrefChange::Arrows(ArrowPrefs {
            sequence_len: 0,
            ..p.arrows
        }),
    );
    assert!((1..=8).contains(&eng(&rig).arrows.sequence_len));
    set(
        &mut rig,
        PrefChange::TrafficLights(TrafficLightPrefs { show: true }),
    );
    assert!(eng(&rig).traffic_lights.show);
}

// ---- the bar follows the engine ----

#[test]
fn the_bar_follows_the_main_engine_and_saturates_on_mate() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    shortcut(&mut rig, 's');
    started(&mut rig, run, 1);
    info(
        &mut rig,
        run,
        report(&Position::new(), 1, 1, 10, 0, &["e2e4"]),
    );
    let even = rig.shell.app.eval_bar_view().clone();
    assert_eq!(even.fill, Some(0.5), "{even:?}");
    info(&mut rig, run, mate_info(1, 1, 3, "e2e4"));
    let white = rig.shell.app.eval_bar_view().clone();
    assert_eq!(white.fill, Some(1.0), "{white:?}");
    assert!(white.label_on_white);
    assert!(
        white.label.as_deref().is_some_and(|l| l.contains('3')),
        "{white:?}"
    );
    info(&mut rig, run, mate_info(1, 1, -2, "e2e4"));
    let black = rig.shell.app.eval_bar_view().clone();
    assert_eq!(black.fill, Some(0.0), "{black:?}");
    assert!(!black.label_on_white);
    assert!(
        black.label.as_deref().is_some_and(|l| l.contains('2')),
        "{black:?}"
    );
}

#[test]
fn a_huge_centipawn_score_saturates_without_overflow() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    shortcut(&mut rig, 's');
    started(&mut rig, run, 1);
    for cp in [i32::MAX, i32::MIN, 100_000, -100_000] {
        info(
            &mut rig,
            run,
            report(&Position::new(), 1, 1, 10, cp, &["e2e4"]),
        );
        let fill = rig.shell.app.eval_bar_view().fill.expect("a fill");
        assert!(fill == 0.0 || fill == 1.0, "{cp}: {fill}");
        assert_eq!(fill == 1.0, cp > 0);
    }
}

#[test]
fn the_scale_and_the_logarithm_change_the_fill() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    shortcut(&mut rig, 's');
    started(&mut rig, run, 1);
    info(
        &mut rig,
        run,
        report(&Position::new(), 1, 1, 10, 100, &["e2e4"]),
    );
    let linear5 = rig.shell.app.eval_bar_view().fill.unwrap();
    assert!((linear5 - 0.6).abs() < 1e-4, "(5 + 1) / 10: {linear5}");
    let p = eng(&rig);
    set(
        &mut rig,
        PrefChange::EvalBar(EvalBarPrefs {
            scale: 1,
            ..p.eval_bar
        }),
    );
    assert_eq!(rig.shell.app.eval_bar_view().fill, Some(1.0), "scale 1");
    let p = eng(&rig);
    set(
        &mut rig,
        PrefChange::EvalBar(EvalBarPrefs {
            scale: 10,
            log: false,
            ..p.eval_bar
        }),
    );
    let s10 = rig.shell.app.eval_bar_view().fill.unwrap();
    assert!((s10 - 0.55).abs() < 1e-4, "{s10}");
    let p = eng(&rig);
    set(
        &mut rig,
        PrefChange::EvalBar(EvalBarPrefs {
            log: true,
            ..p.eval_bar
        }),
    );
    let log = rig.shell.app.eval_bar_view().fill.unwrap();
    assert!(
        (log - 0.625).abs() < 1e-4,
        "wlen = 0.5, (0.5 + 2) / 4 = 0.625: {log}"
    );
}

#[test]
fn turning_the_board_inverts_the_bar() {
    // Spec D453 and criterion "Semaforo e barra invertem ou nao conforme a
    // orientacao": the board is turned by the window, so the bar must be.
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    shortcut(&mut rig, 's');
    started(&mut rig, run, 1);
    info(
        &mut rig,
        run,
        report(&Position::new(), 1, 1, 10, 150, &["e2e4"]),
    );
    assert!(!rig.shell.app.eval_bar_view().flipped);
    rig.msg(Message::Ui(UiMessage::FlipBoard));
    assert!(
        rig.shell.app.eval_bar_view().flipped,
        "the board is turned, the bar must show White at the top"
    );
    rig.msg(Message::Ui(UiMessage::FlipBoard));
    assert!(!rig.shell.app.eval_bar_view().flipped);
}

#[test]
fn turning_the_board_with_the_shortcut_inverts_the_bar_too() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    shortcut(&mut rig, 's');
    started(&mut rig, run, 1);
    info(
        &mut rig,
        run,
        report(&Position::new(), 1, 1, 10, 150, &["e2e4"]),
    );
    rig.press(command(Key::Char('f')));
    assert!(rig.shell.app.eval_bar_view().flipped);
}

#[test]
fn a_dead_engine_stops_feeding_the_bar_the_lights_and_the_arrows() {
    let (mut rig, run) = uci_run_with_lines();
    shortcut(&mut rig, 's');
    shortcut(&mut rig, 'l');
    arrows_on(&mut rig, run);
    assert!(!rig.shell.app.engine_arrows().is_empty());
    assert_eq!(rig.shell.app.traffic_lights().len(), 3);
    assert!(rig.shell.app.eval_bar_view().fill.is_some());
    rig.app_msg(AppMessage::Engine(EngineMessage::Stopped(run)));
    assert!(rig.shell.app.engine_arrows().is_empty(), "arrows are gone");
    assert!(rig.shell.app.traffic_lights().is_empty(), "lights are gone");
    assert_eq!(
        rig.shell.app.eval_bar_view().fill,
        None,
        "no comment on the position: the bar is empty, not the dead score"
    );
    assert!(present(&rig, EVAL_BAR_ID), "the empty outline stays");
    assert_eq!(rig.shell.app.arrows_engine(), None);
}

// ---- arrows ----

#[test]
fn marking_an_engine_draws_numbered_arrows_for_three_lines() {
    let (mut rig, run) = uci_run_with_lines();
    assert!(rig.shell.app.engine_arrows().is_empty(), "off by default");
    arrows_on(&mut rig, run);
    assert!(eng(&rig).arrows.enabled);
    let arrows = rig.shell.app.engine_arrows().to_vec();
    assert_eq!(arrows.len(), 3);
    assert_eq!(arrows[0].kind, ArrowKind::Best);
    assert_eq!(arrows[0].label, None, "the best one is not numbered");
    assert_eq!(arrows[1].label, Some(2));
    assert_eq!(arrows[2].label, Some(3));
    let squares: Vec<(Square, Square)> = arrows.iter().map(|a| (a.from, a.to)).collect();
    assert_eq!(
        squares,
        [
            (sq("e2"), sq("e4")),
            (sq("d2"), sq("d4")),
            (sq("g1"), sq("f3"))
        ]
    );
    arrows_on(&mut rig, run);
    assert!(rig.shell.app.engine_arrows().is_empty(), "again turns off");
}

#[test]
fn two_lines_are_not_numbered_like_the_scid() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetMultiPv(run, 2)));
    started(&mut rig, run, 1);
    let start = Position::new();
    info(&mut rig, run, report(&start, 1, 1, 12, 30, &["e2e4"]));
    info(&mut rig, run, report(&start, 1, 2, 12, 20, &["d2d4"]));
    arrows_on(&mut rig, run);
    let arrows = rig.shell.app.engine_arrows();
    assert_eq!(arrows.len(), 2);
    assert!(arrows.iter().all(|a| a.label.is_none()), "{arrows:?}");
}

#[test]
fn five_lines_make_five_arrows_and_the_rank_is_the_line() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetMultiPv(run, 5)));
    started(&mut rig, run, 1);
    let start = Position::new();
    for (i, uci) in ["e2e4", "d2d4", "g1f3", "c2c4", "b1c3"].iter().enumerate() {
        let n = u8::try_from(i + 1).unwrap();
        info(
            &mut rig,
            run,
            report(&start, 1, n, 12, 50 - 5 * i32::from(n), &[uci]),
        );
    }
    arrows_on(&mut rig, run);
    let arrows = rig.shell.app.engine_arrows();
    let ranks: Vec<u8> = arrows.iter().map(|a| a.rank).collect();
    assert_eq!(ranks, [1, 2, 3, 4, 5]);
    let labels: Vec<Option<u8>> = arrows.iter().map(|a| a.label).collect();
    assert_eq!(labels, [None, Some(2), Some(3), Some(4), Some(5)]);
}

#[test]
fn the_sequence_mode_draws_the_main_line_up_to_the_configured_length() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    started(&mut rig, run, 1);
    let start = Position::new();
    info(
        &mut rig,
        run,
        report(
            &start,
            1,
            1,
            14,
            30,
            &["e2e4", "e7e5", "g1f3", "b8c6", "f1b5"],
        ),
    );
    arrows_on(&mut rig, run);
    let p = eng(&rig);
    set(
        &mut rig,
        PrefChange::Arrows(ArrowPrefs {
            mode: ArrowMode::Sequence,
            sequence_len: 3,
            ..p.arrows
        }),
    );
    let arrows = rig.shell.app.engine_arrows().to_vec();
    assert_eq!(arrows.len(), 3);
    for (i, a) in arrows.iter().enumerate() {
        assert_eq!(a.kind, ArrowKind::Sequence(u8::try_from(i).unwrap()));
        assert_eq!(a.label, Some(u8::try_from(i + 1).unwrap()), "numbered 1..K");
    }
    assert_eq!(arrows[0].side, mychess_core::Color::White);
    assert_eq!(arrows[1].side, mychess_core::Color::Black);
    assert_eq!(arrows[2].side, mychess_core::Color::White);
}

#[test]
fn a_pv_with_an_illegal_move_stops_the_sequence_there() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    started(&mut rig, run, 1);
    let start = Position::new();
    let mut bad = report(&start, 1, 1, 14, 30, &["e2e4", "e7e5"]);
    // Third move: Ra1-a8 is not legal after 1.e4 e5.
    let mut after = start.clone();
    for u in ["e2e4", "e7e5"] {
        let m = after.parse_uci(u).unwrap();
        after.play(m).unwrap();
    }
    let legal = after.legal_moves();
    bad.pv.push(*legal.first().expect("a legal move"));
    // Replace the last move with one from another position (same piece
    // count, wrong side to move for it): the 2nd move twice.
    let second = bad.pv[1];
    bad.pv.push(second);
    info(&mut rig, run, bad);
    arrows_on(&mut rig, run);
    let p = eng(&rig);
    set(
        &mut rig,
        PrefChange::Arrows(ArrowPrefs {
            mode: ArrowMode::Sequence,
            sequence_len: 8,
            ..p.arrows
        }),
    );
    let n = rig.shell.app.engine_arrows().len();
    assert!(
        (3..=3).contains(&n),
        "3 legal moves, the 4th repeats e7e5: {n}"
    );
}

#[test]
fn arrows_follow_the_cursor_and_vanish_when_it_leaves_the_analysed_position() {
    let (mut rig, run) = uci_run_with_lines();
    arrows_on(&mut rig, run);
    assert_eq!(rig.shell.app.engine_arrows().len(), 3);
    let game = rig.game().clone();
    let child = game.children(game.root()).map(|c| c.len()).unwrap_or(0);
    assert_eq!(child, 0);
    // Move the cursor: play e4 on the board (no prompt at the end of the line).
    let mv = Position::new().parse_uci("e2e4").unwrap();
    rig.send(mychess_app::GameMessage::PlayMove {
        mv,
        options: mychess_app::PlayOptions::default(),
    });
    assert!(
        rig.shell.app.engine_arrows().is_empty(),
        "the lines belong to the start position"
    );
}

#[test]
fn clicking_a_tip_plays_the_move_through_the_add_move_path() {
    let (mut rig, run) = uci_run_with_lines();
    arrows_on(&mut rig, run);
    rig.msg(Message::Ui(UiMessage::Board(BoardEvent::PlayEngineArrow {
        from: sq("d2"),
        to: sq("d4"),
        rank: 2,
    })));
    assert_eq!(root_children_ucis(&rig), ["d2d4"]);
}

#[test]
fn a_tip_click_without_an_arrow_engine_or_with_other_squares_does_nothing() {
    let (mut rig, run) = uci_run_with_lines();
    // Arrows off: no engine draws, so nothing to play.
    rig.msg(Message::Ui(UiMessage::Board(BoardEvent::PlayEngineArrow {
        from: sq("e2"),
        to: sq("e4"),
        rank: 1,
    })));
    assert!(root_children_ucis(&rig).is_empty());
    arrows_on(&mut rig, run);
    // Squares that are not those of line 2's arrow.
    rig.msg(Message::Ui(UiMessage::Board(BoardEvent::PlayEngineArrow {
        from: sq("e2"),
        to: sq("e4"),
        rank: 2,
    })));
    assert!(root_children_ucis(&rig).is_empty());
    // A rank that does not exist.
    rig.msg(Message::Ui(UiMessage::Board(BoardEvent::PlayEngineArrow {
        from: sq("e2"),
        to: sq("e4"),
        rank: 9,
    })));
    assert!(root_children_ucis(&rig).is_empty());
}

#[test]
fn click_to_play_off_refuses_the_move() {
    let (mut rig, run) = uci_run_with_lines();
    arrows_on(&mut rig, run);
    let p = eng(&rig);
    set(
        &mut rig,
        PrefChange::Arrows(ArrowPrefs {
            click_to_play: false,
            ..p.arrows
        }),
    );
    rig.msg(Message::Ui(UiMessage::Board(BoardEvent::PlayEngineArrow {
        from: sq("e2"),
        to: sq("e4"),
        rank: 1,
    })));
    assert!(root_children_ucis(&rig).is_empty());
}

#[test]
fn a_locked_analysis_refuses_the_arrow_click() {
    let (mut rig, run) = uci_run_with_lines();
    arrows_on(&mut rig, run);
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::ToggleLock(run)));
    rig.msg(Message::Ui(UiMessage::Board(BoardEvent::PlayEngineArrow {
        from: sq("e2"),
        to: sq("e4"),
        rank: 1,
    })));
    assert!(
        root_children_ucis(&rig).is_empty(),
        "D826: a locked analysis is not played from the board"
    );
}

// ---- the "Arrows" button of the engine panel ----

#[test]
fn the_arrows_button_toggles_the_engine_of_its_section() {
    let (rig, run) = uci_run_with_lines();
    let sent = click_msgs(&rig, "Arrows");
    assert!(
        sent.iter().any(|m| matches!(
            m,
            Message::App(AppMessage::EvalBar(EvalBarMessage::ToggleArrows(r))) if *r == run
        )),
        "{sent:?}"
    );
}

#[test]
fn the_arrows_button_looks_different_when_pressed() {
    let (mut rig, run) = uci_run_with_lines();
    let before = {
        let mut sim = rig.sim(SIZE);
        mychess_gui_fingerprint(&mut sim)
    };
    arrows_on(&mut rig, run);
    let after = {
        let mut sim = rig.sim(SIZE);
        mychess_gui_fingerprint(&mut sim)
    };
    assert_ne!(before, after, "pressed + arrows on the board");
    // The pressed state is a function of arrows_engine() alone: switching
    // off restores the first picture.
    arrows_on(&mut rig, run);
    let again = {
        let mut sim = rig.sim(SIZE);
        mychess_gui_fingerprint(&mut sim)
    };
    assert_eq!(before, again);
}

fn mychess_gui_fingerprint(sim: &mut iced_test::Simulator<'_, Message>) -> String {
    common::fingerprint(sim)
}

#[test]
fn the_arrows_button_is_inert_while_the_engine_is_not_running() {
    let mut rig = Rig::new();
    let id = add_engine(&mut rig, "Fake", mychess_app::engines::Protocol::Uci);
    rig.app_msg(AppMessage::Engine(EngineMessage::Start(
        id,
        mychess_app::engines::StartMode::Analyze,
    )));
    let mut sim = rig.sim(SIZE);
    if sim.click("Arrows").is_ok() {
        let sent = strip_hover(sim.into_messages().collect());
        assert!(
            !sent
                .iter()
                .any(|m| matches!(m, Message::App(AppMessage::EvalBar(_)))),
            "a starting engine cannot draw: {sent:?}"
        );
    }
}

#[test]
fn only_the_marked_engine_draws_when_two_run() {
    let mut rig = Rig::new();
    let first = uci_run(&mut rig);
    let id2 = add_engine(&mut rig, "Second", mychess_app::engines::Protocol::Uci);
    let second = start_with(&mut rig, id2, identity("Second", Some(1..=5), true));
    started(&mut rig, first, 1);
    started(&mut rig, second, 1);
    let start = Position::new();
    info(&mut rig, first, report(&start, 1, 1, 12, 30, &["e2e4"]));
    info(&mut rig, second, report(&start, 1, 1, 12, 10, &["d2d4"]));
    arrows_on(&mut rig, second);
    assert_eq!(rig.shell.app.arrows_engine(), Some(second));
    let arrows = rig.shell.app.engine_arrows();
    assert_eq!(arrows.len(), 1);
    assert_eq!((arrows[0].from, arrows[0].to), (sq("d2"), sq("d4")));
    // Marking the first replaces the second (the last marked wins).
    arrows_on(&mut rig, first);
    assert_eq!(rig.shell.app.arrows_engine(), Some(first));
    let arrows = rig.shell.app.engine_arrows();
    assert_eq!((arrows[0].from, arrows[0].to), (sq("e2"), sq("e4")));
}
