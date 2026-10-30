//! M2-5b player strips and the column of the board (spec D230 to D238 and
//! rules 8 to 11): the widgets are built over view-models and found by text
//! or by widget id in a headless simulator.

#![allow(clippy::float_cmp, clippy::unnecessary_wraps)]

mod m2_4_common;

use iced::{Settings, Size, widget};
use iced_test::Simulator;
use iced_test::selector::Candidate;
use m2_4_common::render_lock;
use mychess_app::{MaterialMode, PlayerStripView, PlayerStrips, PlayersPrefs, RatingView};
use mychess_core::{Color, Position, RatingType, Role};
use mychess_gui::Message;
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::widgets::board::geometry::{
    BoardLayout, CoordsMode, LayoutOptions, Orientation, StripMode, column_metrics,
};
use mychess_gui::widgets::board::marks::MarkBrush;
use mychess_gui::widgets::board::pieces::PieceSet;
use mychess_gui::widgets::board::{BoardCaches, BoardData, BoardOptions, BoardWidget};
use mychess_gui::windows::board_column::{
    BOTTOM_STRIP_ID, BoardColumn, CANVAS_ID, TOP_STRIP_ID, policy_of,
};
use mychess_gui::windows::player_strip::{StripContext, view};

fn white() -> PlayerStripView {
    let mut v = PlayerStripView::new(Color::White);
    v.name = Some("Magnus Carlsen".to_owned());
    v.title = Some("GM".to_owned());
    v.rating = Some(RatingView {
        value: 2850,
        kind: RatingType::Elo,
    });
    v.captured = vec![(Role::Queen, 1), (Role::Pawn, 2)];
    v.advantage = Some(3);
    v.clock = Some(125);
    v.to_move = true;
    v
}

fn black() -> PlayerStripView {
    let mut v = PlayerStripView::new(Color::Black);
    v.name = Some("Hikaru Nakamura".to_owned());
    v.clock = Some(3725);
    v
}

fn context<'a>(
    mode: StripMode,
    prefs: PlayersPrefs,
    clock_slot: bool,
    pieces: &'a PieceSet,
    i18n: &'a I18n,
) -> StripContext<'a> {
    StripContext {
        mode,
        width: 480.0,
        prefs,
        show_side_to_move: true,
        clock_slot,
        countdown: true,
        clock_chars: 5,
        pieces,
        i18n,
    }
}

/// The strip in a window of its own, sending `7` when the placeholder of an
/// unknown player is clicked.
fn strip_sim(strip: &PlayerStripView, ctx: &StripContext<'_>) -> Simulator<'static, u8> {
    let element = view::<u8>(strip, ctx, 7, widget::tooltip::Position::Top);
    Simulator::with_size(Settings::default(), Size::new(480.0, 60.0), element)
}

fn has(sim: &mut Simulator<'static, u8>, text: &str) -> bool {
    sim.find(text).is_ok()
}

#[test]
fn full_strip_shows_title_name_rating_points_and_clock() {
    let _guard = render_lock();
    let (pieces, i18n) = (PieceSet::default(), I18n::new(Language::EnUs));
    let ctx = context(
        StripMode::Full,
        PlayersPrefs::default(),
        true,
        &pieces,
        &i18n,
    );
    let mut sim = strip_sim(&white(), &ctx);
    for text in ["Magnus Carlsen", "GM", "2850", "+3", "2:05"] {
        assert!(has(&mut sim, text), "{text}");
    }
}

#[test]
fn clock_over_an_hour_has_hours_and_a_missing_value_shows_the_placeholder() {
    let _guard = render_lock();
    let (pieces, i18n) = (PieceSet::default(), I18n::new(Language::EnUs));
    let ctx = context(
        StripMode::Full,
        PlayersPrefs::default(),
        true,
        &pieces,
        &i18n,
    );
    assert!(has(&mut strip_sim(&black(), &ctx), "1:02:05"));
    let mut no_value = black();
    no_value.clock = None;
    assert!(has(&mut strip_sim(&no_value, &ctx), "--:--"));
}

#[test]
fn without_clock_data_or_with_clocks_off_nothing_is_drawn_for_the_clock() {
    let _guard = render_lock();
    let (pieces, i18n) = (PieceSet::default(), I18n::new(Language::EnUs));
    let prefs = PlayersPrefs::default();
    // The game has no clocks at all.
    let ctx = context(StripMode::Full, prefs, false, &pieces, &i18n);
    let mut nothing = black();
    nothing.clock = None;
    let mut sim = strip_sim(&nothing, &ctx);
    assert!(!has(&mut sim, "--:--"), "no placeholder without data");
    // Clocks switched off.
    let off = PlayersPrefs {
        show_clocks: false,
        ..prefs
    };
    let ctx = context(StripMode::Full, off, true, &pieces, &i18n);
    let mut sim = strip_sim(&white(), &ctx);
    assert!(has(&mut sim, "Magnus Carlsen"));
    assert!(!has(&mut sim, "2:05"));
}

#[test]
fn ratings_off_hides_only_the_rating() {
    let _guard = render_lock();
    let (pieces, i18n) = (PieceSet::default(), I18n::new(Language::EnUs));
    let prefs = PlayersPrefs {
        show_ratings: false,
        ..PlayersPrefs::default()
    };
    let ctx = context(StripMode::Full, prefs, true, &pieces, &i18n);
    let mut sim = strip_sim(&white(), &ctx);
    assert!(has(&mut sim, "Magnus Carlsen") && has(&mut sim, "GM"));
    assert!(!has(&mut sim, "2850"));
}

#[test]
fn rating_of_another_kind_carries_its_label() {
    let _guard = render_lock();
    let (pieces, i18n) = (PieceSet::default(), I18n::new(Language::PtBr));
    let mut rapid = white();
    rapid.rating = Some(RatingView {
        value: 2700,
        kind: RatingType::Rapid,
    });
    let ctx = context(
        StripMode::Full,
        PlayersPrefs::default(),
        true,
        &pieces,
        &i18n,
    );
    assert!(has(&mut strip_sim(&rapid, &ctx), "2700 R\u{e1}pido"));
}

#[test]
fn long_names_are_cut_with_an_ellipsis_and_the_clock_stays() {
    let _guard = render_lock();
    let (pieces, i18n) = (PieceSet::default(), I18n::new(Language::EnUs));
    let mut long = white();
    long.name = Some("Maximilian Wolfgang von Hohenzollern-Sigmaringen".to_owned());
    let ctx = context(
        StripMode::Full,
        PlayersPrefs::default(),
        true,
        &pieces,
        &i18n,
    );
    let mut sim = strip_sim(&long, &ctx);
    assert!(!has(
        &mut sim,
        "Maximilian Wolfgang von Hohenzollern-Sigmaringen"
    ));
    assert!(has(&mut sim, "2:05"));
    let cut = sim
        .find(|c: Candidate<'_>| match c {
            Candidate::Text { content, .. } if content.starts_with("Maximilian") => {
                Some(content.to_owned())
            }
            _ => None,
        })
        .expect("the cut name");
    assert!(cut.ends_with('\u{2026}'), "{cut}");
}

#[test]
fn compact_strip_keeps_name_points_and_clock() {
    let _guard = render_lock();
    let (pieces, i18n) = (PieceSet::default(), I18n::new(Language::EnUs));
    let ctx = context(
        StripMode::Compact,
        PlayersPrefs::default(),
        true,
        &pieces,
        &i18n,
    );
    let mut sim = strip_sim(&white(), &ctx);
    for text in ["Magnus Carlsen", "+3", "2:05"] {
        assert!(has(&mut sim, text), "{text}");
    }
}

#[test]
fn unknown_player_shows_the_placeholder_and_clicking_it_sends_the_message() {
    let _guard = render_lock();
    let (pieces, i18n) = (PieceSet::default(), I18n::new(Language::EnUs));
    let ctx = context(
        StripMode::Full,
        PlayersPrefs::default(),
        false,
        &pieces,
        &i18n,
    );
    let unknown = PlayerStripView::new(Color::Black);
    let mut sim = strip_sim(&unknown, &ctx);
    assert!(sim.click("?").is_ok());
    let messages: Vec<u8> = sim.into_messages().collect();
    assert_eq!(messages, vec![7]);
}

#[test]
fn a_strip_without_data_shows_no_garbage() {
    let _guard = render_lock();
    let (pieces, i18n) = (PieceSet::default(), I18n::new(Language::EnUs));
    let ctx = context(
        StripMode::Full,
        PlayersPrefs::default(),
        false,
        &pieces,
        &i18n,
    );
    let mut sim = strip_sim(&PlayerStripView::new(Color::White), &ctx);
    assert!(has(&mut sim, "?"));
    for text in ["+0", "0:00", "--:--", "None", "0"] {
        assert!(!has(&mut sim, text), "{text}");
    }
}

// ---- the column of the board ----

struct Column {
    position: Position,
    strips: PlayerStrips,
    brush: MarkBrush,
    pieces: PieceSet,
    caches: BoardCaches,
    i18n: I18n,
}

impl Column {
    fn new() -> Column {
        let strips = PlayerStrips {
            white: white(),
            black: black(),
            clock_slot: true,
        };
        Column {
            position: Position::new(),
            strips,
            brush: MarkBrush::default(),
            pieces: PieceSet::default(),
            caches: BoardCaches::default(),
            i18n: I18n::new(Language::EnUs),
        }
    }

    fn element(
        &self,
        size: Size,
        prefs: PlayersPrefs,
        coords: CoordsMode,
        orientation: Orientation,
    ) -> iced::Element<'_, Message> {
        let data = BoardData {
            position: &self.position,
            last_move: None,
            marks: &[],
            orientation,
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
            prefs,
            countdown: true,
            show_side_to_move: true,
            coords,
            orientation,
            pieces: &self.pieces,
            i18n: &self.i18n,
        }
        .layout(size)
    }
}

fn bounds(sim: &mut Simulator<'_, Message>, id: &'static str) -> Option<iced::Rectangle> {
    sim.find(widget::Id::new(id)).ok().map(|t| t.bounds())
}

#[test]
fn strips_are_exactly_as_wide_as_the_board_at_three_window_sizes() {
    let _guard = render_lock();
    for (w, h) in [(560.0, 480.0), (900.0, 700.0), (1400.0, 900.0)] {
        for coords in [CoordsMode::Inside, CoordsMode::Outside] {
            let column = Column::new();
            let size = Size::new(w, h);
            let element = column.element(size, PlayersPrefs::default(), coords, Orientation::White);
            let mut sim = Simulator::with_size(Settings::default(), size, element);
            let metrics =
                column_metrics(size, coords, policy_of(PlayersPrefs::default())).expect("fits");
            assert_ne!(metrics.mode, StripMode::Hidden, "{w}x{h}");
            let canvas = bounds(&mut sim, CANVAS_ID).expect("canvas");
            let top = bounds(&mut sim, TOP_STRIP_ID).expect("top strip");
            let bottom = bounds(&mut sim, BOTTOM_STRIP_ID).expect("bottom strip");
            let layout = BoardLayout::compute(canvas.size(), LayoutOptions { coords })
                .expect("canvas layout");
            let board_x = canvas.x + layout.board.x;
            assert_eq!(canvas.size(), metrics.canvas, "{w}x{h} {coords:?}");
            assert_eq!(layout.s, metrics.s);
            for strip in [top, bottom] {
                assert_eq!(strip.width, layout.board.width, "{w}x{h} {coords:?}");
                assert_eq!(strip.x, board_x, "{w}x{h} {coords:?}");
                assert_eq!(strip.height, metrics.strip_height);
            }
            assert!(top.y + top.height <= canvas.y && bottom.y >= canvas.y + canvas.height);
        }
    }
}

#[test]
fn black_is_at_the_top_by_default_and_the_flip_swaps_the_strips() {
    let _guard = render_lock();
    let size = Size::new(900.0, 700.0);
    let ys = |orientation| {
        let column = Column::new();
        let element = column.element(
            size,
            PlayersPrefs::default(),
            CoordsMode::Inside,
            orientation,
        );
        let mut sim = Simulator::with_size(Settings::default(), size, element);
        let y_of = |sim: &mut Simulator<'_, Message>, name: &'static str| {
            sim.find(name).expect("name").bounds().y
        };
        (
            y_of(&mut sim, "Magnus Carlsen"),
            y_of(&mut sim, "Hikaru Nakamura"),
        )
    };
    let (white_y, black_y) = ys(Orientation::White);
    assert!(black_y < white_y, "Black on top: {black_y} < {white_y}");
    let (white_y, black_y) = ys(Orientation::Black);
    assert!(white_y < black_y, "White on top after the flip");
}

#[test]
fn hidden_strips_give_the_board_the_whole_height() {
    let _guard = render_lock();
    let size = Size::new(900.0, 700.0);
    let off = PlayersPrefs {
        show_strips: false,
        ..PlayersPrefs::default()
    };
    let column = Column::new();
    let element = column.element(size, off, CoordsMode::Inside, Orientation::White);
    let mut sim = Simulator::with_size(Settings::default(), size, element);
    assert!(bounds(&mut sim, TOP_STRIP_ID).is_none());
    assert!(bounds(&mut sim, BOTTOM_STRIP_ID).is_none());
    assert_eq!(bounds(&mut sim, CANVAS_ID).expect("canvas").height, 696.0);
}

#[test]
fn a_small_panel_gets_compact_strips_and_a_tiny_one_none() {
    let _guard = render_lock();
    let compact = column_metrics(
        Size::new(600.0, 420.0),
        CoordsMode::Inside,
        policy_of(PlayersPrefs::default()),
    )
    .expect("fits");
    assert_eq!(compact.mode, StripMode::Compact);
    let tiny = column_metrics(
        Size::new(300.0, 200.0),
        CoordsMode::Inside,
        policy_of(PlayersPrefs::default()),
    )
    .expect("fits");
    assert_eq!(tiny.mode, StripMode::Hidden);
    let size = Size::new(300.0, 200.0);
    let column = Column::new();
    let element = column.element(
        size,
        PlayersPrefs::default(),
        CoordsMode::Inside,
        Orientation::White,
    );
    let mut sim = Simulator::with_size(Settings::default(), size, element);
    assert!(bounds(&mut sim, TOP_STRIP_ID).is_none());
}

#[test]
fn the_material_mode_is_part_of_the_preferences() {
    assert_eq!(PlayersPrefs::default().material, MaterialMode::Captured);
}
