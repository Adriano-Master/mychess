//! M2-3 board widget with `iced_test` (headless): event scripts through the
//! real canvas (rules 22, 33-40, 47, 58) and pixel *relations* between
//! renders (rules 24-31, 42, 46, 55, 56): equal scenes render equal, scenes
//! the spec says are equivalent render equal, and scenes it says differ,
//! differ. No stored images: everything is compared within one run.

#![allow(clippy::assert_is_empty, clippy::float_cmp, clippy::unnecessary_wraps)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use iced::keyboard::{self, Modifiers, key::Named};
use iced::widget::container;
use iced::{Event, Point, Settings, Size, Theme, mouse};
use iced_test::Simulator;
use iced_test::simulator::press_key;
use mychess_core::{BoardMark, MarkKind, Move, Position, Role, Square};
use mychess_gui::theme::board_theme::BoardThemeId;
use mychess_gui::widgets::board::animation::{MoveAnimation, animation_between};
use mychess_gui::widgets::board::geometry::{BoardLayout, CoordsMode, Orientation};
use mychess_gui::widgets::board::interaction::BoardEvent;
use mychess_gui::widgets::board::marks::MarkBrush;
use mychess_gui::widgets::board::pieces::PieceSet;
use mychess_gui::widgets::board::{BoardCaches, BoardData, BoardOptions, BoardWidget};
use rstest::rstest;

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const PROMO: &str = "1n2k3/P7/8/8/8/8/8/4K3 w - - 0 1";
const CASTLE: &str = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
const KINGS: &str = "4k3/8/8/8/8/8/8/4K3 w - - 0 1";
const SIZE: Size = Size::new(800.0, 800.0);

fn sq(name: &str) -> Square {
    name.parse().expect("square")
}

/// Everything needed to build a `BoardWidget`.
#[derive(Clone)]
struct Scene {
    fen: &'static str,
    last_move: Option<Move>,
    marks: Vec<BoardMark>,
    orientation: Orientation,
    options: BoardOptions,
    accepts: bool,
    animation: Option<(MoveAnimation, f32)>,
    size: Size,
    padding: f32,
}

impl Scene {
    fn new(fen: &'static str) -> Scene {
        Scene {
            fen,
            last_move: None,
            marks: Vec::new(),
            orientation: Orientation::White,
            options: BoardOptions::default(),
            accepts: true,
            animation: None,
            size: SIZE,
            padding: 0.0,
        }
    }

    fn canvas_size(&self) -> Size {
        Size::new(
            self.size.width - 2.0 * self.padding,
            self.size.height - 2.0 * self.padding,
        )
    }

    fn layout(&self) -> Option<BoardLayout> {
        BoardLayout::compute(self.canvas_size(), self.options.layout_options())
    }

    /// Absolute window position of the center of a square.
    fn at(&self, name: &str) -> Point {
        let c = self
            .layout()
            .expect("layout fits")
            .square_center(sq(name), self.orientation);
        Point::new(c.x + self.padding, c.y + self.padding)
    }

    fn with_simulator<R>(&self, f: impl FnOnce(Simulator<'_, BoardEvent>) -> R) -> R {
        let position = Position::from_fen(self.fen).expect("fen");
        let brush = MarkBrush::default();
        let pieces = PieceSet::default();
        let caches = BoardCaches::default();
        let data = BoardData {
            position: &position,
            last_move: self.last_move,
            marks: &self.marks,
            orientation: self.orientation,
            options: self.options,
            accepts_moves: self.accepts,
            brush: &brush,
            pieces: &pieces,
            animation: self.animation.as_ref().map(|(a, t)| (a, *t)),
        };
        let board = BoardWidget::new(data, &caches).into_element();
        let element = if self.padding > 0.0 {
            container(board).padding(self.padding).into()
        } else {
            board
        };
        f(Simulator::with_size(
            Settings::default(),
            self.size,
            element,
        ))
    }

    fn events(
        &self,
        script: impl FnOnce(&Scene, &mut Simulator<'_, BoardEvent>),
    ) -> Vec<BoardEvent> {
        self.with_simulator(|mut sim| {
            script(self, &mut sim);
            sim.into_messages().collect()
        })
    }
}

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

fn set_modifiers(sim: &mut Simulator<'_, BoardEvent>, m: Modifiers) {
    sim.simulate([Event::Keyboard(keyboard::Event::ModifiersChanged(m))]);
}

fn tap(sim: &mut Simulator<'_, BoardEvent>, key: Named) {
    sim.simulate([press_key(keyboard::Key::Named(key), None)]);
}

type Played = (Option<Square>, Option<Square>, Option<Role>, bool);

fn plays(events: &[BoardEvent]) -> Vec<Played> {
    events
        .iter()
        .filter_map(|e| match e {
            BoardEvent::PlayMove { mv, dragged } => {
                Some((mv.from(), mv.to(), mv.promotion(), *dragged))
            }
            _ => None,
        })
        .collect()
}

fn marks_of(events: &[BoardEvent]) -> Vec<BoardMark> {
    events
        .iter()
        .filter_map(|e| match e {
            BoardEvent::ToggleMark(m) => Some(m.clone()),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Event scripts through the real canvas
// ---------------------------------------------------------------------------

#[test]
fn m2_3_widget_offsets_pointer_by_the_canvas_origin() {
    // The canvas sits 50 px inside the window: local coordinates matter.
    let mut scene = Scene::new(START);
    scene.padding = 50.0;
    let events = scene.events(|s, sim| {
        click(sim, s.at("e2"));
        click(sim, s.at("e4"));
    });
    assert_eq!(
        plays(&events),
        [(Some(sq("e2")), Some(sq("e4")), None, false)]
    );
}

#[test]
fn m2_3_widget_press_outside_the_canvas_does_nothing() {
    let mut scene = Scene::new(START);
    scene.padding = 50.0;
    let events = scene.events(|s, sim| {
        // (20, 20) is in the padding, outside the canvas; then a real click on
        // e4 must not be taken as the second click of anything.
        click(sim, Point::new(20.0, 20.0));
        click(sim, s.at("e4"));
    });
    assert!(events.is_empty(), "{events:?}");
}

#[test]
fn m2_3_widget_black_at_the_bottom_click_click_and_drag() {
    let mut scene = Scene::new(START);
    scene.orientation = Orientation::Black;
    let events = scene.events(|s, sim| {
        click(sim, s.at("e2"));
        click(sim, s.at("e4"));
        move_to(sim, s.at("g1"));
        press(sim, mouse::Button::Left);
        move_to(sim, s.at("g2"));
        move_to(sim, s.at("f3"));
        release(sim, mouse::Button::Left);
    });
    assert_eq!(
        plays(&events),
        [
            (Some(sq("e2")), Some(sq("e4")), None, false),
            (Some(sq("g1")), Some(sq("f3")), None, true),
        ]
    );
}

#[test]
fn m2_3_widget_castles_by_clicking_the_own_rook() {
    let scene = Scene::new(CASTLE);
    let events = scene.events(|s, sim| {
        click(sim, s.at("e1"));
        click(sim, s.at("h1"));
    });
    let p = plays(&events);
    assert_eq!(p.len(), 1);
    assert_eq!((p[0].0, p[0].1), (Some(sq("e1")), Some(sq("g1"))));
}

#[test]
fn m2_3_widget_king_on_king_publishes_the_null_move() {
    let scene = Scene::new(KINGS);
    let events = scene.events(|s, sim| {
        click(sim, s.at("e1"));
        click(sim, s.at("e8"));
    });
    assert_eq!(events, [BoardEvent::PlayNullMove]);
}

#[test]
fn m2_3_widget_below_threshold_movement_is_a_click_not_a_drag() {
    let scene = Scene::new(START);
    let events = scene.events(|s, sim| {
        let o = s.at("e2");
        move_to(sim, o);
        press(sim, mouse::Button::Left);
        move_to(sim, Point::new(o.x + 3.0, o.y));
        release(sim, mouse::Button::Left);
        // Still selected: the next click plays the move as a click.
        click(sim, s.at("e4"));
    });
    assert_eq!(
        plays(&events),
        [(Some(sq("e2")), Some(sq("e4")), None, false)]
    );
}

#[test]
fn m2_3_widget_escape_during_a_drag_cancels_the_move() {
    let scene = Scene::new(START);
    let events = scene.events(|s, sim| {
        move_to(sim, s.at("e2"));
        press(sim, mouse::Button::Left);
        move_to(sim, s.at("e3"));
        tap(sim, Named::Escape);
        move_to(sim, s.at("e4"));
        release(sim, mouse::Button::Left);
    });
    assert!(plays(&events).is_empty(), "{events:?}");
}

#[test]
fn m2_3_widget_escape_and_enter_drive_the_promotion_column() {
    let scene = Scene::new(PROMO);
    let enter = scene.events(|s, sim| {
        click(sim, s.at("a7"));
        click(sim, s.at("a8"));
        tap(sim, Named::Enter);
    });
    assert_eq!(
        plays(&enter),
        [(Some(sq("a7")), Some(sq("a8")), Some(Role::Queen), false)]
    );
    let escape = scene.events(|s, sim| {
        click(sim, s.at("a7"));
        click(sim, s.at("a8"));
        tap(sim, Named::Escape);
        // The column is gone: the former knight cell is an ordinary empty square.
        click(sim, s.at("a5"));
    });
    assert!(plays(&escape).is_empty(), "{escape:?}");
}

#[test]
fn m2_3_widget_each_promotion_cell_publishes_its_piece() {
    let scene = Scene::new(PROMO);
    for (cell, role) in [
        ("a8", Role::Queen),
        ("a7", Role::Rook),
        ("a6", Role::Bishop),
        ("a5", Role::Knight),
    ] {
        let events = scene.events(|s, sim| {
            click(sim, s.at("a7"));
            click(sim, s.at("a8"));
            click(sim, s.at(cell));
        });
        assert_eq!(
            plays(&events),
            [(Some(sq("a7")), Some(sq("a8")), Some(role), false)],
            "{cell}"
        );
    }
}

#[rstest]
#[case(Modifiers::empty(), "green")]
#[case(Modifiers::SHIFT, "firebrick")]
#[case(Modifiers::COMMAND, "yellow2")]
#[case(Modifiers::ALT, "blue")]
fn m2_3_widget_right_drag_color_by_modifier(#[case] mods: Modifiers, #[case] color: &str) {
    let scene = Scene::new(START);
    let events = scene.events(|s, sim| {
        set_modifiers(sim, mods);
        move_to(sim, s.at("d4"));
        press(sim, mouse::Button::Right);
        move_to(sim, s.at("f6"));
        release(sim, mouse::Button::Right);
    });
    assert_eq!(
        marks_of(&events),
        [BoardMark::arrow(sq("d4"), sq("f6"), color)]
    );
}

#[test]
fn m2_3_widget_right_release_outside_the_board_marks_nothing() {
    let mut scene = Scene::new(START);
    scene.padding = 50.0;
    let events = scene.events(|s, sim| {
        move_to(sim, s.at("d4"));
        press(sim, mouse::Button::Right);
        move_to(sim, Point::new(10.0, 10.0));
        release(sim, mouse::Button::Right);
    });
    assert!(marks_of(&events).is_empty(), "{events:?}");
}

#[test]
fn m2_3_widget_shift_click_and_command_click_arrow() {
    let scene = Scene::new(START);
    let shift = scene.events(|s, sim| {
        set_modifiers(sim, Modifiers::SHIFT);
        click(sim, s.at("e4"));
    });
    assert_eq!(
        marks_of(&shift),
        [BoardMark::square(MarkKind::Circle, sq("e4"), "skyBlue")]
    );
    let arrow = scene.events(|s, sim| {
        set_modifiers(sim, Modifiers::COMMAND);
        click(sim, s.at("c3"));
        click(sim, s.at("f6"));
    });
    assert_eq!(
        marks_of(&arrow),
        [BoardMark::arrow(sq("c3"), sq("f6"), "skyBlue")]
    );
}

#[test]
fn m2_3_widget_releasing_the_modifiers_returns_to_normal_clicks() {
    let scene = Scene::new(START);
    let events = scene.events(|s, sim| {
        set_modifiers(sim, Modifiers::SHIFT);
        set_modifiers(sim, Modifiers::empty());
        click(sim, s.at("e2"));
        click(sim, s.at("e4"));
    });
    assert!(marks_of(&events).is_empty());
    assert_eq!(plays(&events).len(), 1);
}

#[test]
fn m2_3_widget_ignores_everything_when_it_does_not_accept_moves() {
    let mut scene = Scene::new(START);
    scene.accepts = false;
    let events = scene.events(|s, sim| {
        click(sim, s.at("e2"));
        click(sim, s.at("e4"));
        set_modifiers(sim, Modifiers::SHIFT);
        click(sim, s.at("d4"));
        set_modifiers(sim, Modifiers::empty());
        move_to(sim, s.at("d4"));
        press(sim, mouse::Button::Right);
        move_to(sim, s.at("f6"));
        release(sim, mouse::Button::Right);
        tap(sim, Named::Enter);
        tap(sim, Named::Escape);
    });
    assert!(events.is_empty(), "{events:?}");
}

#[test]
fn m2_3_widget_idle_cursor_movement_publishes_nothing() {
    let scene = Scene::new(START);
    let events = scene.events(|s, sim| {
        for name in ["a1", "e4", "h8"] {
            move_to(sim, s.at(name));
        }
        move_to(sim, Point::new(-1000.0, 5000.0));
    });
    assert!(events.is_empty());
}

#[test]
fn m2_3_widget_survives_non_finite_pointer_positions() {
    let scene = Scene::new(START);
    let events = scene.events(|s, sim| {
        for p in [
            Point::new(f32::NAN, f32::NAN),
            Point::new(f32::INFINITY, 3.0),
            Point::new(2.0, f32::NEG_INFINITY),
        ] {
            move_to(sim, p);
            press(sim, mouse::Button::Left);
            release(sim, mouse::Button::Left);
            press(sim, mouse::Button::Right);
            release(sim, mouse::Button::Right);
        }
        click(sim, s.at("e2"));
    });
    assert!(plays(&events).is_empty() && marks_of(&events).is_empty());
}

#[rstest]
#[case(Size::new(8.0, 8.0))]
#[case(Size::new(7.0, 7.0))]
#[case(Size::new(1.0, 1.0))]
#[case(Size::new(20.0, 500.0))]
#[case(Size::new(500.0, 20.0))]
#[case(Size::new(60.0, 60.0))]
fn m2_3_widget_tiny_canvases_do_not_panic(#[case] size: Size) {
    for coords in [CoordsMode::None, CoordsMode::Outside, CoordsMode::Inside] {
        let mut scene = Scene::new(START);
        scene.size = size;
        scene.options.coords = coords;
        let events = scene.with_simulator(|mut sim| {
            for x in [0.0, 1.0, size.width / 2.0, size.width - 0.5] {
                for y in [0.0, 1.0, size.height / 2.0, size.height - 0.5] {
                    let p = Point::new(x, y);
                    move_to(&mut sim, p);
                    press(&mut sim, mouse::Button::Left);
                    release(&mut sim, mouse::Button::Left);
                    press(&mut sim, mouse::Button::Right);
                    release(&mut sim, mouse::Button::Right);
                }
            }
            assert!(sim.snapshot(&Theme::Light).is_ok());
            sim.into_messages().collect::<Vec<_>>()
        });
        // No legal move fits an unplayable board: whatever was published
        // must still be well-formed.
        for e in events {
            if let BoardEvent::PlayMove { mv, .. } = e {
                assert!(Position::from_fen(START).expect("fen").is_legal(mv));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Render relations
// ---------------------------------------------------------------------------

fn unique_dir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "mychess-m2-3-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// The SHA-256 of the rendered pixels, read back from the file that
/// `Snapshot::matches_hash` writes (the pixels themselves are not exposed).
fn fingerprint(scene: &Scene) -> String {
    let dir = unique_dir();
    let path = dir.join("scene");
    let snapshot = scene.with_simulator(|mut sim| sim.snapshot(&Theme::Light).expect("snapshot"));
    assert!(snapshot.matches_hash(&path).expect("write hash"));
    let file = std::fs::read_dir(&dir)
        .expect("hash dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "sha256"))
        .expect("hash file");
    let hash = std::fs::read_to_string(file).expect("read hash");
    drop(std::fs::remove_dir_all(&dir));
    hash
}

/// Whether two scenes render to exactly the same pixels (compared within
/// one run, so renderer differences between machines do not matter).
fn same_pixels(a: &Scene, b: &Scene) -> bool {
    fingerprint(a) == fingerprint(b)
}

macro_rules! linux_only {
    () => {
        if !cfg!(target_os = "linux") {
            return;
        }
    };
}

fn quiet() -> BoardOptions {
    BoardOptions {
        highlight_last_move: false,
        ..BoardOptions::default()
    }
}

#[test]
fn m2_3_render_is_deterministic() {
    linux_only!();
    let mut s = Scene::new(START);
    s.marks = vec![
        BoardMark::arrow(sq("e2"), sq("e4"), "green"),
        BoardMark::square(MarkKind::Circle, sq("d4"), "skyBlue"),
        BoardMark::square(MarkKind::Text('!'), sq("c3"), "red"),
    ];
    assert!(same_pixels(&s, &s.clone()));
}

#[test]
fn m2_3_render_hidden_marks_equal_no_marks() {
    linux_only!();
    let marks = vec![
        BoardMark::arrow(sq("e2"), sq("e4"), "green"),
        BoardMark::square(MarkKind::Full, sq("d4"), "yellow2"),
        BoardMark::square(MarkKind::Disk, sq("a1"), "blue"),
    ];
    let bare = Scene::new(START);
    let mut hidden = bare.clone();
    hidden.marks = marks.clone();
    hidden.options.show_marks = false;
    let mut shown = bare.clone();
    shown.marks = marks;
    assert!(
        same_pixels(&bare, &hidden),
        "show_marks = false hides every mark"
    );
    assert!(
        !same_pixels(&bare, &shown),
        "marks must be visible when enabled"
    );
}

#[rstest]
#[case(MarkKind::Full)]
#[case(MarkKind::Circle)]
#[case(MarkKind::Disk)]
#[case(MarkKind::Square)]
#[case(MarkKind::Text('A'))]
#[case(MarkKind::Text('+'))]
fn m2_3_render_each_square_mark_kind_is_drawn(#[case] kind: MarkKind) {
    linux_only!();
    let bare = Scene::new(START);
    let mut marked = bare.clone();
    marked.marks = vec![BoardMark::square(kind, sq("d4"), "red")];
    assert!(!same_pixels(&bare, &marked), "{kind:?} is invisible");
}

#[test]
fn m2_3_render_tux_is_not_drawn() {
    linux_only!();
    let bare = Scene::new(START);
    let mut tux = bare.clone();
    tux.marks = vec![BoardMark::square(MarkKind::Tux, sq("d4"), "red")];
    assert!(same_pixels(&bare, &tux), "D165: tux is never drawn");
}

#[test]
fn m2_3_render_unknown_color_falls_back_to_firebrick() {
    linux_only!();
    let with = |color: &str| {
        let mut s = Scene::new(START);
        s.marks = vec![
            BoardMark::square(MarkKind::Circle, sq("d4"), color),
            BoardMark::arrow(sq("a1"), sq("h8"), color),
        ];
        s
    };
    assert!(same_pixels(&with("nonsense"), &with("firebrick")));
    assert!(same_pixels(&with("FireBrick"), &with("firebrick")));
    assert!(same_pixels(&with("#B22222"), &with("firebrick")));
    assert!(!same_pixels(&with("green"), &with("firebrick")));
}

#[test]
fn m2_3_render_equal_colors_by_different_names_are_pixel_identical() {
    linux_only!();
    let with = |color: &str| {
        let mut s = Scene::new(START);
        s.marks = vec![BoardMark::square(MarkKind::Full, sq("d4"), color)];
        s
    };
    assert!(same_pixels(&with("green"), &with("#00ff00")));
    assert!(same_pixels(&with("GREEN"), &with("#0f0")));
    assert!(same_pixels(&with("grey50"), &with("#808080")));
}

#[test]
fn m2_3_render_arrow_direction_and_color_matter() {
    linux_only!();
    let arrow = |from: &str, to: &str, color: &str| {
        let mut s = Scene::new(START);
        s.marks = vec![BoardMark::arrow(sq(from), sq(to), color)];
        s
    };
    assert!(!same_pixels(
        &arrow("d4", "f6", "green"),
        &arrow("f6", "d4", "green")
    ));
    assert!(!same_pixels(
        &arrow("d4", "f6", "green"),
        &arrow("d4", "f6", "blue")
    ));
}

#[test]
fn m2_3_render_last_move_highlight_on_off_and_null() {
    linux_only!();
    let mv = Position::from_fen(START)
        .expect("fen")
        .find_move(sq("e2"), sq("e4"), None)
        .expect("e2e4");
    let after = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1";
    let mut on = Scene::new(after);
    on.last_move = Some(mv);
    let off = Scene::new(after);
    let mut disabled = on.clone();
    disabled.options.highlight_last_move = false;
    let mut null = Scene::new(after);
    null.last_move = Some(Move::Null);
    assert!(!same_pixels(&on, &off), "highlight is visible");
    assert!(same_pixels(&disabled, &off), "highlight_last_move = false");
    assert!(
        same_pixels(&null, &off),
        "the null move has no squares to tint"
    );
}

#[test]
fn m2_3_render_all_themes_differ() {
    linux_only!();
    let prints: std::collections::HashSet<String> = BoardThemeId::ALL
        .iter()
        .map(|&theme| {
            let mut s = Scene::new(START);
            s.options.theme = theme;
            fingerprint(&s)
        })
        .collect();
    assert_eq!(
        prints.len(),
        BoardThemeId::ALL.len(),
        "two themes render alike"
    );
}

#[test]
fn m2_3_render_flipping_changes_the_picture() {
    linux_only!();
    let white = Scene::new(START);
    let mut black = white.clone();
    black.orientation = Orientation::Black;
    assert!(!same_pixels(&white, &black));
}

#[test]
fn m2_3_render_coordinate_modes_differ_from_each_other() {
    linux_only!();
    let prints: std::collections::HashSet<String> = [
        CoordsMode::None,
        CoordsMode::Outside,
        CoordsMode::OutsideBothSides,
        CoordsMode::Inside,
    ]
    .iter()
    .map(|&coords| {
        let mut s = Scene::new(START);
        s.options.coords = coords;
        fingerprint(&s)
    })
    .collect();
    assert_eq!(prints.len(), 4, "two coordinate modes render alike");
}

/// Animation scenes: `old` -> `new`, with the progress at which the sprite is drawn.
fn animated(old: &'static str, new: &'static str, t: f32) -> (Scene, Scene, Scene) {
    let old_pos = Position::from_fen(old).expect("fen");
    let new_pos = Position::from_fen(new).expect("fen");
    let anim = animation_between(&old_pos, &new_pos).expect("one move apart");
    let mut running = Scene::new(new);
    running.options = quiet();
    running.animation = Some((anim, t));
    let mut before = Scene::new(old);
    before.options = quiet();
    let mut after = Scene::new(new);
    after.options = quiet();
    (running, before, after)
}

#[rstest]
#[case(
    START,
    "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
    true
)]
#[case(CASTLE, "r3k2r/8/8/8/8/8/8/R4RK1 b kq - 1 1", true)]
// Captures and promotions: the victim is gone and the promoted piece (not the
// pawn) slides, from the first frame (rule 55), so the start differs.
#[case(
    "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
    "4k3/8/3P4/8/8/8/8/4K3 b - - 0 1",
    false
)]
#[case(
    "4k3/P7/8/8/8/8/8/4K3 w - - 0 1",
    "Q3k3/8/8/8/8/8/8/4K3 b - - 0 1",
    false
)]
#[case(
    "4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1",
    "4k3/8/8/3P4/8/8/8/4K3 b - - 0 1",
    false
)]
fn m2_3_render_animation_endpoints_match_the_still_positions(
    #[case] old: &'static str,
    #[case] new: &'static str,
    #[case] start_equals_old: bool,
) {
    linux_only!();
    let (at_end, before, after) = animated(old, new, 1.0);
    assert!(
        same_pixels(&at_end, &after),
        "progress 1.0 = the new position"
    );
    let (at_start, _, _) = animated(old, new, 0.0);
    assert_eq!(
        same_pixels(&at_start, &before),
        start_equals_old,
        "progress 0.0 vs the old position"
    );
    let (mid, _, _) = animated(old, new, 0.5);
    assert!(
        !same_pixels(&mid, &before) && !same_pixels(&mid, &after),
        "mid-flight differs"
    );
}

#[test]
fn m2_3_render_backward_animation_endpoints() {
    linux_only!();
    // new = the start position, old = after e4.
    let (at_end, _, after) = animated(
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
        START,
        1.0,
    );
    assert!(same_pixels(&at_end, &after));
}

#[test]
fn m2_3_render_does_not_panic_for_the_full_option_matrix() {
    linux_only!();
    let marks = vec![
        BoardMark::square(MarkKind::Full, sq("a1"), "nonsense"),
        BoardMark::square(MarkKind::Circle, sq("h8"), "#abc"),
        BoardMark::square(MarkKind::Disk, sq("d5"), "skyBlue"),
        BoardMark::square(MarkKind::Square, sq("e5"), "green"),
        BoardMark::square(MarkKind::Tux, sq("e6"), "green"),
        BoardMark::square(MarkKind::Text('Z'), sq("b2"), "blue"),
        BoardMark::arrow(sq("a1"), sq("h8"), "green"),
        BoardMark::arrow(sq("h8"), sq("a1"), ""),
        BoardMark::arrow(sq("e4"), sq("e5"), "grey50"),
    ];
    let base = Scene::new("r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1");
    let render = |s: &Scene| {
        s.with_simulator(|mut sim| {
            assert!(sim.snapshot(&Theme::Dark).is_ok());
        });
    };
    let mut marked = base.clone();
    marked.marks = marks;
    marked.size = Size::new(400.0, 300.0);
    for theme in BoardThemeId::ALL {
        let mut s = marked.clone();
        s.options.theme = theme;
        render(&s);
    }
    for coords in [
        CoordsMode::None,
        CoordsMode::Outside,
        CoordsMode::OutsideBothSides,
        CoordsMode::Inside,
    ] {
        for orientation in [Orientation::White, Orientation::Black] {
            let mut s = marked.clone();
            s.options.coords = coords;
            s.orientation = orientation;
            render(&s);
        }
    }
}

/// Renders after running `script` on a simulator over `scene`.
fn fingerprint_after(
    scene: &Scene,
    script: impl FnOnce(&Scene, &mut Simulator<'_, BoardEvent>),
) -> String {
    let dir = unique_dir();
    let path = dir.join("scene");
    let snapshot = scene.with_simulator(|mut sim| {
        script(scene, &mut sim);
        sim.snapshot(&Theme::Light).expect("snapshot")
    });
    assert!(snapshot.matches_hash(&path).expect("write hash"));
    let file = std::fs::read_dir(&dir)
        .expect("hash dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "sha256"))
        .expect("hash file");
    let hash = std::fs::read_to_string(file).expect("read hash");
    drop(std::fs::remove_dir_all(&dir));
    hash
}

#[test]
fn m2_3_render_selection_and_legal_hints() {
    linux_only!();
    let scene = Scene::new(START);
    let hover = fingerprint_after(&scene, |s, sim| move_to(sim, s.at("e2")));
    let selected = fingerprint_after(&scene, |s, sim| click(sim, s.at("e2")));
    assert_ne!(hover, selected, "selecting paints the square and the hints");

    let mut no_hints = scene.clone();
    no_hints.options.show_legal_hints = false;
    let selected_plain = fingerprint_after(&no_hints, |s, sim| click(sim, s.at("e2")));
    let hover_plain = fingerprint_after(&no_hints, |s, sim| move_to(sim, s.at("e2")));
    assert_ne!(selected_plain, selected, "dots and rings are switchable");
    assert_ne!(
        selected_plain, hover_plain,
        "the selected square keeps its fill without hints (rule 28)"
    );
}

#[test]
fn m2_3_render_escape_and_second_click_return_to_the_idle_picture() {
    linux_only!();
    let scene = Scene::new(START);
    let idle = fingerprint_after(&scene, |s, sim| move_to(sim, s.at("e2")));
    let esc = fingerprint_after(&scene, |s, sim| {
        click(sim, s.at("e2"));
        tap(sim, Named::Escape);
    });
    assert_eq!(idle, esc, "Escape clears the selection");
    let toggled = fingerprint_after(&scene, |s, sim| {
        click(sim, s.at("e2"));
        click(sim, s.at("e2"));
    });
    assert_eq!(idle, toggled, "the second click deselects");
}

#[test]
fn m2_3_render_selection_marks_only_own_pieces() {
    linux_only!();
    let scene = Scene::new(START);
    let hover = fingerprint_after(&scene, |s, sim| move_to(sim, s.at("e7")));
    let clicked = fingerprint_after(&scene, |s, sim| click(sim, s.at("e7")));
    assert_eq!(hover, clicked, "an enemy piece is not selectable");
    let empty = fingerprint_after(&scene, |s, sim| move_to(sim, s.at("e4")));
    let clicked = fingerprint_after(&scene, |s, sim| click(sim, s.at("e4")));
    assert_eq!(empty, clicked, "an empty square is not selectable");
}

#[test]
fn m2_3_render_dragging_shows_the_piece_under_the_pointer() {
    linux_only!();
    let scene = Scene::new(START);
    let selected = fingerprint_after(&scene, |s, sim| click(sim, s.at("g1")));
    let dragging = fingerprint_after(&scene, |s, sim| {
        move_to(sim, s.at("g1"));
        press(sim, mouse::Button::Left);
        move_to(sim, s.at("f3"));
    });
    assert_ne!(
        selected, dragging,
        "ghost on the origin and piece on the cursor"
    );
    let mid = fingerprint_after(&scene, |s, sim| {
        move_to(sim, s.at("g1"));
        press(sim, mouse::Button::Left);
        move_to(sim, s.at("g3"));
    });
    assert_ne!(mid, dragging, "the piece follows the pointer");
}

#[test]
fn m2_3_render_promotion_column_opens_and_escape_closes_it() {
    linux_only!();
    let scene = Scene::new(PROMO);
    let selected = fingerprint_after(&scene, |s, sim| click(sim, s.at("a7")));
    let open = fingerprint_after(&scene, |s, sim| {
        click(sim, s.at("a7"));
        click(sim, s.at("a8"));
    });
    assert_ne!(selected, open, "the column is drawn over the board");
    let closed = fingerprint_after(&scene, |s, sim| {
        click(sim, s.at("a7"));
        click(sim, s.at("a8"));
        tap(sim, Named::Escape);
        tap(sim, Named::Escape);
        move_to(sim, s.at("a7"));
    });
    let idle = fingerprint_after(&scene, |s, sim| move_to(sim, s.at("a7")));
    assert_eq!(closed, idle, "Escape twice returns to the idle picture");
}
