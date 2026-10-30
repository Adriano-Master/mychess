//! Design tokens of the interface (spec M2-5, D217, D220, D221): colors,
//! radii, spacing and type sizes. Every widget reads them from
//! [`Tokens::of`]; nothing else hardcodes a color.

use iced::{Color, Theme};

use super::accent::AccentId;
use super::palette::{self, Colors, NagTones};

/// Corner radii in logical pixels (D220).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radii {
    /// Panels.
    pub panel: f32,
    /// Buttons and inputs.
    pub control: f32,
    /// Variation cards.
    pub card: f32,
    /// The current-move pill.
    pub pill: f32,
    /// Small chips (ECO code, title).
    pub chip: f32,
}

/// Spacing scale in logical pixels (multiples of 4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spacing {
    /// 4.
    pub xs: f32,
    /// 8.
    pub sm: f32,
    /// 12.
    pub md: f32,
    /// 16.
    pub lg: f32,
    /// 24.
    pub xl: f32,
}

/// Type sizes in logical pixels (D220); the font is Noto Sans (D141).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypeScale {
    /// Secondary text.
    pub small: f32,
    /// Base text.
    pub base: f32,
    /// Notation text.
    pub notation: f32,
    /// Player names (bold).
    pub name: f32,
    /// Clocks (bold).
    pub clock: f32,
}

/// The colors and measures of one palette.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    /// Window background (and the player strips).
    pub window: Color,
    /// Panel background.
    pub panel: Color,
    /// Cards, clocks, menus.
    pub raised: Color,
    /// Even table rows.
    pub zebra: Color,
    /// Dividers (decorative).
    pub border: Color,
    /// Main text.
    pub text: Color,
    /// Secondary text and icons.
    pub muted: Color,
    /// Accent: selection, toggled buttons, current move.
    pub accent: Color,
    /// Text and icons over [`accent`](Self::accent).
    pub on_accent: Color,
    /// Text of variations.
    pub variation: Color,
    /// Hover background (text over the surface at 7 %).
    pub hover: Color,
    /// Pressed background (text over the surface at 12 %).
    pub pressed: Color,
    /// Low clock and error tint.
    pub danger: Color,
    /// NAG tones.
    pub nag: NagTones,
    /// Radii.
    pub radius: Radii,
    /// Spacing.
    pub space: Spacing,
    /// Type sizes.
    pub font: TypeScale,
}

/// `a` over `b` with `amount` of `a` (0 to 1).
pub fn mix(a: Color, b: Color, amount: f32) -> Color {
    let t = amount.clamp(0.0, 1.0);
    Color::from_rgb(
        a.r * t + b.r * (1.0 - t),
        a.g * t + b.g * (1.0 - t),
        a.b * t + b.b * (1.0 - t),
    )
}

const RADII: Radii = Radii {
    panel: 8.0,
    control: 6.0,
    card: 6.0,
    pill: 5.0,
    chip: 4.0,
};

const SPACE: Spacing = Spacing {
    xs: 4.0,
    sm: 8.0,
    md: 12.0,
    lg: 16.0,
    xl: 24.0,
};

const FONT: TypeScale = TypeScale {
    small: 12.0,
    base: 14.0,
    notation: 15.0,
    name: 15.0,
    clock: 20.0,
};

/// Alpha of a disabled icon (D227).
pub const DISABLED_ALPHA: f32 = 0.5;

impl Tokens {
    fn from_colors(c: &Colors) -> Tokens {
        Tokens {
            window: c.window,
            panel: c.panel,
            raised: c.raised,
            zebra: c.zebra,
            border: c.border,
            text: c.text,
            muted: c.muted,
            accent: c.accent,
            on_accent: c.on_accent,
            variation: c.variation,
            hover: c.hover,
            pressed: mix(c.text, c.panel, 0.12),
            danger: c.danger,
            nag: c.nag,
            radius: RADII,
            space: SPACE,
            font: FONT,
        }
    }

    /// The tokens of the dark palette.
    pub fn dark() -> Tokens {
        Tokens::from_colors(&palette::DARK)
    }

    /// The tokens of the light palette.
    pub fn light() -> Tokens {
        Tokens::from_colors(&palette::LIGHT)
    }

    /// The tokens of a palette with an accent color (spec M9-1).
    pub fn with_accent(dark: bool, accent: AccentId) -> Tokens {
        let mut tokens = if dark {
            Tokens::dark()
        } else {
            Tokens::light()
        };
        tokens.accent = accent.color(dark);
        tokens.on_accent = accent.on_color(dark);
        tokens
    }

    /// The tokens that go with `theme` (the built-in iced themes map to the
    /// palette of the same lightness). The accent is read from the `primary`
    /// color of the theme; a primary that is not one of ours (a built-in
    /// iced theme) gives the default accent (D1603).
    pub fn of(theme: &Theme) -> Tokens {
        let dark = palette::is_dark(theme);
        let accent = AccentId::from_color(theme.palette().primary, dark).unwrap_or_default();
        Tokens::with_accent(dark, accent)
    }

    /// `muted` at the disabled opacity (D220).
    pub fn disabled(&self) -> Color {
        self.muted.scale_alpha(DISABLED_ALPHA)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::contrast::{MIN_GRAPHIC, MIN_TEXT, contrast_ratio};
    use crate::theme::resolve;
    use crate::theme::{ThemeMode, board_theme::BoardTheme, board_theme::BoardThemeId};

    /// Every theme x accent combination (spec M9-1, section 2.4).
    fn both() -> Vec<(String, Tokens)> {
        let mut all = Vec::new();
        for (name, dark) in [("dark", true), ("light", false)] {
            for accent in AccentId::ALL {
                all.push((
                    format!("{name}/{accent:?}"),
                    Tokens::with_accent(dark, accent),
                ));
            }
        }
        all
    }

    #[test]
    fn tokens_of_a_theme_carry_its_accent() {
        for dark in [true, false] {
            for accent in AccentId::ALL {
                let mode = if dark {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                };
                let theme = resolve(mode, iced::theme::Mode::None, accent);
                let t = Tokens::of(&theme);
                assert_eq!(t.accent, accent.color(dark), "{accent:?} dark={dark}");
                assert_eq!(t.on_accent, accent.on_color(dark));
                assert_eq!(t, Tokens::with_accent(dark, accent));
            }
        }
        // The default accent leaves the original palettes untouched (D1600).
        assert_eq!(Tokens::with_accent(true, AccentId::Mychess), Tokens::dark());
        assert_eq!(
            Tokens::with_accent(false, AccentId::Mychess),
            Tokens::light()
        );
    }

    #[test]
    fn accents_differ_from_each_other() {
        for dark in [true, false] {
            for (i, a) in AccentId::ALL.iter().enumerate() {
                for b in &AccentId::ALL[i + 1..] {
                    assert_ne!(a.color(dark), b.color(dark), "{a:?} {b:?}");
                }
            }
        }
    }

    #[test]
    fn the_two_palettes_differ_and_follow_the_theme() {
        assert_ne!(Tokens::dark(), Tokens::light());
        let dark = resolve(ThemeMode::Dark, iced::theme::Mode::None, AccentId::Mychess);
        let light = resolve(ThemeMode::Light, iced::theme::Mode::None, AccentId::Mychess);
        assert_eq!(Tokens::of(&dark), Tokens::dark());
        assert_eq!(Tokens::of(&light), Tokens::light());
        assert_eq!(Tokens::of(&Theme::Dark), Tokens::dark());
        assert_eq!(Tokens::of(&Theme::Light), Tokens::light());
    }

    #[test]
    fn scales_are_the_ones_of_the_spec() {
        let t = Tokens::dark();
        assert_eq!(
            [
                t.radius.panel,
                t.radius.control,
                t.radius.card,
                t.radius.pill,
                t.radius.chip
            ],
            [8.0, 6.0, 6.0, 5.0, 4.0]
        );
        for s in [t.space.xs, t.space.sm, t.space.md, t.space.lg, t.space.xl] {
            assert!((s % 4.0).abs() < f32::EPSILON, "{s}");
        }
    }

    #[test]
    fn text_pairs_reach_4_5() {
        for (name, t) in both() {
            let mut pairs = vec![];
            for bg in [t.window, t.panel, t.raised, t.zebra, t.hover, t.pressed] {
                pairs.push(("text", t.text, bg));
                pairs.push(("muted", t.muted, bg));
            }
            pairs.push(("on_accent", t.on_accent, t.accent));
            pairs.push(("variation", t.variation, t.panel));
            pairs.push(("accent", t.accent, t.panel));
            pairs.push(("danger on raised", t.danger, t.raised));
            for bg in [t.panel, t.zebra, t.hover] {
                for (label, nag) in [
                    ("brilliant", t.nag.brilliant),
                    ("good", t.nag.good),
                    ("interesting", t.nag.interesting),
                    ("dubious", t.nag.dubious),
                    ("mistake", t.nag.mistake),
                    ("blunder", t.nag.blunder),
                    ("eval", t.nag.eval),
                ] {
                    pairs.push((label, nag, bg));
                }
            }
            // A NAG inside the current-move pill uses `on_accent`.
            for (label, fg, bg) in pairs {
                let ratio = contrast_ratio(fg, bg);
                assert!(ratio >= MIN_TEXT, "{name} {label}: {ratio:.2}");
            }
        }
    }

    #[test]
    fn graphics_reach_3() {
        for (name, t) in both() {
            for (label, fg, bg) in [
                ("icon text", t.text, t.panel),
                ("icon muted", t.muted, t.panel),
                ("icon muted raised", t.muted, t.raised),
                ("icon accent", t.accent, t.panel),
                ("icon on accent", t.on_accent, t.accent),
                ("focus ring", t.accent, t.window),
            ] {
                let ratio = contrast_ratio(fg, bg);
                assert!(ratio >= MIN_GRAPHIC, "{name} {label}: {ratio:.2}");
            }
        }
        // Inside coordinates of the Green board theme.
        let green = BoardTheme::of(BoardThemeId::MyChessGreen);
        assert!(contrast_ratio(green.light, green.dark) >= MIN_GRAPHIC);
    }

    #[test]
    fn mix_blends_linearly() {
        let m = mix(Color::WHITE, Color::BLACK, 0.5);
        assert!((m.r - 0.5).abs() < 1e-6 && (m.g - 0.5).abs() < 1e-6);
        assert_eq!(mix(Color::WHITE, Color::BLACK, 2.0), Color::WHITE);
    }

    #[test]
    fn disabled_is_half_transparent() {
        let t = Tokens::dark();
        assert!((t.disabled().a - 0.5).abs() < 1e-6);
    }

    #[test]
    fn no_color_literal_outside_the_theme() {
        fn walk(dir: &std::path::Path, hits: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, hits);
                } else if path.extension().is_some_and(|x| x == "rs") {
                    let inside_theme = path
                        .components()
                        .any(|c| c.as_os_str() == "theme" || c.as_os_str() == "theme.rs");
                    if inside_theme {
                        continue;
                    }
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    // Test modules may build colors.
                    let production = text.split("#[cfg(test)]").next().unwrap_or_default();
                    for (i, line) in production.lines().enumerate() {
                        if line.contains("Color::from_rgb") || line.contains("color!(") {
                            hits.push(format!("{}:{}", path.display(), i + 1));
                        }
                    }
                }
            }
        }
        let mut hits = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut hits,
        );
        // Known literals of earlier slices, outside the 5a territory; the
        // board slice moves them into `theme/` (D221).
        hits.retain(|h| !h.contains("widgets/board/"));
        assert!(hits.is_empty(), "{hits:#?}");
    }
}
