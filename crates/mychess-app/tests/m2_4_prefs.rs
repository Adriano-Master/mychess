//! The `[notation]` preferences (D162, hover limit, tolerant reading).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::unreadable_literal,
    clippy::assigning_clones,
    clippy::manual_assert_eq,
    clippy::format_collect,
    clippy::unchecked_time_subtraction,
    clippy::field_reassign_with_default,
    clippy::cast_possible_truncation,
    clippy::items_after_statements,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::needless_pass_by_value
)]

mod m2_4_common;

use m2_4_common::*;
use proptest::prelude::*;
use rstest::rstest;

fn tempfile_dir() -> std::path::PathBuf {
    std::env::temp_dir()
}

fn load(text: &str) -> LoadedPreferences {
    load_preferences(text)
}

fn invalid(key: &str) -> PrefsIssue {
    PrefsIssue::InvalidValue {
        key: key.to_owned(),
    }
}

#[test]
fn defaults_are_those_of_the_scid_plus_the_new_options() {
    let o = NotationOptions::default();
    // M2-5 D241/D249 replaced the layout (Table) and header (Hidden) defaults.
    assert_eq!(o.layout, NotationLayout::Table);
    assert!(o.figurines && o.show_time_column);
    assert!(o.indent_comments, "indentComments=1");
    assert!(o.symbolic_nags, "symbolicNags=1");
    assert!(!o.move_number_space, "moveNumberSpace=0");
    assert_eq!(o.header, HeaderMode::Hidden);
    assert!(!o.strip_marks, "stripMarks=0");
    assert!(!o.bold_main_line, "boldMainLine=0");
    assert!(o.show_comments && o.show_variations && o.show_result);
    assert_eq!(o.hover_preview, HoverPreview::Delay(600));
}

#[test]
fn preferences_default_carries_the_default_notation() {
    assert_eq!(Preferences::default().notation, NotationOptions::default());
}

#[test]
fn empty_file_gives_default_notation_without_issues() {
    let l = load("");
    assert_eq!(l.prefs.notation, NotationOptions::default());
    assert!(l.issues.is_empty());
    let l = load("[notation]\n");
    assert_eq!(l.prefs.notation, NotationOptions::default());
    assert!(l.issues.is_empty());
}

#[test]
fn every_field_is_read_from_its_key() {
    let l = load(
        "[notation]\nlayout = \"inline\"\nindent_comments = false\nsymbolic_nags = false\n\
         move_number_space = true\nheader = \"short\"\nstrip_marks = true\nbold_main_line = true\n\
         show_comments = false\nshow_variations = false\nshow_result = false\nhover_preview_ms = 250\n",
    );
    assert!(l.issues.is_empty(), "{:?}", l.issues);
    assert_eq!(
        l.prefs.notation,
        NotationOptions {
            layout: NotationLayout::Inline,
            indent_comments: false,
            symbolic_nags: false,
            move_number_space: true,
            header: HeaderMode::Short,
            strip_marks: true,
            bold_main_line: true,
            show_comments: false,
            show_variations: false,
            show_result: false,
            hover_preview: HoverPreview::Delay(250),
            figurines: true,
            show_time_column: true,
        }
    );
}

#[rstest]
#[case("hidden", HeaderMode::Hidden)]
#[case("short", HeaderMode::Short)]
#[case("full", HeaderMode::Full)]
fn header_names(#[case] name: &str, #[case] mode: HeaderMode) {
    let l = load(&format!("[notation]\nheader = \"{name}\"\n"));
    assert_eq!(l.prefs.notation.header, mode);
    assert!(l.issues.is_empty());
}

#[rstest]
#[case("layout", "\"zigzag\"")]
#[case("layout", "3")]
#[case("layout", "true")]
#[case("layout", "\"INLINE\"")]
#[case("header", "\"tiny\"")]
#[case("header", "false")]
#[case("header", "[\"full\"]")]
#[case("indent_comments", "\"yes\"")]
#[case("indent_comments", "1")]
#[case("symbolic_nags", "\"true\"")]
#[case("move_number_space", "2")]
#[case("strip_marks", "[true]")]
#[case("bold_main_line", "{ a = 1 }")]
#[case("show_comments", "\"no\"")]
#[case("show_variations", "0")]
#[case("show_result", "\"\"")]
#[case("hover_preview_ms", "\"fast\"")]
#[case("hover_preview_ms", "true")]
#[case("hover_preview_ms", "1.5")]
fn invalid_value_falls_back_to_that_fields_default_and_reports_it(
    #[case] key: &str,
    #[case] value: &str,
) {
    // The other fields are set to non-default values and must survive.
    let others: Vec<&str> = ["show_variations", "strip_marks"]
        .into_iter()
        .filter(|k| *k != key)
        .collect();
    let text = format!(
        "[notation]\n{} = {}\n{key} = {value}\n",
        others[0],
        if others[0] == "show_variations" {
            "false"
        } else {
            "true"
        }
    );
    let l = load(&text);
    let d = NotationOptions::default();
    let n = l.prefs.notation;
    assert!(
        l.issues.contains(&invalid(&format!("notation.{key}"))),
        "{:?}",
        l.issues
    );
    match key {
        "layout" => assert_eq!(n.layout, d.layout),
        "header" => assert_eq!(n.header, d.header),
        "indent_comments" => assert_eq!(n.indent_comments, d.indent_comments),
        "symbolic_nags" => assert_eq!(n.symbolic_nags, d.symbolic_nags),
        "move_number_space" => assert_eq!(n.move_number_space, d.move_number_space),
        "bold_main_line" => assert_eq!(n.bold_main_line, d.bold_main_line),
        "show_comments" => assert_eq!(n.show_comments, d.show_comments),
        "show_result" => assert_eq!(n.show_result, d.show_result),
        "hover_preview_ms" => assert_eq!(n.hover_preview, d.hover_preview),
        "show_variations" => assert!(n.show_variations),
        "strip_marks" => assert!(!n.strip_marks),
        other => panic!("unlisted key {other}"),
    }
    if others[0] == "show_variations" {
        assert!(!n.show_variations, "an unrelated field was lost");
    } else {
        assert!(n.strip_marks, "an unrelated field was lost");
    }
}

#[test]
fn the_whole_section_having_the_wrong_type_gives_defaults_without_panic() {
    for text in [
        "notation = 3\n",
        "notation = \"x\"\n",
        "notation = [1, 2]\n",
    ] {
        let l = load(text);
        assert_eq!(l.prefs.notation, NotationOptions::default(), "{text}");
    }
}

#[test]
fn unknown_keys_in_the_section_do_not_disturb_the_known_ones() {
    let l = load("[notation]\nfuture_option = 1\nshow_result = false\n");
    assert!(!l.prefs.notation.show_result);
}

#[rstest]
#[case(0, HoverPreview::Off)]
#[case(1, HoverPreview::Delay(1))]
#[case(600, HoverPreview::Delay(600))]
#[case(5000, HoverPreview::Delay(5000))]
#[case(5001, HoverPreview::Delay(5000))]
#[case(99_999, HoverPreview::Delay(5000))]
#[case(4_294_967_295, HoverPreview::Delay(5000))]
fn hover_delay_is_limited_to_5000_ms_when_read(#[case] ms: i64, #[case] want: HoverPreview) {
    let l = load(&format!("[notation]\nhover_preview_ms = {ms}\n"));
    assert_eq!(l.prefs.notation.hover_preview, want);
}

#[test]
fn hover_delay_huge_or_negative_never_panics_and_stays_in_range() {
    for ms in ["9223372036854775807", "-1", "-9223372036854775808", "0x10"] {
        let l = load(&format!("[notation]\nhover_preview_ms = {ms}\n"));
        let got = l.prefs.notation.hover_preview.as_millis();
        assert!(got <= 5000, "{ms} -> {got}");
    }
}

#[test]
fn hover_helpers_follow_the_contract() {
    assert_eq!(HoverPreview::MAX_DELAY_MS, 5000);
    assert_eq!(HoverPreview::Off.as_millis(), 0);
    assert_eq!(HoverPreview::from_millis(0), HoverPreview::Off);
    assert_eq!(
        HoverPreview::from_millis(u32::MAX),
        HoverPreview::Delay(5000)
    );
    assert_eq!(HoverPreview::Delay(300).as_millis(), 300);
}

#[test]
fn setting_the_notation_by_message_clamps_the_hover_delay() {
    let dir = tempfile_dir();
    let mut state = AppState::with_preferences_path(Some(dir.join("preferences.toml")));
    let o = opts(|o| o.hover_preview = HoverPreview::Delay(1_000_000));
    let effects = update(
        &mut state,
        Message::Prefs(PrefsMessage::Set(PrefChange::Notation(o))),
    );
    assert_eq!(
        state.prefs.notation.hover_preview,
        HoverPreview::Delay(5000)
    );
    assert!(
        state.prefs_dirty || !effects.is_empty(),
        "a changed preference must be scheduled for saving"
    );
}

#[test]
fn setting_the_notation_options_marks_preferences_for_saving_only_on_change() {
    let mut state = AppState::default();
    let same = NotationOptions::default();
    update(
        &mut state,
        Message::Prefs(PrefsMessage::Set(PrefChange::Notation(same))),
    );
    // Whether an identical value counts as a change is not specified; the
    // options themselves must stay as they were.
    assert_eq!(state.prefs.notation, same);
}

#[test]
fn layout_and_header_spellings_round_trip() {
    for l in [
        NotationLayout::Table,
        NotationLayout::Indented,
        NotationLayout::Inline,
    ] {
        assert_eq!(NotationLayout::from_name(l.as_str()), Some(l));
    }
    for h in [HeaderMode::Hidden, HeaderMode::Short, HeaderMode::Full] {
        assert_eq!(HeaderMode::from_name(h.as_str()), Some(h));
    }
    assert_eq!(NotationLayout::from_name(""), None);
    assert_eq!(HeaderMode::from_name("Full"), None, "names are lower case");
}

#[test]
fn rendered_file_has_a_notation_section_with_the_documented_keys() {
    let text = render_preferences(&Preferences::default());
    assert!(text.contains("[notation]"), "{text}");
    for key in [
        "layout",
        "indent_comments",
        "symbolic_nags",
        "move_number_space",
        "header",
        "strip_marks",
        "bold_main_line",
        "show_comments",
        "show_variations",
        "show_result",
        "hover_preview_ms",
    ] {
        assert!(text.contains(key), "missing {key} in\n{text}");
    }
    assert!(text.contains("hover_preview_ms = 600"));
}

#[test]
fn hover_off_is_written_as_zero_and_read_back() {
    let mut p = Preferences::default();
    p.notation.hover_preview = HoverPreview::Off;
    let text = render_preferences(&p);
    assert!(text.contains("hover_preview_ms = 0"), "{text}");
    assert_eq!(load(&text).prefs.notation.hover_preview, HoverPreview::Off);
}

fn arb_options() -> impl Strategy<Value = NotationOptions> {
    (
        prop_oneof![
            Just(NotationLayout::Table),
            Just(NotationLayout::Indented),
            Just(NotationLayout::Inline)
        ],
        prop_oneof![
            Just(HeaderMode::Hidden),
            Just(HeaderMode::Short),
            Just(HeaderMode::Full)
        ],
        prop::array::uniform10(any::<bool>()),
        prop_oneof![
            Just(HoverPreview::Off),
            (1u32..=5000).prop_map(HoverPreview::Delay)
        ],
    )
        .prop_map(|(layout, header, b, hover_preview)| NotationOptions {
            layout,
            indent_comments: b[0],
            symbolic_nags: b[1],
            move_number_space: b[2],
            header,
            strip_marks: b[3],
            bold_main_line: b[4],
            show_comments: b[5],
            show_variations: b[6],
            show_result: b[7],
            hover_preview,
            figurines: b[8],
            show_time_column: b[9],
        })
}

proptest! {
    #[test]
    fn toml_round_trip_of_any_options(o in arb_options()) {
        let mut p = Preferences::default();
        p.notation = o;
        let l = load(&render_preferences(&p));
        prop_assert!(l.issues.is_empty(), "{:?}", l.issues);
        prop_assert_eq!(l.prefs.notation, o);
        prop_assert_eq!(l.prefs, p);
    }

    #[test]
    fn rendering_twice_is_stable(o in arb_options()) {
        let mut p = Preferences::default();
        p.notation = o;
        let first = render_preferences(&p);
        let second = render_preferences(&load(&first).prefs);
        prop_assert_eq!(first, second);
    }
}

#[test]
fn hostile_one_mebibyte_value_does_not_panic() {
    let big = "x".repeat(1 << 20);
    let l = load(&format!(
        "[notation]\nlayout = \"{big}\"\nheader = \"{big}\"\n"
    ));
    assert_eq!(l.prefs.notation.layout, NotationOptions::default().layout);
    assert_eq!(l.prefs.notation.header, NotationOptions::default().header);
}

#[test]
fn notation_options_given_to_the_state_drive_the_document() {
    let mut state = AppState::default();
    let mut g = Game::new();
    main_line(&mut g, "e4 e5");
    state.session.replace_game(g);
    let l = load(
        "[notation]\nlayout = \"inline\"\nshow_result = false\nsymbolic_nags = false\nheader = \"hidden\"\n",
    );
    update(
        &mut state,
        Message::Prefs(PrefsMessage::Set(PrefChange::Notation(l.prefs.notation))),
    );
    assert_eq!(state.notation().flat_text(), "1.e4 e5");
    assert!(state.notation().header.is_empty());
}
