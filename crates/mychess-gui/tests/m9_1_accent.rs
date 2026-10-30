//! M9-1 (GUI): the accent color (spec `docs/specs/m9-1-cor-de-realce.md`,
//! criteria 3 and 4, D1600-D1610). Contract tests derived from the spec;
//! headless.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::collections::BTreeSet;
use std::path::Path;

use iced::theme::Palette;
use iced::{Background, Color, Theme};
use mychess_app::{AccentPref, Message as AppMessage, PrefChange, PrefsMessage, ThemePref};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::theme::contrast::{MIN_GRAPHIC, MIN_TEXT, contrast_ratio};
use mychess_gui::theme::notation_colors::NotationPalette;
use mychess_gui::theme::tokens::Tokens;
use mychess_gui::theme::{AccentId, RowLook, ThemeMode, list_row, resolve};
use mychess_gui::{Message, Shell, UiMessage};

const PAIRS: [(AccentPref, AccentId); 9] = [
    (AccentPref::Mychess, AccentId::Mychess),
    (AccentPref::Blue, AccentId::Blue),
    (AccentPref::Purple, AccentId::Purple),
    (AccentPref::Pink, AccentId::Pink),
    (AccentPref::Red, AccentId::Red),
    (AccentPref::Orange, AccentId::Orange),
    (AccentPref::Yellow, AccentId::Yellow),
    (AccentPref::Green, AccentId::Green),
    (AccentPref::Gray, AccentId::Gray),
];

fn pref(change: PrefChange) -> Message {
    Message::App(AppMessage::Prefs(PrefsMessage::Set(change)))
}

fn send(shell: &mut Shell, message: Message) {
    drop(shell.update(message));
}

fn dark_of(theme: &Theme) -> bool {
    mychess_gui::theme::contrast::relative_luminance(theme.palette().background) < 0.35
}

/// Blends `top` (with its alpha) over the opaque `under`.
fn over(top: Color, under: Color) -> Color {
    Color::from_rgb(
        top.r * top.a + under.r * (1.0 - top.a),
        top.g * top.a + under.g * (1.0 - top.a),
        top.b * top.a + under.b * (1.0 - top.a),
    )
}

// ---- (a) the preference reaches the theme (criteria 1 and 4) ----

#[test]
fn choosing_an_accent_through_the_shell_changes_the_tokens_of_its_theme() {
    for (pref_value, id) in PAIRS {
        for (theme_pref, dark) in [(ThemePref::Dark, true), (ThemePref::Light, false)] {
            let mut shell = Shell::new(None);
            send(&mut shell, pref(PrefChange::Theme(theme_pref)));
            send(&mut shell, pref(PrefChange::Accent(pref_value)));
            let tokens = Tokens::of(&shell.theme());
            assert_eq!(tokens.accent, id.color(dark), "{id:?} dark={dark}");
            assert_eq!(tokens.on_accent, id.on_color(dark), "{id:?} dark={dark}");
        }
    }
}

#[test]
fn orange_is_the_orange_of_the_table() {
    let mut shell = Shell::new(None);
    send(&mut shell, pref(PrefChange::Theme(ThemePref::Dark)));
    send(&mut shell, pref(PrefChange::Accent(AccentPref::Orange)));
    assert_eq!(
        Tokens::of(&shell.theme()).accent,
        Color::from_rgb8(0xf5, 0x9a, 0x3d)
    );
    send(&mut shell, pref(PrefChange::Theme(ThemePref::Light)));
    assert_eq!(
        Tokens::of(&shell.theme()).accent,
        Color::from_rgb8(0xb4, 0x5a, 0x0b)
    );
}

#[test]
fn the_default_accent_keeps_the_original_green_and_on_colors() {
    let dark = Tokens::with_accent(true, AccentId::Mychess);
    let light = Tokens::with_accent(false, AccentId::Mychess);
    assert_eq!(dark.accent, Color::from_rgb8(0x7c, 0xc0, 0x4a));
    assert_eq!(light.accent, Color::from_rgb8(0x2f, 0x6b, 0x17));
    assert_eq!(dark.on_accent, Color::from_rgb8(0x12, 0x20, 0x0a));
    assert_eq!(light.on_accent, Color::WHITE);
    assert_eq!(Tokens::of(&Theme::Dark).accent, Tokens::dark().accent);
    assert_eq!(Tokens::of(&Theme::Light).accent, Tokens::light().accent);
}

#[test]
fn switching_light_dark_keeps_the_chosen_accent() {
    let mut shell = Shell::new(None);
    send(&mut shell, pref(PrefChange::Accent(AccentPref::Purple)));
    for theme_pref in [
        ThemePref::Dark,
        ThemePref::Light,
        ThemePref::Dark,
        ThemePref::Light,
    ] {
        send(&mut shell, pref(PrefChange::Theme(theme_pref)));
        let dark = theme_pref == ThemePref::Dark;
        let theme = shell.theme();
        assert_eq!(dark_of(&theme), dark);
        assert_eq!(Tokens::of(&theme).accent, AccentId::Purple.color(dark));
    }
}

#[test]
fn system_mode_follows_the_system_and_keeps_the_chosen_accent() {
    let mut shell = Shell::new(None);
    send(&mut shell, pref(PrefChange::Theme(ThemePref::System)));
    send(&mut shell, pref(PrefChange::Accent(AccentPref::Pink)));
    for (mode, dark) in [
        (iced::theme::Mode::Dark, true),
        (iced::theme::Mode::Light, false),
        (iced::theme::Mode::Dark, true),
    ] {
        send(&mut shell, Message::Ui(UiMessage::SystemTheme(mode)));
        let theme = shell.theme();
        assert_eq!(dark_of(&theme), dark, "{mode:?}");
        assert_eq!(Tokens::of(&theme).accent, AccentId::Pink.color(dark));
    }
    // Changing the accent while following the system also takes effect.
    send(&mut shell, pref(PrefChange::Accent(AccentPref::Blue)));
    assert_eq!(
        Tokens::of(&shell.theme()).accent,
        AccentId::Blue.color(true)
    );
}

#[test]
fn resolve_carries_the_accent_for_every_mode() {
    for id in AccentId::ALL {
        for (mode, system, dark) in [
            (ThemeMode::Dark, iced::theme::Mode::None, true),
            (ThemeMode::Light, iced::theme::Mode::Dark, false),
            (ThemeMode::System, iced::theme::Mode::Dark, true),
            (ThemeMode::System, iced::theme::Mode::Light, false),
        ] {
            let tokens = Tokens::of(&resolve(mode, system, id));
            assert_eq!(tokens.accent, id.color(dark), "{id:?} {mode:?} {system:?}");
        }
    }
}

#[test]
fn the_nine_accents_are_distinct_in_each_theme() {
    for dark in [true, false] {
        let colors: Vec<[u32; 3]> = AccentId::ALL
            .iter()
            .map(|a| {
                let c = a.color(dark);
                [c.r.to_bits(), c.g.to_bits(), c.b.to_bits()]
            })
            .collect();
        let unique: BTreeSet<_> = colors.iter().collect();
        assert_eq!(unique.len(), 9);
    }
}

// ---- (b) contrast of the derived colors (criterion 3, section 2.4) ----

/// Contrast failures of the text colors over the selected and current rows of
/// the game list, `muted` choosing the secondary text.
fn list_row_failures(muted: bool) -> Vec<String> {
    let mut failures = Vec::new();
    for dark in [true, false] {
        for id in AccentId::ALL {
            let theme = resolve(
                if dark {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                },
                iced::theme::Mode::None,
                id,
            );
            let t = Tokens::of(&theme);
            for (name, look) in [
                (
                    "selected",
                    RowLook {
                        selected: true,
                        ..RowLook::default()
                    },
                ),
                (
                    "current",
                    RowLook {
                        current: true,
                        ..RowLook::default()
                    },
                ),
            ] {
                let style = list_row(look)(&theme);
                let Some(Background::Color(background)) = style.background else {
                    panic!("{name} row has a color background");
                };
                let fg = if muted { t.muted } else { t.text };
                let ratio = contrast_ratio(fg, background);
                if ratio < MIN_TEXT {
                    failures.push(format!("{id:?} dark={dark} on {name} row: {ratio:.2}"));
                }
            }
        }
    }
    failures
}

#[test]
fn text_on_the_selected_and_current_rows_of_the_list_reaches_4_5_for_every_accent() {
    let failures = list_row_failures(false);
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn muted_text_on_the_selected_and_current_rows_of_the_list_reaches_4_5_for_every_accent() {
    let failures = list_row_failures(true);
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Contrast failures over the soft highlight of the current move; `muted`
/// chooses the secondary colors (comments, black figurines) or the main ones.
fn notation_failures(muted: bool) -> Vec<String> {
    let mut failures = Vec::new();
    for dark in [true, false] {
        for id in AccentId::ALL {
            let t = Tokens::with_accent(dark, id);
            let p = NotationPalette::from_tokens(&t);
            let texts: Vec<(&str, Color)> = if muted {
                vec![
                    ("comment", p.comment),
                    ("muted", p.muted),
                    ("black_piece", p.black_piece),
                ]
            } else {
                vec![
                    ("main", p.main),
                    ("variation", p.variation),
                    ("white_piece", p.white_piece),
                ]
            };
            for (bg_name, bg) in [("panel", t.panel), ("raised", t.raised)] {
                let background = over(p.current, bg);
                for (what, fg) in &texts {
                    let ratio = contrast_ratio(*fg, background);
                    if ratio < MIN_TEXT {
                        failures.push(format!(
                            "{id:?} dark={dark} {what} over {bg_name}: {ratio:.2}"
                        ));
                    }
                }
            }
        }
    }
    failures
}

#[test]
fn notation_text_over_the_soft_current_highlight_reaches_4_5_for_every_accent() {
    let failures = notation_failures(false);
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn muted_notation_text_over_the_soft_current_highlight_reaches_4_5_for_every_accent() {
    let failures = notation_failures(true);
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn pill_text_and_the_accent_against_surfaces_reach_the_minimums_for_every_accent() {
    let mut failures = Vec::new();
    for dark in [true, false] {
        for id in AccentId::ALL {
            let t = Tokens::with_accent(dark, id);
            let p = NotationPalette::from_tokens(&t);
            let mut check = |name: &str, a: Color, b: Color, min: f32| {
                let ratio = contrast_ratio(a, b);
                if ratio < min {
                    failures.push(format!("{id:?} dark={dark} {name}: {ratio:.2} < {min}"));
                }
            };
            check("on_pill/pill", p.on_pill, p.pill, MIN_TEXT);
            check("on_accent/accent", t.on_accent, t.accent, MIN_TEXT);
            check("accent/panel", t.accent, t.panel, MIN_GRAPHIC);
            check("accent/window", t.accent, t.window, MIN_GRAPHIC);
            check("accent/raised", t.accent, t.raised, MIN_GRAPHIC);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

// ---- (c) D1609: Tokens::of reads the primary within the detected variant ----

fn custom(background: Color, primary: Color) -> Theme {
    Theme::custom(
        "Custom".to_owned(),
        Palette {
            background,
            text: Color::WHITE,
            primary,
            success: Color::WHITE,
            warning: Color::WHITE,
            danger: Color::WHITE,
        },
    )
}

#[test]
fn a_dark_custom_theme_with_the_light_variant_of_an_accent_falls_back_to_the_dark_default() {
    let dark_bg = Tokens::dark().window;
    for id in AccentId::ALL {
        // Light variants that differ from every dark variant.
        let primary = id.color(false);
        if AccentId::from_color(primary, true).is_some() {
            continue;
        }
        let tokens = Tokens::of(&custom(dark_bg, primary));
        assert_eq!(tokens.accent, AccentId::Mychess.color(true), "{id:?}");
        assert_eq!(tokens.on_accent, AccentId::Mychess.on_color(true), "{id:?}");
    }
}

#[test]
fn a_light_custom_theme_with_the_dark_variant_of_an_accent_falls_back_to_the_light_default() {
    let light_bg = Tokens::light().window;
    for id in AccentId::ALL {
        let primary = id.color(true);
        if AccentId::from_color(primary, false).is_some() {
            continue;
        }
        let tokens = Tokens::of(&custom(light_bg, primary));
        assert_eq!(tokens.accent, AccentId::Mychess.color(false), "{id:?}");
    }
}

#[test]
fn a_custom_theme_with_the_right_variant_gives_that_accent_and_unknown_primaries_the_default() {
    let dark_bg = Tokens::dark().window;
    let tokens = Tokens::of(&custom(dark_bg, AccentId::Red.color(true)));
    assert_eq!(tokens.accent, AccentId::Red.color(true));
    let tokens = Tokens::of(&custom(dark_bg, Color::from_rgb8(1, 2, 3)));
    assert_eq!(tokens.accent, AccentId::Mychess.color(true));
}

#[test]
fn builtin_iced_themes_map_to_the_default_accent() {
    for theme in Theme::ALL {
        let t = Tokens::of(theme);
        let dark = dark_of(theme);
        assert_eq!(t.accent, AccentId::Mychess.color(dark), "{theme:?}");
    }
}

// ---- (e) Fluent parity of the accent keys ----

fn ids(language: &str) -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join("menu.ftl");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .filter_map(|l| {
            let (id, _) = l.split_once(" = ")?;
            (!id.starts_with(['#', ' ', '-'])).then(|| id.to_owned())
        })
        .collect()
}

#[test]
fn the_accent_keys_exist_in_both_languages() {
    let (en, pt) = (ids("en-US"), ids("pt-BR"));
    let mut keys: Vec<String> = AccentId::ALL
        .iter()
        .map(|a| a.name_key().to_owned())
        .collect();
    keys.push("menu-view-accent".to_owned());
    keys.push("accent-selected".to_owned());
    for key in &keys {
        assert!(en.contains(key), "{key} missing in en-US");
        assert!(pt.contains(key), "{key} missing in pt-BR");
    }
    let accent_keys = |set: &BTreeSet<String>| -> BTreeSet<String> {
        set.iter()
            .filter(|k| k.starts_with("accent-") || k.as_str() == "menu-view-accent")
            .cloned()
            .collect()
    };
    assert_eq!(accent_keys(&en), accent_keys(&pt));
}

#[test]
fn the_accent_names_resolve_to_distinct_text_without_the_word_variation() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        let mut names = BTreeSet::new();
        for id in AccentId::ALL {
            let text = i18n.tr(id.name_key());
            assert_ne!(text, id.name_key(), "{language:?} {id:?}");
            assert!(
                names.insert(text),
                "{language:?}: duplicate name for {id:?}"
            );
        }
        for key in ["menu-view-accent", "accent-selected"] {
            let text = i18n.tr(key);
            assert_ne!(text, key);
            let lower = text.to_lowercase();
            assert!(!lower.contains("variação") && !lower.contains("variacao"));
        }
    }
    let pt = I18n::new(Language::PtBr);
    for id in AccentId::ALL {
        let lower = pt.tr(id.name_key()).to_lowercase();
        assert!(!lower.contains("varia"), "{lower}");
    }
    assert_eq!(pt.tr("accent-blue"), "Azul");
    assert_eq!(pt.tr("accent-purple"), "Roxo");
    assert_eq!(
        I18n::new(Language::EnUs).tr("accent-mychess"),
        "MyChess green"
    );
}

#[test]
fn the_accent_files_in_pt_br_never_use_the_word_variation() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/i18n/pt-BR/menu.ftl");
    let text = std::fs::read_to_string(path).expect("menu.ftl");
    for line in text
        .lines()
        .filter(|l| l.starts_with("accent-") || l.starts_with("menu-view-accent"))
    {
        let lower = line.to_lowercase();
        assert!(
            !lower.contains("variaç") && !lower.contains("variac"),
            "{line}"
        );
    }
}
