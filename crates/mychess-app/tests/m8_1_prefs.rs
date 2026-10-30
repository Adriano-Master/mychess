//! M8-1a contract tests of the `[play]` preferences (spec D1242, D1253):
//! tolerant reading, clamping, round trip and the `PlayConfig` they make.
#![allow(
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::path::PathBuf;

use mychess_app::engines::EngineId;
use mychess_app::{
    BONUS_RANGE, BookSelection, ColorChoice, MAX_OPENING_LINES, MAX_PLAYER_NAME_CHARS,
    MINUTES_RANGE, MOVETIME_RANGE, NODES_RANGE, PLAY_DEPTH_RANGE, PlayBookPref, PlayColorPref,
    PlayModePref, PlayPrefs, PlayStartPref, Preferences, StartFrom, TimeMode, load_preferences,
    render_preferences,
};
use proptest::prelude::*;
use rstest::rstest;

fn play_of(text: &str) -> PlayPrefs {
    *load_preferences(text).prefs.play
}

fn full() -> PlayPrefs {
    PlayPrefs {
        engine: Some(4),
        color: PlayColorPref::Random,
        mode: PlayModePref::TimeBonus,
        white_minutes: 10,
        white_bonus: 5,
        black_minutes: 7,
        black_bonus: 2,
        depth: 8,
        nodes: 500,
        movetime: 12,
        start: PlayStartPref::Opening,
        opening: "Sicilian".to_owned(),
        openings: vec!["Sicilian: e4 c5".to_owned(), "QGD: d4 d5 c4 e6".to_owned()],
        use_book: true,
        book_path: Some(PathBuf::from("books").join("main.bin")),
        book_selection: PlayBookPref::Best,
        limit_strength: true,
        elo: 1800,
        use_skill: true,
        skill: 4,
        player_name: "Ana Çelik".to_owned(),
        ponder: true,
        store_clock: true,
        coach: true,
        coach_engine: Some(6),
        coach_depth: 9,
        coach_seconds: 4,
        coach_hint: true,
        coach_numbers: false,
        tactical: mychess_app::tactical_game::TacticalPrefs::default(),
    }
}

fn with(play: PlayPrefs) -> Preferences {
    Preferences {
        play: Box::new(play),
        ..Preferences::default()
    }
}

// ---- defaults and absence ----

#[test]
fn the_defaults_follow_the_scid_dialog() {
    let d = PlayPrefs::default();
    assert_eq!(d.mode, PlayModePref::MoveTime);
    assert_eq!((d.depth, d.nodes, d.movetime), (3, 1000, 6));
    assert_eq!(d.color, PlayColorPref::White);
    assert_eq!(d.start, PlayStartPref::NewGame);
    assert!(!d.use_book && !d.limit_strength && !d.use_skill);
    assert_eq!(d.engine, None);
    assert_eq!(*Preferences::default().play, d);
}

#[test]
fn a_file_without_the_section_opens_silently_with_the_defaults() {
    let loaded = load_preferences("[board]\nanimation_ms = 100\n");
    assert_eq!(*loaded.prefs.play, PlayPrefs::default());
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
}

#[test]
fn an_empty_section_is_the_defaults() {
    let loaded = load_preferences("[play]\n");
    assert_eq!(*loaded.prefs.play, PlayPrefs::default());
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
}

// ---- round trip ----

#[test]
fn every_field_survives_a_round_trip() {
    let text = render_preferences(&with(full()));
    assert!(text.contains("[play]"), "{text}");
    let loaded = load_preferences(&text);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(*loaded.prefs.play, full());
}

#[test]
fn rendering_a_loaded_file_again_gives_the_same_text() {
    let first = render_preferences(&with(full()));
    let second = render_preferences(&load_preferences(&first).prefs);
    assert_eq!(first, second);
}

#[test]
fn the_defaults_survive_a_round_trip() {
    let text = render_preferences(&Preferences::default());
    let loaded = load_preferences(&text);
    assert_eq!(*loaded.prefs.play, PlayPrefs::default());
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
}

#[test]
fn text_that_needs_escaping_survives_a_round_trip() {
    let play = PlayPrefs {
        player_name: "A \"quoted\" \\ name\ttab".to_owned(),
        openings: vec!["Name \"x\": e4 e5".to_owned()],
        ..PlayPrefs::default()
    };
    let loaded = load_preferences(&render_preferences(&with(play.clone())));
    assert_eq!(loaded.prefs.play.player_name, play.player_name);
    assert_eq!(loaded.prefs.play.openings, play.openings);
}

proptest! {
    #[test]
    fn numbers_in_range_round_trip(
        wm in MINUTES_RANGE.0..=MINUTES_RANGE.1,
        wb in BONUS_RANGE.0..=BONUS_RANGE.1,
        bm in MINUTES_RANGE.0..=MINUTES_RANGE.1,
        bb in BONUS_RANGE.0..=BONUS_RANGE.1,
        depth in PLAY_DEPTH_RANGE.0..=PLAY_DEPTH_RANGE.1,
        nodes in NODES_RANGE.0..=NODES_RANGE.1,
        movetime in MOVETIME_RANGE.0..=MOVETIME_RANGE.1,
        mode in 0usize..4,
        color in 0usize..3,
        name in "[A-Za-z0-9 çãé]{0,40}",
    ) {
        let play = PlayPrefs {
            white_minutes: wm,
            white_bonus: wb,
            black_minutes: bm,
            black_bonus: bb,
            depth,
            nodes,
            movetime,
            mode: [
                PlayModePref::TimeBonus,
                PlayModePref::Depth,
                PlayModePref::Nodes,
                PlayModePref::MoveTime,
            ][mode],
            color: [PlayColorPref::White, PlayColorPref::Black, PlayColorPref::Random][color],
            player_name: name,
            ..PlayPrefs::default()
        };
        let loaded = load_preferences(&render_preferences(&with(play.clone())));
        prop_assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
        prop_assert_eq!(*loaded.prefs.play, play);
    }

    /// Whatever numbers a hand-edited file holds, the result is in range.
    #[test]
    fn any_numbers_end_up_inside_the_ranges(
        a in any::<i64>(), b in any::<i64>(), c in any::<i64>(), d in any::<i64>(),
        e in any::<i64>(), f in any::<i64>(), g in any::<i64>(),
    ) {
        let text = format!(
            "[play]\nwhite_minutes = {a}\nwhite_bonus = {b}\nblack_minutes = {c}\nblack_bonus = {d}\ndepth = {e}\nnodes = {f}\nmovetime = {g}\n"
        );
        let p = play_of(&text);
        prop_assert!((MINUTES_RANGE.0..=MINUTES_RANGE.1).contains(&p.white_minutes));
        prop_assert!((MINUTES_RANGE.0..=MINUTES_RANGE.1).contains(&p.black_minutes));
        prop_assert!((BONUS_RANGE.0..=BONUS_RANGE.1).contains(&p.white_bonus));
        prop_assert!((BONUS_RANGE.0..=BONUS_RANGE.1).contains(&p.black_bonus));
        prop_assert!((PLAY_DEPTH_RANGE.0..=PLAY_DEPTH_RANGE.1).contains(&p.depth));
        prop_assert!((NODES_RANGE.0..=NODES_RANGE.1).contains(&p.nodes));
        prop_assert!((MOVETIME_RANGE.0..=MOVETIME_RANGE.1).contains(&p.movetime));
    }
}

// ---- tolerant reading (D1242) ----

#[test]
fn a_bad_value_resets_only_its_field_and_reports_an_issue() {
    let loaded = load_preferences(
        "[play]\ncolor = \"green\"\nmode = \"depth\"\ndepth = 99\nnodes = 1\nmovetime = \"x\"\nwhite_minutes = 0\nplayer_name = \"Zé\"\n",
    );
    let p = &loaded.prefs.play;
    assert_eq!(p.color, PlayColorPref::White, "unknown colour: the default");
    assert_eq!(p.mode, PlayModePref::Depth, "the good neighbour is kept");
    assert_eq!(p.depth, PLAY_DEPTH_RANGE.1);
    assert_eq!(p.nodes, NODES_RANGE.0);
    assert_eq!(p.movetime, 6);
    assert_eq!(p.white_minutes, MINUTES_RANGE.0);
    assert_eq!(p.player_name, "Zé");
    assert!(!loaded.issues.is_empty());
}

#[rstest]
#[case("[play]\ncolor = 3\n")]
#[case("[play]\nmode = true\n")]
#[case("[play]\nstart = \"somewhere\"\n")]
#[case("[play]\nbook_selection = []\n")]
#[case("[play]\nuse_book = \"yes\"\n")]
#[case("[play]\nlimit_strength = 1\n")]
#[case("[play]\nengine = -1\n")]
#[case("[play]\nengine = \"abc\"\n")]
#[case("[play]\nopenings = \"not a list\"\n")]
#[case("[play]\nopenings = [1, 2, 3]\n")]
#[case("[play]\nbook_path = 5\n")]
#[case("[play]\nelo = -50\n")]
#[case("[play]\nskill = 1e99\n")]
#[case("[play]\nplayer_name = 9\n")]
#[case("[play]\nunknown_key = 1\n")]
#[case("play = 3\n")]
#[case("play = \"text\"\n")]
#[case("[[play]]\nmode = \"depth\"\n")]
fn hostile_values_never_panic_and_leave_a_usable_configuration(#[case] text: &str) {
    let loaded = load_preferences(text);
    let p = &loaded.prefs.play;
    // The result must always build a valid configuration.
    let config = p.to_config(EngineId(1));
    assert_eq!(config.mode, config.mode.normalized(), "{text}");
    // And the rest of the preferences is untouched.
    assert_eq!(loaded.prefs.board, Preferences::default().board);
}

#[test]
fn a_broken_play_section_does_not_spoil_the_neighbours() {
    let loaded = load_preferences("[board]\nanimation_ms = 123\n[play]\ncolor = 7\n");
    assert_eq!(loaded.prefs.board.animation_ms, 123);
}

#[test]
fn a_file_that_is_not_toml_gives_the_defaults() {
    let loaded = load_preferences("[play\nmode = ");
    assert_eq!(*loaded.prefs.play, PlayPrefs::default());
}

#[rstest]
#[case("timebonus", PlayModePref::TimeBonus)]
#[case("depth", PlayModePref::Depth)]
#[case("nodes", PlayModePref::Nodes)]
#[case("movetime", PlayModePref::MoveTime)]
fn every_mode_spelling_is_read(#[case] spelling: &str, #[case] mode: PlayModePref) {
    assert_eq!(
        play_of(&format!("[play]\nmode = \"{spelling}\"\n")).mode,
        mode
    );
    assert_eq!(mode.as_str(), spelling);
    assert_eq!(PlayModePref::from_name(spelling), Some(mode));
}

#[rstest]
#[case("white", PlayColorPref::White)]
#[case("black", PlayColorPref::Black)]
#[case("random", PlayColorPref::Random)]
fn every_colour_spelling_is_read(#[case] spelling: &str, #[case] color: PlayColorPref) {
    assert_eq!(
        play_of(&format!("[play]\ncolor = \"{spelling}\"\n")).color,
        color
    );
    assert_eq!(color.as_str(), spelling);
}

#[test]
fn the_spelling_is_case_sensitive_and_unknown_names_are_none() {
    assert_eq!(PlayModePref::from_name("Depth"), None);
    assert_eq!(PlayColorPref::from_name(""), None);
    assert_eq!(PlayStartPref::from_name("current "), None);
    assert_eq!(PlayBookPref::from_name("best"), Some(PlayBookPref::Best));
}

#[test]
fn the_list_of_openings_and_the_name_are_bounded() {
    let many: Vec<String> = (0..MAX_OPENING_LINES + 100)
        .map(|i| format!("Line {i}: e4 e5"))
        .collect();
    let long = "ç".repeat(MAX_PLAYER_NAME_CHARS + 50);
    let text = render_preferences(&with(PlayPrefs {
        openings: many,
        player_name: long,
        ..PlayPrefs::default()
    }));
    let p = play_of(&text);
    assert!(p.openings.len() <= MAX_OPENING_LINES);
    assert_eq!(p.player_name.chars().count(), MAX_PLAYER_NAME_CHARS);
    assert!(p.player_name.is_char_boundary(p.player_name.len()));
}

// ---- the configuration they make (D1253) ----

#[rstest]
#[case(PlayModePref::TimeBonus, TimeMode::TimeBonus { white_minutes: 10, white_bonus: 5, black_minutes: 7, black_bonus: 2 })]
#[case(PlayModePref::Depth, TimeMode::Depth(8))]
#[case(PlayModePref::Nodes, TimeMode::Nodes(500))]
#[case(PlayModePref::MoveTime, TimeMode::MoveTime(12))]
fn the_mode_picks_its_own_numbers(#[case] mode: PlayModePref, #[case] expected: TimeMode) {
    let p = PlayPrefs { mode, ..full() };
    assert_eq!(p.time_mode(), expected);
    assert_eq!(p.to_config(EngineId(9)).mode, expected);
}

#[test]
fn the_configuration_carries_the_engine_colour_and_strength() {
    let config = full().to_config(EngineId(3));
    assert_eq!(config.engine, EngineId(3));
    assert_eq!(config.color, ColorChoice::Random);
    assert_eq!(config.strength.elo, Some(1800));
    assert_eq!(config.strength.skill, Some(4));
    let book = config.book.expect("book");
    assert_eq!(book.path, PathBuf::from("books").join("main.bin"));
    assert_eq!(book.selection, BookSelection::Best);
    assert!(matches!(config.start, StartFrom::Opening(ref l) if l.name == "Sicilian"));
}

#[test]
fn strength_off_means_none() {
    let p = PlayPrefs {
        limit_strength: false,
        use_skill: false,
        ..full()
    };
    let c = p.to_config(EngineId(1));
    assert_eq!((c.strength.elo, c.strength.skill), (None, None));
}

#[test]
fn the_book_is_used_only_with_the_switch_on_and_a_path() {
    let off = PlayPrefs {
        use_book: false,
        ..full()
    };
    assert!(off.to_config(EngineId(1)).book.is_none());
    let no_path = PlayPrefs {
        book_path: None,
        ..full()
    };
    assert!(no_path.to_config(EngineId(1)).book.is_none());
}

#[rstest]
#[case(PlayBookPref::Best, BookSelection::Best)]
#[case(PlayBookPref::Weighted, BookSelection::Weighted)]
fn the_book_selection_maps(#[case] pref: PlayBookPref, #[case] expected: BookSelection) {
    let p = PlayPrefs {
        book_selection: pref,
        ..full()
    };
    assert_eq!(p.to_config(EngineId(1)).book.unwrap().selection, expected);
}

#[test]
fn the_start_follows_the_preference() {
    let current = PlayPrefs {
        start: PlayStartPref::Current,
        ..full()
    };
    assert_eq!(
        current.to_config(EngineId(1)).start,
        StartFrom::CurrentPosition
    );
    let new = PlayPrefs {
        start: PlayStartPref::NewGame,
        ..full()
    };
    assert_eq!(new.to_config(EngineId(1)).start, StartFrom::NewGame);
}

#[test]
fn an_opening_that_left_the_list_falls_back_to_a_new_game() {
    let p = PlayPrefs {
        opening: "Gone".to_owned(),
        ..full()
    };
    assert_eq!(p.to_config(EngineId(1)).start, StartFrom::NewGame);
    let empty_name = PlayPrefs {
        opening: String::new(),
        ..full()
    };
    assert_eq!(empty_name.to_config(EngineId(1)).start, StartFrom::NewGame);
}

#[test]
fn the_opening_list_skips_lines_that_do_not_parse() {
    let p = PlayPrefs {
        openings: vec![
            "Good: e4 e5".to_owned(),
            "junk".to_owned(),
            ": e4".to_owned(),
            "Second: d4 d5".to_owned(),
        ],
        ..PlayPrefs::default()
    };
    let names: Vec<_> = p.opening_lines().into_iter().map(|l| l.name).collect();
    assert_eq!(names, ["Good", "Second"]);
}

#[test]
fn the_configuration_is_normalized_even_if_the_preferences_were_set_in_memory() {
    let p = PlayPrefs {
        mode: PlayModePref::Depth,
        depth: 500,
        ..PlayPrefs::default()
    };
    assert_eq!(p.to_config(EngineId(1)).mode, TimeMode::Depth(20));
    let p = PlayPrefs {
        mode: PlayModePref::TimeBonus,
        white_minutes: 0,
        black_minutes: 9999,
        white_bonus: 999,
        ..PlayPrefs::default()
    };
    assert_eq!(
        p.to_config(EngineId(1)).mode,
        TimeMode::TimeBonus {
            white_minutes: 1,
            white_bonus: 60,
            black_minutes: 120,
            black_bonus: 3
        }
    );
}

#[test]
fn sanitized_clamps_everything_and_is_idempotent() {
    let wild = PlayPrefs {
        white_minutes: 0,
        black_minutes: u32::MAX,
        white_bonus: u32::MAX,
        black_bonus: 0,
        depth: 0,
        nodes: 0,
        movetime: u32::MAX,
        elo: u32::MAX,
        skill: u32::MAX,
        ..PlayPrefs::default()
    };
    let once = wild.sanitized();
    assert_eq!(once.white_minutes, MINUTES_RANGE.0);
    assert_eq!(once.black_minutes, MINUTES_RANGE.1);
    assert_eq!(once.white_bonus, BONUS_RANGE.1);
    assert_eq!(once.depth, PLAY_DEPTH_RANGE.0);
    assert_eq!(once.nodes, NODES_RANGE.0);
    assert_eq!(once.movetime, MOVETIME_RANGE.1);
    assert_eq!(once.clone().sanitized(), once);
}
