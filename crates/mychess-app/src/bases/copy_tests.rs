//! Sanity tests of the copy between databases (slice M3-4d; D576 onwards).
//! The full contract suite is the tester's.
#![allow(clippy::large_futures)] // the test rig owns the whole `AppState` across awaits

use mychess_db::GameSource as _;
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::jobs::{COPY_BATCH, run_copy};
use super::{
    BaseError, BasesMessage, BasesNotice, CopyEnd, CopyRefusal, CopyScope, ListMessage,
    OpenControl, PickMode, SharedSource, SlotId,
};
use crate::effect::{Effect, perform};
use crate::flow::AppNotice;
use crate::message::Message;
use crate::prefs::PrefChange;
use crate::state::{AppState, update};

fn pgn(prefix: &str, count: usize) -> String {
    let mut text = String::new();
    for n in 1..=count {
        let _ = write!(
            text,
            "[Event \"E{n}\"]\n[White \"{prefix}{n}\"]\n[Black \"B{n}\"]\n[Result \"*\"]\n\n1. e4 e5 *\n\n"
        );
    }
    text
}

fn write_file(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write");
    path
}

fn make_si4(dir: &Path, name: &str, text: &str) -> PathBuf {
    use mychess_db::si4::Si4Writer;
    use mychess_db::{CancelToken, ImportOptions, NoProgress, import_pgn};
    let base = dir.join(name);
    let mut writer = Si4Writer::create(&base, 0).expect("create");
    import_pgn(
        text.as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .expect("import");
    drop(writer);
    base.with_extension("si4")
}

#[allow(clippy::field_reassign_with_default)] // private fields forbid the struct update
fn state() -> AppState {
    let mut state = AppState::default();
    state.open_pgn_in_slots = true;
    state
}

async fn drive(state: &mut AppState, message: impl Into<Message>) {
    let mut queue: VecDeque<Effect> = update(state, message.into()).into();
    while let Some(effect) = queue.pop_front() {
        if effect.runs_on_host() {
            continue;
        }
        let answer = perform(effect).await;
        queue.extend(update(state, answer));
    }
}

fn list(message: ListMessage) -> Message {
    Message::Bases(BasesMessage::List(message))
}

fn copy(from: SlotId, to: SlotId, scope: CopyScope) -> Message {
    list(ListMessage::CopyGames { from, to, scope })
}

/// A PGN of 5 games in slot 1 and an `.si4` of 3 in slot 2.
async fn two_bases() -> (AppState, SlotId, SlotId, PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("dir");
    let source = write_file(dir.path(), "src.pgn", &pgn("S", 5));
    let target = make_si4(dir.path(), "dst", &pgn("T", 3));
    let mut state = state();
    state.prefs.editing.confirm_copy = false;
    drive(&mut state, BasesMessage::Open(source)).await;
    let from = state.bases.current();
    drive(&mut state, BasesMessage::Open(target.clone())).await;
    let to = state.bases.current();
    assert_ne!(from, to);
    (state, from, to, target, dir)
}

fn notice(state: &AppState) -> Option<&BasesNotice> {
    match state.notice.as_ref() {
        Some(AppNotice::Bases(n)) => Some(n),
        _ => None,
    }
}

fn whites(state: &AppState, id: SlotId) -> Vec<String> {
    state
        .bases
        .list_page(id, 1, 1000)
        .iter()
        .map(|r| r.white.clone())
        .collect()
}

fn end(slot: SlotId, name: &str) -> CopyEnd {
    CopyEnd {
        slot,
        name: name.to_owned(),
    }
}

#[tokio::test]
async fn the_filter_is_appended_to_an_si4_target_in_source_order() {
    let (mut state, from, to, _path, _dir) = two_bases().await;
    // Keep games 2, 4 and 5 of the source; give the target a selection and
    // a position that must survive the reload.
    for n in [1, 3] {
        update(
            &mut state,
            list(ListMessage::Pick {
                slot: from,
                number: n,
                mode: PickMode::Only,
            }),
        );
        update(&mut state, list(ListMessage::RemoveSelected { slot: from }));
    }
    update(
        &mut state,
        list(ListMessage::Pick {
            slot: to,
            number: 2,
            mode: PickMode::Only,
        }),
    );
    update(&mut state, list(ListMessage::RemoveSelected { slot: to }));
    update(
        &mut state,
        list(ListMessage::Pick {
            slot: to,
            number: 3,
            mode: PickMode::Only,
        }),
    );
    let current = state.bases.current();
    drive(&mut state, copy(from, to, CopyScope::Filter)).await;
    let slot = state.bases.slot(to).expect("target");
    assert_eq!(
        (slot.total(), slot.filtered()),
        (6, 5),
        "2 left out stays out"
    );
    assert_eq!(
        whites(&state, to),
        ["T1", "T3", "S2", "S4", "S5"],
        "the new games are at the end, in source order"
    );
    assert_eq!(slot.list.selection.iter().copied().collect::<Vec<_>>(), [3]);
    assert_eq!(state.bases.current(), current, "the current slot is kept");
    assert_eq!(
        notice(&state),
        Some(&BasesNotice::FilterCopied {
            count: 3,
            from: end(from, "src.pgn"),
            to: end(to, "dst.si4"),
        })
    );
    assert!(state.list_job().is_none());
    // The source did not change.
    assert_eq!(state.bases.slot(from).expect("source").filtered(), 3);
}

#[tokio::test]
async fn the_file_on_disk_has_the_games() {
    let (mut state, from, to, path, _dir) = two_bases().await;
    drive(&mut state, copy(from, to, CopyScope::Filter)).await;
    let db = mychess_db::si4::Si4Database::open(&path).expect("open");
    assert_eq!(mychess_db::GameSource::len(&db), 8);
}

#[tokio::test]
async fn selected_rows_are_copied_to_the_clipbase_without_asking() {
    let (mut state, from, _to, _path, _dir) = two_bases().await;
    state.prefs.editing.confirm_copy = true;
    for (n, mode) in [(5, PickMode::Only), (2, PickMode::Toggle)] {
        update(
            &mut state,
            list(ListMessage::Pick {
                slot: from,
                number: n,
                mode,
            }),
        );
    }
    drive(
        &mut state,
        copy(from, SlotId::CLIPBASE, CopyScope::Selection),
    )
    .await;
    assert!(state.copy_prompt.is_none(), "the Clipbase never asks");
    assert_eq!(whites(&state, SlotId::CLIPBASE), ["S2", "S5"]);
    let clip = state.bases.slot(SlotId::CLIPBASE).expect("clipbase");
    assert_eq!((clip.total(), clip.filtered()), (2, 2));
}

#[tokio::test]
async fn refusals_follow_the_scid_precedence() {
    let (mut state, from, to, _path, _dir) = two_bases().await;
    let refused = |state: &AppState| match notice(state) {
        Some(BasesNotice::CopyRefused { reason, .. }) => Some(*reason),
        _ => None,
    };
    // Same database (also when the filter is empty: the last test wins).
    update(&mut state, list(ListMessage::NegateFilter { slot: from }));
    drive(&mut state, copy(from, from, CopyScope::Filter)).await;
    assert_eq!(refused(&state), Some(CopyRefusal::SameBase));
    // A target that is not open.
    let free = SlotId::from_number(5).expect("slot");
    drive(&mut state, copy(from, free, CopyScope::Filter)).await;
    assert_eq!(refused(&state), Some(CopyRefusal::TargetNotOpen));
    // A source that is not open beats a target that is not open too.
    drive(&mut state, copy(free, free, CopyScope::Filter)).await;
    assert_eq!(refused(&state), Some(CopyRefusal::SameBase));
    let other = SlotId::from_number(6).expect("slot");
    drive(&mut state, copy(free, other, CopyScope::Filter)).await;
    assert_eq!(refused(&state), Some(CopyRefusal::SourceNotOpen));
    // A PGN target is read-only, and beats an empty filter.
    drive(&mut state, copy(to, from, CopyScope::Filter)).await;
    assert_eq!(refused(&state), Some(CopyRefusal::TargetReadOnly));
    // An empty filter.
    drive(&mut state, copy(from, to, CopyScope::Filter)).await;
    assert_eq!(refused(&state), Some(CopyRefusal::EmptyFilter));
    drive(&mut state, copy(to, SlotId::CLIPBASE, CopyScope::Selection)).await;
    assert_eq!(refused(&state), Some(CopyRefusal::NothingSelected));
    // Nothing was copied anywhere.
    assert_eq!(state.bases.slot(to).expect("target").total(), 3);
    assert_eq!(state.bases.slot(SlotId::CLIPBASE).expect("clip").total(), 0);
    assert!(state.list_job().is_none());
}

#[tokio::test]
async fn an_si4_marked_read_only_is_not_a_target_and_stays_locked_when_the_file_is() {
    let (mut state, from, to, _path, _dir) = two_bases().await;
    update(
        &mut state,
        list(ListMessage::SetReadOnly {
            slot: to,
            read_only: true,
        }),
    );
    drive(&mut state, copy(from, to, CopyScope::Filter)).await;
    assert!(matches!(
        notice(&state),
        Some(BasesNotice::CopyRefused {
            reason: CopyRefusal::TargetReadOnly,
            ..
        })
    ));
    update(
        &mut state,
        list(ListMessage::SetReadOnly {
            slot: to,
            read_only: false,
        }),
    );
    let slot = state.bases.slot(to).expect("target");
    assert!(!slot.read_only && !slot.is_locked_read_only());
    drive(&mut state, copy(from, to, CopyScope::Filter)).await;
    assert_eq!(state.bases.slot(to).expect("target").total(), 8);
}

#[tokio::test]
async fn a_file_without_write_permission_cannot_be_unlocked() {
    let dir = tempfile::tempdir().expect("dir");
    let path = make_si4(dir.path(), "ro", &pgn("R", 2));
    for ext in ["si4", "sn4", "sg4"] {
        let file = path.with_extension(ext);
        let mut permissions = std::fs::metadata(&file).expect("meta").permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file, permissions).expect("chmod");
    }
    let mut state = state();
    drive(&mut state, BasesMessage::Open(path)).await;
    let id = state.bases.current();
    let slot = state.bases.slot(id).expect("slot");
    assert!(slot.read_only && slot.is_locked_read_only());
    update(
        &mut state,
        list(ListMessage::SetReadOnly {
            slot: id,
            read_only: false,
        }),
    );
    assert!(state.bases.slot(id).expect("slot").read_only);
}

#[tokio::test]
async fn the_question_is_asked_for_a_file_and_answered_by_the_user() {
    let (mut state, from, to, _path, _dir) = two_bases().await;
    state.prefs.editing.confirm_copy = true;
    let effects = update(&mut state, copy(from, to, CopyScope::Filter));
    assert!(effects.is_empty(), "nothing runs before the answer");
    let prompt = state.copy_prompt.clone().expect("a question");
    assert_eq!((prompt.count, prompt.scope), (5, CopyScope::Filter));
    assert_eq!(prompt.to, end(to, "dst.si4"));
    assert!(state.modal_open());
    // No: nothing happens.
    drive(&mut state, list(ListMessage::AnswerCopy(false))).await;
    assert!(state.copy_prompt.is_none() && !state.modal_open());
    assert_eq!(state.bases.slot(to).expect("target").total(), 3);
    // Yes: the copy runs.
    drive(&mut state, copy(from, to, CopyScope::Filter)).await;
    drive(&mut state, list(ListMessage::AnswerCopy(true))).await;
    assert_eq!(state.bases.slot(to).expect("target").total(), 8);
}

#[tokio::test]
async fn the_preference_turns_the_question_off_and_is_saved_in_the_file() {
    let (mut state, from, to, _path, _dir) = two_bases().await;
    state.prefs.editing.confirm_copy = true;
    update(
        &mut state,
        Message::Prefs(crate::prefs::PrefsMessage::Set(PrefChange::ConfirmCopy(
            false,
        ))),
    );
    assert!(!state.prefs.editing.confirm_copy);
    let effects = update(&mut state, copy(from, to, CopyScope::Filter));
    assert_eq!(effects.len(), 1);
    assert!(state.copy_prompt.is_none());
    let text = crate::prefs::render_preferences(&state.prefs);
    assert!(text.contains("confirm_copy = false"), "{text}");
    let loaded = crate::prefs::load_preferences(&text);
    assert!(!loaded.prefs.editing.confirm_copy);
    // A file without the key keeps the default (on).
    let old = crate::prefs::load_preferences("[editing]\nask_to_replace_moves = false\n");
    assert!(old.prefs.editing.confirm_copy);
}

#[tokio::test]
async fn the_limit_of_the_clipbase_is_checked_before_any_game_is_read() {
    let (mut state, from, _to, _path, _dir) = two_bases().await;
    state.bases.set_clipbase_limit(4);
    drive(&mut state, copy(from, SlotId::CLIPBASE, CopyScope::Filter)).await;
    assert_eq!(
        notice(&state),
        Some(&BasesNotice::CopyStopped {
            count: 0,
            error: BaseError::ClipbaseLimit { limit: 4 },
            from: end(from, "src.pgn"),
            to: end(SlotId::CLIPBASE, ""),
        })
    );
    assert_eq!(state.bases.slot(SlotId::CLIPBASE).expect("clip").total(), 0);
}

#[tokio::test]
async fn a_game_marked_deleted_is_copied_with_its_mark() {
    use mychess_db::si4::{FlagOp, GameFlags, Si4Writer};
    let dir = tempfile::tempdir().expect("dir");
    let path = make_si4(dir.path(), "d", &pgn("D", 3));
    {
        let mut writer = Si4Writer::open(&path).expect("writer");
        writer
            .set_flag(2, GameFlags::DELETE, FlagOp::Set)
            .expect("flag");
    }
    let mut state = state();
    drive(&mut state, BasesMessage::Open(path)).await;
    let from = state.bases.current();
    drive(&mut state, copy(from, SlotId::CLIPBASE, CopyScope::Filter)).await;
    let rows = state.bases.list_page(SlotId::CLIPBASE, 1, 10);
    let deleted: Vec<bool> = rows.iter().map(|r| r.deleted).collect();
    assert_eq!(deleted, [false, true, false], "Scid copies the entry whole");
}

#[tokio::test]
async fn a_target_changed_behind_the_slot_is_not_written_and_is_reloaded() {
    use mychess_db::si4::{FlagOp, GameFlags, Si4Writer};
    let (mut state, from, to, path, _dir) = two_bases().await;
    {
        let mut writer = Si4Writer::open(&path).expect("writer");
        writer
            .set_flag(3, GameFlags::DELETE, FlagOp::Set)
            .expect("flag");
    }
    drive(&mut state, copy(from, to, CopyScope::Filter)).await;
    assert!(matches!(
        notice(&state),
        Some(BasesNotice::CopyStopped {
            count: 0,
            error: BaseError::Stale,
            ..
        })
    ));
    let slot = state.bases.slot(to).expect("target");
    assert_eq!(slot.total(), 3, "nothing was written");
    assert!(
        state.bases.list_page(to, 1, 5)[2].deleted,
        "but it reloaded"
    );
    // The stamp is fresh: the next copy goes through.
    drive(&mut state, copy(from, to, CopyScope::Filter)).await;
    assert_eq!(state.bases.slot(to).expect("target").total(), 8);
}

#[tokio::test]
async fn closing_the_target_cancels_the_copy_and_other_actions_wait() {
    let (mut state, from, to, _path, _dir) = two_bases().await;
    let started = update(&mut state, copy(from, to, CopyScope::Filter));
    assert_eq!(started.len(), 1);
    assert_eq!(
        state.list_job().map(|(kind, _)| kind),
        Some(super::ListJobKind::CopyGames)
    );
    // One job at a time.
    update(&mut state, list(ListMessage::ResetFilter { slot: from }));
    assert_eq!(notice(&state), Some(&BasesNotice::ListBusy));
    // Nor is a game of the target read meanwhile.
    let load = update(
        &mut state,
        Message::Bases(BasesMessage::LoadGame {
            slot: to,
            number: 1,
        }),
    );
    assert!(load.is_empty());
    // Closing the target cancels the job.
    let control = state.list_job().map(|(_, c)| c.clone()).expect("job");
    let mut effects = update(&mut state, Message::Bases(BasesMessage::Close(to)));
    assert!(effects.is_empty());
    assert!(control.is_cancelled());
    // The thread is still winding down: the databases stay busy until the
    // answer arrives (D588), though nothing is shown for the job.
    assert!(state.list_job().is_none());
    update(&mut state, list(ListMessage::ResetFilter { slot: from }));
    assert_eq!(notice(&state), Some(&BasesNotice::ListBusy));
    let effect = update(&mut state, copy(from, to, CopyScope::Filter));
    assert!(effect.is_empty(), "no second job");
    // The answer frees them. Cancelled before the first game: silent.
    state.notice = None;
    let answer = perform(started.into_iter().next().expect("effect")).await;
    drop(effects.drain(..));
    update(&mut state, answer);
    assert!(state.notice.is_none());
    update(&mut state, list(ListMessage::ResetFilter { slot: from }));
    assert!(state.notice.is_none(), "free again");
}

#[tokio::test]
async fn a_closed_target_that_got_games_ends_with_a_stopped_notice() {
    let (mut state, from, to, _path, _dir) = two_bases().await;
    let effects = update(&mut state, copy(from, to, CopyScope::Filter));
    let effect = effects.into_iter().next().expect("effect");
    // The copy finishes on disk, then the target is closed before the
    // answer is handled.
    let answer = perform(effect).await;
    update(&mut state, Message::Bases(BasesMessage::Close(to)));
    update(&mut state, answer);
    assert!(matches!(
        notice(&state),
        Some(BasesNotice::CopyStopped { count: 5, .. })
    ));
    assert!(state.list_job().is_none());
}

#[tokio::test]
async fn quitting_cancels_the_job_and_waits_for_its_answer_up_to_a_deadline() {
    let (mut state, from, to, _path, _dir) = two_bases().await;
    let effects = update(&mut state, copy(from, to, CopyScope::Filter));
    let effect = effects.into_iter().next().expect("effect");
    let control = state.list_job().map(|(_, c)| c.clone()).expect("job");
    let out = update(&mut state, Message::Quit);
    assert!(control.is_cancelled());
    assert!(!out.iter().any(|e| matches!(e, Effect::Exit)), "waits");
    assert!(out.iter().any(|e| matches!(e, Effect::QuitDeadline { .. })));
    // The answer lets the quit go on.
    let answer = perform(effect).await;
    let out = update(&mut state, answer);
    assert!(out.iter().any(|e| matches!(e, Effect::Exit)));
}

#[tokio::test]
async fn the_deadline_leaves_without_the_answer() {
    let (mut state, from, to, _path, _dir) = two_bases().await;
    update(&mut state, copy(from, to, CopyScope::Filter));
    update(&mut state, Message::Quit);
    let out = update(&mut state, Message::Bases(BasesMessage::QuitDeadline));
    assert!(out.iter().any(|e| matches!(e, Effect::Exit)));
}

#[tokio::test]
async fn copying_the_selection_to_the_clipbase_keeps_the_deleted_mark() {
    use mychess_db::si4::{FlagOp, GameFlags, Si4Writer};
    let dir = tempfile::tempdir().expect("dir");
    let path = make_si4(dir.path(), "d", &pgn("D", 3));
    {
        let mut writer = Si4Writer::open(&path).expect("writer");
        writer
            .set_flag(2, GameFlags::DELETE, FlagOp::Set)
            .expect("flag");
    }
    let mut state = state();
    drive(&mut state, BasesMessage::Open(path)).await;
    let from = state.bases.current();
    for (n, mode) in [(1, PickMode::Only), (2, PickMode::Toggle)] {
        update(
            &mut state,
            list(ListMessage::Pick {
                slot: from,
                number: n,
                mode,
            }),
        );
    }
    drive(&mut state, list(ListMessage::CopySelected { slot: from })).await;
    let rows = state.bases.list_page(SlotId::CLIPBASE, 1, 10);
    assert_eq!(
        rows.iter().map(|r| r.deleted).collect::<Vec<_>>(),
        [false, true]
    );
    assert_eq!(notice(&state), Some(&BasesNotice::Copied { count: 2 }));
}

#[tokio::test]
async fn a_source_changed_since_it_was_read_is_refused() {
    use mychess_db::si4::{FlagOp, GameFlags, Si4Writer};
    let dir = tempfile::tempdir().expect("dir");
    let path = make_si4(dir.path(), "s", &pgn("S", 3));
    let mut state = state();
    drive(&mut state, BasesMessage::Open(path.clone())).await;
    let from = state.bases.current();
    {
        let mut writer = Si4Writer::open(&path).expect("writer");
        writer
            .set_flag(1, GameFlags::DELETE, FlagOp::Set)
            .expect("flag");
    }
    drive(&mut state, copy(from, SlotId::CLIPBASE, CopyScope::Filter)).await;
    assert!(matches!(
        notice(&state),
        Some(BasesNotice::CopyStopped {
            count: 0,
            error: BaseError::Stale,
            ..
        })
    ));
    assert_eq!(state.bases.slot(SlotId::CLIPBASE).expect("clip").total(), 0);
}

#[tokio::test]
async fn cancelling_keeps_the_batches_already_written() {
    // More games than one batch: the write closure cancels after the first
    // batch, the next game sees it and the loop ends with 5000 copied.
    let dir = tempfile::tempdir().expect("dir");
    let path = write_file(dir.path(), "big.pgn", &pgn("G", COPY_BATCH + 20));
    let mut state = state();
    drive(&mut state, BasesMessage::Open(path)).await;
    let id = state.bases.current();
    let source = state.bases.slot(id).expect("slot").source.clone();
    let control = OpenControl::new();
    let games = u32::try_from(COPY_BATCH + 20).expect("small");
    let total = u64::from(games);
    let mut batches = Vec::new();
    let (copied, _, error) = run_copy(&source, 1..=games, &control, total, &mut |batch| {
        batches.push(batch.len());
        control.cancel();
        Ok(())
    });
    assert_eq!(batches, [COPY_BATCH]);
    assert_eq!(usize::try_from(copied).ok(), Some(COPY_BATCH));
    assert_eq!(error, Some(BaseError::Cancelled));
}

#[tokio::test]
async fn a_rejected_batch_is_not_counted_and_ends_the_copy() {
    let dir = tempfile::tempdir().expect("dir");
    let path = write_file(dir.path(), "g.pgn", &pgn("G", 5));
    let mut state = state();
    drive(&mut state, BasesMessage::Open(path)).await;
    let source: SharedSource = state
        .bases
        .slot(state.bases.current())
        .expect("slot")
        .source
        .clone();
    let control = OpenControl::new();
    let (copied, _, error) = run_copy(&source, 1..=5, &control, 5, &mut |_| {
        Err(BaseError::NameLimit { limit: 7 })
    });
    assert_eq!(
        (copied, error),
        (0, Some(BaseError::NameLimit { limit: 7 }))
    );
    // An unreadable game stops the copy but keeps what came before.
    let mut kept = 0usize;
    let (copied, _, error) = run_copy(&source, [1u32, 2, 99].into_iter(), &control, 3, &mut |b| {
        kept += b.len();
        Ok(())
    });
    assert_eq!((copied, kept), (2, 2));
    assert_eq!(error, Some(BaseError::GameUnreadable { number: 99 }));
}

#[test]
fn name_and_record_limits_have_their_own_errors() {
    use mychess_db::si4::{NameKind, Si4Error};
    assert_eq!(
        BaseError::from_si4(&Si4Error::NameLimit {
            kind: NameKind::Player,
            limit: 9
        }),
        BaseError::NameLimit { limit: 9 }
    );
    assert_eq!(
        BaseError::from_si4(&Si4Error::RecordTooLarge { size: 2, limit: 1 }),
        BaseError::Unencodable
    );
    assert_eq!(
        BaseError::NameLimit { limit: 1 }.message_key(),
        "base-error-name-limit"
    );
}

// ---- tester: the answer to the question revalidates the state (D580) ----

fn refused(state: &AppState) -> Option<CopyRefusal> {
    match notice(state) {
        Some(BasesNotice::CopyRefused { reason, .. }) => Some(*reason),
        _ => None,
    }
}

async fn asked() -> (AppState, SlotId, SlotId, PathBuf, tempfile::TempDir) {
    let (mut state, from, to, path, dir) = two_bases().await;
    state.prefs.editing.confirm_copy = true;
    update(&mut state, copy(from, to, CopyScope::Filter));
    assert!(state.copy_prompt.is_some());
    (state, from, to, path, dir)
}

#[tokio::test]
async fn a_target_closed_during_the_question_is_refused_on_the_answer() {
    let (mut state, _from, to, path, _dir) = asked().await;
    drop(state.bases.remove(to));
    let effects = update(&mut state, list(ListMessage::AnswerCopy(true)));
    assert!(effects.is_empty());
    assert_eq!(refused(&state), Some(CopyRefusal::TargetNotOpen));
    assert!(state.copy_prompt.is_none() && state.list_job().is_none());
    let db = mychess_db::si4::Si4Database::open(&path).expect("open");
    assert_eq!(mychess_db::GameSource::len(&db), 3);
}

#[tokio::test]
async fn a_source_closed_during_the_question_is_refused_on_the_answer() {
    let (mut state, from, _to, _path, _dir) = asked().await;
    drop(state.bases.remove(from));
    let effects = update(&mut state, list(ListMessage::AnswerCopy(true)));
    assert!(effects.is_empty());
    assert_eq!(refused(&state), Some(CopyRefusal::SourceNotOpen));
}

#[tokio::test]
async fn a_filter_emptied_during_the_question_is_refused_on_the_answer() {
    let (mut state, from, _to, _path, _dir) = asked().await;
    state.bases.slot_mut(from).expect("source").filter.negate();
    let effects = update(&mut state, list(ListMessage::AnswerCopy(true)));
    assert!(effects.is_empty());
    assert_eq!(refused(&state), Some(CopyRefusal::EmptyFilter));
}

#[tokio::test]
async fn a_target_marked_read_only_during_the_question_is_refused_on_the_answer() {
    let (mut state, _from, to, _path, _dir) = asked().await;
    state.bases.slot_mut(to).expect("target").read_only = true;
    let effects = update(&mut state, list(ListMessage::AnswerCopy(true)));
    assert!(effects.is_empty());
    assert_eq!(refused(&state), Some(CopyRefusal::TargetReadOnly));
}

#[tokio::test]
async fn a_selection_emptied_during_the_question_is_refused_on_the_answer() {
    let (mut state, from, to, _path, _dir) = two_bases().await;
    state.prefs.editing.confirm_copy = true;
    update(
        &mut state,
        list(ListMessage::Pick {
            slot: from,
            number: 2,
            mode: PickMode::Only,
        }),
    );
    update(&mut state, copy(from, to, CopyScope::Selection));
    assert_eq!(state.copy_prompt.as_ref().map(|p| p.count), Some(1));
    state
        .bases
        .slot_mut(from)
        .expect("source")
        .list
        .selection
        .clear();
    let effects = update(&mut state, list(ListMessage::AnswerCopy(true)));
    assert!(effects.is_empty());
    assert_eq!(refused(&state), Some(CopyRefusal::NothingSelected));
}

#[tokio::test]
async fn a_copied_game_keeps_comments_variations_nags_and_its_start_position() {
    let text = "[Event \"R\"]\n[White \"W\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\n\
        1. e4 {best by test} e5 $1 (1... c5 2. Nf3 {sicilian}) 2. Nf3 1-0\n\n\
        [Event \"F\"]\n[White \"W2\"]\n[Black \"B2\"]\n[Result \"*\"]\n\
        [SetUp \"1\"]\n[FEN \"4k3/8/8/8/8/8/4P3/4K3 w - - 0 1\"]\n\n1. e4 e5 *\n\n";
    let dir = tempfile::tempdir().expect("dir");
    let source = write_file(dir.path(), "src.pgn", text);
    let target = make_si4(dir.path(), "dst", &pgn("T", 1));
    let mut state = state();
    state.prefs.editing.confirm_copy = false;
    drive(&mut state, BasesMessage::Open(source)).await;
    let from = state.bases.current();
    drive(&mut state, BasesMessage::Open(target)).await;
    let to = state.bases.current();
    drive(&mut state, copy(from, to, CopyScope::Filter)).await;
    for (n, copied) in [(1u32, 2u32), (2, 3)] {
        let a = state.bases.slot(from).expect("from").source.lock().game(n);
        let b = state.bases.slot(to).expect("to").source.lock().game(copied);
        let (a, b) = (a.expect("source game"), b.expect("copied game"));
        assert_eq!(a.tags().white, b.tags().white);
        let options = crate::prefs::Preferences::default()
            .pgn_export
            .write_options();
        let text =
            |g: &mychess_core::Game| mychess_pgn::write::game_to_string(g, &options).expect("pgn");
        assert_eq!(text(&a), text(&b), "game {n} survives the copy");
    }
}
