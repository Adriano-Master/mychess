//! Contract tests of the piece tracker (spec
//! `m6-4-tabela-estatisticas-rastreador.md`, slice c: D3352, D3385, D3389,
//! D3391, D3394, D3408, D3409).
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]
mod m6_4_common;

use m6_4_common::*;
use mychess_app::piece_tracker::{
    PieceTrackerNotice, TrackerMode, TrackerScheme, initial_pieces, sister_squares,
};
use mychess_app::{load_preferences, render_preferences};
use mychess_core::Square;

fn find_track(effects: &[Effect]) -> Option<Effect> {
    effects
        .iter()
        .find(|e| matches!(e, Effect::PieceTracker(PieceTrackerEffect::Track { .. })))
        .cloned()
}

fn cell(rig: &Rig, square: Square) -> mychess_app::piece_tracker::TrackerCell {
    rig.state.piece_tracker_view().cells[square.to_usize()]
}

#[test]
fn opening_asks_for_the_queen_of_white_over_the_filter() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(PieceTrackerMessage::Open);
    let Some(Effect::PieceTracker(PieceTrackerEffect::Track { query, filter, .. })) =
        find_track(&effects)
    else {
        panic!("no track effect: {effects:?}");
    };
    assert_eq!(query.squares, [Square::D1]);
    assert_eq!((query.start_move, query.end_move), (1, 20));
    assert_eq!(query.mode, mychess_db::TrackMode::Games);
    assert_eq!(filter.count(), 5);
}

#[test]
fn the_heat_map_is_ready_to_paint() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(PieceTrackerMessage::Open);
    let view = rig.state.piece_tracker_view();
    assert_eq!(view.cells.len(), 64);
    assert_eq!(view.selected_names, ["d1"]);
    assert_eq!((view.games, view.games_skipped), (5, 0));
    assert!(!view.interrupted && !view.stale);
    // Game 1: Qh5, then Qxf7.
    let h5 = cell(&rig, Square::H5);
    let f7 = cell(&rig, Square::F7);
    assert_eq!((h5.count, f7.count), (1, 1));
    assert!((h5.fraction - 1.0).abs() < f32::EPSILON);
    assert_eq!(h5.rgb, [9, 9, 159], "100% in blue");
    assert!((h5.percent - 20.0).abs() < 1e-9, "1 of 5 games");
    assert_eq!(cell(&rig, Square::A1).rgb, [255, 255, 255]);
    // The list: ties by the smaller square (h5 is 39, f7 is 53).
    assert_eq!(view.ranking.len(), 64);
    assert_eq!(view.ranking[0].name, "h5");
    assert_eq!(view.ranking[1].name, "f7");
    assert_eq!(view.ranking[0].position, 1);
}

#[test]
fn the_scheme_only_paints() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(PieceTrackerMessage::Open);
    let effects = rig.raw(PieceTrackerMessage::SetScheme(TrackerScheme::Red));
    assert!(find_track(&effects).is_none());
    assert!(rig.delays.is_empty());
    assert_ne!(cell(&rig, Square::H5).rgb, [9, 9, 159]);
    assert_eq!(rig.state.piece_tracker_view().scheme, TrackerScheme::Red);
}

#[test]
fn selecting_a_piece_counts_at_once_without_the_debounce() {
    // D3423: a click on an option starts the count at once.
    let (mut rig, _slot, _dir) = sample();
    rig.send(PieceTrackerMessage::Open);
    rig.send(PieceTrackerMessage::SelectPiece {
        square: Square::A1,
        sister: true,
    });
    let view = rig.state.piece_tracker_view();
    assert_eq!(view.selected, [Square::A1, Square::H1]);
    assert!(!view.stale, "the map on show is already for the rooks");
    assert_eq!(cell(&rig, Square::H5).count, 0);
    assert!(rig.delays.is_empty(), "no debounce for an option");
    // The right button on a rook, the left on a knight.
    rig.send(PieceTrackerMessage::SelectPiece {
        square: Square::G1,
        sister: false,
    });
    assert_eq!(rig.state.prefs.ptracker.squares, [Square::G1]);
}

#[test]
fn the_knight_of_the_second_game_is_counted_in_games_mode() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(PieceTrackerMessage::SelectPiece {
        square: Square::G1,
        sister: false,
    });
    rig.send(PieceTrackerMessage::Open);
    // Games 3 (Nf3, Ng1) and 4 (Nf3): f3 in two games, g1 in one.
    assert_eq!(cell(&rig, Square::F3).count, 2);
    assert_eq!(cell(&rig, Square::G1).count, 1);
}

#[test]
fn a_square_without_a_piece_is_refused() {
    let mut rig = Rig::new();
    rig.send(PieceTrackerMessage::SelectPiece {
        square: Square::E4,
        sister: false,
    });
    assert_eq!(rig.state.prefs.ptracker.squares, [Square::D1]);
    assert_eq!(
        rig.state.piece_tracker_view().issue,
        Some(PieceTrackerNotice::NotAPiece)
    );
}

#[test]
fn the_window_keeps_the_end_after_the_start() {
    let mut rig = Rig::new();
    rig.send(PieceTrackerMessage::SetEnd(3));
    rig.send(PieceTrackerMessage::SetStart(5));
    let p = &rig.state.prefs.ptracker;
    assert_eq!((p.start_move, p.end_move), (5, 5));
    rig.send(PieceTrackerMessage::SetEnd(2));
    assert_eq!(rig.state.prefs.ptracker.end_move, 5);
    rig.send(PieceTrackerMessage::SetEnd(5000));
    assert_eq!(rig.state.prefs.ptracker.end_move, 999);
}

#[test]
fn time_mode_divides_by_the_moves_of_the_window() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(PieceTrackerMessage::Open);
    rig.send(PieceTrackerMessage::SetMode(TrackerMode::Time));
    rig.fire_delays();
    let view = rig.state.piece_tracker_view();
    assert_eq!(view.mode, TrackerMode::Time);
    assert!(!view.stale);
}

#[test]
fn stop_keeps_the_partial_result_marked_interrupted() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(PieceTrackerMessage::Open);
    let effect = find_track(&effects).unwrap();
    rig.raw(PieceTrackerMessage::Stop);
    let answer = rig.perform(effect);
    rig.raw(answer);
    let view = rig.state.piece_tracker_view();
    assert!(view.interrupted, "P12: the partial result says so");
    assert_eq!(view.cells.len(), 64);
    // No automatic restart: the key did not change.
    let effects = rig.raw(Message::Game(mychess_app::GameMessage::Navigate(
        mychess_app::Nav::End,
    )));
    assert!(find_track(&effects).is_none());
    // "Update" counts again.
    let effects = rig.raw(PieceTrackerMessage::Refresh);
    assert!(find_track(&effects).is_some());
}

#[test]
fn a_stale_answer_is_dropped() {
    let (mut rig, _slot, _dir) = sample();
    let first = rig.raw(PieceTrackerMessage::Open);
    let old = find_track(&first).unwrap();
    let Effect::PieceTracker(PieceTrackerEffect::Track { control, .. }) = &old else {
        unreachable!()
    };
    let control = control.clone();
    let second = rig.raw(PieceTrackerMessage::SelectPiece {
        square: Square::G1,
        sister: false,
    });
    assert!(control.is_cancelled());
    let new = find_track(&second).expect("no result yet: no wait");
    let answer = rig.perform(old);
    rig.raw(answer);
    assert!(rig.state.piece_tracker_view().cells.is_empty());
    let answer = rig.perform(new);
    rig.raw(answer);
    assert_eq!(cell(&rig, Square::F3).count, 2);
}

#[test]
fn a_change_of_the_filter_or_of_the_database_counts_again() {
    let (mut rig, slot, dir) = sample();
    rig.send(PieceTrackerMessage::Open);
    rig.negate_filter(slot);
    assert!(rig.state.piece_tracker_view().stale);
    rig.fire_delays();
    let view = rig.state.piece_tracker_view();
    assert_eq!((view.games, cell(&rig, Square::H5).count), (0, 0));
    rig.open_games(dir.path(), "other.pgn", &GAMES[..1]);
    rig.fire_delays();
    let view = rig.state.piece_tracker_view();
    assert_eq!(view.games, 1);
    assert_eq!(cell(&rig, Square::H5).count, 1);
}

#[test]
fn closing_and_quit_stop_the_count() {
    let (mut rig, _slot, _dir) = sample();
    for end in [false, true] {
        let effects = rig.raw(PieceTrackerMessage::Open);
        let Some(Effect::PieceTracker(PieceTrackerEffect::Track { control, .. })) =
            find_track(&effects)
        else {
            panic!("no track");
        };
        if end {
            rig.raw(Message::Quit);
        } else {
            rig.raw(PieceTrackerMessage::Close);
        }
        assert!(control.is_cancelled());
    }
}

#[test]
fn the_preferences_go_and_come_back() {
    let mut rig = Rig::new();
    rig.send(PieceTrackerMessage::SelectPiece {
        square: Square::A1,
        sister: true,
    });
    rig.send(PieceTrackerMessage::SetStart(3));
    rig.send(PieceTrackerMessage::SetEnd(9));
    rig.send(PieceTrackerMessage::SetMode(TrackerMode::Time));
    rig.send(PieceTrackerMessage::SetScheme(TrackerScheme::XRed));
    let text = render_preferences(&rig.state.prefs);
    let loaded = load_preferences(&text);
    assert_eq!(loaded.issues, []);
    assert_eq!(*loaded.prefs.ptracker, *rig.state.prefs.ptracker);
    let p = &loaded.prefs.ptracker;
    assert_eq!(p.squares, [Square::A1, Square::H1]);
    assert_eq!((p.start_move, p.end_move), (3, 9));
    assert_eq!((p.mode, p.scheme), (TrackerMode::Time, TrackerScheme::XRed));
}

#[test]
fn a_bad_preference_falls_back_field_by_field() {
    let loaded = load_preferences(
        "[ptracker]\nsquares = [\"e4\", \"zz\", \"h8\"]\nstart_move = 5000\nend_move = 2\nscheme = \"pink\"\n",
    );
    let p = &loaded.prefs.ptracker;
    assert_eq!(p.squares, [Square::H8], "only starting squares stay");
    assert_eq!((p.start_move, p.end_move), (999, 999));
    assert_eq!(p.scheme, TrackerScheme::Blue);
}

#[test]
fn the_selection_helpers_follow_the_scid() {
    assert_eq!(initial_pieces().len(), 32);
    assert_eq!(sister_squares(Square::B1), [Square::B1, Square::G1]);
    assert_eq!(sister_squares(Square::D8), [Square::D8]);
}
