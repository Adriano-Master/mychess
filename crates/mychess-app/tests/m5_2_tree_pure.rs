//! M5-2a contract tests of the pure parts: the tricolour bar (D969), the
//! `[tree]` preferences (D970) and the shortcut (D971, D980).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp,
    clippy::assert_is_empty
)]

mod m5_2_common;

use mychess_app::{
    Action, BarSegments, Binding, Key, KeyInput, Modifiers, Preferences, Shortcuts, TreeFastMode,
    TreeIssue, TreePrefs, TreeSort, bar_segments, load_preferences, render_preferences,
};
use mychess_db::tree::TreeError;
use proptest::prelude::*;
use rstest::rstest;

// ---- bar_segments (D969), computed by hand from the formula ----
// success = permille / 10; white = (success - draw/2) * 0.6 + 1, drawn if
// > 0.1; black = (success + draw/2) * 0.6 + 1, drawn if < 60.9.

fn near(a: Option<f32>, b: Option<f32>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() < 1e-3,
        (None, None) => true,
        _ => false,
    }
}

#[rstest]
// 100 % wins for White: white fills the bar, no black.
#[case(1000, 0, Some(61.0), None)]
// 0 %: only the one unit of border is white, black starts there.
#[case(0, 0, Some(1.0), Some(1.0))]
// 50 % made only of draws: grey over the whole bar.
#[case(500, 100, Some(1.0), None)]
// 54.7 % with 37 % draws: 0.6 * (54.7 -+ 18.5) + 1.
#[case(547, 37, Some(22.72), Some(44.92))]
// 50 % without draws: the halves meet in the middle.
#[case(500, 0, Some(31.0), Some(31.0))]
// 25 % with 50 % draws: white 0.6 * 0 + 1, black 0.6 * 50 + 1.
#[case(250, 50, Some(1.0), Some(31.0))]
fn bar_follows_the_scid_formula(
    #[case] permille: u32,
    #[case] draw: u32,
    #[case] white: Option<f32>,
    #[case] black: Option<f32>,
) {
    let bar = bar_segments(permille, draw);
    assert!(near(bar.white_end, white), "{bar:?}");
    assert!(near(bar.black_start, black), "{bar:?}");
}

#[test]
fn white_is_dropped_when_its_end_is_not_above_a_tenth() {
    // s = 0, draw 4: (0 - 2) * 0.6 + 1 = -0.2 (not above 0.1).
    assert_eq!(bar_segments(0, 4).white_end, None);
    // s = 0, draw 2: (0 - 1) * 0.6 + 1 = 0.4 (above 0.1).
    assert!(near(bar_segments(0, 2).white_end, Some(0.4)));
}

#[test]
fn black_is_dropped_when_its_start_is_not_below_60_9() {
    // 99.9 %: 99.9 * 0.6 + 1 = 60.94 (not below 60.9).
    assert_eq!(bar_segments(999, 0).black_start, None);
    // 99.8 %: 99.8 * 0.6 + 1 = 60.88 (below 60.9).
    assert!(near(bar_segments(998, 0).black_start, Some(60.88)));
    assert!(near(bar_segments(997, 0).black_start, Some(60.82)));
}

#[test]
fn the_bar_width_is_the_sixty_units_plus_the_border() {
    assert_eq!(mychess_app::BAR_WIDTH, 61.0);
    assert_eq!(
        bar_segments(1000, 0).white_end,
        Some(mychess_app::BAR_WIDTH)
    );
}

proptest! {
    #[test]
    fn bar_segments_stay_ordered_and_inside_the_bar(permille in 0u32..=1000, draw in 0u32..=100) {
        let BarSegments { white_end, black_start } = bar_segments(permille, draw);
        if let Some(w) = white_end {
            prop_assert!(w > 0.1 && w <= 61.0 + 1e-3, "white {w}");
        }
        if let Some(b) = black_start {
            prop_assert!((1.0 - 1e-3..60.9).contains(&b), "black {b}");
        }
        if let (Some(w), Some(b)) = (white_end, black_start) {
            prop_assert!(w <= b + 1e-3, "white {w} after black {b}");
        }
    }
}

// ---- preferences [tree] (D970) ----

#[test]
fn defaults_are_short_frequency_slow_without_adjust() {
    let d = TreePrefs::default();
    assert!(d.short);
    assert_eq!(d.sort, TreeSort::Frequency);
    assert_eq!(d.fast_mode, TreeFastMode::Slow);
    assert!(!d.auto_adjust);
    assert_eq!(Preferences::default().tree, d);
}

#[test]
fn an_empty_file_gives_the_defaults() {
    let loaded = load_preferences("");
    assert_eq!(loaded.prefs.tree, TreePrefs::default());
}

#[rstest]
#[case(TreeSort::Alphabetical, TreeFastMode::Slow, false, false)]
#[case(TreeSort::Eco, TreeFastMode::Fast, true, true)]
#[case(TreeSort::Frequency, TreeFastMode::FastThenSlow, false, true)]
#[case(TreeSort::Score, TreeFastMode::Slow, true, false)]
fn the_tree_table_round_trips(
    #[case] sort: TreeSort,
    #[case] fast_mode: TreeFastMode,
    #[case] short: bool,
    #[case] auto_adjust: bool,
) {
    let prefs = Preferences {
        tree: TreePrefs {
            short,
            sort,
            fast_mode,
            auto_adjust,
            ..TreePrefs::default()
        },
        ..Preferences::default()
    };
    let text = render_preferences(&prefs);
    assert!(text.contains("[tree]"), "{text}");
    let loaded = load_preferences(&text);
    assert_eq!(loaded.prefs.tree, prefs.tree);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
}

#[test]
fn the_file_uses_the_documented_spellings_and_no_extra_keys() {
    let prefs = Preferences {
        tree: TreePrefs {
            short: false,
            sort: TreeSort::Alphabetical,
            fast_mode: TreeFastMode::FastThenSlow,
            auto_adjust: true,
            ..TreePrefs::default()
        },
        ..Preferences::default()
    };
    let text = render_preferences(&prefs);
    assert!(text.contains("fast_then_slow"), "{text}");
    assert!(text.contains("alphabetical"), "{text}");
    // D970: not persisted (session state or omitted on purpose).
    for key in [
        "autorefresh",
        "visible",
        "locked",
        "training",
        "show_bar",
        "show_frame",
    ] {
        assert!(!text.contains(key), "{key} must not be persisted: {text}");
    }
}

#[test]
fn a_bad_value_costs_only_its_own_field() {
    let loaded = load_preferences(
        "[tree]\nshort = 3\nsort = \"x\"\nfast_mode = \"fast\"\nauto_adjust = true\n",
    );
    let want = TreePrefs {
        fast_mode: TreeFastMode::Fast,
        auto_adjust: true,
        ..TreePrefs::default()
    };
    assert_eq!(loaded.prefs.tree, want);
    assert_eq!(loaded.issues.len(), 2, "{:?}", loaded.issues);
}

#[rstest]
#[case("[tree]\nsort = \"Frequency\"\n")] // spelling is case-sensitive
#[case("[tree]\nsort = 7\n")]
#[case("[tree]\nfast_mode = \"both\"\n")]
#[case("[tree]\nshort = \"yes\"\nauto_adjust = 1\n")]
#[case("tree = 1\n")]
#[case("tree = [1, 2]\n")]
#[case("[tree]\n")]
fn garbage_never_fails_the_load_and_keeps_the_defaults(#[case] text: &str) {
    let loaded = load_preferences(text);
    assert_eq!(loaded.prefs.tree, TreePrefs::default(), "{text}");
}

#[test]
fn a_partial_table_keeps_the_other_defaults() {
    let loaded = load_preferences("[tree]\nsort = \"score\"\n");
    assert_eq!(
        loaded.prefs.tree,
        TreePrefs {
            sort: TreeSort::Score,
            ..TreePrefs::default()
        }
    );
    assert!(loaded.issues.is_empty());
}

#[test]
fn names_of_sorts_and_modes_round_trip_and_reject_other_cases() {
    for s in TreeSort::ALL {
        assert_eq!(TreeSort::from_name(s.name()), Some(s));
    }
    for m in TreeFastMode::ALL {
        assert_eq!(TreeFastMode::from_name(m.name()), Some(m));
    }
    assert_eq!(TreeSort::from_name(""), None);
    assert_eq!(TreeFastMode::from_name("FAST"), None);
    assert_eq!(TreeSort::default(), TreeSort::Frequency);
    assert_eq!(TreeFastMode::default(), TreeFastMode::Slow);
}

// ---- engine errors to issues (D972) ----

#[test]
fn engine_errors_map_to_the_issues_of_the_panel() {
    assert_eq!(TreeIssue::from(TreeError::Cancelled), TreeIssue::Cancelled);
    assert_eq!(
        TreeIssue::from(TreeError::TooManyMoves),
        TreeIssue::TooManyMoves
    );
    assert_eq!(
        TreeIssue::from(TreeError::UnsupportedPosition),
        TreeIssue::UnsupportedPosition
    );
    assert_eq!(
        TreeIssue::from(TreeError::FilterSize {
            filter: 3,
            games: 5
        }),
        TreeIssue::FilterSize
    );
}

// ---- shortcut (D971, D980) ----

fn key(c: char, command: bool, shift: bool) -> KeyInput {
    KeyInput {
        key: Key::Char(c),
        modifiers: Modifiers {
            command,
            shift,
            alt: false,
        },
    }
}

#[test]
fn the_tree_pane_is_command_shift_t() {
    let s = Shortcuts::defaults();
    let expected = Binding::parse("Command+Shift+T").unwrap();
    assert!(
        s.bindings(Action::ToggleTreePane)
            .iter()
            .any(|b| b.key == expected.key && b.modifiers == expected.modifiers),
        "{:?}",
        s.bindings(Action::ToggleTreePane)
    );
    assert_eq!(
        s.lookup(&key('t', true, true)),
        Some(Action::ToggleTreePane)
    );
}

#[test]
fn command_t_is_still_the_trial_mode() {
    let s = Shortcuts::defaults();
    assert_eq!(
        s.lookup(&key('t', true, false)),
        Some(Action::ToggleTrialMode)
    );
}

#[test]
fn the_action_has_a_stable_name_and_a_message_key() {
    assert_eq!(Action::ToggleTreePane.name(), "ToggleTreePane");
    assert_eq!(
        Action::from_name("ToggleTreePane"),
        Some(Action::ToggleTreePane)
    );
    assert!(!Action::ToggleTreePane.message_key().is_empty());
}

#[test]
fn no_default_binding_is_shared_after_adding_the_tree_pane() {
    let s = Shortcuts::defaults();
    let mut seen: Vec<(Key, Modifiers, Action)> = Vec::new();
    for (action, bindings) in s.iter() {
        for b in bindings {
            if let Some((_, _, other)) = seen
                .iter()
                .find(|(k, m, _)| *k == b.key && *m == b.modifiers)
            {
                panic!("{action:?} and {other:?} share {b}");
            }
            seen.push((b.key, b.modifiers, action));
        }
    }
}

#[test]
fn the_tree_pane_shortcut_can_be_rebound_and_turned_off() {
    let mut overrides = std::collections::BTreeMap::new();
    overrides.insert("ToggleTreePane".to_owned(), String::new());
    let (s, issues) = Shortcuts::defaults().with_overrides(&overrides);
    assert!(issues.is_empty(), "{issues:?}");
    assert!(s.bindings(Action::ToggleTreePane).is_empty());
    assert_eq!(s.lookup(&key('t', true, true)), None);
}

#[test]
fn the_tree_pane_is_a_front_end_action_that_changes_nothing_in_update() {
    use mychess_app::{AppState, Message, update};
    let mut state = AppState::default();
    let effects = update(&mut state, Message::Action(Action::ToggleTreePane));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(!state.tree().is_open(), "the layout decides, not update");
}
