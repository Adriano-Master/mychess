//! Tests of the book window flow and of the opening book in the annotation
//! and the report (spec M5-6).

use std::path::PathBuf;

use mychess_book::Book;
use mychess_core::{Move, Position};

use super::AppState;
use super::annotate_tests::{drive, game_of, material_engine, position_of, send, setup};
use crate::annotate::AnnotateMessage;
use crate::book::{
    BookIssue, BookMergeMode, BookMessage, BookNotice, BookPrefs, BookSide, BookTarget, MergedBook,
    SharedBook, WeightScale,
};
use crate::effect::Effect;
use crate::play::{BookSelection, PlayBook};
use crate::prefs::{PrefChange, PrefsMessage};
use crate::session::{GameMessage, Nav};

fn book_msg(state: &mut AppState, message: BookMessage) -> Vec<Effect> {
    send(state, message)
}

fn uci(position: &Position, text: &str) -> Move {
    position.parse_uci(text).expect("uci")
}

/// A state whose game is 1.e4 e5 2.Nf3, with the cursor at the start and
/// an empty book on the left.
fn with_new_book() -> AppState {
    let mut state = AppState::default();
    state
        .session
        .replace_game(game_of(&["e2e4", "e7e5", "g1f3"]));
    drop(state.session.update(GameMessage::Navigate(Nav::Start)));
    book_msg(
        &mut state,
        BookMessage::New {
            side: BookSide::First,
        },
    );
    state
}

fn first_rows(state: &AppState) -> Vec<(String, u16, u8)> {
    state.book_view().sides[0]
        .as_ref()
        .map(|s| {
            s.rows
                .iter()
                .map(|r| (r.san.clone(), r.weight, r.percent))
                .collect()
        })
        .unwrap_or_default()
}

fn notice(state: &AppState) -> Option<BookNotice> {
    state.book().notice
}

#[test]
fn adding_the_game_move_then_tuning_the_weight() {
    let mut state = with_new_book();
    let start = Position::new();
    let e4 = uci(&start, "e2e4");
    book_msg(
        &mut state,
        BookMessage::AddGameMove {
            side: BookSide::First,
        },
    );
    assert_eq!(first_rows(&state), vec![("e4".to_owned(), 1, 100)]);
    assert_eq!(notice(&state), Some(BookNotice::Added(1)));
    // The same move twice is refused.
    book_msg(
        &mut state,
        BookMessage::AddGameMove {
            side: BookSide::First,
        },
    );
    assert_eq!(
        notice(&state),
        Some(BookNotice::Issue(BookIssue::AlreadyThere))
    );
    // A second move, then the weights decide the shares.
    let d4 = uci(&start, "d2d4");
    book_msg(
        &mut state,
        BookMessage::AddMove {
            side: BookSide::First,
            mv: d4,
        },
    );
    book_msg(
        &mut state,
        BookMessage::SetWeight {
            side: BookSide::First,
            mv: e4,
            text: " 300 ".into(),
        },
    );
    assert_eq!(
        first_rows(&state),
        vec![("e4".to_owned(), 300, 100), ("d4".to_owned(), 1, 0)]
    );
    book_msg(
        &mut state,
        BookMessage::ScaleWeight {
            side: BookSide::First,
            mv: e4,
            scale: WeightScale::Half,
        },
    );
    book_msg(
        &mut state,
        BookMessage::ScaleWeight {
            side: BookSide::First,
            mv: d4,
            scale: WeightScale::Half,
        },
    );
    let rows = first_rows(&state);
    assert_eq!((rows[0].1, rows[1].1), (150, 1), "halving stops at 1");
    book_msg(
        &mut state,
        BookMessage::ScaleWeight {
            side: BookSide::First,
            mv: d4,
            scale: WeightScale::Double,
        },
    );
    assert_eq!(first_rows(&state)[1].1, 2);
    book_msg(
        &mut state,
        BookMessage::RemoveMove {
            side: BookSide::First,
            mv: e4,
        },
    );
    assert_eq!(first_rows(&state), vec![("d4".to_owned(), 2, 100)]);
    let view = state.book_view();
    assert!(view.sides[0].as_ref().expect("side").dirty);
}

#[test]
fn bad_weights_are_refused_and_change_nothing() {
    let mut state = with_new_book();
    let e4 = uci(&Position::new(), "e2e4");
    book_msg(
        &mut state,
        BookMessage::AddGameMove {
            side: BookSide::First,
        },
    );
    for (text, issue) in [
        ("0", BookIssue::ZeroWeight),
        ("65521", BookIssue::WeightTooLarge),
        ("99999999999999999999", BookIssue::NotANumber),
        ("-3", BookIssue::NotANumber),
        ("abc", BookIssue::NotANumber),
        ("", BookIssue::NotANumber),
    ] {
        book_msg(
            &mut state,
            BookMessage::SetWeight {
                side: BookSide::First,
                mv: e4,
                text: text.into(),
            },
        );
        assert_eq!(notice(&state), Some(BookNotice::Issue(issue)), "{text:?}");
        assert_eq!(first_rows(&state), vec![("e4".to_owned(), 1, 100)]);
    }
    book_msg(
        &mut state,
        BookMessage::SetWeight {
            side: BookSide::First,
            mv: e4,
            text: "65520".into(),
        },
    );
    assert_eq!(first_rows(&state)[0].1, 65520);
    book_msg(
        &mut state,
        BookMessage::ScaleWeight {
            side: BookSide::First,
            mv: e4,
            scale: WeightScale::Double,
        },
    );
    assert_eq!(
        first_rows(&state)[0].1,
        65520,
        "doubling stops at the maximum"
    );
}

#[test]
fn lines_are_added_removed_and_branches_imported() {
    let mut state = with_new_book();
    book_msg(
        &mut state,
        BookMessage::AddLine {
            side: BookSide::First,
        },
    );
    assert_eq!(notice(&state), Some(BookNotice::Added(3)));
    assert_eq!(first_rows(&state), vec![("e4".to_owned(), 1, 100)]);
    book_msg(
        &mut state,
        BookMessage::RemoveLine {
            side: BookSide::First,
        },
    );
    assert_eq!(notice(&state), Some(BookNotice::Removed(3)));
    assert!(first_rows(&state).is_empty());
    // A variation of the game: 1.e4 e5 2.Nf3 and 1.e4 c5.
    let mut game = game_of(&["e2e4", "e7e5", "g1f3"]);
    let e4 = game.mainline().next().expect("e4");
    let c5 = game
        .position_at(e4)
        .expect("position")
        .parse_uci("c7c5")
        .expect("uci");
    game.add_move(e4, c5).expect("variation");
    state.session.replace_game(game);
    drop(state.session.update(GameMessage::Navigate(Nav::Start)));
    book_msg(
        &mut state,
        BookMessage::ImportBranch {
            side: BookSide::First,
        },
    );
    assert_eq!(
        notice(&state),
        Some(BookNotice::Added(4)),
        "e4, e5, Nf3, c5"
    );
    // At the end of the game there is nothing to add.
    drop(state.session.update(GameMessage::Navigate(Nav::End)));
    book_msg(
        &mut state,
        BookMessage::AddLine {
            side: BookSide::First,
        },
    );
    assert_eq!(
        notice(&state),
        Some(BookNotice::Issue(BookIssue::NothingToAdd))
    );
    book_msg(
        &mut state,
        BookMessage::ImportBranch {
            side: BookSide::First,
        },
    );
    assert_eq!(
        notice(&state),
        Some(BookNotice::Issue(BookIssue::NothingToAdd))
    );
}

#[test]
fn a_missing_book_and_a_set_up_position_are_reported() {
    let mut state = with_new_book();
    book_msg(
        &mut state,
        BookMessage::AddGameMove {
            side: BookSide::Second,
        },
    );
    assert_eq!(notice(&state), Some(BookNotice::Issue(BookIssue::NoBook)));
    book_msg(
        &mut state,
        BookMessage::CopyPosition {
            to: BookSide::First,
        },
    );
    assert_eq!(notice(&state), Some(BookNotice::Issue(BookIssue::NoBook)));
    let chess960 = Position::chess960_start(100).expect("start");
    state
        .session
        .replace_game(mychess_core::Game::from_position(chess960));
    book_msg(
        &mut state,
        BookMessage::AddLine {
            side: BookSide::First,
        },
    );
    assert!(notice(&state).is_some());
}

#[test]
fn moves_are_copied_from_the_other_book() {
    let mut state = with_new_book();
    book_msg(
        &mut state,
        BookMessage::AddGameMove {
            side: BookSide::First,
        },
    );
    book_msg(
        &mut state,
        BookMessage::New {
            side: BookSide::Second,
        },
    );
    book_msg(
        &mut state,
        BookMessage::CopyPosition {
            to: BookSide::Second,
        },
    );
    assert_eq!(notice(&state), Some(BookNotice::Copied));
    let view = state.book_view();
    let second = view.sides[1].as_ref().expect("second");
    assert_eq!(second.rows.len(), 1);
    assert_eq!(second.rows[0].san, "e4");
    assert!(second.rows[0].in_game);
    // Nothing to copy from an empty book.
    book_msg(
        &mut state,
        BookMessage::Close {
            side: BookSide::First,
        },
    );
    assert!(
        state.book_view().confirm_close.is_some(),
        "the book has changes"
    );
    book_msg(&mut state, BookMessage::ConfirmClose(true));
    assert!(state.book_view().sides[0].is_none());
}

#[test]
fn closing_asks_only_when_there_are_unsaved_changes() {
    let mut state = with_new_book();
    book_msg(
        &mut state,
        BookMessage::Close {
            side: BookSide::First,
        },
    );
    assert!(
        state.book_view().sides[0].is_none(),
        "a clean book closes at once"
    );
    book_msg(
        &mut state,
        BookMessage::New {
            side: BookSide::First,
        },
    );
    book_msg(
        &mut state,
        BookMessage::AddGameMove {
            side: BookSide::First,
        },
    );
    book_msg(
        &mut state,
        BookMessage::Close {
            side: BookSide::First,
        },
    );
    assert_eq!(state.book_view().confirm_close, Some(BookSide::First));
    book_msg(&mut state, BookMessage::ConfirmClose(false));
    assert!(state.book_view().sides[0].is_some());
    assert_eq!(state.book_view().confirm_close, None);
    // A new book does not replace one with changes.
    book_msg(
        &mut state,
        BookMessage::New {
            side: BookSide::First,
        },
    );
    assert_eq!(
        notice(&state),
        Some(BookNotice::Issue(BookIssue::UnsavedChanges))
    );
}

#[test]
fn saving_goes_through_an_effect_and_a_late_answer_is_dropped() {
    let mut state = with_new_book();
    book_msg(
        &mut state,
        BookMessage::AddGameMove {
            side: BookSide::First,
        },
    );
    // No file yet.
    assert!(
        book_msg(
            &mut state,
            BookMessage::Save {
                side: BookSide::First
            }
        )
        .is_empty()
    );
    assert_eq!(notice(&state), Some(BookNotice::Issue(BookIssue::NoPath)));
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("out.bin");
    let effects = book_msg(
        &mut state,
        BookMessage::SaveAs {
            side: BookSide::First,
            path: path.clone(),
        },
    );
    let [
        Effect::BookSave {
            request,
            book,
            path: to,
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(to.as_deref(), Some(path.as_path()));
    assert!(state.book().is_busy());
    // Editing the same book meanwhile is refused.
    book_msg(
        &mut state,
        BookMessage::AddGameMove {
            side: BookSide::First,
        },
    );
    assert_eq!(notice(&state), Some(BookNotice::Issue(BookIssue::Busy)));
    let request = *request;
    // The job runs in `perform`; here it is run directly.
    let saved = BookMessage::Saved {
        request,
        result: crate::book::save_book(book, Some(&path)),
    };
    book_msg(&mut state, saved.clone());
    assert_eq!(notice(&state), Some(BookNotice::Saved));
    let view = state.book_view();
    let side = view.sides[0].as_ref().expect("side");
    assert!(!side.dirty);
    assert_eq!(side.path.as_deref(), Some(path.as_path()));
    // The same answer again has no request in flight any more.
    state.book.notice = None;
    book_msg(&mut state, saved);
    assert_eq!(notice(&state), None);
}

#[test]
fn a_loaded_book_answers_only_for_its_request_and_a_failure_is_a_notice() {
    let mut state = AppState::default();
    let effects = book_msg(
        &mut state,
        BookMessage::Load {
            side: BookSide::Second,
            path: PathBuf::from("x.bin"),
        },
    );
    let [
        Effect::BookLoad {
            request, target, ..
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(*target, BookTarget::Side(BookSide::Second));
    let request = *request;
    let loaded = |request| BookMessage::SlotLoaded {
        request,
        side: BookSide::Second,
        result: Ok(SharedBook::new(Book::new())),
    };
    book_msg(&mut state, loaded(crate::RequestId(9999)));
    assert!(state.book_view().sides[1].is_none(), "unknown request");
    book_msg(
        &mut state,
        BookMessage::SlotLoaded {
            request,
            side: BookSide::Second,
            result: Err(BookIssue::NotFound),
        },
    );
    assert_eq!(notice(&state), Some(BookNotice::Issue(BookIssue::NotFound)));
    assert!(!state.book().is_busy());
}

#[test]
fn merging_needs_two_books_and_puts_the_result_on_the_left() {
    let mut state = with_new_book();
    book_msg(
        &mut state,
        BookMessage::AddGameMove {
            side: BookSide::First,
        },
    );
    assert!(book_msg(&mut state, BookMessage::Merge).is_empty());
    assert_eq!(notice(&state), Some(BookNotice::Issue(BookIssue::NoBook)));
    book_msg(
        &mut state,
        BookMessage::New {
            side: BookSide::Second,
        },
    );
    // The left book has unsaved changes.
    assert!(book_msg(&mut state, BookMessage::Merge).is_empty());
    assert_eq!(
        notice(&state),
        Some(BookNotice::Issue(BookIssue::UnsavedChanges))
    );
    book_msg(&mut state, BookMessage::ConfirmClose(true));
    let dir = tempfile::tempdir().expect("dir");
    let effects = book_msg(
        &mut state,
        BookMessage::SaveAs {
            side: BookSide::First,
            path: dir.path().join("a.bin"),
        },
    );
    let [
        Effect::BookSave {
            request,
            book,
            path,
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    let saved = BookMessage::Saved {
        request: *request,
        result: crate::book::save_book(book, path.as_deref()),
    };
    book_msg(&mut state, saved);
    book_msg(
        &mut state,
        BookMessage::SetMergeMode(BookMergeMode::SumWeights),
    );
    let effects = book_msg(&mut state, BookMessage::Merge);
    let [Effect::BookMerge { request, mode, .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(*mode, BookMergeMode::SumWeights);
    let merged = BookMessage::Merged {
        request: *request,
        result: Ok(MergedBook {
            book: SharedBook::new(Book::new()),
            written: 0,
            skipped: 2,
        }),
    };
    book_msg(&mut state, merged);
    assert_eq!(
        notice(&state),
        Some(BookNotice::Merged {
            written: 0,
            skipped: 2
        })
    );
    assert!(state.book_view().sides[0].as_ref().expect("left").dirty);
}

#[test]
fn creating_needs_games_in_the_filter() {
    let mut state = AppState::default();
    assert!(
        book_msg(
            &mut state,
            BookMessage::CreateFromFilter {
                side: BookSide::First
            }
        )
        .is_empty()
    );
    assert_eq!(notice(&state), Some(BookNotice::Issue(BookIssue::NoGames)));
    book_msg(
        &mut state,
        BookMessage::SetCreateOptions(crate::book::BookCreateOptions {
            max_ply: 0,
            min_games: 0,
            min_score: 200,
            only_white: true,
            only_black: true,
            uniform: true,
        }),
    );
    let options = state.book_view().create;
    assert_eq!(
        (options.max_ply, options.min_games, options.min_score),
        (1, 1, 100)
    );
    assert!(options.only_white && !options.only_black);
}

// ---- the opening book ----

fn opening_book(line: &[&str]) -> PlayBook {
    let mut position = Position::new();
    let mut moves = Vec::new();
    for text in line {
        let mv = uci(&position, text);
        moves.push(mv);
        position.play(mv).expect("play");
    }
    let mut book = Book::new();
    book.add_line(&moves, &Position::new(), 1).expect("line");
    PlayBook::new(book, BookSelection::Best)
}

#[test]
fn the_preference_asks_for_the_opening_book_and_the_answer_is_checked() {
    let mut state = AppState::default();
    let effects = send(
        &mut state,
        PrefsMessage::Set(PrefChange::Book(BookPrefs {
            opening_book: Some(PathBuf::from("main.bin")),
        })),
    );
    let (request, path) = effects
        .iter()
        .find_map(|e| match e {
            Effect::BookLoad {
                request,
                target: BookTarget::Opening,
                path,
            } => Some((*request, path.clone())),
            _ => None,
        })
        .expect("a load");
    assert_eq!(path, PathBuf::from("main.bin"));
    // A failure leaves the preference and says why.
    book_msg(
        &mut state,
        BookMessage::OpeningLoaded {
            request,
            result: Err(BookIssue::NotFound),
        },
    );
    assert!(state.opening_probe().is_none());
    let view = state.book_view();
    assert_eq!(
        view.opening.as_ref().and_then(|o| o.issue),
        Some(BookIssue::NotFound)
    );
    assert_eq!(
        state.prefs.book.opening_book,
        Some(PathBuf::from("main.bin"))
    );
    // A new choice and a late answer for the old one.
    let effects = send(
        &mut state,
        PrefsMessage::Set(PrefChange::Book(BookPrefs {
            opening_book: Some(PathBuf::from("other.bin")),
        })),
    );
    let request = effects
        .iter()
        .find_map(|e| match e {
            Effect::BookLoad { request, .. } => Some(*request),
            _ => None,
        })
        .expect("a load");
    book_msg(
        &mut state,
        BookMessage::OpeningLoaded {
            request,
            result: Ok(opening_book(&["e2e4"])),
        },
    );
    assert!(state.opening_probe().is_some());
    assert_eq!(
        state.book_view().opening.and_then(|o| o.loaded_name),
        Some("book".to_owned())
    );
    send(
        &mut state,
        PrefsMessage::Set(PrefChange::Book(BookPrefs::default())),
    );
    assert!(state.opening_probe().is_none());
    assert!(state.book_view().opening.is_none());
}

const GAME: [&str; 20] = [
    "e2e4", "e7e5", "d1h5", "b8c6", "h5e5", "c6e5", "g1f3", "e5f3", "g2f3", "d7d6", "f1c4", "g8f6",
    "d2d3", "f8e7", "c1e3", "e8g8", "b1c3", "c7c6", "e1c1", "b7b5",
];

fn annotate_with_book(book: Option<PlayBook>, use_book: bool) -> (AppState, Vec<Position>) {
    let (mut state, run) = setup(game_of(&GAME));
    state.book.opening = book;
    state.annotate_options.use_book = use_book;
    let effects = send(&mut state, AnnotateMessage::Start);
    let log = drive(&mut state, run, effects, 100, &mut material_engine);
    assert!(!state.annotation_running(), "the run ended");
    let positions = log.requests.iter().map(position_of).collect();
    (state, positions)
}

#[test]
fn book_moves_are_not_searched_and_the_last_one_is_commented() {
    // The book has 1.e4 e5 2.Qh5 Nc6: the game stays in it for four plies.
    let (state, positions) = annotate_with_book(Some(opening_book(&GAME[..4])), true);
    // 21 positions without the book; the first searched is the one before
    // the last book move (3 plies in), so 18 searches.
    assert_eq!(positions.len(), 18);
    assert_eq!(positions[0].fullmove_number(), 2, "after 1.e4 e5 2.Qh5");
    let game = state.session.game();
    let fourth = game.mainline().nth(3).expect("ply 4");
    let comment = game.comment(fourth).expect("node").unwrap_or_default();
    assert!(comment.contains("Last book move (book)"), "{comment}");
    assert!(
        !comment.contains('+') && !comment.contains('#'),
        "no score: {comment}"
    );
    // Nothing is written on the skipped moves.
    for ply in 0..3 {
        let node = game.mainline().nth(ply).expect("ply");
        assert_eq!(game.comment(node).expect("node"), None);
    }
    // The blunder of ply 5 is still found.
    assert!(
        !game
            .nags(game.mainline().nth(4).expect("ply 5"))
            .expect("node")
            .is_empty()
    );
}

#[test]
fn the_move_that_leaves_the_book_lists_the_book_moves() {
    // The book plays 1.e4 e5 2.Nf3; the game plays 2.Qh5.
    let (state, positions) =
        annotate_with_book(Some(opening_book(&["e2e4", "e7e5", "g1f3"])), true);
    assert_eq!(positions.len(), 19);
    let game = state.session.game();
    let third = game.mainline().nth(2).expect("ply 3");
    let comment = game.comment(third).expect("node").unwrap_or_default();
    assert!(
        comment.contains("Move out of book (book: Nf3)"),
        "{comment}"
    );
}

#[test]
fn without_the_option_or_without_a_book_nothing_changes() {
    let (_, positions) = annotate_with_book(Some(opening_book(&GAME[..4])), false);
    assert_eq!(positions.len(), 21);
    let (_, positions) = annotate_with_book(None, true);
    assert_eq!(positions.len(), 21);
    // A book that does not know the first move changes nothing either.
    let (_, positions) = annotate_with_book(Some(opening_book(&["d2d4"])), true);
    assert_eq!(
        positions.len(),
        21,
        "the start is the position before the move out"
    );
}

#[test]
fn the_dialog_says_when_a_book_is_available() {
    let (mut state, _run) = setup(game_of(&GAME[..4]));
    send(&mut state, AnnotateMessage::Open);
    assert!(
        !state
            .annotate_dialog
            .as_ref()
            .expect("dialog")
            .book_available
    );
    send(&mut state, AnnotateMessage::Close);
    state.book.opening = Some(opening_book(&GAME[..2]));
    send(&mut state, AnnotateMessage::Open);
    assert!(
        state
            .annotate_dialog
            .as_ref()
            .expect("dialog")
            .book_available
    );
}

#[test]
fn the_report_marks_book_moves_by_the_polyglot_book() {
    use crate::annotate::PositionEval;
    use crate::report::inputs_from_game_with_book;
    let game = game_of(&GAME[..4]);
    let eval = || PositionEval {
        score: 0.0,
        mate: None,
        pv: Vec::new(),
        depth: 10,
        second: None,
    };
    let evals: Vec<PositionEval> = (0..5).map(|_| eval()).collect();
    let book = opening_book(&GAME[..2]);
    let inputs = inputs_from_game_with_book(&game, &evals, None, Some(&book));
    let flags: Vec<bool> = inputs.moves.iter().map(|m| m.book).collect();
    assert_eq!(flags, [true, true, false, false]);
}

#[test]
fn a_variation_of_a_skipped_move_is_not_searched() {
    let mut game = game_of(&GAME);
    let e4 = game.mainline().next().expect("e4");
    let c5 = uci(game.position_at(e4).expect("position"), "c7c5");
    game.add_move(e4, c5).expect("variation");
    let (mut state, run) = setup(game);
    state.book.opening = Some(opening_book(&GAME[..4]));
    state.annotate_options.use_book = true;
    state.annotate_options.variations = true;
    let effects = send(&mut state, AnnotateMessage::Start);
    let log = drive(&mut state, run, effects, 100, &mut material_engine);
    assert!(!state.annotation_running());
    assert_eq!(
        log.requests.len(),
        18,
        "the variation is skipped with the book"
    );
}
