//! The previews of the appearance window (spec M9-3, D1702, D1703).
//!
//! Every preview is made of the real resources: the interface miniature is
//! drawn by `themer` with the `Theme` of the draft; the boards are the
//! real `BoardWidget`; the pieces are the SVG files of the sets; the strips
//! are the real player strip. Only the cards use a small canvas of sixteen
//! squares (cheap: the window shows many of them at once).

use iced::widget::{canvas, column, container, row, svg, text, themer, tooltip};
use iced::{Alignment, Border, Color, Element, Length, Point, Rectangle, Size, Theme, mouse};
use mychess_core::{Color as Side, Piece, RatingType, Role};

use crate::app_api::{PlayerStripView, RatingView};
use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme::board_theme::BoardTheme;
use crate::theme::notation_colors::NotationPalette;
use crate::theme::{self, RowLook, Tokens};
use crate::widgets::board::geometry::StripMode;
use crate::widgets::board::pieces::PieceSet;
use crate::windows::appearance::{AppearanceDraft, AppearanceMessage};
use crate::windows::dialog_ui::DialogMessage;
use crate::windows::player_strip::{self, StripContext};

/// The message of a preview that is not interactive.
pub fn noop() -> Message {
    Message::Dialog(DialogMessage::Appearance(AppearanceMessage::Noop))
}

/// Squares per side of a card.
const CARD_SQUARES: u8 = 4;

/// The sixteen squares of a card: the light color as the ground, the dark
/// squares over it.
struct MiniBoard {
    light: Color,
    dark: Color,
}

impl<M> canvas::Program<M> for MiniBoard {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let side = bounds.width.min(bounds.height) / f32::from(CARD_SQUARES);
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), self.light);
        for rank in 0..CARD_SQUARES {
            for file in 0..CARD_SQUARES {
                if (rank + file) % 2 == 1 {
                    frame.fill_rectangle(
                        Point::new(f32::from(file) * side, f32::from(rank) * side),
                        Size::new(side, side),
                        self.dark,
                    );
                }
            }
        }
        vec![frame.into_geometry()]
    }
}

/// A 4x4 board with the two colors of `theme`, `side` pixels wide.
pub fn mini_board<'a>(theme: &BoardTheme, side: f32) -> Element<'a, Message> {
    canvas(MiniBoard {
        light: theme.light,
        dark: theme.dark,
    })
    .width(Length::Fixed(side))
    .height(Length::Fixed(side))
    .into()
}

/// The pieces of the first and second row of the big preview.
const ROLES: [Role; 6] = [
    Role::King,
    Role::Queen,
    Role::Rook,
    Role::Bishop,
    Role::Knight,
    Role::Pawn,
];

fn piece_image<'a>(set: &PieceSet, piece: Piece, size: f32) -> Element<'a, Message> {
    match set.handle(piece) {
        Some(handle) => svg(handle.clone())
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .into(),
        None => container(text(""))
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .into(),
    }
}

/// The twelve pieces of `set`, white above and black below, on the squares
/// of `board` (`cell` pixels each).
pub fn piece_grid<'a>(set: &PieceSet, board: &BoardTheme, cell: f32) -> Element<'a, Message> {
    let mut rows = column![];
    for (line, color) in [Side::White, Side::Black].into_iter().enumerate() {
        let mut cells = row![];
        for (column_index, role) in ROLES.into_iter().enumerate() {
            let ground = if (line + column_index) % 2 == 0 {
                board.light
            } else {
                board.dark
            };
            cells = cells.push(
                container(piece_image(set, Piece { color, role }, cell)).style(move |_: &Theme| {
                    container::Style {
                        background: Some(ground.into()),
                        ..container::Style::default()
                    }
                }),
            );
        }
        rows = rows.push(cells);
    }
    rows.into()
}

/// Six pieces of `set` in a row (the content of a card of the piece sets).
pub fn piece_sample<'a>(set: &PieceSet, size: f32) -> Element<'a, Message> {
    let sample = [
        (Side::White, Role::King),
        (Side::Black, Role::Queen),
        (Side::White, Role::Rook),
        (Side::Black, Role::Bishop),
        (Side::White, Role::Knight),
        (Side::Black, Role::Pawn),
    ];
    let mut cells = row![].align_y(Alignment::Center);
    for (color, role) in sample {
        cells = cells.push(piece_image(set, Piece { color, role }, size));
    }
    cells.into()
}

fn pill(theme: &Theme) -> container::Style {
    let t = Tokens::of(theme);
    let palette = NotationPalette::from_tokens(&t);
    container::Style {
        text_color: Some(palette.on_pill),
        background: Some(palette.pill.into()),
        border: Border {
            radius: t.radius.pill.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// A miniature of the window of the application drawn with the theme of
/// the draft: tab bar, a mini board, rows of a list (one selected), the
/// pill of the current move and a primary button.
pub fn theme_miniature<'a>(
    draft: &AppearanceDraft,
    system: iced::theme::Mode,
    board: &BoardTheme,
    i18n: &I18n,
    height: f32,
) -> Element<'a, Message> {
    let preview_theme = theme::resolve(draft.mode, system, draft.accent);
    let tabs = container(
        row![
            container(text(i18n.tr("pane-info")).size(12).style(theme::muted_text))
                .padding([4, 10]),
            column![
                container(text(i18n.tr("pane-notation")).size(12)).padding([4, 10]),
                container(text(""))
                    .width(Length::Fill)
                    .height(2)
                    .style(theme::tab_underline(true)),
            ],
        ]
        .spacing(4),
    )
    .width(Length::Fill)
    .style(theme::pane_title);
    let board_side = (height - 70.0).clamp(40.0, 150.0);
    let line = |label: &'a str, look: RowLook| {
        container(text(label).size(13))
            .padding([3, 8])
            .width(Length::Fill)
            .style(theme::list_row(look))
    };
    let list = column![
        line("1. e4  e5", RowLook::default()),
        line(
            "2. Nf3  Nc6",
            RowLook {
                selected: true,
                ..RowLook::default()
            }
        ),
        container(
            row![
                text("3.").size(13),
                container(text("Bb5").size(13)).padding([1, 6]).style(pill),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
        )
        .padding([3, 8]),
        container(text(i18n.tr("dialog-ok")).size(13))
            .padding([4, 16])
            .style(theme::accent_fill),
    ]
    .spacing(4)
    .width(Length::Fill);
    let body = container(
        row![mini_board(board, board_side), list]
            .spacing(10)
            .align_y(Alignment::Start),
    )
    .padding(10)
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::pane_body);
    let window = column![tabs, container(body).padding(6).height(Length::Fill)]
        .width(Length::Fill)
        .height(Length::Fill);
    themer(Some(preview_theme), window)
        .text_color(|t: &Theme| Tokens::of(t).text)
        .background(|t: &Theme| Tokens::of(t).window.into())
        .into()
}

/// A tooltip around `content`.
pub fn tip(content: Element<'_, Message>, label: String) -> Element<'_, Message> {
    tooltip(
        content,
        container(text(label).size(12))
            .padding([3, 8])
            .style(theme::tooltip_box),
        tooltip::Position::Top,
    )
    .into()
}

/// One strip of the sample: a made-up player with a few captured pieces.
fn sample_view(color: Side, i18n: &I18n) -> PlayerStripView {
    let mut view = PlayerStripView::new(color);
    let (name, rating, clock) = match color {
        Side::White => (i18n.tr("appearance-sample-white"), 2150, 323),
        Side::Black => (i18n.tr("appearance-sample-black"), 2080, 297),
    };
    view.name = Some(name);
    view.rating = Some(RatingView {
        value: rating,
        kind: RatingType::Elo,
    });
    view.clock = Some(clock);
    view.captured = vec![(Role::Knight, 1), (Role::Pawn, 2)];
    view.advantage = (color == Side::White).then_some(3);
    view.to_move = color == Side::White;
    view
}

/// The player strips as the board column draws them, for the draft: both
/// players, with the draft's switches, material and piece set. `width` is
/// the width of a strip.
pub fn strips_sample<'a>(
    draft: &AppearanceDraft,
    set: &PieceSet,
    i18n: &I18n,
    width: f32,
) -> Element<'a, Message> {
    if !draft.players.show_strips {
        return text(i18n.tr("appearance-strips-hidden"))
            .size(13)
            .style(theme::muted_text)
            .into();
    }
    let mode = if draft.players.compact_strips {
        StripMode::Compact
    } else {
        StripMode::Full
    };
    let views = [
        sample_view(Side::Black, i18n),
        sample_view(Side::White, i18n),
    ];
    let ctx = StripContext {
        mode,
        width,
        prefs: draft.players,
        show_side_to_move: true,
        clock_slot: true,
        countdown: true,
        clock_chars: 5,
        pieces: set,
        i18n,
    };
    let mut strips = column![].spacing(6);
    for view in &views {
        strips = strips.push(player_strip::view(
            view,
            &ctx,
            noop(),
            tooltip::Position::Bottom,
        ));
    }
    strips.into()
}
