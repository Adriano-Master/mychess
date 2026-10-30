//! Contract tests of the merge of a batch of games (spec
//! `m7-6-mesclar-partida.md`, section 4, rule 10, criterion 14).
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod m7_6b_common;

use m7_6b_common::*;
use mychess_app::{BATCH_MAX, BatchReport, MergePrefs, PrefChange, PrefsMessage};

fn five() -> Vec<u32> {
    // The game itself, no common position, nothing to add, two that merge.
    vec![A, OTHER_START, A_COPY, B, D]
}

#[test]
fn the_batch_skips_and_counts_and_is_one_undo_step() {
    let (mut rig, slot, _dir) = sample(A);
    let before = rig.state.session.game().clone();
    let depth = rig.state.session.undo_depth();
    let effects = rig.raw(MergeMessage::OpenBatch {
        slot,
        numbers: five(),
    });
    // The game itself is not even read.
    assert!(
        matches!(effects.as_slice(), [Effect::LoadGamesForMerge { numbers, .. }] if numbers.len() == 4)
    );
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    let view = rig.state.merge_view().expect("confirmation");
    assert!(view.batch);
    assert_eq!(view.end_ply, None);
    assert_eq!(
        view.batch_report,
        BatchReport {
            merged: 2,
            skipped_same: 1,
            skipped_no_match: 1,
            skipped_nothing: 1,
            skipped_unreadable: 0,
        }
    );
    assert!(rig.state.session.game().same_content(&before));
    rig.apply();
    assert!(rig.state.merge_view().is_none());
    assert_eq!(rig.state.session.undo_depth(), depth + 1);
    assert!(rig.state.session.is_dirty());
    assert_eq!(
        rig.notice(),
        Some(MergeNotice::BatchDone {
            merged: 2,
            skipped: 3
        })
    );
    rig.undo();
    assert!(rig.state.session.game().same_content(&before));
}

#[test]
fn without_confirmation_the_batch_merges_at_once() {
    let (mut rig, slot, _dir) = sample(A);
    rig.send(PrefsMessage::Set(PrefChange::Merge(MergePrefs {
        end_ply: 80,
        confirm_batch: false,
    })));
    rig.send(MergeMessage::OpenBatch {
        slot,
        numbers: five(),
    });
    assert!(rig.state.merge_view().is_none());
    assert!(rig.state.session.is_dirty());
    assert!(rig.nodes() > 7);
    assert!(matches!(
        rig.notice(),
        Some(MergeNotice::BatchDone { merged: 2, .. })
    ));
}

#[test]
fn cancelling_the_reading_changes_nothing() {
    let (mut rig, slot, _dir) = sample(A);
    let effects = rig.raw(MergeMessage::OpenBatch {
        slot,
        numbers: five(),
    });
    rig.raw(MergeMessage::Cancel);
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    assert!(rig.state.merge_view().is_none());
    assert!(!rig.state.session.is_dirty());
}

#[test]
fn a_batch_that_merges_nothing_only_reports() {
    let (mut rig, slot, _dir) = sample(A);
    rig.send(MergeMessage::OpenBatch {
        slot,
        numbers: vec![OTHER_START, A_COPY],
    });
    assert!(rig.state.merge_view().is_none());
    assert_eq!(
        rig.notice(),
        Some(MergeNotice::BatchDone {
            merged: 0,
            skipped: 2
        })
    );
    assert!(!rig.state.session.is_dirty());
}

#[test]
fn set_end_ply_is_ignored_in_a_batch() {
    let (mut rig, slot, _dir) = sample(A);
    rig.send(MergeMessage::OpenBatch {
        slot,
        numbers: vec![B],
    });
    rig.raw(MergeMessage::SetEndPly(Some(2)));
    assert_eq!(rig.state.merge_view().unwrap().end_ply, None);
}

#[test]
fn the_filter_gives_the_first_games_in_order() {
    let (mut rig, slot, _dir) = sample(A);
    let effects = rig.raw(MergeMessage::OpenFilter { slot, limit: 3 });
    // 1 is the game itself: games 2 and 3 are read.
    assert!(
        matches!(effects.as_slice(), [Effect::LoadGamesForMerge { numbers, .. }] if *numbers == vec![B, C])
    );
}

#[test]
fn too_many_games_are_refused() {
    let (mut rig, slot, _dir) = sample(A);
    let numbers = vec![B; BATCH_MAX + 1];
    let effects = rig.raw(MergeMessage::OpenBatch { slot, numbers });
    assert_eq!(effects, vec![]);
    assert_eq!(rig.notice(), Some(MergeNotice::TooMany));
}

#[test]
fn a_batch_refuses_in_the_same_cases_as_a_single_merge() {
    let (mut rig, slot, _dir) = sample(A);
    rig.send(GameMessage::EnterTrial { null_move: false });
    let effects = rig.raw(MergeMessage::OpenBatch {
        slot,
        numbers: vec![B],
    });
    assert_eq!(effects, vec![]);
    assert_eq!(rig.notice(), Some(MergeNotice::Trial));
}

#[test]
fn the_notices_have_distinct_fluent_keys() {
    let all = [
        MergeNotice::NoGame,
        MergeNotice::Closed,
        MergeNotice::InvalidNumber,
        MergeNotice::SameGame,
        MergeNotice::EmptyGame,
        MergeNotice::Decode,
        MergeNotice::NoMatch,
        MergeNotice::NothingToAdd,
        MergeNotice::Failed,
        MergeNotice::Trial,
        MergeNotice::TooMany,
        MergeNotice::BatchDone {
            merged: 1,
            skipped: 1,
        },
    ];
    let mut keys: Vec<_> = all.iter().map(|n| n.message_key()).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), all.len());
    assert!(keys.iter().all(|k| k.starts_with("merge-")));
    assert_eq!(
        AppNotice::Merge(MergeNotice::NoMatch).message_key(),
        "merge-err-nomatch"
    );
}
