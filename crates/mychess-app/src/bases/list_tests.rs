//! Sanity tests of the list actions (slice M3-4c). The full contract suite
//! is the tester's.
#![allow(clippy::large_futures)] // the test rig owns the whole `AppState` across awaits

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use crate::bases::{
    BaseKind, BasesMessage, BasesNotice, ListColumn, ListMessage, PickMode, SlotId, SortDir,
};
use crate::effect::{Effect, perform};
use crate::flow::AppNotice;
use crate::message::Message;
use crate::state::{AppState, update};

/// Five games: players Echo, alpha, Charlie, Bravo, delta (case matters
/// for the text order), events and lengths varied.
#[allow(clippy::format_push_string)]
fn games() -> String {
    let rows = [
        ("Echo", "Zed", "Rome", "1-0", "1. e4 e5 2. Nf3 *"),
        ("alpha", "Yan", "Oslo", "0-1", "1. d4 *"),
        (
            "Charlie",
            "Xia",
            "Rome",
            "1/2-1/2",
            "1. c4 c5 2. Nc3 Nc6 3. g3 *",
        ),
        ("Bravo", "Wu", "Paris", "1-0", "1. e4 *"),
        ("delta", "Vik", "Oslo", "*", "1. Nf3 d5 *"),
    ];
    let mut text = String::new();
    for (n, (white, black, site, result, moves)) in rows.iter().enumerate() {
        text.push_str(&format!(
            "[Event \"Ev {n}\"]\n[Site \"{site}\"]\n[Date \"2020.01.0{d}\"]\n[White \"{white}\"]\n\
             [Black \"{black}\"]\n[Result \"{result}\"]\n\n{moves}\n\n",
            d = n + 1
        ));
    }
    text
}

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write");
    path
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

async fn opened() -> (AppState, SlotId, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(dir.path(), "g.pgn", &games());
    let mut state = state();
    drive(&mut state, BasesMessage::Open(path)).await;
    let id = state.bases.current();
    assert_eq!(state.bases.slot(id).expect("slot").total(), 5);
    (state, id, dir)
}

fn numbers(state: &AppState, id: SlotId) -> Vec<u32> {
    state
        .bases
        .list_page(id, 1, 100)
        .iter()
        .map(|r| r.number)
        .collect()
}

fn pick(state: &mut AppState, id: SlotId, number: u32, mode: PickMode) {
    update(
        state,
        list(ListMessage::Pick {
            slot: id,
            number,
            mode,
        }),
    );
}

fn selection(state: &AppState, id: SlotId) -> Vec<u32> {
    let slot = state.bases.slot(id).expect("slot");
    slot.list.selection.iter().copied().collect()
}

#[tokio::test]
async fn clicks_select_toggle_and_extend_over_the_view() {
    let (mut state, id, _dir) = opened().await;
    pick(&mut state, id, 2, PickMode::Only);
    pick(&mut state, id, 4, PickMode::Toggle);
    assert_eq!(selection(&state, id), [2, 4]);
    pick(&mut state, id, 2, PickMode::Toggle);
    assert_eq!(selection(&state, id), [4]);
    pick(&mut state, id, 1, PickMode::Only);
    pick(&mut state, id, 3, PickMode::Range);
    assert_eq!(selection(&state, id), [1, 2, 3]);
    let slot = state.bases.slot(id).expect("slot");
    assert_eq!(slot.list.selected, Some(1), "the anchor stays");
    pick(&mut state, id, 99, PickMode::Only);
    assert_eq!(
        selection(&state, id),
        [1, 2, 3],
        "not in the filter: ignored"
    );
    update(
        &mut state,
        list(ListMessage::SelectNumbers {
            slot: id,
            numbers: vec![5, 4, 77],
        }),
    );
    assert_eq!(selection(&state, id), [4, 5]);
}

#[tokio::test]
async fn removing_resetting_negating_and_keeping_change_only_the_filter() {
    let (mut state, id, _dir) = opened().await;
    pick(&mut state, id, 2, PickMode::Only);
    pick(&mut state, id, 4, PickMode::Toggle);
    update(&mut state, list(ListMessage::RemoveSelected { slot: id }));
    assert_eq!(numbers(&state, id), [1, 3, 5]);
    assert!(
        selection(&state, id).is_empty(),
        "removed games leave the selection"
    );
    update(&mut state, list(ListMessage::NegateFilter { slot: id }));
    assert_eq!(numbers(&state, id), [2, 4]);
    update(&mut state, list(ListMessage::ResetFilter { slot: id }));
    assert_eq!(numbers(&state, id), [1, 2, 3, 4, 5]);
    update(
        &mut state,
        list(ListMessage::RemoveAbove {
            slot: id,
            number: 3,
        }),
    );
    assert_eq!(numbers(&state, id), [3, 4, 5]);
    update(
        &mut state,
        list(ListMessage::RemoveBelow {
            slot: id,
            number: 4,
        }),
    );
    assert_eq!(numbers(&state, id), [3, 4]);
    pick(&mut state, id, 4, PickMode::Only);
    update(&mut state, list(ListMessage::ResetFilter { slot: id }));
    pick(&mut state, id, 2, PickMode::Only);
    pick(&mut state, id, 5, PickMode::Toggle);
    update(&mut state, list(ListMessage::KeepSelected { slot: id }));
    assert_eq!(numbers(&state, id), [2, 5]);
    // The database is untouched.
    assert_eq!(state.bases.slot(id).expect("slot").total(), 5);
}

#[tokio::test]
async fn keep_selected_with_nothing_selected_says_so() {
    let (mut state, id, _dir) = opened().await;
    update(&mut state, list(ListMessage::KeepSelected { slot: id }));
    assert_eq!(numbers(&state, id).len(), 5);
    assert_eq!(
        state.notice,
        Some(AppNotice::Bases(BasesNotice::NothingSelected))
    );
}

#[tokio::test]
async fn filter_by_text_ands_the_plus_terms_and_only_reduces() {
    let (mut state, id, _dir) = opened().await;
    let set = |state: &mut AppState, text: &str| {
        update(
            state,
            Message::Bases(BasesMessage::SetSearchText {
                slot: id,
                text: text.to_owned(),
            }),
        );
    };
    set(&mut state, "oslo");
    drive(&mut state, list(ListMessage::FilterByText { slot: id })).await;
    assert!(numbers(&state, id).is_empty(), "case matters by default");
    update(&mut state, list(ListMessage::ResetFilter { slot: id }));
    update(
        &mut state,
        list(ListMessage::SetIgnoreCase {
            slot: id,
            ignore_case: true,
        }),
    );
    drive(&mut state, list(ListMessage::FilterByText { slot: id })).await;
    assert_eq!(numbers(&state, id), [2, 5]);
    assert_eq!(
        state.notice,
        Some(AppNotice::Bases(BasesNotice::Filtered { removed: 3 }))
    );
    set(&mut state, "oslo+delta");
    drive(&mut state, list(ListMessage::FilterByText { slot: id })).await;
    assert_eq!(numbers(&state, id), [5]);
    // A term that matches games already out cannot bring them back.
    set(&mut state, "rome");
    drive(&mut state, list(ListMessage::FilterByText { slot: id })).await;
    assert!(numbers(&state, id).is_empty());
    assert!(state.list_job().is_none());
}

#[tokio::test]
async fn find_moves_the_cursor_and_wraps_to_the_start_when_nothing_is_left() {
    let (mut state, id, _dir) = opened().await;
    update(
        &mut state,
        Message::Bases(BasesMessage::SetSearchText {
            slot: id,
            text: "Rome".to_owned(),
        }),
    );
    let selected = |state: &AppState| state.bases.slot(id).expect("slot").list.selected;
    drive(&mut state, list(ListMessage::Find { slot: id })).await;
    assert_eq!(selected(&state), Some(1));
    drive(&mut state, list(ListMessage::Find { slot: id })).await;
    assert_eq!(selected(&state), Some(3));
    assert_eq!(numbers(&state, id).len(), 5, "find never filters");
    drive(&mut state, list(ListMessage::Find { slot: id })).await;
    assert_eq!(selected(&state), Some(3), "no hit: the cursor stays");
    assert_eq!(state.bases.slot(id).expect("slot").list.first, 1);
    assert_eq!(state.notice, Some(AppNotice::Bases(BasesNotice::NotFound)));
}

#[tokio::test]
async fn sorting_orders_the_view_flips_and_keeps_the_real_numbers() {
    let (mut state, id, _dir) = opened().await;
    drive(
        &mut state,
        list(ListMessage::SortBy {
            slot: id,
            column: ListColumn::White,
        }),
    )
    .await;
    // alpha, Bravo, Charlie, delta, Echo: case-insensitive.
    assert_eq!(numbers(&state, id), [2, 4, 3, 5, 1]);
    assert_eq!(
        state.bases.slot(id).expect("slot").sort(),
        Some((ListColumn::White, SortDir::Ascending))
    );
    drive(
        &mut state,
        list(ListMessage::SortBy {
            slot: id,
            column: ListColumn::White,
        }),
    )
    .await;
    assert_eq!(numbers(&state, id), [1, 5, 3, 4, 2]);
    drive(
        &mut state,
        list(ListMessage::SortBy {
            slot: id,
            column: ListColumn::Length,
        }),
    )
    .await;
    assert_eq!(
        numbers(&state, id),
        [2, 4, 5, 1, 3],
        "ties keep the database order"
    );
    // Not sortable: ignored.
    update(
        &mut state,
        list(ListMessage::SortBy {
            slot: id,
            column: ListColumn::Number,
        }),
    );
    assert_eq!(
        state.bases.slot(id).expect("slot").sort(),
        Some((ListColumn::Length, SortDir::Ascending))
    );
    // The filter and the sort are independent and combine.
    pick(&mut state, id, 4, PickMode::Only);
    update(&mut state, list(ListMessage::RemoveSelected { slot: id }));
    assert_eq!(numbers(&state, id), [2, 5, 1, 3]);
    let slot = state.bases.slot(id).expect("slot");
    assert_eq!(slot.nth_visible(3), Some(1));
    assert_eq!(slot.position_of(1), Some(3));
    // Find follows the view order.
    update(
        &mut state,
        Message::Bases(BasesMessage::SetSearchText {
            slot: id,
            text: "Rome".to_owned(),
        }),
    );
    drive(&mut state, list(ListMessage::Find { slot: id })).await;
    assert_eq!(
        state.bases.slot(id).expect("slot").list.selected,
        Some(1),
        "the first Rome game in view order"
    );
}

#[tokio::test]
async fn sorting_by_rounds_dates_and_countries_does_not_fail() {
    let (mut state, id, _dir) = opened().await;
    for column in ListColumn::ALL.into_iter().filter(|c| c.is_sortable()) {
        drive(&mut state, list(ListMessage::SortBy { slot: id, column })).await;
        let slot = state.bases.slot(id).expect("slot");
        assert_eq!(
            slot.sort(),
            Some((column, SortDir::Ascending)),
            "{column:?}"
        );
        assert_eq!(numbers(&state, id).len(), 5, "{column:?}");
    }
}

#[tokio::test]
async fn copying_the_selection_sends_the_right_games_to_the_clipbase() {
    let (mut state, id, _dir) = opened().await;
    pick(&mut state, id, 2, PickMode::Only);
    pick(&mut state, id, 5, PickMode::Toggle);
    drive(&mut state, list(ListMessage::CopySelected { slot: id })).await;
    let clip = state.bases.slot(SlotId::CLIPBASE).expect("clipbase");
    assert_eq!((clip.total(), clip.filtered()), (2, 2));
    let whites: Vec<String> = state
        .bases
        .list_page(SlotId::CLIPBASE, 1, 10)
        .into_iter()
        .map(|r| r.white)
        .collect();
    assert_eq!(whites, ["alpha", "delta"]);
    assert_eq!(
        state.notice,
        Some(AppNotice::Bases(BasesNotice::Copied { count: 2 }))
    );
    // From the Clipbase itself is refused; nothing selected too.
    update(
        &mut state,
        list(ListMessage::CopySelected {
            slot: SlotId::CLIPBASE,
        }),
    );
    assert_eq!(
        state.notice,
        Some(AppNotice::Bases(BasesNotice::AlreadyInClipbase))
    );
}

#[tokio::test]
async fn flags_are_refused_on_a_pgn_and_the_clipbase() {
    let (mut state, id, _dir) = opened().await;
    pick(&mut state, id, 1, PickMode::Only);
    update(&mut state, list(ListMessage::ToggleDeleted { slot: id }));
    assert_eq!(state.notice, Some(AppNotice::Bases(BasesNotice::ReadOnly)));
    state.notice = None;
    update(
        &mut state,
        list(ListMessage::ToggleDeleted {
            slot: SlotId::CLIPBASE,
        }),
    );
    assert_eq!(state.notice, Some(AppNotice::Bases(BasesNotice::ReadOnly)));
}

fn make_si4(dir: &Path) -> PathBuf {
    use mychess_db::si4::Si4Writer;
    use mychess_db::{CancelToken, ImportOptions, NoProgress, import_pgn};
    let base = dir.join("db");
    let mut writer = Si4Writer::create(&base, 0).expect("create");
    import_pgn(
        games().as_bytes(),
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

#[tokio::test]
async fn toggling_the_deleted_flag_rewrites_the_file_and_reloads_the_slot() {
    let dir = tempfile::tempdir().expect("dir");
    let path = make_si4(dir.path());
    let mut state = state();
    drive(&mut state, BasesMessage::Open(path)).await;
    let id = state.bases.current();
    assert_eq!(state.bases.slot(id).expect("slot").kind, BaseKind::Si4);
    assert!(!state.bases.slot(id).expect("slot").read_only);
    // Keep a filter, a selection and a position across the reload.
    pick(&mut state, id, 2, PickMode::Only);
    pick(&mut state, id, 3, PickMode::Toggle);
    update(
        &mut state,
        list(ListMessage::RemoveAbove {
            slot: id,
            number: 2,
        }),
    );
    let before = state.bases.slot(id).expect("slot").source.clone();
    drive(&mut state, list(ListMessage::ToggleDeleted { slot: id })).await;
    let slot = state.bases.slot(id).expect("slot");
    assert!(slot.source != before, "the source was reopened");
    assert_eq!(selection(&state, id), [2, 3]);
    assert_eq!(numbers(&state, id), [2, 3, 4, 5]);
    let rows = state.bases.list_page(id, 1, 10);
    assert_eq!(
        rows.iter().map(|r| r.deleted).collect::<Vec<_>>(),
        [true, true, false, false]
    );
    // The file on disk says the same: a fresh reader sees the flags.
    let again = mychess_db::si4::Si4Database::open(
        state
            .bases
            .slot(id)
            .and_then(|s| s.path.as_deref())
            .expect("path"),
    )
    .expect("reopen");
    assert!(
        mychess_db::GameSource::entry(&again, 2)
            .expect("entry")
            .flags
            .bits()
            != 0
    );
    // Toggling again clears them.
    drive(&mut state, list(ListMessage::ToggleDeleted { slot: id })).await;
    assert!(state.bases.list_page(id, 1, 10).iter().all(|r| !r.deleted));
}

#[tokio::test]
async fn a_read_only_mark_blocks_flags_and_can_be_undone() {
    let dir = tempfile::tempdir().expect("dir");
    let path = make_si4(dir.path());
    let mut state = state();
    drive(&mut state, BasesMessage::Open(path)).await;
    let id = state.bases.current();
    pick(&mut state, id, 1, PickMode::Only);
    update(
        &mut state,
        list(ListMessage::SetReadOnly {
            slot: id,
            read_only: true,
        }),
    );
    update(&mut state, list(ListMessage::ToggleDeleted { slot: id }));
    assert_eq!(state.notice, Some(AppNotice::Bases(BasesNotice::ReadOnly)));
    update(
        &mut state,
        list(ListMessage::SetReadOnly {
            slot: id,
            read_only: false,
        }),
    );
    drive(&mut state, list(ListMessage::ToggleDeleted { slot: id })).await;
    assert!(state.bases.list_page(id, 1, 1)[0].deleted);
}

#[tokio::test]
async fn a_job_in_flight_blocks_the_next_and_cancelling_keeps_the_filter() {
    let (mut state, id, _dir) = opened().await;
    update(
        &mut state,
        Message::Bases(BasesMessage::SetSearchText {
            slot: id,
            text: "Rome".to_owned(),
        }),
    );
    let effects = update(&mut state, list(ListMessage::FilterByText { slot: id }));
    assert_eq!(effects.len(), 1);
    assert!(state.list_job().is_some());
    update(&mut state, list(ListMessage::ResetFilter { slot: id }));
    assert_eq!(state.notice, Some(AppNotice::Bases(BasesNotice::ListBusy)));
    update(&mut state, list(ListMessage::CancelJob));
    for effect in effects {
        let answer = perform(effect).await;
        update(&mut state, answer);
    }
    assert!(state.list_job().is_none());
    assert_eq!(
        numbers(&state, id).len(),
        5,
        "cancelled: the filter is as it was"
    );
}

#[tokio::test]
async fn filter_selection_and_sort_of_one_slot_never_touch_another() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    for name in ["a.pgn", "b.pgn"] {
        let path = write(dir.path(), name, &games());
        drive(&mut state, BasesMessage::Open(path)).await;
    }
    let (a, b) = (
        SlotId::from_number(1).expect("slot"),
        SlotId::from_number(2).expect("slot"),
    );
    pick(&mut state, a, 2, PickMode::Only);
    update(&mut state, list(ListMessage::RemoveSelected { slot: a }));
    drive(
        &mut state,
        list(ListMessage::SortBy {
            slot: a,
            column: ListColumn::White,
        }),
    )
    .await;
    assert_eq!(numbers(&state, a), [4, 3, 5, 1]);
    assert_eq!(
        numbers(&state, b),
        [1, 2, 3, 4, 5],
        "the other slot is as it was"
    );
    assert_eq!(state.bases.slot(b).expect("slot").sort(), None);
    assert!(selection(&state, b).is_empty());
    // Switching back and forth keeps each slot's filter and sort.
    update(&mut state, Message::Bases(BasesMessage::Select(b)));
    update(&mut state, Message::Bases(BasesMessage::Select(a)));
    assert_eq!(numbers(&state, a), [4, 3, 5, 1]);
}

#[tokio::test]
async fn the_view_never_waits_for_a_writer_and_shows_the_last_page() {
    let (state, id, _dir) = opened().await;
    let shown = state.bases.list_page(id, 1, 3);
    assert_eq!(shown.len(), 3);
    let source = state.bases.slot(id).expect("slot").source.clone();
    let writer = source.lock();
    // A writer holds the database: no waiting, the last page comes back.
    let again = state.bases.list_page(id, 1, 3);
    assert_eq!(again, shown);
    drop(writer);
}

#[tokio::test]
async fn copying_more_games_than_the_clipbase_has_room_for_fails_before_reading() {
    let (mut state, id, _dir) = opened().await;
    state.bases.set_clipbase_limit(1);
    update(
        &mut state,
        list(ListMessage::SelectNumbers {
            slot: id,
            numbers: vec![1, 2],
        }),
    );
    drive(&mut state, list(ListMessage::CopySelected { slot: id })).await;
    assert_eq!(state.bases.slot(SlotId::CLIPBASE).expect("clip").total(), 0);
    assert!(matches!(
        state.notice,
        Some(AppNotice::Bases(BasesNotice::Failed(
            crate::bases::BaseError::ClipbaseLimit { limit: 1 }
        )))
    ));
}

#[tokio::test]
async fn clipbase_commands_wait_for_a_job_on_the_clipbase() {
    let (mut state, _id, _dir) = opened().await;
    let effects = update(
        &mut state,
        list(ListMessage::SortBy {
            slot: SlotId::CLIPBASE,
            column: ListColumn::White,
        }),
    );
    assert_eq!(effects.len(), 1);
    update(&mut state, Message::Bases(BasesMessage::ClipbaseClear));
    assert_eq!(state.notice, Some(AppNotice::Bases(BasesNotice::ListBusy)));
    update(&mut state, list(ListMessage::CancelJob));
}

#[tokio::test]
async fn a_game_is_not_loaded_while_a_job_works_on_its_database() {
    let (mut state, id, _dir) = opened().await;
    update(
        &mut state,
        list(ListMessage::SortBy {
            slot: id,
            column: ListColumn::White,
        }),
    );
    let effects = update(
        &mut state,
        Message::Bases(BasesMessage::LoadGame {
            slot: id,
            number: 1,
        }),
    );
    assert!(effects.is_empty());
    assert_eq!(state.notice, Some(AppNotice::Bases(BasesNotice::ListBusy)));
}

#[tokio::test]
async fn a_file_changed_behind_the_slot_is_not_written_over() {
    let dir = tempfile::tempdir().expect("dir");
    let path = make_si4(dir.path());
    let mut state = state();
    drive(&mut state, BasesMessage::Open(path.clone())).await;
    let id = state.bases.current();
    // Another program flags game 5 and so changes the file.
    {
        let mut writer = mychess_db::si4::Si4Writer::open(&path).expect("writer");
        writer
            .set_flag(
                5,
                mychess_db::si4::GameFlags::DELETE,
                mychess_db::si4::FlagOp::Set,
            )
            .expect("flag");
    }
    pick(&mut state, id, 1, PickMode::Only);
    drive(&mut state, list(ListMessage::ToggleDeleted { slot: id })).await;
    assert_eq!(
        state.notice,
        Some(AppNotice::Bases(BasesNotice::Failed(
            crate::bases::BaseError::Stale
        )))
    );
    let rows = state.bases.list_page(id, 1, 5);
    assert!(!rows[0].deleted, "nothing was written");
    assert!(rows[4].deleted, "the slot was reloaded and shows the disk");
    // And now the slot is current: the next toggle goes through.
    drive(&mut state, list(ListMessage::ToggleDeleted { slot: id })).await;
    assert!(state.bases.list_page(id, 1, 1)[0].deleted);
}
