//! M7-6d (GUI, spec M7-6, criterion 16b): the game browser offers "Merge"
//! with the same enabling rule as the other entries (the game of the editor
//! has moves).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m7_1c_common;

use m7_1c_common::*;
use mychess_app::{BrowseMessage, GameMessage, Nav};

fn browse_game_two(m: &mut Maint) -> mychess_app::SlotId {
    let slot = m.app().bases.current();
    m.rig.app_msg(AppMessage::Bases(BasesMessage::Browse(
        BrowseMessage::Open {
            slot,
            numbers: vec![2],
        },
    )));
    slot
}

#[test]
fn the_browser_merge_button_opens_the_merge_of_the_browsed_game() {
    let mut m = Maint::six();
    let slot = m.app().bases.current();
    // The editor holds game 1, which has moves.
    m.rig.app_msg(BasesMessage::LoadGame { slot, number: 1 });
    m.rig
        .app_msg(GameMessage::Navigate(Nav::GoToMainlinePly(2)));
    assert_eq!(browse_game_two(&mut m), slot);
    let mut sim = m.sim();
    sim.click("Merge").expect("the button is in the browser");
    let sent = format!("{:?}", sim.into_messages().collect::<Vec<_>>());
    assert!(
        sent.contains("Merge") && sent.contains("number: 2"),
        "{sent}"
    );
}

#[test]
fn the_browser_merge_button_does_nothing_when_the_editor_has_no_moves() {
    let mut m = Maint::six();
    browse_game_two(&mut m);
    let mut sim = m.sim();
    sim.click("Merge").expect("the button is in the browser");
    assert_eq!(sim.into_messages().count(), 0);
}
