//! Contract tests of copying the filter (or the selection) to the Clipbase
//! (spec M3-4, rule 8, D577, D580, D581; Scid `sc_filter_copy` 5245-5370).
#![allow(
    clippy::assert_is_empty,
    clippy::format_push_string,
    clippy::unwrap_used,
    clippy::expect_used
)]
mod m3_4d_common;

use m3_4d_common::*;

const CLIP: SlotId = SlotId::CLIPBASE;

fn clip_counts(rig: &Rig) -> (u32, u32) {
    let clip = rig.state.bases.slot(CLIP).expect("clipbase");
    (clip.total(), clip.filtered())
}

fn scrambled() -> Vec<G> {
    ["C", "A", "E", "B", "D"]
        .iter()
        .map(|n| G::new(n, "Opp"))
        .collect()
}

#[test]
fn games_enter_in_source_number_order_even_when_the_view_is_sorted_descending() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let from = rig.open_games(dir.path(), "s.pgn", &scrambled());
    for _ in 0..2 {
        rig.send(l(ListMessage::SortBy {
            slot: from,
            column: ListColumn::White,
        }));
    }
    assert_eq!(
        whites(&rig, from),
        ["E", "D", "C", "B", "A"],
        "view is descending"
    );
    copy(&mut rig, from, CLIP, CopyScope::Filter);
    assert_eq!(whites(&rig, CLIP), ["C", "A", "E", "B", "D"]);
    // The source keeps its own sort.
    assert_eq!(whites(&rig, from), ["E", "D", "C", "B", "A"]);
}

#[test]
fn only_the_filter_is_copied_and_the_clipbase_filter_grows_with_append() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let from = rig.open_games(dir.path(), "s.pgn", &numbered(6));
    rig.pick(from, 2, PickMode::Only);
    rig.pick(from, 5, PickMode::Toggle);
    rig.send(l(ListMessage::RemoveSelected { slot: from }));
    copy(&mut rig, from, CLIP, CopyScope::Filter);
    assert_eq!(whites(&rig, CLIP), ["P1", "P3", "P4", "P6"]);
    assert_eq!(clip_counts(&rig), (4, 4));
    assert_eq!(
        rig.notice(),
        Some(&BasesNotice::FilterCopied {
            count: 4,
            from: end(from, "s.pgn"),
            to: end(CLIP, ""),
        })
    );
    // A second copy goes to the end; a game left out of the Clipbase filter
    // stays out.
    rig.pick(CLIP, 1, PickMode::Only);
    rig.send(l(ListMessage::RemoveSelected { slot: CLIP }));
    assert_eq!(clip_counts(&rig), (4, 3));
    copy(&mut rig, from, CLIP, CopyScope::Filter);
    assert_eq!(clip_counts(&rig), (8, 7));
    assert_eq!(rig.filter_numbers(CLIP), [2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(
        db_whites(&rig, CLIP),
        ["P1", "P3", "P4", "P6", "P1", "P3", "P4", "P6"]
    );
}

#[test]
fn the_selection_copies_only_the_selected_games_in_database_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let from = rig.open_games(dir.path(), "s.pgn", &numbered(8));
    rig.pick(from, 7, PickMode::Only);
    rig.pick(from, 2, PickMode::Toggle);
    rig.pick(from, 4, PickMode::Toggle);
    copy(&mut rig, from, CLIP, CopyScope::Selection);
    assert_eq!(whites(&rig, CLIP), ["P2", "P4", "P7"]);
}

#[test]
fn the_current_game_and_the_selection_of_neither_database_change() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let from = rig.open_games(dir.path(), "s.pgn", &numbered(4));
    let other = rig.open_games(dir.path(), "o.pgn", &numbered(3));
    rig.send(BasesMessage::LoadGame {
        slot: other,
        number: 2,
    });
    rig.send(BasesMessage::LoadGame {
        slot: from,
        number: 3,
    });
    rig.send(BasesMessage::Select(other));
    let current_game = rig.state.session.game().tags().white.clone();
    let cur = (
        rig.state.bases.current(),
        rig.state.bases.slot(from).unwrap().current_game,
        rig.state.bases.slot(other).unwrap().current_game,
    );
    rig.pick(from, 1, PickMode::Only);
    let (sel_from, filter_other) = (rig.selection(from), rig.filter_numbers(other));
    copy(&mut rig, from, CLIP, CopyScope::Filter);
    copy(&mut rig, from, CLIP, CopyScope::Selection);
    assert_eq!(rig.state.session.game().tags().white, current_game);
    assert_eq!(
        (
            rig.state.bases.current(),
            rig.state.bases.slot(from).unwrap().current_game,
            rig.state.bases.slot(other).unwrap().current_game,
        ),
        cur
    );
    assert_eq!(rig.selection(from), sel_from);
    assert_eq!(rig.filter_numbers(other), filter_other);
    assert_eq!(rig.filter_numbers(from), [1, 2, 3, 4]);
}

#[test]
fn copying_to_the_clipbase_never_asks_even_with_the_question_on() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let from = rig.open_games(dir.path(), "s.pgn", &numbered(3));
    confirm(&mut rig, true);
    let effects = rig.raw(copy_msg(from, CLIP, CopyScope::Filter));
    assert!(rig.state.copy_prompt.is_none());
    assert!(!rig.state.modal_open());
    assert_eq!(effects.len(), 1);
    assert!(matches!(effects[0], Effect::CopyGames { .. }));
}

#[test]
fn the_limit_is_checked_before_reading_and_nothing_is_copied_when_it_does_not_fit() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let from = rig.open_games(dir.path(), "s.pgn", &numbered(5));
    rig.state.bases.set_clipbase_limit(4);
    copy(&mut rig, from, CLIP, CopyScope::Filter);
    assert_eq!(clip_counts(&rig), (0, 0));
    assert!(matches!(
        rig.notice(),
        Some(BasesNotice::CopyStopped {
            count: 0,
            error: BaseError::ClipbaseLimit { limit: 4 },
            ..
        })
    ));
    // The selection of three games fits.
    rig.select_all(from, &[1, 2, 3]);
    copy(&mut rig, from, CLIP, CopyScope::Selection);
    assert_eq!(clip_counts(&rig), (3, 3));
    // With three inside, two more do not fit (room 1): all or nothing.
    rig.select_all(from, &[4, 5]);
    copy(&mut rig, from, CLIP, CopyScope::Selection);
    assert_eq!(clip_counts(&rig), (3, 3));
    assert!(matches!(
        rig.notice(),
        Some(BasesNotice::CopyStopped {
            count: 0,
            error: BaseError::ClipbaseLimit { .. },
            ..
        })
    ));
    // Exactly one more does.
    rig.select_all(from, &[4]);
    copy(&mut rig, from, CLIP, CopyScope::Selection);
    assert_eq!(clip_counts(&rig), (4, 4));
    assert_eq!(whites(&rig, CLIP), ["P1", "P2", "P3", "P4"]);
}

#[test]
fn the_content_of_the_games_is_identical() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let games = [rich("Rich1"), G::new("Plain", "Opp"), rich("Rich2")];
    let from = rig.open_games(dir.path(), "s.pgn", &games);
    copy(&mut rig, from, CLIP, CopyScope::Filter);
    let (a, b) = (texts(&rig, from), texts(&rig, CLIP));
    assert_eq!(a, b);
    assert!(a[0].contains("Best by test") && a[0].contains('(') && a[0].contains("$14"));
    assert!(a[0].contains("C20") && a[0].contains("2500"));
}

#[test]
fn cancelling_before_the_first_game_copies_nothing_and_keeps_the_clipbase_consistent() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let from = rig.open_games(dir.path(), "s.pgn", &numbered(5));
    let effects = rig.raw(copy_msg(from, CLIP, CopyScope::Filter));
    assert_eq!(
        rig.state.list_job().map(|j| j.0),
        Some(ListJobKind::CopyGames)
    );
    rig.raw(l(ListMessage::CancelJob));
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    assert_eq!(clip_counts(&rig), (0, 0));
    assert!(rig.state.list_job().is_none());
    assert!(matches!(
        rig.notice(),
        Some(BasesNotice::CopyStopped {
            count: 0,
            error: BaseError::Cancelled,
            ..
        })
    ));
}

#[test]
fn cancelling_in_the_middle_keeps_exactly_what_was_copied() {
    // 5 100 small games, a cancel raised from another thread as soon as the
    // job reports progress. The count is not deterministic; the invariants
    // are: the Clipbase holds exactly the reported games, in order, and its
    // filter covers them.
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let from = rig.open_games(dir.path(), "s.pgn", &numbered(5_100));
    let effects = rig.raw(copy_msg(from, CLIP, CopyScope::Filter));
    let control = rig.state.list_job().map(|j| j.1.clone()).unwrap();
    let watcher = {
        let control = control.clone();
        std::thread::spawn(move || {
            while control.progress().0 < 50 && !control.is_cancelled() {
                std::thread::yield_now();
                if control.progress().1 > 0 && control.progress().0 >= 5_100 {
                    break;
                }
            }
            control.cancel();
        })
    };
    let answer = rig.perform(effects.into_iter().next().unwrap());
    watcher.join().unwrap();
    rig.raw(answer);
    let (total, filtered) = clip_counts(&rig);
    assert_eq!(total, filtered);
    let expected: Vec<String> = (1..=total).map(|n| format!("P{n}")).collect();
    assert_eq!(db_whites(&rig, CLIP), expected);
    match rig.notice() {
        Some(BasesNotice::CopyStopped {
            count,
            error: BaseError::Cancelled,
            ..
        }) => {
            assert_eq!(*count, total);
            assert!(total < 5_100);
        }
        Some(BasesNotice::FilterCopied { count, .. }) => assert_eq!(*count, 5_100),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_deleted_flag_and_the_user_flags_of_an_si4_source_arrive_with_the_game() {
    use mychess_db::si4::{FlagOp, GameFlags, Si4Writer};
    let dir = tempfile::tempdir().unwrap();
    let path = make_si4(dir.path(), "f", &pgn(&numbered(4)));
    {
        let mut w = Si4Writer::open(&path).unwrap();
        w.set_flag(2, GameFlags::DELETE, FlagOp::Set).unwrap();
        w.set_flag(3, GameFlags::CUSTOM1, FlagOp::Set).unwrap();
        w.set_flag(3, GameFlags::BRILLIANCY, FlagOp::Set).unwrap();
    }
    let mut rig = Rig::new();
    let from = rig.open(&path);
    let src_flags = flags_of(&rig, from);
    assert_ne!(src_flags[1], 0);
    assert_ne!(src_flags[2], 0);
    copy(&mut rig, from, CLIP, CopyScope::Filter);
    assert_eq!(flags_of(&rig, CLIP), src_flags);
    let rows = rig.state.bases.list_page(CLIP, 1, 10);
    assert_eq!(
        rows.iter().map(|r| r.deleted).collect::<Vec<_>>(),
        [false, true, false, false]
    );
}

#[test]
fn a_pgn_source_games_copy_with_no_flags() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let from = rig.open_games(dir.path(), "s.pgn", &numbered(3));
    copy(&mut rig, from, CLIP, CopyScope::Filter);
    assert_eq!(flags_of(&rig, CLIP), [0, 0, 0]);
}

#[test]
fn the_clipbase_loses_its_sort_when_it_grows() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let from = rig.open_games(dir.path(), "s.pgn", &scrambled());
    copy(&mut rig, from, CLIP, CopyScope::Filter);
    rig.send(l(ListMessage::SortBy {
        slot: CLIP,
        column: ListColumn::White,
    }));
    assert_eq!(whites(&rig, CLIP), ["A", "B", "C", "D", "E"]);
    copy(&mut rig, from, CLIP, CopyScope::Filter);
    assert_eq!(rig.view(CLIP).len(), 10);
    assert_eq!(
        rig.view(CLIP),
        (1..=10).collect::<Vec<u32>>(),
        "database order again"
    );
}
