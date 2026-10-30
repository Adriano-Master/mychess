#![allow(clippy::float_cmp, clippy::assert_is_empty)]
//! Tests of the widget over the table with a stand-in paragraph: the pointer,
//! the pill, the reveal, the shaping counters. The cache itself is tested in
//! `table_layout/tests.rs`; the real renderer in
//! `tests/m2_5_notation_table.rs`.

use std::collections::HashSet;

use iced::{Point, Size};
use mychess_app::{
    CommentTarget, NotationDoc, NotationLayout, NotationOptions, NotationTable, build_notation,
};
use mychess_core::{Color as Side, Game, Nag, NodeId};
use pretty_assertions::assert_eq;

use super::mock::{MockParagraph, shaped_on_this_thread};
use super::style::{Labels, TextStyle};
use super::table_layout::{Mark, Place};
use super::{Hit, NotationEvent, Panel, Params, TableParams};
use crate::theme::notation_colors::NotationPalette;

/// 5 px per character, 15 px per line.
fn style() -> TextStyle {
    TextStyle {
        size: 10.0,
        line_height: 1.5,
        palette: NotationPalette::of(&iced::Theme::Dark),
        symbolic_nags: true,
        bold_main: false,
        figurines: true,
    }
}

fn labels() -> Labels {
    Labels {
        round: "Round".to_owned(),
        position: "Position".to_owned(),
        empty: "No moves yet".to_owned(),
    }
}

fn play(game: &mut Game, parent: NodeId, uci: &str) -> NodeId {
    let mv = game
        .position_at(parent)
        .expect("position")
        .parse_uci(uci)
        .expect("legal move");
    game.add_move(parent, mv).expect("move")
}

/// 1.e4 {the king pawn} e5! 2.Nf3 (2.Nc3 Nf6) 2...Nc6.
struct Sample {
    game: Game,
    e4: NodeId,
    e5: NodeId,
    nf3: NodeId,
    nc3: NodeId,
    nf6: NodeId,
    nc6: NodeId,
}

fn sample() -> Sample {
    let mut game = Game::new();
    let root = game.root();
    let e4 = play(&mut game, root, "e2e4");
    let e5 = play(&mut game, e4, "e7e5");
    let nf3 = play(&mut game, e5, "g1f3");
    let nc3 = play(&mut game, e5, "b1c3");
    let nf6 = play(&mut game, nc3, "g8f6");
    let nc6 = play(&mut game, nf3, "b8c6");
    game.set_comment(e4, "the king pawn").expect("comment");
    game.add_nag(e5, Nag::new(1).expect("nag")).expect("nag");
    Sample {
        game,
        e4,
        e5,
        nf3,
        nc3,
        nf6,
        nc6,
    }
}

fn long_game(plies: usize) -> (Game, Vec<NodeId>) {
    let mut game = Game::new();
    let mut at = game.root();
    let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
    let mut nodes = Vec::new();
    for ply in 0..plies {
        at = play(&mut game, at, cycle[ply % 4]);
        nodes.push(at);
    }
    (game, nodes)
}

struct Built {
    doc: NotationDoc,
    table: NotationTable,
}

fn build(game: &Game) -> Built {
    let opts = NotationOptions {
        layout: NotationLayout::Table,
        ..NotationOptions::default()
    };
    let doc = build_notation(game, &opts, 1);
    let table = NotationTable::from_doc(&doc, game, &opts);
    Built { doc, table }
}

struct Rig {
    panel: Panel<MockParagraph>,
    labels: Labels,
    style: TextStyle,
    collapsed: HashSet<NodeId>,
}

impl Rig {
    fn new() -> Rig {
        Rig {
            panel: Panel::default(),
            labels: labels(),
            style: style(),
            collapsed: HashSet::new(),
        }
    }

    fn prepare(&mut self, built: &Built, current: Option<NodeId>, token: u64, size: Size) {
        let params = Params {
            doc: &built.doc,
            labels: &self.labels,
            style: &self.style,
            current,
            menu_target: None,
            reveal_token: token,
            table: Some(TableParams {
                table: &built.table,
                collapsed: Some(&self.collapsed),
            }),
        };
        self.panel.prepare(&params, size);
    }

    /// The center of the cell (or card text) of a move, in widget
    /// coordinates.
    fn center_of(&mut self, node: NodeId) -> Point {
        let (rects, _) = self
            .panel
            .table
            .move_rects(node, Mark::Cell)
            .expect("move is shown");
        let rect = rects[0];
        Point::new(
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0 - self.panel.scroll.offset(),
        )
    }
}

const SIZE: Size = Size::new(400.0, 400.0);

#[test]
fn the_first_layout_shapes_only_the_rows_near_the_window() {
    let (game, _) = long_game(300);
    let built = build(&game);
    let mut rig = Rig::new();
    let before = shaped_on_this_thread();
    rig.prepare(&built, None, 0, Size::new(400.0, 300.0));
    let shaped_rows = rig.panel.table.rows_shaped();
    // 300 px window, 150 px of margin below, 32 px a row.
    assert!(
        (9..=15).contains(&shaped_rows),
        "{shaped_rows} of {} rows",
        rig.panel.table.len()
    );
    assert!(rig.panel.table.len() >= 150);
    // The paragraphs are few: the rows shaped, a few small parts each.
    assert!(shaped_on_this_thread() - before < 15 * 8);
}

#[test]
fn moving_the_cursor_shapes_nothing() {
    let s = sample();
    let built = build(&s.game);
    let mut rig = Rig::new();
    rig.prepare(&built, None, 0, SIZE);
    let before = shaped_on_this_thread();
    let rows = rig.panel.table.rows_shaped();
    for node in [s.e4, s.e5, s.nf3, s.nc3, s.nf6, s.nc6] {
        rig.prepare(&built, Some(node), 0, SIZE);
    }
    rig.prepare(&built, None, 0, SIZE);
    assert_eq!(shaped_on_this_thread(), before, "no paragraph was shaped");
    assert_eq!(rig.panel.table.rows_shaped(), rows);
    assert_eq!(rig.panel.table.resized(), 0);
}

#[test]
fn moving_the_cursor_down_a_long_game_shapes_only_rows_not_yet_near() {
    let (game, nodes) = long_game(300);
    let built = build(&game);
    let mut rig = Rig::new();
    rig.prepare(&built, None, 0, Size::new(400.0, 300.0));
    // Within the shaped window: nothing to shape.
    let before = shaped_on_this_thread();
    for &node in nodes.iter().take(12) {
        rig.prepare(&built, Some(node), 0, Size::new(400.0, 300.0));
    }
    assert_eq!(shaped_on_this_thread(), before);
}

#[test]
fn the_current_move_is_a_pill_around_its_text_and_the_root_has_none() {
    let s = sample();
    let built = build(&s.game);
    let mut rig = Rig::new();
    rig.prepare(&built, Some(s.e4), 0, SIZE);
    let row = rig.panel.table.find_move(s.e4);
    let Some(Place::Cell { row, side }) = row else {
        panic!("a cell");
    };
    assert_eq!(side, Side::White);
    assert_eq!(rig.panel.current.cell, Some((row, Side::White)));
    assert_eq!(rig.panel.current.rects.len(), 1);
    let pill = rig.panel.current.rects[0];
    let cell = rig.panel.table.columns();
    assert_eq!(pill.x, cell.white);
    assert!(pill.width < cell.cell);
    // At the start of the game: no pill.
    rig.prepare(&built, None, 0, SIZE);
    assert!(rig.panel.current.rects.is_empty());
    assert_eq!(rig.panel.current.cell, None);
}

#[test]
fn the_current_move_of_a_card_is_a_soft_highlight_on_its_text() {
    let s = sample();
    let built = build(&s.game);
    let mut rig = Rig::new();
    rig.prepare(&built, Some(s.nf6), 0, SIZE);
    assert_eq!(rig.panel.current.cell, None, "text, not a cell");
    assert!(!rig.panel.current.rects.is_empty());
    let (card_rects, _) = rig
        .panel
        .table
        .move_rects(s.nf6, Mark::Pill)
        .expect("card text");
    assert_eq!(rig.panel.current.rects, card_rects);
}

#[test]
fn a_click_on_a_cell_publishes_the_move_and_on_its_symbol_the_same_node() {
    let s = sample();
    let built = build(&s.game);
    let mut rig = Rig::new();
    rig.prepare(&built, None, 0, SIZE);
    for node in [s.e4, s.e5, s.nf3, s.nc6] {
        let at = rig.center_of(node);
        assert!(rig.panel.press_left(at), "{node}");
        let (used, event) = rig.panel.release_left(Some(at));
        assert!(used);
        assert_eq!(event, Some(NotationEvent::Click(node)));
    }
    // The symbol of e5.
    let cols = rig.panel.table.columns();
    let at = rig.center_of(s.e5);
    let on_nag = Point::new(
        cols.black + super::table_layout::PILL_PAD + 10.0 + 5.0,
        at.y,
    );
    let moved = rig.panel.pointer(Some(on_nag), None);
    assert!(moved.redraw);
    assert_eq!(rig.panel.hover.hit, Some(Hit::Nag(s.e5)));
    assert!(rig.panel.press_left(on_nag));
    assert_eq!(
        rig.panel.release_left(Some(on_nag)).1,
        Some(NotationEvent::Click(s.e5))
    );
}

#[test]
fn a_press_on_one_cell_released_on_another_does_nothing() {
    let s = sample();
    let built = build(&s.game);
    let mut rig = Rig::new();
    rig.prepare(&built, None, 0, SIZE);
    let a = rig.center_of(s.e4);
    let b = rig.center_of(s.nf3);
    assert!(rig.panel.press_left(a));
    assert_eq!(rig.panel.release_left(Some(b)).1, None);
}

#[test]
fn hovering_a_cell_reports_it_once_and_highlights_the_whole_column() {
    let s = sample();
    let built = build(&s.game);
    let mut rig = Rig::new();
    rig.prepare(&built, None, 0, SIZE);
    let at = rig.center_of(s.nf3);
    let window = Point::new(at.x + 10.0, at.y + 10.0);
    let moved = rig.panel.pointer(Some(at), Some(window));
    assert!(moved.redraw);
    assert_eq!(
        moved.event,
        Some(NotationEvent::Hover(Some((s.nf3, window))))
    );
    let (cell, _) = rig.panel.table.move_rects(s.nf3, Mark::Cell).expect("cell");
    assert_eq!(rig.panel.hover.rects, cell);
    assert!(!rig.panel.hover.on_text);
    // Anywhere else in the same cell changes nothing.
    let again = rig
        .panel
        .pointer(Some(Point::new(at.x + 20.0, at.y + 3.0)), Some(window));
    assert_eq!(again, super::Moved::default());
    // Leaving.
    assert_eq!(
        rig.panel.pointer(None, None).event,
        Some(NotationEvent::Hover(None))
    );
}

#[test]
fn right_and_middle_buttons_work_on_cells_like_on_text() {
    let s = sample();
    let built = build(&s.game);
    let mut rig = Rig::new();
    rig.prepare(&built, None, 0, SIZE);
    let at = rig.center_of(s.e5);
    let window = Point::new(500.0, 300.0);
    assert_eq!(
        rig.panel.press_right(at, window),
        NotationEvent::Menu {
            hit: Some(Hit::Move(s.e5)),
            position: window
        }
    );
    assert_eq!(
        rig.panel.press_middle(at, window),
        Some(NotationEvent::Middle(Some((s.e5, window))))
    );
    assert_eq!(
        rig.panel.release_middle(),
        Some(NotationEvent::Middle(None))
    );
    // Empty space: the menu of the empty space.
    let empty = Point::new(5.0, SIZE.height - 2.0);
    assert_eq!(
        rig.panel.press_right(empty, window),
        NotationEvent::Menu {
            hit: None,
            position: window
        }
    );
}

#[test]
fn clicking_a_comment_publishes_its_target() {
    let s = sample();
    let built = build(&s.game);
    let mut rig = Rig::new();
    rig.prepare(&built, None, 0, SIZE);
    let mut found = None;
    let mut y = 1.0;
    'scan: while y < SIZE.height {
        let mut x = 1.0;
        while x < SIZE.width - 10.0 {
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
    assert_eq!(target, CommentTarget::After(s.e4));
    assert!(rig.panel.hover.on_text);
    assert!(rig.panel.press_left(p));
    assert_eq!(
        rig.panel.release_left(Some(p)).1,
        Some(NotationEvent::CommentClick(target))
    );
}

#[test]
fn the_header_of_a_card_folds_it_and_the_press_must_end_on_the_header() {
    let s = sample();
    let built = build(&s.game);
    let mut rig = Rig::new();
    rig.prepare(&built, None, 0, SIZE);
    let Some(Place::Span { row, .. }) = rig.panel.table.find_move(s.nc3) else {
        panic!("a card");
    };
    let (top, height) = rig.panel.table.row_span(row).expect("row");
    // The card of the first variation is one level in.
    let header = Point::new(
        super::style::PADDING + super::table_layout::CARD_STEP + 2.0,
        top + height / 2.0,
    );
    assert!(rig.panel.press_left(header));
    assert_eq!(
        rig.panel.release_left(Some(header)).1,
        Some(NotationEvent::ToggleCard(s.nc3))
    );
    // The mouse over the header acts like over the first move: the node of
    // the hover is the first move of the card.
    let _ = rig.panel.pointer(None, None);
    let moved = rig.panel.pointer(Some(header), Some(Point::new(1.0, 1.0)));
    assert!(matches!(
        moved.event,
        Some(NotationEvent::Hover(Some((node, _)))) if node == s.nc3
    ));
    assert_eq!(rig.panel.hover.hit, Some(Hit::Card(s.nc3)));
    // Folding hides the rest of the card.
    rig.collapsed.insert(s.nc3);
    rig.prepare(&built, None, 0, SIZE);
    assert!(rig.panel.table.find_move(s.nf6).is_none());
    // Now the whole card folds back.
    let anywhere = Point::new(150.0, top + 8.0);
    assert!(rig.panel.press_left(anywhere));
    assert_eq!(
        rig.panel.release_left(Some(anywhere)).1,
        Some(NotationEvent::ToggleCard(s.nc3))
    );
}

#[test]
fn a_new_reveal_token_scrolls_to_the_current_move_and_shapes_its_row() {
    let (game, nodes) = long_game(300);
    let built = build(&game);
    let size = Size::new(400.0, 200.0);
    let mut rig = Rig::new();
    rig.prepare(&built, None, 0, size);
    let last = *nodes.last().expect("a last move");
    rig.prepare(&built, Some(last), 1, size);
    let offset = rig.panel.scroll.offset();
    assert!(offset > 1000.0, "the last move is far down: {offset}");
    let rect = rig.panel.current.bounds().expect("a pill");
    assert!(rect.y >= offset && rect.y + rect.height <= offset + size.height);
    let Some(Place::Cell { row, .. }) = rig.panel.table.find_move(last) else {
        panic!("a cell");
    };
    assert!(rig.panel.table.is_shaped(row));
    // The user scrolls away; the same token does not bring it back.
    rig.panel.scroll.set_offset(0.0);
    rig.prepare(&built, Some(last), 1, size);
    assert_eq!(rig.panel.scroll.offset(), 0.0);
    // The start of the game goes back to the top.
    rig.prepare(&built, Some(last), 2, size);
    assert!(rig.panel.scroll.offset() > 0.0);
    rig.prepare(&built, None, 3, size);
    assert_eq!(rig.panel.scroll.offset(), 0.0);
}

#[test]
fn the_wheel_scrolls_three_rows_a_notch_and_shapes_the_rows_that_arrive() {
    let (game, _) = long_game(300);
    let built = build(&game);
    let mut rig = Rig::new();
    rig.prepare(&built, None, 0, Size::new(400.0, 200.0));
    let row_h = rig.panel.table.row_height();
    assert!(rig.panel.wheel(-1.0, None));
    assert_eq!(rig.panel.scroll.offset(), 3.0 * row_h);
    let shaped = rig.panel.table.rows_shaped();
    assert!(rig.panel.wheel(-100.0, None));
    assert!(rig.panel.table.rows_shaped() > shaped);
    let offset = rig.panel.scroll.offset();
    let visible = rig.panel.table.visible(offset, offset + 200.0);
    for row in visible {
        assert!(
            !rig.panel.table.is_move_row(row) || rig.panel.table.is_shaped(row),
            "row {row} is on screen and has no text"
        );
    }
}

#[test]
fn dragging_the_scroll_bar_shapes_the_rows_it_arrives_at() {
    let (game, _) = long_game(300);
    let built = build(&game);
    let mut rig = Rig::new();
    rig.prepare(&built, None, 0, Size::new(400.0, 200.0));
    let (_, height) = rig.panel.thumb().expect("content overflows");
    assert!(rig.panel.press_left(Point::new(400.0 - 2.0, height / 2.0)));
    assert!(rig.panel.drag_to(10_000.0));
    let offset = rig.panel.scroll.offset();
    assert_eq!(offset, rig.panel.scroll.max_offset());
    let last_moves = (0..rig.panel.table.len())
        .rev()
        .find(|&row| rig.panel.table.is_move_row(row))
        .expect("a move row");
    assert!(rig.panel.table.is_shaped(last_moves));
}

#[test]
fn a_changed_table_forgets_the_hover_and_a_text_document_can_replace_it() {
    let s = sample();
    let built = build(&s.game);
    let mut rig = Rig::new();
    rig.prepare(&built, None, 0, SIZE);
    let at = rig.center_of(s.e4);
    let _ = rig.panel.pointer(Some(at), None);
    assert!(rig.panel.hover.hit.is_some());
    // The same widget, now over the text layout of the same document.
    let params = Params {
        doc: &built.doc,
        labels: &rig.labels,
        style: &rig.style,
        current: None,
        menu_target: None,
        reveal_token: 0,
        table: None,
    };
    rig.panel.prepare(&params, SIZE);
    assert!(!rig.panel.table_mode);
    assert_eq!(rig.panel.hover.hit, None);
    assert!(!rig.panel.cache.is_empty());
}

#[test]
fn the_menu_target_is_outlined_on_a_cell_and_soft_on_text() {
    let s = sample();
    let built = build(&s.game);
    let mut rig = Rig::new();
    let params = |target| Params {
        doc: &built.doc,
        labels: &rig.labels,
        style: &rig.style,
        current: None,
        menu_target: Some(target),
        reveal_token: 0,
        table: Some(TableParams {
            table: &built.table,
            collapsed: Some(&rig.collapsed),
        }),
    };
    let first = params(s.e5);
    rig.panel.prepare(&first, SIZE);
    assert!(rig.panel.target.cell.is_some());
    assert_eq!(rig.panel.target.rects.len(), 1);
    let second = params(s.nf6);
    rig.panel.prepare(&second, SIZE);
    assert!(rig.panel.target.cell.is_none());
    assert!(!rig.panel.target.rects.is_empty());
}
