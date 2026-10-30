//! Contract tests of the copy job: one job at a time, closing an end cancels
//! it, late and repeated answers do nothing (spec M3-4, D561, D570, D578,
//! D586).
#![allow(
    clippy::assert_is_empty,
    clippy::format_push_string,
    clippy::unwrap_used,
    clippy::expect_used
)]
mod m3_4d_common;

use m3_4d_common::*;

/// A PGN of 6 games in slot 1, an `.si4` of 3 in slot 2 and a PGN of 4 in
/// slot 3; the job copies 1 -> 2 and is not answered yet.
struct Busy {
    p: Pair,
    third: SlotId,
    effect: Effect,
}

fn busy() -> Busy {
    let mut p = pair(&numbered(6), &numbered(3));
    let third = p.rig.open_games(p.dir.path(), "third.pgn", &numbered(4));
    let effects = p.rig.raw(copy_msg(p.from, p.to, CopyScope::Filter));
    assert_eq!(effects.len(), 1);
    assert!(matches!(effects[0], Effect::CopyGames { .. }));
    Busy {
        effect: effects.into_iter().next().unwrap(),
        p,
        third,
    }
}

fn is_busy(rig: &Rig) -> bool {
    rig.notice() == Some(&BasesNotice::ListBusy)
}

#[test]
fn a_running_copy_exposes_its_kind_and_a_control() {
    let b = busy();
    let (kind, control) = b.p.rig.state.list_job().expect("a job");
    assert_eq!(kind, ListJobKind::CopyGames);
    assert!(!control.is_cancelled());
}

#[test]
fn every_action_that_changes_filter_order_or_database_waits_for_the_job() {
    let mut b = busy();
    let (from, to) = (b.p.from, b.p.to);
    b.p.rig.set_search(from, "P1");
    b.p.rig.pick(from, 1, PickMode::Only);
    let attempts: Vec<ListMessage> = vec![
        ListMessage::CopyGames {
            from,
            to,
            scope: CopyScope::Filter,
        },
        ListMessage::CopyGames {
            from: b.third,
            to: SlotId::CLIPBASE,
            scope: CopyScope::Filter,
        },
        ListMessage::ResetFilter { slot: from },
        ListMessage::NegateFilter { slot: to },
        ListMessage::NegateFilter { slot: b.third },
        ListMessage::SortBy {
            slot: from,
            column: ListColumn::White,
        },
        ListMessage::FilterByText { slot: from },
        ListMessage::KeepSelected { slot: from },
        ListMessage::RemoveSelected { slot: from },
    ];
    for message in attempts {
        b.p.rig.clear_notice();
        let effects = b.p.rig.raw(l(message.clone()));
        assert!(effects.is_empty(), "{message:?}");
        assert!(is_busy(&b.p.rig), "{message:?}: {:?}", b.p.rig.notice());
    }
    assert_eq!(b.p.rig.filter_numbers(from), (1..=6).collect::<Vec<_>>());
    assert_eq!(b.p.rig.filter_numbers(b.third), (1..=4).collect::<Vec<_>>());
}

#[test]
fn a_busy_list_beats_every_refusal() {
    let mut b = busy();
    b.p.rig.clear_notice();
    b.p.rig.raw(copy_msg(b.p.from, b.p.from, CopyScope::Filter));
    assert!(is_busy(&b.p.rig), "{:?}", b.p.rig.notice());
}

#[test]
fn a_job_on_the_clipbase_blocks_copy_paste_and_clear_of_the_clipbase() {
    let mut p = pair(&numbered(3), &numbered(1));
    let effects = p
        .rig
        .raw(copy_msg(p.from, SlotId::CLIPBASE, CopyScope::Filter));
    assert_eq!(effects.len(), 1);
    for message in [
        BasesMessage::ClipbaseClear,
        BasesMessage::ClipbasePaste,
        BasesMessage::ClipbaseCopy,
    ] {
        p.rig.clear_notice();
        let effects = p.rig.raw(message);
        assert!(effects.is_empty());
        assert!(is_busy(&p.rig), "{:?}", p.rig.notice());
    }
}

#[test]
fn loading_a_game_of_either_end_waits_but_the_third_database_is_free() {
    let mut b = busy();
    for slot in [b.p.from, b.p.to] {
        b.p.rig.clear_notice();
        let effects = b.p.rig.raw(BasesMessage::LoadGame { slot, number: 1 });
        assert!(effects.is_empty(), "{slot:?}");
        assert!(is_busy(&b.p.rig));
    }
    b.p.rig.clear_notice();
    let effects = b.p.rig.raw(BasesMessage::LoadGame {
        slot: b.third,
        number: 1,
    });
    assert!(!effects.is_empty(), "an unrelated database loads");
    assert!(!is_busy(&b.p.rig));
}

#[test]
fn the_progress_ends_at_n_of_n_for_the_filter_and_for_the_selection() {
    let mut b = busy();
    let control = b.p.rig.state.list_job().map(|j| j.1.clone()).unwrap();
    assert_eq!(control.progress().0, 0);
    let answer = b.p.rig.perform(b.effect);
    assert_eq!(control.progress(), (6, 6));
    b.p.rig.raw(answer);
    assert!(b.p.rig.state.list_job().is_none());
    // The selection of 2.
    b.p.rig.select_all(b.p.from, &[2, 5]);
    let effects =
        b.p.rig
            .raw(copy_msg(b.p.from, b.p.to, CopyScope::Selection));
    let control = b.p.rig.state.list_job().map(|j| j.1.clone()).unwrap();
    let answer = b.p.rig.perform(effects.into_iter().next().unwrap());
    assert_eq!(control.progress(), (2, 2));
    b.p.rig.raw(answer);
    assert_eq!(disk(&b.p.path).len(), 3 + 6 + 2);
}

#[test]
fn closing_the_source_cancels_the_job_and_the_late_answer_changes_nothing() {
    let mut b = busy();
    let control = b.p.rig.state.list_job().map(|j| j.1.clone()).unwrap();
    b.p.rig.send(BasesMessage::Close(b.p.from));
    assert!(control.is_cancelled());
    assert!(b.p.rig.state.list_job().is_none());
    let before = b.p.rig.notice().cloned();
    let answer = b.p.rig.perform(b.effect);
    b.p.rig.raw(answer);
    assert_eq!(
        b.p.rig.notice().cloned(),
        before,
        "no message for a dead job"
    );
    assert_eq!(disk(&b.p.path).len(), 3, "a cancelled job wrote nothing");
    assert_eq!(b.p.rig.state.bases.slot(b.p.to).unwrap().total(), 3);
}

#[test]
fn closing_the_target_cancels_the_job_and_the_late_answer_changes_nothing() {
    let mut b = busy();
    let control = b.p.rig.state.list_job().map(|j| j.1.clone()).unwrap();
    b.p.rig.send(BasesMessage::Close(b.p.to));
    assert!(control.is_cancelled());
    assert!(b.p.rig.state.list_job().is_none());
    let before = b.p.rig.notice().cloned();
    let answer = b.p.rig.perform(b.effect);
    b.p.rig.raw(answer);
    assert_eq!(b.p.rig.notice().cloned(), before);
    assert_eq!(disk(&b.p.path).len(), 3);
    assert_eq!(b.p.rig.state.bases.slot(b.p.from).unwrap().total(), 6);
    // The source is free for new actions.
    b.p.rig.clear_notice();
    b.p.rig
        .send(l(ListMessage::NegateFilter { slot: b.p.from }));
    assert!(!is_busy(&b.p.rig));
}

#[test]
fn an_answer_for_a_slot_that_was_closed_and_reused_is_not_applied_to_the_new_database() {
    let mut b = busy();
    // The job finishes on disk, then the target is closed and another
    // database takes its place before the answer is handled.
    let answer = b.p.rig.perform(b.effect);
    b.p.rig.send(BasesMessage::Close(b.p.to));
    let other = make_si4(b.p.dir.path(), "other", &pgn(&numbered(5)));
    let reused = b.p.rig.open(&other);
    assert_eq!(reused, b.p.to, "the free slot is reused");
    b.p.rig.clear_notice();
    b.p.rig.raw(answer);
    let slot = b.p.rig.state.bases.slot(reused).unwrap();
    assert_eq!((slot.total(), slot.filtered()), (5, 5));
    assert!(
        b.p.rig.notice().is_none()
            || !matches!(b.p.rig.notice(), Some(BasesNotice::FilterCopied { .. }))
    );
}

#[test]
fn a_repeated_answer_is_not_applied_twice() {
    let mut b = busy();
    let answer = b.p.rig.perform(b.effect);
    b.p.rig.raw(answer.clone());
    let slot = b.p.rig.state.bases.slot(b.p.to).unwrap();
    assert_eq!((slot.total(), slot.filtered()), (9, 9));
    let notice = b.p.rig.notice().cloned();
    b.p.rig.clear_notice();
    b.p.rig.raw(answer);
    let slot = b.p.rig.state.bases.slot(b.p.to).unwrap();
    assert_eq!(
        (slot.total(), slot.filtered()),
        (9, 9),
        "filter did not grow again"
    );
    assert!(b.p.rig.notice().is_none(), "no second message");
    assert!(matches!(
        notice,
        Some(BasesNotice::FilterCopied { count: 6, .. })
    ));
}

#[test]
fn nobody_else_changes_filter_selection_or_current_game() {
    let mut b = busy();
    let third = b.third;
    // Cancel the job first to prepare the third database freely.
    b.p.rig.raw(l(ListMessage::CancelJob));
    let answer = b.p.rig.perform(b.effect);
    b.p.rig.raw(answer);
    b.p.rig.select_all(third, &[2, 3]);
    b.p.rig.send(l(ListMessage::RemoveSelected { slot: third }));
    b.p.rig.send(BasesMessage::LoadGame {
        slot: third,
        number: 4,
    });
    b.p.rig.send(l(ListMessage::SortBy {
        slot: third,
        column: ListColumn::White,
    }));
    let snapshot = (
        b.p.rig.filter_numbers(third),
        b.p.rig.view(third),
        b.p.rig.selection(third),
        b.p.rig.state.bases.slot(third).unwrap().current_game,
        b.p.rig.state.bases.current(),
    );
    copy(&mut b.p.rig, b.p.from, b.p.to, CopyScope::Filter);
    copy(&mut b.p.rig, b.p.from, SlotId::CLIPBASE, CopyScope::Filter);
    let after = (
        b.p.rig.filter_numbers(third),
        b.p.rig.view(third),
        b.p.rig.selection(third),
        b.p.rig.state.bases.slot(third).unwrap().current_game,
        b.p.rig.state.bases.current(),
    );
    assert_eq!(snapshot, after);
}

#[test]
fn cancel_job_stops_a_copy_before_its_first_game_and_nothing_reaches_the_file() {
    let mut b = busy();
    b.p.rig.raw(l(ListMessage::CancelJob));
    let answer = b.p.rig.perform(b.effect);
    b.p.rig.raw(answer);
    assert!(matches!(
        b.p.rig.notice(),
        Some(BasesNotice::CopyStopped {
            count: 0,
            error: BaseError::Cancelled,
            ..
        })
    ));
    assert_eq!(disk(&b.p.path).len(), 3);
    assert!(b.p.rig.state.list_job().is_none());
    // The databases are usable again.
    copy(&mut b.p.rig, b.p.from, b.p.to, CopyScope::Filter);
    assert_eq!(disk(&b.p.path).len(), 9);
}
