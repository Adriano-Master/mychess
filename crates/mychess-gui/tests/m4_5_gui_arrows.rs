//! M4-5 (GUI): the engine arrows on the board (spec D456, D457, D459, D853,
//! D854, regras 8 a 10; Scid `analysis.tcl` `boardVisualizeMoves`,
//! `updateAnalysisBoard`). Pixel probes with the software renderer and mouse
//! scripts through `iced_test`. Headless.
#![allow(
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::unnecessary_wraps,
    clippy::assert_is_empty,
    clippy::many_single_char_names,
    clippy::trivially_copy_pass_by_ref
)]

mod m2_4_common;
mod render_common;

use iced::keyboard::{self, Modifiers};
use iced::{Event, Point, Settings, Size, mouse};
use iced_test::Simulator;
use m2_4_common::{fingerprint, render_lock};
use mychess_app::{ArrowKind, EngineArrow};
use mychess_core::{BoardMark, Color, Position, Square};
use mychess_gui::theme::engine_overlay as eo;
use mychess_gui::widgets::board::animation::{MoveAnimation, animation_between};
use mychess_gui::widgets::board::geometry::{BoardLayout, CoordsMode, Orientation};
use mychess_gui::widgets::board::interaction::BoardEvent;
use mychess_gui::widgets::board::marks::{MarkBrush, engine_arrow_geometry};
use mychess_gui::widgets::board::pieces::PieceSet;
use mychess_gui::widgets::board::{BoardCaches, BoardData, BoardOptions, BoardWidget};
use render_common::{SCALE, Shot, render};

const SIZE: Size = Size::new(800.0, 800.0);
const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const KINGS: &str = "4k3/8/8/8/8/8/8/4K3 w - - 0 1";

fn sq(name: &str) -> Square {
    name.parse().expect("square")
}

fn arrow(from: &str, to: &str, rank: u8, kind: ArrowKind) -> EngineArrow {
    EngineArrow {
        from: sq(from),
        to: sq(to),
        rank,
        kind,
        side: Color::White,
        label: None,
    }
}

struct Scene {
    position: Position,
    arrows: Vec<EngineArrow>,
    click_to_play: bool,
    orientation: Orientation,
    coords: CoordsMode,
    accepts: bool,
    marks: Vec<BoardMark>,
    animation: Option<MoveAnimation>,
}

impl Scene {
    fn new(fen: &str, arrows: Vec<EngineArrow>) -> Scene {
        Scene {
            position: Position::from_fen(fen).expect("fen"),
            arrows,
            click_to_play: true,
            orientation: Orientation::White,
            coords: CoordsMode::None,
            accepts: true,
            marks: Vec::new(),
            animation: None,
        }
    }

    fn without_arrows(&self) -> Scene {
        Scene {
            position: self.position.clone(),
            arrows: Vec::new(),
            click_to_play: self.click_to_play,
            orientation: self.orientation,
            coords: self.coords,
            accepts: self.accepts,
            marks: self.marks.clone(),
            animation: self.animation.clone(),
        }
    }

    fn layout(&self) -> BoardLayout {
        BoardLayout::compute(
            SIZE,
            BoardOptions {
                coords: self.coords,
                ..BoardOptions::default()
            }
            .layout_options(),
        )
        .expect("fits")
    }

    fn at(&self, name: &str) -> Point {
        self.layout().square_center(sq(name), self.orientation)
    }

    fn with_element<R>(&self, f: impl FnOnce(iced::Element<'_, BoardEvent>) -> R) -> R {
        let brush = MarkBrush::default();
        let pieces = PieceSet::default();
        let mut caches = BoardCaches::default();
        caches.sync_arrows(&self.arrows, self.orientation);
        let data = BoardData {
            position: &self.position,
            last_move: None,
            marks: &self.marks,
            orientation: self.orientation,
            options: BoardOptions {
                coords: self.coords,
                ..BoardOptions::default()
            },
            accepts_moves: self.accepts,
            brush: &brush,
            pieces: &pieces,
            animation: self.animation.as_ref().map(|a| (a, 0.5)),
        };
        let widget =
            BoardWidget::new(data, &caches).with_engine_arrows(&self.arrows, self.click_to_play);
        f(widget.into_element())
    }

    fn shot(&self) -> Shot {
        self.with_element(|e| render(e, SIZE))
    }

    fn run(&self, script: impl FnOnce(&Scene, &mut Simulator<'_, BoardEvent>)) -> Vec<BoardEvent> {
        let _guard = render_lock();
        self.with_element(|e| {
            let mut sim = Simulator::with_size(Settings::default(), SIZE, e);
            script(self, &mut sim);
            sim.into_messages().collect()
        })
    }

    fn print(&self) -> String {
        let _guard = render_lock();
        self.with_element(|e| {
            let mut sim = Simulator::with_size(Settings::default(), SIZE, e);
            fingerprint(&mut sim)
        })
    }
}

fn dev(v: f32) -> u32 {
    (v * SCALE) as u32
}

fn px(shot: &Shot, p: Point) -> [u8; 4] {
    shot.pixel(dev(p.x), dev(p.y))
}

/// The logical thickness of what the arrows add on the row `y`.
fn thickness_at(with: &Shot, without: &Shot, y: f32) -> f32 {
    let row = dev(y);
    let n = (0..dev(SIZE.width))
        .filter(|&x| with.pixel(x, row) != without.pixel(x, row))
        .count();
    n as f32 / SCALE
}

/// Whether the pixel is `color` at `ARROW_ALPHA` over what was there.
fn is_blend(with: [u8; 4], under: [u8; 4], color: iced::Color, alpha: f32) -> bool {
    let c = [color.r, color.g, color.b];
    (0..3).all(|i| {
        let want = alpha * c[i] * 255.0 + (1.0 - alpha) * f32::from(under[i]);
        (f32::from(with[i]) - want).abs() <= 4.0
    })
}

fn changed_pixels(a: &Shot, b: &Shot) -> Vec<(u32, u32)> {
    let side = dev(SIZE.width);
    let mut out = Vec::new();
    for y in 0..side {
        for x in 0..side {
            if a.pixel(x, y) != b.pixel(x, y) {
                out.push((x, y));
            }
        }
    }
    out
}

// ---- drawing ----

#[test]
fn an_arrow_is_painted_between_its_squares_and_nowhere_else() {
    let scene = Scene::new(KINGS, vec![arrow("e2", "e4", 1, ArrowKind::Best)]);
    let diff = changed_pixels(&scene.shot(), &scene.without_arrows().shot());
    assert!(!diff.is_empty(), "the arrow is visible");
    let (x0, x1) = (dev(400.0), dev(500.0));
    let (y0, y1) = (dev(400.0), dev(700.0));
    for (x, y) in diff {
        assert!(
            (x0..x1).contains(&x) && (y0..y1).contains(&y),
            "pixel ({x}, {y}) outside e2..e4"
        );
    }
}

#[test]
fn the_best_arrow_uses_the_engine_blue_with_the_arrow_alpha() {
    let scene = Scene::new(KINGS, vec![arrow("e2", "e4", 1, ArrowKind::Best)]);
    let (with, without) = (scene.shot(), scene.without_arrows().shot());
    let p = Point::new(450.0, 600.0);
    assert!(
        is_blend(
            px(&with, p),
            px(&without, p),
            eo::BEST_ARROW,
            eo::ARROW_ALPHA
        ),
        "{:?} over {:?}",
        px(&with, p),
        px(&without, p)
    );
}

#[test]
fn other_lines_are_grey_and_sequence_arrows_follow_the_side() {
    let mut black = arrow("e2", "e4", 1, ArrowKind::Sequence(0));
    black.side = Color::Black;
    let cases = [
        (arrow("e2", "e4", 2, ArrowKind::Other), eo::OTHER_ARROW),
        (
            arrow("e2", "e4", 1, ArrowKind::Sequence(0)),
            eo::SEQUENCE_WHITE,
        ),
        (black, eo::SEQUENCE_BLACK),
    ];
    for (a, color) in cases {
        let scene = Scene::new(KINGS, vec![a]);
        let (with, without) = (scene.shot(), scene.without_arrows().shot());
        let p = Point::new(450.0, 600.0);
        assert!(
            is_blend(px(&with, p), px(&without, p), color, eo::ARROW_ALPHA),
            "{:?}: {:?} over {:?}",
            a.kind,
            px(&with, p),
            px(&without, p)
        );
    }
}

#[test]
fn shaft_thickness_is_014_010_and_007_of_a_square() {
    // s = 100: 14, 10 and 7 px; measured on a row inside the shaft.
    for (kind, want) in [
        (ArrowKind::Best, 14.0),
        (ArrowKind::Other, 10.0),
        (ArrowKind::Sequence(0), 7.0),
    ] {
        let scene = Scene::new(KINGS, vec![arrow("e2", "e4", 1, kind)]);
        let t = thickness_at(&scene.shot(), &scene.without_arrows().shot(), 600.0);
        assert!((t - want).abs() <= 1.5, "{kind:?}: {t} vs {want}");
    }
}

#[test]
fn the_best_arrow_is_painted_on_top_whatever_the_list_order() {
    // The grey arrow c2-g2 crosses the shaft of e2-e4 at the centre of e2.
    let best = arrow("e2", "e5", 1, ArrowKind::Best);
    let other = arrow("c3", "g3", 2, ArrowKind::Other);
    let p = Point::new(450.0, 550.0);
    for list in [vec![best, other], vec![other, best]] {
        let shot = Scene::new(KINGS, list).shot();
        let c = px(&shot, p);
        // On top: mostly blue (low red); below: mostly grey (red ~ 0.45).
        assert!(c[0] < 85, "the blue arrow is on top: {c:?}");
        assert!(c[2] > c[0] + 40, "still blue: {c:?}");
    }
}

#[test]
fn a_numbered_arrow_carries_a_disc_halfway_along_the_shaft() {
    let mut numbered = arrow("e2", "e4", 2, ArrowKind::Other);
    numbered.label = Some(2);
    let plain = Scene::new(KINGS, vec![arrow("e2", "e4", 2, ArrowKind::Other)]);
    let with_number = Scene::new(KINGS, vec![numbered]);
    let layout = plain.layout();
    let shape = engine_arrow_geometry(&layout, sq("e2"), sq("e4"), Orientation::White, 0.10)
        .expect("geometry");
    let mid = Point::new(
        f32::midpoint(shape.tail.x, shape.base.x),
        f32::midpoint(shape.tail.y, shape.base.y),
    );
    // A point of the disc, clear of the shaft and of the digit.
    let probe = Point::new(mid.x + 14.0, mid.y);
    let (a, b) = (with_number.shot(), plain.shot());
    assert_eq!(px(&b, probe), px(&plain.without_arrows().shot(), probe));
    assert_ne!(px(&a, probe), px(&b, probe), "the disc is there");
    let under = px(&plain.without_arrows().shot(), probe);
    // Changed from `ARROW_ALPHA` to 1.0 (fix M4-5, D858): the disc is
    // opaque, so that its digit reads whatever is below. The old probe
    // required the translucent disc, which `scale_alpha(1.0)` produced by
    // mistake (it multiplies, it does not set).
    assert!(
        is_blend(px(&a, probe), under, eo::OTHER_ARROW, 1.0),
        "the disc is the opaque arrow colour: {:?} over {under:?}",
        px(&a, probe)
    );
}

#[test]
fn the_head_of_an_arrow_is_painted_above_the_piece_it_ends_on() {
    // e2-e7 ends on the black pawn of e7: the arrow layer is above the
    // pieces (D853, fix D858), so the pieces show through the 0.8 alpha.
    let scene = Scene::new(START, vec![arrow("e2", "e7", 1, ArrowKind::Best)]);
    let (with, without) = (scene.shot(), scene.without_arrows().shot());
    let shape = engine_arrow_geometry(
        &scene.layout(),
        sq("e2"),
        sq("e7"),
        Orientation::White,
        0.14,
    )
    .expect("geometry");
    let corner = scene.at("e7");
    let square = px(&without, Point::new(corner.x - 46.0, corner.y - 46.0));
    let mut on_piece = 0;
    for step in 10..36 {
        let t = step as f32;
        let p = Point::new(
            shape.tip.x,
            shape.tip.y + t * (shape.base.y - shape.tip.y).signum(),
        );
        if px(&without, p) == square {
            continue;
        }
        on_piece += 1;
        assert!(
            is_blend(
                px(&with, p),
                px(&without, p),
                eo::BEST_ARROW,
                eo::ARROW_ALPHA
            ),
            "the head is under the piece at {p:?}: {:?} over {:?}",
            px(&with, p),
            px(&without, p)
        );
    }
    assert!(
        on_piece >= 3,
        "the probe found only {on_piece} piece pixels"
    );
}

#[test]
fn an_arrow_without_number_has_no_disc() {
    let plain = Scene::new(KINGS, vec![arrow("e2", "e4", 2, ArrowKind::Other)]);
    let diff = changed_pixels(&plain.shot(), &plain.without_arrows().shot());
    // Only the shaft (10 px) and the head (<= ~40 px): never 36 px wide.
    let xs: Vec<u32> = diff.iter().map(|(x, _)| *x).collect();
    let (lo, hi) = (*xs.iter().min().unwrap(), *xs.iter().max().unwrap());
    assert!((hi - lo) as f32 / SCALE < 40.0);
}

#[test]
fn a_castling_arrow_goes_from_king_to_king_destination() {
    let scene = Scene::new(
        "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
        vec![arrow("e1", "g1", 1, ArrowKind::Best)],
    );
    let diff = changed_pixels(&scene.shot(), &scene.without_arrows().shot());
    let xs: Vec<f32> = diff.iter().map(|(x, _)| *x as f32 / SCALE).collect();
    let max = xs.iter().copied().fold(0.0, f32::max);
    let min = xs.iter().copied().fold(f32::MAX, f32::min);
    assert!(min >= 400.0 && max <= 700.0, "e1 to g1 only: {min}..{max}");
    assert!(max > 600.0, "reaches g1 (x 600..700): {max}");
}

#[test]
fn a_turned_board_draws_the_arrow_between_the_turned_squares() {
    let mut scene = Scene::new(KINGS, vec![arrow("e2", "e4", 1, ArrowKind::Best)]);
    scene.orientation = Orientation::Black;
    let diff = changed_pixels(&scene.shot(), &scene.without_arrows().shot());
    assert!(!diff.is_empty());
    // With Black at the bottom e2 is at y 100..200 and e4 at 300..400.
    for (_, y) in diff {
        assert!((dev(100.0)..dev(400.0)).contains(&y), "y = {y}");
    }
}

#[test]
fn arrows_never_paint_over_the_outside_coordinates() {
    let mut scene = Scene::new(KINGS, vec![arrow("a1", "h8", 1, ArrowKind::Best)]);
    scene.coords = CoordsMode::Outside;
    let layout = scene.layout();
    let board = layout.board;
    let diff = changed_pixels(&scene.shot(), &scene.without_arrows().shot());
    assert!(!diff.is_empty());
    for (x, y) in diff {
        let (x, y) = (x as f32 / SCALE, y as f32 / SCALE);
        assert!(
            x >= board.x
                && x <= board.x + board.width
                && y >= board.y
                && y <= board.y + board.height,
            "({x}, {y}) is outside {board:?}"
        );
    }
}

#[test]
fn a_degenerate_arrow_is_skipped_without_panicking() {
    let scene = Scene::new(KINGS, vec![arrow("e4", "e4", 1, ArrowKind::Best)]);
    let diff = changed_pixels(&scene.shot(), &scene.without_arrows().shot());
    assert!(diff.is_empty(), "from == to draws nothing");
}

#[test]
fn arrows_are_hidden_while_a_move_animates() {
    let old = Position::from_fen(START).unwrap();
    let mut new = old.clone();
    let mv = new.parse_uci("e2e4").unwrap();
    new.play(mv).unwrap();
    let anim = animation_between(&old, &new).expect("an animation");
    let mut scene = Scene::new(START, vec![arrow("g1", "f3", 1, ArrowKind::Best)]);
    scene.position = new;
    scene.animation = Some(anim);
    assert_eq!(
        changed_pixels(&scene.shot(), &scene.without_arrows().shot()),
        [],
        "the arrow layer is not drawn during the animation"
    );
    scene.animation = None;
    assert!(
        !changed_pixels(&scene.shot(), &scene.without_arrows().shot()).is_empty(),
        "and comes back at its end"
    );
}

// ---- mouse ----

fn move_to(sim: &mut Simulator<'_, BoardEvent>, p: Point) {
    sim.point_at(p);
    sim.simulate([Event::Mouse(mouse::Event::CursorMoved { position: p })]);
}

fn press(sim: &mut Simulator<'_, BoardEvent>, b: mouse::Button) {
    sim.simulate([Event::Mouse(mouse::Event::ButtonPressed(b))]);
}

fn release(sim: &mut Simulator<'_, BoardEvent>, b: mouse::Button) {
    sim.simulate([Event::Mouse(mouse::Event::ButtonReleased(b))]);
}

fn click(sim: &mut Simulator<'_, BoardEvent>, p: Point) {
    move_to(sim, p);
    press(sim, mouse::Button::Left);
    release(sim, mouse::Button::Left);
}

fn hold(sim: &mut Simulator<'_, BoardEvent>, m: Modifiers) {
    sim.simulate([Event::Keyboard(keyboard::Event::ModifiersChanged(m))]);
}

/// The middle of the head of the arrow `a`: a point of the clickable tip.
fn tip_of(scene: &Scene, a: &EngineArrow) -> Point {
    let shaft = match a.kind {
        ArrowKind::Best | ArrowKind::ReviewBest => 0.14,
        ArrowKind::Other => 0.10,
        ArrowKind::Sequence(_) => 0.07,
    };
    let g = engine_arrow_geometry(&scene.layout(), a.from, a.to, scene.orientation, shaft)
        .expect("geometry");
    let [p, q, r] = g.head;
    Point::new((p.x + q.x + r.x) / 3.0, (p.y + q.y + r.y) / 3.0)
}

fn played(events: &[BoardEvent]) -> Vec<(Square, Square, u8)> {
    events
        .iter()
        .filter_map(|e| match e {
            BoardEvent::PlayEngineArrow { from, to, rank } => Some((*from, *to, *rank)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_left_click_on_the_tip_publishes_the_arrow() {
    let a = arrow("e2", "e4", 1, ArrowKind::Best);
    let scene = Scene::new(START, vec![a]);
    let events = scene.run(|s, sim| click(sim, tip_of(s, &a)));
    assert_eq!(played(&events), [(sq("e2"), sq("e4"), 1)]);
    assert_eq!(
        events.len(),
        1,
        "and nothing else (no piece gesture): {events:?}"
    );
}

#[test]
fn the_rank_of_the_clicked_arrow_is_published() {
    let best = arrow("e2", "e4", 1, ArrowKind::Best);
    let other = arrow("d2", "d4", 3, ArrowKind::Other);
    let scene = Scene::new(START, vec![best, other]);
    let events = scene.run(|s, sim| click(sim, tip_of(s, &other)));
    assert_eq!(played(&events), [(sq("d2"), sq("d4"), 3)]);
}

#[test]
fn a_click_on_the_shaft_or_the_tail_is_not_a_play() {
    let a = arrow("e2", "e4", 1, ArrowKind::Best);
    let scene = Scene::new(START, vec![a]);
    let events = scene.run(|s, sim| {
        click(sim, Point::new(450.0, 560.0)); // shaft, below the head
        click(sim, s.at("e2")); // the tail
    });
    assert_eq!(played(&events), []);
}

#[test]
fn a_click_far_from_every_arrow_is_not_a_play() {
    let a = arrow("e2", "e4", 1, ArrowKind::Best);
    let scene = Scene::new(START, vec![a]);
    let events = scene.run(|s, sim| click(sim, s.at("a8")));
    assert_eq!(played(&events), []);
}

#[test]
fn without_click_to_play_the_tip_is_inert() {
    let a = arrow("e2", "e4", 1, ArrowKind::Best);
    let mut scene = Scene::new(START, vec![a]);
    scene.click_to_play = false;
    let events = scene.run(|s, sim| click(sim, tip_of(s, &a)));
    assert_eq!(played(&events), []);
}

#[test]
fn shift_ctrl_and_alt_clicks_do_not_play_the_arrow() {
    let a = arrow("e2", "e4", 1, ArrowKind::Best);
    let scene = Scene::new(START, vec![a]);
    for (name, m) in [
        ("shift", Modifiers::SHIFT),
        ("command", Modifiers::COMMAND),
        ("ctrl", Modifiers::CTRL),
        ("alt", Modifiers::ALT),
    ] {
        let events = scene.run(|s, sim| {
            hold(sim, m);
            click(sim, tip_of(s, &a));
        });
        assert_eq!(played(&events), [], "{name}");
    }
}

#[test]
fn only_the_left_button_plays_and_the_right_button_marks_underneath() {
    let a = arrow("e2", "e4", 1, ArrowKind::Best);
    let scene = Scene::new(START, vec![a]);
    let events = scene.run(|s, sim| {
        let tip = tip_of(s, &a);
        move_to(sim, tip);
        press(sim, mouse::Button::Right);
        release(sim, mouse::Button::Right);
    });
    assert_eq!(played(&events), []);
    assert_eq!(events.len(), 1, "a circle mark: {events:?}");
    assert!(matches!(events[0], BoardEvent::ToggleMark(_)));
}

#[test]
fn right_drag_marks_still_work_with_engine_arrows_present() {
    let a = arrow("e2", "e4", 1, ArrowKind::Best);
    let scene = Scene::new(START, vec![a]);
    let events = scene.run(|s, sim| {
        move_to(sim, s.at("d4"));
        press(sim, mouse::Button::Right);
        move_to(sim, s.at("f6"));
        release(sim, mouse::Button::Right);
    });
    assert_eq!(
        events,
        [BoardEvent::ToggleMark(BoardMark::arrow(
            sq("d4"),
            sq("f6"),
            "green"
        ))]
    );
}

#[test]
fn moves_by_clicking_pieces_still_work_with_arrows_present() {
    let a = arrow("g1", "f3", 1, ArrowKind::Best);
    let scene = Scene::new(START, vec![a]);
    let events = scene.run(|s, sim| {
        click(sim, s.at("e2"));
        click(sim, s.at("e4"));
    });
    assert!(
        events
            .iter()
            .any(|e| matches!(e, BoardEvent::PlayMove { .. })),
        "{events:?}"
    );
}

#[test]
fn alt_click_on_a_tip_passes_through_to_the_square_below() {
    // The tip of g1-f3 covers f3; with Alt the click goes to the board (no
    // PlayEngineArrow) instead of being taken by the arrow.
    let a = arrow("e2", "e4", 1, ArrowKind::Best);
    let scene = Scene::new(START, vec![a]);
    let with_alt = scene.run(|s, sim| {
        hold(sim, Modifiers::ALT);
        click(sim, tip_of(s, &a));
    });
    assert_eq!(played(&with_alt), []);
}

#[test]
fn a_press_on_the_tip_released_elsewhere_does_not_play() {
    let a = arrow("e2", "e4", 1, ArrowKind::Best);
    let scene = Scene::new(START, vec![a]);
    let events = scene.run(|s, sim| {
        move_to(sim, tip_of(s, &a));
        press(sim, mouse::Button::Left);
        move_to(sim, s.at("a6"));
        release(sim, mouse::Button::Left);
    });
    assert_eq!(played(&events), []);
}

#[test]
fn only_the_first_sequence_arrow_is_clickable() {
    let first = arrow("e2", "e4", 1, ArrowKind::Sequence(0));
    let later = arrow("e7", "e5", 1, ArrowKind::Sequence(1));
    let scene = Scene::new(START, vec![first, later]);
    let events = scene.run(|s, sim| click(sim, tip_of(s, &later)));
    assert_eq!(played(&events), [], "ply 1 is not playable now");
    let events = scene.run(|s, sim| click(sim, tip_of(s, &first)));
    assert_eq!(played(&events), [(sq("e2"), sq("e4"), 1)], "ply 0 is");
}

#[test]
fn a_click_on_the_tip_of_a_turned_board_hits_the_turned_head() {
    let a = arrow("e2", "e4", 1, ArrowKind::Best);
    let mut scene = Scene::new(START, vec![a]);
    scene.orientation = Orientation::Black;
    let events = scene.run(|s, sim| click(sim, tip_of(s, &a)));
    assert_eq!(played(&events), [(sq("e2"), sq("e4"), 1)]);
}

#[test]
fn arrows_cannot_be_clicked_while_a_move_animates() {
    let old = Position::from_fen(START).unwrap();
    let mut new = old.clone();
    let mv = new.parse_uci("d2d4").unwrap();
    new.play(mv).unwrap();
    let anim = animation_between(&old, &new).expect("an animation");
    let a = arrow("g1", "f3", 1, ArrowKind::Best);
    let mut scene = Scene::new(START, vec![a]);
    scene.position = new;
    scene.animation = Some(anim);
    let events = scene.run(|s, sim| click(sim, tip_of(s, &a)));
    assert_eq!(played(&events), []);
}

#[test]
fn arrows_disappear_while_a_piece_is_dragged() {
    // Drag the h2 pawn to the middle of the board: with or without arrows
    // the scene is the same, since the layer is hidden.
    let a = arrow("e2", "e4", 1, ArrowKind::Best);
    let scene = Scene::new(START, vec![a]);
    let drag = |s: &Scene| {
        let _guard = render_lock();
        s.with_element(|e| {
            let mut sim = Simulator::with_size(Settings::default(), SIZE, e);
            move_to(&mut sim, s.at("h2"));
            press(&mut sim, mouse::Button::Left);
            move_to(&mut sim, s.at("h3"));
            move_to(&mut sim, s.at("h5"));
            fingerprint(&mut sim)
        })
    };
    assert_eq!(drag(&scene), drag(&scene.without_arrows()));
    assert_ne!(
        scene.print(),
        scene.without_arrows().print(),
        "idle: visible"
    );
}

#[test]
fn a_board_that_does_not_accept_moves_does_not_play_arrows() {
    let a = arrow("e2", "e4", 1, ArrowKind::Best);
    let mut scene = Scene::new(START, vec![a]);
    scene.accepts = false;
    let events = scene.run(|s, sim| click(sim, tip_of(s, &a)));
    assert_eq!(played(&events), []);
}
