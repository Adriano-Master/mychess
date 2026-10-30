#![allow(clippy::float_cmp, clippy::assert_is_empty)]
//! Tests of the rows of the table with a stand-in paragraph: no renderer, no
//! window. The tests of the widget over the table (pointer, pill, reveal) are
//! in `notation/table_tests.rs`.

use std::collections::HashSet;

use iced::Point;
use mychess_app::{NotationDoc, NotationLayout, NotationOptions, NotationTable, build_notation};
use mychess_core::{Color as Side, Game, Nag, NodeId, Position};
use pretty_assertions::assert_eq;

use super::{
    Body, Columns, FlowKind, MIN_CELL, Mark, ROW_HEIGHT, ROW_HEIGHT_TIMES, TableCache, TableInputs,
    format_time, time_bar_width,
};
use crate::theme::notation_colors::NotationPalette;
use crate::widgets::notation::hit::Hit;
use crate::widgets::notation::mock::MockParagraph;
use crate::widgets::notation::style::{Labels, PADDING, TextStyle};

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

/// 1.e4 e5! 2.Nf3 (2.Nc3 Nf6) 2...Nc6, with a comment after e4.
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

fn options() -> NotationOptions {
    NotationOptions {
        layout: NotationLayout::Table,
        ..NotationOptions::default()
    }
}

struct Built {
    doc: NotationDoc,
    table: NotationTable,
}

fn build(game: &Game, revision: u64) -> Built {
    let opts = options();
    let doc = build_notation(game, &opts, revision);
    let table = NotationTable::from_doc(&doc, game, &opts);
    Built { doc, table }
}

struct Rig {
    cache: TableCache<MockParagraph>,
    labels: Labels,
    style: TextStyle,
    collapsed: HashSet<NodeId>,
}

impl Rig {
    fn new() -> Rig {
        Rig {
            cache: TableCache::default(),
            labels: labels(),
            style: style(),
            collapsed: HashSet::new(),
        }
    }

    fn sync(&mut self, built: &Built, width: f32) -> bool {
        self.cache.sync(&TableInputs {
            table: &built.table,
            doc: &built.doc,
            labels: &self.labels,
            style: &self.style,
            width,
            collapsed: &self.collapsed,
        })
    }

    /// Shapes every move row.
    fn shape_all(&mut self) {
        self.cache.ensure_shaped(0.0, self.cache.content_height());
    }

    fn row_of(&self, node: NodeId) -> usize {
        match self.cache.find_move(node).expect("shown") {
            super::Place::Cell { row, .. } | super::Place::Span { row, .. } => row,
        }
    }
}

const WIDTH: f32 = 400.0;

fn long_game(plies: usize) -> Game {
    let mut game = Game::new();
    let mut at = game.root();
    let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
    for ply in 0..plies {
        at = play(&mut game, at, cycle[ply % 4]);
    }
    game
}

#[test]
fn columns_leave_room_for_the_number_and_never_get_narrower_than_the_minimum() {
    let s = style();
    let wide = Columns::of(400.0, &s, false);
    assert_eq!(wide.number, PADDING);
    assert!(wide.white > wide.number && wide.black == wide.white + wide.cell);
    assert!(wide.time >= wide.black + wide.cell);
    let narrow = Columns::of(60.0, &s, true);
    assert_eq!(narrow.cell, MIN_CELL);
    let with_times = Columns::of(400.0, &s, true);
    assert!(with_times.cell < wide.cell, "the time column takes room");
}

#[test]
fn the_rows_of_the_table_are_stacked_without_gaps_or_overlap() {
    let s = sample();
    let built = build(&s.game, 1);
    let mut rig = Rig::new();
    assert!(rig.sync(&built, WIDTH));
    let mut y = PADDING;
    for row in &rig.cache.rows {
        assert_eq!(row.y, y);
        assert!(row.height > 0.0);
        y += row.height;
    }
    assert_eq!(rig.cache.content_height(), y + PADDING);
    // Move rows have the fixed height.
    for row in &rig.cache.rows {
        if matches!(row.body, Body::Moves { .. }) {
            assert_eq!(row.height, ROW_HEIGHT);
        }
    }
}

#[test]
fn move_rows_alternate_the_zebra_background_among_themselves() {
    let s = sample();
    let built = build(&s.game, 1);
    let mut rig = Rig::new();
    rig.sync(&built, WIDTH);
    let zebra: Vec<bool> = rig
        .cache
        .rows
        .iter()
        .filter(|r| matches!(r.body, Body::Moves { .. }))
        .map(|r| r.zebra)
        .collect();
    assert_eq!(zebra, [false, true, false, true][..zebra.len()]);
    assert!(zebra.len() >= 3);
}

#[test]
fn syncing_shapes_no_move_row_until_it_is_near_the_window() {
    let built = build(&long_game(300), 1);
    let mut rig = Rig::new();
    rig.sync(&built, WIDTH);
    assert_eq!(rig.cache.rows_shaped(), 0);
    let before = rig.cache.shaped();
    // A window of 100 px: it and half a window on each side.
    rig.cache.ensure_shaped(0.0, 100.0);
    let shaped = rig.cache.rows_shaped();
    // 150 px from the top of the content, 32 px a row, a row of slack.
    assert!((4..=7).contains(&shaped), "{shaped} rows shaped");
    assert!(rig.cache.shaped() > before);
    // Asking again costs nothing.
    let paragraphs = rig.cache.shaped();
    rig.cache.ensure_shaped(0.0, 100.0);
    assert_eq!(rig.cache.shaped(), paragraphs);
    // Far down only the rows there are shaped, those above stay.
    rig.cache.ensure_shaped(2000.0, 2100.0);
    assert!(rig.cache.rows_shaped() > shaped);
    assert!(rig.cache.rows_shaped() < 20);
    assert!(rig.cache.is_shaped(0));
    assert!(!rig.cache.is_shaped(40));
}

#[test]
fn the_same_inputs_do_nothing_and_a_new_width_lays_out_without_shaping_move_rows() {
    let s = sample();
    let built = build(&s.game, 1);
    let mut rig = Rig::new();
    rig.sync(&built, WIDTH);
    rig.shape_all();
    let stamp = rig.cache.stamp();
    assert!(!rig.sync(&built, WIDTH), "nothing changed");
    assert_eq!(rig.cache.stamp(), stamp);
    let (rows, paragraphs) = (rig.cache.rows_shaped(), rig.cache.shaped());
    assert!(rig.sync(&built, 220.0));
    assert_eq!(rig.cache.rows_shaped(), rows, "move rows are width-free");
    assert_eq!(rig.cache.shaped(), paragraphs, "no shaping at all");
    assert!(
        rig.cache.resized() > 0,
        "the comment and the card re-wrapped"
    );
    let narrow = rig.cache.content_height();
    rig.sync(&built, 2000.0);
    assert!(rig.cache.content_height() <= narrow);
}

#[test]
fn a_new_text_style_shapes_everything_again() {
    let s = sample();
    let built = build(&s.game, 1);
    let mut rig = Rig::new();
    rig.sync(&built, WIDTH);
    rig.shape_all();
    let rows = rig.cache.rows_shaped();
    rig.style.figurines = false;
    rig.sync(&built, WIDTH);
    assert_eq!(rig.cache.rows_shaped(), rows, "lazily: none yet");
    rig.shape_all();
    assert_eq!(rig.cache.rows_shaped(), rows * 2);
}

#[test]
fn editing_one_nag_shapes_only_its_row() {
    let mut s = sample();
    let first = build(&s.game, 1);
    let mut rig = Rig::new();
    rig.sync(&first, WIDTH);
    rig.shape_all();
    let rows = rig.cache.rows_shaped();
    assert!(rows >= 3);
    s.game
        .add_nag(s.nf3, Nag::new(2).expect("nag"))
        .expect("nag");
    let mut second = build(&s.game, 2);
    second.doc.keep_revisions(&first.doc);
    second.table.keep_revisions(&first.table);
    let nf3_row = {
        let mut probe = Rig::new();
        probe.sync(&second, WIDTH);
        probe.row_of(s.nf3)
    };
    rig.sync(&second, WIDTH);
    rig.shape_all();
    assert_eq!(rig.cache.rows_shaped() - rows, 1, "one row");
    assert!(rig.cache.is_shaped(nf3_row));
}

#[test]
fn folding_a_card_shapes_only_that_card() {
    let s = sample();
    let built = build(&s.game, 1);
    let mut rig = Rig::new();
    rig.sync(&built, WIDTH);
    rig.shape_all();
    let open_height = rig.cache.content_height();
    let (rows, paragraphs) = (rig.cache.rows_shaped(), rig.cache.shaped());
    rig.collapsed.insert(s.nc3);
    assert!(rig.sync(&built, WIDTH));
    assert_eq!(rig.cache.rows_shaped(), rows);
    assert_eq!(rig.cache.shaped() - paragraphs, 1, "the folded card");
    let card = rig
        .cache
        .rows
        .iter()
        .find_map(|r| match &r.body {
            Body::Flow(flow) if matches!(flow.kind, FlowKind::Card { .. }) => Some((r, flow)),
            _ => None,
        })
        .expect("the card");
    assert!(matches!(
        card.1.kind,
        FlowKind::Card {
            collapsed: true,
            toggle: Some(node)
        } if node == s.nc3
    ));
    assert!(rig.cache.content_height() <= open_height);
    // The folded card shows its first move and nothing of the rest.
    assert!(rig.cache.find_move(s.nf6).is_none(), "nf6 is hidden");
    assert!(rig.cache.find_move(s.nc3).is_some(), "the first move stays");
}

#[test]
fn a_game_that_starts_with_black_has_a_row_with_dots_in_the_white_column() {
    let position = Position::from_fen("7k/8/8/8/8/8/4p3/R3K3 b - - 0 1").expect("fen");
    let mut game = Game::from_position(position);
    let first = game
        .position_at(game.root())
        .expect("position")
        .parse_uci("h8g8")
        .expect("move");
    let g8 = game.add_move(game.root(), first).expect("move");
    let built = build(&game, 1);
    let mut rig = Rig::new();
    rig.sync(&built, WIDTH);
    rig.shape_all();
    assert_eq!(
        rig.cache.find_move(g8),
        Some(super::Place::Cell {
            row: 0,
            side: Side::Black
        })
    );
    let Some(Body::Moves {
        shaped: Some(shaped),
        data,
    }) = rig.cache.rows.first().map(|r| &r.body)
    else {
        panic!("a shaped move row");
    };
    assert!(data.white.is_none() && data.black.is_some());
    assert!(shaped.dots.is_some(), "the white column shows the dots");
    assert!(shaped.white.is_none());
    // A row that starts with White has none.
    let normal = build(&sample().game, 1);
    let mut other = Rig::new();
    other.sync(&normal, WIDTH);
    other.shape_all();
    let Some(Body::Moves {
        shaped: Some(shaped),
        ..
    }) = other.cache.rows.first().map(|r| &r.body)
    else {
        panic!("a shaped move row");
    };
    assert!(shaped.dots.is_none());
}

#[test]
fn locating_a_point_in_a_cell_gives_the_move_and_on_its_symbol_the_nag() {
    let s = sample();
    let built = build(&s.game, 1);
    let mut rig = Rig::new();
    rig.sync(&built, WIDTH);
    rig.shape_all();
    let cols = rig.cache.columns();
    let row_of = |node| rig.row_of(node);
    // The cell of e4: any height of the row, anywhere in the column.
    let (top, height) = rig.cache.row_span(row_of(s.e4)).expect("row");
    for dy in [0.5, height / 2.0, height - 0.5] {
        let found = rig
            .cache
            .locate(Point::new(cols.white + cols.cell - 1.0, top + dy))
            .expect("the cell");
        assert_eq!(found.hit, Some(Hit::Move(s.e4)));
    }
    // The cell of e5 (Black) has an annotation at the end of its text; the
    // column of the move is `Move` outside the symbol.
    let (top, height) = rig.cache.row_span(row_of(s.e5)).expect("row");
    let middle = top + height / 2.0;
    let on_move = rig
        .cache
        .locate(Point::new(cols.black + 8.0, middle))
        .expect("black cell");
    assert_eq!(on_move.hit, Some(Hit::Move(s.e5)));
    // "e5" is two characters of 5 px; the `!` follows after the gap.
    let on_nag = rig
        .cache
        .locate(Point::new(
            cols.black + super::PILL_PAD + 10.0 + 5.0,
            middle,
        ))
        .expect("nag");
    assert_eq!(on_nag.hit, Some(Hit::Nag(s.e5)));
    // The number column and the gap right of the cells are nothing.
    assert!(
        rig.cache
            .locate(Point::new(cols.number + 1.0, middle))
            .is_none()
    );
    assert!(
        rig.cache
            .locate(Point::new(cols.black + cols.cell + 2.0, middle))
            .is_none()
    );
    // Above and below the rows.
    assert!(
        rig.cache
            .locate(Point::new(cols.white + 2.0, 1.0))
            .is_none()
    );
    assert!(
        rig.cache
            .locate(Point::new(
                cols.white + 2.0,
                rig.cache.content_height() + 10.0
            ))
            .is_none()
    );
}

#[test]
fn the_start_of_a_row_without_a_white_move_is_empty_in_the_white_column() {
    // 1.e4 e5 2.Nf3 (2.Nc3 Nf6) 2...Nc6: the continuation row has no white.
    let s = sample();
    let built = build(&s.game, 1);
    let mut rig = Rig::new();
    rig.sync(&built, WIDTH);
    rig.shape_all();
    let row = rig.row_of(s.nc6);
    let (top, height) = rig.cache.row_span(row).expect("row");
    let cols = rig.cache.columns();
    assert!(
        rig.cache
            .locate(Point::new(cols.white + 3.0, top + height / 2.0))
            .is_none(),
        "dots are not clickable"
    );
    let found = rig
        .cache
        .locate(Point::new(cols.black + 3.0, top + height / 2.0))
        .expect("black cell");
    assert_eq!(found.hit, Some(Hit::Move(s.nc6)));
}

#[test]
fn a_card_has_a_header_that_folds_and_text_that_goes_to_its_moves() {
    let s = sample();
    let built = build(&s.game, 1);
    let mut rig = Rig::new();
    rig.sync(&built, WIDTH);
    let card_row = rig.row_of(s.nc3);
    let (top, height) = rig.cache.row_span(card_row).expect("row");
    let level = f32::from(rig.cache.rows[card_row].indent.min(4));
    let card_x = PADDING + level * super::CARD_STEP;
    // The gutter at the left of the card.
    let header = rig
        .cache
        .locate(Point::new(card_x + 2.0, top + height / 2.0))
        .expect("header");
    assert_eq!(header.hit, Some(Hit::Card(s.nc3)));
    // The text of the card: its first move.
    let (rects, _) = rig.cache.move_rects(s.nc3, Mark::Pill).expect("rects");
    let rect = rects[0];
    let on_move = rig
        .cache
        .locate(Point::new(rect.x + 1.0, rect.y + rect.height / 2.0))
        .expect("move");
    assert_eq!(on_move.hit, Some(Hit::Move(s.nc3)));
    // Folded: the whole card is the header.
    rig.collapsed.insert(s.nc3);
    rig.sync(&built, WIDTH);
    let (top, height) = rig.cache.row_span(rig.row_of(s.nc3)).expect("row");
    let anywhere = rig
        .cache
        .locate(Point::new(WIDTH / 2.0, top + height / 2.0))
        .expect("folded card");
    assert_eq!(anywhere.hit, Some(Hit::Card(s.nc3)));
}

#[test]
fn the_rectangles_of_a_move_are_the_cell_or_the_pill_around_its_text() {
    let s = sample();
    let built = build(&s.game, 1);
    let mut rig = Rig::new();
    rig.sync(&built, WIDTH);
    let cols = rig.cache.columns();
    let (cell, place) = rig.cache.move_rects(s.e4, Mark::Cell).expect("cell");
    let (pill, _) = rig.cache.move_rects(s.e4, Mark::Pill).expect("pill");
    assert_eq!(
        place,
        super::Place::Cell {
            row: rig.row_of(s.e4),
            side: Side::White
        }
    );
    // Shaping the row happened because the pill needed the text width.
    assert!(rig.cache.is_shaped(rig.row_of(s.e4)));
    assert_eq!(cell.len(), 1);
    assert_eq!(pill[0].x, cell[0].x);
    assert_eq!(pill[0].x, cols.white);
    assert!(pill[0].width < cell[0].width, "the pill hugs the text");
    // "e4" is two characters of 5 px.
    assert_eq!(pill[0].width, 10.0 + 2.0 * super::PILL_PAD);
    assert_eq!(pill[0].height, cell[0].height);
}

#[test]
fn nothing_panics_for_a_node_the_table_does_not_have() {
    let s = sample();
    let built = build(&s.game, 1);
    let mut rig = Rig::new();
    rig.sync(&built, WIDTH);
    let stranger = Game::new().root();
    assert!(rig.cache.find_move(stranger).is_none());
    assert!(rig.cache.move_rects(stranger, Mark::Pill).is_none());
    assert!(
        rig.cache
            .hit_rects(&crate::widgets::notation::layout::Located {
                entry: 999,
                span: 0,
                hit: None
            })
            .is_empty()
    );
    assert!(rig.cache.locate(Point::new(f32::NAN, f32::NAN)).is_none());
    rig.cache.ensure_row(999);
    assert!(rig.cache.row_span(999).is_none());
}

#[test]
fn the_empty_game_shows_its_message_and_the_header_and_result_rows_are_text() {
    let built = build(&Game::new(), 1);
    let mut rig = Rig::new();
    rig.sync(&built, WIDTH);
    let kinds: Vec<_> = rig
        .cache
        .rows
        .iter()
        .map(|r| match &r.body {
            Body::Flow(f) => f.kind.clone(),
            Body::Moves { .. } => FlowKind::Comment,
        })
        .collect();
    assert!(kinds.contains(&FlowKind::Empty), "{kinds:?}");
    assert!(rig.cache.first_move().is_none());
    assert!(rig.cache.content_height() > 2.0 * PADDING);
}

/// The sample with times on the main line: 5 s, 12 s and 75 s moves.
fn timed() -> (Game, Vec<NodeId>) {
    let mut game = Game::new();
    let root = game.root();
    let e4 = play(&mut game, root, "e2e4");
    let e5 = play(&mut game, e4, "e7e5");
    let nf3 = play(&mut game, e5, "g1f3");
    game.set_comment(e4, "[%emt 0:00:05]").expect("comment");
    game.set_comment(e5, "[%emt 0:00:12]").expect("comment");
    game.set_comment(nf3, "[%emt 0:01:15]").expect("comment");
    (game, vec![e4, e5, nf3])
}

#[test]
fn the_time_column_is_on_only_with_data_and_a_wide_enough_panel() {
    let (game, nodes) = timed();
    let built = build(&game, 1);
    assert!(built.table.has_times);
    let mut rig = Rig::new();
    rig.sync(&built, WIDTH);
    assert!(rig.cache.has_times());
    assert_eq!(rig.cache.row_height(), ROW_HEIGHT_TIMES);
    assert_eq!(rig.cache.max_spent(), 75);
    rig.shape_all();
    // The longest move has the longest bar; the others scale.
    assert_eq!(time_bar_width(75, 75), super::TIME_BAR_MAX);
    assert!(time_bar_width(5, 75) < time_bar_width(12, 75));
    let time_of = |rig: &Rig, node: NodeId, slot: usize| -> Option<u32> {
        match rig.cache.rows.get(rig.row_of(node)).map(|r| &r.body) {
            Some(Body::Moves {
                shaped: Some(shaped),
                ..
            }) => shaped.times[slot].as_ref().map(|t| t.spent),
            _ => None,
        }
    };
    assert_eq!(time_of(&rig, nodes[0], 0), Some(5));
    assert_eq!(time_of(&rig, nodes[1], 1), Some(12));
    assert_eq!(time_of(&rig, nodes[2], 0), Some(75));
    // A narrow panel hides the column and the rows go back to 32.
    rig.sync(&built, 200.0);
    assert!(!rig.cache.has_times());
    assert_eq!(rig.cache.row_height(), ROW_HEIGHT);
    assert_eq!(rig.cache.max_spent(), 0);
    // A game without times never has one.
    let plain = build(&sample().game, 1);
    let mut other = Rig::new();
    other.sync(&plain, WIDTH);
    assert!(!other.cache.has_times());
}

#[test]
fn estimated_times_are_marked_and_the_text_of_a_time_is_compact() {
    assert_eq!(format_time(0), "0s");
    assert_eq!(format_time(59), "59s");
    assert_eq!(format_time(60), "1:00");
    assert_eq!(format_time(75), "1:15");
    assert_eq!(format_time(3725), "62:05");
    assert_eq!(time_bar_width(0, 0), super::TIME_BAR_MIN);
    assert_eq!(time_bar_width(0, 100), super::TIME_BAR_MIN);
    assert_eq!(time_bar_width(1000, 100), super::TIME_BAR_MAX);

    // Only clocks: 3:00 then 2:55 then 2:50 on White's side.
    let mut game = Game::new();
    let root = game.root();
    let e4 = play(&mut game, root, "e2e4");
    let e5 = play(&mut game, e4, "e7e5");
    let nf3 = play(&mut game, e5, "g1f3");
    let nc6 = play(&mut game, nf3, "b8c6");
    let bb5 = play(&mut game, nc6, "f1b5");
    game.set_comment(e4, "[%clk 0:03:00]").expect("comment");
    game.set_comment(nf3, "[%clk 0:02:55]").expect("comment");
    game.set_comment(bb5, "[%clk 0:02:50]").expect("comment");
    let built = build(&game, 1);
    let mut rig = Rig::new();
    rig.sync(&built, WIDTH);
    rig.shape_all();
    let row = rig.row_of(nf3);
    let Some(Body::Moves {
        shaped: Some(shaped),
        ..
    }) = rig.cache.rows.get(row).map(|r| &r.body)
    else {
        panic!("shaped");
    };
    let time = shaped.times[0].as_ref().expect("an estimate");
    assert!(time.derived);
    assert_eq!(time.spent, 5);
}
