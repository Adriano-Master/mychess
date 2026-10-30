//! Contract tests of the corrections of the M6-3b review round (spec
//! `m6-3-jogadores-torneios-spelling.md`, D3472 onwards): `[player_tools]`
//! changes while the preferences load, a stale filter link, reads and
//! writes of the same file, the version of the Clipbase, the reload of the
//! game on screen, and the small fixes of the name checker.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

mod m6_3b_common;

use m6_3b_common::*;
use mychess_app::{FinderEffect, GameMessage, NameKind, Nav, PlayerInfoEffect, SpellingEffect};

fn misspelled() -> Vec<G> {
    vec![
        g(
            "Aberto",
            "Rio BRA",
            "2020.01.10",
            "Alfa, Alf",
            "Beta, B",
            "1-0",
            0,
            0,
        ),
        g(
            "Aberto",
            "Rio BRA",
            "2020.01.11",
            "Beta, B",
            "Alfa, Alf",
            "0-1",
            0,
            0,
        ),
        g(
            "Aberto Brasileiro",
            "Rio de Janeiro BRA",
            "2020.05.20",
            "Alfa, A",
            "Delt, D",
            "1/2-1/2",
            0,
            0,
        ),
    ]
}

fn unrated() -> Vec<G> {
    vec![
        g(
            "Aberto",
            "Rio BRA",
            "2020.01.10",
            "Alfa, A",
            "Beta, B",
            "1-0",
            0,
            0,
        ),
        g(
            "Aberto",
            "Rio BRA",
            "2020.03.10",
            "Alfa, A",
            "Beta, B",
            "1-0",
            0,
            0,
        ),
    ]
}

fn has_scan(fx: &[Effect]) -> bool {
    fx.iter()
        .any(|e| matches!(e, Effect::Spelling(SpellingEffect::Scan { .. })))
}

/// Starts a write of the names of the current slot and leaves it in flight.
fn start_write(rig: &mut Rig) -> Effect {
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.raw(SpellingMessage::Apply);
    let fx = rig.raw(SpellingMessage::ConfirmApply(true));
    fx.into_iter()
        .find(|e| matches!(e, Effect::Spelling(SpellingEffect::ApplyNames { .. })))
        .expect("the write started")
}

// ---- 1: [player_tools] while the preferences load ----

#[test]
fn a_player_tools_change_before_preferences_loaded_keeps_the_other_fields_of_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    std::fs::write(
        &path,
        "[player_tools]\nspelling_file = \"mine.ssp\"\nphotos_dir = \"pics\"\n\
         load_at_start = false\nhistory = [\"Kasparov, G\"]\n\
         [player_tools.spellcheck]\nambiguous = true\n",
    )
    .unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut state = AppState::with_preferences_path(Some(path.clone()));
    let fx = update(&mut state, Message::Startup);
    let load = fx
        .into_iter()
        .find(|e| matches!(e, Effect::LoadPreferences { .. }))
        .expect("the preferences are read");
    // Before the file arrives the user toggles an option of the checker.
    update(
        &mut state,
        Message::Spelling(SpellingMessage::SetSurnames(true)),
    );
    let loaded = rt.block_on(perform(load));
    update(&mut state, loaded);
    let tools = &state.prefs.player_tools;
    assert!(tools.spellcheck.surnames, "the change made meanwhile wins");
    assert!(
        tools.spellcheck.ambiguous,
        "the other field of the file stays"
    );
    assert_eq!(tools.spelling_file, "mine.ssp");
    assert_eq!(tools.photos_dir, "pics");
    assert!(!tools.load_at_start);
    assert_eq!(tools.history, ["Kasparov, G"]);
}

// ---- 2: the filter link ----

#[test]
fn a_filter_link_answer_does_not_overwrite_a_filter_made_while_it_ran() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "six", &six());
    rig.send(PlayerInfoMessage::Open {
        name: "Alfa, A".to_owned(),
    });
    let link = rig.raw(PlayerInfoMessage::FilterLink("-fw".to_owned()));
    let Effect::PlayerInfo(PlayerInfoEffect::SetFilter { .. }) = &link[0] else {
        panic!("filter effect: {link:?}");
    };
    let answer = rig.perform(link[0].clone());
    // A newer filter: the user negates it (all games in -> none).
    rig.send(BasesMessage::List(ListMessage::NegateFilter { slot }));
    let newer = rig.filter_numbers(slot);
    rig.raw(answer);
    assert_eq!(rig.filter_numbers(slot), newer, "the newer filter stays");
    assert!(matches!(
        rig.state.notice.as_ref(),
        Some(AppNotice::PlayerInfo(PlayerInfoNotice::Stale))
    ));
}

// ---- 3: no reads while the file is rewritten ----

#[test]
fn nothing_reads_the_file_while_the_names_are_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenAddElo);
    let write = start_write(&mut rig);
    // The plan of Add Elo does not start.
    let fx = rig.raw(SpellingMessage::PlanElo);
    assert!(
        fx.iter()
            .all(|e| !matches!(e, Effect::Spelling(SpellingEffect::PlanElo { .. }))),
        "{fx:?}"
    );
    // Nor a rescan of the checker.
    assert!(!has_scan(&rig.raw(SpellingMessage::Rescan)));
    // Nor the table of the player finder.
    let fx = rig.raw(FinderMessage::OpenPlayers { prefix: None });
    assert!(
        fx.iter()
            .all(|e| !matches!(e, Effect::Finder(FinderEffect::Build { .. }))),
        "{fx:?}"
    );
    // When the write ends the table is built.
    let answer = rig.perform(write);
    let fx = rig.raw(answer);
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::Finder(FinderEffect::Build { .. }))),
        "built again after the write: {fx:?}"
    );
}

#[test]
fn starting_a_write_stops_the_scan_of_the_checker_and_it_runs_again_after() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &unrated());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenAddElo);
    rig.send(SpellingMessage::PlanElo);
    assert!(
        rig.state.add_elo_view().unwrap().planned.is_some(),
        "a plan"
    );
    let fx = rig.raw(SpellingMessage::OpenCheck(NameKind::Player));
    let Some(Effect::Spelling(SpellingEffect::Scan { control, .. })) = fx
        .iter()
        .find(|e| matches!(e, Effect::Spelling(SpellingEffect::Scan { .. })))
    else {
        panic!("scan started: {fx:?}");
    };
    let control = control.clone();
    let fx = rig.raw(SpellingMessage::ConfirmElo);
    assert!(control.is_cancelled(), "the scan yields to the write");
    let write = fx
        .into_iter()
        .find(|e| matches!(e, Effect::Spelling(SpellingEffect::ApplyElo { .. })))
        .expect("write");
    let answer = rig.perform(write);
    let fx = rig.raw(answer);
    assert!(has_scan(&fx), "the checker scans again: {fx:?}");
}

// ---- 4: the version of the Clipbase ----

fn clipbase_with(rig: &mut Rig, dir: &std::path::Path) {
    let (slot, _) = rig.open_si4(dir, "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::ConfirmCopy(
        false,
    ))));
    rig.send(BasesMessage::List(ListMessage::CopyGames {
        from: slot,
        to: SlotId::CLIPBASE,
        scope: mychess_app::CopyScope::Filter,
    }));
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
}

#[test]
fn a_correction_listed_before_the_clipbase_changed_in_place_is_stale() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    clipbase_with(&mut rig, dir.path());
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    assert!(!rig.state.spell_view().unwrap().rows.is_empty());
    // Ratings are added to the Clipbase meanwhile (same source, new version).
    rig.send(SpellingMessage::OpenAddElo);
    rig.send(SpellingMessage::PlanElo);
    assert!(rig.state.add_elo_view().unwrap().planned.is_some());
    rig.send(SpellingMessage::ConfirmElo);
    let fx = rig.raw(SpellingMessage::Apply);
    assert!(
        fx.iter().all(|e| !matches!(e, Effect::Spelling(_))),
        "{fx:?}"
    );
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::Stale));
}

#[test]
fn an_elo_plan_made_before_the_clipbase_changed_in_place_is_stale() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    clipbase_with(&mut rig, dir.path());
    rig.send(SpellingMessage::OpenAddElo);
    rig.send(SpellingMessage::PlanElo);
    assert!(rig.state.add_elo_view().unwrap().planned.is_some());
    // The names are corrected in place meanwhile.
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.send(SpellingMessage::Apply);
    assert!(matches!(
        rig.spelling_notice(),
        Some(SpellingNotice::Applied { .. })
    ));
    let fx = rig.raw(SpellingMessage::ConfirmElo);
    assert!(
        fx.iter().all(|e| !matches!(e, Effect::Spelling(_))),
        "{fx:?}"
    );
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::Stale));
}

// ---- 5: the game on screen after a correction ----

fn open_first_game(rig: &mut Rig, slot: SlotId) {
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    assert!(!rig.state.is_dirty());
}

fn load_base_game(fx: &[Effect]) -> Option<Effect> {
    fx.iter()
        .find(|e| matches!(e, Effect::LoadBaseGame { .. }))
        .cloned()
}

fn dirty(rig: &mut Rig) {
    rig.send(GameMessage::Navigate(Nav::GameEnd));
    rig.send(GameMessage::EnterMoveText("Bc4".to_owned()));
    assert!(rig.state.is_dirty(), "the game was edited");
}

#[test]
fn the_clean_game_on_screen_is_read_again_after_the_correction() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    open_first_game(&mut rig, slot);
    let write = start_write(&mut rig);
    let answer = rig.perform(write);
    let fx = rig.raw(answer);
    assert!(load_base_game(&fx).is_some(), "reloaded: {fx:?}");
}

#[test]
fn an_edited_game_keeps_its_names_and_the_user_is_told() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    open_first_game(&mut rig, slot);
    dirty(&mut rig);
    let write = start_write(&mut rig);
    let answer = rig.perform(write);
    let fx = rig.raw(answer);
    assert!(load_base_game(&fx).is_none(), "{fx:?}");
    assert_eq!(
        rig.spelling_notice(),
        Some(SpellingNotice::GameNotRefreshed)
    );
    assert!(rig.state.is_dirty());
}

#[test]
fn a_reload_answer_that_arrives_after_the_game_was_edited_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    open_first_game(&mut rig, slot);
    let write = start_write(&mut rig);
    let answer = rig.perform(write);
    let fx = rig.raw(answer);
    let reload = load_base_game(&fx).expect("reload asked");
    dirty(&mut rig);
    let loaded = rig.perform(reload);
    rig.raw(loaded);
    assert!(rig.state.is_dirty(), "the edit is still there");
    assert!(rig.state.discard_prompt.is_none(), "no question either");
    assert_eq!(
        rig.spelling_notice(),
        Some(SpellingNotice::GameNotRefreshed)
    );
}

// ---- 10: smaller fixes ----

#[test]
fn confirming_without_a_question_on_screen_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    let fx = rig.raw(SpellingMessage::ConfirmApply(true));
    assert!(
        fx.iter().all(|e| !matches!(e, Effect::Spelling(_))),
        "{fx:?}"
    );
    assert_eq!(disk_players(&path)[0].0, "Alfa, Alf");
}

#[test]
fn an_edited_new_name_is_trimmed_before_it_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    let view = rig.state.spell_view().unwrap();
    let row = view
        .rows
        .iter()
        .find(|r| r.row.old == "Alfa, Alf")
        .expect("row");
    let index = row.index;
    rig.send(SpellingMessage::EditNew(index, "  Alfa, A \t".to_owned()));
    rig.send(SpellingMessage::Apply);
    rig.send(SpellingMessage::ConfirmApply(true));
    assert_eq!(disk_players(&path)[0].0, "Alfa, A");
}

#[test]
fn a_new_name_equal_to_the_old_one_after_trimming_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    let total = rig.state.spell_view().unwrap().total;
    for index in 0..total {
        let old = rig.state.spell_view().unwrap().rows[index].row.old.clone();
        rig.send(SpellingMessage::EditNew(index, format!(" {old} ")));
    }
    rig.send(SpellingMessage::Apply);
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::NoneSelected));
}

// ---- 6: round of D3487 onwards ----

#[test]
fn a_no_op_player_tools_change_before_the_load_is_not_lost() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    // The file has the option on; the default is off.
    std::fs::write(&path, "[player_tools.spellcheck]\nsurnames = true\n").unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut state = AppState::with_preferences_path(Some(path));
    let fx = update(&mut state, Message::Startup);
    let load = fx
        .into_iter()
        .find(|e| matches!(e, Effect::LoadPreferences { .. }))
        .expect("the preferences are read");
    // Off already (the default): the change is a no-op over what is on show.
    update(
        &mut state,
        Message::Spelling(SpellingMessage::SetSurnames(false)),
    );
    let loaded = rt.block_on(perform(load));
    update(&mut state, loaded);
    assert!(
        !state.prefs.player_tools.spellcheck.surnames,
        "the choice made while loading wins over the file"
    );
}

#[test]
fn correcting_the_clipbase_in_place_bumps_the_data_revision() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    clipbase_with(&mut rig, dir.path());
    let rev = |rig: &Rig| rig.state.bases.slot(SlotId::CLIPBASE).unwrap().data_rev();
    let before = rev(&rig);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.send(SpellingMessage::Apply);
    assert!(matches!(
        rig.spelling_notice(),
        Some(SpellingNotice::Applied { .. })
    ));
    let after_names = rev(&rig);
    assert_ne!(before, after_names, "the names changed under the panels");
    // Ratings too.
    rig.send(SpellingMessage::OpenAddElo);
    rig.send(SpellingMessage::PlanElo);
    rig.send(SpellingMessage::ConfirmElo);
    assert_ne!(
        after_names,
        rev(&rig),
        "the ratings changed under the panels"
    );
}

#[test]
fn a_cancelled_write_lets_the_reads_start_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    let write = start_write(&mut rig);
    let fx = rig.raw(FinderMessage::OpenPlayers { prefix: None });
    assert!(
        fx.iter()
            .all(|e| !matches!(e, Effect::Finder(FinderEffect::Build { .. }))),
        "{fx:?}"
    );
    rig.raw(BasesMessage::List(ListMessage::CancelJob));
    let answer = rig.perform(write);
    let fx = rig.raw(answer);
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::Finder(FinderEffect::Build { .. }))),
        "built again after the cancelled write: {fx:?}"
    );
}

#[test]
fn quitting_during_the_write_exits_when_the_write_answers() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    let write = start_write(&mut rig);
    let fx = rig.raw(Message::Quit);
    assert!(
        fx.iter().all(|e| !matches!(e, Effect::Exit)),
        "waits for the write: {fx:?}"
    );
    let answer = rig.perform(write);
    let fx = rig.raw(answer);
    assert!(
        fx.iter().any(|e| matches!(e, Effect::Exit)),
        "the quit goes on when the write is done: {fx:?}"
    );
}

#[test]
fn a_finder_task_that_died_tells_the_user() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    let fx = rig.raw(FinderMessage::OpenPlayers { prefix: None });
    let Some(Effect::Finder(FinderEffect::Build { request, .. })) = fx
        .into_iter()
        .find(|e| matches!(e, Effect::Finder(FinderEffect::Build { .. })))
    else {
        panic!("build started");
    };
    rig.raw(FinderMessage::Failed {
        panel: mychess_app::FinderPanel::Players,
        request,
    });
    assert_eq!(rig.notice(), Some(&AppNotice::Finder(FinderNotice::Failed)));
}

#[test]
fn a_count_that_ended_without_figures_tells_the_user() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    let fx = rig.raw(PlayerInfoMessage::Open {
        name: "Alfa, A".to_owned(),
    });
    let Some(Effect::PlayerInfo(PlayerInfoEffect::Compute { request, .. })) = fx
        .into_iter()
        .find(|e| matches!(e, Effect::PlayerInfo(PlayerInfoEffect::Compute { .. })))
    else {
        panic!("compute started");
    };
    rig.raw(PlayerInfoMessage::Computed {
        request,
        result: Box::new(None),
    });
    assert_eq!(
        rig.notice(),
        Some(&AppNotice::PlayerInfo(PlayerInfoNotice::Failed))
    );
}
