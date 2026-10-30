//! Tests of the bookmark flows with real files (spec M3-6).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use super::{BookmarkNotice, BookmarksMessage, EditorMessage, Folder, LoadedBookmarks, NodeKind};
use crate::bases::{BasesMessage, SlotId};
use crate::effect::{Effect, perform};
use crate::flow::{AppNotice, Command};
use crate::message::Message;
use crate::session::{GameMessage, Nav};
use crate::shortcuts::{Action, Binding, KeyInput, Shortcuts};
use crate::state::{AppState, update};

const THREE: &str = "[White \"A, X\"]\n[Black \"B\"]\n[Site \"Rome, Italy\"]\n[Date \"2001.05.06\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 1-0\n\n\
                     [White \"C\"]\n[Black \"D\"]\n[Result \"0-1\"]\n\n1. d4 d5 2. c4 0-1\n\n\
                     [White \"E\"]\n[Black \"F\"]\n\n1. c4 *\n";
/// The same games with the second and third swapped.
const SWAPPED: &str = "[White \"A, X\"]\n[Black \"B\"]\n[Site \"Rome, Italy\"]\n[Date \"2001.05.06\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 1-0\n\n\
                       [White \"E\"]\n[Black \"F\"]\n\n1. c4 *\n\n\
                       [White \"C\"]\n[Black \"D\"]\n[Result \"0-1\"]\n\n1. d4 d5 2. c4 0-1\n";

#[allow(clippy::field_reassign_with_default)]
fn state() -> AppState {
    let mut state = AppState::default();
    state.open_pgn_in_slots = true;
    state
}

async fn drive(state: &mut AppState, message: impl Into<Message>) -> Vec<Effect> {
    let mut seen = Vec::new();
    let mut queue: VecDeque<Effect> = update(state, message.into()).into();
    while let Some(effect) = queue.pop_front() {
        if effect.runs_on_host() {
            continue;
        }
        seen.push(effect.clone());
        let answer = perform(effect).await;
        queue.extend(update(state, answer));
    }
    seen
}

fn slot1() -> SlotId {
    SlotId::from_number(1).expect("slot")
}

fn write(dir: &Path, text: &str) -> PathBuf {
    let path = dir.join("games.pgn");
    std::fs::write(&path, text).expect("write");
    path
}

async fn open_and_load(state: &mut AppState, path: &Path, number: u32) {
    drive(state, BasesMessage::Open(path.to_path_buf())).await;
    drive(
        state,
        BasesMessage::LoadGame {
            slot: slot1(),
            number,
        },
    )
    .await;
}

fn bookmark_notice(state: &AppState) -> Option<&BookmarkNotice> {
    match &state.notice {
        Some(AppNotice::Bookmark(n)) => Some(n),
        _ => None,
    }
}

fn first_game_id(state: &AppState) -> super::BookmarkId {
    state.bookmarks.roots()[0].id
}

#[tokio::test]
async fn the_game_being_edited_is_bookmarked_with_its_identity_and_ply() {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(dir.path(), THREE);
    let mut state = state();
    open_and_load(&mut state, &path, 1).await;
    update(
        &mut state,
        Message::Game(GameMessage::Navigate(Nav::GoToMainlinePly(2))),
    );
    assert!(state.can_bookmark_current());
    drive(&mut state, BookmarksMessage::AddCurrent).await;
    let game = state.bookmarks.roots()[0].game().expect("game").clone();
    assert_eq!(
        game.base,
        state
            .bases
            .slot(slot1())
            .expect("slot")
            .path
            .clone()
            .expect("path")
    );
    assert_eq!((game.game, game.ply, game.year), (1, 2, 2001));
    assert_eq!((game.white.as_str(), game.black.as_str()), ("A, X", "B"));
    assert_eq!(game.result, "1-0");
    assert_eq!(game.title, "games.pgn:  A - B, 1-0,  Rome 2001");
    assert_eq!(bookmark_notice(&state), Some(&BookmarkNotice::Added));
}

#[tokio::test]
async fn a_cursor_in_a_variation_keeps_the_ply_of_the_main_line_before_it() {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(
        dir.path(),
        "[White \"A\"]\n\n1. e4 e5 2. Nf3 (2. Nc3 Nf6) 2... Nc6 *\n",
    );
    let mut state = state();
    open_and_load(&mut state, &path, 1).await;
    let variation = {
        let game = state.session.game();
        let e5 = game.mainline().nth(1).expect("1...e5");
        game.children(e5).expect("children")[1]
    };
    update(
        &mut state,
        Message::Game(GameMessage::Navigate(Nav::GoToNode(variation))),
    );
    let bookmark = state.current_bookmark().expect("bookmark");
    assert_eq!(bookmark.ply, 2);
}

#[tokio::test]
async fn a_new_game_and_the_clipbase_cannot_be_bookmarked() {
    let mut state = state();
    assert!(!state.can_bookmark_current());
    drive(&mut state, BookmarksMessage::AddCurrent).await;
    assert_eq!(bookmark_notice(&state), Some(&BookmarkNotice::CannotAdd));
    assert!(state.bookmarks.is_empty());
}

#[tokio::test]
async fn going_to_a_bookmark_of_an_open_base_loads_its_game_at_its_ply() {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(dir.path(), THREE);
    let mut state = state();
    open_and_load(&mut state, &path, 2).await;
    update(
        &mut state,
        Message::Game(GameMessage::Navigate(Nav::GoToMainlinePly(3))),
    );
    drive(&mut state, BookmarksMessage::AddCurrent).await;
    drive(
        &mut state,
        BasesMessage::LoadGame {
            slot: slot1(),
            number: 1,
        },
    )
    .await;
    assert_eq!(state.session.game().tags().white, "A, X");
    let id = first_game_id(&state);
    drive(&mut state, BookmarksMessage::Go(id)).await;
    assert_eq!(state.session.game().tags().white, "C");
    assert_eq!(state.session.game().ply(state.session.cursor()), Ok(3));
}

#[tokio::test]
async fn going_when_the_game_is_already_in_the_editor_only_moves_the_cursor() {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(dir.path(), THREE);
    let mut state = state();
    open_and_load(&mut state, &path, 2).await;
    update(
        &mut state,
        Message::Game(GameMessage::Navigate(Nav::GoToMainlinePly(1))),
    );
    drive(&mut state, BookmarksMessage::AddCurrent).await;
    update(
        &mut state,
        Message::Game(GameMessage::Navigate(Nav::GameEnd)),
    );
    let id = first_game_id(&state);
    let effects = drive(&mut state, BookmarksMessage::Go(id)).await;
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::LoadBaseGame { .. })),
        "{effects:?}"
    );
    assert_eq!(state.session.game().ply(state.session.cursor()), Ok(1));
}

#[tokio::test]
async fn going_opens_a_closed_base_and_finds_the_game_after_a_reorder() {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(dir.path(), THREE);
    let mut state = state();
    open_and_load(&mut state, &path, 2).await;
    drive(&mut state, BookmarksMessage::AddCurrent).await;
    drive(&mut state, BasesMessage::Close(slot1())).await;
    assert!(state.bases.slot(slot1()).is_none());
    // The games were reordered while the base was closed: "C" is third now.
    write(dir.path(), SWAPPED);
    let id = first_game_id(&state);
    drive(&mut state, BookmarksMessage::Go(id)).await;
    assert_eq!(state.session.game().tags().white, "C");
    let slot = state.bases.slot(state.bases.current()).expect("slot");
    assert_eq!(slot.current_game, Some(3));
    assert!(state.flow.bookmark_go.is_none());
}

#[tokio::test]
async fn a_bookmark_of_a_missing_base_warns_by_name_and_stays_in_the_list() {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(dir.path(), THREE);
    let mut state = state();
    open_and_load(&mut state, &path, 1).await;
    drive(&mut state, BookmarksMessage::AddCurrent).await;
    drive(&mut state, BasesMessage::Close(slot1())).await;
    std::fs::remove_file(&path).expect("remove");
    let id = first_game_id(&state);
    let recent_before = state.prefs.files.recent.clone();
    assert!(!recent_before.is_empty());
    state.prefs_dirty = false;
    drive(&mut state, BookmarksMessage::Go(id)).await;
    assert!(matches!(
        bookmark_notice(&state),
        Some(BookmarkNotice::BaseUnavailable { title }) if title.starts_with("games.pgn")
    ));
    // A bookmark never touches the recent files (D706).
    assert_eq!(state.prefs.files.recent, recent_before);
    assert!(!state.prefs_dirty);
    assert_eq!(state.bookmarks.len(), 1);
    assert!(state.flow.bookmark_go.is_none());
}

#[tokio::test]
async fn the_editor_cancel_restores_and_ok_keeps() {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(dir.path(), THREE);
    let mut state = state();
    open_and_load(&mut state, &path, 1).await;
    drive(&mut state, BookmarksMessage::AddCurrent).await;
    let before = state.bookmarks.clone();

    let edit = |state: &mut AppState, m: EditorMessage| {
        update(state, Message::Bookmarks(BookmarksMessage::Edit(m)))
    };
    update(&mut state, Message::Bookmarks(BookmarksMessage::OpenEditor));
    assert!(state.modal_open());
    edit(&mut state, EditorMessage::NewFolder);
    edit(&mut state, EditorMessage::Rename("Openings".to_owned()));
    edit(&mut state, EditorMessage::MoveUp);
    edit(&mut state, EditorMessage::Cancel);
    assert!(state.bookmark_editor.is_none());
    assert_eq!(state.bookmarks, before);

    update(&mut state, Message::Bookmarks(BookmarksMessage::OpenEditor));
    edit(&mut state, EditorMessage::NewFolder);
    edit(&mut state, EditorMessage::Rename("Openings".to_owned()));
    edit(&mut state, EditorMessage::MoveUp);
    // Indent the game (second now) into the folder.
    let game_id = state
        .bookmark_editor
        .as_ref()
        .expect("editor")
        .working
        .roots()[1]
        .id;
    edit(&mut state, EditorMessage::Select(Some(game_id)));
    edit(&mut state, EditorMessage::Indent);
    edit(&mut state, EditorMessage::Ok);
    assert!(state.bookmark_editor.is_none());
    let rows = state.bookmarks.rows();
    assert_eq!((rows[0].title.as_str(), rows[0].depth), ("Openings", 0));
    assert_eq!(rows[1].depth, 1);
}

#[tokio::test]
async fn go_from_the_editor_keeps_the_edits_and_goes() {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(dir.path(), THREE);
    let mut state = state();
    open_and_load(&mut state, &path, 2).await;
    drive(&mut state, BookmarksMessage::AddCurrent).await;
    drive(
        &mut state,
        BasesMessage::LoadGame {
            slot: slot1(),
            number: 1,
        },
    )
    .await;
    update(&mut state, Message::Bookmarks(BookmarksMessage::OpenEditor));
    let id = first_game_id(&state);
    update(
        &mut state,
        Message::Bookmarks(BookmarksMessage::Edit(EditorMessage::Select(Some(id)))),
    );
    update(
        &mut state,
        Message::Bookmarks(BookmarksMessage::Edit(EditorMessage::Rename(
            "Mine".to_owned(),
        ))),
    );
    drive(&mut state, BookmarksMessage::Edit(EditorMessage::Go)).await;
    assert!(state.bookmark_editor.is_none());
    assert_eq!(state.bookmarks.roots()[0].title(), "Mine");
    assert_eq!(state.session.game().tags().white, "C");
}

#[test]
fn an_empty_folder_title_and_deleting_in_the_editor() {
    let mut state = AppState::default();
    update(&mut state, Message::Bookmarks(BookmarksMessage::OpenEditor));
    let edit = |state: &mut AppState, m: EditorMessage| {
        update(state, Message::Bookmarks(BookmarksMessage::Edit(m)))
    };
    edit(&mut state, EditorMessage::NewFolder);
    edit(&mut state, EditorMessage::Rename(String::new()));
    edit(&mut state, EditorMessage::AddCurrent);
    assert_eq!(bookmark_notice(&state), Some(&BookmarkNotice::CannotAdd));
    edit(&mut state, EditorMessage::NewFolder);
    edit(&mut state, EditorMessage::Delete);
    edit(&mut state, EditorMessage::Ok);
    assert_eq!(state.bookmarks.len(), 1);
    assert_eq!(state.bookmarks.roots()[0].title(), super::NEW_FOLDER_TITLE);
}

#[test]
fn saving_is_one_at_a_time_and_a_quit_waits_for_it() {
    let dir = tempfile::tempdir().expect("dir");
    let prefs = dir.path().join("preferences.toml");
    let mut state = AppState::with_preferences_path(Some(prefs.clone()));
    // The file was read already.
    update(
        &mut state,
        Message::Bookmarks(BookmarksMessage::Loaded(LoadedBookmarks::default())),
    );
    let add_folder = |state: &mut AppState| {
        update(state, Message::Bookmarks(BookmarksMessage::OpenEditor));
        update(
            state,
            Message::Bookmarks(BookmarksMessage::Edit(EditorMessage::NewFolder)),
        );
        update(
            state,
            Message::Bookmarks(BookmarksMessage::Edit(EditorMessage::Ok)),
        )
    };
    let first = add_folder(&mut state);
    assert!(matches!(
        first.as_slice(),
        [Effect::SaveBookmarks { path, .. }] if *path == dir.path().join("bookmarks.toml")
    ));
    let second = add_folder(&mut state);
    assert!(second.is_empty(), "one save in flight: {second:?}");
    let third = update(
        &mut state,
        Message::Bookmarks(BookmarksMessage::Saved(Ok(()))),
    );
    assert!(
        matches!(third.as_slice(), [Effect::SaveBookmarks { bookmarks, .. }] if bookmarks.len() == 2)
    );
    // A quit now waits for the save.
    let quit = update(&mut state, Message::Quit);
    assert!(
        !quit
            .iter()
            .any(|e| matches!(e, Effect::Exit | Effect::SavePreferences { .. }))
    );
    let after = update(
        &mut state,
        Message::Bookmarks(BookmarksMessage::Saved(Ok(()))),
    );
    assert!(
        after
            .iter()
            .any(|e| matches!(e, Effect::SavePreferences { .. }))
    );
}

#[test]
fn a_failed_save_keeps_the_list_and_warns() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
    update(
        &mut state,
        Message::Bookmarks(BookmarksMessage::Loaded(LoadedBookmarks::default())),
    );
    update(&mut state, Message::Bookmarks(BookmarksMessage::OpenEditor));
    update(
        &mut state,
        Message::Bookmarks(BookmarksMessage::Edit(EditorMessage::NewFolder)),
    );
    update(
        &mut state,
        Message::Bookmarks(BookmarksMessage::Edit(EditorMessage::Ok)),
    );
    let error = crate::error::FileError::new(crate::error::FileErrorKind::PermissionDenied, None);
    update(
        &mut state,
        Message::Bookmarks(BookmarksMessage::Saved(Err(error))),
    );
    assert_eq!(bookmark_notice(&state), Some(&BookmarkNotice::SaveFailed));
    assert_eq!(state.bookmarks.len(), 1);
}

#[tokio::test]
async fn startup_reads_the_file_next_to_the_preferences_and_the_list_survives_a_restart() {
    let dir = tempfile::tempdir().expect("dir");
    let prefs = dir.path().join("preferences.toml");
    let mut first = AppState::with_preferences_path(Some(prefs.clone()));
    drive(&mut first, Message::Startup).await;
    update(&mut first, Message::Bookmarks(BookmarksMessage::OpenEditor));
    update(
        &mut first,
        Message::Bookmarks(BookmarksMessage::Edit(EditorMessage::NewFolder)),
    );
    update(
        &mut first,
        Message::Bookmarks(BookmarksMessage::Edit(EditorMessage::Rename(
            "Keep me".to_owned(),
        ))),
    );
    drive(&mut first, BookmarksMessage::Edit(EditorMessage::Ok)).await;
    assert!(dir.path().join("bookmarks.toml").exists());

    let mut second = AppState::with_preferences_path(Some(prefs));
    drive(&mut second, Message::Startup).await;
    assert_eq!(second.bookmarks.roots()[0].title(), "Keep me");
}

#[test]
fn a_bookmark_added_while_the_file_is_being_read_is_kept() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
    update(&mut state, Message::Startup);
    assert!(state.flow.bookmarks_loading);
    // The editor does not open while loading.
    update(&mut state, Message::Bookmarks(BookmarksMessage::OpenEditor));
    assert!(state.bookmark_editor.is_none());
    let mut loaded = LoadedBookmarks::default();
    let id = loaded
        .bookmarks
        .insert_after(
            None,
            NodeKind::Folder(Folder {
                title: "From file".to_owned(),
                children: Vec::new(),
            }),
        )
        .expect("insert");
    assert!(loaded.bookmarks.get(id).is_some());
    update(
        &mut state,
        Message::Bookmarks(BookmarksMessage::Loaded(loaded)),
    );
    assert_eq!(state.bookmarks.len(), 1);
}

#[tokio::test]
async fn bookmarks_added_while_loading_go_before_the_first_folder_in_order() {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(dir.path(), THREE);
    let mut state = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
    update(&mut state, Message::Startup);
    assert!(state.flow.bookmarks_loading);
    open_and_load(&mut state, &path, 1).await;
    drive(&mut state, BookmarksMessage::AddCurrent).await;
    drive(&mut state, BookmarksMessage::AddCurrent).await;
    let mut loaded = LoadedBookmarks::default();
    for title in ["Folder", "Last"] {
        loaded
            .bookmarks
            .insert_after(
                None,
                NodeKind::Folder(Folder {
                    title: title.to_owned(),
                    children: Vec::new(),
                }),
            )
            .expect("insert");
    }
    update(
        &mut state,
        Message::Bookmarks(BookmarksMessage::Loaded(loaded)),
    );
    let kinds: Vec<bool> = state
        .bookmarks
        .roots()
        .iter()
        .map(super::tree::BookmarkNode::is_folder)
        .collect();
    assert_eq!(kinds, [false, false, true, true]);
}

#[test]
fn clearing_the_recent_files_saves_the_preferences() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
    update(
        &mut state,
        Message::PreferencesLoaded(crate::prefs::LoadedPreferences::default()),
    );
    state.prefs.files.recent.push(PathBuf::from("a.pgn"));
    let effects = update(&mut state, Message::Command(Command::ClearRecent));
    assert!(state.prefs.files.recent.is_empty());
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::SavePreferences { .. }))
    );
}

#[test]
fn the_bookmark_keys_are_in_the_central_map() {
    let table = Shortcuts::defaults();
    let key = |text: &str| {
        let b = Binding::parse(text).expect("binding");
        KeyInput {
            key: b.key,
            modifiers: b.modifiers,
        }
    };
    assert_eq!(table.lookup(&key("Command+D")), Some(Action::AddBookmark));
    assert_eq!(
        table.lookup(&key("Command+Shift+D")),
        Some(Action::ManageBookmarks)
    );
    assert_eq!(Action::from_name("AddBookmark"), Some(Action::AddBookmark));
    assert_eq!(
        Action::all().filter(|a| *a == Action::AddBookmark).count(),
        0
    );
}
