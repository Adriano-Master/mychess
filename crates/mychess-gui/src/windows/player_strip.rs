//! One player strip above or below the board (spec M2-5, D230 to D238).
//!
//! The strip is made of ordinary iced widgets, not canvas drawing: an avatar
//! (the generic silhouette), the title, the name, the rating, the captured
//! pieces with the points ahead, and the clock. What it shows comes from the
//! application ([`PlayerStripView`]); how much of it fits comes from the
//! strip mode that [`column_metrics`](crate::widgets::board::geometry::column_metrics)
//! picked. Everything that can be decided without a window (clock text,
//! truncation, which pieces and of which color, the tones of the two sides)
//! is a plain function with its own tests.
// Pixel estimates on small bounded counts.
#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]

use fluent_bundle::FluentArgs;
use iced::widget::{button, container, row, svg, text, tooltip};
use iced::{Alignment, Color, Element, Length, Padding, Theme};
use mychess_core::{Color as Side, Piece};

use crate::app_api::{MaterialMode, PlayerStripView, PlayersPrefs, RatingView};
use crate::fonts::NOTO_SANS_BOLD;
use crate::i18n::I18n;
use crate::theme::{self, Tokens};
use crate::widgets::board::geometry::StripMode;
use crate::widgets::board::pieces::PieceSet;
use crate::widgets::icon::Icon;

/// Clocks at or below this many seconds get the danger tint (D235).
pub const LOW_CLOCK_SECONDS: u32 = 10;

/// Estimated width of a character, in ems (Noto Sans Bold averages about 0.6;
/// the slack covers wide letters, so a cut name never overflows its line).
const CHAR_EM: f32 = 0.66;
/// The name is cut to this many characters before the rating is touched.
const MIN_NAME_CHARS: usize = 4;
/// Side of the avatar of a full strip (D232).
const AVATAR: f32 = 40.0;
/// Side of the side-to-move dot (D236).
const DOT: f32 = 8.0;
/// Horizontal padding inside a strip.
const PAD_X: f32 = 8.0;
/// Space between the parts of a line.
const GAP: f32 = 6.0;
/// Space between the avatar, the body and the clock of a strip.
const PART_GAP: f32 = GAP + 2.0;
/// Room left unused on a line, against the error of the text estimates.
const SLACK: f32 = 8.0;
/// Font size of the title chip.
const TITLE_SIZE: f32 = 11.0;
/// Largest and smallest side of a captured piece.
const PIECE_MAX: f32 = 16.0;
const PIECE_MIN: f32 = 10.0;

/// Seconds as `m:ss`, or `h:mm:ss` from one hour on (D235).
pub fn format_clock(seconds: u32) -> String {
    let (hours, minutes, secs) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

/// `text` cut to at most `max_chars` characters, the last one being `…` when
/// something was cut.
pub fn ellipsize(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    match max_chars {
        0 => String::new(),
        n => {
            let mut out: String = text.chars().take(n - 1).collect();
            out.push('\u{2026}');
            out
        }
    }
}

fn text_width(chars: usize, size: f32) -> f32 {
    chars as f32 * size * CHAR_EM
}

fn chars_in(width: f32, size: f32) -> usize {
    if width <= 0.0 || !width.is_finite() {
        return 0;
    }
    // Non-negative here, and a few hundred at most.
    #[allow(clippy::cast_sign_loss)]
    let chars = (width / (size * CHAR_EM)).floor() as usize;
    chars
}

/// The name and the rating as they fit in `avail` pixels (D237): the name is
/// cut first, down to `MIN_NAME_CHARS`; then the rating is cut, and
/// dropped when not even two characters of it fit.
pub fn fit_name_and_rating(
    avail: f32,
    name: &str,
    rating: Option<&str>,
    name_size: f32,
    rating_size: f32,
) -> (String, Option<String>) {
    let name_len = name.chars().count();
    let Some(rating) = rating else {
        return (ellipsize(name, chars_in(avail, name_size)), None);
    };
    let rating_len = rating.chars().count();
    let wanted = text_width(name_len, name_size) + GAP + text_width(rating_len, rating_size);
    if wanted <= avail {
        return (name.to_owned(), Some(rating.to_owned()));
    }
    let name_room = avail - GAP - text_width(rating_len, rating_size);
    let name_budget = chars_in(name_room, name_size);
    if name_budget >= MIN_NAME_CHARS.min(name_len) {
        return (ellipsize(name, name_budget), Some(rating.to_owned()));
    }
    let kept = MIN_NAME_CHARS.min(name_len);
    let rating_room = avail - GAP - text_width(kept, name_size);
    let rating_budget = chars_in(rating_room, rating_size);
    if rating_budget >= 2 {
        (
            ellipsize(name, kept),
            Some(ellipsize(rating, rating_budget)),
        )
    } else {
        (ellipsize(name, chars_in(avail, name_size)), None)
    }
}

/// The pieces a strip draws, grouped by type, and the color they have.
///
/// With `Difference` they are the surplus pieces of the strip's own side;
/// with `Captured` they are the pieces of the other side that this side
/// took (see `mychess_app::Material`). `Off` draws nothing.
pub fn strip_pieces(view: &PlayerStripView, mode: MaterialMode) -> Vec<(Piece, u8)> {
    let color = match mode {
        MaterialMode::Off => return Vec::new(),
        MaterialMode::Difference => view.color,
        MaterialMode::Captured => other(view.color),
    };
    view.captured
        .iter()
        .map(|&(role, count)| (Piece { color, role }, count))
        .collect()
}

fn other(side: Side) -> Side {
    match side {
        Side::White => Side::Black,
        Side::Black => Side::White,
    }
}

/// The light and the dark tone of the palette, in that order: the colors of
/// the white and the black side in the avatar and in the side-to-move dot.
/// Both come from the tokens, so they follow the theme (the light one is
/// the more luminous of the text and panel colors).
pub fn side_tones(tokens: &Tokens) -> (Color, Color) {
    let luminance = |c: Color| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    if luminance(tokens.text) >= luminance(tokens.panel) {
        (tokens.text, tokens.panel)
    } else {
        (tokens.panel, tokens.text)
    }
}

fn side_tone(tokens: &Tokens, side: Side) -> Color {
    let (light, dark) = side_tones(tokens);
    match side {
        Side::White => light,
        Side::Black => dark,
    }
}

/// How the clock of a strip is painted: `(background, text)`.
///
/// The side to move has the accent background (or the danger one when its
/// clock is low); the other side sits on the raised surface, with the text
/// in the danger tint when low.
pub fn clock_colors(tokens: &Tokens, to_move: bool, seconds: Option<u32>) -> (Color, Color) {
    let low = seconds.is_some_and(|s| s <= LOW_CLOCK_SECONDS);
    match (to_move, low) {
        (true, true) => (tokens.danger, tokens.on_accent),
        (true, false) => (tokens.accent, tokens.on_accent),
        (false, true) => (tokens.raised, tokens.danger),
        (false, false) => (tokens.raised, tokens.text),
    }
}

/// The text of the clock, if the strip draws one: the time, or the `none`
/// placeholder when the game has clocks but this side has no value yet.
pub fn clock_text(
    view: &PlayerStripView,
    clock_slot: bool,
    show_clocks: bool,
    none: &str,
) -> Option<String> {
    if !show_clocks || !clock_slot {
        return None;
    }
    Some(view.clock.map_or_else(|| none.to_owned(), format_clock))
}

/// The width in characters both clocks of a column reserve: wide enough for
/// `h:mm:ss` when some clock needs it, else for `mm:ss`.
pub fn clock_chars<'a>(views: impl IntoIterator<Item = &'a PlayerStripView>) -> usize {
    let hours = views
        .into_iter()
        .any(|v| v.clock.is_some_and(|s| s >= 3600));
    if hours { 7 } else { 5 }
}

/// What the strip needs besides the [`PlayerStripView`].
pub struct StripContext<'a> {
    /// `Full` or `Compact` (a hidden strip is not built).
    pub mode: StripMode,
    /// Width of the strip: the width of the board.
    pub width: f32,
    /// The player strip preferences.
    pub prefs: PlayersPrefs,
    /// Whether the side to move gets a dot.
    pub show_side_to_move: bool,
    /// The game has clocks: draw the clock slot.
    pub clock_slot: bool,
    /// The clocks count down: only then is a small value "low time". The
    /// clocks of a game that only counts (fixed time per move, depth,
    /// nodes) are never painted as low (D1482).
    pub countdown: bool,
    /// Characters the clock slot reserves (see [`clock_chars`]).
    pub clock_chars: usize,
    /// The piece images (the selected set).
    pub pieces: &'a PieceSet,
    /// Texts.
    pub i18n: &'a I18n,
}

fn rating_text(rating: RatingView, i18n: &I18n) -> String {
    use mychess_core::RatingType;
    let kind = match rating.kind {
        RatingType::Rapid => Some("player-rating-kind-rapid"),
        RatingType::Uscf => Some("player-rating-kind-uscf"),
        RatingType::Dwz => Some("player-rating-kind-dwz"),
        RatingType::Ecf => Some("player-rating-kind-ecf"),
        RatingType::Iccf => Some("player-rating-kind-iccf"),
        _ => None,
    };
    match kind {
        Some(key) => format!("{} {}", rating.value, i18n.tr(key)),
        None => rating.value.to_string(),
    }
}

/// The tooltip of the name: title, name and rating.
fn name_tooltip(view: &PlayerStripView, rating: Option<&str>, i18n: &I18n) -> String {
    let Some(name) = &view.name else {
        return i18n.tr("player-unknown");
    };
    let mut args = FluentArgs::new();
    args.set("name", name.as_str());
    args.set("has_title", if view.title.is_some() { "yes" } else { "no" });
    args.set("title", view.title.clone().unwrap_or_default());
    args.set("has_rating", if rating.is_some() { "yes" } else { "no" });
    args.set("rating", rating.unwrap_or_default().to_owned());
    i18n.tr_args("player-tooltip", &args)
}

fn tip<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    label: String,
    position: tooltip::Position,
) -> Element<'a, M> {
    tooltip(content, text(label).size(12), position)
        .style(theme::tooltip_box)
        .into()
}

fn muted_style(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(Tokens::of(theme).muted),
    }
}

fn avatar<'a, M: 'a>(side: Side) -> Element<'a, M> {
    let glyph = svg(Icon::Avatar.handle())
        .width(Length::Fixed(AVATAR * 0.6))
        .height(Length::Fixed(AVATAR * 0.6))
        .style(move |theme: &Theme, _| {
            let tokens = Tokens::of(theme);
            svg::Style {
                color: Some(side_tone(&tokens, other(side))),
            }
        });
    container(glyph)
        .width(Length::Fixed(AVATAR))
        .height(Length::Fixed(AVATAR))
        .center_x(Length::Fixed(AVATAR))
        .center_y(Length::Fixed(AVATAR))
        .style(move |theme: &Theme| {
            let tokens = Tokens::of(theme);
            container::Style {
                background: Some(side_tone(&tokens, side).into()),
                border: iced::Border {
                    radius: tokens.radius.control.into(),
                    width: 1.0,
                    color: tokens.border,
                },
                ..container::Style::default()
            }
        })
        .into()
}

fn side_dot<'a, M: 'a>(side: Side) -> Element<'a, M> {
    container(iced::widget::Space::new())
        .width(Length::Fixed(DOT))
        .height(Length::Fixed(DOT))
        .style(move |theme: &Theme| {
            let tokens = Tokens::of(theme);
            container::Style {
                background: Some(side_tone(&tokens, side).into()),
                border: iced::Border {
                    radius: (DOT / 2.0).into(),
                    width: 1.0,
                    color: tokens.muted,
                },
                ..container::Style::default()
            }
        })
        .into()
}

fn title_chip<'a, M: 'a>(title: String) -> Element<'a, M> {
    container(text(title).size(TITLE_SIZE).font(NOTO_SANS_BOLD))
        .padding([1, 4])
        .style(|theme: &Theme| {
            let tokens = Tokens::of(theme);
            container::Style {
                text_color: Some(tokens.text),
                background: Some(tokens.raised.into()),
                border: iced::Border {
                    radius: tokens.radius.chip.into(),
                    ..iced::Border::default()
                },
                ..container::Style::default()
            }
        })
        .into()
}

/// The captured pieces, grouped by type, `size` pixels each.
fn pieces_row<'a, M: 'a>(pieces: &[(Piece, u8)], set: &PieceSet, size: f32) -> Element<'a, M> {
    let mut groups = row![].spacing(GAP - 2.0).align_y(Alignment::Center);
    for &(piece, count) in pieces {
        let Some(handle) = set.handle(piece) else {
            continue;
        };
        let mut group = row![].align_y(Alignment::Center);
        for _ in 0..count {
            group = group.push(
                svg(handle.clone())
                    .width(Length::Fixed(size))
                    .height(Length::Fixed(size)),
            );
        }
        groups = groups.push(group);
    }
    groups.into()
}

fn piece_count(pieces: &[(Piece, u8)]) -> usize {
    pieces.iter().map(|&(_, n)| usize::from(n)).sum()
}

/// The side of one captured piece so that `pieces` fit in `room` pixels.
fn piece_size(pieces: &[(Piece, u8)], room: f32, max: f32) -> f32 {
    let count = piece_count(pieces);
    if count == 0 {
        return max;
    }
    let gaps = (pieces.len().saturating_sub(1)) as f32 * (GAP - 2.0);
    ((room - gaps) / count as f32).clamp(PIECE_MIN, max)
}

fn clock_widget<'a, M: 'a>(
    time: String,
    view: &PlayerStripView,
    ctx: &StripContext<'_>,
    tip_position: tooltip::Position,
) -> Element<'a, M> {
    let full = ctx.mode == StripMode::Full;
    let size = if full {
        Tokens::dark().font.clock
    } else {
        Tokens::dark().font.base
    };
    let width = clock_width(ctx.clock_chars, size);
    let (to_move, seconds) = (view.to_move, view.clock);
    // A clock that only counts is never "low time".
    let low_source = seconds.filter(|_| ctx.countdown);
    let label = (seconds.is_some()).then(|| {
        let mut args = FluentArgs::new();
        args.set("time", time.clone());
        ctx.i18n.tr_args("player-clock", &args)
    });
    let clock = container(
        text(time)
            .size(size)
            .font(NOTO_SANS_BOLD)
            .wrapping(text::Wrapping::None),
    )
    .width(Length::Fixed(width))
    .padding([2, 8])
    .align_x(iced::alignment::Horizontal::Right)
    .style(move |theme: &Theme| {
        let tokens = Tokens::of(theme);
        let (background, color) = clock_colors(&tokens, to_move, low_source);
        container::Style {
            text_color: Some(color),
            background: Some(background.into()),
            border: iced::Border {
                radius: tokens.radius.control.into(),
                ..iced::Border::default()
            },
            ..container::Style::default()
        }
    });
    match label {
        Some(label) => tip(clock, label, tip_position),
        None => clock.into(),
    }
}

/// Width of the clock box for `chars` characters at `size`.
fn clock_width(chars: usize, size: f32) -> f32 {
    text_width(chars, size) + 2.0 * PAD_X
}

/// What a strip draws and how much room each part gets, decided before any
/// widget is built.
struct Plan {
    full: bool,
    name_size: f32,
    small: f32,
    rating: Option<String>,
    clock: Option<String>,
    pieces: Vec<(Piece, u8)>,
    piece_side: f32,
    advantage: Option<u32>,
    advantage_text: Option<String>,
    show_avatar: bool,
    show_dot: bool,
    /// Pixels the name and the rating may use.
    name_room: f32,
}

fn advantage_width(text: Option<&String>, small: f32) -> f32 {
    text.map_or(0.0, |t| text_width(t.chars().count(), small) + GAP)
}

fn plan(view: &PlayerStripView, ctx: &StripContext<'_>) -> Plan {
    let full = ctx.mode == StripMode::Full;
    let font = Tokens::dark().font;
    let (name_size, small) = (if full { font.name } else { font.base }, font.small);
    let rating = if ctx.prefs.show_ratings {
        view.rating.map(|r| rating_text(r, ctx.i18n))
    } else {
        None
    };
    let clock = clock_text(
        view,
        ctx.clock_slot,
        ctx.prefs.show_clocks,
        &ctx.i18n.tr("player-clock-none"),
    );
    let pieces = strip_pieces(view, ctx.prefs.material);
    let advantage = view.advantage.filter(|_| !pieces.is_empty() || full);
    let advantage_text = advantage.map(|points| {
        let mut args = FluentArgs::new();
        args.set("points", i64::from(points));
        ctx.i18n.tr_args("player-advantage-short", &args)
    });
    let show_avatar = full && ctx.prefs.show_avatars;
    let show_dot = ctx.show_side_to_move;

    let clock_font = if full { font.clock } else { font.base };
    let clock_px = clock
        .as_ref()
        .map_or(0.0, |_| clock_width(ctx.clock_chars, clock_font) + PART_GAP);
    let avatar_px = if show_avatar { AVATAR + PART_GAP } else { 0.0 };
    let dot_px = if show_dot { DOT + GAP } else { 0.0 };
    let title_px = view.title.as_ref().map_or(0.0, |t| {
        text_width(t.chars().count(), TITLE_SIZE) + 8.0 + GAP
    });
    let body_px = ctx.width - 2.0 * PAD_X - avatar_px - clock_px - SLACK;
    let points_px = advantage_width(advantage_text.as_ref(), small);
    // Captured pieces and the points never vanish before the name. The
    // full strip gives them a line of their own; the compact one shares the
    // line with the name, so they are reserved first (at most half of it).
    let piece_room = if full {
        body_px - points_px
    } else {
        (body_px * 0.5).max(0.0)
    };
    let piece_side = piece_size(&pieces, piece_room, if full { PIECE_MAX } else { 14.0 });
    let shared_px = if full || (pieces.is_empty() && advantage_text.is_none()) {
        0.0
    } else {
        pieces_width(&pieces, piece_side) + points_px + GAP
    };
    Plan {
        full,
        name_size,
        small,
        rating,
        clock,
        pieces,
        piece_side,
        advantage,
        advantage_text,
        show_avatar,
        show_dot,
        name_room: body_px - dot_px - title_px - shared_px,
    }
}

fn pieces_width(pieces: &[(Piece, u8)], side: f32) -> f32 {
    piece_count(pieces) as f32 * side + pieces.len().saturating_sub(1) as f32 * (GAP - 2.0)
}

/// The placeholder of an unknown player: a button that opens the header.
fn unknown_name<'a, M: Clone + 'a>(
    ctx: &StripContext<'_>,
    size: f32,
    message: M,
    tip_position: tooltip::Position,
) -> Element<'a, M> {
    let unknown = button(
        text(ctx.i18n.tr("player-name-unknown"))
            .size(size)
            .font(NOTO_SANS_BOLD),
    )
    .padding(0)
    .style(|theme: &Theme, status| {
        let tokens = Tokens::of(theme);
        button::Style {
            background: None,
            text_color: if matches!(status, button::Status::Hovered) {
                tokens.text
            } else {
                tokens.muted
            },
            ..button::Style::default()
        }
    })
    .on_press(message);
    tip(unknown, ctx.i18n.tr("player-unknown"), tip_position)
}

/// Dot, title, name and rating.
fn name_line<'a, M: Clone + 'a>(
    view: &PlayerStripView,
    plan: &Plan,
    ctx: &StripContext<'_>,
    on_unknown_name: M,
    tip_position: tooltip::Position,
) -> Element<'a, M> {
    let mut line = row![].spacing(GAP).align_y(Alignment::Center);
    if plan.show_dot {
        line = line.push(if view.to_move {
            side_dot(view.color)
        } else {
            // Keeps the names of both strips aligned.
            container(iced::widget::Space::new())
                .width(Length::Fixed(DOT))
                .height(Length::Fixed(DOT))
                .into()
        });
    }
    if let Some(title) = &view.title {
        line = line.push(title_chip(title.clone()));
    }
    let Some(name) = &view.name else {
        return line
            .push(unknown_name(
                ctx,
                plan.name_size,
                on_unknown_name,
                tip_position,
            ))
            .into();
    };
    let (shown, rating_shown) = fit_name_and_rating(
        plan.name_room,
        name,
        plan.rating.as_deref(),
        plan.name_size,
        plan.small,
    );
    let label = text(shown)
        .size(plan.name_size)
        .font(NOTO_SANS_BOLD)
        .wrapping(text::Wrapping::None);
    line = line.push(tip(
        label,
        name_tooltip(view, plan.rating.as_deref(), ctx.i18n),
        tip_position,
    ));
    if let Some(rating) = rating_shown {
        line = line.push(
            text(rating)
                .size(plan.small)
                .style(muted_style)
                .wrapping(text::Wrapping::None),
        );
    }
    line.into()
}

/// The captured pieces and the points ahead, when there is something.
fn captured_line<'a, M: 'a>(
    plan: &Plan,
    ctx: &StripContext<'_>,
    tip_position: tooltip::Position,
) -> Option<Element<'a, M>> {
    if plan.pieces.is_empty() && plan.advantage_text.is_none() {
        return None;
    }
    let mut line = row![pieces_row(&plan.pieces, ctx.pieces, plan.piece_side)]
        .spacing(GAP)
        .align_y(Alignment::Center);
    if let (Some(label), Some(points)) = (&plan.advantage_text, plan.advantage) {
        let mut args = FluentArgs::new();
        args.set("points", i64::from(points));
        let label = text(label.clone())
            .size(plan.small)
            .style(muted_style)
            .wrapping(text::Wrapping::None);
        line = line.push(tip(
            label,
            ctx.i18n.tr_args("player-advantage", &args),
            tip_position,
        ));
    }
    Some(container(line).clip(true).into())
}

/// Builds the strip. `on_unknown_name` is sent when the unknown-player
/// placeholder is clicked (it opens the header dialog, D233, D238).
/// `tip_position` is where the tooltips open: away from the board.
pub fn view<'a, M: Clone + 'a>(
    view: &PlayerStripView,
    ctx: &StripContext<'_>,
    on_unknown_name: M,
    tip_position: tooltip::Position,
) -> Element<'a, M> {
    let plan = plan(view, ctx);
    let height = ctx.mode.height();
    let first = name_line(view, &plan, ctx, on_unknown_name, tip_position);
    let captured = captured_line(&plan, ctx, tip_position);

    let mut strip = row![]
        .spacing(PART_GAP)
        .align_y(Alignment::Center)
        .width(Length::Fixed(ctx.width - 2.0 * PAD_X))
        .height(Length::Fixed(height));
    if plan.show_avatar {
        strip = strip.push(avatar(view.color));
    }
    let body: Element<'a, M> = if plan.full {
        let mut lines = iced::widget::column![first].spacing(2);
        if let Some(captured) = captured {
            lines = lines.push(captured);
        }
        container(lines).width(Length::Fill).into()
    } else {
        let mut line = row![first].spacing(GAP).align_y(Alignment::Center);
        if let Some(captured) = captured {
            line = line.push(captured);
        }
        container(line).width(Length::Fill).clip(true).into()
    };
    strip = strip.push(body);
    if let Some(time) = plan.clock {
        strip = strip.push(clock_widget(time, view, ctx, tip_position));
    }
    container(strip)
        .padding(Padding::from([0.0, PAD_X]))
        .width(Length::Fixed(ctx.width))
        .height(Length::Fixed(height))
        .align_y(iced::alignment::Vertical::Center)
        .style(|theme: &Theme| {
            let tokens = Tokens::of(theme);
            container::Style {
                text_color: Some(tokens.text),
                background: Some(tokens.window.into()),
                border: iced::Border {
                    radius: tokens.radius.control.into(),
                    ..iced::Border::default()
                },
                ..container::Style::default()
            }
        })
        .into()
}

#[cfg(test)]
mod tests {
    use mychess_core::{RatingType, Role};

    use super::*;
    use crate::i18n::Language;

    fn strip(color: Side) -> PlayerStripView {
        PlayerStripView::new(color)
    }

    #[test]
    fn clock_formats_minutes_and_hours() {
        for (seconds, text) in [
            (0, "0:00"),
            (9, "0:09"),
            (59, "0:59"),
            (60, "1:00"),
            (754, "12:34"),
            (3599, "59:59"),
            (3600, "1:00:00"),
            (3661, "1:01:01"),
            (36_000, "10:00:00"),
        ] {
            assert_eq!(format_clock(seconds), text, "{seconds}");
        }
    }

    #[test]
    fn ellipsize_cuts_on_characters_and_marks_the_cut() {
        assert_eq!(ellipsize("Kasparov", 8), "Kasparov");
        assert_eq!(ellipsize("Kasparov", 5), "Kasp\u{2026}");
        assert_eq!(ellipsize("Kasparov", 1), "\u{2026}");
        assert_eq!(ellipsize("Kasparov", 0), "");
        // Not bytes: accents and wide characters count once.
        assert_eq!(ellipsize("Jos\u{e9} R. Capablanca", 4), "Jos\u{2026}");
        assert_eq!(ellipsize("", 3), "");
    }

    #[test]
    fn name_is_cut_before_the_rating() {
        let (name, rating) =
            fit_name_and_rating(150.0, "Garry Kimovich Kasparov", Some("2851"), 15.0, 12.0);
        assert!(name.ends_with('\u{2026}'), "{name}");
        assert_eq!(rating.as_deref(), Some("2851"));
        // Everything fits: nothing changes.
        let (name, rating) = fit_name_and_rating(400.0, "Kasparov", Some("2851"), 15.0, 12.0);
        assert_eq!(
            (name.as_str(), rating.as_deref()),
            ("Kasparov", Some("2851"))
        );
    }

    #[test]
    fn rating_is_cut_or_dropped_after_the_name_reached_its_minimum() {
        let (name, rating) = fit_name_and_rating(75.0, "Kasparov", Some("2851 Rapid"), 15.0, 12.0);
        assert_eq!(name.chars().count(), MIN_NAME_CHARS);
        assert!(
            rating.as_deref().is_none_or(|r| r.chars().count() <= 4),
            "{rating:?}"
        );
        // No room for the rating at all.
        let (name, rating) = fit_name_and_rating(45.0, "Kasparov", Some("2851"), 15.0, 12.0);
        assert_eq!(rating, None);
        assert!(name.chars().count() <= 5, "{name}");
    }

    #[test]
    fn zero_or_negative_room_never_panics() {
        for room in [0.0, -10.0, f32::NAN, f32::INFINITY] {
            drop(fit_name_and_rating(
                room,
                "Kasparov",
                Some("2851"),
                15.0,
                12.0,
            ));
            drop(fit_name_and_rating(room, "Kasparov", None, 15.0, 12.0));
        }
    }

    #[test]
    fn pieces_take_the_color_of_the_mode() {
        let mut white = strip(Side::White);
        white.captured = vec![(Role::Queen, 1), (Role::Pawn, 3)];
        let colors = |mode| {
            strip_pieces(&white, mode)
                .iter()
                .map(|(p, n)| (p.color, p.role, *n))
                .collect::<Vec<_>>()
        };
        // Surplus: White's own pieces.
        assert_eq!(
            colors(MaterialMode::Difference),
            vec![(Side::White, Role::Queen, 1), (Side::White, Role::Pawn, 3)]
        );
        // Captured: the Black pieces that White took.
        assert_eq!(
            colors(MaterialMode::Captured),
            vec![(Side::Black, Role::Queen, 1), (Side::Black, Role::Pawn, 3)]
        );
        assert_eq!(colors(MaterialMode::Off), vec![]);
        let mut black = strip(Side::Black);
        black.captured = vec![(Role::Rook, 2)];
        assert_eq!(
            strip_pieces(&black, MaterialMode::Captured)[0].0.color,
            Side::White
        );
        assert_eq!(
            strip_pieces(&black, MaterialMode::Difference)[0].0.color,
            Side::Black
        );
    }

    #[test]
    fn side_tones_follow_the_theme_and_differ() {
        for tokens in [Tokens::dark(), Tokens::light()] {
            let (light, dark) = side_tones(&tokens);
            let lum = |c: Color| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
            assert!(lum(light) > lum(dark));
        }
    }

    #[test]
    fn clock_colors_mark_the_side_to_move_and_the_low_clock() {
        let t = Tokens::dark();
        assert_eq!(clock_colors(&t, true, Some(300)), (t.accent, t.on_accent));
        assert_eq!(clock_colors(&t, false, Some(300)), (t.raised, t.text));
        assert_eq!(clock_colors(&t, false, Some(10)), (t.raised, t.danger));
        assert_eq!(clock_colors(&t, true, Some(10)), (t.danger, t.on_accent));
        assert_eq!(clock_colors(&t, false, Some(11)), (t.raised, t.text));
        assert_eq!(clock_colors(&t, false, None), (t.raised, t.text));
    }

    #[test]
    fn clock_text_follows_the_slot_and_the_preference() {
        let mut view = strip(Side::White);
        assert_eq!(clock_text(&view, false, true, "--:--"), None, "no clocks");
        assert_eq!(
            clock_text(&view, true, true, "--:--").as_deref(),
            Some("--:--"),
            "slot reserved, no value yet"
        );
        assert_eq!(
            clock_text(&view, true, false, "--:--"),
            None,
            "switched off"
        );
        view.clock = Some(125);
        assert_eq!(
            clock_text(&view, true, true, "--:--").as_deref(),
            Some("2:05")
        );
    }

    #[test]
    fn clock_slot_is_wide_enough_for_hours_only_when_needed() {
        let mut a = strip(Side::White);
        let b = strip(Side::Black);
        assert_eq!(clock_chars([&a, &b]), 5);
        a.clock = Some(3600);
        assert_eq!(clock_chars([&a, &b]), 7);
    }

    #[test]
    fn rating_text_adds_the_kind_only_when_it_is_not_elo() {
        let i18n = I18n::new(Language::EnUs);
        let rating = |kind| RatingView { value: 1850, kind };
        assert_eq!(rating_text(rating(RatingType::Elo), &i18n), "1850");
        assert_eq!(rating_text(rating(RatingType::Rating), &i18n), "1850");
        assert_eq!(rating_text(rating(RatingType::Rapid), &i18n), "1850 Rapid");
        assert_eq!(rating_text(rating(RatingType::Uscf), &i18n), "1850 USCF");
        let pt = I18n::new(Language::PtBr);
        assert_eq!(
            rating_text(rating(RatingType::Rapid), &pt),
            "1850 R\u{e1}pido"
        );
    }

    #[test]
    fn tooltip_omits_missing_title_and_rating() {
        let i18n = I18n::new(Language::EnUs);
        let mut view = strip(Side::White);
        assert_eq!(name_tooltip(&view, None, &i18n), "Unknown player");
        view.name = Some("Kasparov".to_owned());
        assert_eq!(name_tooltip(&view, None, &i18n), "Kasparov");
        assert_eq!(name_tooltip(&view, Some("2851"), &i18n), "Kasparov, 2851");
        view.title = Some("GM".to_owned());
        assert_eq!(
            name_tooltip(&view, Some("2851"), &i18n),
            "GM Kasparov, 2851"
        );
        assert_eq!(name_tooltip(&view, None, &i18n), "GM Kasparov");
    }

    #[test]
    fn piece_size_shrinks_to_fit_but_not_below_the_minimum() {
        let pawns = vec![(
            Piece {
                color: Side::Black,
                role: Role::Pawn,
            },
            8,
        )];
        assert_eq!(piece_size(&pawns, 400.0, PIECE_MAX), PIECE_MAX);
        let size = piece_size(&pawns, 100.0, PIECE_MAX);
        assert!((PIECE_MIN..PIECE_MAX).contains(&size), "{size}");
        assert_eq!(piece_size(&pawns, 10.0, PIECE_MAX), PIECE_MIN);
        assert_eq!(piece_size(&[], 10.0, PIECE_MAX), PIECE_MAX);
    }
}
