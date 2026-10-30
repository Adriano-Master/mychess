//! M7-1c review fixes (app side): the preview can be stopped, the question
//! about the altered game is forgotten when it is given up, the repair looks
//! at the databases open when it starts.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m7_1b_common;

use m7_1b_common::*;
use mychess_app::{DiscardChoice, GameMessage, Nav, PlayOptions};
use tempfile::tempdir;

fn compact() -> MaintRequest {
    MaintRequest::Compact {
        names: false,
        games: true,
    }
}

fn delete_two(rig: &mut Rig, slot: SlotId) {
    rig.select(slot, &[2]);
    rig.send(BasesMessage::List(ListMessage::KeepSelected { slot }));
    rig.ask(MaintRequest::Flags {
        scope: FlagScope::Filter,
        flag: MaintFlag::Delete,
        action: FlagAction::Set,
    });
    rig.send(BasesMessage::List(ListMessage::ResetFilter { slot }));
}

#[test]
fn the_preview_shows_in_the_view_and_stop_cancels_it_for_real() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "g", &six());
    delete_two(&mut rig, slot);
    let notice_before = rig.maint_notice();
    let effects = rig.raw(MaintenanceMessage::Start(compact()));
    let Some(Effect::Maintenance(MaintenanceEffect::Preview { control, .. })) = effects.first()
    else {
        panic!("a preview effect: {effects:?}");
    };
    let control = control.clone();
    assert!(rig.state.maintenance_view().previewing);
    assert!(rig.state.maintenance_view().working());
    assert!(rig.state.maintenance().preview_progress().is_some());
    assert!(!control.is_cancelled());
    rig.raw(MaintenanceMessage::Stop);
    assert!(control.is_cancelled(), "the work looks at this flag");
    assert!(!rig.state.maintenance_view().previewing);
    // The answer of the stopped work is dropped without a word.
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    assert!(rig.state.maintenance_view().confirm.is_none());
    assert_eq!(rig.maint_notice(), notice_before, "no new notice");
}

#[test]
fn a_cancelled_strip_preview_counts_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "g", &six());
    let effects = rig.raw(MaintenanceMessage::Start(MaintRequest::Strip {
        what: StripChoice::Comments,
        scope: ScopeChoice::All,
    }));
    let Some(Effect::Maintenance(MaintenanceEffect::Preview { control, .. })) = effects.first()
    else {
        panic!("a preview effect: {effects:?}");
    };
    control.cancel();
    let answer = rig.perform(effects.into_iter().next().unwrap());
    let Message::Maintenance(MaintenanceMessage::Previewed { result, .. }) = answer else {
        panic!("the answer of the preview");
    };
    assert!(result.is_none(), "a stopped count has no result");
}

#[test]
fn a_finished_preview_clears_the_flag_of_the_view() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "g", &six());
    delete_two(&mut rig, slot);
    rig.send(MaintenanceMessage::Start(compact()));
    assert!(!rig.state.maintenance_view().previewing);
    assert!(rig.state.maintenance_view().confirm.is_some());
}

fn play_a_move(rig: &mut Rig) {
    rig.send(GameMessage::Navigate(Nav::End));
    let mv = rig.state.session.position().parse_uci("f1b5").expect("uci");
    rig.send(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
}

#[test]
fn giving_up_the_question_about_the_altered_game_forgets_the_operation() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "g", &six());
    delete_two(&mut rig, slot);
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    play_a_move(&mut rig);
    rig.send(MaintenanceMessage::Start(compact()));
    rig.send(MaintenanceMessage::Confirm(true));
    assert!(rig.state.maintenance_waits_for_save(), "the question is on");
    rig.send(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(!rig.state.maintenance_waits_for_save());
    assert_eq!(rig.state.maintenance_job(), None);
}

#[test]
fn the_repair_looks_at_the_files_open_when_it_starts() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let path = make_si4(dir.path(), "r", &six());
    rig.ask(MaintRequest::Repair {
        path: path.clone(),
        drop_unreadable: false,
    });
    assert!(rig.state.maintenance_view().confirm.is_some());
    // The file is opened while the question is on screen.
    rig.send(BasesMessage::Open(path));
    rig.send(MaintenanceMessage::Confirm(true));
    assert_eq!(
        rig.state.maintenance_job(),
        None,
        "no repair of an open file"
    );
    assert_eq!(rig.maint_notice(), Some(MaintNotice::RepairOpen));
}
