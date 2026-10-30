//! M5-2a second review: the user changes the filter of the slot while an
//! "adjust" search flies (D990).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m5_2_common;

use m5_2_common::*;
use tempfile::tempdir;

#[test]
fn a_user_filter_made_while_an_adjust_search_flies_survives_the_answer_and_the_close() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let slot = rig.state.bases.current();
    rig.play("e2e4");
    // Adjust on: the search is sent over the whole base, not run yet.
    let effects = rig.raw(TreeMessage::SetAdjust(true));
    let mut sent = jobs(effects);
    assert_eq!(sent.len(), 1, "one search is in flight");
    let job = sent.remove(0);
    // The user filters the list while it flies.
    rig.keep(slot, &[2, 4]);
    assert_eq!(rig.filter_numbers(slot), [2, 4], "precondition");
    // The old answer (adjusted over all five games) arrives.
    let answer = rig.run_job(job);
    let more = rig.raw(answer);
    assert_eq!(
        rig.filter_numbers(slot),
        [2, 4],
        "the stale adjustment is not applied"
    );
    // The search is done again over the user's filter.
    let mut queue: Vec<Effect> = more;
    while let Some(effect) = queue.pop() {
        if effect.runs_on_host() {
            continue;
        }
        let message = rig.perform(effect);
        queue.extend(rig.raw(message));
    }
    // Games 2 and 4 have no 1.e4: the adjusted filter is empty.
    assert_eq!(rig.filter_numbers(slot), Vec::<u32>::new());
    rig.send(TreeMessage::Close);
    assert_eq!(
        rig.filter_numbers(slot),
        [2, 4],
        "the user's filter is what comes back"
    );
}
