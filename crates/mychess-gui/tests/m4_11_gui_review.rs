//! M4-11 (GUI): the arrow of the best move on the board of the guided review
//! and its option (spec `docs/specs/m4-11-revisao-animada.md`, sections 3 to
//! 5, D1665, D1667, D1677). Headless; the engine answers with synthetic
//! events and the timers of the walk are delivered by hand.
#![allow(
    clippy::float_cmp,
    clippy::assert_is_empty,
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

mod m4_68_gui_common;
mod render_common;

use iced::{Event, Point, Settings, Size, mouse};
use iced_test::Simulator;
use m4_68_gui_common::*;
use mychess_app::report::{ReportMessage, ReviewStep, SideFilter};
use mychess_app::{ArrowKind, EngineArrow, LanguagePref};
use mychess_core::{Color, Position, Square};
use mychess_gui::i18n::Language;
use mychess_gui::widgets::board::animation::animation_between;
use mychess_gui::widgets::board::geometry::BoardLayout;
use mychess_gui::widgets::board::interaction::BoardEvent;
use mychess_gui::widgets::board::marks::{MarkBrush, engine_arrow_geometry};
use mychess_gui::widgets::board::pieces::PieceSet;
use mychess_gui::widgets::board::{BoardCaches, BoardData, BoardOptions, BoardWidget};
use render_common::{SCALE, Shot, render};

const BOARD: Size = Size::new(800.0, 800.0);
const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const PLAN: [(i32, &[&str]); 8] = [
    (30, &["e2e4"]),
    (30, &["e7e5"]),
    (30, &["g1f3"]),
    (30, &["b8c6"]),
    (30, &["g1f3"]),
    (-120, &["g7g6"]),
    (3000, &["h5f7"]),
    (3000, &[]),
];

fn sq(name: &str) -> Square {
    name.parse().expect("square")
}

fn review_arrow(from: &str, to: &str) -> EngineArrow {
    EngineArrow {
        from: sq(from),
        to: sq(to),
        rank: 1,
        kind: ArrowKind::ReviewBest,
        side: Color::White,
        label: None,
    }
}

// ---- the board widget ----

fn with_board<R>(
    arrows: &[EngineArrow],
    animated: bool,
    f: impl FnOnce(iced::Element<'_, BoardEvent>, &BoardLayout) -> R,
) -> R {
    let mut position = Position::from_fen(START).unwrap();
    let mut anim = None;
    if animated {
        let old = position.clone();
        let mv = position.parse_uci("e2e4").unwrap();
        position.play(mv).unwrap();
        anim = animation_between(&old, &position);
    }
    let brush = MarkBrush::default();
    let pieces = PieceSet::default();
    let mut caches = BoardCaches::default();
    caches.sync_arrows(
        arrows,
        mychess_gui::widgets::board::geometry::Orientation::White,
    );
    let options = BoardOptions::default();
    let layout = BoardLayout::compute(BOARD, options.layout_options()).expect("fits");
    let data = BoardData {
        position: &position,
        last_move: None,
        marks: &[],
        orientation: mychess_gui::widgets::board::geometry::Orientation::White,
        options,
        accepts_moves: true,
        brush: &brush,
        pieces: &pieces,
        animation: anim.as_ref().map(|a| (a, 0.5)),
    };
    let widget = BoardWidget::new(data, &caches).with_engine_arrows(arrows, true);
    f(widget.into_element(), &layout)
}

fn shot(arrows: &[EngineArrow], animated: bool) -> Shot {
    let _guard = common::render_lock();
    with_board(arrows, animated, |e, _| render(e, BOARD))
}

fn differing_pixels(a: &Shot, b: &Shot) -> usize {
    let side = (BOARD.width * SCALE) as u32;
    let mut n = 0;
    for y in 0..side {
        for x in 0..side {
            if a.pixel(x, y) != b.pixel(x, y) {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn a_review_arrow_is_painted_on_the_board() {
    let arrows = [review_arrow("g1", "f3")];
    assert!(differing_pixels(&shot(&arrows, false), &shot(&[], false)) > 0);
}

#[test]
fn a_review_arrow_is_not_painted_while_a_piece_slides() {
    let arrows = [review_arrow("g1", "f3")];
    assert_eq!(differing_pixels(&shot(&arrows, true), &shot(&[], true)), 0);
}

#[test]
fn a_click_on_the_head_of_the_review_arrow_plays_nothing() {
    let arrow = review_arrow("e2", "e4");
    let _guard = common::render_lock();
    let events: Vec<BoardEvent> = with_board(&[arrow], false, |element, layout| {
        let geometry = engine_arrow_geometry(
            layout,
            arrow.from,
            arrow.to,
            mychess_gui::widgets::board::geometry::Orientation::White,
            0.14,
        )
        .expect("geometry");
        let [p, q, r] = geometry.head;
        let tip = Point::new((p.x + q.x + r.x) / 3.0, (p.y + q.y + r.y) / 3.0);
        let mut sim = Simulator::with_size(Settings::default(), BOARD, element);
        sim.point_at(tip);
        sim.simulate([
            Event::Mouse(mouse::Event::CursorMoved { position: tip }),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ]);
        sim.into_messages().collect()
    });
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, BoardEvent::PlayEngineArrow { .. })),
        "{events:?}"
    );
}

// ---- the review in the window ----

fn report_rig(language: LanguagePref) -> Rig {
    let mut rig = Rig::with_language(language);
    rig.set_game(game_of(&SCHOLAR));
    let run = uci_run(&mut rig);
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Report)));
    let mut fx = up(&mut rig, AppMessage::Report(ReportMessage::Generate));
    let mut searches = Searches::new();
    for k in 0..30 {
        if !rig.shell.app.annotation_running() {
            break;
        }
        let (cp, pv) = PLAN[k.min(7)];
        match searches.answer(&mut rig, run, &fx, cp, pv) {
            Some(next) => fx = next,
            None => break,
        }
    }
    assert!(rig.shell.app.report_data().is_some(), "a report");
    rig
}

fn start(rig: &mut Rig) {
    up(
        rig,
        AppMessage::Report(ReportMessage::StartReview {
            side: SideFilter::Both,
            highlights: false,
        }),
    );
}

fn finish_walk(rig: &mut Rig) {
    for _ in 0..20 {
        let Some(walk) = rig.shell.app.report.walk else {
            return;
        };
        up(
            rig,
            AppMessage::Report(ReportMessage::WalkTick { stamp: walk.stamp }),
        );
    }
    panic!("the walk does not end");
}

fn step(rig: &Rig) -> ReviewStep {
    rig.shell.app.report.review.as_ref().unwrap().step
}

#[test]
fn the_window_draws_the_arrow_of_the_verdict_but_not_during_the_walk() {
    let mut rig = report_rig(LanguagePref::EnUs);
    rig.shell.app.prefs.board.animation_ms = 200;
    start(&mut rig);
    if rig.shell.app.report.walk.is_some() {
        assert!(rig.shell.app.board_arrows().is_empty(), "walking: no arrow");
        finish_walk(&mut rig);
    }
    assert_eq!(step(&rig), ReviewStep::Looking);
    assert_eq!(rig.shell.app.board_arrows().len(), 1);
}

#[test]
fn the_checkbox_of_the_card_turns_the_verdict_arrow_off_and_on() {
    let mut rig = report_rig(LanguagePref::EnUs);
    start(&mut rig);
    finish_walk(&mut rig);
    assert!(rig.shell.app.prefs.report.review_arrow, "on by default");
    assert_eq!(rig.shell.app.board_arrows().len(), 1);
    let label = "Show the arrow of the best move";
    let sent = click_msgs(&rig, label);
    apply(&mut rig, sent);
    assert!(!rig.shell.app.prefs.report.review_arrow);
    assert!(rig.shell.app.board_arrows().is_empty());
    // See the best move still shows it.
    up(&mut rig, AppMessage::Report(ReportMessage::ReviewShowBest));
    assert_eq!(rig.shell.app.board_arrows().len(), 1);
    let sent = click_msgs(&rig, label);
    apply(&mut rig, sent);
    assert!(rig.shell.app.prefs.report.review_arrow);
}

#[test]
fn try_again_hides_the_arrow_in_the_window_too() {
    let mut rig = report_rig(LanguagePref::EnUs);
    start(&mut rig);
    finish_walk(&mut rig);
    up(&mut rig, AppMessage::Report(ReportMessage::ReviewTry));
    assert_eq!(step(&rig), ReviewStep::Trying);
    assert!(rig.shell.app.board_arrows().is_empty());
}

#[test]
fn the_new_checkbox_is_translated_in_both_languages() {
    let en = I18n::new(Language::EnUs).tr("report-review-arrow");
    let pt = I18n::new(Language::PtBr).tr("report-review-arrow");
    for text in [&en, &pt] {
        assert!(!text.is_empty() && text != "report-review-arrow", "{text}");
    }
    assert_ne!(en, pt);
    // And the card of the review shows it in Portuguese.
    let mut rig = report_rig(LanguagePref::PtBr);
    start(&mut rig);
    finish_walk(&mut rig);
    assert!(rig.sim(SIZE).find(pt.as_str()).is_ok(), "{pt}");
}
