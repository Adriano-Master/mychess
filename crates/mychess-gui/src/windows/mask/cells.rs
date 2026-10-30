//! What a mask adds to the table of the tree (spec M5-4c, D1827 to D1829):
//! the group of marks before the move, the comment column, the strip with
//! the comment of the position and the title line.
//!
//! Colors are never the only indicator: the NAG is text and every marker is
//! a distinct shape (and has a name in the menu that sets it).

use std::path::PathBuf;

use fluent_bundle::FluentArgs;
use iced::widget::text::Wrapping;
use iced::widget::{column, container, mouse_area, row, space, text, tooltip};
use iced::{Alignment, Border, Element, Length};
use mychess_app::{Marker, MaskBusy, MaskCell, MaskView, TreeView};

use super::state::{Hover, MaskUiMessage};
use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme::mask_colors::{self, MaskColors};
use crate::theme::{self, Tokens};
use crate::widgets::icon::{Icon, IconTone, icon};

/// Width of the comment column.
pub(crate) const COMMENT_WIDTH: f32 = 240.0;
/// Size of a marker icon.
const MARKER_SIZE: f32 = 14.0;
/// Side of the color swatch.
const SWATCH: f32 = 12.0;
/// Width of the NAG slot (two characters).
const NAG_WIDTH: f32 = 24.0;
/// Space between the slots.
const GAP: f32 = 3.0;
/// Text size of the table (the same as the tree's).
const TEXT_SIZE: f32 = 12.0;
/// Width of the group of marks.
pub(crate) const MARKS_WIDTH: f32 = 2.0 * MARKER_SIZE + SWATCH + NAG_WIDTH + 3.0 * GAP;

/// What the panel needs to know about the masks: the open mask, the
/// preferences, the job that runs and the comment under the pointer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MaskCtx {
    /// A mask is open.
    pub open: bool,
    /// The game is standard chess (D1835).
    pub enabled: bool,
    /// The mask has unsaved changes.
    pub dirty: bool,
    /// Draw the marks (D1831).
    pub show_markers: bool,
    /// Show the whole comment in a tool tip.
    pub tooltips: bool,
    /// "Load the last mask when the tree opens".
    pub auto_load: bool,
    /// The recent files, newest first.
    pub recent: Vec<PathBuf>,
    /// The job that runs.
    pub busy: Option<MaskBusy>,
    /// The comment under the pointer with its whole text.
    pub hover: Option<(Hover, String)>,
    /// The command key is held.
    pub command: bool,
    /// The game of the editor can take a merge now (spec M7-6d). The
    /// database is the one of the rows, not the current one.
    pub can_merge: bool,
}

impl MaskCtx {
    /// The context when only the snapshot is known (tests, previews): the
    /// preferences come from the mask view, the recent list too.
    pub fn from_view(view: &TreeView) -> MaskCtx {
        match &view.mask {
            Some(mask) => MaskCtx {
                open: true,
                enabled: mask.enabled,
                dirty: mask.dirty,
                show_markers: mask.show_markers,
                tooltips: mask.tooltips,
                auto_load: mask.auto_load,
                recent: mask.recent.clone(),
                busy: mask.busy,
                hover: None,
                command: false,
                can_merge: false,
            },
            None => MaskCtx {
                enabled: true,
                show_markers: true,
                tooltips: true,
                ..MaskCtx::default()
            },
        }
    }

    /// The context of the shell: the preferences are known even without an
    /// open mask, and the comment under the pointer is fetched whole.
    pub fn of(shell: &Shell, view: &TreeView) -> MaskCtx {
        let app = &shell.app;
        let prefs = &app.prefs.mask;
        let mut ctx = MaskCtx::from_view(view);
        ctx.enabled = app.session.game().variant() == mychess_core::Variant::Standard;
        ctx.show_markers = prefs.show_markers;
        ctx.tooltips = prefs.tooltips;
        ctx.auto_load = prefs.auto_load;
        ctx.recent.clone_from(&prefs.recent);
        ctx.busy = app.mask_busy();
        ctx.command = shell.ui.games.command;
        ctx.can_merge = crate::windows::merge_dialog::can_merge(app);
        if ctx.tooltips
            && ctx.open
            && let Some(hover) = &shell.ui.dialogs.mask.hover
        {
            let full = match hover {
                Hover::Position => app.mask_comment(None),
                Hover::Row(target) => app.mask_comment(Some(target)),
            };
            ctx.hover = full.map(|text| (hover.clone(), text));
        }
        ctx
    }

    /// The group of marks is drawn (a mask is open and the option is on).
    pub fn marks_visible(&self) -> bool {
        self.open && self.show_markers
    }
}

fn ui_message(message: MaskUiMessage) -> Message {
    Message::MaskUi(message)
}

/// The icon of a marker; the two shapes that stand for White and Black are
/// drawn by [`disc`].
fn marker_icon(marker: &Marker) -> Icon {
    match marker {
        Marker::Include => Icon::KeepSelected,
        Marker::Exclude => Icon::RemoveFromFilter,
        Marker::MainLine => Icon::Arrow,
        Marker::Bookmark => Icon::Bookmark,
        Marker::NewLine => Icon::AddVariation,
        Marker::ToBeVerified => Icon::Search,
        Marker::ToTrain => Icon::Target,
        Marker::Dubious => Icon::Negate,
        Marker::ToRemove => Icon::Trash,
        _ => Icon::Marks,
    }
}

/// A disc: hollow for White, filled for Black.
fn disc<'a>(size: f32, filled: bool) -> Element<'a, Message> {
    let inner = container(space())
        .width(Length::Fixed(size - 2.0))
        .height(Length::Fixed(size - 2.0))
        .style(move |theme| {
            let t = Tokens::of(theme);
            container::Style {
                background: filled.then(|| t.text.into()),
                border: Border {
                    color: t.text,
                    width: 1.5,
                    radius: (size / 2.0).into(),
                },
                ..container::Style::default()
            }
        });
    container(inner).center(Length::Fixed(size)).into()
}

/// The glyph of a marker, `size` pixels wide.
pub(crate) fn marker_glyph<'a>(marker: &Marker, size: f32) -> Element<'a, Message> {
    match marker {
        Marker::White => disc(size, false),
        Marker::Black => disc(size, true),
        other => icon(marker_icon(other), size, IconTone::Text),
    }
}

/// A square with the color of a mask color (nothing for the default).
pub(crate) fn swatch<'a>(color: mychess_app::MaskColor, size: f32) -> Element<'a, Message> {
    container(space())
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |theme| {
            let hue = MaskColors::of(&Tokens::of(theme)).swatch(&color);
            match hue {
                Some(hue) => mask_colors::swatch_box(theme, hue),
                None => container::Style::default(),
            }
        })
        .into()
}

fn slot(content: Option<Element<'_, Message>>, width: f32) -> Element<'_, Message> {
    container(content.unwrap_or_else(|| space().into()))
        .width(Length::Fixed(width))
        .center_y(Length::Shrink)
        .into()
}

/// The group of marks of a row: the two markers, the swatch and the NAG.
/// A row with no cell (`[end]`, the total) gets the same room, empty, so the
/// columns line up (D1827).
pub(crate) fn marks<'a>(cell: Option<&MaskCell>) -> Element<'a, Message> {
    let Some(cell) = cell else {
        return space().width(Length::Fixed(MARKS_WIDTH)).into();
    };
    let marker = |index: usize| {
        cell.markers
            .get(index)
            .and_then(Option::as_ref)
            .map(|m| marker_glyph(m, MARKER_SIZE))
    };
    let color =
        (cell.color != mychess_app::MaskColor::White).then(|| swatch(cell.color.clone(), SWATCH));
    let nag = cell.nag.as_ref().map(|nag| {
        text(nag.as_str().to_owned())
            .size(TEXT_SIZE)
            .wrapping(Wrapping::None)
            .into()
    });
    row![
        slot(marker(0), MARKER_SIZE),
        slot(marker(1), MARKER_SIZE),
        slot(color, SWATCH),
        slot(nag, NAG_WIDTH),
    ]
    .spacing(GAP)
    .width(Length::Fixed(MARKS_WIDTH))
    .align_y(Alignment::Center)
    .into()
}

/// The text of a move of the table: highlighted when it is in the mask
/// (D1827), in the danger tone when the entry is illegal (case 12).
pub(crate) fn move_text<'a>(
    san: String,
    width: f32,
    in_mask: bool,
    illegal: bool,
) -> Element<'a, Message> {
    text(san)
        .size(TEXT_SIZE)
        .width(Length::Fixed(width))
        .style(move |theme| {
            let t = Tokens::of(theme);
            let color = if illegal {
                Some(t.danger)
            } else if in_mask {
                Some(MaskColors::of(&t).highlight)
            } else {
                None
            };
            text::Style { color }
        })
        .into()
}

/// The first line of a comment, with an ellipsis when there is more.
fn shown(comment: &str, more: bool) -> String {
    if more {
        format!("{comment}…")
    } else {
        comment.to_owned()
    }
}

/// The most a tool tip shows of a comment (a comment may have 64 KiB).
const TIP_MAX_LINES: usize = 20;
const TIP_MAX_CHARS: usize = 2000;

/// `text` cut to what a tool tip can hold, with an ellipsis when cut.
pub fn clip_tip(text: &str) -> String {
    let mut out = String::new();
    let mut lines = 0;
    for (count, ch) in text.chars().enumerate() {
        if count >= TIP_MAX_CHARS || (ch == '\n' && lines + 1 >= TIP_MAX_LINES) {
            out.push('…');
            return out;
        }
        if ch == '\n' {
            lines += 1;
        }
        out.push(ch);
    }
    out
}

fn tip<'a>(full: &str) -> Element<'a, Message> {
    container(text(clip_tip(full)).size(12))
        .max_width(420.0)
        .padding([4, 8])
        .style(theme::tooltip_box)
        .into()
}

/// The comment column of a row. With tool tips on, the pointer over it asks
/// the shell for the whole text (D1827); a comment that is one line is its
/// own tool tip.
pub(crate) fn comment_cell<'a>(
    cell: Option<&MaskCell>,
    hover: Hover,
    ctx: &MaskCtx,
) -> Element<'a, Message> {
    let Some(cell) = cell.filter(|c| !c.comment.is_empty()) else {
        return space().width(Length::Fixed(COMMENT_WIDTH)).into();
    };
    let label = container(
        text(shown(&cell.comment, cell.comment_more))
            .size(TEXT_SIZE)
            .wrapping(Wrapping::None),
    )
    .width(Length::Fixed(COMMENT_WIDTH))
    .clip(true);
    if !ctx.tooltips {
        return label.into();
    }
    let full = match &ctx.hover {
        Some((at, text)) if *at == hover => text.clone(),
        _ => shown(&cell.comment, cell.comment_more),
    };
    let body: Element<'a, Message> = if cell.comment_more {
        mouse_area(label)
            .on_enter(ui_message(MaskUiMessage::Hover(Some(hover))))
            .on_exit(ui_message(MaskUiMessage::Hover(None)))
            .into()
    } else {
        label.into()
    };
    tooltip(body, tip(&full), tooltip::Position::Bottom).into()
}

/// The line above the table: the name of the file, with a dot when the mask
/// was altered (D1830).
pub(crate) fn title<'a>(view: &MaskView, i18n: &I18n) -> Element<'a, Message> {
    let mut args = FluentArgs::new();
    args.set(
        "name",
        view.file_name
            .clone()
            .unwrap_or_else(|| i18n.tr("tree-mask-unnamed")),
    );
    let key = if view.dirty {
        "tree-mask-title-dirty"
    } else {
        "tree-mask-title"
    };
    text(i18n.tr_args(key, &args))
        .size(12)
        .style(theme::muted_text)
        .into()
}

/// The strip with the first line of the comment of the position (D1829); a
/// double click opens the editor.
pub(crate) fn strip<'a>(view: &MaskView, ctx: &MaskCtx) -> Option<Element<'a, Message>> {
    if view.position_comment.is_empty() {
        return None;
    }
    let label = container(
        text(shown(&view.position_comment, view.position_comment_more))
            .size(TEXT_SIZE + 1.0)
            .wrapping(Wrapping::None),
    )
    .padding([4, 8])
    .width(Length::Fill)
    .clip(true)
    .style(mask_colors::strip);
    let mut area =
        mouse_area(label).on_double_click(ui_message(MaskUiMessage::OpenPositionComment));
    if view.position_comment_more && ctx.tooltips {
        area = area
            .on_enter(ui_message(MaskUiMessage::Hover(Some(Hover::Position))))
            .on_exit(ui_message(MaskUiMessage::Hover(None)));
    }
    if !ctx.tooltips {
        return Some(area.into());
    }
    let full = match &ctx.hover {
        Some((Hover::Position, text)) => text.clone(),
        _ => shown(&view.position_comment, view.position_comment_more),
    };
    Some(tooltip(area, tip(&full), tooltip::Position::Bottom).into())
}

/// A column with the title and the strip, when there is something to show.
pub(crate) fn header<'a>(view: &MaskView, ctx: &MaskCtx, i18n: &I18n) -> Element<'a, Message> {
    let mut page = column![title(view, i18n)].spacing(4);
    if let Some(strip) = strip(view, ctx) {
        page = page.push(strip);
    }
    page.into()
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    #[test]
    fn the_marks_group_has_a_fixed_width() {
        assert_eq!(MARKS_WIDTH, 2.0 * 14.0 + 12.0 + 24.0 + 9.0);
    }

    #[test]
    fn a_tool_tip_is_cut_at_twenty_lines_or_two_thousand_characters() {
        assert_eq!(clip_tip("short"), "short");
        let mut lines = String::new();
        for i in 0..30 {
            let _ = writeln!(lines, "l{i}");
        }
        let cut = clip_tip(&lines);
        assert!(cut.ends_with('…'));
        assert_eq!(cut.lines().count(), 20);
        let long = "é".repeat(5000);
        assert_eq!(clip_tip(&long).chars().count(), 2001);
    }

    #[test]
    fn a_cut_comment_ends_in_an_ellipsis() {
        assert_eq!(shown("book move", true), "book move…");
        assert_eq!(shown("book move", false), "book move");
    }

    #[test]
    fn every_marker_but_the_two_discs_has_an_icon() {
        for marker in Marker::ALL {
            if !matches!(marker, Marker::White | Marker::Black) {
                assert_ne!(marker_icon(&marker), Icon::Marks, "{marker:?}");
            }
        }
    }
}
