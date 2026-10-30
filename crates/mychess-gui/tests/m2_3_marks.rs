//! M2-3 marks: arrow geometry (rule 43, Scid `GetArrowCoords`), boxes
//! (`GetBox`), colors by modifier and by name (rules 46 and 48), and the
//! mark round trip through the core (`toggle_mark` / `embedded_commands`).

#![allow(clippy::assert_is_empty, clippy::float_cmp, clippy::unnecessary_wraps)]

use iced::{Color, Point, Rectangle, Size};
use mychess_core::{
    BoardMark, EmbeddedCommand, MarkKind, Square, chessbase_color, embedded_commands, toggle_mark,
};
use mychess_gui::theme::board_theme::{BoardTheme, BoardThemeId};
use mychess_gui::theme::mark_colors::{
    DEFAULT_MARK_COLOR_NAME, mark_color_or_default, parse_mark_color,
};
use mychess_gui::widgets::board::geometry::{BoardLayout, CoordsMode, LayoutOptions, Orientation};
use mychess_gui::widgets::board::interaction::Mods;
use mychess_gui::widgets::board::marks::{
    CIRCLE_PORTION, DEFAULT_SHRINK, MarkBrush, TEXT_PORTION, arrow_geometry, color_for_modifiers,
    inner_box,
};
use proptest::prelude::*;
use rstest::rstest;

fn sq(name: &str) -> Square {
    name.parse().expect("square")
}

fn layout() -> BoardLayout {
    BoardLayout::compute(
        Size::new(800.0, 800.0),
        LayoutOptions {
            coords: CoordsMode::None,
        },
    )
    .expect("fits")
}

fn near(a: Point, b: (f32, f32)) {
    assert!(
        (a.x - b.0).abs() < 0.01 && (a.y - b.1).abs() < 0.01,
        "{a:?} != {b:?}"
    );
}

// ---------------------------------------------------------------------------
// Arrow geometry (rule 43)
// ---------------------------------------------------------------------------

#[test]
fn m2_3_default_shrink_is_the_scid_value() {
    assert!((DEFAULT_SHRINK - 0.6).abs() < f32::EPSILON);
}

#[test]
fn m2_3_arrow_e2_e4_has_tail_and_tip_30_px_from_the_centers() {
    let l = layout();
    assert_eq!(l.s, 100.0);
    let a =
        arrow_geometry(&l, sq("e2"), sq("e4"), Orientation::White, DEFAULT_SHRINK).expect("arrow");
    // c(e2) = (450, 650), c(e4) = (450, 450).
    near(a.tail, (450.0, 620.0));
    near(a.tip, (450.0, 480.0));
    assert_eq!(a.head[0], a.tip, "the head's vertex is the tip");
}

#[test]
fn m2_3_arrow_knight_move_uses_a_shrink_lambda_of_0_15() {
    let l = layout();
    let a = arrow_geometry(&l, sq("b1"), sq("c3"), Orientation::White, 0.6).expect("arrow");
    // c(b1) = (150, 750), c(c3) = (250, 550); d = (100, -200); 0.15 * d.
    near(a.tail, (165.0, 720.0));
    near(a.tip, (235.0, 580.0));
}

#[test]
fn m2_3_arrow_long_diagonal() {
    let l = layout();
    let a = arrow_geometry(&l, sq("a1"), sq("h8"), Orientation::White, 0.6).expect("arrow");
    // |dx| = |dy| uses the horizontal edge: lambda = 1/14, k = 0.6/14.
    near(a.tail, (80.0, 720.0));
    near(a.tip, (720.0, 80.0));
}

#[test]
fn m2_3_arrow_horizontal_uses_the_vertical_edge() {
    let l = layout();
    // a1 -> h1: |dx| > |dy|; lambda = 50/700, k = 0.6 * lambda; k*d = 30.
    let a = arrow_geometry(&l, sq("a1"), sq("h1"), Orientation::White, 0.6).expect("arrow");
    near(a.tail, (80.0, 750.0));
    near(a.tip, (720.0, 750.0));
    // Right to left.
    let a = arrow_geometry(&l, sq("h1"), sq("a1"), Orientation::White, 0.6).expect("arrow");
    near(a.tail, (720.0, 750.0));
    near(a.tip, (80.0, 750.0));
}

#[test]
fn m2_3_arrow_between_adjacent_squares() {
    let l = layout();
    let a = arrow_geometry(&l, sq("e2"), sq("e3"), Orientation::White, 0.6).expect("arrow");
    near(a.tail, (450.0, 620.0));
    near(a.tip, (450.0, 580.0));
    let a = arrow_geometry(&l, sq("e2"), sq("f3"), Orientation::White, 0.6).expect("arrow");
    near(a.tail, (480.0, 620.0));
    near(a.tip, (520.0, 580.0));
}

#[test]
fn m2_3_shrink_above_0_667_is_limited_for_adjacent_squares_only() {
    let l = layout();
    let adj = arrow_geometry(&l, sq("e2"), sq("e3"), Orientation::White, 0.95).expect("arrow");
    // k = 0.667 * 0.5 -> tail at 650 - 33.35.
    near(adj.tail, (450.0, 650.0 - 33.35));
    let far = arrow_geometry(&l, sq("e2"), sq("e4"), Orientation::White, 0.9).expect("arrow");
    near(far.tail, (450.0, 650.0 - 0.9 * 0.25 * 200.0));
}

#[test]
fn m2_3_arrow_to_the_same_square_does_not_exist() {
    let l = layout();
    for o in [Orientation::White, Orientation::Black] {
        assert!(arrow_geometry(&l, sq("e4"), sq("e4"), o, 0.6).is_none());
    }
}

#[test]
fn m2_3_arrow_dimensions_scale_with_the_square() {
    for size in [200.0, 480.0, 800.0, 1600.0] {
        let l = BoardLayout::compute(
            Size::new(size, size),
            LayoutOptions {
                coords: CoordsMode::None,
            },
        )
        .expect("fits");
        let a = arrow_geometry(&l, sq("a1"), sq("h8"), Orientation::White, 0.6).expect("arrow");
        assert!((a.shaft_width - 0.14 * l.s).abs() < 0.001);
        let [tip, c1, c2] = a.head;
        let width = (c1.x - c2.x).hypot(c1.y - c2.y);
        assert!((width - 0.40 * l.s).abs() < 0.01, "head width {width}");
        let len = (a.base.x - tip.x).hypot(a.base.y - tip.y);
        assert!((len - 0.38 * l.s).abs() < 0.01, "head length {len}");
        // Corners are symmetric around the axis.
        let d1 = (c1.x - tip.x).hypot(c1.y - tip.y);
        let d2 = (c2.x - tip.x).hypot(c2.y - tip.y);
        assert!((d1 - d2).abs() < 0.01);
    }
}

#[test]
fn m2_3_arrow_is_unaffected_by_nan_or_negative_shrink() {
    let l = layout();
    for shrink in [f32::NAN, f32::INFINITY, -3.0, 5.0, 0.0] {
        let a = arrow_geometry(&l, sq("a1"), sq("h8"), Orientation::White, shrink);
        if let Some(a) = a {
            for p in [a.tail, a.tip, a.base] {
                assert!(p.x.is_finite() && p.y.is_finite(), "{shrink}");
            }
        }
    }
}

#[test]
fn m2_3_black_arrow_is_the_white_arrow_rotated_half_a_turn() {
    let l = layout();
    for (a, b) in [("e2", "e4"), ("b1", "c3"), ("a1", "h8"), ("g7", "g8")] {
        let (from, to) = (sq(a), sq(b));
        let black = arrow_geometry(&l, from, to, Orientation::Black, 0.6).expect("arrow");
        let white = arrow_geometry(
            &l,
            Square::new(63 - u32::from(from)),
            Square::new(63 - u32::from(to)),
            Orientation::White,
            0.6,
        )
        .expect("arrow");
        // Mirror image of the squares: Black's e2->e4 equals White's d7->d5.
        for (x, y) in [(black.tail, white.tail), (black.tip, white.tip)] {
            assert!((x.x - y.x).abs() < 0.01 && (x.y - y.y).abs() < 0.01);
        }
    }
}

fn squares() -> impl Strategy<Value = Square> {
    (0u32..64).prop_map(Square::new)
}

proptest! {
    #[test]
    fn m2_3_prop_arrow_tail_and_tip_stay_inside_their_squares(
        from in squares(), to in squares(), black in any::<bool>(),
        shrink in 0.0f32..1.0, size in 160.0f32..2000.0,
    ) {
        let l = BoardLayout::compute(
            Size::new(size, size),
            LayoutOptions { coords: CoordsMode::Outside },
        ).expect("fits");
        let o = if black { Orientation::Black } else { Orientation::White };
        match arrow_geometry(&l, from, to, o, shrink) {
            None => prop_assert!(from == to, "no arrow between distinct squares"),
            Some(a) => {
                prop_assert!(from != to);
                let pad = 0.01;
                let fr = l.square_rect(from, o);
                let tr = l.square_rect(to, o);
                let inside = |r: Rectangle, p: Point| {
                    p.x >= r.x - pad && p.x <= r.x + r.width + pad
                        && p.y >= r.y - pad && p.y <= r.y + r.height + pad
                };
                prop_assert!(inside(fr, a.tail), "tail {:?} not in {:?}", a.tail, fr);
                prop_assert!(inside(tr, a.tip), "tip {:?} not in {:?}", a.tip, tr);
                prop_assert!((a.shaft_width - 0.14 * l.s).abs() < 0.01);
            }
        }
    }

    #[test]
    fn m2_3_prop_default_arrow_always_exists_between_distinct_squares(
        from in squares(), to in squares(),
    ) {
        prop_assume!(from != to);
        let l = layout();
        for o in [Orientation::White, Orientation::Black] {
            prop_assert!(arrow_geometry(&l, from, to, o, DEFAULT_SHRINK).is_some());
        }
    }
}

// ---------------------------------------------------------------------------
// Boxes (GetBox) and defaults
// ---------------------------------------------------------------------------

#[test]
fn m2_3_scid_box_portions() {
    assert!((CIRCLE_PORTION - 0.85).abs() < f32::EPSILON);
    assert!((TEXT_PORTION - 0.80).abs() < f32::EPSILON);
}

#[test]
fn m2_3_inner_box_is_concentric_with_the_requested_side() {
    let l = layout();
    let rect = l.square_rect(sq("d4"), Orientation::White);
    for portion in [CIRCLE_PORTION, TEXT_PORTION, 1.0, 0.5] {
        let b = inner_box(rect, portion);
        assert!((b.width - portion * rect.width).abs() < 0.001);
        assert_eq!(b.width, b.height);
        assert!((b.center().x - rect.center().x).abs() < 0.001);
        assert!((b.center().y - rect.center().y).abs() < 0.001);
    }
    let circle = inner_box(rect, CIRCLE_PORTION);
    assert!((circle.width - 85.0).abs() < 0.001);
}

#[test]
fn m2_3_brush_defaults_to_a_sky_blue_circle() {
    let b = MarkBrush::default();
    assert_eq!(b.kind, MarkKind::Circle);
    assert_eq!(b.color, "skyBlue");
}

// ---------------------------------------------------------------------------
// Color by modifier (rule 48)
// ---------------------------------------------------------------------------

#[test]
fn m2_3_color_for_modifiers_table_and_precedence() {
    let m = |shift, command, alt| Mods {
        shift,
        command,
        alt,
    };
    assert_eq!(color_for_modifiers(m(false, false, false)), "green");
    assert_eq!(color_for_modifiers(m(true, false, false)), "firebrick");
    assert_eq!(color_for_modifiers(m(false, true, false)), "yellow2");
    assert_eq!(color_for_modifiers(m(false, false, true)), "blue");
    assert_eq!(color_for_modifiers(m(true, true, true)), "firebrick");
    assert_eq!(color_for_modifiers(m(true, true, false)), "firebrick");
    assert_eq!(color_for_modifiers(m(true, false, true)), "firebrick");
    assert_eq!(color_for_modifiers(m(false, true, true)), "yellow2");
}

#[test]
fn m2_3_every_gesture_color_is_drawn_with_its_own_color_and_not_the_fallback() {
    for name in ["green", "firebrick", "yellow2", "blue", "skyBlue"] {
        assert!(parse_mark_color(name).is_some(), "{name}");
    }
    let fallback = mark_color_or_default("not-a-color");
    assert_ne!(fallback, mark_color_or_default("green"));
    assert_ne!(fallback, mark_color_or_default("yellow2"));
    assert_ne!(fallback, mark_color_or_default("blue"));
    assert_ne!(fallback, mark_color_or_default("skyBlue"));
}

// ---------------------------------------------------------------------------
// parse_mark_color (rule 46)
// ---------------------------------------------------------------------------

fn rgb8(r: u8, g: u8, b: u8) -> Option<Color> {
    Some(Color::from_rgb8(r, g, b))
}

#[rstest]
// the 14 of the Scid comment editor palette (comment.tcl:11)
#[case("cornsilk2", rgb8(238, 232, 205))]
#[case("wheat2", rgb8(238, 216, 174))]
#[case("burlyWood3", rgb8(205, 170, 125))]
#[case("grey50", rgb8(128, 128, 128))]
#[case("gray10", rgb8(26, 26, 26))]
#[case("rosyBrown", rgb8(188, 143, 143))]
#[case("mediumPurple1", rgb8(171, 130, 255))]
#[case("royalBlue3", rgb8(58, 95, 205))]
#[case("skyBlue", rgb8(135, 206, 235))]
#[case("cadetBlue2", rgb8(142, 229, 238))]
#[case("paleGreen2", rgb8(144, 238, 144))]
#[case("yellow3", rgb8(205, 205, 0))]
#[case("darkOrange1", rgb8(255, 127, 0))]
#[case("firebrick", rgb8(178, 34, 34))]
// ChessBase
#[case("green", rgb8(0, 255, 0))]
#[case("yellow2", rgb8(238, 238, 0))]
#[case("blue", rgb8(0, 0, 255))]
#[case("turquoise", rgb8(64, 224, 208))]
// board schemes
#[case("steelblue4", rgb8(54, 100, 139))]
#[case("rosybrown4", rgb8(139, 105, 105))]
#[case("tan4", rgb8(139, 90, 43))]
#[case("lightsteelblue", rgb8(176, 196, 222))]
#[case("steelblue", rgb8(70, 130, 180))]
#[case("grey20", rgb8(51, 51, 51))]
#[case("grey", rgb8(190, 190, 190))]
// basics
#[case("red", rgb8(255, 0, 0))]
#[case("white", rgb8(255, 255, 255))]
#[case("black", rgb8(0, 0, 0))]
// hex
#[case("#fff", rgb8(255, 255, 255))]
#[case("#f80", rgb8(255, 136, 0))]
#[case("#ff8000", rgb8(255, 128, 0))]
#[case("#FF8000", rgb8(255, 128, 0))]
#[case("#000000", rgb8(0, 0, 0))]
// case-insensitive like Tk
#[case("GREY50", rgb8(128, 128, 128))]
#[case("SkyBlue", rgb8(135, 206, 235))]
#[case("FIREBRICK", rgb8(178, 34, 34))]
#[case("  green  ", rgb8(0, 255, 0))]
fn m2_3_parse_mark_color_accepts(#[case] text: &str, #[case] expected: Option<Color>) {
    assert_eq!(parse_mark_color(text), expected, "{text:?}");
}

#[rstest]
#[case("")]
#[case("   ")]
#[case("#")]
#[case("#12")]
#[case("#1234")]
#[case("#12345")]
#[case("#1234567")]
#[case("#gggggg")]
#[case("#ggg")]
#[case("#é")]
#[case("#-12")]
#[case("notacolor")]
#[case("grey101")]
#[case("gray999")]
#[case("greyX")]
#[case("grey-5")]
#[case("e4")]
#[case("🙂")]
#[case("gr\u{0000}ey")]
fn m2_3_parse_mark_color_rejects(#[case] text: &str) {
    assert_eq!(parse_mark_color(text), None, "{text:?}");
}

#[test]
fn m2_3_grey_family_is_round_255_n_over_100() {
    for n in 0..=100u32 {
        let expected = ((255.0 * f64::from(n)) / 100.0).round();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let level = expected as u8;
        for prefix in ["grey", "gray"] {
            assert_eq!(
                parse_mark_color(&format!("{prefix}{n}")),
                rgb8(level, level, level),
                "{prefix}{n}"
            );
        }
    }
}

#[test]
fn m2_3_unknown_color_falls_back_to_firebrick_instead_of_vanishing() {
    assert_eq!(DEFAULT_MARK_COLOR_NAME, "firebrick");
    assert_eq!(
        mark_color_or_default("whatever"),
        Color::from_rgb8(178, 34, 34)
    );
    assert_eq!(
        mark_color_or_default("skyBlue"),
        Color::from_rgb8(135, 206, 235)
    );
}

proptest! {
    #[test]
    fn m2_3_prop_parse_mark_color_never_panics(text in ".{0,24}") {
        let _ = parse_mark_color(&text);
        let _ = mark_color_or_default(&text);
    }

    #[test]
    fn m2_3_prop_hex_round_trip(r in any::<u8>(), g in any::<u8>(), b in any::<u8>()) {
        let text = format!("#{r:02x}{g:02x}{b:02X}");
        prop_assert_eq!(parse_mark_color(&text), rgb8(r, g, b));
    }
}

// ---------------------------------------------------------------------------
// Board themes (rule 24)
// ---------------------------------------------------------------------------

/// `(id, light, dark, selected, hint, frame)` copied from the spec table.
const THEME_TABLE: [(BoardThemeId, &str, &str, &str, &str, &str); 10] = [
    (
        BoardThemeId::ScidBlueWhite,
        "#f3f3f3",
        "#7389b6",
        "#f3f484",
        "#b8cbf8",
        "steelblue4",
    ),
    (
        BoardThemeId::ScidBlueish,
        "#d0e0d0",
        "#80a0a0",
        "#b0d0e0",
        "#f0f0a0",
        "grey",
    ),
    (
        BoardThemeId::ScidMThomas,
        "#d3d9a8",
        "#51a068",
        "#e0d873",
        "#86a000",
        "grey20",
    ),
    (
        BoardThemeId::ScidGreenYellow,
        "#e0d070",
        "#70a070",
        "#b0d0e0",
        "#bebebe",
        "#29764e",
    ),
    (
        BoardThemeId::ScidBrown,
        "#d0c0a0",
        "#a08050",
        "#b0d0e0",
        "#bebebe",
        "tan4",
    ),
    (
        BoardThemeId::ScidTan,
        "#fbdbc4",
        "#cc9c83",
        "#b0d0e0",
        "#bebebe",
        "rosybrown4",
    ),
    (
        BoardThemeId::ScidGrey,
        "#9d9d9b",
        "#7a7977",
        "#6c8898",
        "#bebebe",
        "steelblue4",
    ),
    (
        BoardThemeId::ScidRosy,
        "#f8dbcc",
        "rosybrown",
        "#b0d0e0",
        "#bebebe",
        "rosybrown4",
    ),
    (
        BoardThemeId::ScidSteelBlue,
        "lightsteelblue",
        "steelblue",
        "#51a068",
        "#e0d873",
        "#002958",
    ),
    (
        BoardThemeId::ScidGreen,
        "#46a631",
        "#006400",
        "#008064",
        "rosybrown4",
        "#8bb869",
    ),
];

#[test]
fn m2_3_there_are_eleven_distinct_themes_and_the_default_is_the_mychess_green() {
    // M2-5 (D219): the ten presets of the Scid plus the Green, now the default.
    assert_eq!(BoardThemeId::ALL.len(), 11);
    let unique: std::collections::HashSet<_> = BoardThemeId::ALL.iter().collect();
    assert_eq!(unique.len(), 11);
    assert_eq!(BoardThemeId::default(), BoardThemeId::MyChessGreen);
    assert_eq!(BoardThemeId::ALL[0], BoardThemeId::MyChessGreen);
}

#[test]
fn m2_3_theme_colors_match_the_spec_table() {
    for (id, light, dark, selected, hint, frame) in THEME_TABLE {
        let t = BoardTheme::of(id);
        let c = |s: &str| parse_mark_color(s).unwrap_or_else(|| panic!("spec color {s}"));
        assert_eq!(t.id, id);
        assert_eq!(t.light, c(light), "{id:?} light");
        assert_eq!(t.dark, c(dark), "{id:?} dark");
        assert_eq!(t.selected, c(selected), "{id:?} selected");
        assert_eq!(t.hint, c(hint), "{id:?} hint");
        assert_eq!(t.frame, c(frame), "{id:?} frame");
    }
}

#[test]
fn m2_3_last_move_color_is_grey_in_every_theme() {
    let grey = parse_mark_color("grey").expect("grey");
    for id in BoardThemeId::ALL {
        assert_eq!(BoardTheme::of(id).last_move, grey, "{id:?}");
    }
}

#[test]
fn m2_3_light_and_dark_squares_always_differ() {
    for id in BoardThemeId::ALL {
        let t = BoardTheme::of(id);
        assert_ne!(t.light, t.dark, "{id:?}");
    }
}

#[test]
fn m2_3_kebab_names_are_unique_and_round_trip() {
    let mut seen = std::collections::HashSet::new();
    for id in BoardThemeId::ALL {
        let name = id.kebab_name();
        assert!(seen.insert(name), "duplicate {name}");
        assert!(name.starts_with("scid-") || name == "mychess-green");
        assert!(
            name.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'),
            "{name} must be kebab-case"
        );
        assert_eq!(BoardThemeId::from_kebab_name(name), Some(id));
    }
    assert_eq!(
        BoardThemeId::from_kebab_name("scid-blue-white"),
        Some(BoardThemeId::ScidBlueWhite)
    );
    assert_eq!(BoardThemeId::from_kebab_name("Scid-Blue-White"), None);
    assert_eq!(BoardThemeId::from_kebab_name(""), None);
    assert_eq!(BoardThemeId::from_kebab_name("nope"), None);
}

#[test]
fn m2_3_theme_name_keys_exist_in_both_languages() {
    use mychess_gui::i18n::{I18n, Language};
    for lang in Language::ALL {
        let i18n = I18n::new(lang);
        for id in BoardThemeId::ALL {
            assert!(
                i18n.has(id.name_key()),
                "{} missing in {lang:?}",
                id.name_key()
            );
            assert_ne!(i18n.tr(id.name_key()), id.name_key());
        }
    }
}

// ---------------------------------------------------------------------------
// Round trip with the core: what the gestures emit survives toggle_mark.
// ---------------------------------------------------------------------------

fn gesture_marks() -> Vec<BoardMark> {
    let mut out = Vec::new();
    for color in [
        "green",
        "firebrick",
        "yellow2",
        "blue",
        "skyBlue",
        "#ff0000",
    ] {
        out.push(BoardMark::square(MarkKind::Circle, sq("d4"), color));
        out.push(BoardMark::arrow(sq("d4"), sq("f6"), color));
        out.push(BoardMark::arrow(sq("h1"), sq("a8"), color));
        out.push(BoardMark::square(MarkKind::Disk, sq("a1"), color));
        out.push(BoardMark::square(MarkKind::Full, sq("h8"), color));
        out.push(BoardMark::square(MarkKind::Text('!'), sq("e5"), color));
        out.push(BoardMark::square(MarkKind::Text('1'), sq("e5"), color));
    }
    out
}

#[test]
fn m2_3_toggling_a_gesture_mark_twice_restores_the_comment() {
    for comment in [
        "",
        "plain text",
        "[%clk 0:01:00] a note",
        "[%draw circle,b1,green] keep",
        "[%csl Ge4,Rd5] [%cal Ge2e4]",
    ] {
        for mark in gesture_marks() {
            let once = toggle_mark(comment, &mark);
            assert_ne!(
                once.trim(),
                comment.trim(),
                "{mark:?} changed nothing in {comment:?}"
            );
            let twice = toggle_mark(&once, &mark);
            assert_eq!(twice.trim(), comment.trim(), "{mark:?} on {comment:?}");
        }
    }
}

#[test]
fn m2_3_a_toggled_gesture_mark_reads_back_as_the_same_mark() {
    for mark in gesture_marks() {
        let text = toggle_mark("", &mark);
        let read: Vec<BoardMark> = embedded_commands(&text)
            .into_iter()
            .filter_map(|c| match c {
                EmbeddedCommand::Mark(m) => Some(m),
                _ => None,
            })
            .collect();
        assert_eq!(read, std::slice::from_ref(&mark), "{text:?}");
        // and the GUI can paint its color (not the unknown-color fallback).
        assert!(parse_mark_color(&mark.color).is_some(), "{}", mark.color);
    }
}

#[test]
fn m2_3_toggling_an_arrow_replaces_the_reverse_or_recolored_one_like_scid() {
    // Scid `drawArrow` (end.tcl:1547-1570): only the identical arrow erases;
    // another color or the reverse direction is replaced by the new arrow.
    let a = BoardMark::arrow(sq("d4"), sq("f6"), "green");
    let once = toggle_mark("", &a);
    let reverse = BoardMark::arrow(sq("f6"), sq("d4"), "yellow2");
    assert_eq!(toggle_mark(&once, &reverse), "[%draw arrow,f6,d4,yellow2]");
    let recolored = BoardMark::arrow(sq("d4"), sq("f6"), "blue");
    assert_eq!(toggle_mark(&once, &recolored), "[%draw arrow,d4,f6,blue]");
    assert_eq!(toggle_mark(&once, &a).trim(), "");
}

#[test]
fn m2_3_all_four_formats_of_rule_41_are_read_and_paintable() {
    let text = "[%draw circle,d4,green] [%csl Gd5] [%cal Re2e4] [%mark e5 firebrick]";
    let marks: Vec<BoardMark> = embedded_commands(text)
        .into_iter()
        .filter_map(|c| match c {
            EmbeddedCommand::Mark(m) => Some(m),
            _ => None,
        })
        .collect();
    assert_eq!(marks.len(), 4, "{marks:?}");
    for m in &marks {
        assert!(parse_mark_color(&m.color).is_some(), "{m:?}");
    }
    assert!(marks.iter().any(|m| m.kind == MarkKind::Arrow));
}

#[test]
fn m2_3_chessbase_letters_map_to_colors_the_gui_can_paint() {
    for letter in "GRYBOC".chars() {
        if let Some(name) = chessbase_color(letter) {
            assert!(parse_mark_color(name).is_some(), "{letter} -> {name}");
        }
    }
    assert_eq!(chessbase_color('G'), Some("green"));
    assert_eq!(chessbase_color('R'), Some("firebrick"));
    assert_eq!(chessbase_color('Y'), Some("yellow2"));
    assert_eq!(chessbase_color('B'), Some("blue"));
}

proptest! {
    #[test]
    fn m2_3_prop_toggle_twice_is_identity_for_any_square_pair(
        from in squares(), to in squares(), which in 0usize..4,
    ) {
        let color = ["green", "firebrick", "yellow2", "blue"][which];
        let mark = if from == to {
            BoardMark::square(MarkKind::Circle, from, color)
        } else {
            BoardMark::arrow(from, to, color)
        };
        let once = toggle_mark("note", &mark);
        let twice = toggle_mark(&once, &mark);
        prop_assert_eq!(twice.trim(), "note");
    }
}
