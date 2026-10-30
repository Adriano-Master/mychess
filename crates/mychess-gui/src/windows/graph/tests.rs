//! Tests of the graph panel: the pixel geometry, the canvas events and the
//! panel built from a real game (spec M4-7).

use iced::widget::canvas::Program;
use iced::{Event, Point, Rectangle, Settings, Size, mouse};
use iced_test::Simulator;
use mychess_app::graphs::{GraphKind, GraphMessage, GraphPrefs, GraphStyle, score_graph_view};
use mychess_app::{GameMessage, Message as AppMessage, Nav};
use mychess_core::{Game, Position};

use super::*;
use crate::shell::{Shell, UiMessage};
use crate::windows::layout::PaneKind;

/// A game of `n` plies where every move has `[%eval ply/10]` and a clock.
fn annotated_game(n: usize) -> Game {
    let mut game = Game::new();
    game.tags_mut().white = "Anna".to_owned();
    game.tags_mut().black = "Boris".to_owned();
    let mut pos = Position::new();
    let mut node = game.root();
    for i in 0..n {
        let uci = ["g1f3", "g8f6", "f3g1", "f6g8"][i % 4];
        let mv = pos.parse_uci(uci).expect("legal");
        pos.play(mv).expect("play");
        node = game.add_move(node, mv).expect("add");
        let eval = (i as f32 - 3.0) / 2.0;
        game.set_comment(node, &format!("[%eval {eval:.2}] [%clk 0:05:00]"))
            .expect("comment");
    }
    game
}

fn open_shell(game: Game) -> Shell {
    let mut shell = Shell::new(None);
    // The texts are looked up by their English wording, whatever the
    // locale of the machine.
    shell.i18n = I18n::new(crate::i18n::Language::EnUs);
    shell.app.session.replace_game(game);
    drop(shell.update(Message::Ui(UiMessage::TogglePane(PaneKind::Graph))));
    shell
}

#[test]
fn the_plot_needs_room() {
    assert!(plot_rect(Size::new(10.0, 10.0)).is_none());
    assert!(plot_rect(Size::new(f32::NAN, 200.0)).is_none());
    let plot = plot_rect(Size::new(400.0, 200.0)).expect("plot");
    assert_eq!(plot.x, MARGIN_LEFT);
    assert_eq!(plot.width, 400.0 - MARGIN_LEFT - MARGIN_RIGHT);
    assert_eq!(plot.height, 200.0 - MARGIN_TOP - MARGIN_BOTTOM);
}

#[test]
fn pixels_and_graph_units_round_trip() {
    let game = annotated_game(12);
    let view = score_graph_view(&game, GraphPrefs::default());
    let plot = plot_rect(Size::new(500.0, 240.0)).expect("plot");
    for x in [0.0, 1.0, 2.5, view.x_axis.max] {
        let back = x_at_px(&view.x_axis, px_x(&view.x_axis, x, plot), plot);
        assert!((back - x).abs() < 1e-3, "{x} -> {back}");
    }
    // Up is positive: the top of the axis is the top of the plot.
    assert!((px_y(&view.y_axis, view.y_axis.max, plot) - plot.y).abs() < 1e-3);
    assert!((px_y(&view.y_axis, view.y_axis.min, plot) - (plot.y + plot.height)).abs() < 1e-3);
}

#[test]
fn bars_grow_from_the_zero_line_and_stay_in_the_plot() {
    let game = annotated_game(12);
    let view = score_graph_view(&game, GraphPrefs::default());
    let plot = plot_rect(Size::new(500.0, 240.0)).expect("plot");
    let zero = px_y(&view.y_axis, 0.0, plot);
    let (up, up_size) = bar_rect(&view.x_axis, &view.y_axis, (1.0, 2.0), plot).expect("up");
    assert!((up.y + up_size.height - zero).abs() < 1e-3);
    let (down, _) = bar_rect(&view.x_axis, &view.y_axis, (1.0, -2.0), plot).expect("down");
    assert!((down.y - zero).abs() < 1e-3);
    // A value beyond the axis is cut at the plot edge.
    let (top, size) = bar_rect(&view.x_axis, &view.y_axis, (1.0, 1e9), plot).expect("huge");
    assert!((top.y - plot.y).abs() < 1e-3 && top.y + size.height <= plot.y + plot.height);
    assert!(bar_rect(&view.x_axis, &view.y_axis, (1.0, f32::NAN), plot).is_none());
    assert!(bar_rect(&view.x_axis, &view.y_axis, (1.0, 0.0), plot).is_none());
    assert!(bar_rect(&view.x_axis, &view.y_axis, (1e6, 1.0), plot).is_none());
}

#[test]
fn move_labels_and_bar_lookup() {
    assert_eq!(move_label(1), "1.");
    assert_eq!(move_label(2), "1...");
    assert_eq!(move_label(7), "4.");
    let bars = [(1.0, 0.5), (1.5, -0.5), (2.5, 1.0)];
    assert_eq!(bar_at_ply(&bars, 1), Some(0.5));
    assert_eq!(bar_at_ply(&bars, 2), Some(-0.5));
    assert_eq!(bar_at_ply(&bars, 3), None, "a gap has no value");
    assert_eq!(bar_at_ply(&bars, 4), Some(1.0));
    assert_eq!(bar_at_ply(&bars, 99), None);
    assert_eq!(bar_at_ply(&[], 1), None);
}

fn canvas_for<'a>(view: &'a ScoreGraphView, ui: &'a GraphUi, i18n: &'a I18n) -> GraphCanvas<'a> {
    GraphCanvas {
        view,
        current: None,
        ui,
        i18n,
        report: false,
    }
}

fn press() -> Event {
    Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
}

#[test]
fn a_click_in_the_plot_publishes_the_graph_x_and_one_outside_does_nothing() {
    let game = annotated_game(12);
    let view = score_graph_view(
        &game,
        GraphPrefs {
            style: GraphStyle::Bars,
            ..GraphPrefs::default()
        },
    );
    let (ui, i18n) = (GraphUi::default(), I18n::default());
    let canvas = canvas_for(&view, &ui, &i18n);
    let bounds = Rectangle::new(Point::new(50.0, 20.0), Size::new(500.0, 240.0));
    let plot = plot_rect(bounds.size()).expect("plot");
    let x = 2.0;
    let at = Point::new(
        bounds.x + px_x(&view.x_axis, x, plot),
        bounds.y + plot.y + plot.height / 2.0,
    );
    let action = canvas
        .update(&mut false, &press(), bounds, mouse::Cursor::Available(at))
        .expect("an action");
    let (message, _, status) = action.into_inner();
    let Some(Message::App(AppMessage::Graph(GraphMessage::Click(got)))) = message else {
        panic!("expected a click, got {message:?}");
    };
    assert!((got - x).abs() < 0.02, "{got}");
    assert_eq!(status, iced::event::Status::Captured);
    // On the label strip under the plot, and with the cursor away.
    let strip = Point::new(at.x, bounds.y + bounds.height - 2.0);
    assert!(
        canvas
            .update(
                &mut false,
                &press(),
                bounds,
                mouse::Cursor::Available(strip)
            )
            .is_none()
    );
    assert!(
        canvas
            .update(&mut false, &press(), bounds, mouse::Cursor::Unavailable)
            .is_none()
    );
}

#[test]
fn hovering_names_the_ply_and_the_value() {
    let game = annotated_game(12);
    let view = score_graph_view(
        &game,
        GraphPrefs {
            style: GraphStyle::Bars,
            ..GraphPrefs::default()
        },
    );
    let (ui, i18n) = (GraphUi::default(), I18n::default());
    let canvas = canvas_for(&view, &ui, &i18n);
    // Ply 5 is White's third move: eval (4 - 3) / 2 = 0.5.
    assert_eq!(canvas.hover_text(5).as_deref(), Some("3. +0.50"));
    assert_eq!(canvas.hover_text(2).as_deref(), Some("1... -1.00"));
    assert_eq!(canvas.hover_text(99), None);
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(500.0, 240.0));
    let plot = plot_rect(bounds.size()).expect("plot");
    let over = Point::new(px_x(&view.x_axis, 3.0, plot), plot.y + 10.0);
    assert_eq!(
        canvas.hovered_ply(bounds, mouse::Cursor::Available(over)),
        Some(5)
    );
    assert_eq!(canvas.hovered_ply(bounds, mouse::Cursor::Unavailable), None);
}

#[test]
fn a_time_graph_hovers_in_seconds() {
    let game = annotated_game(12);
    let prefs = GraphPrefs {
        kind: GraphKind::Time,
        ..GraphPrefs::default()
    };
    let view = score_graph_view(&game, prefs);
    let (ui, i18n) = (GraphUi::default(), I18n::default());
    let canvas = canvas_for(&view, &ui, &i18n);
    // The first move spent 1 s (the start clock is the larger first clock
    // plus one, D763); later moves spent none.
    assert_eq!(canvas.hover_text(1).as_deref(), Some("1. 1 s"));
}

#[test]
fn the_cache_is_dropped_only_when_the_stamp_changes() {
    let mut ui = GraphUi::default();
    ui.sync(0);
    ui.sync(3);
    assert_eq!(ui.seen_stamp.get(), 3);
    ui.sync(3);
    assert_eq!(ui.seen_stamp.get(), 3);
}

#[test]
fn opening_the_pane_builds_the_graph_and_closing_forgets_it() {
    let mut shell = open_shell(annotated_game(12));
    assert!(shell.ui.is_open(PaneKind::Graph));
    assert!(shell.app.graph().is_open());
    let graph = shell.app.graph().view().expect("view");
    assert_eq!(graph.bars.len(), 12);
    drop(shell.update(Message::Ui(UiMessage::TogglePane(PaneKind::Graph))));
    assert!(!shell.app.graph().is_open());
    assert!(shell.app.graph().view().is_none());
}

#[test]
fn the_pane_survives_other_panes_and_goes_with_the_reset() {
    let mut shell = open_shell(annotated_game(4));
    let toggle = |shell: &mut Shell, kind| {
        drop(shell.update(Message::Ui(UiMessage::TogglePane(kind))));
    };
    toggle(&mut shell, PaneKind::Info);
    toggle(&mut shell, PaneKind::Analysis);
    assert!(shell.ui.layout().panes().contains(&PaneKind::Graph));
    assert!(shell.app.graph().is_open());
    drop(shell.update(Message::Ui(UiMessage::ResetLayout)));
    assert!(!shell.ui.is_open(PaneKind::Graph));
    assert!(!shell.app.graph().is_open());
    assert!(!shell.ui.layout().panes().contains(&PaneKind::Graph));
}

#[test]
fn the_shortcut_opens_the_pane_and_a_click_moves_the_cursor() {
    let mut shell = Shell::new(None);
    shell.app.session.replace_game(annotated_game(12));
    let binding = shell
        .app
        .shortcuts
        .bindings(mychess_app::Action::ToggleGraphPane);
    assert_eq!(binding.len(), 1);
    drop(shell.update(Message::Ui(UiMessage::Action(
        mychess_app::Action::ToggleGraphPane,
    ))));
    assert!(shell.ui.is_open(PaneKind::Graph) && shell.app.graph().is_open());
    drop(shell.update(Message::App(AppMessage::Graph(GraphMessage::Click(2.0)))));
    assert_eq!(
        shell.app.session.game().ply(shell.app.session.cursor()),
        Ok(3)
    );
    assert_eq!(shell.app.graph().current_x(), Some(2.0));
    drop(
        shell.update(Message::App(AppMessage::Game(GameMessage::Navigate(
            Nav::Start,
        )))),
    );
    assert_eq!(shell.app.graph().current_x(), None);
}

#[test]
fn the_panel_shows_the_title_and_the_options() {
    let shell = open_shell(annotated_game(12));
    let mut sim = Simulator::with_size(Settings::default(), Size::new(900.0, 400.0), view(&shell));
    assert!(sim.find("Anna - Boris").is_ok());
    assert!(sim.find("Highlight current move").is_ok());
    assert!(sim.find("Invert White").is_ok());
    assert!(sim.find("Invert Black").is_ok());
    assert!(sim.find("No scores or times in this game").is_err());
    sim.click("Invert Black").expect("checkbox");
    let sent: Vec<Message> = sim.into_messages().collect();
    assert!(matches!(
        sent.as_slice(),
        [Message::App(AppMessage::Graph(
            GraphMessage::ToggleInvertBlack
        ))]
    ));
}

#[test]
fn a_game_without_data_says_so() {
    let shell = open_shell(Game::new());
    let mut sim = Simulator::with_size(Settings::default(), Size::new(900.0, 400.0), view(&shell));
    assert!(sim.find("No scores or times in this game").is_ok());
}

#[test]
fn the_texts_are_translated() {
    let mut shell = open_shell(Game::new());
    shell.i18n = I18n::new(crate::i18n::Language::PtBr);
    let mut sim = Simulator::with_size(Settings::default(), Size::new(900.0, 400.0), view(&shell));
    assert!(sim.find("Sem pontuações nem tempos nesta partida").is_ok());
    assert!(sim.find("Destacar o lance atual").is_ok());
}

#[test]
fn the_line_style_clicks_drags_and_hovers_by_ply() {
    let game = annotated_game(12);
    let view = score_graph_view(&game, GraphPrefs::default());
    assert!(view.line.is_some(), "the default style is the line");
    let (ui, i18n) = (GraphUi::default(), I18n::default());
    let canvas = canvas_for(&view, &ui, &i18n);
    let bounds = Rectangle::new(Point::new(50.0, 20.0), Size::new(500.0, 240.0));
    let plot = plot_rect(bounds.size()).expect("plot");
    let plies = view.plies;
    let at_ply = |ply: usize| {
        Point::new(
            bounds.x + line_px_x(ply as f32, plies, plot),
            bounds.y + plot.y + plot.height / 2.0,
        )
    };
    let click_of = |action: Option<Action<Message>>| {
        let (message, _, _) = action.expect("an action").into_inner();
        match message {
            Some(Message::App(AppMessage::Graph(GraphMessage::GoToPly(ply)))) => ply,
            other => panic!("expected a go-to-ply, got {other:?}"),
        }
    };
    let mut dragging = false;
    let first = canvas.update(
        &mut dragging,
        &press(),
        bounds,
        mouse::Cursor::Available(at_ply(4)),
    );
    assert_eq!(click_of(first), 4);
    assert!(dragging);
    // The start position is reachable too (D1556).
    let start = canvas.update(
        &mut dragging,
        &press(),
        bounds,
        mouse::Cursor::Available(at_ply(0)),
    );
    assert_eq!(click_of(start), 0);
    // Moving with the button held walks the game.
    let moved = Event::Mouse(mouse::Event::CursorMoved {
        position: at_ply(7),
    });
    let drag = canvas.update(
        &mut dragging,
        &moved,
        bounds,
        mouse::Cursor::Available(at_ply(7)),
    );
    assert_eq!(click_of(drag), 7);
    let release = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    assert!(
        canvas
            .update(&mut dragging, &release, bounds, mouse::Cursor::Unavailable)
            .is_none()
    );
    assert!(!dragging);
    // Without the button, moving only redraws.
    let (message, _, _) = canvas
        .update(
            &mut dragging,
            &moved,
            bounds,
            mouse::Cursor::Available(at_ply(7)),
        )
        .expect("a redraw")
        .into_inner();
    assert!(message.is_none());
    assert_eq!(canvas.hover_text(5).as_deref(), Some("3. +0.50"));
}

#[test]
fn line_pixel_maps_stay_inside_the_plot() {
    let plot = Rectangle::new(Point::new(34.0, 8.0), Size::new(400.0, 100.0));
    assert_eq!(line_px_x(0.0, 10, plot), 34.0);
    assert_eq!(line_px_x(10.0, 10, plot), 434.0);
    assert_eq!(line_px_y(0.0, plot), 58.0);
    assert_eq!(line_px_y(5.0, plot), 8.0, "out of range is clamped");
    assert_eq!(line_ply_at_px(34.0, 10, plot), None, "ply 0 is not a move");
    assert_eq!(line_ply_at_px(434.0, 10, plot), Some(10));
}
