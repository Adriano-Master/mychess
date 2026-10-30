//! Regression tests of the second review round of M6-3b (spec
//! `m6-3-jogadores-torneios-spelling.md`, D3494 onwards): a cancelled quit,
//! Add Elo in a read-only base, dialogs opened over running work, the
//! statistics build that a write waits for, and the small fixes of the
//! name checker and of the tournament finder.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

mod m6_3b_common;

use m6_3b_common::*;
use mychess_app::{
    BasesMessage, CopyScope, DiscardChoice, GameMessage, ListMessage, MaskEdit, MaskMessage,
    MaskScope, MaskTarget, NameKind, Nav, PrefChange, PrefsMessage, SpellingEffect,
    TournamentAction, TournamentField, TreeMessage,
};

fn games(event: &'static str, a: &'static str, b: &'static str) -> Vec<G> {
    vec![g(event, "Rio BRA", "2020.01.10", a, b, "1-0", 0, 0)]
}

fn misspelled() -> Vec<G> {
    let mut all = games("Aberto", "Alfa, Alf", "Beta, B");
    all.extend(games("Aberto", "Beta, B", "Alfa, Alf"));
    all.extend(games("Aberto Brasileiro", "Alfa, A", "Delt, D"));
    all
}

fn unrated() -> Vec<G> {
    games("Aberto", "Alfa, A", "Beta, B")
}

fn spelling_effects(fx: &[Effect]) -> Vec<&SpellingEffect> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Spelling(s) => Some(s),
            _ => None,
        })
        .collect()
}

fn has_scan(fx: &[Effect]) -> bool {
    spelling_effects(fx)
        .iter()
        .any(|e| matches!(e, SpellingEffect::Scan { .. }))
}

fn has_load(fx: &[Effect]) -> bool {
    spelling_effects(fx)
        .iter()
        .any(|e| matches!(e, SpellingEffect::Load { .. }))
}

fn dirty(rig: &mut Rig, slot: SlotId) {
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    rig.send(GameMessage::Navigate(Nav::GameEnd));
    rig.send(GameMessage::EnterMoveText("Bc4".to_owned()));
    assert!(rig.state.is_dirty(), "the game was edited");
}

/// Like `Rig::send`, but the searches of the finders are not run: the rows on
/// screen stay the ones of the database as it was (they wait for a debounce).
fn send_without_finders(rig: &mut Rig, message: impl Into<Message>) {
    let mut queue = vec![message.into()];
    while let Some(message) = queue.pop() {
        for effect in update(&mut rig.state, message) {
            let skip = effect.runs_on_host()
                || matches!(
                    effect,
                    Effect::Finder(_)
                        | Effect::ScheduleAutoplayTick { .. }
                        | Effect::SavePreferences { .. }
                );
            if !skip {
                queue.push(rig.perform(effect));
            }
        }
    }
}

// ---- B1: a cancelled quit ----

#[test]
fn cancelling_the_quit_lets_the_spelling_file_load_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    dirty(&mut rig, slot);
    rig.raw(Message::Quit);
    assert!(rig.state.discard_prompt.is_some(), "unsaved changes asked");
    // While the question is open nothing starts.
    assert!(!has_load(&rig.raw(SpellingMessage::Reload)));
    rig.raw(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(has_load(&rig.raw(SpellingMessage::Reload)), "Reload works");
}

#[test]
fn cancelling_the_quit_restarts_the_load_that_the_quit_cut() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    dirty(&mut rig, slot);
    let path = rig.data.path().join("spelling.ssp");
    std::fs::write(&path, SSP).unwrap();
    let first = rig.raw(SpellingMessage::Load(path));
    rig.raw(Message::Quit);
    let fx = rig.raw(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(has_load(&fx), "the load starts again: {fx:?}");
    // The answer of the cut load is dropped; the new one counts.
    let old = rig.perform(first[0].clone());
    rig.raw(old);
    assert!(!rig.state.spelling_status_view().loaded);
    let new = fx
        .into_iter()
        .find(|e| matches!(e, Effect::Spelling(SpellingEffect::Load { .. })))
        .unwrap();
    let answer = rig.perform(new);
    rig.raw(answer);
    assert!(rig.state.spelling_status_view().loaded);
}

fn has_effect(fx: &[Effect], wanted: fn(&SpellingEffect) -> bool) -> bool {
    spelling_effects(fx).into_iter().any(wanted)
}

fn has_stats(fx: &[Effect]) -> bool {
    has_effect(fx, |e| matches!(e, SpellingEffect::Stats { .. }))
}

fn has_photos(fx: &[Effect]) -> bool {
    has_effect(fx, |e| matches!(e, SpellingEffect::Photos { .. }))
}

fn load_path(fx: &[Effect]) -> Option<std::path::PathBuf> {
    spelling_effects(fx).into_iter().find_map(|e| match e {
        SpellingEffect::Load { path, .. } => path.clone(),
        _ => None,
    })
}

/// Loads the spelling file but leaves its follow-up work (photos and the
/// statistics) in flight: the effects are returned, not performed.
fn load_with_work_in_flight(rig: &mut Rig) -> Vec<Effect> {
    let path = rig.data.path().join("spelling.ssp");
    std::fs::write(&path, SSP).unwrap();
    let fx = rig.raw(SpellingMessage::Load(path));
    let loaded = rig.perform(fx[0].clone());
    let fx = rig.raw(loaded);
    assert!(has_stats(&fx) && has_photos(&fx), "work starts: {fx:?}");
    fx
}

/// The build the quit cut still holds the files: the new one starts when the
/// thread answers (D3510).
fn assert_stats_restart_after_old_answer(rig: &mut Rig, work: &[Effect], undone: &[Effect]) {
    assert!(!has_stats(undone), "waits for the old thread: {undone:?}");
    let old = work
        .iter()
        .find(|e| matches!(e, Effect::Spelling(SpellingEffect::Stats { .. })))
        .cloned()
        .expect("the build in flight");
    let answer = rig.perform(old);
    let fx = rig.raw(answer);
    assert_eq!(
        count_effects(&fx, |e| matches!(e, SpellingEffect::Stats { .. })),
        1,
        "{fx:?}"
    );
}

fn pick_request(fx: &[Effect]) -> mychess_app::RequestId {
    fx.iter()
        .find_map(|e| match e {
            Effect::PickFile(spec) => Some(spec.request),
            _ => None,
        })
        .expect("a file dialog")
}

/// The quit is asked and "Save" is chosen: the file dialog is out.
fn quit_and_choose_save(rig: &mut Rig) -> mychess_app::RequestId {
    rig.raw(Message::Quit);
    assert!(rig.state.discard_prompt.is_some(), "unsaved changes asked");
    pick_request(&rig.raw(Message::AnswerDiscard(DiscardChoice::Save)))
}

#[test]
fn cancelling_the_quit_scans_again_and_computes_the_statistics_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    dirty(&mut rig, slot);
    let work = load_with_work_in_flight(&mut rig);
    // The scan is in flight (its answer is not delivered) when the quit cuts
    // it.
    let scan = rig.raw(SpellingMessage::OpenCheck(NameKind::Player));
    assert!(has_scan(&scan));
    rig.raw(Message::Quit);
    let fx = rig.raw(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(has_scan(&fx), "the scan starts again by itself: {fx:?}");
    assert_stats_restart_after_old_answer(&mut rig, &work, &fx);
}

#[test]
fn cancelling_the_quit_scans_again_when_the_scan_ended_interrupted() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    dirty(&mut rig, slot);
    rig.load_ssp(SSP);
    let scan = rig.raw(SpellingMessage::OpenCheck(NameKind::Player));
    let scan = scan
        .into_iter()
        .find(|e| matches!(e, Effect::Spelling(SpellingEffect::Scan { .. })))
        .unwrap();
    rig.raw(Message::Quit);
    // The thread ends at once, cut: the answer comes before the question is
    // answered.
    let answer = rig.perform(scan);
    rig.raw(answer);
    let fx = rig.raw(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(has_scan(&fx), "{fx:?}");
}

#[test]
fn cancelling_the_quit_restarts_a_load_whose_cut_answer_came_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    dirty(&mut rig, slot);
    let path = rig.data.path().join("spelling.ssp");
    std::fs::write(&path, SSP).unwrap();
    let load = rig.raw(SpellingMessage::Load(path.clone()));
    rig.raw(Message::Quit);
    // Real order: the thread answers `Cancelled` in milliseconds, then the
    // user clicks Cancel.
    let answer = rig.perform(load[0].clone());
    rig.raw(answer);
    assert!(!rig.state.spelling_status_view().loaded);
    let fx = rig.raw(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert_eq!(load_path(&fx), Some(path.clone()), "{fx:?}");
    // It was a load the user asked for: the file is remembered when it ends.
    let new = fx
        .into_iter()
        .find(|e| matches!(e, Effect::Spelling(SpellingEffect::Load { .. })))
        .unwrap();
    let answer = rig.perform(new);
    rig.raw(answer);
    let status = rig.state.spelling_status_view();
    assert!(status.loaded);
    assert_eq!(status.path, Some(path));
}

#[test]
fn cancelling_the_quit_starts_the_photos_of_a_load_that_ended_during_the_quit() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    dirty(&mut rig, slot);
    let path = rig.data.path().join("spelling.ssp");
    std::fs::write(&path, SSP).unwrap();
    let load = rig.raw(SpellingMessage::Load(path));
    // The thread ended well just before the quit; its answer is delivered
    // after it.
    let answer = rig.perform(load[0].clone());
    rig.raw(Message::Quit);
    let during = rig.raw(answer);
    assert!(!has_photos(&during), "nothing starts while quitting");
    let fx = rig.raw(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(has_photos(&fx), "{fx:?}");
}

#[test]
fn cancelling_the_quit_restarts_the_photos_whose_cut_answer_came_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    dirty(&mut rig, slot);
    let fx = load_with_work_in_flight(&mut rig);
    let photos = fx
        .into_iter()
        .find(|e| matches!(e, Effect::Spelling(SpellingEffect::Photos { .. })))
        .unwrap();
    rig.raw(Message::Quit);
    let answer = rig.perform(photos);
    rig.raw(answer);
    let fx = rig.raw(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(has_photos(&fx), "{fx:?}");
}

#[test]
fn a_load_cancelled_by_the_user_does_not_start_after_a_cancelled_quit() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    dirty(&mut rig, slot);
    let path = rig.data.path().join("spelling.ssp");
    std::fs::write(&path, SSP).unwrap();
    let load = rig.raw(SpellingMessage::Load(path));
    rig.raw(SpellingMessage::CancelLoad);
    rig.raw(Message::Quit);
    let answer = rig.perform(load[0].clone());
    rig.raw(answer);
    let fx = rig.raw(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(!has_load(&fx), "the user had cancelled it: {fx:?}");
    assert!(!rig.state.spelling_status_view().loaded);
}

#[test]
fn a_load_cancelled_by_the_user_before_the_answer_does_not_start_after_a_cancelled_quit() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    dirty(&mut rig, slot);
    let path = rig.data.path().join("spelling.ssp");
    std::fs::write(&path, SSP).unwrap();
    rig.raw(SpellingMessage::Load(path));
    rig.raw(Message::Quit);
    rig.raw(SpellingMessage::CancelLoad);
    let fx = rig.raw(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(!has_load(&fx), "{fx:?}");
}

// ---- B1: the other ways to give the quit up ----

#[test]
fn cancelling_the_file_dialog_of_save_gives_the_quit_up() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    dirty(&mut rig, slot);
    let work = load_with_work_in_flight(&mut rig);
    let request = quit_and_choose_save(&mut rig);
    let fx = rig.raw(Message::FilePicked(request, None));
    assert_stats_restart_after_old_answer(&mut rig, &work, &fx);
    assert!(has_photos(&fx), "{fx:?}");
}

#[test]
fn a_failed_save_gives_the_quit_up() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    dirty(&mut rig, slot);
    let work = load_with_work_in_flight(&mut rig);
    let request = quit_and_choose_save(&mut rig);
    // The parent of the file is a file: the write fails.
    let parent = dir.path().join("not_a_dir");
    std::fs::write(&parent, "x").unwrap();
    let fx = rig.raw(Message::FilePicked(request, Some(parent.join("out.pgn"))));
    let save = fx
        .into_iter()
        .find(|e| matches!(e, Effect::SavePgn { .. }))
        .expect("the save is asked");
    let answer = rig.perform(save);
    let fx = rig.raw(answer);
    assert!(rig.state.notice.is_some(), "the failure is told");
    assert_stats_restart_after_old_answer(&mut rig, &work, &fx);
}

#[test]
fn cancelling_the_question_of_the_mask_gives_the_quit_up() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.send(BasesMessage::Select(slot));
    let work = load_with_work_in_flight(&mut rig);
    rig.send(TreeMessage::Mask(MaskMessage::New));
    rig.send(TreeMessage::Mask(MaskMessage::Edit {
        target: MaskTarget::San("e4".to_owned()),
        edit: MaskEdit::Add,
        scope: MaskScope::One,
    }));
    rig.raw(Message::Quit);
    assert!(rig.state.mask_prompt().is_some(), "the mask is asked about");
    let fx = rig.raw(TreeMessage::Mask(MaskMessage::Answer(
        DiscardChoice::Cancel,
    )));
    assert_stats_restart_after_old_answer(&mut rig, &work, &fx);
}

// ---- B1: the write that waited for the statistics ----

#[test]
fn a_write_deferred_during_a_cancelled_quit_runs_when_the_quit_is_given_up() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    dirty(&mut rig, slot);
    let fx = load_with_work_in_flight(&mut rig);
    let stats = fx
        .into_iter()
        .find(|e| matches!(e, Effect::Spelling(SpellingEffect::Stats { .. })))
        .unwrap();
    let scan = rig.raw(SpellingMessage::OpenCheck(NameKind::Player));
    let scan = scan
        .into_iter()
        .find(|e| matches!(e, Effect::Spelling(SpellingEffect::Scan { .. })))
        .unwrap();
    let answer = rig.perform(scan);
    rig.raw(answer);
    rig.raw(SpellingMessage::Apply);
    let fx = rig.raw(SpellingMessage::ConfirmApply(true));
    assert!(spelling_effects(&fx).is_empty(), "deferred: {fx:?}");
    // The quit comes and the build ends (cut) while the question is open.
    rig.raw(Message::Quit);
    let answer = rig.perform(stats);
    let during = rig.raw(answer);
    assert!(spelling_effects(&during).is_empty(), "{during:?}");
    let fx = rig.raw(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(
        has_effect(&fx, |e| matches!(e, SpellingEffect::ApplyNames { .. })),
        "the write is not lost: {fx:?}"
    );
}

#[test]
fn opening_add_elo_keeps_the_deferred_write_of_the_name_checker() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    let fx = load_with_work_in_flight(&mut rig);
    let stats = fx
        .into_iter()
        .find(|e| matches!(e, Effect::Spelling(SpellingEffect::Stats { .. })))
        .unwrap();
    let scan = rig.raw(SpellingMessage::OpenCheck(NameKind::Player));
    let scan = scan
        .into_iter()
        .find(|e| matches!(e, Effect::Spelling(SpellingEffect::Scan { .. })))
        .unwrap();
    let answer = rig.perform(scan);
    rig.raw(answer);
    rig.raw(SpellingMessage::Apply);
    rig.raw(SpellingMessage::ConfirmApply(true));
    // Another dialog opens: the write of the checker still waits for its
    // build.
    rig.raw(SpellingMessage::OpenAddElo);
    let answer = rig.perform(stats);
    let fx = rig.raw(answer);
    assert!(
        has_effect(&fx, |e| matches!(e, SpellingEffect::ApplyNames { .. })),
        "{fx:?}"
    );
}

// ---- I2: Add Elo in a read-only base ----

#[test]
fn planning_add_elo_in_a_read_only_base_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "p.pgn", &unrated());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenAddElo);
    let fx = rig.raw(SpellingMessage::PlanElo);
    assert!(spelling_effects(&fx).is_empty(), "{fx:?}");
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::ReadOnly));
}

#[test]
fn planning_add_elo_in_a_base_made_read_only_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &unrated());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenAddElo);
    rig.send(Message::Bases(BasesMessage::List(
        ListMessage::SetReadOnly {
            slot,
            read_only: true,
        },
    )));
    let fx = rig.raw(SpellingMessage::PlanElo);
    assert!(spelling_effects(&fx).is_empty(), "{fx:?}");
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::ReadOnly));
}

// ---- I3: a dialog opened over running work ----

#[test]
fn reopening_the_checker_stops_the_scan_of_the_old_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    let first = rig.raw(SpellingMessage::OpenCheck(NameKind::Player));
    let Some(SpellingEffect::Scan { control, .. }) = spelling_effects(&first).first().copied()
    else {
        panic!("scan: {first:?}");
    };
    let control = control.clone();
    assert!(!control.is_cancelled());
    rig.raw(SpellingMessage::OpenCheck(NameKind::Event));
    assert!(control.is_cancelled(), "the old scan stops");
}

#[test]
fn reopening_add_elo_stops_the_plan_of_the_old_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &unrated());
    rig.load_ssp(SSP);
    rig.raw(SpellingMessage::OpenAddElo);
    let first = rig.raw(SpellingMessage::PlanElo);
    let Some(SpellingEffect::PlanElo { control, .. }) = spelling_effects(&first).first().copied()
    else {
        panic!("plan: {first:?}");
    };
    let control = control.clone();
    rig.raw(SpellingMessage::OpenAddElo);
    assert!(control.is_cancelled(), "the old plan stops");
}

#[test]
fn opening_add_elo_again_during_its_write_keeps_the_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &unrated());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenAddElo);
    rig.send(SpellingMessage::SetEloOnlyFilter(false));
    rig.send(SpellingMessage::PlanElo);
    let fx = rig.raw(SpellingMessage::ConfirmElo);
    assert_eq!(fx.len(), 1);
    assert!(rig.state.add_elo_flags().unwrap().writing);
    rig.raw(SpellingMessage::OpenAddElo);
    assert!(
        rig.state.add_elo_flags().unwrap().writing,
        "the dialog that writes stays"
    );
}

#[test]
fn opening_the_checker_again_during_its_write_keeps_the_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.raw(SpellingMessage::Apply);
    let fx = rig.raw(SpellingMessage::ConfirmApply(true));
    assert_eq!(fx.len(), 1);
    assert!(rig.state.spell_flags().unwrap().writing);
    let more = rig.raw(SpellingMessage::OpenCheck(NameKind::Event));
    assert!(spelling_effects(&more).is_empty(), "{more:?}");
    assert!(rig.state.spell_flags().unwrap().writing);
}

// ---- I4: the statistics build the write waits for ----

#[test]
fn a_write_waits_for_the_statistics_build_it_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    let path = rig.data.path().join("spelling.ssp");
    std::fs::write(&path, SSP).unwrap();
    let fx = rig.raw(SpellingMessage::Load(path));
    let loaded = rig.perform(fx[0].clone());
    let fx = rig.raw(loaded);
    let stats = fx
        .into_iter()
        .find(|e| matches!(e, Effect::Spelling(SpellingEffect::Stats { .. })))
        .expect("the build starts");
    let Effect::Spelling(SpellingEffect::Stats { control, .. }) = &stats else {
        unreachable!();
    };
    let control = control.clone();
    // The checker, scanned while the build still runs.
    let fx = rig.raw(SpellingMessage::OpenCheck(NameKind::Player));
    let scan = fx
        .into_iter()
        .find(|e| matches!(e, Effect::Spelling(SpellingEffect::Scan { .. })))
        .expect("scan");
    let answer = rig.perform(scan);
    rig.raw(answer);
    rig.raw(SpellingMessage::Apply);
    let fx = rig.raw(SpellingMessage::ConfirmApply(true));
    assert!(control.is_cancelled(), "the build is told to stop");
    assert!(
        spelling_effects(&fx).is_empty(),
        "the write does not start while the build runs: {fx:?}"
    );
    // The build ends: the write starts.
    let answer = rig.perform(stats);
    let fx = rig.raw(answer);
    assert!(
        spelling_effects(&fx)
            .iter()
            .any(|e| matches!(e, SpellingEffect::ApplyNames { .. })),
        "{fx:?}"
    );
}

// ---- M2: a stale answer keeps the kind of the write in flight ----

#[test]
fn a_stale_answer_does_not_change_the_kind_of_the_write_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.raw(SpellingMessage::Apply);
    let fx = rig.raw(SpellingMessage::ConfirmApply(true));
    let first = rig.perform(fx[0].clone());
    // The follow-up work (statistics, reading the game) runs to its end.
    rig.send(first.clone());
    // The events are corrected next.
    rig.send(SpellingMessage::OpenCheck(NameKind::Event));
    rig.raw(SpellingMessage::Apply);
    let fx = rig.raw(SpellingMessage::ConfirmApply(true));
    assert_eq!(fx.len(), 1, "{fx:?} {:?}", rig.spelling_notice());
    // The first answer comes again, late.
    rig.raw(first);
    let second = rig.perform(fx[0].clone());
    rig.raw(second);
    assert!(matches!(
        rig.spelling_notice(),
        Some(SpellingNotice::Applied {
            kind: NameKind::Event,
            ..
        })
    ));
}

// ---- M3: options changed during a write ----

#[test]
fn options_changed_during_a_write_make_the_checker_scan_when_it_ends() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.raw(SpellingMessage::Apply);
    let fx = rig.raw(SpellingMessage::ConfirmApply(true));
    let during = rig.raw(SpellingMessage::SetSurnames(true));
    assert!(!has_scan(&during), "no scan while the file is written");
    let answer = rig.perform(fx[0].clone());
    let after = rig.raw(answer);
    assert!(has_scan(&after), "{after:?}");
}

// ---- M5: the tournament finder after the Clipbase grew ----

#[test]
fn a_filter_link_after_the_clipbase_grew_is_stale() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::ConfirmCopy(
        false,
    ))));
    let (b, _) = rig.open_si4(dir.path(), "b", &games("Outro", "Zeta, Z", "Eta, E"));
    let (a, _) = rig.open_si4(dir.path(), "a", &misspelled());
    let copy = |rig: &mut Rig, from| {
        send_without_finders(
            rig,
            BasesMessage::List(ListMessage::CopyGames {
                from,
                to: SlotId::CLIPBASE,
                scope: CopyScope::Filter,
            }),
        );
    };
    copy(&mut rig, a);
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    rig.send(FinderMessage::OpenTournaments { player: None });
    for field in [TournamentField::PlayersMin, TournamentField::GamesMin] {
        rig.send(FinderMessage::SetTournamentField(field, "1".into()));
    }
    rig.send(FinderMessage::UpdateTournaments);
    assert!(
        rig.state
            .tournament_finder_view()
            .and_then(|v| v.rows)
            .is_some_and(|r| !r.is_empty()),
        "the tournaments of the Clipbase"
    );
    // Another database is copied into the Clipbase: same source, more games.
    copy(&mut rig, b);
    let before = rig
        .state
        .bases
        .slot(SlotId::CLIPBASE)
        .unwrap()
        .filter()
        .count();
    rig.raw(FinderMessage::ClickTournament {
        index: 0,
        action: TournamentAction::ShowInFilter,
    });
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Finder(FinderNotice::Stale))
    );
    assert_eq!(
        rig.state
            .bases
            .slot(SlotId::CLIPBASE)
            .unwrap()
            .filter()
            .count(),
        before
    );
}

// ---- 3rd review of B1: more ways to give the quit up (D3509 onwards) ----

fn count_effects(fx: &[Effect], wanted: fn(&SpellingEffect) -> bool) -> usize {
    spelling_effects(fx)
        .into_iter()
        .filter(|e| wanted(e))
        .count()
}

fn count_loads(fx: &[Effect]) -> usize {
    count_effects(fx, |e| matches!(e, SpellingEffect::Load { .. }))
}

fn count_photos(fx: &[Effect]) -> usize {
    count_effects(fx, |e| matches!(e, SpellingEffect::Photos { .. }))
}

fn count_writes(fx: &[Effect]) -> usize {
    count_effects(fx, |e| matches!(e, SpellingEffect::ApplyNames { .. }))
}

#[test]
fn a_spelling_command_after_a_quit_abandoned_without_a_call_works_again() {
    // The net alone: the question is dropped without telling the spelling flow.
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    dirty(&mut rig, slot);
    rig.raw(Message::Quit);
    rig.state.discard_prompt = None;
    let fx = rig.raw(SpellingMessage::Reload);
    assert_eq!(count_loads(&fx), 1, "Reload loads: {fx:?}");
}

#[test]
fn a_spelling_command_does_not_undo_a_quit_waiting_for_the_question() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    dirty(&mut rig, slot);
    rig.raw(Message::Quit);
    for _ in 0..2 {
        let fx = rig.raw(SpellingMessage::Reload);
        assert!(!has_load(&fx), "the quit goes on: {fx:?}");
    }
}

#[test]
fn a_spelling_command_does_not_undo_a_quit_waiting_for_the_preferences() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.state = AppState::with_preferences_path(Some(dir.path().join("prefs.toml")));
    rig.state.open_pgn_in_slots = true;
    rig.state
        .set_player_data_dir(Some(rig.data.path().to_path_buf()));
    let (_slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.raw(Message::PreferencesLoaded(
        mychess_app::LoadedPreferences::default(),
    ));
    // The save of the preferences is in flight (nobody answers it), so the
    // quit waits for it before the exit.
    rig.raw(Message::Quit);
    let fx = rig.raw(SpellingMessage::Reload);
    assert!(!has_load(&fx), "the quit goes on: {fx:?}");
}

#[test]
fn the_cancel_of_the_quit_restarts_the_load_exactly_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    dirty(&mut rig, slot);
    let path = rig.data.path().join("spelling.ssp");
    std::fs::write(&path, SSP).unwrap();
    let load = rig.raw(SpellingMessage::Load(path));
    rig.raw(Message::Quit);
    let answer = rig.perform(load[0].clone());
    rig.raw(answer);
    let fx = rig.raw(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert_eq!(count_loads(&fx), 1, "{fx:?}");
    assert_eq!(count_photos(&fx), 0, "the photos follow the load: {fx:?}");
}

#[test]
fn the_cancel_of_the_quit_restarts_the_photos_exactly_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    dirty(&mut rig, slot);
    let path = rig.data.path().join("spelling.ssp");
    std::fs::write(&path, SSP).unwrap();
    let load = rig.raw(SpellingMessage::Load(path));
    let answer = rig.perform(load[0].clone());
    rig.raw(Message::Quit);
    rig.raw(answer);
    let fx = rig.raw(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert_eq!(count_photos(&fx), 1, "{fx:?}");
    assert_eq!(count_loads(&fx), 0, "{fx:?}");
}

#[test]
fn the_write_deferred_during_a_quit_runs_once_even_if_the_old_build_answers_late() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    dirty(&mut rig, slot);
    let fx = load_with_work_in_flight(&mut rig);
    let stats = fx
        .into_iter()
        .find(|e| matches!(e, Effect::Spelling(SpellingEffect::Stats { .. })))
        .unwrap();
    let scan = rig.raw(SpellingMessage::OpenCheck(NameKind::Player));
    let scan = scan
        .into_iter()
        .find(|e| matches!(e, Effect::Spelling(SpellingEffect::Scan { .. })))
        .unwrap();
    let answer = rig.perform(scan);
    rig.raw(answer);
    rig.raw(SpellingMessage::Apply);
    rig.raw(SpellingMessage::ConfirmApply(true));
    rig.raw(Message::Quit);
    // The build is still running when the quit is given up: the write waits
    // for it to let go of the files (D3510).
    let fx = rig.raw(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert_eq!(count_writes(&fx), 0, "the build holds the files: {fx:?}");
    let answer = rig.perform(stats);
    let fx = rig.raw(answer);
    assert_eq!(count_writes(&fx), 1, "the write runs now: {fx:?}");
    // Nothing runs it a second time.
    let fx = rig.raw(SpellingMessage::Reload);
    assert_eq!(count_writes(&fx), 0, "{fx:?}");
}

#[test]
fn the_deferred_write_is_revalidated_when_its_slot_was_closed_meanwhile() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (target, _) = rig.open_si4(dir.path(), "a", &misspelled());
    load_with_work_in_flight(&mut rig);
    let scan = rig.raw(SpellingMessage::OpenCheck(NameKind::Player));
    let scan = scan
        .into_iter()
        .find(|e| matches!(e, Effect::Spelling(SpellingEffect::Scan { .. })))
        .unwrap();
    let answer = rig.perform(scan);
    rig.raw(answer);
    rig.raw(SpellingMessage::Apply);
    rig.raw(SpellingMessage::ConfirmApply(true));
    let (other, _) = rig.open_si4(dir.path(), "b", &misspelled());
    dirty(&mut rig, other);
    rig.raw(Message::Quit);
    assert!(rig.state.discard_prompt.is_some(), "the quit asks");
    // Closing the slot of the checker drops the question and gives the quit
    // up: the deferred write finds its slot gone and does not start.
    let fx = rig.raw(BasesMessage::Close(target));
    assert_eq!(count_writes(&fx), 0, "{fx:?}");
}
