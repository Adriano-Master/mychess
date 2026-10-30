//! Contract tests of copying games to an editable `.si4` (spec M3-4, rule 8,
//! D577-D579, D581, D582; Scid `sc_filter_copy` 5245, `sc_savegame` 9041).
//! Every check on disk goes through an independent `Si4Database::open`.
#![allow(
    clippy::assert_is_empty,
    clippy::format_push_string,
    clippy::unwrap_used,
    clippy::expect_used
)]
mod m3_4d_common;

use m3_4d_common::*;
use mychess_db::si4::{FlagOp, GameFlags, Si4Writer};

fn source_games() -> Vec<G> {
    vec![
        rich("S1"),
        G::new("S2", "Opp").result("0-1").moves("1. d4 d5 2. c4 *"),
        rich("S3"),
        G::new("S4", "Opp").result("1/2-1/2").moves("1. Nf3 Nf6 *"),
        G::new("S5", "Opp"),
    ]
}

#[test]
fn the_filter_is_appended_to_the_file_in_source_order_and_the_file_is_coherent() {
    let mut p = pair(&source_games(), &numbered(3));
    // Keep S1, S3 and S5.
    p.rig.select_all(p.from, &[2, 4]);
    p.rig.send(l(ListMessage::RemoveSelected { slot: p.from }));
    copy(&mut p.rig, p.from, p.to, CopyScope::Filter);

    let on_disk = disk(&p.path);
    assert_eq!(on_disk.len(), 6);
    let names: Vec<&str> = on_disk.iter().map(|g| g.white.as_str()).collect();
    assert_eq!(names, ["P1", "P2", "P3", "S1", "S3", "S5"]);
    // An independent import of the same three games gives the same records
    // (derived data included).
    let fresh_dir = tempfile::tempdir().unwrap();
    let all = source_games();
    let fresh = make_si4(
        fresh_dir.path(),
        "fresh",
        &pgn(&[all[0].clone(), all[2].clone(), all[4].clone()]),
    );
    assert_eq!(&on_disk[3..], &disk(&fresh)[..]);
    // The original games are untouched.
    let before = make_si4(fresh_dir.path(), "before", &pgn(&numbered(3)));
    assert_eq!(&on_disk[..3], &disk(&before)[..]);
    // Rich content survived (comment, variation, NAG).
    assert!(on_disk[3].text.contains("Best by test"));
    assert!(on_disk[3].text.contains("$14") || on_disk[3].text.contains('!'));
    assert_eq!(
        p.rig.notice(),
        Some(&BasesNotice::FilterCopied {
            count: 3,
            from: end(p.from, "src.pgn"),
            to: end(p.to, "dst.si4"),
        })
    );
}

#[test]
fn the_slot_shows_what_is_on_disk_and_keeps_filter_selection_and_position() {
    let mut p = pair(&source_games(), &numbered(6));
    let (from, to) = (p.from, p.to);
    // A game out of the target filter, a selection, a position and a loaded
    // game in the target.
    p.rig.pick(to, 2, PickMode::Only);
    p.rig.send(l(ListMessage::RemoveSelected { slot: to }));
    p.rig.pick(to, 5, PickMode::Only);
    p.rig.pick(to, 6, PickMode::Toggle);
    p.rig
        .send(BasesMessage::SetListFirst { slot: to, first: 3 });
    p.rig.send(BasesMessage::LoadGame {
        slot: to,
        number: 4,
    });
    p.rig.send(BasesMessage::Select(from));
    let (sel, anchor, first) = (p.rig.selection(to), p.rig.anchor(to), p.rig.first(to));
    let current = p.rig.state.bases.current();
    copy(&mut p.rig, from, to, CopyScope::Filter);
    let slot = p.rig.state.bases.slot(to).unwrap();
    assert_eq!(
        (slot.total(), slot.filtered()),
        (11, 10),
        "game 2 stays out"
    );
    assert_eq!(p.rig.selection(to), sel);
    assert_eq!(p.rig.anchor(to), anchor);
    assert_eq!(p.rig.first(to), first);
    assert_eq!(slot.current_game, Some(4));
    assert_eq!(p.rig.state.bases.current(), current);
    let mut want: Vec<u32> = vec![1, 3, 4, 5, 6];
    want.extend(7..=11);
    assert_eq!(p.rig.filter_numbers(to), want);
    // The slot's data is the file's.
    assert_eq!(db_whites(&p.rig, to).len(), disk(&p.path).len());
    assert_eq!(db_whites(&p.rig, to)[6..], ["S1", "S2", "S3", "S4", "S5"]);
}

#[test]
fn the_sort_of_the_target_is_lost_because_the_new_games_are_not_in_it() {
    let mut p = pair(&source_games(), &numbered(3));
    for _ in 0..2 {
        p.rig.send(l(ListMessage::SortBy {
            slot: p.to,
            column: ListColumn::White,
        }));
    }
    assert_eq!(whites(&p.rig, p.to), ["P3", "P2", "P1"]);
    copy(&mut p.rig, p.from, p.to, CopyScope::Filter);
    assert_eq!(p.rig.view(p.to), (1..=8).collect::<Vec<u32>>());
}

#[test]
fn the_selection_of_the_source_goes_to_the_file_too() {
    let mut p = pair(&source_games(), &numbered(1));
    p.rig.select_all(p.from, &[5, 2]);
    copy(&mut p.rig, p.from, p.to, CopyScope::Selection);
    let names: Vec<String> = disk(&p.path).into_iter().map(|g| g.white).collect();
    assert_eq!(names, ["P1", "S2", "S5"]);
}

#[test]
fn the_sort_of_the_source_does_not_change_the_order_in_the_file() {
    let games: Vec<G> = ["C", "A", "B"].iter().map(|n| G::new(n, "Opp")).collect();
    let mut p = pair(&games, &numbered(1));
    for _ in 0..2 {
        p.rig.send(l(ListMessage::SortBy {
            slot: p.from,
            column: ListColumn::White,
        }));
    }
    copy(&mut p.rig, p.from, p.to, CopyScope::Filter);
    let names: Vec<String> = disk(&p.path).into_iter().map(|g| g.white).collect();
    assert_eq!(names, ["P1", "C", "A", "B"]);
}

#[test]
fn an_si4_source_with_deleted_and_user_flags_copies_them_to_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let src = make_si4(dir.path(), "src", &pgn(&numbered(3)));
    {
        let mut w = Si4Writer::open(&src).unwrap();
        w.set_flag(1, GameFlags::DELETE, FlagOp::Set).unwrap();
        w.set_flag(3, GameFlags::CUSTOM2, FlagOp::Set).unwrap();
        w.set_flag(3, GameFlags::TACTICS, FlagOp::Set).unwrap();
    }
    let target = make_si4(dir.path(), "dst", &pgn(&numbered(1)));
    let mut rig = Rig::new();
    let from = rig.open(&src);
    let to = rig.open(&target);
    confirm(&mut rig, false);
    let source_flags = disk(&src).iter().map(|g| g.flags).collect::<Vec<_>>();
    copy(&mut rig, from, to, CopyScope::Filter);
    let copied: Vec<u32> = disk(&target).iter().skip(1).map(|g| g.flags).collect();
    assert_eq!(copied, source_flags, "the whole entry flags travel (D581)");
    assert_ne!(copied[0] & GameFlags::DELETE.bits(), 0);
    // The deleted game is dimmed in the new rows.
    let rows = rig.state.bases.list_page(to, 1, 10);
    assert_eq!(
        rows.iter().map(|r| r.deleted).collect::<Vec<_>>(),
        [false, true, false, false]
    );
}

#[test]
fn a_file_changed_behind_the_slot_is_not_written_and_the_slot_is_reloaded() {
    let mut p = pair(&source_games(), &numbered(3));
    // Another program flags a game.
    {
        let mut w = Si4Writer::open(&p.path).unwrap();
        w.set_flag(1, GameFlags::DELETE, FlagOp::Set).unwrap();
    }
    let before = disk(&p.path);
    copy(&mut p.rig, p.from, p.to, CopyScope::Filter);
    assert!(matches!(
        p.rig.notice(),
        Some(BasesNotice::CopyStopped {
            count: 0,
            error: BaseError::Stale,
            ..
        })
    ));
    assert_eq!(disk(&p.path), before, "nothing was written");
    assert!(
        p.rig.state.bases.list_page(p.to, 1, 5)[0].deleted,
        "reloaded"
    );
    // A second attempt works with the fresh stamp.
    copy(&mut p.rig, p.from, p.to, CopyScope::Filter);
    assert_eq!(disk(&p.path).len(), 8);
}

#[test]
fn a_target_that_grew_behind_the_slot_is_stale() {
    // The file grew behind the slot's back.
    let mut p = pair(&source_games(), &numbered(3));
    {
        let mut w = Si4Writer::open(&p.path).unwrap();
        let extra = make_si4(p.dir.path(), "extra", &pgn(&numbered(1)));
        let mut db = mychess_db::si4::Si4Database::open(&extra).unwrap();
        let game = mychess_db::GameSource::game(&mut db, 1).unwrap();
        w.add_game(&game, GameFlags::empty()).unwrap();
    }
    copy(&mut p.rig, p.from, p.to, CopyScope::Filter);
    assert!(matches!(
        p.rig.notice(),
        Some(BasesNotice::CopyStopped {
            error: BaseError::Stale,
            count: 0,
            ..
        })
    ));
    assert_eq!(disk(&p.path).len(), 4);
}

#[test]
fn a_live_writer_on_the_target_stops_the_copy_and_leaves_the_slot_alone() {
    let mut p = pair(&source_games(), &numbered(3));
    let (filter, sel) = (p.rig.filter_numbers(p.to), p.rig.selection(p.to));
    let writer = Si4Writer::open(&p.path).unwrap();
    copy(&mut p.rig, p.from, p.to, CopyScope::Filter);
    assert!(matches!(
        p.rig.notice(),
        Some(BasesNotice::CopyStopped {
            count: 0,
            error: BaseError::AlreadyOpen,
            ..
        })
    ));
    let slot = p.rig.state.bases.slot(p.to).unwrap();
    assert_eq!((slot.total(), slot.filtered()), (3, 3));
    assert_eq!(p.rig.filter_numbers(p.to), filter);
    assert_eq!(p.rig.selection(p.to), sel);
    assert!(p.rig.state.list_job().is_none());
    drop(writer);
    copy(&mut p.rig, p.from, p.to, CopyScope::Filter);
    assert_eq!(disk(&p.path).len(), 8);
}

#[test]
fn more_than_one_batch_is_written_in_order_and_the_file_stays_coherent() {
    // 5 100 games: one batch of 5 000 and one of 100.
    let big = numbered(5_100);
    let mut p = pair(&big, &numbered(2));
    copy(&mut p.rig, p.from, p.to, CopyScope::Filter);
    let on_disk = disk(&p.path);
    assert_eq!(on_disk.len(), 5_102);
    assert_eq!(on_disk[2].white, "P1");
    assert_eq!(on_disk[5_001].white, "P5000");
    assert_eq!(on_disk[5_002].white, "P5001");
    assert_eq!(on_disk[5_101].white, "P5100");
    let slot = p.rig.state.bases.slot(p.to).unwrap();
    assert_eq!((slot.total(), slot.filtered()), (5_102, 5_102));
    assert_eq!(
        p.rig.notice(),
        Some(&BasesNotice::FilterCopied {
            count: 5_100,
            from: end(p.from, "src.pgn"),
            to: end(p.to, "dst.si4"),
        })
    );
}

#[test]
fn cancelling_in_the_middle_of_a_batch_writes_that_batch_and_reopens_consistently() {
    let mut p = pair(&numbered(5_100), &numbered(2));
    let (from, to) = (p.from, p.to);
    let effects = p.rig.raw(copy_msg(from, to, CopyScope::Filter));
    let control = p.rig.state.list_job().map(|j| j.1.clone()).unwrap();
    let watcher = {
        let control = control.clone();
        std::thread::spawn(move || {
            while control.progress().0 < 50 && !control.is_cancelled() {
                std::thread::yield_now();
                if control.progress().0 >= 5_100 {
                    break;
                }
            }
            control.cancel();
        })
    };
    let answer = p.rig.perform(effects.into_iter().next().unwrap());
    watcher.join().unwrap();
    p.rig.raw(answer);
    let on_disk = disk(&p.path);
    let copied = on_disk.len() - 2;
    let slot = p.rig.state.bases.slot(to).unwrap();
    assert_eq!(slot.total() as usize, on_disk.len(), "reopened consistent");
    assert_eq!(slot.filtered() as usize, on_disk.len());
    for (i, g) in on_disk[2..].iter().enumerate() {
        assert_eq!(g.white, format!("P{}", i + 1), "prefix, in order");
    }
    match p.rig.notice() {
        Some(BasesNotice::CopyStopped {
            count,
            error: BaseError::Cancelled,
            ..
        }) => {
            assert_eq!(*count as usize, copied);
            assert!(copied > 0 && copied < 5_100, "{copied}");
        }
        Some(BasesNotice::FilterCopied { count, .. }) => assert_eq!(*count, 5_100),
        other => panic!("{other:?}"),
    }
    assert!(p.rig.state.list_job().is_none());
}

#[test]
fn an_unreadable_source_game_ends_the_loop_keeping_the_games_before_it() {
    let dir = tempfile::tempdir().unwrap();
    let src = make_si4(dir.path(), "src", &pgn(&numbered(5)));
    let target = make_si4(dir.path(), "dst", &pgn(&numbered(1)));
    let mut rig = Rig::new();
    let from = rig.open(&src);
    let to = rig.open(&target);
    confirm(&mut rig, false);
    // Cut the game file inside game 3 after the slot was loaded.
    let offset = {
        let db = mychess_db::si4::Si4Database::open(&src).unwrap();
        u64::from(mychess_db::GameSource::entry(&db, 3).unwrap().offset)
    };
    let sg4 = src.with_extension("sg4");
    let file = std::fs::OpenOptions::new().write(true).open(&sg4).unwrap();
    file.set_len(offset + 1).unwrap();
    drop(file);
    copy(&mut rig, from, to, CopyScope::Filter);
    assert!(
        matches!(
            rig.notice(),
            Some(BasesNotice::CopyStopped {
                count: 2,
                error: BaseError::GameUnreadable { number: 3 },
                ..
            })
        ),
        "{:?}",
        rig.notice()
    );
    let names: Vec<String> = disk(&target).into_iter().map(|g| g.white).collect();
    assert_eq!(names, ["P1", "P1", "P2"]);
    assert_eq!(rig.state.bases.slot(to).unwrap().total(), 3);
}

#[test]
fn the_source_keeps_filter_selection_and_sort_after_the_copy() {
    let mut p = pair(&source_games(), &numbered(1));
    p.rig.select_all(p.from, &[2, 3]);
    p.rig.send(l(ListMessage::SortBy {
        slot: p.from,
        column: ListColumn::White,
    }));
    let (view, sel, filter) = (
        p.rig.view(p.from),
        p.rig.selection(p.from),
        p.rig.filter_numbers(p.from),
    );
    copy(&mut p.rig, p.from, p.to, CopyScope::Filter);
    assert_eq!(p.rig.view(p.from), view);
    assert_eq!(p.rig.selection(p.from), sel);
    assert_eq!(p.rig.filter_numbers(p.from), filter);
}

#[test]
fn a_pgn_source_and_an_si4_source_give_the_same_records_in_the_target() {
    let dir = tempfile::tempdir().unwrap();
    let games = source_games();
    let src_si4 = make_si4(dir.path(), "ssrc", &pgn(&games));
    let t1 = make_si4(dir.path(), "t1", &pgn(&numbered(1)));
    let t2 = make_si4(dir.path(), "t2", &pgn(&numbered(1)));
    let mut rig = Rig::new();
    let pgn_slot = rig.open_games(dir.path(), "p.pgn", &games);
    let si4_slot = rig.open(&src_si4);
    let a = rig.open(&t1);
    let b = rig.open(&t2);
    confirm(&mut rig, false);
    copy(&mut rig, pgn_slot, a, CopyScope::Filter);
    copy(&mut rig, si4_slot, b, CopyScope::Filter);
    assert_eq!(disk(&t1), disk(&t2));
    assert_eq!(disk(&t1).len(), 6);
}
