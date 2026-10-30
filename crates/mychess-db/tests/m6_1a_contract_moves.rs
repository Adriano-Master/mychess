//! Contract tests of the search by a sequence of moves, written from the
//! spec `docs/specs/m6-1-buscas-avancadas.md` (comportamento 25 a 29, regras
//! 25 a 29 of "Lances", D3225 to D3230, D3252 to D3254) over the PGN fixture
//! `fixtures/m6_1a_contract_moves.pgn` and games built in code.
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

use mychess_db::{
    CancelToken, CheckTest, Filter, FilterOp, GameSource, MAX_MOVE_TOKENS, MoveToken, MovesSearch,
    NoProgress, SearchError, SideFilter, search_moves,
};
use pretty_assertions::assert_eq;
use support::{db_of, game, numbers, pgn_fixture, run_moves, values};

fn fixture() -> mychess_db::PgnDatabase {
    let db = pgn_fixture("m6_1a_contract_moves.pgn");
    assert_eq!(db.len(), 11, "fixture must load");
    db
}

fn parse(text: &str, side: SideFilter, test: CheckTest) -> Result<MovesSearch, SearchError> {
    MovesSearch::parse(text, side, test, FilterOp::Reset)
}

fn q(text: &str) -> MovesSearch {
    parse(text, SideFilter::Both, CheckTest::No).unwrap()
}

fn vals<D: GameSource + ?Sized>(db: &mut D, s: &MovesSearch) -> Vec<(u32, u8)> {
    values(&run_moves(db, s).0)
}

// ------------------------------------------------------------ matching

#[test]
fn a_sequence_needs_consecutive_moves_and_the_value_is_the_first_ply_plus_two() {
    let mut db = fixture();
    assert_eq!(vals(&mut db, &q("e4 e5 Nf3")), vec![(1, 2), (7, 2)]);
    assert_eq!(vals(&mut db, &q("Nf3 Nc6 Bb5")), vec![(1, 4)]);
    // Not consecutive: Bb5 does not follow Nf3 directly.
    assert_eq!(vals(&mut db, &q("Nf3 Bb5")), Vec::<(u32, u8)>::new());
    assert_eq!(vals(&mut db, &q("Nf3")), vec![(1, 4), (6, 3), (7, 4)]);
}

#[test]
fn the_wildcard_is_any_real_move_and_does_not_match_the_end_of_the_game() {
    let mut db = fixture();
    assert_eq!(vals(&mut db, &q("e4 ? Nf3")), vec![(1, 2), (7, 2)]);
    assert_eq!(vals(&mut db, &q("e4 ?")), vec![(1, 2), (4, 2), (7, 2)]);
    // Game 11 ends with 1.e4; game 6 ends with 2.Nf3.
    assert_eq!(numbers(&run_moves(&mut db, &q("e4")).0), vec![1, 4, 7, 11]);
    assert_eq!(numbers(&run_moves(&mut db, &q("Nf3 ?")).0), vec![1, 7]);
    assert_eq!(numbers(&run_moves(&mut db, &q("e4 ? ?")).0), vec![1, 4, 7]);
    // A third real move is needed in game 4 (e4 d5 exd5) but not in game 11.
    assert!(!numbers(&run_moves(&mut db, &q("e4 ? ?")).0).contains(&11));
}

#[test]
fn the_side_filters_only_the_first_move() {
    let mut db = fixture();
    let s = |text, side| parse(text, side, CheckTest::No).unwrap();
    assert_eq!(
        vals(&mut db, &s("e5 Nf3", SideFilter::Black)),
        vec![(1, 3), (6, 2), (7, 3)]
    );
    assert_eq!(
        vals(&mut db, &s("e5 Nf3", SideFilter::White)),
        Vec::<(u32, u8)>::new()
    );
    assert_eq!(
        vals(&mut db, &s("e5 Nf3", SideFilter::Both)),
        vec![(1, 3), (6, 2), (7, 3)]
    );
    // The second move belongs to the other side whatever the filter says.
    assert_eq!(
        vals(&mut db, &s("Nf3 Nc6", SideFilter::White)),
        vec![(1, 4), (7, 4)]
    );
    assert_eq!(
        vals(&mut db, &s("Nf3 Nc6", SideFilter::Black)),
        Vec::<(u32, u8)>::new()
    );
    // A Black-to-move start (game 6) is read from its FEN.
    assert_eq!(
        vals(&mut db, &s("e5", SideFilter::Black)).first(),
        Some(&(1, 3))
    );
    assert!(vals(&mut db, &s("e5", SideFilter::Black)).contains(&(6, 2)));
}

// --------------------------------------------------------- check test

#[test]
fn check_test_and_the_suffix_raise() {
    let mut db = fixture();
    let run = |db: &mut mychess_db::PgnDatabase, text, test| {
        let s = parse(text, SideFilter::Both, test).unwrap();
        (s.check_test, numbers(&run_moves(db, &s).0))
    };
    // Mate in game 3, check in game 5.
    assert_eq!(run(&mut db, "Qh4", CheckTest::No), (CheckTest::No, vec![3]));
    assert_eq!(
        run(&mut db, "Qh4", CheckTest::Check),
        (CheckTest::Check, vec![])
    );
    assert_eq!(
        run(&mut db, "Qh4", CheckTest::Mate),
        (CheckTest::Mate, vec![])
    );
    assert_eq!(
        run(&mut db, "Qh4+", CheckTest::No),
        (CheckTest::Check, vec![3])
    );
    assert_eq!(
        run(&mut db, "Qh4#", CheckTest::No),
        (CheckTest::Mate, vec![3])
    );
    assert_eq!(
        run(&mut db, "Qh4+", CheckTest::Mate),
        (CheckTest::Mate, vec![])
    );
    assert_eq!(run(&mut db, "a8=Q", CheckTest::No).1, vec![5]);
    assert_eq!(run(&mut db, "a8=Q", CheckTest::Check).1, Vec::<u32>::new());
    assert_eq!(
        run(&mut db, "a8=Q+", CheckTest::No),
        (CheckTest::Check, vec![5])
    );
    assert_eq!(
        run(&mut db, "a8Q+", CheckTest::No),
        (CheckTest::Check, vec![5])
    );
}

// ------------------------------------------------------ reading words

#[test]
fn words_are_read_tolerantly() {
    let mut db = fixture();
    let ok = |db: &mut mychess_db::PgnDatabase, text: &str| numbers(&run_moves(db, &q(text)).0);
    assert_eq!(ok(&mut db, "1.e4 1...e5 2.Nf3"), vec![1, 7]);
    assert_eq!(ok(&mut db, "12. e4"), ok(&mut db, "e4"));
    assert_eq!(ok(&mut db, "Nf6 0-0"), vec![1]);
    assert_eq!(ok(&mut db, "Nf6 O-O"), vec![1]);
    assert_eq!(ok(&mut db, "exd5!?"), vec![4]);
    assert_eq!(ok(&mut db, "exd5"), vec![4]);
    for annotated in ["e4!", "e4!!", "e4?", "e4??", "e4!?", "e4?!"] {
        assert_eq!(ok(&mut db, annotated), vec![1, 4, 7, 11], "{annotated}");
        assert!(
            matches!(q(annotated).tokens[0], MoveToken::San(_)),
            "{annotated} is not the wildcard"
        );
    }
    // Promotion with or without "=", the SAN of the engine of the spec.
    assert_eq!(ok(&mut db, "a8=Q"), vec![5]);
    assert_eq!(ok(&mut db, "a8Q"), vec![5]);
    // The numbers and nothing else make an empty list.
    assert_eq!(
        parse("1. 2...", SideFilter::Both, CheckTest::No).unwrap_err(),
        SearchError::EmptyMoveList
    );
}

#[test]
fn hostile_words_are_bad_tokens_and_never_panic() {
    let bad = [
        "Ng1-f3", "e2e4", "Z9", "e9", "e0", "i4", "ed5", "N", "Nf", "O-O-O-O", "=Q", "xx",
        "\u{e9}4", "e4\u{0}", "Kxe", "e8=K", "9", "O-O-", "N**", "abc",
    ];
    for word in bad {
        let r = parse(&format!("e4 {word}"), SideFilter::Both, CheckTest::No);
        assert!(
            matches!(r, Err(SearchError::BadMoveToken { .. })),
            "{word:?} gave {r:?}"
        );
    }
    let long = "e".repeat(5000);
    match parse(&long, SideFilter::Both, CheckTest::No) {
        Err(SearchError::BadMoveToken { token }) => assert!(token.chars().count() <= 32),
        other => panic!("{other:?}"),
    }
    // Lower case pieces are not SAN: they never match a Nf3.
    for word in ["nf3", "NF3"] {
        let mut db = fixture();
        match parse(word, SideFilter::Both, CheckTest::No) {
            Err(_) => {}
            Ok(s) => assert!(numbers(&run_moves(&mut db, &s).0).is_empty(), "{word}"),
        }
    }
    let many = vec!["e4"; MAX_MOVE_TOKENS + 1].join(" ");
    assert!(matches!(
        parse(&many, SideFilter::Both, CheckTest::No),
        Err(SearchError::BadMoveToken { .. })
    ));
    let max = vec!["e4"; MAX_MOVE_TOKENS].join(" ");
    assert!(parse(&max, SideFilter::Both, CheckTest::No).is_ok());
}

#[test]
fn an_empty_list_and_a_leading_wildcard_are_refused_before_any_work() {
    for text in ["", "   ", "\t\n"] {
        assert_eq!(
            parse(text, SideFilter::Both, CheckTest::No).unwrap_err(),
            SearchError::EmptyMoveList
        );
    }
    for text in ["?", "? e4", "  ? ? e4"] {
        assert_eq!(
            parse(text, SideFilter::Both, CheckTest::No).unwrap_err(),
            SearchError::FirstMoveIsWildcard,
            "{text:?}"
        );
    }
    // Built by hand, the same checks hold when running.
    let (_fx, mut db) = db_of(vec![game("e2e4")]);
    let mut s = q("e4");
    s.tokens.clear();
    let mut filter = Filter::new(1);
    let before = filter.clone();
    let r = search_moves(
        &mut db,
        &s,
        &mut filter,
        &mut NoProgress,
        &CancelToken::new(),
    );
    assert!(r.is_err());
    assert_eq!(filter, before);
}

#[test]
fn disambiguation_must_be_the_generated_one() {
    let mut db = fixture();
    let mut found = |text: &str| numbers(&run_moves(&mut db, &q(text)).0);
    assert_eq!(found("Rad1"), vec![9]);
    assert_eq!(found("Rd1"), Vec::<u32>::new());
    assert_eq!(found("R1d1"), Vec::<u32>::new());
    assert_eq!(found("Rhd1"), Vec::<u32>::new());
    // Knights on the same file: the rank decides.
    assert_eq!(found("N3c2"), vec![10]);
    assert_eq!(found("Nac2"), Vec::<u32>::new());
    assert_eq!(found("N1c2"), Vec::<u32>::new());
    assert_eq!(found("Nc2"), Vec::<u32>::new());
    // The other knight has no competitor for b3.
    assert_eq!(found("N3c2 ? Nb3"), vec![10]);
    assert_eq!(found("Nab3"), Vec::<u32>::new());
}

// ---------------------------------------------------- where it starts

#[test]
fn the_first_move_may_start_up_to_ply_254_and_the_value_is_limited() {
    let shuffle = |n: usize| cs::shuffle(n);
    let (_fx, mut db) = db_of(vec![
        game(&format!("{} e2e4", shuffle(252))),      // e4 at ply 252
        game(&format!("{} e2e4 e7e5", shuffle(252))), // e5 at ply 253
        game(&format!("{} e2e4 e7e5 g1f3", shuffle(252))), // Nf3 at ply 254
        game(&format!("{} e2e4 e7e5 g1f3 b8c6", shuffle(252))), // Nc6 at ply 255
        game(&format!("{} e2e4 ?", shuffle(0)).replace(" ?", "")),
    ]);
    assert_eq!(
        vals(&mut db, &q("e4")),
        vec![(1, 254), (2, 254), (3, 254), (4, 254), (5, 2)]
    );
    assert_eq!(vals(&mut db, &q("e5")), vec![(2, 255), (3, 255), (4, 255)]);
    // Nc6 at ply 255 is beyond the start window.
    let nc6 = vals(&mut db, &q("Nc6"));
    assert!(
        !nc6.iter().any(|(n, _)| *n == 4),
        "ply 255 is beyond the start window: {nc6:?}"
    );
}

#[test]
fn the_start_window_is_an_option_and_the_sequence_may_run_past_it() {
    let moves = format!("{} e2e4 e7e5 g1f3", cs::shuffle(252));
    let (_fx, mut db) = db_of(vec![game(&moves)]);
    // e4 starts at ply 252 and continues to ply 254 and beyond.
    assert_eq!(vals(&mut db, &q("e4 e5 Nf3")), vec![(1, 254)]);
    let mut s = q("e5");
    s.start_plies = (0, 252);
    assert!(vals(&mut db, &s).is_empty());
    s.start_plies = (253, 253);
    assert_eq!(vals(&mut db, &s), vec![(1, 255)]);
    s.start_plies = (0, 1000);
    assert!(!vals(&mut db, &s).is_empty());
}

// ---------------------------------------------------------- variations

#[test]
fn variations_are_searched_only_when_asked_and_the_main_line_wins() {
    let mut db = fixture();
    let mut s = q("c5 Nf3 d6");
    assert!(vals(&mut db, &s).is_empty());
    s.in_variations = true;
    // The variation starts replacing 1...e5 (ply 1): value 1 + 2.
    assert_eq!(vals(&mut db, &s), vec![(7, 3)]);
    // Nested variation: the value is still the one of the outermost branch.
    let mut s = q("Nc6 d4");
    s.in_variations = true;
    assert_eq!(vals(&mut db, &s), vec![(7, 3)]);
    // Found in the main line first: no change of value.
    let mut s = q("Nf3 Nc6");
    s.in_variations = true;
    assert_eq!(vals(&mut db, &s), vec![(1, 4), (7, 4)]);
    // Sequence that starts in the main line and goes on in a variation.
    let mut s = q("e4 c5 Nf3");
    assert!(vals(&mut db, &s).is_empty());
    s.in_variations = true;
    assert_eq!(vals(&mut db, &s), vec![(7, 2)]);
    // The first move in a variation, found only after the main line failed.
    let mut s = q("Nf3 d6");
    s.in_variations = true;
    assert_eq!(vals(&mut db, &s), vec![(7, 3)]);
}

// ----------------------------------------------- null move and no pre-test

#[test]
fn the_null_move_is_written_with_two_dashes() {
    let (_fx, mut db) = db_of(vec![game("e2e4 0000 d2d4"), game("e2e4 e7e5 d2d4")]);
    let s = parse("e4 -- d4", SideFilter::Both, CheckTest::No).unwrap();
    assert_eq!(vals_si4(&mut db, &s), vec![(1, 2)]);
}

fn vals_si4(db: &mut mychess_db::si4::Si4Database, s: &MovesSearch) -> Vec<(u32, u8)> {
    values(&run_moves(db, s).0)
}

#[test]
fn the_pre_test_never_changes_the_answer() {
    let mut db = fixture();
    for text in [
        "e4",
        "e4 e5 Nf3",
        "e4 ? Nf3",
        "Nf3 ?",
        "exd5",
        "O-O",
        "0-0",
        "Qh4#",
        "a8=Q+",
        "Rad1",
        "N3c2",
        "Nf3 Nc6 Bb5 a6",
        "d4 d5 c4 e6",
        "e5 Nf3",
        "Bb5 ? Ba4",
    ] {
        for in_variations in [false, true] {
            let mut s = q(text);
            s.in_variations = in_variations;
            let fast = run_moves(&mut db, &s).0;
            s.fast_path = false;
            let slow = run_moves(&mut db, &s).0;
            assert_eq!(
                fast.as_bytes(),
                slow.as_bytes(),
                "{text} vars={in_variations}"
            );
        }
    }
}

// -------------------------------------------------- operation and scan

#[test]
fn and_or_reset_follow_the_operation() {
    let mut db = fixture();
    let mut seed = Filter::new(11);
    seed.fill(0);
    seed.set(1, 90); // matches "e4"
    seed.set(2, 91); // does not
    let go = |op, db: &mut mychess_db::PgnDatabase| {
        let mut f = seed.clone();
        let s = MovesSearch { op, ..q("e4") };
        let rep = search_moves(db, &s, &mut f, &mut NoProgress, &CancelToken::new()).unwrap();
        (values(&f), rep.start_count)
    };
    assert_eq!(go(FilterOp::And, &mut db), (vec![(1, 2)], 2));
    assert_eq!(
        go(FilterOp::Or, &mut db),
        (vec![(1, 90), (2, 91), (4, 2), (7, 2), (11, 2)], 11)
    );
    assert_eq!(
        go(FilterOp::Reset, &mut db),
        (vec![(1, 2), (4, 2), (7, 2), (11, 2)], 11)
    );
}

#[test]
fn broken_records_are_listed_and_empty_ones_leave_the_filter() {
    let (fx, db) = db_of(vec![game("e2e4"), game("e2e4"), game("e2e4"), game("d2d4")]);
    drop(db);
    base::zero_length(&fx, 1);
    base::break_offset(&fx, 2);
    let mut db = fx.open();
    let (f, rep) = run_moves(&mut db, &q("e4"));
    assert_eq!(numbers(&f), vec![3]);
    assert_eq!(rep.unreadable, vec![2]);
    assert_eq!(rep.unreadable_total, 1);
}

#[test]
fn cancel_keeps_the_filter_and_progress_ends_complete() {
    let (_fx, mut db) = cs::crowd(5000, &["e2e4 e7e5", "d2d4 d7d5"]);
    let mut filter = Filter::new(db.len());
    filter.fill(0);
    filter.set(3, 5);
    let before = filter.as_bytes().to_vec();
    let token = CancelToken::new();
    let mut sink = cs::Recorder::cancelling(&token);
    let s = MovesSearch {
        op: FilterOp::Or,
        ..q("e4")
    };
    let rep = search_moves(&mut db, &s, &mut filter, &mut sink, &token).unwrap();
    assert!(rep.interrupted);
    assert_eq!(filter.as_bytes(), &before[..]);
    let mut sink = cs::Recorder::recording();
    search_moves(&mut db, &s, &mut filter, &mut sink, &CancelToken::new()).unwrap();
    assert!(sink.calls.windows(2).all(|w| w[0].0 <= w[1].0));
    assert_eq!(*sink.calls.last().unwrap(), (5000, 5000));
}

#[test]
fn threads_and_the_pre_test_do_not_change_the_bytes() {
    let templates = [
        "e2e4 e7e5 g1f3",
        "d2d4 d7d5 c2c4",
        "",
        "e2e4 e7e5 (c7c5 g1f3) b1c3",
    ];
    let (_fx, mut db) = cs::crowd(mychess_db::BLOCK_GAMES * 2 + 5, &templates);
    for in_variations in [false, true] {
        let mut s = q("e4 ? Nf3");
        s.in_variations = in_variations;
        let one = cs::with_threads(1, || run_moves(&mut db, &s).0);
        let many = cs::with_threads(4, || run_moves(&mut db, &s).0);
        s.fast_path = false;
        let slow = cs::with_threads(3, || run_moves(&mut db, &s).0);
        assert_eq!(one.as_bytes(), many.as_bytes());
        assert_eq!(one.as_bytes(), slow.as_bytes());
        assert_eq!(one.get(1), 2);
        assert_eq!(one.get(2), 0);
        assert_eq!(one.get(3), 0);
        assert_eq!(one.get(4), u8::from(in_variations) * 2);
    }
}
