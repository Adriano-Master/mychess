#![allow(clippy::float_cmp, clippy::assert_is_empty)]
//! Tests of the widget logic with a stand-in paragraph: no renderer, no
//! window. The `iced_test` tests with the real renderer are in
//! `tests/notation_simulator.rs`.

use iced::{Point, Rectangle, Size};
use mychess_app::{CommentTarget, NotationDoc, NotationLayout, NotationOptions, build_notation};
use mychess_core::{Game, Nag, NodeId, Position};
use pretty_assertions::assert_eq;

use super::mock::{MockParagraph, shaped_on_this_thread};
use super::style::{Labels, TextStyle};
use super::{Hit, NotationEvent, Panel, Params};
use crate::theme::notation_colors::NotationPalette;

/// 5 px per character, 15 px per line.
fn style() -> TextStyle {
    TextStyle {
        size: 10.0,
        line_height: 1.5,
        palette: NotationPalette::of(&iced::Theme::Light),
        symbolic_nags: true,
        bold_main: false,
        figurines: false,
    }
}

const SIZE: Size = Size::new(400.0, 100.0);

fn play(game: &mut Game, parent: NodeId, uci: &str) -> NodeId {
    let mv = game
        .position_at(parent)
        .expect("position")
        .parse_uci(uci)
        .expect("legal move");
    game.add_move(parent, mv).expect("move")
}

/// 1.e4 e5 2.Nf3 (2.Nc3) 2...Nc6 with a comment after e4.
fn sample() -> (Game, Vec<NodeId>) {
    let mut game = Game::new();
    let root = game.root();
    let e4 = play(&mut game, root, "e2e4");
    let e5 = play(&mut game, e4, "e7e5");
    let nf3 = play(&mut game, e5, "g1f3");
    let nc3 = play(&mut game, e5, "b1c3");
    let nc6 = play(&mut game, nf3, "b8c6");
    game.set_comment(e4, "the king pawn").expect("comment");
    game.add_nag(e5, Nag::new(1).expect("nag")).expect("nag");
    (game, vec![e4, e5, nf3, nc3, nc6])
}

fn doc(game: &Game, revision: u64) -> NotationDoc {
    build_notation(game, &NotationOptions::default(), revision)
}

fn labels() -> Labels {
    Labels {
        round: "Round".to_owned(),
        position: "Position".to_owned(),
        empty: "No moves yet".to_owned(),
    }
}

struct Rig {
    panel: Panel<MockParagraph>,
    labels: Labels,
    style: TextStyle,
}

impl Rig {
    fn new() -> Rig {
        Rig {
            panel: Panel::default(),
            labels: labels(),
            style: style(),
        }
    }

    fn prepare(&mut self, doc: &NotationDoc, current: Option<NodeId>, token: u64, size: Size) {
        let params = Params {
            doc,
            labels: &self.labels,
            style: &self.style,
            current,
            menu_target: None,
            reveal_token: token,
            table: None,
        };
        self.panel.prepare(&params, size);
    }

    /// The center of the first line of the span of a move, in widget
    /// coordinates.
    fn center_of(&self, node: NodeId) -> Point {
        let (entry, span) = self.panel.cache.find_move(node).expect("move is shown");
        let rect = self.panel.cache.span_rects(entry, span)[0];
        Point::new(
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0 - self.panel.scroll.offset(),
        )
    }
}

fn paragraphs(doc: &NotationDoc) -> usize {
    doc.header.iter().len().min(1) + doc.blocks.len() + usize::from(doc.result.is_some())
}

#[test]
fn the_first_layout_shapes_each_paragraph_once() {
    let (game, _) = sample();
    let doc = doc(&game, 1);
    let mut rig = Rig::new();
    let before = shaped_on_this_thread();
    rig.prepare(&doc, None, 0, SIZE);
    assert_eq!(shaped_on_this_thread() - before, paragraphs(&doc));
    assert_eq!(rig.panel.cache.shaped(), paragraphs(&doc));
}

#[test]
fn moving_the_cursor_shapes_nothing() {
    let (game, nodes) = sample();
    let doc = doc(&game, 1);
    let mut rig = Rig::new();
    rig.prepare(&doc, None, 0, SIZE);
    let before = shaped_on_this_thread();
    for &node in &nodes {
        rig.prepare(&doc, Some(node), 0, SIZE);
    }
    rig.prepare(&doc, None, 0, SIZE);
    assert_eq!(shaped_on_this_thread(), before, "no paragraph was shaped");
    assert_eq!(rig.panel.cache.resized(), 0);
}

#[test]
fn editing_one_block_shapes_only_that_block() {
    let (mut game, nodes) = sample();
    let first = doc(&game, 1);
    let mut rig = Rig::new();
    rig.prepare(&first, None, 0, SIZE);
    // A comment on the move inside the variation changes its block only.
    game.set_comment(nodes[3], "weaker").expect("comment");
    let mut second = doc(&game, 2);
    second.keep_revisions(&first);
    let changed = second
        .blocks
        .iter()
        .filter(|b| b.rev == second.revision)
        .count();
    assert!(changed >= 1 && changed < second.blocks.len());
    let before = rig.panel.cache.shaped();
    rig.prepare(&second, None, 0, SIZE);
    let after = rig.panel.cache.shaped();
    // The changed blocks, plus the result which is rebuilt only if it
    // changed (it did not).
    assert_eq!(after - before, changed);
}

#[test]
fn a_new_width_lays_out_again_without_shaping() {
    let (game, _) = sample();
    let doc = doc(&game, 1);
    let mut rig = Rig::new();
    rig.prepare(&doc, None, 0, SIZE);
    let shaped = rig.panel.cache.shaped();
    rig.prepare(&doc, None, 0, Size::new(220.0, 100.0));
    assert_eq!(rig.panel.cache.shaped(), shaped);
    assert!(rig.panel.cache.resized() > 0);
    let height_narrow = rig.panel.cache.content_height();
    rig.prepare(&doc, None, 0, Size::new(2000.0, 100.0));
    assert!(rig.panel.cache.content_height() <= height_narrow);
}

#[test]
fn a_new_text_style_shapes_everything_again() {
    let (game, _) = sample();
    let doc = doc(&game, 1);
    let mut rig = Rig::new();
    rig.prepare(&doc, None, 0, SIZE);
    let shaped = rig.panel.cache.shaped();
    rig.style.symbolic_nags = false;
    rig.prepare(&doc, None, 0, SIZE);
    assert_eq!(rig.panel.cache.shaped() - shaped, paragraphs(&doc));
}

#[test]
fn the_same_document_and_size_do_nothing() {
    let (game, _) = sample();
    let doc = doc(&game, 1);
    let mut rig = Rig::new();
    rig.prepare(&doc, None, 0, SIZE);
    let stamp = rig.panel.cache.stamp();
    rig.prepare(&doc, None, 0, SIZE);
    assert_eq!(rig.panel.cache.stamp(), stamp);
}

#[test]
fn the_empty_game_shows_its_message_and_the_result() {
    let doc = doc(&Game::new(), 1);
    let mut rig = Rig::new();
    rig.prepare(&doc, None, 0, SIZE);
    // Header, the "no moves" paragraph and the result.
    assert_eq!(rig.panel.cache.len(), paragraphs(&doc) + 1);
    assert!(rig.panel.cache.first_move().is_none());
    // The start marker has nowhere to be and nothing breaks.
    assert!(rig.panel.current.rects.is_empty());
}

#[test]
fn the_pointer_over_a_move_reports_it_once_and_leaving_reports_none() {
    let (game, nodes) = sample();
    let doc = doc(&game, 1);
    let mut rig = Rig::new();
    rig.prepare(&doc, None, 0, Size::new(400.0, 400.0));
    let at = rig.center_of(nodes[0]);
    let window = Point::new(at.x + 10.0, at.y + 10.0);
    let moved = rig.panel.pointer(Some(at), Some(window));
    assert!(moved.redraw);
    assert_eq!(
        moved.event,
        Some(NotationEvent::Hover(Some((nodes[0], window))))
    );
    assert_eq!(rig.panel.hover.hit, Some(Hit::Move(nodes[0])));
    // Moving inside the same move changes nothing.
    let again = rig
        .panel
        .pointer(Some(Point::new(at.x + 1.0, at.y)), Some(window));
    assert_eq!(again, super::Moved::default());
    // Leaving.
    let left = rig.panel.pointer(None, None);
    assert_eq!(left.event, Some(NotationEvent::Hover(None)));
}

#[test]
fn a_click_on_a_move_publishes_its_node_and_a_drag_off_it_does_not() {
    let (game, nodes) = sample();
    let doc = doc(&game, 1);
    let mut rig = Rig::new();
    rig.prepare(&doc, None, 0, Size::new(400.0, 400.0));
    let at = rig.center_of(nodes[2]);
    assert!(rig.panel.press_left(at));
    let (used, event) = rig.panel.release_left(Some(at));
    assert!(used);
    assert_eq!(event, Some(NotationEvent::Click(nodes[2])));
    // Press on one move, release on another.
    assert!(rig.panel.press_left(rig.center_of(nodes[0])));
    let (_, event) = rig.panel.release_left(Some(rig.center_of(nodes[1])));
    assert_eq!(event, None);
}

#[test]
fn clicking_numbers_blanks_or_empty_space_does_nothing() {
    let (game, _) = sample();
    let doc = doc(&game, 1);
    let mut rig = Rig::new();
    rig.prepare(&doc, None, 0, Size::new(400.0, 400.0));
    // The header takes the first lines; the number "1." heads the first
    // move block. Probe the whole widget and make sure only clickable
    // spans ever press.
    let mut pressed = 0;
    let mut y = 1.0;
    while y < 399.0 {
        let mut x = 1.0;
        while x < 399.0 {
            if rig.panel.press_left(Point::new(x, y)) {
                pressed += 1;
                let _ = rig.panel.release_left(Some(Point::new(x, y)));
            }
            x += 3.0;
        }
        y += 5.0;
    }
    assert!(pressed > 0, "some spans are clickable");
    // Far below the text.
    assert!(!rig.panel.press_left(Point::new(100.0, 390.0)));
}

#[test]
fn a_click_on_a_comment_publishes_its_target() {
    let (game, nodes) = sample();
    let doc = doc(&game, 1);
    let mut rig = Rig::new();
    rig.prepare(&doc, None, 0, Size::new(400.0, 400.0));
    let mut found = None;
    let mut y = 1.0;
    'scan: while y < 399.0 {
        let mut x = 1.0;
        while x < 399.0 {
            let p = Point::new(x, y);
            let _ = rig.panel.pointer(Some(p), None);
            if let Some(Hit::Comment(target)) = rig.panel.hover.hit {
                found = Some((p, target));
                break 'scan;
            }
            x += 2.0;
        }
        y += 4.0;
    }
    let (p, target) = found.expect("the comment is on screen");
    assert_eq!(target, CommentTarget::After(nodes[0]));
    assert!(rig.panel.press_left(p));
    let (_, event) = rig.panel.release_left(Some(p));
    assert_eq!(event, Some(NotationEvent::CommentClick(target)));
}

#[test]
fn right_click_reports_the_item_or_empty_space() {
    let (game, nodes) = sample();
    let doc = doc(&game, 1);
    let mut rig = Rig::new();
    rig.prepare(&doc, None, 0, Size::new(400.0, 400.0));
    let at = rig.center_of(nodes[1]);
    let window = Point::new(500.0, 300.0);
    assert_eq!(
        rig.panel.press_right(at, window),
        NotationEvent::Menu {
            hit: Some(Hit::Move(nodes[1])),
            position: window
        }
    );
    assert_eq!(
        rig.panel.press_right(Point::new(300.0, 390.0), window),
        NotationEvent::Menu {
            hit: None,
            position: window
        }
    );
}

#[test]
fn the_middle_button_previews_while_held() {
    let (game, nodes) = sample();
    let doc = doc(&game, 1);
    let mut rig = Rig::new();
    rig.prepare(&doc, None, 0, Size::new(400.0, 400.0));
    let at = rig.center_of(nodes[4]);
    let window = Point::new(9.0, 9.0);
    assert_eq!(
        rig.panel.press_middle(at, window),
        Some(NotationEvent::Middle(Some((nodes[4], window))))
    );
    assert_eq!(
        rig.panel.release_middle(),
        Some(NotationEvent::Middle(None))
    );
    assert_eq!(rig.panel.release_middle(), None, "only after a press");
    assert_eq!(
        rig.panel.press_middle(Point::new(390.0, 390.0), window),
        None
    );
}

fn long_game(plies: usize) -> (Game, NodeId) {
    let mut game = Game::new();
    let mut at = game.root();
    let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
    for ply in 0..plies {
        let uci = cycle[ply % 4];
        at = play(&mut game, at, uci);
    }
    (game, at)
}

fn tall_rig(plies: usize) -> (Rig, NotationDoc, Game, NodeId) {
    let (game, last) = long_game(plies);
    let opts = NotationOptions {
        layout: NotationLayout::Inline,
        ..NotationOptions::default()
    };
    let doc = build_notation(&game, &opts, 1);
    let mut rig = Rig::new();
    rig.prepare(&doc, None, 0, Size::new(200.0, 60.0));
    (rig, doc, game, last)
}

#[test]
fn the_wheel_scrolls_and_stops_at_the_ends() {
    let (mut rig, ..) = tall_rig(120);
    assert_eq!(rig.panel.scroll.offset(), 0.0);
    assert!(!rig.panel.wheel(1.0, None), "already at the top");
    assert!(rig.panel.wheel(-1.0, None));
    assert_eq!(rig.panel.scroll.offset(), 3.0 * 15.0, "three lines a notch");
    assert!(rig.panel.wheel(-10_000.0, None));
    let max = rig.panel.scroll.max_offset();
    assert_eq!(rig.panel.scroll.offset(), max);
    assert!(!rig.panel.wheel(-1.0, None));
}

#[test]
fn a_new_reveal_token_scrolls_to_the_current_move_and_the_same_token_does_not() {
    let (mut rig, doc, _game, last) = tall_rig(120);
    assert!(rig.panel.scroll.max_offset() > 0.0);
    let size = Size::new(200.0, 60.0);
    rig.prepare(&doc, Some(last), 1, size);
    let offset = rig.panel.scroll.offset();
    assert!(offset > 0.0, "the last move is far down");
    let rect = rig
        .panel
        .current
        .bounds()
        .expect("current move has a place");
    assert!(rect.y >= offset && rect.y + rect.height <= offset + 60.0);
    // The user scrolls away; the cursor moves without a new token: stays.
    rig.panel.scroll.set_offset(0.0);
    rig.prepare(&doc, Some(last), 1, size);
    assert_eq!(rig.panel.scroll.offset(), 0.0);
    // A new token brings it back.
    rig.prepare(&doc, Some(last), 2, size);
    assert!(rig.panel.scroll.offset() > 0.0);
    // Going to the start scrolls to the top.
    rig.prepare(&doc, None, 3, size);
    assert_eq!(rig.panel.scroll.offset(), 0.0);
}

#[test]
fn the_scroll_bar_can_be_dragged_and_clicked() {
    let (mut rig, ..) = tall_rig(120);
    let (top, height) = rig.panel.thumb().expect("content overflows");
    assert_eq!(top, 0.0);
    let x = 200.0 - 2.0;
    // Grab the thumb and drag it to the bottom.
    assert!(rig.panel.press_left(Point::new(x, height / 2.0)));
    assert!(rig.panel.drag_to(10_000.0));
    assert_eq!(rig.panel.scroll.offset(), rig.panel.scroll.max_offset());
    let (used, event) = rig.panel.release_left(None);
    assert!(used);
    assert_eq!(event, None);
    assert!(!rig.panel.drag_to(0.0), "the drag ended");
    // A click on the track above the thumb jumps there.
    assert!(rig.panel.press_left(Point::new(x, 2.0)));
    assert!(rig.panel.scroll.offset() < rig.panel.scroll.max_offset());
}

#[test]
fn the_bar_is_not_part_of_the_text() {
    let (mut rig, ..) = tall_rig(120);
    let moved = rig.panel.pointer(Some(Point::new(199.0, 5.0)), None);
    assert_eq!(moved, super::Moved::default());
}

#[test]
fn hover_rectangles_follow_the_span_and_current_marks_the_cursor_move() {
    let (game, nodes) = sample();
    let doc = doc(&game, 1);
    let mut rig = Rig::new();
    rig.prepare(&doc, Some(nodes[1]), 0, Size::new(400.0, 400.0));
    assert_eq!(rig.panel.current.rects.len(), 1);
    let (entry, span) = rig.panel.cache.find_move(nodes[1]).expect("move");
    let expected: Vec<Rectangle> = rig.panel.cache.span_rects(entry, span);
    assert_eq!(rig.panel.current.rects, expected);
    // The start of the game is a bar of no width on the first move.
    rig.prepare(&doc, None, 0, Size::new(400.0, 400.0));
    let bar = rig.panel.current.rects[0];
    assert_eq!(bar.width, 0.0);
    let first = rig.panel.cache.first_move().expect("first move");
    assert_eq!(bar.x, rig.panel.cache.span_rects(first.0, first.1)[0].x);
}

#[test]
fn a_changed_document_forgets_the_hover() {
    let (mut game, nodes) = sample();
    let first = doc(&game, 1);
    let mut rig = Rig::new();
    rig.prepare(&first, None, 0, Size::new(400.0, 400.0));
    let at = rig.center_of(nodes[0]);
    let _ = rig.panel.pointer(Some(at), None);
    assert!(rig.panel.hover.hit.is_some());
    game.set_comment(nodes[0], "").expect("comment");
    let mut second = doc(&game, 2);
    second.keep_revisions(&first);
    rig.prepare(&second, None, 0, Size::new(400.0, 400.0));
    assert_eq!(rig.panel.hover.hit, None);
}

#[test]
fn the_start_position_marker_needs_the_start_fen_not_a_position() {
    // A game from a FEN with Black to move still has a first move.
    let position = Position::from_fen("7k/8/8/8/8/8/4p3/R3K3 b - - 0 1").expect("fen");
    let mut game = Game::from_position(position);
    let first = game
        .position_at(game.root())
        .expect("position")
        .parse_uci("h8g8")
        .expect("move");
    game.add_move(game.root(), first).expect("move");
    let doc = doc(&game, 1);
    let mut rig = Rig::new();
    rig.prepare(&doc, None, 0, Size::new(400.0, 400.0));
    assert!(rig.panel.cache.first_move().is_some());
}

#[test]
fn a_move_with_a_figurine_is_one_item_made_of_two_spans() {
    let (game, nodes) = sample();
    let doc = doc(&game, 1);
    let mut rig = Rig::new();
    rig.style.figurines = true;
    rig.prepare(&doc, None, 0, Size::new(400.0, 400.0));
    let nf3 = nodes[2];
    let (entry, first) = rig.panel.cache.find_move(nf3).expect("move");
    // Glyph, then `f3`: the same item.
    let glyph = rig.panel.cache.span_rects(entry, first)[0];
    let on_text = Point::new(
        glyph.x + glyph.width + 3.0,
        glyph.y + glyph.height / 2.0 - rig.panel.scroll.offset(),
    );
    let moved = rig.panel.pointer(Some(on_text), None);
    assert!(moved.redraw);
    assert_eq!(rig.panel.hover.hit, Some(Hit::Move(nf3)));
    // Hovering the glyph afterwards is the same item: nothing changes.
    let on_glyph = Point::new(
        glyph.x + 1.0,
        glyph.y + glyph.height / 2.0 - rig.panel.scroll.offset(),
    );
    assert_eq!(
        rig.panel.pointer(Some(on_glyph), None),
        super::Moved::default()
    );
    // The highlight covers both spans.
    assert_eq!(rig.panel.hover.rects.len(), 2);
    let width: f32 = rig.panel.hover.rects.iter().map(|r| r.width).sum();
    assert_eq!(width, 3.0 * 5.0, "one glyph and `f3`, 5 px a character");
    // Clicking on the text part goes to the move.
    assert!(rig.panel.press_left(on_text));
    assert_eq!(
        rig.panel.release_left(Some(on_text)).1,
        Some(NotationEvent::Click(nf3))
    );
    // The current marker covers the whole move too.
    rig.prepare(&doc, Some(nf3), 0, Size::new(400.0, 400.0));
    assert_eq!(rig.panel.current.rects.len(), 2);
}
