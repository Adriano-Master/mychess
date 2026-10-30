//! M7-1b contract: the duplicate games (spec `m7-1-manutencao.md`, sections 5
//! and 6, rules 16 to 26, criterion 27): the criteria and their defaults, the
//! two warnings of weak criteria, "undelete first", the protection, the
//! filter set to what was marked, and the checker of pairs.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m7_1b_common;

use m7_1b_common::*;
use mychess_app::maintenance::{DeleteChoice, WeakCriteria};
use mychess_db::GameSource;
use mychess_db::si4::Si4Database;
use tempfile::tempdir;

/// The duplicate base with the criteria the spec uses to test the engine:
/// the games have fewer than 10 half moves, so "skip short" is off.
fn twins_rig(dir: &std::path::Path) -> (Rig, SlotId, std::path::PathBuf) {
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir, "dups", &twins());
    rig.set_prefs(|p| p.twins.skip_short = false);
    rig.send(MaintenanceMessage::Open);
    (rig, slot, path)
}

fn confirm_of(rig: &Rig) -> mychess_app::maintenance::ConfirmView {
    rig.state.maintenance_view().confirm.expect("a question")
}

#[test]
fn the_defaults_are_those_of_the_scid_and_undelete_first_is_off() {
    let rig = Rig::new();
    let t = rig.state.prefs.maintenance.twins;
    assert!(t.exact_names && t.same_colors && t.same_site && t.same_round);
    assert!(t.same_year && t.same_month && t.same_moves && t.skip_short);
    assert!(!t.same_event && !t.same_day && !t.same_result && !t.same_eco);
    assert!(t.set_filter && t.keep_commented && t.keep_with_variations);
    assert!(
        !t.undelete_first,
        "D3524: the work of the user is not erased"
    );
    assert!(!t.copy_ratings && !t.only_filter);
    assert_eq!(t.delete, DeleteChoice::Shorter);
}

#[test]
fn the_search_reads_then_asks_with_the_count_and_marks_only_after_the_yes() {
    let dir = tempdir().unwrap();
    let (mut rig, slot, path) = twins_rig(dir.path());
    let before = files_of(&path);
    let effects = rig.raw(MaintenanceMessage::Start(MaintRequest::FindTwins));
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(rig.state.maintenance_job(), Some(MaintJobKind::Twins));
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    assert_eq!(files_of(&path), before, "the search only reads");
    // 1, 2 and 4 are one group; the shorter game of (1,2) is game 1; the
    // pair (2,4) would delete the commented game 4, which is kept.
    let confirm = confirm_of(&rig);
    assert_eq!(confirm.kind, ConfirmKind::MarkTwins);
    assert_eq!(confirm.count, 1);
    assert!(confirm.weak.is_empty());
    assert_eq!(disk_deleted(&path), vec![false; 5]);
    rig.send(MaintenanceMessage::Confirm(true));
    assert_eq!(disk_deleted(&path), vec![true, false, false, false, false]);
    assert_eq!(
        rig.filter_numbers(slot),
        vec![1],
        "the filter is what was marked"
    );
    assert_eq!(
        rig.maint_notice(),
        Some(MaintNotice::TwinsMarked {
            marked: 1,
            undeleted: 0
        })
    );
}

#[test]
fn answering_no_marks_nothing_and_leaves_the_filter() {
    let dir = tempdir().unwrap();
    let (mut rig, slot, path) = twins_rig(dir.path());
    rig.send(MaintenanceMessage::Start(MaintRequest::FindTwins));
    rig.send(MaintenanceMessage::Confirm(false));
    assert_eq!(disk_deleted(&path), vec![false; 5]);
    assert_eq!(rig.filter_numbers(slot), vec![1, 2, 3, 4, 5]);
    assert_eq!(rig.state.maintenance_job(), None);
}

#[test]
fn without_the_protection_the_commented_game_is_marked_too() {
    let dir = tempdir().unwrap();
    let (mut rig, _, path) = twins_rig(dir.path());
    rig.set_prefs(|p| {
        p.twins.keep_commented = false;
        p.twins.delete = DeleteChoice::HigherNumber;
    });
    rig.send(MaintenanceMessage::Start(MaintRequest::FindTwins));
    assert_eq!(confirm_of(&rig).count, 2);
    rig.send(MaintenanceMessage::Confirm(true));
    assert_eq!(disk_deleted(&path), vec![false, true, false, true, false]);
}

#[test]
fn the_lower_number_choice_marks_games_one_and_two() {
    let dir = tempdir().unwrap();
    let (mut rig, _, path) = twins_rig(dir.path());
    rig.set_prefs(|p| p.twins.delete = DeleteChoice::LowerNumber);
    rig.send(MaintenanceMessage::Start(MaintRequest::FindTwins));
    assert_eq!(confirm_of(&rig).count, 2);
    rig.send(MaintenanceMessage::Confirm(true));
    assert_eq!(disk_deleted(&path), vec![true, true, false, false, false]);
}

#[test]
fn a_search_with_nothing_to_delete_says_so_and_asks_nothing() {
    let dir = tempdir().unwrap();
    let (mut rig, slot, path) = twins_rig(dir.path());
    // Only the filter 3, 4, 5 is looked at: no pair among them.
    rig.select(slot, &[3, 4, 5]);
    rig.send(BasesMessage::List(ListMessage::KeepSelected { slot }));
    rig.set_prefs(|p| p.twins.only_filter = true);
    rig.send(MaintenanceMessage::Start(MaintRequest::FindTwins));
    assert_eq!(rig.maint_notice(), Some(MaintNotice::TwinsNone));
    assert!(rig.state.maintenance_view().confirm.is_none());
    assert_eq!(disk_deleted(&path), vec![false; 5]);
    // The games outside the filter come in again: the pair is found.
    rig.send(BasesMessage::List(ListMessage::ResetFilter { slot }));
    rig.select(slot, &[1, 2, 3]);
    rig.send(BasesMessage::List(ListMessage::KeepSelected { slot }));
    rig.send(MaintenanceMessage::Start(MaintRequest::FindTwins));
    assert_eq!(confirm_of(&rig).count, 1);
}

#[test]
fn turning_set_filter_off_leaves_the_filter_alone() {
    let dir = tempdir().unwrap();
    let (mut rig, slot, path) = twins_rig(dir.path());
    rig.set_prefs(|p| p.twins.set_filter = false);
    rig.send(MaintenanceMessage::Start(MaintRequest::FindTwins));
    rig.send(MaintenanceMessage::Confirm(true));
    assert_eq!(disk_deleted(&path), vec![true, false, false, false, false]);
    assert_eq!(rig.filter_numbers(slot), vec![1, 2, 3, 4, 5]);
}

#[test]
fn weak_criteria_are_confirmed_before_the_search_with_both_warnings() {
    let dir = tempdir().unwrap();
    let (mut rig, _, path) = twins_rig(dir.path());
    // The default criteria are not weak: the search starts at once.
    let effects = rig.raw(MaintenanceMessage::Start(MaintRequest::FindTwins));
    assert_eq!(effects.len(), 1);
    rig.raw(MaintenanceMessage::Stop);
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    assert_eq!(rig.state.maintenance_job(), None);
    rig.set_prefs(|p| {
        p.twins.same_moves = false;
        p.twins.same_site = false;
        p.twins.same_round = false;
    });
    let effects = rig.raw(MaintenanceMessage::Start(MaintRequest::FindTwins));
    assert!(effects.is_empty(), "the warning comes first: {effects:?}");
    let confirm = confirm_of(&rig);
    assert_eq!(confirm.kind, ConfirmKind::WeakTwinCriteria);
    assert_eq!(
        confirm.weak,
        vec![WeakCriteria::NoMoves, WeakCriteria::FewFields]
    );
    assert_eq!(rig.state.maintenance_job(), None);
    rig.send(MaintenanceMessage::Confirm(false));
    assert_eq!(rig.state.maintenance_job(), None);
    assert_eq!(disk_deleted(&path), vec![false; 5]);
    rig.send(MaintenanceMessage::Start(MaintRequest::FindTwins));
    let effects = rig.raw(MaintenanceMessage::Confirm(true));
    assert_eq!(effects.len(), 1, "the yes starts the search: {effects:?}");
}

#[test]
fn undelete_first_asks_with_the_count_then_searches_and_reports_what_it_undeleted() {
    let dir = tempdir().unwrap();
    let (mut rig, slot, path) = twins_rig(dir.path());
    // The user deleted game 1 by hand; with the option off the search
    // ignores it (a deleted game is not a candidate).
    rig.select(slot, &[1]);
    rig.send(BasesMessage::List(ListMessage::ToggleDeleted { slot }));
    assert_eq!(disk_deleted(&path), vec![true, false, false, false, false]);
    rig.set_prefs(|p| p.twins.undelete_first = true);
    rig.send(MaintenanceMessage::Start(MaintRequest::FindTwins));
    let confirm = confirm_of(&rig);
    assert_eq!(confirm.kind, ConfirmKind::UndeleteFirst);
    assert_eq!(confirm.count, 1, "one game loses its mark");
    assert_eq!(
        disk_deleted(&path),
        vec![true, false, false, false, false],
        "nothing before the answer"
    );
    rig.send(MaintenanceMessage::Confirm(true));
    // The search ran on the undeleted base and now asks about the marking.
    assert_eq!(disk_deleted(&path), vec![false; 5]);
    let confirm = confirm_of(&rig);
    assert_eq!(confirm.kind, ConfirmKind::MarkTwins);
    assert_eq!(confirm.count, 1);
    rig.send(MaintenanceMessage::Confirm(true));
    assert_eq!(disk_deleted(&path), vec![true, false, false, false, false]);
    assert_eq!(
        rig.maint_notice(),
        Some(MaintNotice::TwinsMarked {
            marked: 1,
            undeleted: 1
        })
    );
}

#[test]
fn the_search_can_be_stopped_and_then_asks_nothing() {
    let dir = tempdir().unwrap();
    let (mut rig, _, path) = twins_rig(dir.path());
    let before = files_of(&path);
    let effects = rig.raw(MaintenanceMessage::Start(MaintRequest::FindTwins));
    rig.raw(MaintenanceMessage::Stop);
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    assert!(rig.state.maintenance_view().confirm.is_none());
    assert_eq!(rig.state.maintenance_job(), None);
    assert_eq!(rig.state.maintenance_view().twin_pairs, 0);
    assert_eq!(files_of(&path), before);
}

// ---- the checker ----

#[test]
fn the_checker_walks_the_pairs_flips_each_side_and_asks_before_undeleting_all() {
    let dir = tempdir().unwrap();
    let (mut rig, _, path) = twins_rig(dir.path());
    rig.set_prefs(|p| p.twins.set_filter = false);
    rig.send(MaintenanceMessage::Start(MaintRequest::FindTwins));
    rig.send(MaintenanceMessage::Confirm(true));
    assert_eq!(disk_deleted(&path), vec![true, false, false, false, false]);
    // The pairs of the search: (1,2) and (2,4).
    rig.send(MaintenanceMessage::OpenChecker);
    let pair = rig.state.twin_pair_view().expect("a pair");
    assert_eq!((pair.position, pair.total), (1, 2));
    assert_eq!((pair.left, pair.right), (1, 2));
    assert!(pair.left_deleted && !pair.right_deleted);
    let diff = pair.diff.expect("the comparison came back");
    assert!(diff.has_differences(), "the moves of 2 go further");
    // Next pair.
    rig.send(MaintenanceMessage::TwinStep(1));
    let pair = rig.state.twin_pair_view().unwrap();
    assert_eq!((pair.position, pair.left, pair.right), (2, 2, 4));
    rig.send(MaintenanceMessage::TwinStep(1));
    assert_eq!(
        rig.state.twin_pair_view().unwrap().position,
        2,
        "the last stays"
    );
    // `1` flips the left game (2), `2` the right one (4), `t` both.
    rig.send(MaintenanceMessage::TwinToggle(TwinSide::Left));
    assert_eq!(disk_deleted(&path), vec![true, true, false, false, false]);
    assert!(rig.state.twin_pair_view().is_some(), "the pairs still hold");
    rig.send(MaintenanceMessage::TwinToggle(TwinSide::Right));
    assert_eq!(disk_deleted(&path), vec![true, true, false, true, false]);
    rig.send(MaintenanceMessage::TwinToggleBoth);
    assert_eq!(disk_deleted(&path), vec![true, false, false, false, false]);
    let pair = rig.state.twin_pair_view().unwrap();
    assert!(!pair.left_deleted && !pair.right_deleted);
    // `U` asks first, with the count of the deleted games.
    rig.send(MaintenanceMessage::TwinUndeleteAll);
    let confirm = confirm_of(&rig);
    assert_eq!(confirm.kind, ConfirmKind::UndeleteAll);
    assert_eq!(confirm.count, 1);
    assert_eq!(disk_deleted(&path), vec![true, false, false, false, false]);
    rig.send(MaintenanceMessage::Confirm(true));
    assert_eq!(disk_deleted(&path), vec![false; 5]);
}

#[test]
fn the_checker_needs_pairs_and_goes_away_when_the_games_are_rewritten() {
    let dir = tempdir().unwrap();
    let (mut rig, slot, _) = twins_rig(dir.path());
    rig.send(MaintenanceMessage::OpenChecker);
    assert_eq!(rig.maint_notice(), Some(MaintNotice::NoPair));
    assert!(rig.state.twin_pair_view().is_none());
    rig.send(MaintenanceMessage::Start(MaintRequest::FindTwins));
    rig.send(MaintenanceMessage::Confirm(false));
    rig.send(MaintenanceMessage::OpenChecker);
    assert!(rig.state.twin_pair_view().is_some());
    // A sort renumbers the games: the pairs mean nothing any more.
    rig.send(MaintenanceMessage::Start(MaintRequest::Sort {
        keys: vec![SortCriterion::Result],
        reverse: true,
        compact_after: false,
    }));
    rig.send(MaintenanceMessage::Confirm(true));
    assert!(rig.state.twin_pair_view().is_none());
    assert_eq!(rig.state.maintenance_view().twin_pairs, 0);
    let _ = slot;
}

#[test]
fn sharing_tags_asks_with_the_count_and_writes_the_rating_of_the_other_game() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    // Two games alike; the second one knows the rating of White.
    let mut games = vec![
        g(
            "Aberto",
            "Rio",
            "2020.01.10",
            "Alfa, A",
            "Beta, B",
            "*",
            0,
            0,
        ),
        g(
            "Aberto",
            "Rio",
            "2020.01.10",
            "Alfa, A",
            "Beta, B",
            "*",
            2500,
            0,
        ),
    ];
    for game in &mut games {
        game.moves = "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6";
    }
    let (_, path) = rig.open_si4(dir.path(), "share", &games);
    rig.set_prefs(|p| p.twins.skip_short = false);
    rig.send(MaintenanceMessage::Open);
    rig.send(MaintenanceMessage::Start(MaintRequest::FindTwins));
    rig.send(MaintenanceMessage::Confirm(false));
    rig.send(MaintenanceMessage::OpenChecker);
    let before = files_of(&path);
    rig.send(MaintenanceMessage::TwinShare);
    let confirm = confirm_of(&rig);
    assert_eq!(confirm.kind, ConfirmKind::ShareTags);
    assert_eq!(confirm.count, 1, "WhiteElo");
    assert_eq!(files_of(&path), before, "nothing before the yes");
    assert_eq!(rig.state.twin_pair_view().unwrap().share.unwrap().len(), 1);
    rig.send(MaintenanceMessage::Confirm(true));
    let db = Si4Database::open(&path).unwrap();
    assert_eq!(db.entry(1).unwrap().white_elo, 2500);
    assert_eq!(
        rig.maint_notice(),
        Some(MaintNotice::TagsShared { count: 1 })
    );
    // Nothing is left to share.
    rig.send(MaintenanceMessage::TwinShare);
    assert_eq!(rig.maint_notice(), Some(MaintNotice::NothingToShare));
}

#[test]
fn the_clipbase_searches_and_marks_in_memory() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "dups", &twins());
    rig.select(slot, &[1, 2, 3, 4, 5]);
    rig.send(BasesMessage::List(ListMessage::CopySelected { slot }));
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    rig.set_prefs(|p| p.twins.skip_short = false);
    rig.send(MaintenanceMessage::Start(MaintRequest::FindTwins));
    assert_eq!(confirm_of(&rig).count, 1);
    rig.send(MaintenanceMessage::Confirm(true));
    let clip = rig.state.bases.slot(SlotId::CLIPBASE).unwrap();
    let source = clip.source.clone();
    let deleted: Vec<bool> = (1..=5)
        .map(|n| {
            source
                .read()
                .entry(n)
                .unwrap()
                .flags
                .contains(mychess_db::si4::GameFlags::DELETE)
        })
        .collect();
    assert_eq!(deleted, vec![true, false, false, false, false]);
}
