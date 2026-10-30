//! M4-5 contract tests of the preferences (D455, D820), the actions and the
//! default shortcuts (D823).
#![allow(
    clippy::assert_is_empty,
    clippy::bool_assert_comparison,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m4_5_common;

use std::collections::HashSet;

use m4_5_common::*;
use mychess_app::{
    Action, AppState, ArrowMode, ArrowPrefs, Binding, EVAL_SCALE_RANGE, EngineViewPrefs,
    EvalBarMessage, EvalBarPrefs, KeyInput, Message, Modifiers, PrefChange, Preferences,
    PrefsIssue, SEQUENCE_LEN_RANGE, SequenceSide, Shortcuts, TrafficLightPrefs, load_preferences,
    render_preferences, update,
};
use rstest::rstest;

fn custom() -> EngineViewPrefs {
    EngineViewPrefs {
        eval_bar: EvalBarPrefs {
            show: true,
            scale: 8,
            log: true,
            ticks: true,
            label: false,
        },
        arrows: ArrowPrefs {
            enabled: true,
            mode: ArrowMode::Sequence,
            number_lines: false,
            sequence_len: 6,
            sequence_side: SequenceSide::Black,
            click_to_play: false,
        },
        traffic_lights: TrafficLightPrefs { show: true },
    }
}

fn loaded(text: &str) -> (EngineViewPrefs, Vec<PrefsIssue>) {
    let l = load_preferences(text);
    (*l.prefs.engine_view, l.issues)
}

#[test]
fn defaults_are_the_scid_ones() {
    let d = EngineViewPrefs::default();
    assert!(!d.eval_bar.show && !d.eval_bar.log && !d.eval_bar.ticks && d.eval_bar.label);
    assert_eq!(d.eval_bar.scale, 5);
    assert!(!d.arrows.enabled && d.arrows.number_lines && d.arrows.click_to_play);
    assert_eq!(d.arrows.mode, ArrowMode::Arrows);
    assert_eq!(d.arrows.sequence_len, 3);
    assert_eq!(d.arrows.sequence_side, SequenceSide::Both);
    assert!(!d.traffic_lights.show);
    assert_eq!(*Preferences::default().engine_view, d);
}

#[test]
fn ranges_are_the_ones_of_the_spec() {
    assert_eq!(EVAL_SCALE_RANGE, (1, 10));
    assert_eq!(SEQUENCE_LEN_RANGE, (1, 8));
}

#[rstest]
#[case(0, 1)]
#[case(1, 1)]
#[case(5, 5)]
#[case(10, 10)]
#[case(11, 10)]
#[case(255, 10)]
fn the_scale_change_is_limited(#[case] given: u8, #[case] kept: u8) {
    let mut state = AppState::default();
    set_eval(&mut state, |p| p.scale = given);
    assert_eq!(eval_prefs(&state).scale, kept);
}

#[rstest]
#[case(0, 1)]
#[case(1, 1)]
#[case(3, 3)]
#[case(8, 8)]
#[case(9, 8)]
#[case(255, 8)]
fn the_sequence_length_change_is_limited(#[case] given: u8, #[case] kept: u8) {
    let mut state = AppState::default();
    set_arrows(&mut state, |p| p.sequence_len = given);
    assert_eq!(arrow_prefs(&state).sequence_len, kept);
}

#[test]
fn the_toggles_change_only_their_own_field() {
    let mut state = AppState::default();
    pref(&mut state, PrefChange::EvalBar(custom().eval_bar));
    pref(&mut state, PrefChange::Arrows(custom().arrows));
    pref(
        &mut state,
        PrefChange::TrafficLights(custom().traffic_lights),
    );
    assert_eq!(*state.prefs.engine_view, custom());
    bar(&mut state, EvalBarMessage::ToggleBar);
    let mut want = custom();
    want.eval_bar.show = false;
    assert_eq!(*state.prefs.engine_view, want);
    bar(&mut state, EvalBarMessage::ToggleTrafficLights);
    want.traffic_lights.show = false;
    assert_eq!(*state.prefs.engine_view, want);
    bar(&mut state, EvalBarMessage::ToggleArrowNumbers);
    want.arrows.number_lines = true;
    assert_eq!(*state.prefs.engine_view, want);
}

#[test]
fn preferences_round_trip_through_the_file() {
    let mut prefs = Preferences::default();
    *prefs.engine_view = custom();
    let text = render_preferences(&prefs);
    let l = load_preferences(&text);
    assert_eq!(*l.prefs.engine_view, custom(), "{text}");
    assert!(l.issues.is_empty(), "{:?}", l.issues);
}

#[test]
fn default_preferences_round_trip_too() {
    let text = render_preferences(&Preferences::default());
    let l = load_preferences(&text);
    assert_eq!(*l.prefs.engine_view, EngineViewPrefs::default());
    assert!(l.issues.is_empty());
}

#[test]
fn the_file_has_its_own_engine_view_tables_and_leaves_analysis_alone() {
    let mut prefs = Preferences::default();
    *prefs.engine_view = custom();
    let text = render_preferences(&prefs);
    for table in [
        "[engine_view.eval_bar]",
        "[engine_view.arrows]",
        "[engine_view.traffic_lights]",
    ] {
        assert!(text.contains(table), "missing {table} in\n{text}");
    }
    // No bar field leaked into [analysis].
    let analysis = text
        .split("[analysis")
        .nth(1)
        .and_then(|rest| rest.split("\n[").next())
        .unwrap_or("");
    assert!(!analysis.contains("sequence_len") && !analysis.contains("ticks"));
}

#[test]
fn an_old_file_without_engine_view_is_valid_and_gets_the_defaults() {
    let old = "version = 1\n\n[board]\nanimation_ms = 200\n\n[analysis]\nmultipv = 2\n";
    let (v, issues) = loaded(old);
    assert_eq!(v, EngineViewPrefs::default());
    assert!(
        !issues
            .iter()
            .any(|i| format!("{i:?}").contains("engine_view")),
        "{issues:?}"
    );
    assert!(load_preferences("").issues.is_empty());
}

#[test]
fn a_partial_engine_view_table_keeps_the_rest_at_default() {
    let (v, issues) = loaded("[engine_view.eval_bar]\nshow = true\nscale = 7\n");
    assert!(v.eval_bar.show);
    assert_eq!(v.eval_bar.scale, 7);
    assert!(v.eval_bar.label && !v.eval_bar.log);
    assert_eq!(v.arrows, ArrowPrefs::default());
    assert!(issues.is_empty());
}

#[rstest]
#[case("scale = 5.5")]
#[case("scale = \"five\"")]
#[case("scale = true")]
#[case("scale = [1, 2]")]
fn a_scale_that_is_not_an_integer_resets_to_the_default(#[case] line: &str) {
    let (v, issues) = loaded(&format!("[engine_view.eval_bar]\nshow = true\n{line}\n"));
    assert_eq!(v.eval_bar.scale, 5);
    assert!(v.eval_bar.show, "the good field survives");
    assert!(!issues.is_empty());
}

#[rstest]
#[case(0)]
#[case(-7)]
#[case(11)]
#[case(9_999_999_999)]
#[case(i64::MIN)]
fn an_integer_scale_out_of_range_never_escapes_the_range(#[case] given: i64) {
    let (v, _) = loaded(&format!("[engine_view.eval_bar]\nscale = {given}\n"));
    let s = v.eval_bar.scale;
    assert!((1..=10).contains(&s), "{s}");
}

#[rstest]
#[case(0)]
#[case(-1)]
#[case(9)]
#[case(300)]
#[case(i64::MAX)]
fn a_sequence_length_out_of_range_never_escapes_the_range(#[case] given: i64) {
    let (v, _) = loaded(&format!("[engine_view.arrows]\nsequence_len = {given}\n"));
    assert!((1..=8).contains(&v.arrows.sequence_len));
}

#[test]
fn hostile_values_fall_back_field_by_field_with_issues() {
    let text = "\
[engine_view.eval_bar]
show = \"yes\"
log = 3
ticks = true
label = []

[engine_view.arrows]
enabled = true
mode = \"bogus\"
sequence_side = 7
number_lines = \"no\"
click_to_play = false

[engine_view.traffic_lights]
show = 1.5
";
    let (v, issues) = loaded(text);
    let d = EngineViewPrefs::default();
    assert_eq!(v.eval_bar.show, d.eval_bar.show);
    assert_eq!(v.eval_bar.log, d.eval_bar.log);
    assert_eq!(v.eval_bar.label, d.eval_bar.label);
    assert!(v.eval_bar.ticks, "valid field kept");
    assert!(v.arrows.enabled && !v.arrows.click_to_play);
    assert_eq!(v.arrows.mode, d.arrows.mode);
    assert_eq!(v.arrows.sequence_side, d.arrows.sequence_side);
    assert_eq!(v.arrows.number_lines, d.arrows.number_lines);
    assert_eq!(v.traffic_lights, d.traffic_lights);
    assert!(issues.len() >= 6, "{issues:?}");
}

#[rstest]
#[case("engine_view = 5\n")]
#[case("engine_view = \"x\"\n")]
#[case("engine_view = []\n")]
#[case("[engine_view]\neval_bar = 3\narrows = \"a\"\ntraffic_lights = []\n")]
#[case("[engine_view.eval_bar]\nshow = \"\\u0000\"\n")]
#[case("[engine_view]\n\n[engine_view.arrows]\nmode = \"\"\n")]
fn a_wrong_shape_of_the_tables_does_not_panic_and_yields_defaults(#[case] text: &str) {
    let (v, issues) = loaded(text);
    assert!((1..=10).contains(&v.eval_bar.scale) && (1..=8).contains(&v.arrows.sequence_len));
    assert_eq!(v.eval_bar.show, false);
    assert!(
        !issues.is_empty() || text.contains("[engine_view]\n\n"),
        "{text}"
    );
}

#[test]
fn the_names_of_modes_and_sides_are_read_back() {
    let (v, issues) =
        loaded("[engine_view.arrows]\nmode = \"sequence\"\nsequence_side = \"white\"\n");
    assert_eq!(v.arrows.mode, ArrowMode::Sequence);
    assert_eq!(v.arrows.sequence_side, SequenceSide::White);
    assert!(issues.is_empty());
    let (v, _) = loaded("[engine_view.arrows]\nsequence_side = \"black\"\n");
    assert_eq!(v.arrows.sequence_side, SequenceSide::Black);
}

#[test]
fn preferences_loaded_message_replaces_the_engine_view_and_refreshes_the_views() {
    let mut prefs = Preferences::default();
    prefs.engine_view.eval_bar.show = true;
    prefs.engine_view.eval_bar.scale = 2;
    let mut state = AppState::default();
    update(
        &mut state,
        Message::PreferencesLoaded(mychess_app::LoadedPreferences {
            prefs,
            issues: Vec::new(),
        }),
    );
    assert!(eval_prefs(&state).show);
    assert_eq!(eval_prefs(&state).scale, 2);
    play(&mut state, "e2e4");
    comment(&mut state, "[%eval 1]");
    assert!((state.eval_bar_view().fill.unwrap() - 0.75).abs() < 1e-4);
}

// ----------------------------------------------------------------- shortcuts

fn press(modifiers: Modifiers, ch: char) -> KeyInput {
    let b = Binding::parse(&format!(
        "{}{}",
        if modifiers == Modifiers::COMMAND_SHIFT {
            "Command+Shift+"
        } else {
            "Command+"
        },
        ch.to_ascii_uppercase()
    ))
    .unwrap();
    KeyInput {
        key: b.key,
        modifiers: b.modifiers,
    }
}

#[rstest]
#[case(Action::ToggleEvalBar, 'S')]
#[case(Action::ToggleTrafficLights, 'L')]
#[case(Action::ToggleArrowNumbers, 'M')]
fn the_new_actions_have_command_shift_defaults(#[case] action: Action, #[case] key: char) {
    let shortcuts = Shortcuts::defaults();
    let want = Binding::parse(&format!("Command+Shift+{key}")).unwrap();
    assert!(
        shortcuts
            .bindings(action)
            .iter()
            .any(|b| b.key == want.key && b.modifiers == Modifiers::COMMAND_SHIFT),
        "{action:?}: {:?}",
        shortcuts.bindings(action)
    );
    assert_eq!(
        shortcuts.lookup(&press(Modifiers::COMMAND_SHIFT, key)),
        Some(action)
    );
}

#[test]
fn no_two_actions_share_a_default_key() {
    let shortcuts = Shortcuts::defaults();
    let mut seen: HashSet<(String, Modifiers)> = HashSet::new();
    for (action, keys) in shortcuts.iter() {
        for b in keys {
            let id = (format!("{:?}", b.key), b.modifiers);
            assert!(seen.insert(id), "{action:?} reuses {b:?}");
        }
    }
}

#[test]
fn the_keys_of_the_scid_that_are_taken_stay_with_their_actions() {
    // Ctrl+Shift+C and Ctrl+Shift+N of the Scid are other commands here.
    let shortcuts = Shortcuts::defaults();
    for ch in ['C', 'N'] {
        let a = shortcuts.lookup(&press(Modifiers::COMMAND_SHIFT, ch));
        assert!(
            !matches!(
                a,
                Some(
                    Action::ToggleEvalBar
                        | Action::ToggleTrafficLights
                        | Action::ToggleArrowNumbers
                )
            ),
            "{ch}: {a:?}"
        );
    }
}

#[test]
fn message_keys_names_and_ids_of_all_actions_are_unique_and_round_trip() {
    let mut keys = HashSet::new();
    let mut names = HashSet::new();
    let shortcuts = Shortcuts::defaults();
    for (action, _) in shortcuts.iter().filter(|(a, _)| {
        !matches!(
            a,
            Action::OpenRecent(_) | Action::SelectBase(_) | Action::StartEngine(_)
        )
    }) {
        assert!(keys.insert(action.message_key()), "{action:?}");
        assert!(names.insert(action.name()), "{action:?}");
        if !matches!(
            action,
            Action::OpenRecent(_) | Action::SelectBase(_) | Action::StartEngine(_)
        ) {
            assert_eq!(Action::from_name(action.name()), Some(action));
        }
        assert!(!action.message_key().is_empty());
    }
    for a in [
        Action::ToggleEvalBar,
        Action::ToggleTrafficLights,
        Action::ToggleArrowNumbers,
    ] {
        assert!(shortcuts.iter().any(|(x, _)| x == a));
        assert_eq!(Action::from_name(a.name()), Some(a));
    }
}

#[test]
fn the_actions_drive_the_toggles() {
    let mut state = AppState::default();
    msg(&mut state, Action::ToggleEvalBar);
    assert!(eval_prefs(&state).show);
    msg(&mut state, Action::ToggleTrafficLights);
    assert!(state.prefs.engine_view.traffic_lights.show);
    msg(&mut state, Action::ToggleArrowNumbers);
    assert!(!arrow_prefs(&state).number_lines);
    msg(&mut state, Action::ToggleEvalBar);
    assert!(!eval_prefs(&state).show);
}

#[test]
fn the_toggles_do_no_blocking_work_in_update() {
    let mut state = AppState::default();
    for m in [
        EvalBarMessage::ToggleBar,
        EvalBarMessage::ToggleTrafficLights,
        EvalBarMessage::ToggleArrowNumbers,
        EvalBarMessage::CycleArrowMode,
        EvalBarMessage::CycleSequenceSide,
    ] {
        let fx = update(&mut state, Message::EvalBar(m));
        assert!(
            fx.iter().all(|e| !matches!(
                e,
                mychess_app::Effect::StartEngine { .. } | mychess_app::Effect::EngineCommand { .. }
            )),
            "{fx:?}"
        );
    }
}

#[test]
fn the_user_can_rebind_an_action_away_from_the_default() {
    let overrides: std::collections::BTreeMap<String, String> =
        [("ToggleEvalBar".to_owned(), "Command+Shift+F12".to_owned())]
            .into_iter()
            .collect();
    let (shortcuts, issues) = Shortcuts::defaults().with_overrides(&overrides);
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(
        shortcuts.lookup(&KeyInput {
            key: Binding::parse("Command+Shift+F12").unwrap().key,
            modifiers: Modifiers::COMMAND_SHIFT,
        }),
        Some(Action::ToggleEvalBar)
    );
    assert_eq!(
        shortcuts.lookup(&press(Modifiers::COMMAND_SHIFT, 'S')),
        None
    );
}
