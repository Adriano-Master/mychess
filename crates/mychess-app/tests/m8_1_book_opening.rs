//! M8-1a contract tests of the book and of the specific opening (spec
//! D1234-D1236, D1246, D1247) and of the `BookProbe` served by `PlayBook`.
#![allow(
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_1_common;

use std::collections::BTreeSet;
use std::time::Instant;

use m8_1_common::*;
use mychess_app::{
    BookSelection, OpeningLine, PlayBook, PlayController, PlayOutput, PlayPhase, PlayQuestion,
    PlayRemark, StartFrom, TimeMode, load_play_book,
};
use mychess_book::Book;
use mychess_core::{Color, GameResult, Position};
use rstest::rstest;

const DEPTH: TimeMode = TimeMode::Depth(2);

fn with_book(color: Color, book: PlayBook, mode: TimeMode) -> mychess_app::PlaySetup {
    let mut s = setup(color, mode);
    s.book = Some(book);
    s
}

fn opening(color: Color, text: &str) -> mychess_app::PlaySetup {
    let mut s = setup(color, DEPTH);
    s.config.start = StartFrom::Opening(OpeningLine::parse(text).expect("line"));
    s
}

// ---- the book plays for the engine ----

#[test]
fn the_engine_plays_the_best_book_move_at_once_without_asking_the_engine_or_the_clock() {
    let (c, out, t0) = begin(with_book(
        Color::Black,
        mini_book(BookSelection::Best),
        blitz(),
    ));
    assert!(thinks(&out).is_empty(), "no search for a book move");
    let PlayOutput::Moved(m) = out[0].clone() else {
        panic!("{out:?}");
    };
    assert_eq!(m.mv, Position::new().parse_uci("e2e4").unwrap());
    assert!(m.free);
    assert_eq!(m.spent, std::time::Duration::ZERO);
    assert_eq!(c.phase(), PlayPhase::Player);
    // D1248: no time, no increment for a free move.
    assert_eq!(c.shown(Color::White, t0 + 3 * SEC), 300 * SEC);
    assert_eq!(c.shown(Color::Black, t0 + 3 * SEC), 297 * SEC);
}

#[test]
fn weighted_selection_follows_the_weights_over_many_seeds() {
    let mut firsts = Vec::new();
    for seed in 0..200u64 {
        let mut s = setup(Color::Black, DEPTH);
        s.book = Some(mini_book(BookSelection::Weighted));
        s.seed = seed;
        let (c, out, _) = begin(s);
        assert!(thinks(&out).is_empty());
        firsts.push(c.game().mv(c.tip()).expect("mv"));
    }
    let e4 = Position::new().parse_uci("e2e4").expect("uci");
    let d4 = Position::new().parse_uci("d2d4").expect("uci");
    let set: BTreeSet<_> = firsts.iter().map(|m| format!("{m:?}")).collect();
    assert_eq!(set.len(), 2, "both book moves get played: {set:?}");
    let n_e4 = firsts.iter().filter(|m| **m == e4).count();
    let n_d4 = firsts.iter().filter(|m| **m == d4).count();
    assert_eq!(n_e4 + n_d4, 200);
    assert!(n_e4 > n_d4, "e4 (70) must beat d4 (30): {n_e4} vs {n_d4}");
}

#[test]
fn the_same_seed_gives_the_same_book_move() {
    let play = || {
        let mut s = setup(Color::Black, DEPTH);
        s.book = Some(mini_book(BookSelection::Weighted));
        s.seed = 12345;
        let (c, _, _) = begin(s);
        c.game().mv(c.tip()).expect("mv")
    };
    assert_eq!(play(), play());
}

#[test]
fn the_last_book_move_of_the_engine_is_commented_and_the_book_stops_guiding() {
    // Book: 1.e4 e5 2.Nf3. The engine (White) plays e4, the player e5, the
    // engine Nf3 (the last move of the book), then it has to think.
    let (mut c, _, t0) = begin(with_book(
        Color::Black,
        mini_book(BookSelection::Best),
        DEPTH,
    ));
    assert!(c.book_active());
    let out = c.player_move(uci(&c, "e7e5"), t0);
    assert!(
        comment_at_tip(&c).is_some_and(|t| t == "Last book move (book)"),
        "the engine's Nf3 closes the book: {:?}",
        comment_at_tip(&c)
    );
    assert!(thinks(&out).is_empty(), "Nf3 is a book move too");
    assert!(!c.book_active());
    assert_eq!(mainline_ucis(c.game()), ["e2e4", "e7e5", "g1f3"]);
    // Now the book is over: the next player move makes the engine think.
    let out = c.player_move(uci(&c, "b8c6"), t0);
    assert_eq!(thinks(&out).len(), 1);
}

#[test]
fn moves_inside_the_book_get_no_comment() {
    let (c, _, _) = begin(with_book(
        Color::Black,
        mini_book(BookSelection::Best),
        DEPTH,
    ));
    assert_eq!(
        comment_at_tip(&c),
        None,
        "e4 is in the book, with more to follow"
    );
}

// ---- the player leaves the book (D1234, D1247) ----

fn player_white_book() -> (PlayController, Instant) {
    // The book knows 1.e4 and 1.d4 for White, nothing else.
    let (c, _, t0) = begin(with_book(
        Color::White,
        mini_book(BookSelection::Best),
        blitz(),
    ));
    (c, t0)
}

#[test]
fn leaving_the_book_while_it_still_has_moves_asks_and_stops_the_clock() {
    let (mut c, t0) = player_white_book();
    // c4 is not in the book (e4 and d4 are).
    let c4 = uci(&c, "c2c4");
    let out = c.player_move(c4, t0);
    assert_eq!(out, [PlayOutput::Ask(PlayQuestion::LeaveBook { mv: c4 })]);
    assert_eq!(
        c.phase(),
        PlayPhase::Asking(PlayQuestion::LeaveBook { mv: c4 })
    );
    assert_eq!(c.game().mainline().count(), 0, "not played yet");
    assert!(!c.clock().is_running());
    assert!(c.is_live());
}

#[test]
fn a_move_that_is_in_the_book_never_asks() {
    let (mut c, t0) = player_white_book();
    let out = c.player_move(uci(&c, "d2d4"), t0);
    assert!(!out.iter().any(|o| matches!(o, PlayOutput::Ask(_))));
    assert_eq!(c.game().mainline().count(), 1);
}

#[test]
fn answering_no_takes_the_move_back_and_the_clock_goes_on() {
    let (mut c, t0) = player_white_book();
    let c4 = uci(&c, "c2c4");
    c.player_move(c4, t0 + 5 * SEC);
    let out = c.answer(false, t0 + 60 * SEC);
    assert_eq!(out, [PlayOutput::Notice(PlayRemark::MoveTakenBack)]);
    assert_eq!(c.phase(), PlayPhase::Player);
    assert_eq!(c.game().mainline().count(), 0);
    assert_eq!(c.position().fen(), Position::new().fen());
    assert!(c.clock().is_running());
    assert!(c.book_active(), "the book still guides");
    // The question time was not charged: 5 s before it, none during it.
    assert_eq!(c.shown(Color::White, t0 + 60 * SEC), 295 * SEC);
    // The player can now play a book move.
    let out = c.player_move(uci(&c, "e2e4"), t0 + 61 * SEC);
    assert_eq!(moved_count(&out), 2, "e4 and the engine's e5 from the book");
}

#[test]
fn answering_yes_plays_the_move_comments_it_and_the_engine_thinks() {
    let (mut c, t0) = player_white_book();
    let c4 = uci(&c, "c2c4");
    c.player_move(c4, t0 + 5 * SEC);
    let out = c.answer(true, t0 + 60 * SEC);
    assert_eq!(moved_count(&out), 1);
    assert_eq!(thinks(&out).len(), 1, "out of the book: the engine thinks");
    assert!(!c.book_active());
    let comment = comment_at_tip(&c).expect("comment");
    assert!(
        comment.starts_with("Move out of book (book"),
        "got {comment:?}"
    );
    assert!(comment.contains("e4"), "lists the book moves: {comment:?}");
    // The time before the question counts, the question itself does not.
    assert_eq!(c.shown(Color::White, t0 + 60 * SEC), (300 - 5 + 3) * SEC);
}

#[test]
fn the_time_before_the_question_is_recorded_as_the_time_of_the_move() {
    let (mut c, t0) = player_white_book();
    c.player_move(uci(&c, "c2c4"), t0 + 5 * SEC);
    let out = c.answer(true, t0 + 60 * SEC);
    let PlayOutput::Moved(m) = out[0].clone() else {
        panic!("{out:?}");
    };
    assert_eq!(
        m.spent,
        5 * SEC,
        "the player thought 5 s before the question"
    );
    assert_eq!(c.clock().move_times()[0].spent, 5 * SEC);
}

#[test]
fn an_answer_without_a_question_does_nothing() {
    let (mut c, t0) = player_white_book();
    assert!(c.answer(true, t0).is_empty());
    assert!(c.answer(false, t0).is_empty());
    assert_eq!(c.phase(), PlayPhase::Player);
}

#[test]
fn no_move_and_no_tick_is_accepted_while_a_question_waits() {
    let (mut c, t0) = player_white_book();
    c.player_move(uci(&c, "c2c4"), t0);
    assert_eq!(
        c.player_move(uci(&c, "e2e4"), t0),
        [PlayOutput::Notice(PlayRemark::NotYourTurn)]
    );
    assert!(
        c.tick(t0 + 9_999 * SEC).is_empty(),
        "no flag during the question"
    );
}

#[test]
fn a_book_that_does_not_know_the_position_asks_nothing_and_comments_nothing() {
    // D1247: out of the book since the start (a position the book lacks).
    let mut s = fen_setup(Color::White, DEPTH, "3k4/8/8/8/8/8/4R3/4K3 w - - 0 1");
    s.book = Some(mini_book(BookSelection::Best));
    let (mut c, _, t0) = begin(s);
    let out = c.player_move(uci(&c, "e2e3"), t0);
    assert!(!out.iter().any(|o| matches!(o, PlayOutput::Ask(_))));
    assert_eq!(comment_at_tip(&c), None);
    assert_eq!(
        thinks(&out).len(),
        1,
        "the engine thinks: the book is silent"
    );
}

#[test]
fn the_engine_thinks_for_itself_when_the_book_has_no_move_for_it() {
    let mut s = fen_setup(Color::Black, DEPTH, "3k4/8/8/8/8/8/4R3/4K3 w - - 0 1");
    s.book = Some(mini_book(BookSelection::Best));
    let (_, out, _) = begin(s);
    assert_eq!(thinks(&out).len(), 1);
}

#[test]
fn leaving_the_book_by_the_engine_marks_nothing_when_it_follows_the_book() {
    let (mut c, _, t0) = begin(with_book(
        Color::White,
        mini_book(BookSelection::Best),
        DEPTH,
    ));
    c.player_move(uci(&c, "e2e4"), t0);
    // The engine answered e5 from the book (no search).
    assert_eq!(mainline_ucis(c.game()), ["e2e4", "e7e5"]);
    assert_eq!(comment_at_tip(&c), None, "e5 is in the book, Nf3 follows");
}

#[test]
fn a_book_line_ending_the_book_on_the_players_last_move_is_commented() {
    // After 1.e4 e5 2.Nf3 the book knows nothing: the player's Nf3 is the
    // last book move and gets the comment.
    let book = book_with_lines(&[(&["e2e4", "e7e5", "g1f3"], 50)], BookSelection::Best);
    let (mut c, _, t0) = begin(with_book(Color::White, book, DEPTH));
    c.player_move(uci(&c, "e2e4"), t0);
    let out = c.player_move(uci(&c, "g1f3"), t0);
    assert_eq!(comment_at_tip(&c).as_deref(), Some("Last book move (book)"));
    assert_eq!(thinks(&out).len(), 1);
    assert!(!c.book_active());
}

// ---- the specific opening (D1235, D1246) ----

#[test]
fn the_engine_plays_the_line_for_free_and_the_player_follows_it_without_questions() {
    // Player White: e4 (player) c5 (engine) Nf3 (player) d6 (engine).
    let (mut c, out, t0) = begin(opening(Color::White, "Sicilian: e4 c5 Nf3 d6"));
    assert!(out.is_empty());
    let out = c.player_move(uci(&c, "e2e4"), t0);
    assert!(!out.iter().any(|o| matches!(o, PlayOutput::Ask(_))));
    assert!(thinks(&out).is_empty(), "c5 comes from the line");
    assert_eq!(mainline_ucis(c.game()), ["e2e4", "c7c5"]);
    let out = c.player_move(uci(&c, "g1f3"), t0);
    assert!(thinks(&out).is_empty());
    assert_eq!(mainline_ucis(c.game()), ["e2e4", "c7c5", "g1f3", "d7d6"]);
    assert_eq!(c.phase(), PlayPhase::Player);
    // After the line the engine thinks for itself.
    let out = c.player_move(uci(&c, "d2d4"), t0);
    assert!(!out.iter().any(|o| matches!(o, PlayOutput::Ask(_))));
    assert_eq!(thinks(&out).len(), 1);
}

#[test]
fn the_free_moves_of_the_line_cost_the_engine_no_time_and_no_increment() {
    let (mut c, _, t0) = begin({
        let mut s = opening(Color::White, "A: e4 c5");
        s.config.mode = blitz();
        s
    });
    c.player_move(uci(&c, "e2e4"), t0 + 10 * SEC);
    // Black's clock: nothing charged, no increment, starts at the free move.
    assert_eq!(c.shown(Color::Black, t0 + 10 * SEC), 300 * SEC);
    assert_eq!(c.shown(Color::White, t0 + 10 * SEC), 293 * SEC);
}

#[test]
fn the_engine_with_white_opens_the_line_when_the_game_begins() {
    let (c, out, _) = begin(opening(Color::Black, "Ruy: e4 e5 Nf3 Nc6 Bb5"));
    assert!(thinks(&out).is_empty());
    assert_eq!(mainline_ucis(c.game()), ["e2e4"]);
    assert_eq!(c.phase(), PlayPhase::Player);
}

#[test]
fn leaving_the_line_asks_and_no_undoes_the_move() {
    let (mut c, _, t0) = begin(opening(Color::White, "Sicilian: e4 c5"));
    let d4 = uci(&c, "d2d4");
    let out = c.player_move(d4, t0);
    assert_eq!(out, [PlayOutput::Ask(PlayQuestion::LeaveLine { mv: d4 })]);
    assert_eq!(c.game().mainline().count(), 0);
    let out = c.answer(false, t0);
    assert_eq!(out, [PlayOutput::Notice(PlayRemark::MoveTakenBack)]);
    assert_eq!(c.phase(), PlayPhase::Player);
    // The line is still alive: e4 gets the engine's c5 for free.
    let out = c.player_move(uci(&c, "e2e4"), t0);
    assert!(thinks(&out).is_empty());
    assert_eq!(mainline_ucis(c.game()), ["e2e4", "c7c5"]);
}

#[test]
fn leaving_the_line_with_yes_frees_it_and_the_engine_thinks() {
    let (mut c, _, t0) = begin(opening(Color::White, "Sicilian: e4 c5"));
    c.player_move(uci(&c, "d2d4"), t0);
    let out = c.answer(true, t0);
    assert_eq!(mainline_ucis(c.game()), ["d2d4"]);
    assert_eq!(thinks(&out).len(), 1);
    assert_eq!(thinks(&out)[0].moves.len(), 1);
    // No more questions later.
    c.opponent_moved(Some(uci(&c, "d7d5")), None, t0, None);
    let out = c.player_move(uci(&c, "c2c4"), t0);
    assert!(!out.iter().any(|o| matches!(o, PlayOutput::Ask(_))));
}

#[test]
fn a_specific_opening_starts_from_the_standard_position_even_if_another_was_given() {
    let mut s = fen_setup(Color::White, DEPTH, "3k4/8/8/8/8/8/4R3/4K3 w - - 0 1");
    s.config.start = StartFrom::Opening(OpeningLine::parse("A: e4 e5").expect("line"));
    let (c, _, _) = begin(s);
    assert_eq!(c.game().start_position(), &Position::new());
}

#[test]
fn the_opening_name_is_written_to_the_tag() {
    let (c, _, _) = begin(opening(Color::White, "Queen's Gambit: d4 d5 c4"));
    assert_eq!(c.game().tags().get_extra("Opening"), Some("Queen's Gambit"));
}

#[test]
fn a_game_without_an_opening_has_no_opening_tag() {
    let (c, _, _) = begin(setup(Color::White, DEPTH));
    assert_eq!(c.game().tags().get_extra("Opening"), None);
}

#[test]
fn after_the_line_the_book_takes_over() {
    let mut s = opening(Color::Black, "A: e4 e5");
    s.book = Some(mini_book(BookSelection::Best));
    let (mut c, _, _) = begin(s);
    // The engine opened the line with e4; the player (Black) must answer
    // e5 and the book then plays Nf3 (the last book move).
    let t0 = Instant::now();
    let out = c.player_move(uci(&c, "e7e5"), t0);
    assert!(thinks(&out).is_empty(), "Nf3 comes from the book");
    assert_eq!(mainline_ucis(c.game()), ["e2e4", "e7e5", "g1f3"]);
    assert_eq!(comment_at_tip(&c).as_deref(), Some("Last book move (book)"));
}

#[rstest]
#[case("Sicilian: e4 c5 Nf3", 3)]
#[case("Sicilian: 1.e4 c5 2.Nf3", 3)]
#[case("Sicilian: 1. e4 c5 2. Nf3 d6", 4)]
#[case("  Name with spaces :  e4  e5 ", 2)]
fn opening_lines_parse_with_move_numbers_and_spaces(#[case] text: &str, #[case] n: usize) {
    let line = OpeningLine::parse(text).expect("line");
    assert_eq!(line.moves.len(), n, "{line:?}");
    assert_eq!(line.to_moves().expect("legal").len(), n);
}

#[rstest]
#[case("Sicilian: 1.e4 1...c5 2.Nf3 Nc6", &["e4", "c5", "Nf3", "Nc6"])]
#[case("Italian: 1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 1-0", &["e4", "e5", "Nf3", "Nc6", "Bc4", "Bc5"])]
#[case("Draw: e4 e5 1/2-1/2", &["e4", "e5"])]
#[case("Open: e4 e5 *", &["e4", "e5"])]
#[case("Castle: e4 e5 Nf3 Nc6 Bc4 Bc5 0-0", &["e4", "e5", "Nf3", "Nc6", "Bc4", "Bc5", "O-O"])]
#[case("Castle: e4 e5 Nf3 Nc6 Bc4 Bc5 O-O", &["e4", "e5", "Nf3", "Nc6", "Bc4", "Bc5", "O-O"])]
fn move_numbers_results_and_zero_castling_are_tolerated(
    #[case] text: &str,
    #[case] moves: &[&str],
) {
    let line = OpeningLine::parse(text).expect("line");
    assert_eq!(line.moves, moves, "{text}");
    assert!(line.to_moves().is_some(), "{text}");
}

#[test]
fn a_bare_number_is_not_a_move_number_and_makes_the_line_illegal() {
    let line = OpeningLine::parse("Odd: 12").expect("parses");
    assert_eq!(line.moves, ["12"]);
    assert_eq!(line.to_moves(), None);
}

#[test]
fn a_gigantic_line_is_handled_without_panic_or_delay() {
    let text = format!("Huge: {}", "e4 ".repeat(200_000));
    let line = OpeningLine::parse(&text).expect("parses");
    assert_eq!(line.moves.len(), 200_000);
    assert_eq!(line.to_moves(), None, "the second e4 is illegal");
}

#[rstest]
#[case("")]
#[case("no colon here e4 e5")]
#[case(": e4 e5")]
#[case("Name:")]
#[case("Name:   ")]
fn malformed_opening_lines_are_rejected(#[case] text: &str) {
    assert_eq!(OpeningLine::parse(text), None);
}

#[test]
fn an_illegal_line_parses_but_has_no_moves() {
    let line = OpeningLine::parse("Bad: e4 e4").expect("parses");
    assert_eq!(line.to_moves(), None);
    let line = OpeningLine::parse("Bad: Zz9").expect("parses");
    assert_eq!(line.to_moves(), None);
}

#[test]
fn the_list_keeps_valid_lines_and_skips_the_rest() {
    let list = OpeningLine::parse_list("A: e4 e5\njunk\n\nB: d4 d5\n: x\nC:\n");
    let names: Vec<_> = list.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["A", "B"]);
}

// ---- the book as a probe (D1234, M4-6) ----

#[test]
fn the_play_book_serves_the_annotation_probe() {
    use mychess_app::BookProbe;
    let book = mini_book(BookSelection::Best);
    let start = Position::new();
    let moves = BookProbe::moves(&book, &start);
    let set: BTreeSet<_> = moves.iter().map(|m| start.uci(*m)).collect();
    assert_eq!(set, BTreeSet::from(["e2e4".to_owned(), "d2d4".to_owned()]));
    assert_eq!(BookProbe::name(&book), "book");
    // A position the book lacks.
    let other = Position::from_fen("3k4/8/8/8/8/8/4R3/4K3 w - - 0 1").unwrap();
    assert!(BookProbe::moves(&book, &other).is_empty());
}

#[test]
fn the_probe_name_is_the_file_stem_of_the_loaded_book() {
    use mychess_app::BookProbe;
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("Gambits.bin");
    let start = Position::new();
    let mut book = Book::new();
    book.set_position_moves(&start, &[(start.parse_uci("e2e4").unwrap(), 5)])
        .unwrap();
    book.save_as(&path).unwrap();
    let loaded = load_play_book(&path, BookSelection::Weighted).expect("load");
    assert_eq!(BookProbe::name(&loaded), "Gambits");
    assert_eq!(loaded.book_name(), "Gambits");
    assert_eq!(BookProbe::moves(&loaded, &start).len(), 1);
    assert!(loaded.has_moves(&start));
}

#[test]
fn a_book_file_that_is_missing_or_garbage_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().expect("dir");
    assert!(load_play_book(&dir.path().join("none.bin"), BookSelection::Best).is_err());
    let bad = dir.path().join("bad.bin");
    std::fs::write(&bad, b"123").expect("write");
    assert!(load_play_book(&bad, BookSelection::Best).is_err());
    let empty = dir.path().join("empty.bin");
    std::fs::write(&empty, b"").expect("write");
    // An empty file is a valid empty book (no entries).
    if let Ok(book) = load_play_book(&empty, BookSelection::Best) {
        assert!(!book.has_moves(&Position::new()));
        assert_eq!(book.pick(&Position::new(), 5), None);
    }
}

#[rstest]
#[case(BookSelection::Best)]
#[case(BookSelection::Weighted)]
fn picking_in_a_position_without_book_moves_gives_none(#[case] selection: BookSelection) {
    let book = mini_book(selection);
    let other = Position::from_fen("3k4/8/8/8/8/8/4R3/4K3 w - - 0 1").unwrap();
    assert_eq!(book.pick(&other, 0), None);
    assert_eq!(book.pick(&other, u64::MAX), None);
}

#[test]
fn best_ignores_the_roll_and_weighted_does_not() {
    let start = Position::new();
    let best = mini_book(BookSelection::Best);
    let e4 = start.parse_uci("e2e4").unwrap();
    for roll in [0, 1, 69, 70, 99, u64::MAX] {
        assert_eq!(best.pick(&start, roll), Some(e4));
    }
    let weighted = best.clone().with_selection(BookSelection::Weighted);
    let picks: BTreeSet<_> = (0..100u64)
        .map(|r| format!("{:?}", weighted.pick(&start, r).unwrap()))
        .collect();
    assert_eq!(picks.len(), 2);
}

#[test]
fn a_game_result_stays_unknown_while_the_book_plays() {
    let (c, _, _) = begin(with_book(
        Color::Black,
        mini_book(BookSelection::Best),
        DEPTH,
    ));
    assert_eq!(c.result(), GameResult::Unknown);
}
