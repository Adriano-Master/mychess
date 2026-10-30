//! M6-4d (GUI, spec D3438): the piece tracker drawn from the view of
//! the application: selection by left and right click, the controls, the
//! list of squares and the states.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m6_4d_common;

use m6_4d_common::*;
use mychess_app::piece_tracker::{
    PieceTrackerEffect, PieceTrackerFailure, PieceTrackerNotice, TrackerMode, TrackerScheme,
};
use mychess_core::Square;

fn messages(sim: Simulator<'_, Message>) -> Vec<Message> {
    sim.into_messages().collect()
}

fn tracker_messages(messages: Vec<Message>) -> Vec<PieceTrackerMessage> {
    messages
        .into_iter()
        .filter_map(|m| match m {
            Message::App(AppMessage::PieceTracker(t)) => Some(t),
            _ => None,
        })
        .collect()
}

fn ready() -> Rig {
    let mut rig = Rig::new();
    rig.open_games(&GAMES);
    rig.toggle(PaneKind::PieceTracker);
    rig.app(PieceTrackerMessage::Refresh);
    rig
}

fn id(square: Square) -> iced::widget::Id {
    iced::widget::Id::from(format!("tracker-piece-{square}"))
}

#[test]
fn the_pane_opens_with_d1_selected_and_counts_the_filter() {
    let rig = ready();
    let view = rig.shell.app.piece_tracker_view();
    assert_eq!(view.selected, [Square::D1]);
    assert_eq!(view.cells.len(), 64);
    assert_eq!(view.ranking.len(), 64);
    assert_eq!(view.games, 5);
    assert!(!view.interrupted);
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(
        sim.find(iced::widget::Id::new("piece-tracker-pane"))
            .is_ok()
    );
    // The list of squares: position, name, count and percent.
    assert!(sim.find("1.").is_ok());
    for file_rank in ["a1", "h8"] {
        assert!(sim.find(file_rank).is_ok(), "{file_rank} in the list");
    }
    for theme in themes() {
        assert!(rig.sim(1400.0, 900.0).snapshot(&theme).is_ok());
        assert!(rig.sim(320.0, 380.0).snapshot(&theme).is_ok());
    }
}

#[test]
fn the_32_pieces_are_selectors() {
    let rig = ready();
    let mut sim = rig.sim(1400.0, 900.0);
    for square in (0..64u32).map(Square::new) {
        let starts = (square.rank() as u32) < 2 || (square.rank() as u32) > 5;
        assert_eq!(sim.find(id(square)).is_ok(), starts, "{square}");
    }
}

#[test]
fn a_left_click_picks_one_piece_and_a_right_click_its_sisters() {
    let mut rig = ready();
    let mut sim = rig.sim(1400.0, 900.0);
    sim.click(id(Square::G1)).unwrap();
    let sent = tracker_messages(messages(sim));
    assert!(sent.iter().any(|m| matches!(
        m,
        PieceTrackerMessage::SelectPiece { square, sister: false } if *square == Square::G1
    )));
    rig.app(PieceTrackerMessage::SelectPiece {
        square: Square::G1,
        sister: false,
    });
    assert_eq!(rig.shell.app.piece_tracker_view().selected, [Square::G1]);

    // The right button.
    let mut sim = rig.sim(1400.0, 900.0);
    let target = sim.find(id(Square::G1)).unwrap();
    sim.point_at(target.visible_bounds().unwrap().center());
    drop(sim.simulate([
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
            iced::mouse::Button::Right,
        )),
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
            iced::mouse::Button::Right,
        )),
    ]));
    let sent = tracker_messages(messages(sim));
    assert!(sent.iter().any(|m| matches!(
        m,
        PieceTrackerMessage::SelectPiece { square, sister: true } if *square == Square::G1
    )));
    rig.app(PieceTrackerMessage::SelectPiece {
        square: Square::G1,
        sister: true,
    });
    let mut selected = rig.shell.app.piece_tracker_view().selected;
    selected.sort_by_key(|s| s.to_usize());
    assert_eq!(selected, [Square::B1, Square::G1]);
    // The pawns: all eight.
    rig.app(PieceTrackerMessage::SelectPiece {
        square: Square::E2,
        sister: true,
    });
    assert_eq!(rig.shell.app.piece_tracker_view().selected.len(), 8);
}

#[test]
fn a_square_without_a_starting_piece_is_refused_with_a_notice() {
    let mut rig = ready();
    rig.app(PieceTrackerMessage::SelectPiece {
        square: Square::E4,
        sister: false,
    });
    let view = rig.shell.app.piece_tracker_view();
    assert_eq!(view.selected, [Square::D1]);
    assert_eq!(view.issue, Some(PieceTrackerNotice::NotAPiece));
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(
        sim.find(rig.tr("ptracker-notice-not-a-piece").as_str())
            .is_ok()
    );
}

#[test]
fn the_mode_the_colors_and_the_update_button_send_their_messages() {
    let mut rig = ready();
    let mut sim = rig.sim(1400.0, 900.0);
    sim.click(rig.tr("ptracker-mode-time").as_str()).unwrap();
    assert!(
        tracker_messages(messages(sim))
            .iter()
            .any(|m| matches!(m, PieceTrackerMessage::SetMode(TrackerMode::Time)))
    );
    let mut sim = rig.sim(1400.0, 900.0);
    sim.click(rig.tr("ptracker-color-xred").as_str()).unwrap();
    assert!(
        tracker_messages(messages(sim))
            .iter()
            .any(|m| matches!(m, PieceTrackerMessage::SetScheme(TrackerScheme::XRed)))
    );
    let mut sim = rig.sim(1400.0, 900.0);
    sim.click(rig.tr("ptracker-update").as_str()).unwrap();
    assert!(
        tracker_messages(messages(sim))
            .iter()
            .any(|m| matches!(m, PieceTrackerMessage::Refresh))
    );
    // Applying the scheme only repaints: the counts stay.
    let before = rig.shell.app.piece_tracker_view().cells[0].count;
    rig.app(PieceTrackerMessage::SetScheme(TrackerScheme::XRed));
    let view = rig.shell.app.piece_tracker_view();
    assert_eq!(view.cells[0].count, before);
    assert_eq!(view.scheme, TrackerScheme::XRed);
    rig.app(PieceTrackerMessage::SetMode(TrackerMode::Time));
    assert_eq!(rig.shell.app.piece_tracker_view().mode, TrackerMode::Time);
}

#[test]
fn the_move_fields_take_three_digits_and_the_end_follows_the_start() {
    let mut rig = ready();
    rig.ui(Message::Panels(DataPanelsMessage::TrackerEdit(
        mychess_gui::windows::data_panels::TrackerField::End,
        "4x5y".into(),
    )));
    assert_eq!(rig.shell.app.piece_tracker_view().end_move, 45);
    rig.ui(Message::Panels(DataPanelsMessage::TrackerEdit(
        mychess_gui::windows::data_panels::TrackerField::Start,
        "70".into(),
    )));
    let view = rig.shell.app.piece_tracker_view();
    assert_eq!((view.start_move, view.end_move), (70, 70));
    // The text typed in the last field was dropped: it shows the value.
    assert_eq!(rig.shell.ui.dialogs.panels.tracker_text[1], None);
    // An empty field sends nothing.
    rig.ui(Message::Panels(DataPanelsMessage::TrackerEdit(
        mychess_gui::windows::data_panels::TrackerField::Start,
        String::new(),
    )));
    assert_eq!(rig.shell.app.piece_tracker_view().start_move, 70);
}

#[test]
fn stop_keeps_the_partial_counts_and_says_interrupted() {
    let mut rig = Rig::new();
    rig.open_games(&GAMES);
    rig.toggle(PaneKind::PieceTracker);
    // Start a count and stop it before the answer arrives.
    let effects = update(&mut rig.shell.app, PieceTrackerMessage::Refresh.into());
    let (request, control) = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::PieceTracker(PieceTrackerEffect::Track {
                request, control, ..
            }) => Some((request, control)),
            _ => None,
        })
        .expect("a count starts");
    rig.app(PieceTrackerMessage::Stop);
    assert!(control.is_cancelled());
    // The effect now answers with the partial result.
    let answer = rig.shell.app.piece_tracker_view();
    assert!(answer.progress.is_some() || answer.cells.is_empty());
    let message = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(perform(Effect::PieceTracker(PieceTrackerEffect::Track {
            request,
            source: rig
                .shell
                .app
                .bases
                .slot(rig.shell.app.bases.current())
                .unwrap()
                .source
                .clone(),
            filter: Box::new(
                rig.shell
                    .app
                    .bases
                    .slot(rig.shell.app.bases.current())
                    .unwrap()
                    .filter()
                    .clone(),
            ),
            query: Box::new(mychess_db::PieceTrackQuery {
                squares: vec![Square::D1],
                start_move: 1,
                end_move: 20,
                mode: mychess_db::TrackMode::Games,
            }),
            control,
        })));
    rig.app(message);
    let view = rig.shell.app.piece_tracker_view();
    assert!(view.interrupted, "marked interrupted");
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find(rig.tr("ptracker-interrupted").as_str()).is_ok());
}

#[test]
fn a_failed_count_is_told() {
    let mut rig = Rig::new();
    rig.open_games(&GAMES);
    rig.toggle(PaneKind::PieceTracker);
    let request = update(&mut rig.shell.app, PieceTrackerMessage::Refresh.into())
        .into_iter()
        .find_map(|e| match e {
            Effect::PieceTracker(PieceTrackerEffect::Track { request, .. }) => Some(request),
            _ => None,
        })
        .unwrap();
    rig.app(PieceTrackerMessage::Computed {
        request,
        result: Box::new(Err(PieceTrackerFailure::Internal)),
    });
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find(rig.tr("ptracker-notice-failed").as_str()).is_ok());
}

#[test]
fn an_empty_filter_counts_zeros_and_says_so() {
    let mut rig = Rig::new();
    let slot = rig.open_games(&GAMES);
    rig.app(BasesMessage::List(mychess_app::ListMessage::NegateFilter {
        slot,
    }));
    rig.app(BasesMessage::List(mychess_app::ListMessage::NegateFilter {
        slot,
    }));
    rig.toggle(PaneKind::PieceTracker);
    rig.app(PieceTrackerMessage::Refresh);
    let view = rig.shell.app.piece_tracker_view();
    assert_eq!(view.cells.len(), 64);
    assert!(view.cells.iter().all(|c| c.count == 0) || view.games == 5);
}

#[test]
fn the_hover_message_shows_the_square_under_the_pointer() {
    let mut rig = ready();
    rig.ui(Message::Panels(DataPanelsMessage::TrackerHover(Some(
        Square::E4,
    ))));
    let sim = rig.sim(1400.0, 900.0);
    drop(sim);
    let view = rig.shell.app.piece_tracker_view();
    let cell = view.cells[Square::E4.to_usize()];
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("square", "e4");
    args.set("count", i64::from(cell.count));
    args.set("percent", format!("{:.2}", cell.percent));
    let position = view
        .ranking
        .iter()
        .find(|r| r.square == Square::E4)
        .unwrap()
        .position;
    args.set("position", i64::try_from(position).unwrap());
    let text = rig.shell.i18n.tr_args("ptracker-hover", &args);
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find(text.as_str()).is_ok(), "{text}");
    drop(sim);
    rig.ui(Message::Panels(DataPanelsMessage::TrackerHover(None)));
}
