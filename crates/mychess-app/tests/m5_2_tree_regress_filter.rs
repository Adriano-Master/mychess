//! M5-2a review regressions, part 2: the filter of the user and the filter
//! the tree writes (D979 "user filter", D986 filter loss, D987 deferred
//! restore, D988 closed tree).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m5_2_common;

use std::collections::VecDeque;

use m5_2_common::*;
use mychess_app::{CopyScope, ListColumn, PrefChange, PrefsMessage};
use tempfile::tempdir;

/// The std base, tree open, adjust on, cursor after 1.e4: the filter is
/// `[1, 3, 5]`.
fn adjusted_after_e4(dir: &std::path::Path) -> (Rig, SlotId) {
    let mut rig = Rig::std_with_tree(dir);
    let slot = rig.state.bases.current();
    rig.play("e2e4");
    rig.send(TreeMessage::SetAdjust(true));
    assert_eq!(rig.filter_numbers(slot), [1, 3, 5], "precondition");
    (rig, slot)
}

/// Runs the effects and the ones their answers produce.
fn settle(rig: &mut Rig, effects: Vec<Effect>) {
    let mut queue: VecDeque<Effect> = effects.into();
    while let Some(effect) = queue.pop_front() {
        if effect.runs_on_host() {
            continue;
        }
        let answer = rig.perform(effect);
        queue.extend(rig.raw(answer));
    }
}

// ---- D979: the filter of the user ----

#[test]
fn a_filter_made_by_the_user_while_adjusted_becomes_the_base_of_the_next_search() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = adjusted_after_e4(dir.path());
    rig.keep(slot, &[1, 3]);
    assert_eq!(
        rig.filter_numbers(slot),
        [1, 3],
        "the user's filter is there"
    );
    rig.play("e7e5");
    assert_eq!(
        rig.filter_numbers(slot),
        [1],
        "base [1, 3] meets reach {{1}}"
    );
    rig.nav(Nav::Back(1));
    // The old base (all five games) would give [1, 3, 5] again.
    assert_eq!(rig.filter_numbers(slot), [1, 3]);
}

#[test]
fn a_negated_filter_made_while_adjusted_is_not_overwritten_by_the_old_base() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = adjusted_after_e4(dir.path());
    rig.send(BasesMessage::List(ListMessage::NegateFilter { slot }));
    assert_eq!(rig.filter_numbers(slot), [2, 4], "precondition: negated");
    rig.play("e7e5");
    // [2, 4] has no game that goes 1.e4 e5; the old base would show [1].
    assert_eq!(rig.filter_numbers(slot), Vec::<u32>::new());
    rig.send(TreeMessage::Close);
    assert_eq!(
        rig.filter_numbers(slot),
        [2, 4],
        "the negated one is the base"
    );
}

#[test]
fn closing_right_after_the_user_changed_the_filter_keeps_the_users_filter() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = adjusted_after_e4(dir.path());
    rig.keep(slot, &[1, 3]);
    rig.send(TreeMessage::Close);
    assert_eq!(rig.filter_numbers(slot), [1, 3]);
}

#[test]
fn closing_after_a_refresh_with_a_user_filter_restores_the_users_filter_not_the_old_one() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = adjusted_after_e4(dir.path());
    rig.keep(slot, &[1, 3]);
    rig.play("e7e5");
    assert_eq!(rig.filter_numbers(slot), [1]);
    rig.send(TreeMessage::Close);
    assert_eq!(rig.filter_numbers(slot), [1, 3]);
}

#[test]
fn closing_without_any_change_of_the_user_restores_the_filter_before_the_adjust() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let slot = rig.state.bases.current();
    rig.send(TreeMessage::SetAdjust(true));
    rig.play("e2e4");
    assert_eq!(rig.filter_numbers(slot), [1, 3, 5]);
    rig.play("e7e5");
    assert_eq!(rig.filter_numbers(slot), [1]);
    rig.nav(Nav::Back(1));
    rig.nav(Nav::Start);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5]);
    rig.nav(Nav::End);
    assert_eq!(rig.filter_numbers(slot), [1]);
    rig.send(TreeMessage::Close);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5]);
}

#[test]
fn turning_adjust_off_and_on_after_a_user_filter_stays_coherent() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = adjusted_after_e4(dir.path());
    rig.keep(slot, &[1, 3]);
    rig.play("e7e5");
    assert_eq!(rig.filter_numbers(slot), [1]);
    // Off: what is there is the base from now on.
    rig.send(TreeMessage::SetAdjust(false));
    assert_eq!(rig.filter_numbers(slot), [1]);
    // On again, back in the position of 1.e4: base [1] meets reach {1, 3, 5}.
    rig.send(TreeMessage::SetAdjust(true));
    rig.nav(Nav::Back(1));
    assert_eq!(rig.filter_numbers(slot), [1]);
    rig.send(TreeMessage::Close);
    assert_eq!(rig.filter_numbers(slot), [1]);
    // And a reopened panel adjusts from that base.
    rig.send(TreeMessage::Open);
    assert_eq!(rig.filter_numbers(slot), [1]);
}

#[test]
fn a_user_filter_after_turning_adjust_off_is_never_touched_by_the_tree() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = adjusted_after_e4(dir.path());
    rig.send(TreeMessage::SetAdjust(false));
    rig.keep(slot, &[1, 5]);
    rig.play("e7e5");
    rig.nav(Nav::Back(1));
    assert_eq!(rig.filter_numbers(slot), [1, 5]);
    rig.send(TreeMessage::Close);
    assert_eq!(rig.filter_numbers(slot), [1, 5]);
}

// ---- D987: restore deferred by a running list job ----

#[test]
fn closing_with_a_list_job_running_restores_the_filter_when_the_job_ends() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = adjusted_after_e4(dir.path());
    let job = rig.raw(BasesMessage::List(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    assert!(rig.state.list_job().is_some(), "precondition: a job runs");
    rig.raw(TreeMessage::Close);
    assert_eq!(
        rig.filter_numbers(slot),
        [1, 3, 5],
        "waits for the job: the list is not changed under it"
    );
    settle(&mut rig, job);
    assert!(rig.state.list_job().is_none());
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5], "restored");
    assert!(!rig.state.tree().searching());
}

#[test]
fn the_filter_of_the_user_wins_over_a_restore_deferred_by_a_list_job() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = adjusted_after_e4(dir.path());
    // The user narrows the list, then starts a sort and closes the panel.
    rig.keep(slot, &[1, 3]);
    let job = rig.raw(BasesMessage::List(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    assert!(rig.state.list_job().is_some(), "precondition: a job runs");
    rig.raw(TreeMessage::Close);
    settle(&mut rig, job);
    assert!(rig.state.list_job().is_none());
    assert_eq!(rig.filter_numbers(slot), [1, 3]);
}

#[test]
fn a_text_filter_that_ends_after_closing_wins_over_the_deferred_restore() {
    let dir = tempdir().unwrap();
    // Only game 2 has the White player "Zed".
    let text = format!(
        "{}{}{}",
        game_pgn("1. e4 e5", "1-0", 0, 0, "2000.01.01", None),
        game_pgn("1. e4 c5", "1-0", 0, 0, "2000.01.01", None)
            .replace("[White \"W\"]", "[White \"Zed\"]"),
        game_pgn("1. e4 e6", "1-0", 0, 0, "2000.01.01", None),
    );
    let mut rig = Rig::new();
    let slot = rig.open_pgn(dir.path(), "t.pgn", &text);
    rig.open_tree();
    rig.play("e2e4");
    rig.send(TreeMessage::SetAdjust(true));
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3]);
    rig.send(BasesMessage::SetSearchText {
        slot,
        text: "Zed".to_owned(),
    });
    let job = rig.raw(BasesMessage::List(ListMessage::FilterByText { slot }));
    assert!(rig.state.list_job().is_some(), "precondition: a job runs");
    rig.raw(TreeMessage::Close);
    settle(&mut rig, job);
    assert!(rig.state.list_job().is_none());
    assert_eq!(
        rig.filter_numbers(slot),
        [2],
        "the result of the user's search"
    );
}

// ---- D986: a lost TreeState, a base that changes size ----

#[test]
fn a_base_that_grows_during_an_adjusted_search_leaves_a_whole_filter_after_close() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let source = rig.open_pgn(dir.path(), "std.pgn", STD);
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::ConfirmCopy(
        false,
    ))));
    let copy = |rig: &mut Rig| {
        rig.send(BasesMessage::List(ListMessage::CopyGames {
            from: source,
            to: SlotId::CLIPBASE,
            scope: CopyScope::Filter,
        }));
    };
    copy(&mut rig);
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    rig.open_tree();
    rig.send(TreeMessage::SetAdjust(true));
    // A search is in flight with a filter of 5 games ...
    let mut flight = jobs(rig.raw(GameMessage::PlayMove {
        mv: rig.state.session.position().parse_uci("e2e4").unwrap(),
        options: PlayOptions::default(),
    }));
    assert_eq!(flight.len(), 1);
    // ... and the base grows to 10 meanwhile.
    copy(&mut rig);
    let clip = rig.state.bases.slot(SlotId::CLIPBASE).unwrap();
    assert_eq!(clip.total(), 10);
    // The stale search answers `FilterSize` (D972) and everything settles.
    let answer = rig.run_job(flight.remove(0));
    assert!(format!("{answer:?}").contains("FilterSize"), "{answer:?}");
    let more = rig.raw(answer);
    settle(&mut rig, more);
    assert!(!rig.state.tree().searching());
    let clip = rig.state.bases.slot(SlotId::CLIPBASE).unwrap();
    assert_eq!(
        clip.filter().len(),
        10,
        "the filter has the size of the base"
    );
    // Six of the ten games reach 1.e4.
    assert_eq!(rig.view().total.expect("total").games, 6);
    assert_eq!(rig.filtered(SlotId::CLIPBASE), 6);
    rig.send(TreeMessage::Close);
    assert_eq!(
        rig.filtered(SlotId::CLIPBASE),
        10,
        "no adjusted filter is left behind"
    );
    // Reopening and closing again does not lose or stick anything.
    rig.send(TreeMessage::Open);
    rig.send(TreeMessage::Close);
    assert_eq!(rig.filtered(SlotId::CLIPBASE), 10);
}

// ---- D988: a closed tree ----

#[test]
fn with_the_panel_closed_moves_and_navigation_search_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.send(TreeMessage::Close);
    let started = rig.started();
    for uci in ["e2e4", "e7e5", "g1f3"] {
        let mv = rig.state.session.position().parse_uci(uci).unwrap();
        let effects = rig.raw(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        });
        assert_eq!(tree_searches(&effects), 0);
        settle(&mut rig, effects);
    }
    rig.nav(Nav::Start);
    assert_eq!(rig.started(), started);
    assert_eq!(rig.view().body, TreeBody::Closed);
    assert!(!rig.state.tree().searching());
}

#[test]
fn closing_the_slot_with_the_panel_closed_does_not_reuse_the_state_for_the_next_base() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let first = rig.open_pgn(dir.path(), "a.pgn", STD);
    rig.open_tree();
    assert_eq!(rig.sorted_sans(), ["Nf3", "d4", "e4"]);
    rig.send(TreeMessage::Close);
    rig.send(BasesMessage::Close(first));
    // Another base, with the same number of games and other moves, in the
    // slot that was freed.
    let other = "[Result \"1-0\"]\n\n1. c4 e5 1-0\n\n\
        [Result \"1-0\"]\n\n1. c4 c5 1-0\n\n\
        [Result \"1-0\"]\n\n1. g3 d5 1-0\n\n\
        [Result \"1-0\"]\n\n1. b3 d5 1-0\n\n\
        [Result \"1-0\"]\n\n1. c4 Nf6 1-0\n\n";
    let second = rig.open_pgn(dir.path(), "b.pgn", other);
    rig.send(TreeMessage::Open);
    assert_eq!(rig.state.bases.slot(second).unwrap().total(), 5);
    assert_eq!(rig.sorted_sans(), ["b3", "c4", "g3"]);
    assert_eq!(rig.row("c4").games, 3);
    assert_eq!(rig.view().total.expect("total").games, 5);
}

#[test]
fn switching_slots_with_the_panel_closed_searches_the_right_base_on_reopening() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let first = rig.open_pgn(dir.path(), "a.pgn", STD);
    rig.open_tree();
    rig.send(TreeMessage::Close);
    let other = "[Result \"1-0\"]\n\n1. c4 e5 1-0\n\n";
    let second = rig.open_pgn(dir.path(), "b.pgn", other);
    assert_ne!(first, second);
    rig.send(TreeMessage::Open);
    assert_eq!(rig.sans(), ["c4"]);
    rig.send(TreeMessage::Close);
    rig.send(BasesMessage::Select(first));
    rig.send(TreeMessage::Open);
    assert_eq!(rig.sorted_sans(), ["Nf3", "d4", "e4"]);
    assert_eq!(rig.view().total.expect("total").games, 5);
}
