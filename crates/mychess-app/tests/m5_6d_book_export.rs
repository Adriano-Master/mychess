//! M5-6d contract tests, app side: "Export branch to PGN" (D2160-D2169).
//! Spec `docs/specs/m5-6-janela-do-livro.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

use std::path::{Path, PathBuf};

use mychess_app::{
    AppState, BasesMessage, BookExportOptions, BookIssue, BookMessage, BookNotice, BookSide,
    Effect, GameMessage, Message, Nav, perform, update,
};
use mychess_core::Game;

const FIRST: BookSide = BookSide::First;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn pump(rt: &tokio::runtime::Runtime, state: &mut AppState, message: Message) -> Vec<Effect> {
    let mut done = Vec::new();
    let mut queue = vec![message];
    while let Some(message) = queue.pop() {
        for effect in update(state, message) {
            if effect.runs_on_host() || matches!(effect, Effect::ScheduleAutoplayTick { .. }) {
                continue;
            }
            done.push(effect.clone());
            queue.push(rt.block_on(perform(effect)));
        }
    }
    done
}

fn game_of(ucis: &[&str]) -> Game {
    let mut game = Game::new();
    let mut node = game.root();
    for uci in ucis {
        let mv = game
            .position_at(node)
            .expect("position")
            .parse_uci(uci)
            .expect("uci");
        node = game.add_move(node, mv).expect("legal");
    }
    game
}

/// A book with 1.e4 (weight 10) e5 Nf3 (weight 1 each) and 1.d4 (weight 1),
/// the cursor at the start of the game 1.e4 e5 2.Nf3.
fn rig(rt: &tokio::runtime::Runtime) -> AppState {
    let mut state = AppState::default();
    state
        .session
        .replace_game(game_of(&["e2e4", "e7e5", "g1f3"]));
    drop(state.session.update(GameMessage::Navigate(Nav::Start)));
    pump(rt, &mut state, BookMessage::New { side: FIRST }.into());
    pump(rt, &mut state, BookMessage::AddLine { side: FIRST }.into());
    let d4 = state
        .session
        .game()
        .position_at(state.session.cursor())
        .expect("position")
        .parse_uci("d2d4")
        .expect("uci");
    pump(
        rt,
        &mut state,
        BookMessage::AddMove {
            side: FIRST,
            mv: d4,
        }
        .into(),
    );
    let e4 = state
        .session
        .game()
        .position_at(state.session.cursor())
        .expect("position")
        .parse_uci("e2e4")
        .expect("uci");
    pump(
        rt,
        &mut state,
        BookMessage::SetWeight {
            side: FIRST,
            mv: e4,
            text: "10".to_owned(),
        }
        .into(),
    );
    state
}

fn export(rt: &tokio::runtime::Runtime, state: &mut AppState, path: &Path) -> Vec<Effect> {
    pump(
        rt,
        state,
        BookMessage::ExportBranch {
            side: FIRST,
            path: path.to_path_buf(),
        }
        .into(),
    )
}

fn options(state: &mut AppState, plies: u32, lines: u32, weight: u16) {
    drop(update(
        state,
        BookMessage::SetExportOptions(BookExportOptions {
            max_plies: plies,
            max_lines: lines,
            min_weight: weight,
        })
        .into(),
    ));
}

fn notice(state: &AppState) -> Option<BookNotice> {
    state.book_view().notice
}

fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn games_in(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .expect("read")
        .matches("[Event ")
        .count()
}

#[test]
fn the_default_options_are_12_plies_200_lines_and_no_weight_floor() {
    let o = BookExportOptions::default();
    assert_eq!((o.max_plies, o.max_lines, o.min_weight), (12, 200, 0));
}

#[test]
fn the_options_are_clamped_to_their_ranges() {
    let rt = runtime();
    let mut state = rig(&rt);
    options(&mut state, 0, 0, 0);
    let o = state.book_view().export;
    assert_eq!((o.max_plies, o.max_lines), (1, 1));
    options(&mut state, 9_999, 99_999, u16::MAX);
    let o = state.book_view().export;
    assert_eq!(o.max_plies, 200);
    assert_eq!(o.max_lines, 5_000, "the maximum is 5000 lines");
    assert!(o.min_weight <= 65_520);
}

#[test]
fn every_line_of_the_branch_becomes_a_game_heaviest_move_first() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = rig(&rt);
    let path = dir.path().join("branch.pgn");
    let effects = export(&rt, &mut state, &path);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::BookExport { .. }))
    );
    assert_eq!(
        notice(&state),
        Some(BookNotice::Exported {
            lines: 2,
            truncated: false
        })
    );
    let text = std::fs::read_to_string(&path).expect("read");
    assert_eq!(games_in(&path), 2);
    let e4 = text.find("1. e4 e5 2. Nf3").expect("main line");
    let d4 = text.find("1. d4").expect("second line");
    assert!(e4 < d4, "the heavier line comes first");
    assert!(!text.contains("[FEN"), "the standard start needs no FEN");
    assert!(!text.contains('\r'), "lines end with LF");
    assert_eq!(files(dir.path()), ["branch.pgn"], "no temporary is left");
    assert!(state.book_view().export_progress.is_none());
}

#[test]
fn the_depth_limit_cuts_the_lines() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = rig(&rt);
    options(&mut state, 1, 200, 0);
    let path = dir.path().join("b.pgn");
    export(&rt, &mut state, &path);
    let text = std::fs::read_to_string(&path).expect("read");
    assert_eq!(games_in(&path), 2);
    assert!(text.contains("1. e4") && !text.contains("e5"));
}

#[test]
fn the_line_limit_stops_the_export_and_says_so() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = rig(&rt);
    options(&mut state, 12, 1, 0);
    let path = dir.path().join("b.pgn");
    export(&rt, &mut state, &path);
    assert_eq!(
        notice(&state),
        Some(BookNotice::Exported {
            lines: 1,
            truncated: true
        })
    );
    assert_eq!(games_in(&path), 1);
    assert!(std::fs::read_to_string(&path).unwrap().contains("1. e4 e5"));
}

#[test]
fn exactly_the_limit_is_not_a_truncation() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = rig(&rt);
    options(&mut state, 12, 2, 0);
    export(&rt, &mut state, &dir.path().join("b.pgn"));
    assert_eq!(
        notice(&state),
        Some(BookNotice::Exported {
            lines: 2,
            truncated: false
        })
    );
}

#[test]
fn the_weight_floor_drops_the_light_moves() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = rig(&rt);
    options(&mut state, 12, 200, 5);
    let path = dir.path().join("b.pgn");
    export(&rt, &mut state, &path);
    let text = std::fs::read_to_string(&path).expect("read");
    assert_eq!(games_in(&path), 1);
    assert!(text.contains("1. e4") && !text.contains("d4") && !text.contains("e5"));
}

#[test]
fn a_branch_from_the_middle_of_the_game_carries_its_start_position() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = rig(&rt);
    drop(state.session.update(GameMessage::Navigate(Nav::Forward(1))));
    let path = dir.path().join("b.pgn");
    export(&rt, &mut state, &path);
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(
        text.contains("[SetUp \"1\"]") && text.contains("[FEN "),
        "{text}"
    );
    assert!(text.contains("e5"));
}

#[test]
fn a_position_without_book_moves_is_refused_without_a_job() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = rig(&rt);
    drop(state.session.update(GameMessage::Navigate(Nav::End)));
    let effects = export(&rt, &mut state, &dir.path().join("b.pgn"));
    assert!(effects.is_empty());
    assert_eq!(
        notice(&state),
        Some(BookNotice::Issue(BookIssue::EmptyBranch))
    );
    assert!(files(dir.path()).is_empty());
}

#[test]
fn a_side_without_a_book_is_refused() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = rig(&rt);
    let effects = pump(
        &rt,
        &mut state,
        BookMessage::ExportBranch {
            side: BookSide::Second,
            path: dir.path().join("b.pgn"),
        }
        .into(),
    );
    assert!(effects.is_empty());
    assert_eq!(notice(&state), Some(BookNotice::Issue(BookIssue::NoBook)));
}

#[test]
fn the_name_gets_the_pgn_extension() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = rig(&rt);
    export(&rt, &mut state, &dir.path().join("lines"));
    assert_eq!(files(dir.path()), ["lines.pgn"]);
}

#[test]
fn a_chosen_existing_file_is_replaced_but_a_renamed_one_never_is() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = rig(&rt);
    // Confirmed in the dialog: replaced.
    let chosen = dir.path().join("a.pgn");
    std::fs::write(&chosen, "old").expect("write");
    export(&rt, &mut state, &chosen);
    assert_eq!(games_in(&chosen), 2);
    // "b" becomes "b.pgn", which the dialog did not confirm: kept.
    let other = dir.path().join("b.pgn");
    std::fs::write(&other, "precious").expect("write");
    export(&rt, &mut state, &dir.path().join("b"));
    assert_eq!(
        notice(&state),
        Some(BookNotice::Issue(BookIssue::FileExists))
    );
    assert_eq!(std::fs::read_to_string(&other).unwrap(), "precious");
    assert_eq!(files(dir.path()), ["a.pgn", "b.pgn"]);
}

#[test]
fn the_file_of_an_open_database_is_never_overwritten() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = rig(&rt);
    state.open_pgn_in_slots = true;
    let base = dir.path().join("games.pgn");
    std::fs::write(&base, "[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n").expect("write");
    pump(&rt, &mut state, BasesMessage::Open(base.clone()).into());
    let before = std::fs::read(&base).expect("read");
    export(&rt, &mut state, &base);
    assert_eq!(
        notice(&state),
        Some(BookNotice::Issue(BookIssue::ProtectedFile))
    );
    assert_eq!(std::fs::read(&base).expect("read"), before);
}

#[test]
fn the_file_of_a_book_is_never_overwritten_by_the_pgn() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = rig(&rt);
    let bin = dir.path().join("book.pgn");
    pump(
        &rt,
        &mut state,
        BookMessage::SaveAs {
            side: FIRST,
            path: bin.clone(),
        }
        .into(),
    );
    let before = std::fs::read(&bin).expect("read");
    export(&rt, &mut state, &bin);
    assert_eq!(
        notice(&state),
        Some(BookNotice::Issue(BookIssue::ProtectedFile))
    );
    assert_eq!(std::fs::read(&bin).expect("read"), before);
}

#[test]
fn stopping_keeps_the_job_until_it_answers_and_writes_nothing() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = rig(&rt);
    let path: PathBuf = dir.path().join("b.pgn");
    let effects = update(
        &mut state,
        BookMessage::ExportBranch {
            side: FIRST,
            path: path.clone(),
        }
        .into(),
    );
    drop(update(&mut state, BookMessage::CancelExport.into()));
    drop(update(&mut state, BookMessage::CancelExport.into()));
    let view = state.book_view();
    assert!(view.export_stopping && view.export_progress.is_some());
    assert_eq!(notice(&state), Some(BookNotice::ExportStopping));
    for effect in effects {
        let answer = rt.block_on(perform(effect));
        drop(update(&mut state, answer));
    }
    assert!(state.book_view().export_progress.is_none());
    assert_eq!(notice(&state), Some(BookNotice::ExportCancelled));
    assert!(files(dir.path()).is_empty(), "no file, no temporary");
}

#[test]
fn only_one_export_runs_at_a_time() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = rig(&rt);
    let first = update(
        &mut state,
        BookMessage::ExportBranch {
            side: FIRST,
            path: dir.path().join("a.pgn"),
        }
        .into(),
    );
    assert_eq!(first.len(), 1);
    let second = update(
        &mut state,
        BookMessage::ExportBranch {
            side: FIRST,
            path: dir.path().join("b.pgn"),
        }
        .into(),
    );
    assert!(second.is_empty());
    assert_eq!(notice(&state), Some(BookNotice::Issue(BookIssue::Busy)));
}

#[test]
fn a_missing_folder_fails_without_a_file() {
    let rt = runtime();
    let dir = tempfile::tempdir().expect("dir");
    let mut state = rig(&rt);
    export(&rt, &mut state, &dir.path().join("none").join("b.pgn"));
    assert!(matches!(notice(&state), Some(BookNotice::Issue(_))));
    assert!(files(dir.path()).is_empty());
}

#[test]
fn the_notice_keys_exist() {
    assert_eq!(
        BookNotice::Exported {
            lines: 1,
            truncated: true
        }
        .key(),
        "book-notice-exported-truncated"
    );
    assert_eq!(BookIssue::EmptyBranch.key(), "book-issue-empty-branch");
}
