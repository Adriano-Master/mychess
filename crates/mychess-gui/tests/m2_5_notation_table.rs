//! Headless tests of the notation table with `iced_test` and the real
//! renderer (spec M2-5, D241-D254): pointer scripts in, events out; pixels
//! for the pill and the zebra rows.
//!
//! The coordinates are not guessed: they come from a `TableCache` over the
//! same paragraph type, text style and width as the widget under test.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::float_cmp
)]

mod m2_4_common;
mod render_common;

use std::borrow::Cow;
use std::collections::HashSet;
use std::path::Path;

use iced::advanced::text::Renderer as TextRenderer;
use iced::{Event, Point, Rectangle, Settings, Size, Theme, mouse};
use iced_test::Simulator;
use mychess_app::{NotationLayout, NotationOptions, NotationTable, build_notation};
use mychess_core::{Game, Nag, NodeId, SanMode};
use mychess_gui::theme::notation_colors::NotationPalette;
use mychess_gui::widgets::notation::scroll::BAR_WIDTH;
use mychess_gui::widgets::notation::style::PADDING;
use mychess_gui::widgets::notation::table_layout::{
    CARD_STEP, Mark, PILL_PAD, TableCache, TableInputs,
};
use mychess_gui::widgets::notation::{Hit, Labels, NotationEvent, NotationView, text_style};
use render_common::{SCALE, render};

type Paragraph = <iced::Renderer as TextRenderer>::Paragraph;

const SIZE: Size = Size::new(520.0, 360.0);
const TEXT_SIZE: f32 = 15.0;

fn labels() -> Labels {
    Labels {
        round: "Round".to_owned(),
        position: "Position".to_owned(),
        empty: "No moves yet".to_owned(),
    }
}

fn san(game: &mut Game, parent: NodeId, text: &str) -> NodeId {
    let position = game.position_at(parent).expect("position").clone();
    let parsed = position.parse_san(text, SanMode::Strict).expect("legal");
    game.add_move(parent, parsed.mv).expect("move")
}

/// The moves a test needs to find again.
struct Rich {
    game: Game,
    e4: NodeId,
    e5: NodeId,
    nf3: NodeId,
    nc6: NodeId,
    bb5: NodeId,
    a6: NodeId,
    nf6: NodeId,
    o_o: NodeId,
}

/// 1.e4 {the king pawn} e5 2.Nf3! Nc6 3.Bb5 a6 (3...Nf6 4.O-O) 4.Ba4.
fn rich() -> Rich {
    let mut game = Game::new();
    let root = game.root();
    let e4 = san(&mut game, root, "e4");
    let e5 = san(&mut game, e4, "e5");
    let nf3 = san(&mut game, e5, "Nf3");
    let nc6 = san(&mut game, nf3, "Nc6");
    let bb5 = san(&mut game, nc6, "Bb5");
    let a6 = san(&mut game, bb5, "a6");
    let nf6 = san(&mut game, bb5, "Nf6");
    let o_o = san(&mut game, nf6, "O-O");
    san(&mut game, a6, "Ba4");
    game.set_comment(e4, "the king pawn").expect("comment");
    game.add_nag(nf3, Nag::new(1).expect("nag")).expect("nag");
    Rich {
        game,
        e4,
        e5,
        nf3,
        nc6,
        bb5,
        a6,
        nf6,
        o_o,
    }
}

struct Rig {
    _render: m2_4_common::RenderGuard,
    doc: mychess_app::NotationDoc,
    table: NotationTable,
    labels: Labels,
    theme: Theme,
    collapsed: HashSet<NodeId>,
    cache: TableCache<Paragraph>,
}

impl Rig {
    fn new(game: &Game, theme: Theme) -> Rig {
        let guard = m2_4_common::render_lock();
        let options = NotationOptions {
            layout: NotationLayout::Table,
            ..NotationOptions::default()
        };
        let doc = build_notation(game, &options, 1);
        let table = NotationTable::from_doc(&doc, game, &options);
        let mut rig = Rig {
            _render: guard,
            doc,
            table,
            labels: labels(),
            theme,
            collapsed: HashSet::new(),
            cache: TableCache::default(),
        };
        rig.sync();
        rig
    }

    fn sync(&mut self) {
        let style = text_style(&self.theme, TEXT_SIZE, true, false).with_figurines(true);
        self.cache.sync(&TableInputs {
            table: &self.table,
            doc: &self.doc,
            labels: &self.labels,
            style: &style,
            width: SIZE.width - BAR_WIDTH,
            collapsed: &self.collapsed,
        });
        self.cache.ensure_shaped(0.0, SIZE.height);
    }

    fn rect_of(&mut self, node: NodeId, mark: Mark) -> Rectangle {
        self.cache.move_rects(node, mark).expect("move is shown").0[0]
    }

    fn centre_of(&mut self, node: NodeId) -> Point {
        self.rect_of(node, Mark::Cell).center()
    }

    fn view(&self, current: Option<NodeId>) -> Simulator<'_, NotationEvent> {
        let style = text_style(&self.theme, TEXT_SIZE, true, false).with_figurines(true);
        let view = NotationView::new(&self.doc, &self.labels, style, current, |event| event)
            .table(&self.table, &self.collapsed)
            .into_element();
        Simulator::with_size(Settings::default(), SIZE, view)
    }
}

fn moved(position: Point) -> Event {
    Event::Mouse(mouse::Event::CursorMoved { position })
}

fn press(button: mouse::Button) -> Event {
    Event::Mouse(mouse::Event::ButtonPressed(button))
}

fn dark() -> Theme {
    mychess_gui::theme::resolve(
        mychess_gui::theme::ThemeMode::Dark,
        iced::theme::Mode::None,
        mychess_gui::theme::AccentId::Mychess,
    )
}

fn light() -> Theme {
    mychess_gui::theme::resolve(
        mychess_gui::theme::ThemeMode::Light,
        iced::theme::Mode::None,
        mychess_gui::theme::AccentId::Mychess,
    )
}

#[test]
fn clicking_a_cell_publishes_its_move() {
    let r = rich();
    let mut rig = Rig::new(&r.game, dark());
    for node in [r.e4, r.e5, r.nf3, r.nc6, r.bb5, r.a6] {
        let at = rig.centre_of(node);
        let mut sim = rig.view(None);
        sim.point_at(at);
        sim.simulate([moved(at)]);
        sim.simulate(iced_test::simulator::click());
        let events: Vec<_> = sim.into_messages().collect();
        assert!(
            events.contains(&NotationEvent::Click(node)),
            "{node}: {events:?}"
        );
    }
}

#[test]
fn clicking_the_far_end_of_a_cell_still_hits_it() {
    // The whole column answers, not only the text (D252).
    let r = rich();
    let mut rig = Rig::new(&r.game, dark());
    let cell = rig.rect_of(r.nf3, Mark::Cell);
    let at = Point::new(cell.x + cell.width - 3.0, cell.y + 3.0);
    let mut sim = rig.view(None);
    sim.point_at(at);
    sim.simulate([moved(at)]);
    sim.simulate(iced_test::simulator::click());
    let events: Vec<_> = sim.into_messages().collect();
    assert!(events.contains(&NotationEvent::Click(r.nf3)), "{events:?}");
}

#[test]
fn clicking_the_annotation_of_a_move_goes_to_that_move() {
    let r = rich();
    let mut rig = Rig::new(&r.game, dark());
    let pill = rig.rect_of(r.nf3, Mark::Pill);
    // The `!` is the last thing in the pill, before its right padding.
    let at = Point::new(
        pill.x + pill.width - PILL_PAD - 2.0,
        pill.y + pill.height / 2.0,
    );
    let located = rig
        .cache
        .locate(Point::new(at.x, at.y))
        .expect("something is there");
    assert_eq!(located.hit, Some(Hit::Nag(r.nf3)));
    let mut sim = rig.view(None);
    sim.point_at(at);
    sim.simulate([moved(at)]);
    sim.simulate(iced_test::simulator::click());
    let events: Vec<_> = sim.into_messages().collect();
    assert!(events.contains(&NotationEvent::Click(r.nf3)), "{events:?}");
}

#[test]
fn right_clicking_a_cell_asks_for_the_menu_of_that_move() {
    let r = rich();
    let mut rig = Rig::new(&r.game, dark());
    let at = rig.centre_of(r.e5);
    let mut sim = rig.view(None);
    sim.point_at(at);
    sim.simulate([moved(at)]);
    sim.simulate([press(mouse::Button::Right)]);
    let events: Vec<_> = sim.into_messages().collect();
    assert!(
        events.iter().any(|e| matches!(
            e,
            NotationEvent::Menu { hit: Some(Hit::Move(n)), .. } if *n == r.e5
        )),
        "{events:?}"
    );
}

#[test]
fn hovering_a_cell_reports_it() {
    let r = rich();
    let mut rig = Rig::new(&r.game, dark());
    let at = rig.centre_of(r.bb5);
    let mut sim = rig.view(None);
    sim.point_at(at);
    sim.simulate([moved(at)]);
    let events: Vec<_> = sim.into_messages().collect();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, NotationEvent::Hover(Some((n, _))) if *n == r.bb5)),
        "{events:?}"
    );
}

#[test]
fn clicking_a_move_inside_a_variation_card_goes_there() {
    let r = rich();
    let mut rig = Rig::new(&r.game, dark());
    for node in [r.nf6, r.o_o] {
        let at = rig.centre_of(node);
        let mut sim = rig.view(None);
        sim.point_at(at);
        sim.simulate([moved(at)]);
        sim.simulate(iced_test::simulator::click());
        let events: Vec<_> = sim.into_messages().collect();
        assert!(
            events.contains(&NotationEvent::Click(node)),
            "{node}: {events:?}"
        );
    }
}

#[test]
fn clicking_the_header_of_a_card_asks_to_fold_it_and_a_folded_card_hides_its_moves() {
    let r = rich();
    let mut rig = Rig::new(&r.game, dark());
    let first = rig.rect_of(r.nf6, Mark::Pill);
    // The gutter is at the left edge of the card (one level in), on its
    // first line.
    let at = Point::new(PADDING + CARD_STEP + 2.0, first.y + first.height / 2.0);
    let mut sim = rig.view(None);
    sim.point_at(at);
    sim.simulate([moved(at)]);
    sim.simulate(iced_test::simulator::click());
    let events: Vec<_> = sim.into_messages().collect();
    assert!(
        events.contains(&NotationEvent::ToggleCard(r.nf6)),
        "{events:?}"
    );

    rig.collapsed.insert(r.nf6);
    rig.sync();
    assert!(rig.cache.find_move(r.o_o).is_none(), "4.O-O is folded away");
    assert!(rig.cache.find_move(r.nf6).is_some());
}

#[test]
fn the_current_move_is_a_pill_in_the_accent_color() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let r = rich();
    let theme = dark();
    let mut rig = Rig::new(&r.game, theme.clone());
    let pill = rig.rect_of(r.nf3, Mark::Pill);
    let palette = NotationPalette::of(&theme);
    let want = to_bytes(palette.pill);
    let style = text_style(&theme, TEXT_SIZE, true, false).with_figurines(true);
    let element = |current| {
        NotationView::new(&rig.doc, &rig.labels, style, current, |event| event)
            .table(&rig.table, &rig.collapsed)
            .into_element()
    };
    let with = render::<NotationEvent>(element(Some(r.nf3)), SIZE);
    // Left of the text, inside the padding of the pill.
    let probe = |shot: &render_common::Shot, x: f32, y: f32| {
        shot.pixel((x * SCALE) as u32, (y * SCALE) as u32)
    };
    let (x, y) = (pill.x + 2.0, pill.y + pill.height / 2.0);
    assert_eq!(probe(&with, x, y), want, "the pill behind Nf3");
    // The cursor on another move: the same place is not accent.
    let other = render::<NotationEvent>(element(Some(r.e4)), SIZE);
    assert_ne!(probe(&other, x, y), want);
    // At the start of the game nothing has a pill.
    let start = render::<NotationEvent>(element(None), SIZE);
    assert_ne!(probe(&start, x, y), want);
}

#[test]
fn even_move_rows_have_the_zebra_background() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let r = rich();
    let theme = dark();
    let mut rig = Rig::new(&r.game, theme.clone());
    let palette = NotationPalette::of(&theme);
    let style = text_style(&theme, TEXT_SIZE, true, false).with_figurines(true);
    let element = NotationView::new(&rig.doc, &rig.labels, style, None, |event| event)
        .table(&rig.table, &rig.collapsed)
        .into_element();
    let shot = render::<NotationEvent>(element, SIZE);
    // The right end of a row, past the cells and the time column.
    let x = SIZE.width - BAR_WIDTH - 4.0;
    let color_at = |rig: &mut Rig, node: NodeId| {
        let rect = rig.rect_of(node, Mark::Cell);
        shot.pixel((x * SCALE) as u32, ((rect.y + 3.0) * SCALE) as u32)
    };
    // 1.e4 is the first move row (plain); 1...e5 starts the second (zebra).
    let first = color_at(&mut rig, r.e4);
    let second = color_at(&mut rig, r.e5);
    assert_eq!(second, to_bytes(palette.zebra));
    assert_ne!(first, to_bytes(palette.zebra));
}

fn to_bytes(color: iced::Color) -> [u8; 4] {
    [
        (color.r * 255.0).round() as u8,
        (color.g * 255.0).round() as u8,
        (color.b * 255.0).round() as u8,
        255,
    ]
}

#[test]
fn the_table_renders_in_both_themes() {
    let r = rich();
    for theme in [dark(), light()] {
        let rig = Rig::new(&r.game, theme.clone());
        let mut sim = rig.view(Some(r.nf3));
        assert!(sim.snapshot(&theme).is_ok());
    }
}

/// Writes the table as PNG files, for looking at it. Run by hand:
/// `MYCHESS_SNAPSHOT_DIR=/some/dir cargo test -p mychess-gui --test
/// m2_5_notation_table -- --ignored dump`.
#[test]
#[ignore = "writes images to $MYCHESS_SNAPSHOT_DIR; run by hand"]
fn dump_the_table_as_images() {
    let Some(dir) = std::env::var_os("MYCHESS_SNAPSHOT_DIR") else {
        return;
    };
    let fonts = [
        "NotoSans-Regular",
        "NotoSans-Bold",
        "NotoSansMath-Regular",
        "NotoSansSymbols2-Regular",
    ]
    .map(|name| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/fonts")
            .join(format!("{name}.ttf"));
        Cow::Owned(std::fs::read(path).expect("font file"))
    });
    let r = rich();
    for (name, theme) in [("dark", dark()), ("light", light())] {
        let rig = Rig::new(&r.game, theme.clone());
        let style = text_style(&theme, TEXT_SIZE, true, false).with_figurines(true);
        let view = NotationView::new(&rig.doc, &rig.labels, style, Some(r.nf3), |event| event)
            .table(&rig.table, &rig.collapsed)
            .into_element();
        let settings = Settings {
            fonts: fonts.to_vec(),
            default_font: iced::Font::with_name("Noto Sans"),
            ..Settings::default()
        };
        let mut sim = Simulator::with_size(settings, SIZE, view);
        let snapshot = sim.snapshot(&theme).expect("snapshot");
        let path = Path::new(&dir).join(format!("table-{name}"));
        // `matches_image` only writes a file that does not exist yet.
        for old in ["png"].map(|ext| path.with_file_name(format!("table-{name}-wgpu.{ext}"))) {
            drop(std::fs::remove_file(old));
        }
        snapshot.matches_image(path).expect("image written");
    }
}

// ---- measurements (run by hand, in release mode: `cargo test --release -p
// mychess-gui --test m2_5_notation_table -- --ignored --nocapture measure`) ----

/// 300 plies on the main line and 150 variations of 4 plies each (the game of
/// the performance goals, D253).
fn big_game() -> (Game, Vec<NodeId>) {
    let mut game = Game::new();
    let mut at = game.root();
    let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
    let mut line = Vec::new();
    for ply in 0..300 {
        let mv = game
            .position_at(at)
            .expect("position")
            .parse_uci(cycle[ply % 4])
            .expect("legal");
        let next = game.add_move(at, mv).expect("move");
        // The variation is added after the main move, so it stays a variation.
        if ply % 2 == 0 {
            let mut v = at;
            for uci in ["a2a3", "a7a6", "a3a4", "a6a5"] {
                let mv = game
                    .position_at(v)
                    .expect("position")
                    .parse_uci(uci)
                    .expect("legal");
                v = game.add_move(v, mv).expect("move");
            }
        }
        at = next;
        line.push(at);
    }
    (game, line)
}

#[test]
#[ignore = "timing report, run by hand in release mode"]
#[allow(clippy::too_many_lines)]
fn measure_the_table() {
    use std::time::Instant;
    let _guard = m2_4_common::render_lock();
    let (game, line) = big_game();
    let options = NotationOptions {
        layout: NotationLayout::Table,
        ..NotationOptions::default()
    };
    let t = Instant::now();
    let doc = build_notation(&game, &options, 1);
    println!(
        "build_notation: {:?} ({} blocks)",
        t.elapsed(),
        doc.blocks.len()
    );
    let t = Instant::now();
    let table = NotationTable::from_doc(&doc, &game, &options);
    println!(
        "NotationTable::from_doc: {:?} ({} rows)",
        t.elapsed(),
        table.rows.len()
    );

    let theme = dark();
    let style = text_style(&theme, TEXT_SIZE, true, false).with_figurines(true);
    let labels = labels();
    let collapsed = HashSet::new();
    let inputs = TableInputs {
        table: &table,
        doc: &doc,
        labels: &labels,
        style: &style,
        width: 520.0,
        collapsed: &collapsed,
    };
    // The first shaping of a process loads the system fonts: not part of the
    // frame being measured.
    let mut warm = TableCache::<Paragraph>::default();
    let t = Instant::now();
    warm.sync(&inputs);
    println!(
        "first sync of the process (loads the fonts): {:?}",
        t.elapsed()
    );
    let mut cache = TableCache::<Paragraph>::default();
    let t = Instant::now();
    cache.sync(&inputs);
    println!(
        "sync (rows, cards and comments shaped; move rows lazy): {:?} ({} paragraphs)",
        t.elapsed(),
        cache.shaped()
    );
    let t = Instant::now();
    cache.ensure_shaped(0.0, 800.0);
    println!(
        "first frame, move rows near an 800 px window: {:?} ({} rows, {} paragraphs in all)",
        t.elapsed(),
        cache.rows_shaped(),
        cache.shaped()
    );
    let t = Instant::now();
    for _ in 0..1000 {
        cache.sync(&inputs);
    }
    println!(
        "sync with nothing changed, per call: {:?}",
        t.elapsed() / 1000
    );
    let paragraphs = cache.shaped();
    let t = Instant::now();
    for node in line.iter().take(20) {
        drop(cache.move_rects(*node, Mark::Pill));
    }
    println!(
        "pill of the current move, per cursor move: {:?} ({} paragraphs shaped)",
        t.elapsed() / 20,
        cache.shaped() - paragraphs
    );
    let t = Instant::now();
    for step in 0..100_u16 {
        let top = 800.0 + f32::from(step) * 32.0;
        cache.ensure_shaped(top, top + 800.0);
    }
    println!(
        "scrolling 100 rows, per row: {:?} ({} move rows shaped in all)",
        t.elapsed() / 100,
        cache.rows_shaped()
    );
    let mut edited = game.clone();
    edited
        .add_nag(line[10], Nag::new(1).expect("nag"))
        .expect("nag");
    let mut doc2 = build_notation(&edited, &options, 2);
    doc2.keep_revisions(&doc);
    let mut table2 = NotationTable::from_doc(&doc2, &edited, &options);
    table2.keep_revisions(&table);
    let (rows, paragraphs) = (cache.rows_shaped(), cache.shaped());
    let t = Instant::now();
    cache.sync(&TableInputs {
        table: &table2,
        doc: &doc2,
        ..inputs
    });
    cache.ensure_shaped(0.0, 800.0);
    println!(
        "one NAG edited: {:?} ({} rows, {} paragraphs shaped)",
        t.elapsed(),
        cache.rows_shaped() - rows,
        cache.shaped() - paragraphs
    );
    let paragraphs = cache.shaped();
    let t = Instant::now();
    cache.sync(&TableInputs {
        table: &table2,
        doc: &doc2,
        width: 380.0,
        ..inputs
    });
    println!(
        "width change: {:?} ({} paragraphs shaped, {} laid out again)",
        t.elapsed(),
        cache.shaped() - paragraphs,
        cache.resized()
    );
    let t = Instant::now();
    let mut sim = Simulator::with_size(
        Settings::default(),
        Size::new(520.0, 800.0),
        NotationView::new(&doc, &labels, style, Some(line[5]), |event| event)
            .table(&table, &collapsed)
            .into_element(),
    );
    println!(
        "first layout through the toolkit (build the UI): {:?}",
        t.elapsed()
    );
    let t = Instant::now();
    assert!(sim.snapshot(&theme).is_ok());
    println!(
        "draw and read back one frame (software, 2x): {:?}",
        t.elapsed()
    );
}
