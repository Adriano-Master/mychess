//! The piece tracker panel (spec M6-4d, D3438): a heat map of the
//! squares a piece goes to over the games of the filter (White at the
//! bottom, never turned), the 32 pieces of the starting position as
//! selectors (left button: that piece; right button: also its sisters;
//! double click: also update), the controls and the list of the 64 squares.
// The map is at most a few thousand logical pixels wide.
#![allow(clippy::cast_precision_loss)]

use fluent_bundle::FluentArgs;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::canvas::{self, Action, Frame, Path, Stroke};
use iced::widget::{Id, column, container, mouse_area, row, scrollable, svg, text, text_input};
use iced::{Alignment, Element, Length, Point, Rectangle, Size, mouse};
use mychess_app::Message as AppMessage;
use mychess_app::piece_tracker::{
    PieceTrackerMessage, PieceTrackerView, TrackerCell, TrackerMode, TrackerScheme, initial_piece,
};
use mychess_core::{File, Rank, Square};

use super::data_panels::{
    DataPanelsMessage, DataPanelsUi, TRACKER_END_ID, TRACKER_START_ID, TrackerField, digits,
};
use super::panel_kit::{self, BODY_TEXT, action, bold, cell, choice, danger, flow, muted};
use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme::chart_colors::{heat_edge, heat_fill};
use crate::theme::{self, Tokens};
use crate::widgets::board::pieces::PieceSet;
use crate::widgets::chart::{StaticLayer, label};
use crate::windows::panes;

/// Id of the body of the panel (for tests and automation).
pub const TRACKER_PANE_ID: &str = "piece-tracker-pane";

/// Room for the coordinates around the map.
const COORD_MARGIN: f32 = 16.0;
/// Below this side the count is not written inside a square.
const MIN_TEXT_SQUARE: f32 = 34.0;
/// Side of a drawing of a piece in the selector.
const PIECE_SIDE: f32 = 30.0;

fn tracker(message: PieceTrackerMessage) -> Message {
    Message::App(AppMessage::PieceTracker(message))
}

fn ui(message: DataPanelsMessage) -> Message {
    Message::Panels(message)
}

/// The text of a color scheme.
pub fn scheme_key(scheme: TrackerScheme) -> &'static str {
    match scheme {
        TrackerScheme::Black => "ptracker-color-black",
        TrackerScheme::Red => "ptracker-color-red",
        TrackerScheme::Yellow => "ptracker-color-yellow",
        TrackerScheme::Cyan => "ptracker-color-cyan",
        TrackerScheme::Blue => "ptracker-color-blue",
        TrackerScheme::XBlack => "ptracker-color-xblack",
        TrackerScheme::XRed => "ptracker-color-xred",
        TrackerScheme::XYellow => "ptracker-color-xyellow",
        TrackerScheme::XCyan => "ptracker-color-xcyan",
        TrackerScheme::XBlue => "ptracker-color-xblue",
    }
}

/// The geometry of the map inside a canvas of `size`: the origin and the
/// side of a square. `None` when there is no room.
fn map_geometry(size: Size) -> Option<(Point, f32)> {
    let side = (size.width - COORD_MARGIN).min(size.height - COORD_MARGIN);
    if !size.width.is_finite() || !size.height.is_finite() || side < 80.0 {
        return None;
    }
    let cell = side / 8.0;
    let origin = Point::new(
        COORD_MARGIN + (size.width - COORD_MARGIN - side) / 2.0,
        ((size.height - COORD_MARGIN - side) / 2.0).max(0.0),
    );
    Some((origin, cell))
}

/// The square under `at` (a point of the canvas).
fn square_at(size: Size, at: Point) -> Option<Square> {
    let (origin, cell) = map_geometry(size)?;
    let fx = (at.x - origin.x) / cell;
    let fy = (at.y - origin.y) / cell;
    if !(0.0..8.0).contains(&fx) || !(0.0..8.0).contains(&fy) {
        return None;
    }
    // Both are in 0..8 here.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (file, row_from_top) = (fx as u32, fy as u32);
    Some(Square::new((7 - row_from_top) * 8 + file))
}

/// What the cached map depends on.
#[derive(PartialEq)]
struct HeatKey {
    cells: Vec<TrackerCell>,
    selected: Vec<Square>,
    stale: bool,
    /// Accent, muted text and the edge of the squares.
    colors: [iced::Color; 3],
    origin: Point,
    side: f32,
}

/// State of the canvas: the square last published as hovered and the
/// cached map.
#[derive(Default)]
pub struct HeatState {
    hovered: Option<Square>,
    layer: StaticLayer<HeatKey>,
}

impl HeatState {
    /// How many times the map was painted (for tests).
    pub fn redraws(&self) -> u32 {
        self.layer.redraws()
    }
}

/// The canvas of the heat map.
pub struct HeatMap {
    cells: Vec<TrackerCell>,
    selected: Vec<Square>,
    stale: bool,
}

impl HeatMap {
    /// The map of `cells` with `selected` squares ringed; `stale` fades it.
    pub fn new(cells: Vec<TrackerCell>, selected: Vec<Square>, stale: bool) -> Self {
        Self {
            cells,
            selected,
            stale,
        }
    }
}

impl HeatMap {
    /// The squares, the chosen starting squares and the coordinates.
    fn paint_static(&self, frame: &mut Frame, t: &Tokens, origin: Point, side: f32) {
        let alpha = if self.stale { 0.6 } else { 1.0 };
        for cell in &self.cells {
            let index = cell.square.to_usize();
            let (file, rank) = (index % 8, index / 8);
            let top_left = Point::new(
                origin.x + file as f32 * side,
                origin.y + (7 - rank) as f32 * side,
            );
            let fill = heat_fill(cell.rgb, alpha);
            frame.fill_rectangle(top_left, Size::new(side, side), fill);
            frame.stroke(
                &Path::rectangle(top_left, Size::new(side, side)),
                Stroke::default().with_width(1.0).with_color(heat_edge(t)),
            );
            if side >= MIN_TEXT_SQUARE && cell.count > 0 {
                label(
                    frame,
                    cell.count.to_string(),
                    Point::new(top_left.x + side / 2.0, top_left.y + side / 2.0),
                    (Horizontal::Center, Vertical::Center),
                    theme::contrast_on(heat_fill(cell.rgb, 1.0)),
                );
            }
        }
        for square in &self.selected {
            let index = square.to_usize();
            let top_left = Point::new(
                origin.x + (index % 8) as f32 * side,
                origin.y + (7 - index / 8) as f32 * side,
            );
            frame.stroke(
                &Path::rectangle(
                    Point::new(top_left.x + 2.0, top_left.y + 2.0),
                    Size::new(side - 4.0, side - 4.0),
                ),
                Stroke::default().with_width(2.5).with_color(t.accent),
            );
        }
        for i in 0..8u8 {
            let file = char::from(b'a' + i);
            label(
                frame,
                file.to_string(),
                Point::new(
                    origin.x + (f32::from(i) + 0.5) * side,
                    origin.y + 8.0 * side + 2.0,
                ),
                (Horizontal::Center, Vertical::Top),
                t.muted,
            );
            label(
                frame,
                (i + 1).to_string(),
                Point::new(origin.x - 4.0, origin.y + (7.0 - f32::from(i) + 0.5) * side),
                (Horizontal::Right, Vertical::Center),
                t.muted,
            );
        }
    }
}

impl canvas::Program<Message> for HeatMap {
    /// The square last published as hovered.
    type State = HeatState;

    fn update(
        &self,
        state: &mut HeatState,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        match event {
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) => {
                let now = cursor
                    .position_in(bounds)
                    .and_then(|p| square_at(bounds.size(), p));
                if now == state.hovered {
                    return None;
                }
                state.hovered = now;
                Some(Action::publish(ui(DataPanelsMessage::TrackerHover(now))))
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &HeatState,
        renderer: &iced::Renderer,
        theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let Some((origin, side)) = map_geometry(bounds.size()) else {
            return Vec::new();
        };
        let t = Tokens::of(theme);
        let key = HeatKey {
            cells: self.cells.clone(),
            selected: self.selected.clone(),
            stale: self.stale,
            colors: [t.accent, t.muted, heat_edge(&t)],
            origin,
            side,
        };
        // The map is cached; only the square under the pointer is drawn per
        // frame.
        let layer = state.layer.geometry(renderer, bounds.size(), key, |frame| {
            self.paint_static(frame, &t, origin, side);
        });
        let mut frame = Frame::new(renderer, bounds.size());
        if let Some(square) = &state.hovered {
            let index = square.to_usize();
            let top_left = Point::new(
                origin.x + (index % 8) as f32 * side,
                origin.y + (7 - index / 8) as f32 * side,
            );
            frame.stroke(
                &Path::rectangle(top_left, Size::new(side, side)),
                Stroke::default().with_width(2.0).with_color(t.text),
            );
        }
        vec![layer, frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &HeatState,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.hovered.is_some() {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

/// The 32 drawings of the starting position, four rows of eight.
fn selector<'a>(view: &PieceTrackerView, set: &PieceSet, i18n: &I18n) -> Element<'a, Message> {
    let mut grid = column![].spacing(2);
    for rank in [Rank::Eighth, Rank::Seventh, Rank::Second, Rank::First] {
        let mut line = row![].spacing(2);
        for file in File::ALL {
            let square = Square::from_coords(file, rank);
            let chosen = view.selected.contains(&square);
            let picture: Element<'a, Message> =
                match initial_piece(square).and_then(|piece| set.handle(piece)) {
                    Some(handle) => svg(handle.clone())
                        .width(Length::Fixed(PIECE_SIDE))
                        .height(Length::Fixed(PIECE_SIDE))
                        .into(),
                    None => text(square.to_string()).size(BODY_TEXT).into(),
                };
            let tile = container(picture)
                .padding(2)
                .style(move |theme: &iced::Theme| {
                    let t = Tokens::of(theme);
                    container::Style {
                        background: Some(
                            if chosen {
                                theme::tokens::mix(t.accent, t.panel, 0.35)
                            } else {
                                t.raised
                            }
                            .into(),
                        ),
                        border: iced::Border {
                            radius: t.radius.chip.into(),
                            width: if chosen { 2.0 } else { 0.0 },
                            color: t.accent,
                        },
                        ..container::Style::default()
                    }
                })
                .id(format!("tracker-piece-{square}"));
            line = line.push(
                mouse_area(tile)
                    .on_press(tracker(PieceTrackerMessage::SelectPiece {
                        square,
                        sister: false,
                    }))
                    .on_right_press(tracker(PieceTrackerMessage::SelectPiece {
                        square,
                        sister: true,
                    }))
                    .on_double_click(tracker(PieceTrackerMessage::Refresh)),
            );
        }
        grid = grid.push(line);
    }
    column![muted(i18n.tr("ptracker-pick-hint")), grid]
        .spacing(4)
        .into()
}

fn hover_line<'a>(
    view: &PieceTrackerView,
    state: &DataPanelsUi,
    i18n: &I18n,
) -> Element<'a, Message> {
    let Some(square) = state.tracker_hover else {
        return muted(String::new());
    };
    let Some(cell) = view.cells.get(square.to_usize()) else {
        return muted(String::new());
    };
    let position = view
        .ranking
        .iter()
        .find(|r| r.square == square)
        .map_or(0, |r| r.position);
    let mut args = FluentArgs::new();
    args.set("square", square.to_string());
    args.set("count", i64::from(cell.count));
    args.set("percent", i18n.decimal(&format!("{:.2}", cell.percent)));
    args.set("position", i64::try_from(position).unwrap_or(0));
    muted(i18n.tr_args("ptracker-hover", &args))
}

fn move_field<'a>(
    view: &PieceTrackerView,
    state: &DataPanelsUi,
    field: TrackerField,
) -> Element<'a, Message> {
    let (index, value) = match field {
        TrackerField::Start => (0, view.start_move),
        TrackerField::End => (1, view.end_move),
    };
    let shown = state.tracker_text[index]
        .clone()
        .unwrap_or_else(|| value.to_string());
    text_input("", &shown)
        .id(Id::new(match field {
            TrackerField::Start => TRACKER_START_ID,
            TrackerField::End => TRACKER_END_ID,
        }))
        .on_input(move |v| ui(DataPanelsMessage::TrackerEdit(field, v)))
        .on_submit(tracker(PieceTrackerMessage::Refresh))
        .size(BODY_TEXT)
        .padding([2, 6])
        .width(Length::Fixed(56.0))
        .into()
}

/// The new text and, when it is a number, the value to send for an edit of
/// the first or last move (three digits, 0 to 999).
pub fn edit_move(text: &str) -> (String, Option<u16>) {
    let text = digits(text, 3);
    let value = text.parse::<u16>().ok();
    (text, value)
}

fn controls<'a>(
    view: &PieceTrackerView,
    state: &DataPanelsUi,
    i18n: &I18n,
) -> Element<'a, Message> {
    let mut modes = row![muted(i18n.tr("ptracker-stat"))]
        .spacing(4)
        .align_y(Alignment::Center);
    for (mode, key) in [
        (TrackerMode::Games, "ptracker-mode-games"),
        (TrackerMode::Time, "ptracker-mode-time"),
    ] {
        modes = modes.push(choice(
            i18n.tr(key),
            view.mode == mode,
            Some(tracker(PieceTrackerMessage::SetMode(mode))),
        ));
    }
    let moves = row![
        muted(i18n.tr("ptracker-moves")),
        move_field(view, state, TrackerField::Start),
        muted(i18n.tr("ptracker-moves-to")),
        move_field(view, state, TrackerField::End),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let mut colors = row![muted(i18n.tr("ptracker-color"))]
        .spacing(4)
        .align_y(Alignment::Center);
    for scheme in TrackerScheme::ALL {
        colors = colors.push(choice(
            i18n.tr(scheme_key(*scheme)),
            view.scheme == *scheme,
            Some(tracker(PieceTrackerMessage::SetScheme(*scheme))),
        ));
    }
    let mut buttons = row![action(
        i18n.tr("ptracker-update"),
        Some(tracker(PieceTrackerMessage::Refresh))
    )]
    .spacing(6);
    if view.progress.is_some() {
        buttons = buttons.push(action(
            i18n.tr("ptracker-stop"),
            Some(tracker(PieceTrackerMessage::Stop)),
        ));
    }
    let mut page = column![flow(modes), moves, flow(colors), buttons].spacing(8);
    if let Some((done, total)) = view.progress {
        page = page.push(panel_kit::progress(done, total));
    }
    page.into()
}

fn status<'a>(view: &PieceTrackerView, i18n: &I18n) -> Element<'a, Message> {
    let mut page = column![].spacing(2);
    if view.unavailable {
        return muted(i18n.tr("ptracker-no-base"));
    }
    if let Some(issue) = view.issue {
        page = page.push(danger(i18n.tr(issue.message_key())));
    }
    if view.cells.is_empty() {
        return page.push(muted(i18n.tr("ptracker-nothing-yet"))).into();
    }
    if view.games == 0 {
        page = page.push(muted(i18n.tr("ptracker-empty-filter")));
    }
    let mut args = FluentArgs::new();
    args.set("used", i64::from(view.games_used));
    args.set("games", i64::from(view.games));
    page = page.push(muted(i18n.tr_args("ptracker-counted", &args)));
    if view.games_skipped > 0 {
        let mut args = FluentArgs::new();
        args.set("n", i64::from(view.games_skipped));
        page = page.push(muted(i18n.tr_args("ptracker-skipped", &args)));
    }
    if view.interrupted {
        page = page.push(danger(i18n.tr("ptracker-interrupted")));
    }
    page.into()
}

fn ranking<'a>(view: &PieceTrackerView) -> Element<'a, Message> {
    let mut rows = column![].spacing(1);
    for (i, r) in view.ranking.iter().enumerate() {
        let line = row![
            cell(format!("{}.", r.position), 34.0, Horizontal::Right),
            cell(r.name.clone(), 34.0, Horizontal::Left),
            cell(r.count.to_string(), 70.0, Horizontal::Right),
            cell(format!("{:.2}%", r.percent), 70.0, Horizontal::Right),
        ]
        .spacing(6);
        rows = rows.push(
            container(line)
                .padding([1, 6])
                .style(theme::list_row(theme::RowLook {
                    zebra: i % 2 == 1,
                    selected: false,
                    current: false,
                    dimmed: view.stale,
                })),
        );
    }
    scrollable(rows).height(Length::Fill).into()
}

/// The content of the panel for `view`.
pub fn content<'a>(
    view: &PieceTrackerView,
    state: &DataPanelsUi,
    set: &PieceSet,
    i18n: &I18n,
) -> Element<'a, Message> {
    let map = canvas::Canvas::new(HeatMap::new(
        view.cells.clone(),
        view.selected.clone(),
        view.stale,
    ))
    .width(Length::Fill)
    .height(Length::Fill);
    let left = column![
        map,
        hover_line(view, state, i18n),
        selector(view, set, i18n)
    ]
    .spacing(6)
    .width(Length::FillPortion(3));
    let right = column![
        controls(view, state, i18n),
        status(view, i18n),
        text(i18n.tr("ptracker-ranking"))
            .size(BODY_TEXT)
            .font(bold()),
        ranking(view),
    ]
    .spacing(8)
    .width(Length::FillPortion(2));
    row![left, right].spacing(12).padding(8).into()
}

/// The panel.
pub fn view(shell: &Shell) -> Element<'_, Message> {
    let view = shell.app.piece_tracker_view();
    panes::body(
        content(
            &view,
            &shell.ui.dialogs.panels,
            &shell.ui.dialogs.pieces,
            &shell.i18n,
        ),
        TRACKER_PANE_ID,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_map_keeps_white_at_the_bottom() {
        let size = Size::new(416.0, 416.0);
        let (origin, cell) = map_geometry(size).unwrap();
        // The middle of a1 is bottom-left, the middle of h8 top-right.
        let a1 = Point::new(origin.x + cell / 2.0, origin.y + 7.5 * cell);
        let h8 = Point::new(origin.x + 7.5 * cell, origin.y + cell / 2.0);
        assert_eq!(square_at(size, a1), Some(Square::A1));
        assert_eq!(square_at(size, h8), Some(Square::H8));
        assert_eq!(square_at(size, Point::new(1.0, 1.0)), None);
    }

    #[test]
    fn a_tiny_canvas_has_no_map() {
        assert!(map_geometry(Size::new(60.0, 60.0)).is_none());
        assert!(map_geometry(Size::new(f32::NAN, 300.0)).is_none());
    }

    #[test]
    fn move_fields_take_three_digits() {
        assert_eq!(edit_move("12a"), ("12".to_owned(), Some(12)));
        assert_eq!(edit_move("12345"), ("123".to_owned(), Some(123)));
        assert_eq!(edit_move(""), (String::new(), None));
    }
}
