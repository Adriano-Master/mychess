//! Contract tests of the grid of the game browser (spec M3-4, slice 4e:
//! D596, D597, D599, D601; Scid `browseGames`, `gamelist.tcl` 1463+).
#![allow(
    clippy::assert_is_empty,
    clippy::format_push_string,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::expect_used
)]
mod m3_4e_common;

use m3_4e_common::*;
use mychess_db::GameSource;
use mychess_db::si4::{GameFlags, Si4Database};
use rstest::rstest;

/// The table of the Scid's "quick hack to proportion the grid".
fn scid_columns(n: usize) -> usize {
    if n > 36 {
        7
    } else if n > 20 {
        6
    } else if n > 12 {
        5
    } else if n > 6 || n == 4 {
        4
    } else {
        3
    }
}

#[test]
fn the_grid_columns_follow_the_scid_for_every_count() {
    for n in 0..=64 {
        assert_eq!(grid_columns(n), scid_columns(n), "{n} games");
    }
}

#[rstest]
#[case(0, 3)]
#[case(1, 3)]
#[case(2, 3)]
#[case(3, 3)]
#[case(4, 4)]
#[case(5, 3)]
#[case(6, 3)]
#[case(7, 4)]
#[case(12, 4)]
#[case(13, 5)]
#[case(20, 5)]
#[case(21, 6)]
#[case(36, 6)]
#[case(37, 7)]
#[case(64, 7)]
fn the_edges_of_the_column_table(#[case] n: usize, #[case] columns: usize) {
    assert_eq!(grid_columns(n), columns);
}

#[test]
fn the_limit_of_the_grid_is_64() {
    assert_eq!(MAX_BROWSER_GAMES, 64);
}

// ---- opening ----

#[test]
fn several_numbers_open_a_grid_at_the_end_of_each_game_in_the_order_asked() {
    let (mut rig, slot, _d) = setup(8);
    open_browser(&mut rig, slot, &[5, 2, 7]);
    assert_eq!(br(&rig).mode, BrowseMode::Grid);
    assert_eq!(numbers_shown(&rig), [5, 2, 7]);
    // No filter ply: Scid's browseGames shows the end.
    assert_eq!(plies(&rig), [6, 6, 6]);
    assert_eq!(br(&rig).columns(), 3);
    for (i, white) in ["W5", "W2", "W7"].iter().enumerate() {
        let h = &br(&rig).games[i].ready().unwrap().header;
        assert_eq!((h.white.as_str(), h.result.as_str()), (*white, "1-0"));
    }
    assert!(
        br(&rig).games[0].ready().unwrap().doc.is_none(),
        "no move text in the grid"
    );
}

#[test]
fn the_grid_is_read_by_one_effect_with_all_the_numbers() {
    let (mut rig, slot, _d) = setup(8);
    let effects = rig.raw(open_msg(slot, &[3, 1, 2]));
    let reads: Vec<_> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::ReadBrowserGames { numbers, .. } => Some(numbers.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(reads, [vec![3, 1, 2]]);
}

#[test]
fn repeated_zero_and_oversized_numbers_are_cleaned_up() {
    let (mut rig, slot, _d) = setup(5);
    open_browser(&mut rig, slot, &[2, 0, 2, 3, 99, 5]);
    // 99 is the last game (5), which is already there.
    assert_eq!(numbers_shown(&rig), [2, 3, 5]);
    assert_eq!(br(&rig).mode, BrowseMode::Grid);
    send(&mut rig, BrowseMessage::Close);
    // Only one game is left after the cleanup: the single browser.
    open_browser(&mut rig, slot, &[4, 4, 0]);
    assert_eq!(br(&rig).mode, BrowseMode::Single);
    assert_eq!(numbers_shown(&rig), [4]);
}

#[rstest]
#[case(64, 64, false)]
#[case(65, 64, true)]
#[case(70, 64, true)]
#[case(100, 64, true)]
fn a_selection_over_the_limit_shows_the_first_and_says_so(
    #[case] asked: u32,
    #[case] shown: usize,
    #[case] notice: bool,
) {
    let (mut rig, slot, _d) = setup(100);
    rig.clear_notice();
    let numbers: Vec<u32> = (1..=asked).collect();
    open_browser(&mut rig, slot, &numbers);
    assert_eq!(br(&rig).games.len(), shown);
    assert_eq!(numbers_shown(&rig), (1..=shown as u32).collect::<Vec<_>>());
    assert_eq!(br(&rig).columns(), 7);
    let expected = BasesNotice::BrowserLimit {
        shown: shown as u32,
        asked,
    };
    if notice {
        assert!(has_notice(&rig, &expected), "{:?}", rig.state.notice);
    } else {
        assert_eq!(rig.notice(), None);
    }
}

// ---- moving the boards ----

#[test]
fn step_start_and_end_move_all_boards_or_the_one_named() {
    let (mut rig, slot, _d) = setup(4);
    open_browser(&mut rig, slot, &[1, 2, 3, 4]);
    send(&mut rig, BrowseMessage::GoToStart { game: None });
    assert_eq!(plies(&rig), [0, 0, 0, 0]);
    send(
        &mut rig,
        BrowseMessage::Step {
            game: Some(2),
            delta: 3,
        },
    );
    assert_eq!(plies(&rig), [0, 0, 3, 0]);
    send(
        &mut rig,
        BrowseMessage::Step {
            game: None,
            delta: 2,
        },
    );
    assert_eq!(plies(&rig), [2, 2, 5, 2]);
    send(
        &mut rig,
        BrowseMessage::Step {
            game: None,
            delta: 2,
        },
    );
    assert_eq!(plies(&rig), [4, 4, 6, 4], "the one at the end stays there");
    send(&mut rig, BrowseMessage::GoToEnd { game: Some(0) });
    assert_eq!(plies(&rig), [6, 4, 6, 4]);
    send(
        &mut rig,
        BrowseMessage::Step {
            game: None,
            delta: -100,
        },
    );
    assert_eq!(plies(&rig), [0, 0, 0, 0]);
    // A board that is not there is ignored.
    send(
        &mut rig,
        BrowseMessage::Step {
            game: Some(9),
            delta: 1,
        },
    );
    send(&mut rig, BrowseMessage::GoToEnd { game: Some(99) });
    assert_eq!(plies(&rig), [0, 0, 0, 0]);
}

#[test]
fn flip_all_or_one_and_the_boards_keep_their_own_orientation() {
    let (mut rig, slot, _d) = setup(3);
    open_browser(&mut rig, slot, &[1, 2, 3]);
    send(&mut rig, BrowseMessage::Flip { game: Some(1) });
    let flipped = |rig: &Rig| br(rig).games.iter().map(|g| g.flipped).collect::<Vec<_>>();
    assert_eq!(flipped(&rig), [false, true, false]);
    send(&mut rig, BrowseMessage::Flip { game: None });
    assert_eq!(flipped(&rig), [true, false, true]);
    send(&mut rig, BrowseMessage::Flip { game: Some(7) });
    assert_eq!(flipped(&rig), [true, false, true]);
}

#[test]
fn the_pointer_decides_the_focus_and_a_wrong_board_is_ignored() {
    let (mut rig, slot, _d) = setup(4);
    open_browser(&mut rig, slot, &[1, 2, 3, 4]);
    assert_eq!(br(&rig).focus, 0);
    send(&mut rig, BrowseMessage::Focus(2));
    assert_eq!(br(&rig).focus, 2);
    send(&mut rig, BrowseMessage::Focus(4));
    send(&mut rig, BrowseMessage::Focus(usize::MAX));
    assert_eq!(br(&rig).focus, 2);
}

#[test]
fn autoplay_and_the_game_keys_are_for_the_single_browser_only() {
    let (mut rig, slot, _d) = setup(4);
    open_browser(&mut rig, slot, &[1, 2]);
    send(&mut rig, BrowseMessage::GoToStart { game: None });
    let effects = rig.raw(browse(BrowseMessage::ToggleAutoplay));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(!br(&rig).autoplay);
    assert_eq!(plies(&rig), [0, 0]);
    let effects = rig.raw(browse(BrowseMessage::GoToGame(GameStep::Next)));
    assert!(effects.is_empty());
    assert_eq!(numbers_shown(&rig), [1, 2]);
}

#[test]
fn loading_from_the_grid_takes_the_game_asked_at_its_own_ply_and_closes() {
    let (mut rig, slot, _d) = setup(5);
    open_browser(&mut rig, slot, &[2, 4, 5]);
    send(
        &mut rig,
        BrowseMessage::Step {
            game: Some(1),
            delta: -4,
        },
    );
    assert_eq!(plies(&rig), [6, 2, 6]);
    send(
        &mut rig,
        BrowseMessage::LoadIntoBoard {
            game: 1,
            keep_open: false,
        },
    );
    assert!(rig.state.browser.is_none());
    assert_eq!(editor_white(&rig), "W4");
    assert_eq!(editor_ply(&rig), 2);
}

#[test]
fn loading_from_the_grid_with_keep_open_leaves_the_grid_as_it_was() {
    let (mut rig, slot, _d) = setup(5);
    open_browser(&mut rig, slot, &[2, 4, 5]);
    send(
        &mut rig,
        BrowseMessage::LoadIntoBoard {
            game: 2,
            keep_open: true,
        },
    );
    assert_eq!(editor_white(&rig), "W5");
    assert_eq!(editor_ply(&rig), 6);
    assert_eq!(numbers_shown(&rig), [2, 4, 5]);
    assert_eq!(plies(&rig), [6, 6, 6]);
}

// ---- the deleted mark ----

fn si4_setup(count: usize) -> (Rig, SlotId, std::path::PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = make_si4(dir.path(), "db", &pgn(&sixes(count)));
    let mut rig = Rig::new();
    let slot = rig.open(&path);
    (rig, slot, path, dir)
}

fn deleted_on_disk(path: &std::path::Path) -> Vec<u32> {
    let db = Si4Database::open(path).unwrap();
    (1..=GameSource::len(&db))
        .filter(|n| db.entry(*n).unwrap().flags.contains(GameFlags::DELETE))
        .collect()
}

#[test]
fn ctrl_delete_marks_the_game_under_the_pointer_in_an_editable_si4() {
    let (mut rig, slot, path, _d) = si4_setup(4);
    // The selection of the list is not the browser's business: it must
    // come back untouched.
    rig.select_all(slot, &[1]);
    open_browser(&mut rig, slot, &[1, 2, 3]);
    send(&mut rig, BrowseMessage::ToggleDeleted { game: 1 });
    assert_eq!(deleted_on_disk(&path), [2]);
    assert_eq!(rig.selection(slot), [1], "selection restored");
    assert_eq!(rig.anchor(slot), Some(1));
    assert!(rig.state.browser.is_some(), "the browser stays");
    let marks: Vec<bool> = br(&rig).games.iter().map(|g| g.deleted).collect();
    assert_eq!(marks, [false, true, false], "the board shows the mark");
    // Toggling again clears it.
    send(&mut rig, BrowseMessage::ToggleDeleted { game: 1 });
    assert_eq!(deleted_on_disk(&path), Vec::<u32>::new());
    let marks: Vec<bool> = br(&rig).games.iter().map(|g| g.deleted).collect();
    assert_eq!(marks, [false, false, false]);
}

#[test]
fn a_game_already_marked_is_shown_marked_when_the_browser_opens() {
    let (mut rig, slot, path, _d) = si4_setup(3);
    rig.select_all(slot, &[3]);
    rig.send(l(ListMessage::ToggleDeleted { slot }));
    assert_eq!(deleted_on_disk(&path), [3]);
    open_browser(&mut rig, slot, &[2, 3]);
    let marks: Vec<bool> = br(&rig).games.iter().map(|g| g.deleted).collect();
    assert_eq!(marks, [false, true]);
}

#[test]
fn the_mark_of_a_board_that_is_not_there_does_nothing() {
    let (mut rig, slot, path, _d) = si4_setup(3);
    open_browser(&mut rig, slot, &[1, 2]);
    let effects = rig.raw(browse(BrowseMessage::ToggleDeleted { game: 9 }));
    assert!(effects.is_empty());
    assert_eq!(deleted_on_disk(&path), Vec::<u32>::new());
}

#[test]
fn the_mark_is_refused_on_a_pgn_a_read_only_si4_and_the_clipbase() {
    // PGN
    let (mut rig, slot, _d) = setup(3);
    open_browser(&mut rig, slot, &[1, 2]);
    rig.clear_notice();
    let effects = rig.raw(browse(BrowseMessage::ToggleDeleted { game: 0 }));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(rig.notice(), Some(&BasesNotice::ReadOnly));
    assert!(br(&rig).games.iter().all(|g| !g.deleted));

    // Read-only .si4
    let (mut rig, slot, path, _d) = si4_setup(3);
    rig.send(l(ListMessage::SetReadOnly {
        slot,
        read_only: true,
    }));
    open_browser(&mut rig, slot, &[1, 2]);
    rig.clear_notice();
    let effects = rig.raw(browse(BrowseMessage::ToggleDeleted { game: 0 }));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(rig.notice(), Some(&BasesNotice::ReadOnly));
    assert_eq!(deleted_on_disk(&path), Vec::<u32>::new());

    // Clipbase
    let (mut rig, slot, _d) = setup(3);
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    rig.send(BasesMessage::ClipbaseCopy);
    rig.send(BasesMessage::LoadGame { slot, number: 2 });
    rig.send(BasesMessage::ClipbaseCopy);
    open_browser(&mut rig, SlotId::CLIPBASE, &[1, 2]);
    assert_eq!(br(&rig).slot, SlotId::CLIPBASE);
    rig.clear_notice();
    let effects = rig.raw(browse(BrowseMessage::ToggleDeleted { game: 0 }));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(rig.notice(), Some(&BasesNotice::ReadOnly));
}

#[test]
fn the_mark_is_refused_while_another_job_of_the_list_runs() {
    let (mut rig, slot, path, _d) = si4_setup(3);
    let job = rig.raw(l(ListMessage::SortBy {
        slot,
        column: mychess_app::ListColumn::White,
    }));
    assert_eq!(job.len(), 1);
    // The browser cannot even open over a job of its slot.
    rig.clear_notice();
    let effects = rig.raw(open_msg(slot, &[1, 2]));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(rig.notice(), Some(&BasesNotice::ListBusy));
    assert!(rig.state.browser.is_none());
    assert_eq!(deleted_on_disk(&path), Vec::<u32>::new());
}
