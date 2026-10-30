//! Contract tests of the search by position, written from the spec
//! `docs/specs/m6-1-buscas-avancadas.md` (comportamento 5 a 11, regras 1 a 16,
//! the nine corrections to Scid) over small PGN fixtures of their own
//! (`fixtures/m6_1a_contract_board.pgn`).
#![allow(
    clippy::assert_is_empty,
    clippy::trivially_copy_pass_by_ref,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    unused_qualifications,
    clippy::field_reassign_with_default,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    dead_code
)]

#[path = "common/m3_3a_support.rs"]
mod base;
#[path = "common/m6_1a_contract_support.rs"]
mod cs;
#[path = "common/m6_1a_support.rs"]
mod support;

use mychess_core::Position;
use mychess_db::{
    BoardMatch, BoardSearch, CancelToken, Filter, FilterOp, NoProgress, SearchError, search_board,
};
use pretty_assertions::assert_eq;
use support::{
    db_of, game, game_fen, numbers, oracle_board, pgn_fixture, pos_after, run_board, values,
};

fn fen(text: &str) -> Position {
    Position::from_fen(text).unwrap()
}

fn kind(pos: Position, kind: BoardMatch) -> BoardSearch {
    BoardSearch {
        kind,
        ..BoardSearch::new(pos)
    }
}

fn fixture() -> mychess_db::PgnDatabase {
    let db = pgn_fixture("m6_1a_contract_board.pgn");
    assert_eq!(mychess_db::GameSource::len(&db), 10, "fixture must load");
    db
}

// ------------------------------------------------------------ the kinds

#[test]
fn exact_follows_a_transposition_to_the_same_ply() {
    let mut db = fixture();
    let (f, r) = run_board(&mut db, &BoardSearch::new(pos_after("e2e4 e7e5 g1f3 b8c6")));
    // Games 1, 2 and 4 (main line) reach it after 4 plies: value 5.
    assert_eq!(values(&f), vec![(1, 5), (2, 5), (4, 5)]);
    assert_eq!(r.matched, 3);
    assert_eq!(r.start_count, 10);
}

#[test]
fn pawns_files_and_material_accept_other_pieces_on_other_squares() {
    let mut db = fixture();
    let target = pos_after("e2e4 e7e5 g1f3 b8c6");
    let mut found = |k| values(&run_board(&mut db, &kind(target.clone(), k)).0);
    // Game 3 (Nc3 Nc6 at ply 4) has the same pawns and counts.
    // The pawns are in place already after 1.e4 e5 (ply 2) in games 1, 3, 4;
    // in game 2 only after 2.e4 e5 (ply 4). Game 3 is not a "knight" match.
    assert_eq!(
        found(BoardMatch::Pawns),
        vec![(1, 3), (2, 5), (3, 3), (4, 3)]
    );
    // Files only counts pawns per file: pushing e2-e4 changes nothing.
    assert_eq!(
        found(BoardMatch::Files),
        vec![(1, 1), (2, 1), (3, 1), (4, 1)]
    );
    // Material: White to move with all pieces is already the start (ply 0)
    // for every standard game.
    assert_eq!(
        found(BoardMatch::Material),
        vec![(1, 1), (2, 1), (3, 1), (4, 1)]
    );
}

#[test]
fn castling_rights_and_en_passant_square_never_decide() {
    // Same squares, rights and en passant differ in both directions.
    let with = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
    let without = "r3k2r/8/8/8/8/8/8/R3K2R w - - 0 1";
    let (_fx, mut db) = db_of(vec![game_fen(without, "a1a2")]);
    assert_eq!(
        values(&run_board(&mut db, &BoardSearch::new(with)).0),
        vec![(1, 1)]
    );
    let (_fx, mut db) = db_of(vec![game_fen(
        "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
        "a1a2",
    )]);
    assert_eq!(
        values(&run_board(&mut db, &BoardSearch::new(fen(without))).0),
        vec![(1, 1)]
    );
    // En passant: the game reaches the position with a capturable pawn; the
    // searched FEN has no en passant square.
    let (_fx, mut db) = db_of(vec![game("d2d4 h7h6 a2a3 a7a6 d4d5 e7e5")]);
    let squares = fen("rnbqkbnr/1ppp1pp1/p6p/3Pp3/8/P7/1PP1PPPP/RNBQKBNR w KQkq - 0 4");
    assert_eq!(
        values(&run_board(&mut db, &BoardSearch::new(squares)).0),
        vec![(1, 7)]
    );
}

#[test]
fn the_side_to_move_always_has_to_agree() {
    let (_fx, mut db) = db_of(vec![game("e2e4 e7e5")]);
    for k in [
        BoardMatch::Exact,
        BoardMatch::Pawns,
        BoardMatch::Files,
        BoardMatch::Material,
    ] {
        let black = fen("rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 2");
        // Black to move with these squares never happens in the game; for
        // Material black-to-move happens at ply 1 (1.e4 played).
        let (f, _) = run_board(&mut db, &kind(black, k));
        // Black to move with the same counts: after 1.e4 (ply 1); the pawns
        // are on the same files there, not on the same squares.
        let expected = if matches!(k, BoardMatch::Material | BoardMatch::Files) {
            vec![(1, 2)]
        } else {
            vec![]
        };
        assert_eq!(values(&f), expected, "{k:?}");
    }
}

// --------------------------------------------------------- ignore colors

#[test]
fn ignore_colors_flips_ranks_colours_and_side_but_never_the_files() {
    let target = fen("4k3/8/8/8/8/8/P7/4K3 w - - 0 1");
    let mut db = fixture();
    let (plain, _) = run_board(&mut db, &BoardSearch::new(target.clone()));
    assert_eq!(values(&plain), vec![(10, 1)]);
    let mut q = BoardSearch::new(target);
    q.ignore_colors = true;
    let (f, _) = run_board(&mut db, &q);
    // 5: Pa7 with Black to move is the upside-down image. Not 6 (h-file:
    // left-right mirror), 7 (P on h2), 8 (wrong side), 9 (colours only).
    assert_eq!(values(&f), vec![(5, 1), (10, 1)]);
}

#[test]
fn ignore_colors_works_both_ways() {
    let mut db = fixture();
    let mut q = BoardSearch::new(fen("4k3/p7/8/8/8/8/8/4K3 b - - 0 1"));
    q.ignore_colors = true;
    assert_eq!(values(&run_board(&mut db, &q).0), vec![(5, 1), (10, 1)]);
}

#[test]
fn the_normal_search_wins_when_both_would_match_and_gives_the_ply() {
    // Black Ra8/Ke8 against White Ra1/Ke1 is its own image. Game: Black to
    // move at ply 0 (the flipped target), then White to move at ply 2.
    let start = "r3k3/8/8/8/8/8/8/R3K3 b - - 0 1";
    let (_fx, mut db) = db_of(vec![game_fen(start, "e8d8 e1d1")]);
    let mut q = BoardSearch::new(fen("r3k3/8/8/8/8/8/8/R3K3 w - - 0 1"));
    q.ignore_colors = true;
    // Ply 0 matches only flipped; the normal pass finds nothing earlier than
    // ply 2? Positions after 2 plies differ (kings moved), so flipped wins.
    assert_eq!(values(&run_board(&mut db, &q).0), vec![(1, 1)]);
    // The normal target at ply 0 is found by the normal pass, not flipped.
    let (_fx, mut db) = db_of(vec![game_fen(
        "r3k3/8/8/8/8/8/8/R3K3 w - - 0 1",
        "e1d1 e8d8",
    )]);
    assert_eq!(values(&run_board(&mut db, &q).0), vec![(1, 1)]);
}

// ------------------------------------------------------------ variations

#[test]
fn variations_need_the_option_and_the_outermost_branch_gives_the_ply() {
    let mut db = fixture();
    // 1.e4 c5 2.Nf3 (variation of game 4, outer branch at ply 1).
    let t1 = pos_after("e2e4 c7c5 g1f3");
    // 1.e4 c5 2.Nc3 Nc6 (nested variation of the outer one).
    let t2 = pos_after("e2e4 c7c5 b1c3 b8c6");
    let mut q1 = BoardSearch::new(t1);
    let mut q2 = BoardSearch::new(t2);
    assert!(numbers(&run_board(&mut db, &q1).0).is_empty());
    assert!(numbers(&run_board(&mut db, &q2).0).is_empty());
    q1.in_variations = true;
    q2.in_variations = true;
    assert_eq!(values(&run_board(&mut db, &q1).0), vec![(4, 2)]);
    assert_eq!(values(&run_board(&mut db, &q2).0), vec![(4, 2)]);
}

#[test]
fn the_main_line_beats_a_variation_even_if_the_variation_is_earlier() {
    // Variation first in the text, main line later (code-built game).
    let (_fx, mut db) = db_of(vec![game(
        "g1f3 (e2e4 e7e5 d2d4) g8f6 f3g1 f6g8 e2e4 e7e5 d2d4",
    )]);
    let mut q = BoardSearch::new(pos_after("e2e4 e7e5"));
    q.in_variations = true;
    assert_eq!(values(&run_board(&mut db, &q).0), vec![(1, 7)]);
    // Same with the colours ignored and the flipped image in the variation.
    q.ignore_colors = true;
    assert_eq!(values(&run_board(&mut db, &q).0), vec![(1, 7)]);
}

#[test]
fn the_main_normal_pass_beats_flipped_and_variation_hits() {
    // The main
    // line reaches the normal target (White to move, Ke1/Ke8) at ply 3; the
    // main normal pass goes first, so the value is 4, not 1. The variation
    // (a null move) would give the normal target at its branch.
    let start = "4k3/p7/8/8/8/8/P7/3K4 b - - 0 1";
    let (_fx, mut db) = db_of(vec![game_fen(start, "e8d8 d1e1 d8e8 (0000)")]);
    let target = fen("4k3/p7/8/8/8/8/P7/4K3 w - - 0 1");
    let mut q = BoardSearch::new(target);
    q.ignore_colors = true;
    q.in_variations = true;
    let (f, _) = run_board(&mut db, &q);
    // Main normal: Ke8/Ke1 white to move happens at ply 3 (value 4).
    assert_eq!(values(&f), vec![(1, 4)]);
}

#[test]
fn variation_ply_is_limited_to_255() {
    let moves = format!("{} e2e4 e7e5 (e7e6 d2d4) g1f3", cs::shuffle(300));
    let (_fx, mut db) = db_of(vec![game(&moves)]);
    let mut q = BoardSearch::new(pos_after("e2e4 e7e6 d2d4"));
    q.in_variations = true;
    // The variation branches at ply 301 (wrapping 302 in a byte gives 46).
    assert_eq!(values(&run_board(&mut db, &q).0), vec![(1, 255)]);
}

#[test]
fn games_without_variations_ignore_the_option() {
    let mut db = fixture();
    for k in [
        BoardMatch::Exact,
        BoardMatch::Pawns,
        BoardMatch::Files,
        BoardMatch::Material,
    ] {
        let mut q = kind(pos_after("e2e4 e7e5 g1f3 b8c6"), k);
        let plain = run_board(&mut db, &q).0;
        q.in_variations = true;
        let with = run_board(&mut db, &q).0;
        // Only game 4 has variations; every other value is identical.
        for n in [1, 2, 3] {
            assert_eq!(plain.get(n), with.get(n), "game {n} {k:?}");
        }
    }
}

// ----------------------------------------------------------- limit 255

#[test]
fn a_match_after_ply_254_is_255_for_every_kind() {
    let moves = format!("{} e2e4 d7d5 e4d5", cs::shuffle(300));
    let (_fx, mut db) = db_of(vec![game(&moves)]);
    for k in [
        BoardMatch::Exact,
        BoardMatch::Pawns,
        BoardMatch::Files,
        BoardMatch::Material,
    ] {
        let target = if k == BoardMatch::Material {
            Position::new()
        } else {
            pos_after("e2e4 d7d5 e4d5")
        };
        let (f, _) = run_board(&mut db, &kind(target, k));
        // Material of the start position holds from ply 0.
        let expected = if k == BoardMatch::Material { 1 } else { 255 };
        assert_eq!(values(&f), vec![(1, expected)], "{k:?}");
    }
    // Boundary: the position at ply 253 has value 254, at ply 254 value 255.
    let (_fx, mut db) = db_of(vec![
        game(&format!("{} e2e4", cs::shuffle(252))),
        game(&format!("{} e2e4 e7e5", cs::shuffle(252))),
    ]);
    let (f, _) = run_board(&mut db, &BoardSearch::new(pos_after("e2e4")));
    assert_eq!(values(&f), vec![(1, 254), (2, 254)]);
    let (f, _) = run_board(&mut db, &BoardSearch::new(pos_after("e2e4 e7e5")));
    assert_eq!(values(&f), vec![(2, 255)]);
}

// -------------------------------------------- shortcuts and own start

#[test]
fn an_index_that_lies_never_removes_a_game_with_its_own_start() {
    // Start games with a promotion available; the index claims "all the
    // material is left" and "16 pawn changes".
    let start = "7k/P6P/8/8/8/8/p6p/K7 w - - 0 1";
    let (fx, db) = db_of(vec![
        game_fen(start, "a7a8q h8g7"),
        game_fen(start, "a7a8r h8g7"),
    ]);
    drop(db);
    cs::spoil_index(&fx, 1);
    cs::empty_index(&fx, 2);
    let mut db = fx.open();
    for (k, target) in [
        (BoardMatch::Exact, pos_after_fen(start, "a7a8q")),
        (BoardMatch::Pawns, pos_after_fen(start, "a7a8q")),
        (BoardMatch::Material, pos_after_fen(start, "a7a8q")),
        (BoardMatch::Files, pos_after_fen(start, "a7a8q")),
    ] {
        let (f, _) = run_board(&mut db, &kind(target, k));
        assert!(
            f.contains(1),
            "{k:?}: spoiled index must not discard a START game"
        );
    }
    // The empty index must not make the second game wrongly match either.
    let (f, _) = run_board(
        &mut db,
        &kind(pos_after_fen(start, "a7a8q"), BoardMatch::Exact),
    );
    assert!(!f.contains(2));
}

fn pos_after_fen(start: &str, ucis: &str) -> Position {
    let mut pos = fen(start);
    for u in ucis.split_whitespace() {
        let mv = pos.parse_uci(u).unwrap();
        pos.play(mv).unwrap();
    }
    pos
}

// ---------------------------------------------------- operation, bank

#[test]
fn the_first_character_chooses_the_operation() {
    for (text, op) in [
        ("A", FilterOp::And),
        ("and", FilterOp::And),
        ("0", FilterOp::And),
        ("O", FilterOp::Or),
        ("or", FilterOp::Or),
        ("1", FilterOp::Or),
        ("R", FilterOp::Reset),
        ("2", FilterOp::Reset),
        ("zzz", FilterOp::Reset),
        ("", FilterOp::Reset),
    ] {
        assert_eq!(FilterOp::from_scid_str(text), op, "{text:?}");
    }
}

#[test]
fn and_or_reset_keep_values_and_zero_non_matches_even_in_or() {
    let mut db = fixture();
    let target = pos_after("e2e4 e7e5 g1f3 b8c6");
    let mut seed = Filter::new(10);
    seed.fill(0);
    seed.set(1, 33); // matches
    seed.set(3, 44); // does not match Exact
    seed.set(10, 55); // does not match
    let run = |op: FilterOp, db: &mut mychess_db::PgnDatabase| {
        let mut f = seed.clone();
        let q = BoardSearch {
            op,
            ..BoardSearch::new(target.clone())
        };
        let r = search_board(db, &q, &mut f, &mut NoProgress, &CancelToken::new()).unwrap();
        (values(&f), r)
    };
    let (and, r) = run(FilterOp::And, &mut db);
    assert_eq!(and, vec![(1, 5)]);
    assert_eq!(r.start_count, 3);
    // OR: games already in keep their value untouched, even when they do not
    // match; the others come in with the search value.
    let (or, r) = run(FilterOp::Or, &mut db);
    assert_eq!(or, vec![(1, 33), (2, 5), (3, 44), (4, 5), (10, 55)]);
    assert_eq!(r.start_count, 10);
    let (reset, _) = run(FilterOp::Reset, &mut db);
    assert_eq!(reset, vec![(1, 5), (2, 5), (4, 5)]);
}

#[test]
fn another_database_only_changes_its_own_filter_and_needs_its_own_size() {
    let (_fx, mut mine) = db_of(vec![game("e2e4 e7e5"), game("d2d4")]);
    let mut other = fixture();
    let position = pos_after("e2e4 e7e5");
    let mut mine_filter = Filter::new(2);
    let mut other_filter = Filter::new(10);
    let before = mine_filter.clone();
    let q = BoardSearch::new(position);
    search_board(
        &mut other,
        &q,
        &mut other_filter,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(mine_filter, before, "the current filter must not change");
    assert_eq!(values(&other_filter), vec![(1, 3), (3, 3), (4, 3)]);
    // The same position searched in the current database.
    search_board(
        &mut mine,
        &q,
        &mut mine_filter,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(values(&mine_filter), vec![(1, 3)]);
    // The filter of one database cannot serve the other.
    let mut wrong = Filter::new(2);
    let err = search_board(
        &mut other,
        &q,
        &mut wrong,
        &mut NoProgress,
        &CancelToken::new(),
    );
    assert!(matches!(err, Err(SearchError::FilterSize { .. })));
    assert_eq!(wrong, Filter::new(2));
}

// ------------------------------------- unreadable, empty record, cancel

#[test]
fn records_that_are_gone_or_broken_leave_or_are_reported() {
    let (fx, db) = db_of(vec![game("e2e4"), game("e2e4"), game("e2e4")]);
    drop(db);
    base::zero_length(&fx, 1);
    base::break_offset(&fx, 2);
    let mut db = fx.open();
    let (f, r) = run_board(&mut db, &BoardSearch::new(Position::new()));
    assert_eq!(numbers(&f), vec![3]);
    assert_eq!(r.unreadable, vec![2]);
    assert_eq!(r.unreadable_total, 1);
    // OR: a broken game that was outside stays outside.
    let mut seed = Filter::new(3);
    seed.fill(0);
    let q = BoardSearch {
        op: FilterOp::Or,
        ..BoardSearch::new(Position::new())
    };
    search_board(&mut db, &q, &mut seed, &mut NoProgress, &CancelToken::new()).unwrap();
    assert_eq!(numbers(&seed), vec![3]);
}

#[test]
fn cancelling_keeps_the_filter_byte_for_byte_and_is_reported() {
    let (_fx, mut db) = cs::crowd(5000, &["e2e4 e7e5", "d2d4", "e2e4 c7c5 g1f3"]);
    let mut filter = Filter::new(db.len());
    filter.fill(0);
    filter.set(7, 200);
    let before = filter.as_bytes().to_vec();
    let token = CancelToken::new();
    let mut sink = cs::Recorder::cancelling(&token);
    let q = BoardSearch {
        op: FilterOp::Or,
        ..BoardSearch::new(pos_after("e2e4 e7e5"))
    };
    let report = search_board(&mut db, &q, &mut filter, &mut sink, &token).unwrap();
    assert!(report.interrupted);
    assert_eq!(filter.as_bytes(), &before[..]);
    assert_eq!(report.matched, 1);
    // Cancelled before starting.
    let token = CancelToken::new();
    token.cancel();
    let report = search_board(&mut db, &q, &mut filter, &mut NoProgress, &token).unwrap();
    assert!(report.interrupted);
    assert_eq!(filter.as_bytes(), &before[..]);
}

#[test]
fn progress_never_goes_back_and_ends_complete() {
    let (_fx, mut db) = cs::crowd(5000, &["e2e4 e7e5", "d2d4"]);
    let mut filter = Filter::new(db.len());
    let mut sink = cs::Recorder::recording();
    search_board(
        &mut db,
        &BoardSearch::new(Position::new()),
        &mut filter,
        &mut sink,
        &CancelToken::new(),
    )
    .unwrap();
    assert!(!sink.calls.is_empty());
    assert!(sink.calls.windows(2).all(|w| w[0].0 <= w[1].0));
    assert!(sink.calls.iter().all(|&(d, t)| d <= t && t == 5000));
    assert_eq!(*sink.calls.last().unwrap(), (5000, 5000));
}

#[test]
fn one_or_many_threads_and_the_oracle_agree_over_several_blocks() {
    let templates = [
        "e2e4 e7e5 g1f3",
        "e2e4 e7e5 (c7c5 g1f3) g1f3 b8c6",
        "d2d4 d7d5",
        "",
        "e2e4 c7c5",
    ];
    let (_fx, mut db) = cs::crowd(mychess_db::BLOCK_GAMES * 2 + 37, &templates);
    for (k, flip, vars) in [
        (BoardMatch::Exact, false, false),
        (BoardMatch::Pawns, true, true),
        (BoardMatch::Files, false, true),
        (BoardMatch::Material, true, false),
    ] {
        let q = BoardSearch {
            kind: k,
            ignore_colors: flip,
            in_variations: vars,
            ..BoardSearch::new(pos_after("e2e4 e7e5"))
        };
        let one = cs::with_threads(1, || run_board(&mut db, &q).0);
        let many = cs::with_threads(4, || run_board(&mut db, &q).0);
        assert_eq!(one.as_bytes(), many.as_bytes(), "{k:?}");
        for (i, t) in templates.iter().enumerate() {
            let expected = oracle_board(&game(t), k, &q.position, flip, vars);
            for n in (i + 1..=one.len() as usize).step_by(templates.len() * 97) {
                assert_eq!(
                    Some(one.get(n as u32)).filter(|&v| v != 0),
                    expected,
                    "{k:?} template {i} game {n}"
                );
            }
        }
    }
}
