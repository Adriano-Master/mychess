//! Extra contract tests of the piece tracker controller (spec
//! `m6-4-tabela-estatisticas-rastreador.md`, section 2.5, rules P1 to P13 as
//! seen through the app, D3410, D3417, D3419, D3420).
#![allow(
    let_underscore_drop,
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]
mod m6_4_common;

use std::path::Path;

use m6_4_common::*;
use mychess_app::piece_tracker::{
    PieceTrackerNotice, TrackerCell, TrackerMode, initial_pieces, sister_squares,
};
use mychess_app::{load_preferences, render_preferences};
use mychess_core::Square;

fn find_track(effects: &[Effect]) -> Option<Effect> {
    effects
        .iter()
        .find(|e| matches!(e, Effect::PieceTracker(PieceTrackerEffect::Track { .. })))
        .cloned()
}

fn cell(rig: &Rig, square: Square) -> TrackerCell {
    rig.state.piece_tracker_view().cells[square.to_usize()]
}

fn count(rig: &Rig, name: &str) -> u32 {
    cell(rig, name.parse().expect("square")).count
}

/// One game of a hand-written PGN.
fn game(tags: &str, moves: &str) -> String {
    format!(
        "[Event \"T\"]\n[Site \"S\"]\n[Date \"2020.01.01\"]\n[Round \"1\"]\n[White \"W\"]\n\
         [Black \"B\"]\n[Result \"*\"]\n{tags}\n{moves}\n\n"
    )
}

fn open_text(rig: &mut Rig, dir: &Path, name: &str, text: &str) -> SlotId {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write");
    rig.send(BasesMessage::Open(path));
    rig.state.bases.current()
}

fn select(rig: &mut Rig, square: &str, sister: bool) {
    rig.send(PieceTrackerMessage::SelectPiece {
        square: square.parse().expect("square"),
        sister,
    });
    rig.fire_delays();
}

/// Kingside castling of both colours, queenside of both, and a game with the
/// two kings castling on the other side.
fn castling_base() -> String {
    let a = game("", "1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 4. O-O Nf6 *");
    let b = game(
        "",
        "1. d4 d5 2. Nc3 Nc6 3. Bf4 Bf5 4. Qd2 Qd7 5. O-O-O O-O-O *",
    );
    let c = game("", "1. e4 e5 2. Nf3 Nf6 3. Bc4 Bc5 4. O-O O-O *");
    format!("{a}{b}{c}")
}

#[test]
fn every_starting_square_is_a_piece_and_the_other_thirty_two_are_refused() {
    let mut rig = Rig::new();
    let pieces = initial_pieces();
    assert_eq!(pieces.len(), 32);
    for file in 0..8u8 {
        for rank in [2u8, 3, 4, 5] {
            let name = format!("{}{}", char::from(b'a' + file), rank + 1);
            let square: Square = name.parse().unwrap();
            let before = rig.state.prefs.ptracker.squares.clone();
            let effects = rig.raw(PieceTrackerMessage::SelectPiece {
                square,
                sister: false,
            });
            assert!(effects.is_empty(), "{name}: {effects:?}");
            assert_eq!(rig.state.prefs.ptracker.squares, before, "{name}");
            assert_eq!(
                rig.state.piece_tracker_view().issue,
                Some(PieceTrackerNotice::NotAPiece),
                "{name}"
            );
        }
    }
    // The 32 drawings can all be chosen, by the left and by the right button.
    for piece in pieces {
        let square = piece.0;
        for sister in [false, true] {
            rig.raw(PieceTrackerMessage::SelectPiece { square, sister });
            let chosen = rig.state.prefs.ptracker.squares.clone();
            assert!(chosen.contains(&square), "{square:?} {sister}");
            if !sister {
                assert_eq!(chosen, [square]);
            }
        }
    }
}

#[test]
fn sister_pieces_follow_the_scid_selection_rules() {
    // Queens and kings only select themselves, even with the right button.
    for name in ["d1", "d8", "e1", "e8"] {
        let sq: Square = name.parse().unwrap();
        assert_eq!(sister_squares(sq), [sq], "{name}");
    }
    // Rooks, knights and bishops select both of their side.
    for (a, b) in [
        ("a1", "h1"),
        ("b1", "g1"),
        ("c1", "f1"),
        ("a8", "h8"),
        ("b8", "g8"),
        ("c8", "f8"),
    ] {
        let (a, b): (Square, Square) = (a.parse().unwrap(), b.parse().unwrap());
        assert_eq!(sister_squares(a), [a, b]);
        assert_eq!(sister_squares(b), [a, b]);
    }
    // A pawn selects the eight pawns of its colour, and only them.
    for (rank, color) in [(2u8, "white"), (7, "black")] {
        let all: Vec<Square> = (0..8u8)
            .map(|f| format!("{}{rank}", char::from(b'a' + f)).parse().unwrap())
            .collect();
        for &sq in &all {
            let mut got = sister_squares(sq);
            got.sort_by_key(|s| s.to_usize());
            assert_eq!(got, all, "{color} pawn {sq:?}");
        }
    }
}

#[test]
fn castling_moves_the_rook_and_the_king() {
    // Games: A white O-O (ply 7); B both O-O-O (plies 9 and 10); C both O-O
    // (plies 7 and 8).
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    open_text(&mut rig, dir.path(), "c.pgn", &castling_base());
    rig.send(PieceTrackerMessage::Open);
    for (rook, square, expected) in [
        ("h1", "f1", 2), // games A and C
        ("a1", "d1", 1), // game B
        ("h8", "f8", 1), // game C
        ("a8", "d8", 1), // game B
        ("e1", "g1", 2), // king, games A and C
        ("e1", "c1", 1), // game B
        ("e8", "g8", 1), // game C
        ("e8", "c8", 1), // game B
        ("a1", "f1", 0), // the other rook never goes there
        ("h1", "d1", 0),
    ] {
        select(&mut rig, rook, false);
        assert_eq!(count(&rig, square), expected, "{rook} -> {square}");
    }
    // The king's own destination is not counted for a rook.
    select(&mut rig, "h1", false);
    assert_eq!(count(&rig, "g1"), 0);
}

#[test]
fn the_window_cuts_the_counted_plies_but_not_the_following() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    open_text(&mut rig, dir.path(), "c.pgn", &castling_base());
    rig.send(PieceTrackerMessage::Open);
    select(&mut rig, "h1", false);
    assert_eq!(count(&rig, "f1"), 2, "default window 1..20");

    // P1: start 4 -> ply 7: the castling of games A and C is the first
    // counted ply.
    rig.send(PieceTrackerMessage::SetStart(4));
    rig.fire_delays();
    assert_eq!(count(&rig, "f1"), 2, "ply 7 is inside the window");

    // Start 5 -> ply 9: games A and C have only 8 plies and are ignored (P3),
    // game B is counted but its h1 rook never moves.
    rig.send(PieceTrackerMessage::SetStart(5));
    rig.fire_delays();
    let view = rig.state.piece_tracker_view();
    assert_eq!(count(&rig, "f1"), 0);
    assert_eq!((view.games, view.games_used, view.games_skipped), (3, 1, 2));

    // End 3 -> ply 6: the castling at ply 7 is out of the window.
    rig.send(PieceTrackerMessage::SetStart(1));
    rig.send(PieceTrackerMessage::SetEnd(3));
    rig.fire_delays();
    assert_eq!(count(&rig, "f1"), 0);
    // End 4 -> ply 8: in.
    rig.send(PieceTrackerMessage::SetEnd(4));
    rig.fire_delays();
    assert_eq!(count(&rig, "f1"), 2);
}

#[test]
fn start_zero_and_one_are_the_same_window() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    open_text(&mut rig, dir.path(), "c.pgn", &castling_base());
    rig.send(PieceTrackerMessage::Open);
    select(&mut rig, "g1", false);
    rig.send(PieceTrackerMessage::SetStart(1));
    rig.fire_delays();
    let one: Vec<u32> = rig
        .state
        .piece_tracker_view()
        .cells
        .iter()
        .map(|c| c.count)
        .collect();
    rig.send(PieceTrackerMessage::SetStart(0));
    rig.send(PieceTrackerMessage::SetEnd(0));
    rig.fire_delays();
    // Start 0 with end 0: the end becomes the start (0), which is still the
    // plies 1 and 2.
    let view = rig.state.piece_tracker_view();
    assert_eq!((view.start_move, view.end_move), (0, 0));
    let zero: Vec<u32> = view.cells.iter().map(|c| c.count).collect();
    assert!(
        zero.iter().sum::<u32>() <= one.iter().sum::<u32>(),
        "{zero:?} {one:?}"
    );
    // The knight g1 moves at ply 3 in game A (Nf3) and C: outside 1..2.
    assert_eq!(count(&rig, "f3"), 0);
}

#[test]
fn games_without_a_standard_start_empty_or_short_are_skipped_but_still_divide() {
    let fen =
        "[SetUp \"1\"]\n[FEN \"rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1\"]\n";
    let text = format!(
        "{}{}{}{}",
        game("", "1. Nf3 *"),
        game(fen, "1... e5 *"),
        game("", "*"),
        game("", "1. Nf3 Nf6 *"),
    );
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = open_text(&mut rig, dir.path(), "m.pgn", &text);
    assert_eq!(rig.state.bases.slot(slot).unwrap().total(), 4);
    select(&mut rig, "g1", false);
    rig.send(PieceTrackerMessage::Open);
    let view = rig.state.piece_tracker_view();
    assert_eq!(count(&rig, "f3"), 2);
    assert_eq!(view.games, 4, "P13: the filter, not the games used");
    assert_eq!(view.games_used + view.games_skipped, 4);
    assert_eq!(view.games_skipped, 2, "the FEN game and the empty one");
    let f3 = cell(&rig, "f3".parse().unwrap());
    assert!((f3.percent - 50.0).abs() < 1e-9, "{f3:?}");
}

#[test]
fn en_passant_removes_the_captured_pawn_and_time_is_in_whole_moves() {
    // 1.e4 a6 2.e5 d5 3.exd6 (e.p.) a5 4.Nf3 a4 5.Nc3 h6 6.Nd5 Ra6: twelve
    // plies. The d5 pawn is captured at ply 5 and must disappear (P8); the
    // Scid leaves a ghost there.
    let text = game(
        "",
        "1. e4 a6 2. e5 d5 3. exd6 a5 4. Nf3 a4 5. Nc3 h6 6. Nd5 Ra6 *",
    );
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    open_text(&mut rig, dir.path(), "ep.pgn", &text);
    rig.send(PieceTrackerMessage::SetMode(TrackerMode::Time));
    rig.send(PieceTrackerMessage::SetEnd(6));
    rig.send(PieceTrackerMessage::Open);
    select(&mut rig, "d7", true); // the eight black pawns
    // Games mode first: d5 is reached once.
    rig.send(PieceTrackerMessage::SetMode(TrackerMode::Games));
    rig.fire_delays();
    assert_eq!(count(&rig, "d5"), 1);
    assert_eq!(count(&rig, "a6"), 1);

    rig.send(PieceTrackerMessage::SetMode(TrackerMode::Time));
    rig.fire_delays();
    let view = rig.state.piece_tracker_view();
    assert_eq!(view.mode, TrackerMode::Time);
    // d5 holds the pawn after ply 4 only: 1 ply = 0 whole moves. A ghost
    // would give (12 - 4 + 1) / 2 = 4.
    assert_eq!(count(&rig, "d5"), 0, "no ghost after the en passant");
    // a7 holds a pawn after ply 1 only (the first ply is white's): 1/2 = 0.
    assert_eq!(count(&rig, "a7"), 0);
    // a6: after plies 2, 3, 4, 5; a4: after plies 8 to 12 (5).
    assert_eq!(count(&rig, "a6"), 2);
    assert_eq!(count(&rig, "a4"), 2);
    // d7 is occupied after plies 1, 2, 3.
    assert_eq!(count(&rig, "d7"), 1);
    // P9, time mode: divisor = games x (end + 1 - start) = 1 x 6.
    let a6 = cell(&rig, "a6".parse().unwrap());
    assert!((a6.percent - 33.33).abs() < 1e-9, "{a6:?}");
}

#[test]
fn promotion_keeps_the_pawn_followed() {
    // A white pawn runs to the promotion square and becomes a queen that
    // keeps being followed until the end of the window.
    let text = game(
        "",
        "1. h4 g5 2. hxg5 h6 3. gxh6 Nf6 4. h7 Ng8 5. hxg8=Q Nc6 6. Qxf7+ Kxf7 *",
    );
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    open_text(&mut rig, dir.path(), "p.pgn", &text);
    rig.send(PieceTrackerMessage::Open);
    select(&mut rig, "h2", false);
    // h4, g5, h6, h7, g8 (promotion), then f7 where it is captured.
    for square in ["h4", "g5", "h6", "h7", "g8", "f7"] {
        assert_eq!(count(&rig, square), 1, "{square}");
    }
    assert_eq!(count(&rig, "h8"), 0);
}

#[test]
fn closing_and_reopening_drops_the_answer_of_the_first_count() {
    let (mut rig, _slot, _dir) = sample();
    let first = rig.raw(PieceTrackerMessage::Open);
    let old = find_track(&first).unwrap();
    rig.raw(PieceTrackerMessage::Close);
    let second = rig.raw(PieceTrackerMessage::Open);
    let new = find_track(&second).expect("a new count after reopening");
    let answer = rig.perform(old);
    rig.raw(answer);
    assert!(
        rig.state.piece_tracker_view().cells.is_empty(),
        "the answer of the closed panel is not shown"
    );
    let answer = rig.perform(new);
    rig.raw(answer);
    assert_eq!(rig.state.piece_tracker_view().cells.len(), 64);
}

#[test]
fn a_burst_of_changes_counts_only_once() {
    let (mut rig, slot, _dir) = sample();
    rig.send(PieceTrackerMessage::Open);
    // Three changes of the filter in a row: three debounces, one count.
    rig.negate_filter(slot);
    rig.negate_filter(slot);
    rig.negate_filter(slot);
    let delays = std::mem::take(&mut rig.delays);
    assert_eq!(delays.len(), 3);
    let mut counts = 0;
    for delay in &delays {
        let message = Rig::elapsed(delay).unwrap();
        counts += rig
            .raw(message)
            .iter()
            .filter(|e| find_track(std::slice::from_ref(e)).is_some())
            .count();
    }
    assert_eq!(counts, 1, "only the last delay starts the count");
}

#[test]
fn stop_without_a_count_is_harmless_and_a_new_key_clears_the_interrupted_mark() {
    let (mut rig, slot, _dir) = sample();
    // Nothing open, nothing running.
    let effects = rig.raw(PieceTrackerMessage::Stop);
    assert!(effects.is_empty());
    assert!(!rig.state.piece_tracker_view().interrupted);

    let effects = rig.raw(PieceTrackerMessage::Open);
    let effect = find_track(&effects).unwrap();
    rig.raw(PieceTrackerMessage::Stop);
    let answer = rig.perform(effect);
    rig.raw(answer);
    assert!(rig.state.piece_tracker_view().interrupted);
    // A change of the filter is not a "Stop": the next result is complete.
    rig.negate_filter(slot);
    rig.fire_delays();
    let view = rig.state.piece_tracker_view();
    assert!(!view.interrupted, "{view:?}");
    assert!(!view.stale);
    // "Update" after the stop counts everything again.
    rig.negate_filter(slot);
    rig.fire_delays();
    let effects = rig.raw(PieceTrackerMessage::Refresh);
    let effect = find_track(&effects).expect("refresh starts a count");
    let answer = rig.perform(effect);
    rig.raw(answer);
    assert!(!rig.state.piece_tracker_view().interrupted);
    assert_eq!(rig.state.piece_tracker_view().games, 5);
}

#[test]
fn a_stopped_count_whose_key_changed_is_dropped() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(PieceTrackerMessage::Open);
    let old = find_track(&effects).unwrap();
    rig.raw(PieceTrackerMessage::Stop);
    // Before the partial answer arrives the user picks another piece.
    let second = rig.raw(PieceTrackerMessage::SelectPiece {
        square: Square::G1,
        sister: false,
    });
    // The selection is debounced (a result is not on screen yet, so it may
    // also start at once): either way the partial answer of the old key must
    // not be shown as the result of the new one.
    let answer = rig.perform(old);
    rig.raw(answer);
    let view = rig.state.piece_tracker_view();
    if find_track(&second).is_none() {
        assert!(
            view.stale || view.cells.is_empty() || !view.interrupted,
            "{view:?}"
        );
    }
    rig.fire_delays();
    let view = rig.state.piece_tracker_view();
    assert!(!view.interrupted);
    assert_eq!(view.selected, [Square::G1]);
}

#[test]
fn the_scheme_does_not_change_the_counts_nor_the_key() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(PieceTrackerMessage::Open);
    let before: Vec<u32> = rig
        .state
        .piece_tracker_view()
        .cells
        .iter()
        .map(|c| c.count)
        .collect();
    for scheme in mychess_app::piece_tracker::TrackerScheme::ALL {
        let effects = rig.raw(PieceTrackerMessage::SetScheme(*scheme));
        assert!(find_track(&effects).is_none(), "{scheme:?}");
        assert!(rig.delays.is_empty());
        let view = rig.state.piece_tracker_view();
        assert!(!view.stale);
        let after: Vec<u32> = view.cells.iter().map(|c| c.count).collect();
        assert_eq!(before, after);
    }
}

#[test]
fn hostile_window_values_are_limited() {
    let mut rig = Rig::new();
    for value in [0u16, 1, 998, 999, 1000, u16::MAX] {
        rig.send(PieceTrackerMessage::SetStart(value));
        rig.send(PieceTrackerMessage::SetEnd(value));
        let p = &rig.state.prefs.ptracker;
        assert!(p.start_move <= 999 && p.end_move <= 999, "{value}");
        assert!(p.end_move >= p.start_move, "{value}");
    }
}

#[test]
fn no_base_or_an_empty_filter_gives_sixty_four_zeros() {
    let mut rig = Rig::new();
    rig.send(PieceTrackerMessage::Open);
    let view = rig.state.piece_tracker_view();
    assert!(view.cells.iter().all(|c| c.count == 0));
    assert_eq!(view.games, 0);

    let (mut rig, slot, _dir) = sample();
    rig.negate_filter(slot);
    rig.send(PieceTrackerMessage::Open);
    let view = rig.state.piece_tracker_view();
    assert_eq!(view.cells.len(), 64);
    assert!(view.cells.iter().all(|c| c.count == 0 && c.percent == 0.0));
    assert_eq!(view.games, 0);
    assert!(!view.interrupted);
}

#[test]
fn closing_the_current_base_while_counting_leaves_no_map() {
    let (mut rig, slot, dir) = sample();
    let effects = rig.raw(PieceTrackerMessage::Open);
    let old = find_track(&effects).unwrap();
    let other = rig.open_games(dir.path(), "other.pgn", &GAMES[..1]);
    assert_ne!(other, slot);
    rig.send(BasesMessage::Close(other));
    // The old answer is for a key that is not the one now.
    let answer = rig.perform(old);
    rig.raw(answer);
    rig.fire_delays();
    let _ = rig.state.piece_tracker_view();
}

#[test]
fn a_corrupt_preferences_text_gives_the_defaults_without_panic() {
    let defaults = load_preferences("");
    for text in [
        "[[[[ptracker",
        "[ptracker\nsquares = [",
        "[ptracker]\nstart_move = 18446744073709551615\n",
        "\u{0}\u{1}\u{2}[ptracker]",
        "[ptracker]\nsquares = \"d1\"\n",
        "[ptracker]\nsquares = [1, 2, true, \"\", \"d1d1\", \"A1\"]\nstart_move = -3\nend_move = \"x\"\nmode = 7\nscheme = []\n",
    ] {
        let loaded = load_preferences(text);
        let p = &loaded.prefs.ptracker;
        assert!(p.start_move <= 999 && p.end_move <= 999, "{text:?}");
        assert!(p.end_move >= p.start_move, "{text:?}");
        assert!(
            p.squares
                .iter()
                .all(|s| initial_pieces().iter().any(|i| i.0 == *s)),
            "{text:?}: {:?}",
            p.squares
        );
        // The panel opens on whatever was read.
        let mut rig = Rig::new();
        rig.state.prefs = loaded.prefs;
        rig.send(PieceTrackerMessage::Open);
        let _ = rig.state.piece_tracker_view();
    }
    // A text that is not TOML is reported and gives the defaults.
    let loaded = load_preferences("[[[[ptracker");
    assert!(!loaded.issues.is_empty());
    assert_eq!(*loaded.prefs.ptracker, *defaults.prefs.ptracker);
    // What is written back reads back.
    let text = render_preferences(&loaded.prefs);
    assert_eq!(
        *load_preferences(&text).prefs.ptracker,
        *loaded.prefs.ptracker
    );
}

#[test]
fn the_tracker_is_independent_of_the_game_on_screen() {
    // The map follows the filter, not the editor: loading another game does
    // not recount.
    let (mut rig, slot, _dir) = sample();
    rig.send(PieceTrackerMessage::Open);
    rig.send(BasesMessage::LoadGame { slot, number: 3 });
    assert!(rig.delays.is_empty(), "{:?}", rig.delays);
    assert!(!rig.state.piece_tracker_view().stale);
}
