//! The column of the board: the player strip of the side at the top, the
//! board, and the strip of the side at the bottom (spec M2-5, D230, D237).
//!
//! [`column_metrics`] decides, from the space the panel offers, how big the
//! board is and whether the strips are full, compact or hidden; the strips
//! are exactly as wide as the board and the canvas gets an exact size, so
//! the strips line up with the squares. Which strip sits at the top follows
//! the orientation of the board (Black at the top by default).

use iced::widget::{column, container, responsive, row};
use iced::{Element, Length, Padding, Size};
use mychess_app::TagField;
use mychess_core::Color as Side;

use crate::app_api::{EvalBarView, PlayerStripView, PlayerStrips, PlayersPrefs, TrafficLight};
use crate::i18n::I18n;
use crate::shell::{Message, UiMessage};
use crate::widgets::board::BoardWidget;
use crate::widgets::board::geometry::{
    ColumnMetrics, CoordsMode, Orientation, StripMode, StripPolicy, column_metrics,
};
use crate::widgets::board::pieces::PieceSet;
use crate::widgets::board::premove::PremoveBoardEvent;
use crate::widgets::eval_bar;
use crate::windows::dialog_ui::DialogMessage;
use crate::windows::online_ui::OnlineUiMessage;
use crate::windows::player_strip::{self, StripContext, clock_chars};

/// Widget id of the strip at the top.
pub const TOP_STRIP_ID: &str = "board-strip-top";
/// Widget id of the strip at the bottom.
pub const BOTTOM_STRIP_ID: &str = "board-strip-bottom";
/// Widget id of the evaluation bar.
pub const EVAL_BAR_ID: &str = "board-eval-bar";
/// Widget id of the traffic lights.
pub const LIGHTS_ID: &str = "board-traffic-lights";
/// Widget id of the canvas of the board.
pub const CANVAS_ID: &str = "board-canvas";

/// Everything the column draws.
#[derive(Clone)]
pub struct BoardColumn<'a> {
    /// The board canvas.
    pub board: BoardWidget<'a>,
    /// The content of the two strips.
    pub strips: &'a PlayerStrips,
    /// The player strip preferences.
    pub prefs: PlayersPrefs,
    /// The clocks count down (see `StripContext::countdown`).
    pub countdown: bool,
    /// Whether the side to move gets a dot.
    pub show_side_to_move: bool,
    /// The coordinate mode (it decides the margins of the canvas).
    pub coords: CoordsMode,
    /// Who is at the bottom.
    pub orientation: Orientation,
    /// The selected piece set (for the captured pieces).
    pub pieces: &'a PieceSet,
    /// Texts.
    pub i18n: &'a I18n,
}

/// Space between the board and a bar, and between the two bars.
pub const BAR_GAP: f32 = 4.0;

/// The evaluation bar and the traffic lights beside the board (spec M4-5,
/// D459). `None` and an empty list draw nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct SideBars<'a> {
    /// The evaluation bar, if shown.
    pub eval: Option<&'a EvalBarView>,
    /// Whether the bar reserves room for its label (the preference, not
    /// whether there is a value now: the board must not move, D858).
    pub eval_label: bool,
    /// The strips of the traffic lights; empty when hidden.
    pub lights: &'a [TrafficLight],
}

impl SideBars<'_> {
    /// Whether anything is drawn.
    pub fn any(&self) -> bool {
        self.eval.is_some() || !self.lights.is_empty()
    }

    /// The width they take beside a board of squares of side `s`,
    /// including the gap before each.
    pub fn width(&self, s: f32) -> f32 {
        let mut total = 0.0;
        if self.eval.is_some() {
            total += BAR_GAP + eval_bar::bar_width(s, self.eval_label);
        }
        if !self.lights.is_empty() {
            total += BAR_GAP + eval_bar::strip_width(s);
        }
        total
    }
}

/// The side whose strip is at the bottom.
pub fn bottom_side(orientation: Orientation) -> Side {
    match orientation {
        Orientation::White => Side::White,
        Orientation::Black => Side::Black,
    }
}

/// What the preferences ask of the strips before the space decides.
pub fn policy_of(prefs: PlayersPrefs) -> StripPolicy {
    if !prefs.show_strips {
        StripPolicy::Off
    } else if prefs.compact_strips {
        StripPolicy::PreferCompact
    } else {
        StripPolicy::Auto
    }
}

fn field_of(side: Side) -> TagField {
    match side {
        Side::White => TagField::White,
        Side::Black => TagField::Black,
    }
}

impl<'a> BoardColumn<'a> {
    /// The column as an element filling its panel.
    pub fn into_element(self) -> Element<'a, Message> {
        self.into_element_with(SideBars::default())
    }

    /// The column with the bars beside the board; they shrink the board
    /// with the rest of the column, so they always fit.
    pub fn into_element_with(self, bars: SideBars<'a>) -> Element<'a, Message> {
        responsive(move |size| self.layout_with(size, bars)).into()
    }

    /// The column for a given available size (what the `responsive` calls).
    pub fn layout(&self, size: Size) -> Element<'a, Message> {
        self.layout_with(size, SideBars::default())
    }

    /// [`layout`](Self::layout) with bars beside the board.
    pub fn layout_with(&self, size: Size, bars: SideBars<'a>) -> Element<'a, Message> {
        let policy = policy_of(self.prefs);
        let empty = || {
            container(column![])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };
        let Some(first) = column_metrics(size, self.coords, policy) else {
            return empty();
        };
        // The widths of the bars grow with the board but never beyond what
        // the first (bar-less) measure gives, so one more pass suffices.
        let reserved = bars.width(first.s);
        let Some(metrics) = column_metrics(
            Size::new(size.width - reserved, size.height),
            self.coords,
            policy,
        ) else {
            return empty();
        };
        let extra = bars.width(metrics.s);
        // With a premove setup (online game, opponent's turn) the board
        // publishes its own events too (spec M8-3, D1391).
        let board = if self.board.has_premove() {
            self.board
                .clone()
                .into_premove_element()
                .map(|event| match event {
                    PremoveBoardEvent::Board(event) => Message::Ui(UiMessage::Board(event)),
                    PremoveBoardEvent::Premove(event) => {
                        Message::Online(OnlineUiMessage::Premove(event))
                    }
                })
        } else {
            self.board
                .clone()
                .into_element()
                .map(|event| Message::Ui(UiMessage::Board(event)))
        };
        let canvas = container(board)
            .id(CANVAS_ID)
            .width(Length::Fixed(metrics.canvas.width))
            .height(Length::Fixed(metrics.canvas.height));
        let mut stack = column![]
            .spacing(metrics.gap)
            .width(Length::Fixed(metrics.total.width));
        if metrics.mode != StripMode::Hidden {
            let bottom = bottom_side(self.orientation);
            let top = match bottom {
                Side::White => Side::Black,
                Side::Black => Side::White,
            };
            stack = stack.push(self.strip(
                top,
                &metrics,
                TOP_STRIP_ID,
                iced::widget::tooltip::Position::Top,
            ));
        }
        stack = stack.push(canvas);
        if metrics.mode != StripMode::Hidden {
            let bottom = bottom_side(self.orientation);
            stack = stack.push(self.strip(
                bottom,
                &metrics,
                BOTTOM_STRIP_ID,
                iced::widget::tooltip::Position::Bottom,
            ));
        }
        let mut page = row![stack].spacing(BAR_GAP);
        if bars.any() {
            page = page.push(Self::side_bars(bars, &metrics));
        }
        let left = ((size.width - metrics.total.width - extra) / 2.0)
            .round()
            .max(0.0);
        container(page)
            .padding(Padding::ZERO.left(left).top(metrics.origin.y))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// The bars, as tall as the eight squares and level with them.
    fn side_bars(bars: SideBars<'a>, metrics: &ColumnMetrics) -> Element<'a, Message> {
        let height = 8.0 * metrics.s;
        let top = if metrics.mode == StripMode::Hidden {
            metrics.strip_inset
        } else {
            metrics.strip_height + metrics.gap + metrics.strip_inset
        };
        let mut strips = row![].spacing(BAR_GAP);
        if let Some(view) = bars.eval {
            strips = strips.push(
                container(eval_bar::eval_bar(
                    view,
                    eval_bar::bar_width(metrics.s, bars.eval_label),
                    height,
                ))
                .id(EVAL_BAR_ID),
            );
        }
        if !bars.lights.is_empty() {
            strips = strips.push(
                container(eval_bar::traffic_lights(
                    bars.lights,
                    eval_bar::strip_width(metrics.s),
                    height,
                ))
                .id(LIGHTS_ID),
            );
        }
        container(strips).padding(Padding::ZERO.top(top)).into()
    }

    fn view_of(&self, side: Side) -> &'a PlayerStripView {
        self.strips.of(side)
    }

    fn strip(
        &self,
        side: Side,
        metrics: &ColumnMetrics,
        id: &'static str,
        tip: iced::widget::tooltip::Position,
    ) -> Element<'a, Message> {
        let ctx = StripContext {
            mode: metrics.mode,
            width: metrics.strip_width,
            prefs: self.prefs,
            show_side_to_move: self.show_side_to_move,
            clock_slot: self.strips.clock_slot,
            countdown: self.countdown,
            clock_chars: clock_chars([&self.strips.white, &self.strips.black]),
            pieces: self.pieces,
            i18n: self.i18n,
        };
        let strip = player_strip::view(
            self.view_of(side),
            &ctx,
            Message::Dialog(DialogMessage::OpenHeader(Some(field_of(side)))),
            tip,
        );
        // The strip starts at the first square, not at the coordinates.
        container(
            container(strip)
                .id(id)
                .width(Length::Fixed(metrics.strip_width)),
        )
        .padding(Padding::ZERO.left(metrics.strip_inset))
        .width(Length::Fixed(metrics.total.width))
        .into()
    }
}
