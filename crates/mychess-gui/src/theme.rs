//! Theme resolution and the styles of the application shell.
//!
//! The interface colors live in [`palette`] ("MyChess Dark" and "MyChess
//! Light", spec M2-5, D217) and reach the widgets through [`Tokens`]; the
//! board colors live in [`board_theme`]. Nothing outside this module
//! hardcodes a color (D221).

pub mod accent;
pub mod board_theme;
pub mod book_styles;
pub mod chart_colors;
pub mod contrast;
pub mod engine_overlay;
pub mod graph_colors;
pub mod mark_colors;
pub mod mask_colors;
pub mod notation_colors;
pub mod palette;
pub mod report_colors;
pub mod tablebase_colors;
pub mod tokens;
pub mod tree_colors;

pub use accent::AccentId;
pub use tokens::Tokens;

use iced::widget::{button, container};
use iced::{Border, Color, Theme};

/// The interface theme preference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ThemeMode {
    /// Follow the operating system.
    #[default]
    System,
    /// Light.
    Light,
    /// Dark.
    Dark,
}

/// The iced theme for a preference and the last reported system mode: the
/// custom "MyChess Dark" or "MyChess Light" (D217). While the system has not
/// answered (`Mode::None`) the light theme is used. The accent color travels
/// inside the theme (spec M9-1, D1603).
pub fn resolve(mode: ThemeMode, system: iced::theme::Mode, accent: AccentId) -> Theme {
    let dark = match mode {
        ThemeMode::Light => false,
        ThemeMode::Dark => true,
        ThemeMode::System => matches!(system, iced::theme::Mode::Dark),
    };
    palette::iced_theme(dark, accent)
}

/// Text color that reads well on `background`.
pub fn contrast_on(background: Color) -> Color {
    let luminance = 0.2126 * background.r + 0.7152 * background.g + 0.0722 * background.b;
    if luminance > 0.55 {
        Color::BLACK
    } else {
        Color::WHITE
    }
}

/// The title bar of a pane: thin, only a handle to drag (D214).
pub fn pane_title(theme: &Theme) -> container::Style {
    let t = Tokens::of(theme);
    container::Style {
        text_color: Some(t.muted),
        background: Some(t.window.into()),
        ..container::Style::default()
    }
}

/// The body of a pane: a rounded panel without a border (D214).
pub fn pane_body(theme: &Theme) -> container::Style {
    let t = Tokens::of(theme);
    container::Style {
        text_color: Some(t.text),
        background: Some(t.panel.into()),
        border: Border {
            radius: t.radius.panel.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// The look of the pane grid: the drop target and the dividers in the
/// accent (D214).
pub fn pane_grid(theme: &Theme) -> iced::widget::pane_grid::Style {
    use iced::widget::pane_grid::{Highlight, Line, Style};
    let t = Tokens::of(theme);
    Style {
        hovered_region: Highlight {
            background: t.accent.scale_alpha(0.25).into(),
            border: Border {
                width: 2.0,
                color: t.accent,
                radius: t.radius.panel.into(),
            },
        },
        hovered_split: Line {
            color: t.accent.scale_alpha(0.6),
            width: 2.0,
        },
        picked_split: Line {
            color: t.accent,
            width: 2.0,
        },
    }
}

/// The content of a pane: transparent, the body paints its own rounded
/// panel.
pub fn pane_content(_theme: &Theme) -> container::Style {
    container::Style::default()
}

/// A tab of the notation panel: flat text, the active one in full strength
/// (the underline is [`tab_underline`]).
pub fn tab_button(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let t = Tokens::of(theme);
        let color = if active { t.text } else { t.muted };
        let background = match status {
            button::Status::Hovered => Some(t.hover.into()),
            button::Status::Pressed => Some(t.pressed.into()),
            button::Status::Active | button::Status::Disabled => None,
        };
        button::Style {
            background,
            text_color: color,
            border: Border {
                radius: t.radius.control.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

/// How a tab of the strip of databases looks (spec M3-4d).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TabLook {
    /// The tab of the current database.
    pub active: bool,
    /// The pointer is over it.
    pub hovered: bool,
    /// A drag is over it and a drop here would copy games.
    pub drop_target: bool,
    /// The drag started here and is now over another tab.
    pub drag_source: bool,
}

/// The surface of a tab that is not a button (so it can see the whole
/// drag): flat text, a wash on hover, and an accent frame when it is the
/// target of a drop.
pub fn tab_surface(look: TabLook) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let t = Tokens::of(theme);
        let text_color = if look.active || look.drop_target {
            t.text
        } else {
            t.muted
        };
        let (background, border) = if look.drop_target {
            (
                Some(t.accent.scale_alpha(0.2).into()),
                Border {
                    color: t.accent,
                    width: 1.5,
                    radius: t.radius.control.into(),
                },
            )
        } else if look.drag_source {
            (
                Some(t.pressed.into()),
                Border {
                    radius: t.radius.control.into(),
                    ..Border::default()
                },
            )
        } else if look.hovered {
            (
                Some(t.hover.into()),
                Border {
                    radius: t.radius.control.into(),
                    ..Border::default()
                },
            )
        } else {
            (
                None,
                Border {
                    radius: t.radius.control.into(),
                    ..Border::default()
                },
            )
        };
        container::Style {
            text_color: Some(text_color),
            background,
            border,
            ..container::Style::default()
        }
    }
}

/// The line under a tab: the accent when active, nothing otherwise.
pub fn tab_underline(active: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let t = Tokens::of(theme);
        container::Style {
            background: Some(if active { t.accent } else { Color::TRANSPARENT }.into()),
            ..container::Style::default()
        }
    }
}

/// The strip under the tabs and above the command bar: a hairline.
pub fn divider(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Tokens::of(theme).border.into()),
        ..container::Style::default()
    }
}

/// An accent dot (the Comment tab when the move has a comment).
pub fn accent_text(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(Tokens::of(theme).accent),
    }
}

/// A round sample of an accent color in a menu (spec M9-1); the current one
/// has a thick ring in the text color (besides the check mark, D1605).
pub fn swatch(theme: &Theme, color: Color, current: bool) -> container::Style {
    let t = Tokens::of(theme);
    container::Style {
        background: Some(color.into()),
        border: Border {
            radius: 999.0.into(),
            width: if current { 2.0 } else { 1.0 },
            color: if current { t.text } else { t.border },
        },
        ..container::Style::default()
    }
}

/// A card to pick one of several looks (board themes, piece sets in the
/// appearance window, spec M9-3): a raised surface, the chosen one framed in
/// the accent (besides the mark the caller draws, so it does not rest on
/// color alone).
pub fn choice_card(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let t = Tokens::of(theme);
        let background = match status {
            button::Status::Hovered => tokens::mix(t.text, t.raised, 0.07),
            button::Status::Pressed => tokens::mix(t.text, t.raised, 0.12),
            button::Status::Active | button::Status::Disabled => t.raised,
        };
        button::Style {
            background: Some(background.into()),
            text_color: t.text,
            border: Border {
                radius: t.radius.card.into(),
                width: if selected { 2.0 } else { 1.0 },
                color: if selected { t.accent } else { t.border },
            },
            ..button::Style::default()
        }
    }
}

/// The side column and the preview backdrop of the appearance window: the
/// window color, rounded.
pub fn modal_sidebar(theme: &Theme) -> container::Style {
    let t = Tokens::of(theme);
    container::Style {
        text_color: Some(t.text),
        background: Some(t.window.into()),
        border: Border {
            radius: t.radius.panel.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// A flat fill with the accent and the text over it: the primary button of
/// a miniature of the interface.
pub fn accent_fill(theme: &Theme) -> container::Style {
    let t = Tokens::of(theme);
    container::Style {
        text_color: Some(t.on_accent),
        background: Some(t.accent.into()),
        border: Border {
            radius: t.radius.control.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// The surface of the ribbon under the tabs (spec M9-2): a raised band with
/// a hairline border.
pub fn ribbon_panel(theme: &Theme) -> container::Style {
    let t = Tokens::of(theme);
    container::Style {
        background: Some(t.raised.into()),
        border: Border {
            color: t.border,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

/// A menu entry: flat, highlighted on hover, muted when disabled.
pub fn menu_item(theme: &Theme, status: button::Status) -> button::Style {
    let t = Tokens::of(theme);
    let base = button::Style {
        background: None,
        text_color: t.text,
        border: Border {
            radius: t.radius.control.into(),
            ..Border::default()
        },
        ..button::Style::default()
    };
    match status {
        button::Status::Active => base,
        button::Status::Hovered => button::Style {
            background: Some(t.hover.into()),
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(t.pressed.into()),
            ..base
        },
        button::Status::Disabled => button::Style {
            text_color: t.disabled(),
            ..base
        },
    }
}

/// The colour of a dot of the FICS seek graph: lightning, blitz, standard
/// and anything else, in this order (spec M8-9c, D2626).
pub fn seek_dot(class: usize) -> Color {
    match class {
        0 => Color::from_rgb(0.85, 0.25, 0.2),
        1 => Color::from_rgb(0.2, 0.45, 0.9),
        2 => Color::from_rgb(0.2, 0.65, 0.3),
        _ => Color::from_rgb(0.55, 0.55, 0.55),
    }
}

/// The dim text of shortcut labels.
pub fn muted_text(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(Tokens::of(theme).muted),
    }
}

/// The dimmed backdrop of a modal box.
pub fn scrim(theme: &Theme) -> container::Style {
    let t = Tokens::of(theme);
    container::Style {
        background: Some(t.window.scale_alpha(0.6).into()),
        ..container::Style::default()
    }
}

/// The status bar.
pub fn status_bar(theme: &Theme) -> container::Style {
    let t = Tokens::of(theme);
    container::Style {
        text_color: Some(t.muted),
        background: Some(t.window.into()),
        ..container::Style::default()
    }
}

/// A tooltip balloon: raised surface, rounded, with a thin border.
pub fn tooltip_box(theme: &Theme) -> container::Style {
    let t = Tokens::of(theme);
    container::Style {
        text_color: Some(t.text),
        background: Some(t.raised.into()),
        border: Border {
            radius: t.radius.control.into(),
            width: 1.0,
            color: t.border,
        },
        ..container::Style::default()
    }
}

/// How a row of the game list is painted (spec M3-4, D540).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RowLook {
    /// An odd row of the list: the zebra background.
    pub zebra: bool,
    /// The selected row.
    pub selected: bool,
    /// The game being edited.
    pub current: bool,
    /// The game is marked for deletion: dimmed text.
    pub dimmed: bool,
}

/// Share of the accent in the selected row of a list. The largest value at
/// which the muted text keeps 4.5:1 for every accent in both themes (the
/// dark Yellow allows 0.16; spec M9-1 D1636).
const SELECTED_ROW_TINT: f32 = 0.15;
/// Share of the accent in the row of the game being edited.
const CURRENT_ROW_TINT: f32 = 0.08;

/// The container style of a game-list row: the selection and the current
/// game get accent tints, the rest a zebra background.
pub fn list_row(look: RowLook) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let t = Tokens::of(theme);
        let background = if look.selected {
            Some(tokens::mix(t.accent, t.panel, SELECTED_ROW_TINT))
        } else if look.current {
            Some(tokens::mix(t.accent, t.panel, CURRENT_ROW_TINT))
        } else if look.zebra {
            Some(t.zebra)
        } else {
            None
        };
        container::Style {
            text_color: Some(if look.dimmed { t.disabled() } else { t.text }),
            background: background.map(Into::into),
            border: Border {
                radius: t.radius.chip.into(),
                ..Border::default()
            },
            ..container::Style::default()
        }
    }
}

/// The header row of the game list: muted text on the window color.
pub fn list_header(theme: &Theme) -> container::Style {
    let t = Tokens::of(theme);
    container::Style {
        text_color: Some(t.muted),
        background: Some(t.window.into()),
        ..container::Style::default()
    }
}

/// The scroll bar of the game list (a vertical slider): a thin rail and a
/// handle in the accent.
pub fn list_scrollbar(
    theme: &Theme,
    status: iced::widget::vertical_slider::Status,
) -> iced::widget::vertical_slider::Style {
    use iced::widget::slider::Rail;
    use iced::widget::vertical_slider::{Handle, HandleShape, Status, Style};
    let t = Tokens::of(theme);
    let handle = match status {
        Status::Active => tokens::mix(t.muted, t.panel, 0.6),
        Status::Hovered => t.muted,
        Status::Dragged => t.accent,
    };
    Style {
        rail: Rail {
            backgrounds: (t.border.into(), t.border.into()),
            width: 2.0,
            border: Border::default(),
        },
        handle: Handle {
            shape: HandleShape::Rectangle {
                width: 28,
                border_radius: t.radius.chip.into(),
            },
            background: handle.into(),
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
        },
    }
}

/// What a command-bar button looks like (D225, D227).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonLook {
    /// Large buttons of the primary row: a raised surface.
    Primary,
    /// Small buttons of the secondary row: flat until hovered.
    Secondary,
}

/// The style of a command-bar button. `toggled` buttons (trial mode on,
/// autoplay running) fill with the accent; the hover and pressed states
/// come from the tokens (D220); a disabled button keeps its surface and
/// the caller dims the icon.
pub fn command_button(
    look: ButtonLook,
    toggled: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let t = Tokens::of(theme);
        let rest = if toggled {
            t.accent
        } else {
            match look {
                ButtonLook::Primary => t.raised,
                ButtonLook::Secondary => Color::TRANSPARENT,
            }
        };
        let background = match status {
            button::Status::Active | button::Status::Disabled => rest,
            button::Status::Hovered if toggled => tokens::mix(t.on_accent, t.accent, 0.12),
            button::Status::Pressed if toggled => tokens::mix(t.on_accent, t.accent, 0.2),
            button::Status::Hovered => t.hover,
            button::Status::Pressed => t.pressed,
        };
        button::Style {
            background: Some(background.into()),
            text_color: if toggled { t.on_accent } else { t.text },
            border: Border {
                radius: t.radius.control.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use iced::theme::Mode;

    use super::*;

    #[test]
    fn explicit_modes_ignore_the_system() {
        assert_eq!(
            resolve(ThemeMode::Light, Mode::Dark, AccentId::Mychess),
            palette::iced_theme(false, AccentId::Mychess)
        );
        assert_eq!(
            resolve(ThemeMode::Dark, Mode::Light, AccentId::Mychess),
            palette::iced_theme(true, AccentId::Mychess)
        );
    }

    #[test]
    fn system_mode_follows_the_system_and_defaults_to_light() {
        let (dark, light) = (
            palette::iced_theme(true, AccentId::Mychess),
            palette::iced_theme(false, AccentId::Mychess),
        );
        assert_eq!(
            resolve(ThemeMode::System, Mode::Dark, AccentId::Mychess),
            dark
        );
        assert_eq!(
            resolve(ThemeMode::System, Mode::Light, AccentId::Mychess),
            light
        );
        assert_eq!(
            resolve(ThemeMode::System, Mode::None, AccentId::Mychess),
            light
        );
    }

    #[test]
    fn resolved_themes_are_the_custom_mychess_ones() {
        assert_eq!(
            resolve(ThemeMode::Dark, Mode::None, AccentId::Mychess).to_string(),
            palette::DARK_NAME
        );
        assert_eq!(
            resolve(ThemeMode::Light, Mode::None, AccentId::Mychess).to_string(),
            palette::LIGHT_NAME
        );
    }

    #[test]
    fn command_buttons_react_to_the_state() {
        let theme = resolve(ThemeMode::Dark, Mode::None, AccentId::Mychess);
        let t = Tokens::of(&theme);
        let bg = |look, toggled, status| command_button(look, toggled)(&theme, status).background;
        assert_eq!(
            bg(ButtonLook::Primary, false, button::Status::Active),
            Some(t.raised.into())
        );
        assert_eq!(
            bg(ButtonLook::Secondary, false, button::Status::Hovered),
            Some(t.hover.into())
        );
        assert_eq!(
            bg(ButtonLook::Secondary, false, button::Status::Pressed),
            Some(t.pressed.into())
        );
        assert_eq!(
            bg(ButtonLook::Secondary, true, button::Status::Active),
            Some(t.accent.into())
        );
        assert_eq!(
            command_button(ButtonLook::Secondary, true)(&theme, button::Status::Active).text_color,
            t.on_accent
        );
    }

    #[test]
    fn contrast_picks_dark_text_on_light_backgrounds() {
        assert_eq!(contrast_on(Color::WHITE), Color::BLACK);
        assert_eq!(contrast_on(Color::BLACK), Color::WHITE);
    }
}
