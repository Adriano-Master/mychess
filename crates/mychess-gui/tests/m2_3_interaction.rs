//! M2-3 mouse state machine by event scripts (rules 33-40, 47; "Estrategia de
//! teste" 2). Derived from the spec; only the public `Interaction` API is used.

#![allow(clippy::assert_is_empty, clippy::float_cmp, clippy::unnecessary_wraps)]

use iced::{Point, Size};
use mychess_core::{BoardMark, MarkKind, Move, Position, Role, Square};
use mychess_gui::widgets::board::geometry::{BoardLayout, CoordsMode, LayoutOptions, Orientation};
use mychess_gui::widgets::board::interaction::{
    BoardContext, BoardEvent, CursorKind, DRAG_THRESHOLD, Input, Interaction, Mods,
};
use mychess_gui::widgets::board::marks::MarkBrush;
use mychess_gui::widgets::board::moves::{DropResolution, legal_destinations, resolve_drop};
use mychess_gui::widgets::board::promotion::{promotion_cells, promotion_role_at};
use proptest::prelude::*;
use rstest::rstest;

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const PROMO: &str = "1n2k3/P7/8/8/8/8/8/4K3 w - - 0 1";
const CASTLE: &str = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
const KINGS: &str = "4k3/8/8/8/8/8/8/4K3 w - - 0 1";

fn sq(name: &str) -> Square {
    name.parse().expect("square")
}

struct Rig {
    layout: BoardLayout,
    position: Position,
    brush: MarkBrush,
    orientation: Orientation,
    accepts: bool,
    it: Interaction,
}

impl Rig {
    fn new(fen: &str) -> Rig {
        Rig {
            layout: BoardLayout::compute(
                Size::new(800.0, 800.0),
                LayoutOptions {
                    coords: CoordsMode::None,
                },
            )
            .expect("layout fits"),
            position: Position::from_fen(fen).expect("fen"),
            brush: MarkBrush::default(),
            orientation: Orientation::White,
            accepts: true,
            it: Interaction::default(),
        }
    }

    fn flipped(fen: &str) -> Rig {
        Rig {
            orientation: Orientation::Black,
            ..Rig::new(fen)
        }
    }

    fn at(&self, name: &str) -> Point {
        self.layout.square_center(sq(name), self.orientation)
    }

    fn send(&mut self, input: Input) -> Vec<BoardEvent> {
        let ctx = BoardContext {
            layout: self.layout,
            orientation: self.orientation,
            position: &self.position,
            accepts_moves: self.accepts,
            brush: &self.brush,
        };
        self.it.handle(input, &ctx)
    }

    fn press_at(&mut self, pos: Point, mods: Mods) -> Vec<BoardEvent> {
        self.send(Input::LeftPressed { pos, mods })
    }

    fn release_at(&mut self, pos: Point) -> Vec<BoardEvent> {
        self.send(Input::LeftReleased {
            pos,
            mods: Mods::default(),
        })
    }

    fn move_to(&mut self, pos: Point) -> Vec<BoardEvent> {
        self.send(Input::CursorMoved { pos })
    }

    fn press(&mut self, name: &str) -> Vec<BoardEvent> {
        let pos = self.at(name);
        self.press_at(pos, Mods::default())
    }

    fn release(&mut self, name: &str) -> Vec<BoardEvent> {
        let pos = self.at(name);
        self.release_at(pos)
    }

    fn go(&mut self, name: &str) -> Vec<BoardEvent> {
        let pos = self.at(name);
        self.move_to(pos)
    }

    fn click(&mut self, name: &str) -> Vec<BoardEvent> {
        let mut out = self.press(name);
        out.extend(self.release(name));
        out
    }

    fn click_with(&mut self, name: &str, mods: Mods) -> Vec<BoardEvent> {
        let pos = self.at(name);
        let mut out = self.press_at(pos, mods);
        out.extend(self.send(Input::LeftReleased { pos, mods }));
        out
    }

    /// Press on `from`, wander over `via`, release on `to`.
    fn drag(&mut self, from: &str, via: &[&str], to: &str) -> Vec<BoardEvent> {
        let mut out = self.press(from);
        for v in via {
            out.extend(self.go(v));
        }
        out.extend(self.go(to));
        out.extend(self.release(to));
        out
    }

    fn right_gesture(&mut self, from: &str, to: Option<&str>, mods: Mods) -> Vec<BoardEvent> {
        let start = self.at(from);
        let mut out = self.send(Input::RightPressed { pos: start, mods });
        let end = to.map_or(start, |t| self.at(t));
        out.extend(self.move_to(end));
        out.extend(self.send(Input::RightReleased { pos: end, mods }));
        out
    }
}

fn plays(events: &[BoardEvent]) -> Vec<(Move, bool)> {
    events
        .iter()
        .filter_map(|e| match e {
            BoardEvent::PlayMove { mv, dragged } => Some((*mv, *dragged)),
            _ => None,
        })
        .collect()
}

fn only_play(events: &[BoardEvent]) -> (Move, bool) {
    let p = plays(events);
    assert_eq!(p.len(), 1, "expected exactly one PlayMove in {events:?}");
    p[0]
}

fn marks(events: &[BoardEvent]) -> Vec<BoardMark> {
    events
        .iter()
        .filter_map(|e| match e {
            BoardEvent::ToggleMark(m) => Some(m.clone()),
            _ => None,
        })
        .collect()
}

fn ft(mv: Move) -> (Option<Square>, Option<Square>, Option<Role>) {
    (mv.from(), mv.to(), mv.promotion())
}

const SHIFT: Mods = Mods {
    shift: true,
    command: false,
    alt: false,
};
const COMMAND: Mods = Mods {
    shift: false,
    command: true,
    alt: false,
};
const ALT: Mods = Mods {
    shift: false,
    command: false,
    alt: true,
};

// ---------------------------------------------------------------------------
// Click-click (rule 34)
// ---------------------------------------------------------------------------

#[test]
fn m2_3_click_click_plays_and_resets() {
    let mut r = Rig::new(START);
    assert!(plays(&r.click("e2")).is_empty());
    assert_eq!(r.it.selected(), Some(sq("e2")));
    let (mv, dragged) = only_play(&r.click("e4"));
    assert_eq!(ft(mv), (Some(sq("e2")), Some(sq("e4")), None));
    assert!(!dragged, "click-click must be animated (not dragged)");
    assert!(r.it.is_idle());
}

#[test]
fn m2_3_click_on_empty_or_enemy_square_selects_nothing() {
    let mut r = Rig::new(START);
    r.click("e4");
    assert_eq!(r.it.selected(), None);
    r.click("e7");
    assert_eq!(r.it.selected(), None);
    assert!(r.it.is_idle());
}

#[test]
fn m2_3_click_outside_the_board_clears_the_selection() {
    let mut r = Rig::new(START);
    r.click("e2");
    let events = r.press_at(Point::new(-5.0, 400.0), Mods::default());
    assert!(plays(&events).is_empty());
    assert_eq!(r.it.selected(), None);
}

#[test]
fn m2_3_selected_then_click_on_empty_illegal_square_clears_selection() {
    let mut r = Rig::new(START);
    r.click("e2");
    assert!(plays(&r.click("e5")).is_empty());
    assert_eq!(r.it.selected(), None);
}

#[test]
fn m2_3_selected_then_click_on_enemy_non_capturable_piece_clears_selection() {
    let mut r = Rig::new(START);
    r.click("e2");
    assert!(plays(&r.click("e7")).is_empty());
    assert_eq!(r.it.selected(), None);
}

#[test]
fn m2_3_reselecting_another_own_piece_moves_the_selection() {
    let mut r = Rig::new(START);
    r.click("e2");
    r.click("d2");
    assert_eq!(r.it.selected(), Some(sq("d2")));
    let (mv, _) = only_play(&r.click("d4"));
    assert_eq!(ft(mv), (Some(sq("d2")), Some(sq("d4")), None));
}

#[test]
fn m2_3_clicking_the_selected_piece_again_deselects() {
    let mut r = Rig::new(START);
    r.click("e2");
    let events = r.click("e2");
    assert!(plays(&events).is_empty());
    assert_eq!(r.it.selected(), None);
}

#[test]
fn m2_3_destination_before_origin_is_not_accepted() {
    // D151: unlike the Scid, clicking the destination first does nothing.
    let mut r = Rig::new(START);
    r.click("e4");
    let events = r.click("e2");
    assert!(plays(&events).is_empty());
    assert_eq!(r.it.selected(), Some(sq("e2")));
}

#[test]
fn m2_3_capture_by_click_click() {
    let mut r = Rig::new("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1");
    r.click("e4");
    let (mv, _) = only_play(&r.click("d5"));
    assert!(mv.is_capture());
    assert_eq!(ft(mv), (Some(sq("e4")), Some(sq("d5")), None));
}

#[test]
fn m2_3_en_passant_by_click_click() {
    let mut r = Rig::new("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1");
    r.click("e5");
    let (mv, _) = only_play(&r.click("d6"));
    assert!(mv.is_en_passant());
}

#[test]
fn m2_3_pinned_piece_cannot_move() {
    // Knight e2 is pinned by the rook on e8.
    let fen = "4r1k1/8/8/8/8/8/4N3/4K3 w - - 0 1";
    let mut r = Rig::new(fen);
    assert!(legal_destinations(&r.position, sq("e2")).is_empty());
    r.click("e2");
    assert!(plays(&r.click("c3")).is_empty());
    assert!(plays(&r.drag("e2", &["e3"], "c3")).is_empty());
    assert!(r.it.is_idle());
}

#[test]
fn m2_3_black_to_move_cannot_pick_white_pieces() {
    let mut r = Rig::new("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1");
    r.click("e4");
    assert_eq!(r.it.selected(), None);
    r.click("e7");
    assert_eq!(r.it.selected(), Some(sq("e7")));
    let (mv, _) = only_play(&r.click("e5"));
    assert_eq!(ft(mv), (Some(sq("e7")), Some(sq("e5")), None));
}

// ---------------------------------------------------------------------------
// Drag (rules 35 and 36)
// ---------------------------------------------------------------------------

#[test]
fn m2_3_drag_to_a_legal_square_plays_a_dragged_move() {
    let mut r = Rig::new(START);
    let (mv, dragged) = only_play(&r.drag("g1", &["g2"], "f3"));
    assert_eq!(ft(mv), (Some(sq("g1")), Some(sq("f3")), None));
    assert!(dragged, "dragged moves are not animated (rule 56)");
    assert!(r.it.is_idle());
}

#[test]
fn m2_3_drag_threshold_is_strictly_greater_than_four_pixels() {
    assert_eq!(DRAG_THRESHOLD, 4.0);
    let mut r = Rig::new(START);
    let origin = r.at("e2");
    r.press("e2");
    r.move_to(Point::new(origin.x + 4.0, origin.y));
    assert!(r.it.drag().is_none(), "exactly 4 px is still a click");
    r.move_to(Point::new(origin.x + 4.01, origin.y));
    assert!(r.it.drag().is_some(), "more than 4 px starts the drag");
    assert_eq!(r.it.drag().map(|d| d.from), Some(sq("e2")));
}

#[test]
fn m2_3_threshold_is_euclidean_and_measured_from_the_press_point() {
    let mut r = Rig::new(START);
    let o = r.at("e2");
    r.press("e2");
    r.move_to(Point::new(o.x + 2.8, o.y + 2.8)); // 3.96 px
    assert!(r.it.drag().is_none());
    r.move_to(Point::new(o.x + 3.0, o.y + 3.0)); // 4.24 px
    assert!(r.it.drag().is_some());

    let mut r = Rig::new(START);
    r.press("e2");
    r.move_to(Point::new(o.x + 3.0, o.y));
    r.move_to(Point::new(o.x, o.y));
    r.move_to(Point::new(o.x - 3.0, o.y));
    assert!(r.it.drag().is_none(), "small wiggles never accumulate");
    r.move_to(Point::new(o.x, o.y + 4.5));
    assert!(r.it.drag().is_some());
}

#[test]
fn m2_3_movement_below_the_threshold_is_a_plain_click() {
    let mut r = Rig::new(START);
    let o = r.at("e2");
    r.press("e2");
    r.move_to(Point::new(o.x + 3.0, o.y));
    let events = r.release_at(Point::new(o.x + 3.0, o.y));
    assert!(plays(&events).is_empty());
    assert!(r.it.drag().is_none());
    assert_eq!(
        r.it.selected(),
        Some(sq("e2")),
        "still selected after a click"
    );
}

#[test]
fn m2_3_small_movement_across_a_square_edge_does_not_play_a_move() {
    // Press near the e2/e3 border and release 3 px away, on e3: a click, not
    // a drag, so no move is played.
    let mut r = Rig::new(START);
    let c = r.at("e2");
    let press = Point::new(c.x, c.y - 49.0); // top edge of e2
    r.press_at(press, Mods::default());
    r.move_to(Point::new(c.x, c.y - 52.0));
    let events = r.release_at(Point::new(c.x, c.y - 52.0)); // inside e3
    assert!(plays(&events).is_empty());
}

#[test]
fn m2_3_dropping_outside_the_board_cancels_and_clears() {
    for off in [
        Point::new(-20.0, 400.0),
        Point::new(900.0, 400.0),
        Point::new(400.0, 801.0),
        Point::new(400.0, -3.0),
        Point::new(800.0, 800.0),
    ] {
        let mut r = Rig::new(START);
        r.press("e2");
        r.go("e3");
        r.move_to(off);
        assert!(r.it.drag().is_some(), "drag follows the pointer outside");
        let events = r.release_at(off);
        assert!(plays(&events).is_empty(), "{off:?}");
        assert!(r.it.is_idle(), "{off:?}");
    }
}

#[test]
fn m2_3_drag_may_leave_the_canvas_and_come_back() {
    let mut r = Rig::new(START);
    r.press("e2");
    r.go("e3");
    r.move_to(Point::new(-300.0, -300.0));
    assert!(r.it.drag().is_some());
    r.go("e4");
    let (mv, dragged) = only_play(&r.release("e4"));
    assert_eq!(ft(mv), (Some(sq("e2")), Some(sq("e4")), None));
    assert!(dragged);
}

#[test]
fn m2_3_dropping_on_the_origin_keeps_the_piece_selected() {
    let mut r = Rig::new(START);
    let events = r.drag("e2", &["e3"], "e2");
    assert!(plays(&events).is_empty());
    assert_eq!(r.it.selected(), Some(sq("e2")));
    assert!(r.it.drag().is_none());
}

#[test]
fn m2_3_dropping_on_an_illegal_square_cancels_and_clears() {
    let mut r = Rig::new(START);
    let events = r.drag("e2", &["e3"], "e5");
    assert!(plays(&events).is_empty());
    assert!(r.it.is_idle());
}

#[test]
fn m2_3_dragging_over_other_squares_does_not_play_until_release() {
    let mut r = Rig::new(START);
    let mut all = r.press("b1");
    for name in ["b2", "c3", "d4", "c3"] {
        all.extend(r.go(name));
    }
    assert!(plays(&all).is_empty());
    let (mv, _) = only_play(&r.release("c3"));
    assert_eq!(ft(mv), (Some(sq("b1")), Some(sq("c3")), None));
}

#[test]
fn m2_3_release_without_a_press_does_nothing() {
    let mut r = Rig::new(START);
    assert!(r.release("e4").is_empty());
    assert!(r.it.is_idle());
}

#[test]
fn m2_3_press_on_an_enemy_piece_never_starts_a_drag() {
    let mut r = Rig::new(START);
    r.press("e7");
    r.go("e6");
    r.go("e5");
    assert!(r.it.drag().is_none());
    assert!(plays(&r.release("e5")).is_empty());
}

#[test]
fn m2_3_second_click_after_a_drag_of_the_selected_piece_deselects_only_on_click() {
    // Drag from the origin back to the origin keeps selection; a later plain
    // click on the same piece is then the "second click" and deselects.
    let mut r = Rig::new(START);
    r.drag("e2", &["e3"], "e2");
    assert_eq!(r.it.selected(), Some(sq("e2")));
    r.click("e2");
    assert_eq!(r.it.selected(), None);
}

#[test]
fn m2_3_flipped_board_click_and_drag() {
    let mut r = Rig::flipped(START);
    r.click("e2");
    let (mv, d1) = only_play(&r.click("e4"));
    assert_eq!(ft(mv), (Some(sq("e2")), Some(sq("e4")), None));
    assert!(!d1);
    let (mv, d2) = only_play(&r.drag("g1", &["g2"], "f3"));
    assert_eq!(ft(mv), (Some(sq("g1")), Some(sq("f3")), None));
    assert!(d2);
}

#[test]
fn m2_3_hit_test_uses_the_orientation_in_the_context() {
    // Same pixel, two orientations: the piece under (50, 750) is a1's rook
    // for White at the bottom and h8's rook for Black at the bottom.
    let mut w = Rig::new(START);
    w.press_at(Point::new(50.0, 750.0), Mods::default());
    assert_eq!(w.it.selected(), Some(sq("a1")));
    let mut b = Rig::flipped("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1");
    b.press_at(Point::new(50.0, 750.0), Mods::default());
    assert_eq!(b.it.selected(), Some(sq("h8")));
}

// ---------------------------------------------------------------------------
// Promotion (rule 38)
// ---------------------------------------------------------------------------

#[rstest]
#[case("a8", Role::Queen)]
#[case("a7", Role::Rook)]
#[case("a6", Role::Bishop)]
#[case("a5", Role::Knight)]
fn m2_3_promotion_column_picks_each_piece_by_click_click(#[case] cell: &str, #[case] role: Role) {
    let mut r = Rig::new(PROMO);
    r.click("a7");
    let opened = r.click("a8");
    assert!(plays(&opened).is_empty(), "no move before the choice");
    assert!(r.it.pending_promotion().is_some());
    let (mv, dragged) = only_play(&r.click(cell));
    assert_eq!(ft(mv), (Some(sq("a7")), Some(sq("a8")), Some(role)));
    assert!(!dragged);
    assert!(r.it.pending_promotion().is_none());
}

#[rstest]
#[case("a8", Role::Queen)]
#[case("a7", Role::Rook)]
#[case("a6", Role::Bishop)]
#[case("a5", Role::Knight)]
fn m2_3_promotion_by_dragging_marks_the_move_as_dragged(#[case] cell: &str, #[case] role: Role) {
    let mut r = Rig::new(PROMO);
    assert!(plays(&r.drag("a7", &[], "a8")).is_empty());
    let (mv, dragged) = only_play(&r.click(cell));
    assert_eq!(mv.promotion(), Some(role));
    assert!(dragged);
}

#[test]
fn m2_3_promotion_with_capture_offers_the_four_pieces() {
    let mut r = Rig::new(PROMO);
    r.click("a7");
    r.click("b8");
    let pending = r.it.pending_promotion().expect("pending promotion");
    assert_eq!(pending.candidates.len(), 4);
    assert!(pending.candidates.iter().all(|m| m.is_capture()));
    let (mv, _) = only_play(&r.click("b6"));
    assert_eq!(ft(mv), (Some(sq("a7")), Some(sq("b8")), Some(Role::Bishop)));
}

#[test]
fn m2_3_promotion_escape_cancels_without_a_move() {
    let mut r = Rig::new(PROMO);
    r.click("a7");
    r.click("a8");
    let events = r.send(Input::Escape);
    assert!(plays(&events).is_empty());
    assert!(r.it.pending_promotion().is_none());
    // The pawn can still be played afterwards.
    r.send(Input::Escape);
    assert!(r.it.is_idle());
    r.click("a7");
    r.click("a8");
    assert!(r.it.pending_promotion().is_some());
}

#[test]
fn m2_3_promotion_enter_picks_the_queen() {
    let mut r = Rig::new(PROMO);
    r.click("a7");
    r.click("a8");
    let (mv, _) = only_play(&r.send(Input::Enter));
    assert_eq!(mv.promotion(), Some(Role::Queen));
    assert!(r.it.is_idle());
}

#[test]
fn m2_3_enter_without_pending_promotion_does_nothing() {
    let mut r = Rig::new(PROMO);
    r.click("a7");
    assert!(r.send(Input::Enter).is_empty());
    assert_eq!(r.it.selected(), Some(sq("a7")));
}

#[rstest]
#[case("h1")]
#[case("e4")]
#[case("e1")]
#[case("b8")]
fn m2_3_click_outside_the_column_cancels_and_is_consumed(#[case] target: &str) {
    let mut r = Rig::new(PROMO);
    r.click("a7");
    r.click("a8");
    let events = r.click(target);
    assert!(plays(&events).is_empty(), "{target}");
    assert!(r.it.pending_promotion().is_none());
    assert_eq!(r.it.selected(), None, "the click was consumed ({target})");
}

#[test]
fn m2_3_click_outside_the_whole_board_also_cancels_the_promotion() {
    let mut r = Rig::new(PROMO);
    r.click("a7");
    r.click("a8");
    r.press_at(Point::new(-10.0, -10.0), Mods::default());
    assert!(r.it.pending_promotion().is_none());
}

#[test]
fn m2_3_position_change_cancels_the_promotion() {
    let mut r = Rig::new(PROMO);
    r.click("a7");
    r.click("a8");
    r.send(Input::PositionChanged);
    assert!(r.it.is_idle());
    assert!(r.it.pending_promotion().is_none());
}

#[test]
fn m2_3_black_promotion_in_a_bottom_row_column_grows_upwards() {
    let mut r = Rig::new("4k3/8/8/8/8/8/p7/4K3 b - - 0 1");
    r.click("a2");
    r.click("a1");
    let expect = [
        ("a1", Role::Queen),
        ("a2", Role::Rook),
        ("a3", Role::Bishop),
        ("a4", Role::Knight),
    ];
    for (cell, role) in expect {
        let cells = promotion_cells(&r.layout, sq("a1"), Orientation::White);
        let center = r.at(cell);
        assert_eq!(promotion_role_at(&cells, center), Some(role), "{cell}");
    }
    let (mv, _) = only_play(&r.click("a4"));
    assert_eq!(mv.promotion(), Some(Role::Knight));
}

#[test]
fn m2_3_promotion_works_with_the_board_flipped() {
    let mut r = Rig::flipped(PROMO);
    r.click("a7");
    r.click("a8");
    // With Black at the bottom a8 is in the bottom display row: the column
    // grows upwards, i.e. towards display row 4 (rank 5).
    let (mv, _) = only_play(&r.click("a5"));
    assert_eq!(mv.promotion(), Some(Role::Knight));
}

#[test]
fn m2_3_pending_promotion_blocks_other_gestures() {
    let mut r = Rig::new(PROMO);
    r.click("a7");
    r.click("a8");
    // Right button and shift click are ignored while the column is open.
    assert!(
        r.send(Input::RightPressed {
            pos: r.at("e4"),
            mods: Mods::default()
        })
        .is_empty()
    );
    assert!(r.it.mark_drag().is_none());
}

#[rstest]
#[case(Orientation::White, "a8", true)]
#[case(Orientation::White, "h8", true)]
#[case(Orientation::White, "a1", false)]
#[case(Orientation::White, "h1", false)]
#[case(Orientation::Black, "a8", false)]
#[case(Orientation::Black, "h8", false)]
#[case(Orientation::Black, "a1", true)]
#[case(Orientation::Black, "h1", true)]
fn m2_3_promotion_cells_are_inside_and_ordered(
    #[case] o: Orientation,
    #[case] to: &str,
    #[case] grows_down: bool,
) {
    let layout = Rig::new(START).layout;
    let cells = promotion_cells(&layout, sq(to), o);
    let roles: Vec<Role> = cells.iter().map(|c| c.role).collect();
    assert_eq!(roles, [Role::Queen, Role::Rook, Role::Bishop, Role::Knight]);
    assert_eq!(
        cells[0].rect,
        layout.square_rect(sq(to), o),
        "queen on the square"
    );
    for (i, c) in cells.iter().enumerate() {
        assert_eq!(c.rect.x, cells[0].rect.x, "same column");
        assert_eq!(c.rect.width, layout.s);
        assert!(c.rect.y >= layout.board.y);
        assert!(c.rect.y + c.rect.height <= layout.board.y + layout.board.height);
        #[allow(clippy::cast_precision_loss)]
        let step = i as f32 * layout.s;
        let expected = if grows_down {
            cells[0].rect.y + step
        } else {
            cells[0].rect.y - step
        };
        assert_eq!(c.rect.y, expected);
    }
}

#[test]
fn m2_3_resolve_drop_promotion_has_exactly_the_four_candidates() {
    let pos = Position::from_fen(PROMO).expect("fen");
    for to in ["a8", "b8"] {
        match resolve_drop(&pos, sq("a7"), sq(to)) {
            DropResolution::NeedsPromotion { candidates } => {
                let mut roles: Vec<Role> =
                    candidates.iter().filter_map(|m| m.promotion()).collect();
                roles.sort_by_key(|r| *r as u8);
                assert_eq!(roles, [Role::Knight, Role::Bishop, Role::Rook, Role::Queen]);
            }
            other => panic!("expected a promotion, got {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Castling (rule 36)
// ---------------------------------------------------------------------------

#[rstest]
#[case("g1", "h1")]
#[case("c1", "a1")]
fn m2_3_castling_by_king_destination_or_by_own_rook(#[case] king_to: &str, #[case] rook: &str) {
    let mut by_king = Rig::new(CASTLE);
    by_king.click("e1");
    let (a, _) = only_play(&by_king.click(king_to));
    let mut by_rook = Rig::new(CASTLE);
    by_rook.click("e1");
    let (b, _) = only_play(&by_rook.click(rook));
    assert!(a.is_castle() && b.is_castle());
    assert_eq!(a, b, "both gestures produce the same castling move");
    assert_eq!(a.from(), Some(sq("e1")));
    assert_eq!(a.to(), Some(sq(king_to)));
}

#[rstest]
#[case("g1", "h1")]
#[case("c1", "a1")]
fn m2_3_castling_by_dragging_the_king(#[case] king_to: &str, #[case] rook: &str) {
    let mut by_king = Rig::new(CASTLE);
    let (a, da) = only_play(&by_king.drag("e1", &["f1"], king_to));
    let mut by_rook = Rig::new(CASTLE);
    let (b, db) = only_play(&by_rook.drag("e1", &["f1"], rook));
    assert!(a.is_castle() && a == b && da && db);
}

#[test]
fn m2_3_castling_destinations_show_the_king_final_square() {
    let pos = Position::from_fen(CASTLE).expect("fen");
    let dest: Vec<Square> = legal_destinations(&pos, sq("e1"))
        .into_iter()
        .map(|d| d.square)
        .collect();
    assert!(dest.contains(&sq("g1")) && dest.contains(&sq("c1")));
    assert!(!dest.contains(&sq("h1")) && !dest.contains(&sq("a1")));
}

#[test]
fn m2_3_castling_through_an_attacked_square_is_not_offered() {
    // The black rook on f8 attacks f1: O-O is illegal, O-O-O is fine.
    let mut r = Rig::new("r3kr2/8/8/8/8/8/8/R3K2R w KQ - 0 1");
    r.click("e1");
    assert!(plays(&r.click("g1")).is_empty());
    assert_eq!(r.it.selected(), None);
    r.click("e1");
    let (mv, _) = only_play(&r.click("c1"));
    assert_eq!(
        mv.castling_side(),
        Some(mychess_core::CastlingSide::QueenSide)
    );
}

#[test]
fn m2_3_chess960_king_takes_own_rook() {
    // King g1, queen-side rook f1 ("F"): castling puts the king on c1.
    let mut r = Rig::new("4k3/8/8/8/8/8/8/5RK1 w F - 0 1");
    r.click("g1");
    let (mv, _) = only_play(&r.click("f1"));
    assert!(mv.is_castle());
    assert_eq!(mv.to(), Some(sq("c1")));

    let mut r = Rig::new("4k3/8/8/8/8/8/8/5RK1 w F - 0 1");
    let (mv, d) = only_play(&r.drag("g1", &["f1"], "f1"));
    assert!(mv.is_castle() && d);
}

#[test]
fn m2_3_chess960_king_that_does_not_move_castles_by_the_rook() {
    // King g1 and rook h1: the king stays on g1 and the rook goes to f1.
    let mut r = Rig::new("4k3/8/8/8/8/8/8/6KR w H - 0 1");
    r.click("g1");
    let (mv, _) = only_play(&r.click("h1"));
    assert!(mv.is_castle());
}

// ---------------------------------------------------------------------------
// King on king: null move (rule 37)
// ---------------------------------------------------------------------------

#[test]
fn m2_3_king_on_the_other_king_asks_for_the_null_move() {
    let mut r = Rig::new(KINGS);
    r.click("e1");
    let events = r.click("e8");
    assert_eq!(
        events
            .iter()
            .filter(|e| **e == BoardEvent::PlayNullMove)
            .count(),
        1
    );
    assert!(plays(&events).is_empty());
    assert!(r.it.is_idle());
}

#[test]
fn m2_3_dragging_the_king_on_the_other_king_asks_for_the_null_move() {
    let mut r = Rig::new(KINGS);
    let events = r.drag("e1", &["e2"], "e8");
    assert!(events.contains(&BoardEvent::PlayNullMove));
    let mut r = Rig::new("4k3/8/8/8/8/8/8/4K3 b - - 0 1");
    let events = r.drag("e8", &["e7"], "e1");
    assert!(events.contains(&BoardEvent::PlayNullMove), "also for Black");
}

#[test]
fn m2_3_only_a_king_on_a_king_is_a_null_move() {
    let pos = Position::from_fen("4k3/8/8/8/8/8/R7/4K3 w - - 0 1").expect("fen");
    assert_eq!(
        resolve_drop(&pos, sq("a2"), sq("e8")),
        DropResolution::Illegal
    );
    assert_eq!(
        resolve_drop(&pos, sq("e1"), sq("e1")),
        DropResolution::Illegal
    );
    assert_eq!(
        resolve_drop(&pos, sq("e1"), sq("e8")),
        DropResolution::NullMove
    );
}

#[test]
fn m2_3_null_move_is_proposed_even_when_in_check() {
    // The app refuses the null move in check (Scid main.tcl:1318-1330); the
    // GUI still proposes it.
    let pos = Position::from_fen("4k3/8/8/8/8/8/4r3/4K3 w - - 0 1").expect("fen");
    assert!(pos.is_check());
    assert_eq!(
        resolve_drop(&pos, sq("e1"), sq("e8")),
        DropResolution::NullMove
    );
}

// ---------------------------------------------------------------------------
// Invalidation and accepts_moves (rules 39 and 40)
// ---------------------------------------------------------------------------

#[test]
fn m2_3_position_or_orientation_change_cancels_everything() {
    for change in [Input::PositionChanged, Input::OrientationChanged] {
        let mut r = Rig::new(START);
        r.press("e2");
        r.go("e4");
        assert!(r.it.drag().is_some());
        r.send(change);
        assert!(r.it.is_idle(), "{change:?}");
        assert!(plays(&r.release("e4")).is_empty(), "{change:?}");

        let mut r = Rig::new(START);
        r.click("e2");
        r.send(change);
        assert_eq!(r.it.selected(), None, "{change:?}");

        let mut r = Rig::new(START);
        let s = r.at("d4");
        r.send(Input::RightPressed {
            pos: s,
            mods: Mods::default(),
        });
        assert!(r.it.mark_drag().is_some());
        r.send(change);
        assert!(r.it.mark_drag().is_none(), "{change:?}");
        let ev = r.send(Input::RightReleased {
            pos: s,
            mods: Mods::default(),
        });
        assert!(marks(&ev).is_empty(), "no mark after invalidation");

        let mut r = Rig::new(START);
        r.send(Input::LeftPressed {
            pos: r.at("c3"),
            mods: COMMAND,
        });
        assert!(r.it.arrow_start().is_some());
        r.send(change);
        assert_eq!(r.it.arrow_start(), None, "{change:?}");
    }
}

#[test]
fn m2_3_invalidating_an_idle_board_requests_nothing() {
    let mut r = Rig::new(START);
    assert!(r.send(Input::PositionChanged).is_empty());
    assert!(r.send(Input::OrientationChanged).is_empty());
}

#[test]
fn m2_3_nothing_is_published_when_moves_are_not_accepted() {
    let mut r = Rig::new(START);
    r.accepts = false;
    let mut all = Vec::new();
    all.extend(r.click("e2"));
    all.extend(r.click("e4"));
    all.extend(r.drag("g1", &["g2"], "f3"));
    all.extend(r.right_gesture("d4", Some("f6"), Mods::default()));
    all.extend(r.right_gesture("d4", None, Mods::default()));
    all.extend(r.click_with("e4", SHIFT));
    all.extend(r.click_with("e4", COMMAND));
    all.extend(r.click_with("e5", COMMAND));
    all.extend(r.send(Input::Enter));
    assert!(plays(&all).is_empty() && marks(&all).is_empty(), "{all:?}");
    assert!(!all.contains(&BoardEvent::PlayNullMove));
    assert!(r.it.is_idle());
}

#[test]
fn m2_3_turning_off_accepts_moves_mid_gesture_stops_the_gesture() {
    let mut r = Rig::new(START);
    r.press("e2");
    r.go("e4");
    r.accepts = false;
    r.go("e4");
    let events = r.release("e4");
    assert!(plays(&events).is_empty());
    assert!(r.it.is_idle());
}

// ---------------------------------------------------------------------------
// Escape (rule 40)
// ---------------------------------------------------------------------------

#[test]
fn m2_3_escape_while_idle_is_silent() {
    let mut r = Rig::new(START);
    assert!(r.send(Input::Escape).is_empty());
}

#[test]
fn m2_3_escape_cancels_a_drag_and_the_later_release_plays_nothing() {
    let mut r = Rig::new(START);
    r.press("e2");
    r.go("e3");
    assert!(r.it.drag().is_some());
    r.send(Input::Escape);
    assert!(r.it.drag().is_none());
    assert!(plays(&r.release("e4")).is_empty());
}

#[test]
fn m2_3_escape_cancels_the_selection() {
    let mut r = Rig::new(START);
    r.click("e2");
    r.send(Input::Escape);
    assert_eq!(r.it.selected(), None);
    // And a following click on a destination is not a move.
    assert!(plays(&r.click("e4")).is_empty());
}

#[test]
fn m2_3_escape_peels_one_layer_at_a_time_in_the_spec_order() {
    // promotion first ...
    let mut r = Rig::new(PROMO);
    r.click("a7");
    r.click("a8");
    r.send(Input::Escape);
    assert!(r.it.pending_promotion().is_none());
    // ... eventually everything is gone after at most two more presses.
    r.send(Input::Escape);
    r.send(Input::Escape);
    assert!(r.it.is_idle());

    // mark gesture before selection
    let mut r = Rig::new(START);
    r.click("e2");
    let s = r.at("d4");
    r.send(Input::RightPressed {
        pos: s,
        mods: Mods::default(),
    });
    r.send(Input::Escape);
    assert!(r.it.mark_drag().is_none());
    assert_eq!(
        r.it.selected(),
        Some(sq("e2")),
        "selection survives the first Esc"
    );
    r.send(Input::Escape);
    assert_eq!(r.it.selected(), None);
}

#[test]
fn m2_3_escape_cancels_a_pending_two_click_arrow() {
    let mut r = Rig::new(START);
    r.click_with("c3", COMMAND);
    assert_eq!(r.it.arrow_start(), Some(sq("c3")));
    r.send(Input::Escape);
    assert_eq!(r.it.arrow_start(), None);
}

// ---------------------------------------------------------------------------
// Marks (rule 47)
// ---------------------------------------------------------------------------

#[test]
fn m2_3_right_click_on_a_square_toggles_a_circle() {
    let mut r = Rig::new(START);
    let m = marks(&r.right_gesture("d4", None, Mods::default()));
    assert_eq!(m, [BoardMark::square(MarkKind::Circle, sq("d4"), "green")]);
}

#[rstest]
#[case(Mods::default(), "green")]
#[case(SHIFT, "firebrick")]
#[case(COMMAND, "yellow2")]
#[case(ALT, "blue")]
#[case(Mods { shift: true, command: true, alt: true }, "firebrick")]
#[case(Mods { shift: true, command: true, alt: false }, "firebrick")]
#[case(Mods { shift: true, command: false, alt: true }, "firebrick")]
#[case(Mods { shift: false, command: true, alt: true }, "yellow2")]
fn m2_3_right_drag_color_follows_the_modifiers(#[case] mods: Mods, #[case] color: &str) {
    for o in [Orientation::White, Orientation::Black] {
        let mut r = Rig::new(START);
        r.orientation = o;
        let arrow = marks(&r.right_gesture("d4", Some("f6"), mods));
        assert_eq!(
            arrow,
            [BoardMark::arrow(sq("d4"), sq("f6"), color)],
            "{o:?}"
        );
        let circle = marks(&r.right_gesture("d4", None, mods));
        assert_eq!(
            circle,
            [BoardMark::square(MarkKind::Circle, sq("d4"), color)],
            "{o:?}"
        );
    }
}

#[test]
fn m2_3_color_is_decided_when_the_button_is_pressed() {
    // Modifiers at release time are not used (the spec ties the color to the
    // gesture); pressing with Shift and releasing without keeps firebrick.
    let mut r = Rig::new(START);
    let a = r.at("d4");
    let b = r.at("f6");
    r.send(Input::RightPressed {
        pos: a,
        mods: SHIFT,
    });
    r.move_to(b);
    let ev = r.send(Input::RightReleased {
        pos: b,
        mods: Mods::default(),
    });
    assert_eq!(
        marks(&ev),
        [BoardMark::arrow(sq("d4"), sq("f6"), "firebrick")]
    );
}

#[test]
fn m2_3_right_release_outside_the_board_marks_nothing() {
    let mut r = Rig::new(START);
    let a = r.at("d4");
    r.send(Input::RightPressed {
        pos: a,
        mods: Mods::default(),
    });
    let ev = r.send(Input::RightReleased {
        pos: Point::new(900.0, 900.0),
        mods: Mods::default(),
    });
    assert!(marks(&ev).is_empty());
    assert!(r.it.mark_drag().is_none());
}

#[test]
fn m2_3_right_press_outside_the_board_starts_nothing() {
    let mut r = Rig::new(START);
    let ev = r.send(Input::RightPressed {
        pos: Point::new(-1.0, -1.0),
        mods: Mods::default(),
    });
    assert!(ev.is_empty() && r.it.mark_drag().is_none());
    // A stray release without a press marks nothing.
    let ev = r.send(Input::RightReleased {
        pos: r.at("e4"),
        mods: Mods::default(),
    });
    assert!(marks(&ev).is_empty());
}

#[test]
fn m2_3_escape_during_the_right_gesture_marks_nothing() {
    let mut r = Rig::new(START);
    let a = r.at("d4");
    r.send(Input::RightPressed {
        pos: a,
        mods: Mods::default(),
    });
    r.go("f6");
    r.send(Input::Escape);
    let ev = r.send(Input::RightReleased {
        pos: r.at("f6"),
        mods: Mods::default(),
    });
    assert!(marks(&ev).is_empty());
}

#[test]
fn m2_3_right_drag_tracks_the_square_under_the_pointer() {
    let mut r = Rig::new(START);
    let a = r.at("d4");
    r.send(Input::RightPressed {
        pos: a,
        mods: Mods::default(),
    });
    r.go("e5");
    assert_eq!(r.it.mark_drag().and_then(|g| g.current), Some(sq("e5")));
    r.move_to(Point::new(-9.0, -9.0));
    assert_eq!(r.it.mark_drag().and_then(|g| g.current), None);
    r.go("f6");
    let ev = r.send(Input::RightReleased {
        pos: r.at("f6"),
        mods: Mods::default(),
    });
    assert_eq!(marks(&ev), [BoardMark::arrow(sq("d4"), sq("f6"), "green")]);
}

#[test]
fn m2_3_right_gesture_works_while_a_piece_is_selected_without_changing_it() {
    let mut r = Rig::new(START);
    r.click("e2");
    let m = marks(&r.right_gesture("d4", Some("f6"), Mods::default()));
    assert_eq!(m.len(), 1);
    assert_eq!(r.it.selected(), Some(sq("e2")));
}

#[test]
fn m2_3_piece_drag_has_priority_over_right_button() {
    let mut r = Rig::new(START);
    r.press("e2");
    r.go("e4");
    let ev = r.send(Input::RightPressed {
        pos: r.at("d4"),
        mods: Mods::default(),
    });
    assert!(ev.is_empty());
    assert!(r.it.mark_drag().is_none());
    assert!(r.it.drag().is_some());
}

#[test]
fn m2_3_left_press_during_the_right_gesture_is_ignored() {
    let mut r = Rig::new(START);
    r.send(Input::RightPressed {
        pos: r.at("d4"),
        mods: Mods::default(),
    });
    assert!(r.press("e2").is_empty());
    assert_eq!(r.it.selected(), None);
}

#[test]
fn m2_3_shift_click_paints_the_brush_without_selecting() {
    let mut r = Rig::new(START);
    let m = marks(&r.click_with("e2", SHIFT));
    assert_eq!(
        m,
        [BoardMark::square(MarkKind::Circle, sq("e2"), "skyBlue")]
    );
    assert_eq!(r.it.selected(), None);
    assert!(r.it.is_idle());

    r.brush = MarkBrush {
        kind: MarkKind::Text('!'),
        color: "#ff0000".to_owned(),
    };
    let m = marks(&r.click_with("h5", SHIFT));
    assert_eq!(
        m,
        [BoardMark::square(MarkKind::Text('!'), sq("h5"), "#ff0000")]
    );
}

#[test]
fn m2_3_shift_click_outside_the_board_marks_nothing() {
    let mut r = Rig::new(START);
    let ev = r.press_at(Point::new(-1.0, 400.0), SHIFT);
    assert!(marks(&ev).is_empty());
}

#[test]
fn m2_3_shift_click_does_not_disturb_a_selection() {
    let mut r = Rig::new(START);
    r.click("e2");
    r.click_with("d4", SHIFT);
    assert_eq!(r.it.selected(), Some(sq("e2")));
}

#[test]
fn m2_3_command_click_twice_draws_an_arrow_with_the_brush_color() {
    let mut r = Rig::new(START);
    assert!(marks(&r.click_with("c3", COMMAND)).is_empty());
    assert_eq!(r.it.arrow_start(), Some(sq("c3")));
    // press+release of the second click: only one ToggleMark.
    let m = marks(&r.click_with("f6", COMMAND));
    assert_eq!(m, [BoardMark::arrow(sq("c3"), sq("f6"), "skyBlue")]);
    assert_eq!(r.it.arrow_start(), None);
}

#[test]
fn m2_3_command_click_on_the_same_square_cancels_the_arrow() {
    let mut r = Rig::new(START);
    r.click_with("c3", COMMAND);
    let m = marks(&r.click_with("c3", COMMAND));
    assert!(m.is_empty());
    assert_eq!(r.it.arrow_start(), None);
}

#[test]
fn m2_3_command_click_outside_cancels_and_a_plain_click_too() {
    let mut r = Rig::new(START);
    r.click_with("c3", COMMAND);
    r.press_at(Point::new(-4.0, -4.0), COMMAND);
    assert_eq!(r.it.arrow_start(), None);

    let mut r = Rig::new(START);
    r.click_with("c3", COMMAND);
    r.click("e2");
    assert_eq!(
        r.it.arrow_start(),
        None,
        "a plain click abandons the gesture"
    );
    assert_eq!(r.it.selected(), Some(sq("e2")));
}

#[test]
fn m2_3_command_click_does_not_play_moves() {
    let mut r = Rig::new(START);
    r.click("e2");
    let ev = r.click_with("e4", COMMAND);
    assert!(plays(&ev).is_empty());
}

// ---------------------------------------------------------------------------
// Hover, cursor and idle behavior (rules 23 and 58)
// ---------------------------------------------------------------------------

#[test]
fn m2_3_idle_cursor_movement_requests_no_redraw() {
    let mut r = Rig::new(START);
    assert!(
        r.go("e4").is_empty(),
        "rule 58: idle movement is not captured"
    );
    assert_eq!(r.it.hover(), Some(sq("e4")));
    r.send(Input::CursorLeft);
    assert_eq!(r.it.hover(), None);
}

#[test]
fn m2_3_non_finite_pointer_positions_do_not_panic() {
    let mut r = Rig::new(START);
    r.click("e2");
    for p in [
        Point::new(f32::NAN, f32::NAN),
        Point::new(f32::INFINITY, 1.0),
        Point::new(1.0, f32::NEG_INFINITY),
    ] {
        r.move_to(p);
        r.press_at(p, Mods::default());
        r.release_at(p);
        r.send(Input::RightPressed {
            pos: p,
            mods: Mods::default(),
        });
        r.send(Input::RightReleased {
            pos: p,
            mods: Mods::default(),
        });
    }
    r.press("e2");
    r.move_to(Point::new(f32::NAN, 3.0));
    let ev = r.release_at(Point::new(f32::NAN, 3.0));
    assert!(plays(&ev).is_empty());
}

#[test]
fn m2_3_cursor_kind_follows_the_state() {
    let mut r = Rig::new(PROMO);
    let kind = |r: &Rig, p: Option<Point>| {
        let ctx = BoardContext {
            layout: r.layout,
            orientation: r.orientation,
            position: &r.position,
            accepts_moves: r.accepts,
            brush: &r.brush,
        };
        r.it.cursor_kind(p, &ctx)
    };
    assert_eq!(kind(&r, Some(r.at("a7"))), CursorKind::Grab);
    assert_eq!(kind(&r, Some(r.at("e1"))), CursorKind::Grab);
    assert_eq!(
        kind(&r, Some(r.at("e8"))),
        CursorKind::Default,
        "enemy king"
    );
    assert_eq!(kind(&r, Some(r.at("d4"))), CursorKind::Default, "empty");
    assert_eq!(kind(&r, None), CursorKind::Default);
    r.click("a7");
    r.click("a8");
    assert_eq!(kind(&r, Some(r.at("a6"))), CursorKind::Pointer);
    assert_eq!(kind(&r, Some(r.at("d4"))), CursorKind::Default);
    r.send(Input::Escape);
    r.send(Input::Escape);
    r.press("a7");
    r.go("a8");
    assert_eq!(kind(&r, Some(r.at("a8"))), CursorKind::Grabbing);
    assert_eq!(kind(&r, None), CursorKind::Grabbing);
    r.release("a7");
    r.accepts = false;
    r.send(Input::Escape);
    assert_eq!(kind(&r, Some(r.at("e1"))), CursorKind::Default);
}

// ---------------------------------------------------------------------------
// legal_destinations (strategy 1, "Destinos")
// ---------------------------------------------------------------------------

#[test]
fn m2_3_legal_destinations_mark_captures_and_en_passant() {
    let pos = Position::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").expect("fen");
    let d = legal_destinations(&pos, sq("e5"));
    let e6 = d.iter().find(|x| x.square == sq("e6")).expect("e6");
    let d6 = d.iter().find(|x| x.square == sq("d6")).expect("d6");
    assert!(!e6.capture);
    assert!(d6.capture, "en passant draws a ring on the destination");
}

#[test]
fn m2_3_promotion_destination_is_listed_once() {
    let pos = Position::from_fen(PROMO).expect("fen");
    let d = legal_destinations(&pos, sq("a7"));
    assert_eq!(d.iter().filter(|x| x.square == sq("a8")).count(), 1);
    assert_eq!(d.iter().filter(|x| x.square == sq("b8")).count(), 1);
    assert!(d.iter().find(|x| x.square == sq("b8")).expect("b8").capture);
}

#[test]
fn m2_3_legal_destinations_of_an_empty_or_enemy_square_is_empty() {
    let pos = Position::from_fen(START).expect("fen");
    assert!(legal_destinations(&pos, sq("e4")).is_empty());
    assert!(legal_destinations(&pos, sq("e7")).is_empty());
}

const PERFT_FENS: [&str; 6] = [
    START,
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
    "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
    "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
];

#[test]
fn m2_3_legal_destinations_match_legal_moves_from_on_perft_positions() {
    for fen in PERFT_FENS {
        let pos = Position::from_fen(fen).expect("fen");
        for &from in &Square::ALL {
            let moves = pos.legal_moves_from(from);
            let dests = legal_destinations(&pos, from);
            let mut expect: Vec<Square> = moves.iter().filter_map(|m| m.to()).collect();
            expect.sort_unstable();
            expect.dedup();
            let mut got: Vec<Square> = dests.iter().map(|d| d.square).collect();
            got.sort_unstable();
            assert_eq!(got, expect, "{fen} from {from}");
            for d in &dests {
                let any_capture = moves
                    .iter()
                    .any(|m| m.to() == Some(d.square) && m.is_capture());
                assert_eq!(d.capture, any_capture, "{fen} {from}->{}", d.square);
            }
        }
    }
}

#[test]
fn m2_3_every_legal_move_can_be_entered_by_click_click() {
    for fen in PERFT_FENS {
        let pos = Position::from_fen(fen).expect("fen");
        for mv in pos.legal_moves() {
            let (Some(from), Some(to)) = (mv.from(), mv.to()) else {
                continue;
            };
            let mut r = Rig::new(fen);
            let a = r.at(&from.to_string());
            let b = r.at(&to.to_string());
            let sent = plays_from_all(&mut r, a, b, mv);
            assert_eq!(sent, [mv], "{fen} {mv:?}");
        }
    }
}

/// Replays click-click (and the column) collecting every published move.
fn plays_from_all(r: &mut Rig, a: Point, b: Point, mv: Move) -> Vec<Move> {
    let mut all = Vec::new();
    all.extend(r.press_at(a, Mods::default()));
    all.extend(r.release_at(a));
    all.extend(r.press_at(b, Mods::default()));
    all.extend(r.release_at(b));
    if r.it.pending_promotion().is_some() {
        let to = mv.to().expect("to");
        let cells = promotion_cells(&r.layout, to, Orientation::White);
        let role = mv.promotion().expect("promotion");
        let p = cells
            .iter()
            .find(|c| c.role == role)
            .expect("cell")
            .rect
            .center();
        all.extend(r.press_at(p, Mods::default()));
        all.extend(r.release_at(p));
    }
    plays(&all).into_iter().map(|(m, _)| m).collect()
}

// ---------------------------------------------------------------------------
// Random scripts: whatever the user does, the GUI only proposes legal moves.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Step {
    LeftPress(f32, f32, u8),
    LeftRelease(f32, f32),
    RightPress(f32, f32, u8),
    RightRelease(f32, f32),
    Move(f32, f32),
    Left,
    Escape,
    Enter,
    PositionChanged,
    OrientationChanged,
}

fn mods_from_bits(b: u8) -> Mods {
    Mods {
        shift: b & 1 != 0,
        command: b & 2 != 0,
        alt: b & 4 != 0,
    }
}

fn step_strategy() -> impl Strategy<Value = Step> {
    let coord = || -50.0f32..850.0;
    prop_oneof![
        (coord(), coord(), 0u8..8).prop_map(|(x, y, m)| Step::LeftPress(x, y, m)),
        (coord(), coord()).prop_map(|(x, y)| Step::LeftRelease(x, y)),
        (coord(), coord(), 0u8..8).prop_map(|(x, y, m)| Step::RightPress(x, y, m)),
        (coord(), coord()).prop_map(|(x, y)| Step::RightRelease(x, y)),
        (coord(), coord()).prop_map(|(x, y)| Step::Move(x, y)),
        Just(Step::Left),
        Just(Step::Escape),
        Just(Step::Enter),
        Just(Step::PositionChanged),
        Just(Step::OrientationChanged),
    ]
}

proptest! {
    #[test]
    fn m2_3_prop_random_scripts_only_propose_legal_moves_and_never_panic(
        fen_index in 0usize..8,
        flipped in any::<bool>(),
        steps in proptest::collection::vec(step_strategy(), 1..80),
    ) {
        let fens = [PERFT_FENS[0], PERFT_FENS[1], PERFT_FENS[2], PERFT_FENS[3],
                    PROMO, CASTLE, KINGS, "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1"];
        let mut r = Rig::new(fens[fen_index]);
        if flipped { r.orientation = Orientation::Black; }
        for step in steps {
            let events = match step {
                Step::LeftPress(x, y, m) => r.press_at(Point::new(x, y), mods_from_bits(m)),
                Step::LeftRelease(x, y) => r.release_at(Point::new(x, y)),
                Step::RightPress(x, y, m) => r.send(Input::RightPressed { pos: Point::new(x, y), mods: mods_from_bits(m) }),
                Step::RightRelease(x, y) => r.send(Input::RightReleased { pos: Point::new(x, y), mods: Mods::default() }),
                Step::Move(x, y) => r.move_to(Point::new(x, y)),
                Step::Left => r.send(Input::CursorLeft),
                Step::Escape => r.send(Input::Escape),
                Step::Enter => r.send(Input::Enter),
                Step::PositionChanged => {
                    let ev = r.send(Input::PositionChanged);
                    prop_assert!(r.it.is_idle());
                    ev
                }
                Step::OrientationChanged => {
                    let ev = r.send(Input::OrientationChanged);
                    prop_assert!(r.it.is_idle());
                    ev
                }
            };
            for e in events {
                match e {
                    BoardEvent::PlayMove { mv, .. } => {
                        prop_assert!(r.position.is_legal(mv), "illegal proposal {:?}", mv);
                        prop_assert!(!mv.is_null());
                    }
                    BoardEvent::ToggleMark(m) => {
                        if m.kind == MarkKind::Arrow {
                            prop_assert!(m.to.is_some() && m.to != Some(m.from));
                        } else {
                            prop_assert!(m.to.is_none());
                        }
                    }
                    BoardEvent::PlayNullMove
                    | BoardEvent::PlayEngineArrow { .. }
                    | BoardEvent::Redraw => {}
                }
            }
            // Structural invariants.
            if let Some(p) = r.it.pending_promotion() {
                prop_assert_eq!(p.candidates.len(), 4);
                prop_assert!(r.it.drag().is_none());
            }
            if let Some(sel) = r.it.selected() {
                prop_assert!(r.position.piece_at(sel).is_some_and(|p| p.color == r.position.turn()));
            }
        }
    }
}

#[test]
fn m2_3_second_press_during_a_drag_leaves_a_consistent_state() {
    // Found by the random script property: a lost button release (focus
    // change) followed by another press. The drag must not outlive the
    // selection it belongs to.
    let mut r = Rig::new(START);
    r.press("b1");
    r.go("b4");
    assert!(r.it.drag().is_some());
    r.press("b8"); // enemy knight: the press clears the selection
    assert_eq!(r.it.selected(), None);
    assert!(
        r.it.drag().is_none(),
        "a drag from {:?} is still active with nothing selected",
        r.it.drag().map(|d| d.from)
    );
    // And a later release over a legal square must not play anything.
    assert!(plays(&r.release("c3")).is_empty());
}
