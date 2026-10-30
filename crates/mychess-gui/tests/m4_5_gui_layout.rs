//! M4-5 (GUI): the evaluation bar and the traffic lights beside the board
//! column (spec D459): level with the eight squares, never outside the
//! panel, and the board gives them room.

#![allow(clippy::float_cmp)]

mod m2_4_common;

use iced::{Settings, Size, widget};
use iced_test::Simulator;
use m2_4_common::render_lock;
use mychess_app::{EvalBarView, PlayerStrips, PlayersPrefs, Rgb, TrafficLight};
use mychess_core::Position;
use mychess_gui::Message;
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::widgets::board::geometry::{
    BoardLayout, CoordsMode, LayoutOptions, Orientation, column_metrics,
};
use mychess_gui::widgets::board::marks::MarkBrush;
use mychess_gui::widgets::board::pieces::PieceSet;
use mychess_gui::widgets::board::{BoardCaches, BoardData, BoardOptions, BoardWidget};
use mychess_gui::widgets::eval_bar::strip_width;
use mychess_gui::windows::board_column::{
    BoardColumn, CANVAS_ID, EVAL_BAR_ID, LIGHTS_ID, SideBars, policy_of,
};

struct Rig {
    position: Position,
    strips: PlayerStrips,
    brush: MarkBrush,
    pieces: PieceSet,
    caches: BoardCaches,
    i18n: I18n,
}

impl Rig {
    fn new() -> Rig {
        Rig {
            position: Position::new(),
            strips: PlayerStrips::default(),
            brush: MarkBrush::default(),
            pieces: PieceSet::default(),
            caches: BoardCaches::default(),
            i18n: I18n::new(Language::EnUs),
        }
    }

    fn element<'a>(
        &'a self,
        size: Size,
        coords: CoordsMode,
        bars: SideBars<'a>,
    ) -> iced::Element<'a, Message> {
        let data = BoardData {
            position: &self.position,
            last_move: None,
            marks: &[],
            orientation: Orientation::White,
            options: BoardOptions {
                coords,
                ..BoardOptions::default()
            },
            accepts_moves: false,
            brush: &self.brush,
            pieces: &self.pieces,
            animation: None,
        };
        BoardColumn {
            board: BoardWidget::new(data, &self.caches),
            strips: &self.strips,
            prefs: PlayersPrefs::default(),
            countdown: true,
            show_side_to_move: true,
            coords,
            orientation: Orientation::White,
            pieces: &self.pieces,
            i18n: &self.i18n,
        }
        .layout_with(size, bars)
    }
}

fn bounds(sim: &mut Simulator<'_, Message>, id: &'static str) -> Option<iced::Rectangle> {
    sim.find(widget::Id::new(id)).ok().map(|t| t.bounds())
}

fn lights(n: u8) -> Vec<TrafficLight> {
    (1..=n)
        .map(|rank| TrafficLight {
            rank,
            color: Rgb {
                r: 0x20 * rank,
                g: 0xf8,
                b: 0,
            },
        })
        .collect()
}

#[test]
fn the_bar_is_level_with_the_eight_squares_and_inside_the_panel() {
    let _guard = render_lock();
    let view = EvalBarView {
        fill: Some(0.7),
        ..EvalBarView::default()
    };
    for (w, h) in [(560.0, 480.0), (900.0, 700.0), (1400.0, 900.0)] {
        for coords in [CoordsMode::Inside, CoordsMode::Outside] {
            let rig = Rig::new();
            let size = Size::new(w, h);
            let bars = SideBars {
                eval: Some(&view),
                lights: &[],
                eval_label: true,
            };
            let mut sim =
                Simulator::with_size(Settings::default(), size, rig.element(size, coords, bars));
            let canvas = bounds(&mut sim, CANVAS_ID).expect("canvas");
            let bar = bounds(&mut sim, EVAL_BAR_ID).expect("bar");
            let layout =
                BoardLayout::compute(canvas.size(), LayoutOptions { coords }).expect("layout");
            assert_eq!(bar.height, 8.0 * layout.s, "{w}x{h} {coords:?}");
            assert_eq!(bar.y, canvas.y + layout.board.y, "{w}x{h} {coords:?}");
            assert!(bar.x >= canvas.x + canvas.width, "{w}x{h} {coords:?}");
            assert!(bar.x + bar.width <= w + 0.5, "{w}x{h} {coords:?}: {bar:?}");
            let free =
                column_metrics(size, coords, policy_of(PlayersPrefs::default())).expect("fits");
            assert!(canvas.width <= free.canvas.width, "the board gives room");
        }
    }
}

#[test]
fn bar_and_lights_sit_side_by_side_and_lights_are_narrow() {
    let _guard = render_lock();
    let view = EvalBarView::default();
    let strips = lights(3);
    let rig = Rig::new();
    let size = Size::new(900.0, 700.0);
    let bars = SideBars {
        eval: Some(&view),
        lights: &strips,
        eval_label: true,
    };
    let mut sim = Simulator::with_size(
        Settings::default(),
        size,
        rig.element(size, CoordsMode::Outside, bars),
    );
    let canvas = bounds(&mut sim, CANVAS_ID).expect("canvas");
    let bar = bounds(&mut sim, EVAL_BAR_ID).expect("bar");
    let light = bounds(&mut sim, LIGHTS_ID).expect("lights");
    assert!(bar.x + bar.width <= light.x, "bar then lights");
    assert_eq!(bar.height, light.height);
    assert_eq!(bar.y, light.y);
    let layout = BoardLayout::compute(
        canvas.size(),
        LayoutOptions {
            coords: CoordsMode::Outside,
        },
    )
    .expect("layout");
    assert_eq!(light.width, strip_width(layout.s));
    assert!(light.x + light.width <= size.width + 0.5);
}

#[test]
fn without_bars_nothing_is_added_and_the_board_keeps_its_size() {
    let _guard = render_lock();
    let rig = Rig::new();
    let size = Size::new(900.0, 700.0);
    let mut sim = Simulator::with_size(
        Settings::default(),
        size,
        rig.element(size, CoordsMode::Outside, SideBars::default()),
    );
    assert!(bounds(&mut sim, EVAL_BAR_ID).is_none());
    assert!(bounds(&mut sim, LIGHTS_ID).is_none());
    let canvas = bounds(&mut sim, CANVAS_ID).expect("canvas");
    let free = column_metrics(
        size,
        CoordsMode::Outside,
        policy_of(PlayersPrefs::default()),
    )
    .expect("fits");
    assert_eq!(canvas.size(), free.canvas);
}
