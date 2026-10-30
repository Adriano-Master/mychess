//! M7-1b contract: sorting carries the filter, the selection and the open
//! game through the permutation; compacting asks with the count and starts
//! over (spec `m7-1-manutencao.md`, sections 8 and 9, rules 3, 40 and 41,
//! criteria 26 and 30).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m7_1b_common;

use m7_1b_common::*;
use mychess_app::{DiscardChoice, Pending};
use mychess_app::{GameMessage, Nav, PlayOptions};
use mychess_db::si4::{NameKind, Si4Database};
use tempfile::tempdir;

/// Six games, each with a player of its own and the years out of order:
/// the sort by year puts them at `ORDER` (0-based new positions).
fn shuffled() -> Vec<G> {
    let game = |white, date| g("Open", "Rio", date, white, "Zed, Z", "1-0", 0, 0);
    vec![
        game("P1", "2022.01.01"),
        game("P2", "2020.01.01"),
        game("P3", "2021.01.01"),
        game("P4", "2020.06.01"),
        game("P5", "2023.01.01"),
        game("P6", "2019.01.01"),
    ]
}

/// Stable sort by year: P6, P2, P4, P3, P1, P5.
const ORDER: [u32; 6] = [4, 1, 3, 2, 5, 0];

fn by_year() -> MaintRequest {
    MaintRequest::Sort {
        keys: vec![SortCriterion::Year],
        reverse: false,
        compact_after: false,
    }
}

fn whites(path: &std::path::Path) -> Vec<String> {
    disk_games(path).into_iter().map(|g| g.0).collect()
}

/// Extends the main line of the open game (all the sample games end after
/// `2... Nc6`) with `3. Bb5`, so the game is altered.
fn play_a_move(rig: &mut Rig) {
    rig.send(GameMessage::Navigate(Nav::End));
    let mv = rig.state.session.position().parse_uci("f1b5").expect("uci");
    rig.send(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
}

#[test]
fn sorting_asks_with_the_count_and_does_nothing_until_the_answer() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "g", &shuffled());
    let before = files_of(&path);
    let effects = rig.send(MaintenanceMessage::Start(by_year()));
    assert!(effects.is_empty());
    let view = rig.state.maintenance_view();
    let confirm = view.confirm.expect("a question");
    assert_eq!(confirm.kind, ConfirmKind::Sort);
    assert_eq!(confirm.count, 6, "the games whose index is rewritten");
    assert!(confirm.backup_offered);
    assert_eq!(files_of(&path), before, "nothing before the answer");
    assert_eq!(rig.state.maintenance_job(), None);
    // "No" drops the question and still writes nothing.
    rig.send(MaintenanceMessage::Confirm(false));
    assert!(rig.state.maintenance_view().confirm.is_none());
    assert_eq!(files_of(&path), before);
    // "Yes" sorts.
    rig.send(MaintenanceMessage::Start(by_year()));
    rig.send(MaintenanceMessage::Confirm(true));
    assert_eq!(
        whites(&path),
        ["P6", "P2", "P4", "P3", "P1", "P5"].map(String::from)
    );
}

#[test]
fn the_permutation_carries_the_filter_the_selection_and_the_open_game() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "g", &shuffled());
    // The open game is P1 (number 1); the filter is P1, P3, P6; the
    // selection is P1 and P3, with P3 under the cursor.
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    rig.select(slot, &[1, 3, 6]);
    rig.send(BasesMessage::List(ListMessage::KeepSelected { slot }));
    rig.select(slot, &[1, 3]);
    let opened = rig.state.session.game().tags().white.clone();
    assert_eq!(opened, "P1");
    rig.send(MaintenanceMessage::Start(by_year()));
    rig.send(MaintenanceMessage::Confirm(true));
    assert_eq!(whites(&path)[4], "P1", "P1 moved to the fifth place");
    // Old 1, 3, 6 are new 5, 4, 1.
    let new = |old: u32| ORDER[(old - 1) as usize] + 1;
    assert_eq!(rig.filter_numbers(slot), vec![new(6), new(3), new(1)]);
    assert_eq!(rig.filter_numbers(slot), vec![1, 4, 5]);
    let state = rig.state.bases.slot(slot).unwrap();
    let selection: Vec<u32> = state.list.selection.iter().copied().collect();
    assert_eq!(selection, vec![4, 5]);
    assert_eq!(
        state.current_game,
        Some(5),
        "the open game follows its number"
    );
    // The game on the editor is still P1, and number 5 of the file is P1.
    assert_eq!(rig.state.session.game().tags().white, "P1");
    assert_eq!(disk_games(&path)[4].0, "P1");
    // The cursor of the selection moved too.
    assert_eq!(state.list.selected, Some(4));
    // The sorted view of the list was dropped (the order is the file's).
    assert!(state.list.sort.is_none());
}

#[test]
fn a_descending_sort_carries_the_filter_too_and_ties_keep_the_file_order() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "g", &shuffled());
    rig.send(MaintenanceMessage::Start(by_year()));
    rig.send(MaintenanceMessage::Confirm(true));
    // The file is P6, P2, P4, P3, P1, P5 now; the filter is P6 and P1.
    rig.select(slot, &[1, 5]);
    rig.send(BasesMessage::List(ListMessage::KeepSelected { slot }));
    rig.send(MaintenanceMessage::Start(MaintRequest::Sort {
        keys: vec![SortCriterion::Year],
        reverse: true,
        compact_after: false,
    }));
    rig.send(MaintenanceMessage::Confirm(true));
    // Descending by year: P5, P1, P3, P2, P4, P6 (P2 stays before P4).
    assert_eq!(
        whites(&path),
        ["P5", "P1", "P3", "P2", "P4", "P6"].map(String::from)
    );
    assert_eq!(rig.filter_numbers(slot), vec![2, 6], "P1 and P6");
}

#[test]
fn an_edited_game_is_not_lost_by_a_sort_and_keeps_its_number() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "g", &shuffled());
    rig.send(BasesMessage::LoadGame { slot, number: 3 });
    play_a_move(&mut rig);
    assert!(rig.state.is_dirty());
    rig.send(MaintenanceMessage::Start(by_year()));
    rig.send(MaintenanceMessage::Confirm(true));
    assert!(rig.state.discard_prompt.is_none(), "no game is rewritten");
    assert!(rig.state.is_dirty(), "the edits are still there");
    assert_eq!(rig.state.session.game().tags().white, "P3");
    // P3 is the fourth game of the sorted file.
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, Some(4));
}

#[test]
fn a_sort_needs_between_one_and_fifteen_criteria() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "g", &shuffled());
    let before = files_of(&path);
    for keys in [Vec::new(), vec![SortCriterion::Date; 16]] {
        let effects = rig.raw(MaintenanceMessage::Start(MaintRequest::Sort {
            keys,
            reverse: false,
            compact_after: false,
        }));
        assert!(effects.is_empty());
        assert_eq!(rig.maint_notice(), Some(MaintNotice::BadSort));
        assert!(rig.state.maintenance_view().confirm.is_none());
    }
    rig.send(MaintenanceMessage::Start(MaintRequest::Sort {
        keys: vec![SortCriterion::Date; 15],
        reverse: false,
        compact_after: false,
    }));
    assert!(
        rig.state.maintenance_view().confirm.is_some(),
        "15 is the limit"
    );
    assert_eq!(files_of(&path), before);
}

#[test]
fn the_last_sort_is_remembered_in_the_preferences() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "g", &shuffled());
    rig.send(MaintenanceMessage::Start(MaintRequest::Sort {
        keys: vec![SortCriterion::Eco, SortCriterion::Date],
        reverse: true,
        compact_after: true,
    }));
    // Asking is not using: the preference changes when the job starts.
    assert_ne!(rig.state.prefs.maintenance.sort_keys.len(), 2);
    rig.send(MaintenanceMessage::Confirm(true));
    let prefs = &rig.state.prefs.maintenance;
    assert_eq!(
        prefs.sort_keys,
        vec![SortCriterion::Eco, SortCriterion::Date]
    );
    assert!(prefs.sort_reverse);
    assert!(prefs.compact_after_rewrite);
}

// ---- compacting ----

fn delete(rig: &mut Rig, games: &[u32]) {
    let slot = rig.state.bases.current();
    rig.select(slot, games);
    rig.send(BasesMessage::List(ListMessage::KeepSelected { slot }));
    rig.ask(MaintRequest::Flags {
        scope: FlagScope::Filter,
        flag: MaintFlag::Delete,
        action: FlagAction::Set,
    });
    rig.send(BasesMessage::List(ListMessage::ResetFilter { slot }));
}

#[test]
fn compacting_counts_what_will_be_deleted_before_asking_and_writes_nothing_meanwhile() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "g", &six());
    delete(&mut rig, &[2, 4]);
    let before = files_of(&path);
    let effects = rig.raw(MaintenanceMessage::Start(MaintRequest::Compact {
        names: false,
        games: true,
    }));
    assert!(
        matches!(
            effects.as_slice(),
            [Effect::Maintenance(MaintenanceEffect::Preview { .. })]
        ),
        "the count is an effect, not a job: {effects:?}"
    );
    assert_eq!(rig.state.maintenance_job(), None);
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    let confirm = rig.state.maintenance_view().confirm.expect("question");
    assert_eq!(confirm.kind, ConfirmKind::Compact);
    assert_eq!(confirm.count, 2, "N games will be deleted for good");
    assert!(confirm.backup_offered);
    assert_eq!(files_of(&path), before);
    // "No": nothing.
    rig.send(MaintenanceMessage::Confirm(false));
    assert_eq!(files_of(&path), before);
    assert_eq!(rig.state.bases.slot(slot).unwrap().total(), 6);
}

#[test]
fn compacting_removes_the_deleted_games_resets_the_list_and_starts_a_new_game() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "g", &six());
    delete(&mut rig, &[2, 4]);
    rig.send(BasesMessage::LoadGame { slot, number: 5 });
    rig.select(slot, &[5, 6]);
    rig.send(BasesMessage::List(ListMessage::KeepSelected { slot }));
    assert_eq!(rig.state.session.game().tags().white, "Alfa, A");
    rig.send(MaintenanceMessage::Start(MaintRequest::Compact {
        names: false,
        games: true,
    }));
    rig.send(MaintenanceMessage::Confirm(true));
    assert_eq!(disk_games(&path).len(), 4, "the deleted games are gone");
    let state = rig.state.bases.slot(slot).unwrap();
    assert_eq!(state.total(), 4);
    assert_eq!(
        rig.filter_numbers(slot),
        vec![1, 2, 3, 4],
        "the filter is everything"
    );
    assert!(state.list.selection.is_empty());
    assert_eq!(state.current_game, None);
    assert_eq!(
        rig.state.session.game().tags().white,
        mychess_core::Tags::default().white,
        "the game of the editor is a new one"
    );
    assert_eq!(
        rig.maint_notice(),
        Some(MaintNotice::Compacted {
            games_removed: 2,
            names_removed: 0
        })
    );
}

#[test]
fn a_database_with_nothing_to_compact_says_so_and_starts_no_job() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "g", &six());
    let before = files_of(&path);
    rig.send(MaintenanceMessage::Start(MaintRequest::Compact {
        names: true,
        games: true,
    }));
    assert_eq!(rig.maint_notice(), Some(MaintNotice::NothingToDo));
    assert_eq!(rig.state.maintenance_job(), None);
    assert!(rig.state.maintenance_view().confirm.is_none());
    assert_eq!(files_of(&path), before);
}

#[test]
fn the_names_column_compacts_on_its_own_without_a_question() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "g", &six());
    delete(&mut rig, &[5, 6]);
    rig.send(MaintenanceMessage::Start(MaintRequest::Compact {
        names: false,
        games: true,
    }));
    rig.send(MaintenanceMessage::Confirm(true));
    let players = |p: &std::path::Path| {
        let db = Si4Database::open(p).unwrap();
        mychess_db::GameSource::names(&db).num_names(NameKind::Player)
    };
    assert_eq!(players(&path), 4, "Delta is still in the name file");
    rig.send(MaintenanceMessage::Start(MaintRequest::Compact {
        names: true,
        games: false,
    }));
    assert!(
        rig.state.maintenance_view().confirm.is_none(),
        "no game is deleted"
    );
    assert_eq!(players(&path), 3, "the unused name is gone");
    assert_eq!(disk_games(&path).len(), 4);
}

#[test]
fn compacting_asks_the_altered_game_first_and_cancelling_starts_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "g", &six());
    delete(&mut rig, &[2]);
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    play_a_move(&mut rig);
    let before = files_of(&path);
    rig.send(MaintenanceMessage::Start(MaintRequest::Compact {
        names: false,
        games: true,
    }));
    rig.send(MaintenanceMessage::Confirm(true));
    assert!(
        matches!(
            rig.state.discard_prompt.as_ref().map(|p| &p.pending),
            Some(Pending::Maintenance)
        ),
        "the altered game is asked about before the games are rewritten"
    );
    assert_eq!(rig.state.maintenance_job(), None);
    rig.send(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert_eq!(
        rig.state.maintenance_job(),
        None,
        "cancelling starts no job"
    );
    assert_eq!(files_of(&path), before);
    assert!(rig.state.is_dirty(), "the edits stay");
    // Again, and "don't save": the job runs.
    rig.send(MaintenanceMessage::Start(MaintRequest::Compact {
        names: false,
        games: true,
    }));
    rig.send(MaintenanceMessage::Confirm(true));
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(disk_games(&path).len(), 5);
    assert!(!rig.state.is_dirty());
}

#[test]
fn a_preview_that_comes_back_after_the_slot_was_closed_is_dropped() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "g", &six());
    delete(&mut rig, &[2]);
    let effects = rig.raw(MaintenanceMessage::Start(MaintRequest::Compact {
        names: false,
        games: true,
    }));
    rig.send(BasesMessage::Close(slot));
    let before = files_of(&path);
    let answer = rig.perform(effects.into_iter().next().unwrap());
    let out = rig.raw(answer);
    assert!(out.is_empty(), "{out:?}");
    assert!(rig.state.maintenance_view().confirm.is_none());
    assert_eq!(rig.state.maintenance_job(), None);
    assert_eq!(files_of(&path), before);
}
