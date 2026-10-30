//! Contract tests of the saved advanced searches and the `.sso` import
//! (spec `m6-1-buscas-avancadas.md`, slice M6-1b: D3235, D3250-D3252, D3382;
//! decision of the orchestrator 6; names and files after D639/D640/D643 of
//! `m3-7-buscas-salvas.md`).
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]
mod m6_1b_common;

use std::path::{Path, PathBuf};

use m6_1b_common::*;
use mychess_app::{
    BoardKind, CheckKind, MaterialPiece, RequestId, SavedCommand, SavedFailure, SideKind,
};
use mychess_core::{Color, File};
use rstest::rstest;

fn set(rig: &mut Rig, message: AdvancedSearchMessage) {
    rig.send(Message::AdvancedSearch(message));
}

fn adv(message: AdvancedSearchMessage) -> Message {
    Message::AdvancedSearch(message)
}

fn names(rig: &Rig) -> Vec<String> {
    rig.state
        .advanced_search_view()
        .expect("open")
        .saved
        .into_iter()
        .map(|e| e.name)
        .collect()
}

fn rig_in(dir: &Path) -> Rig {
    let mut rig = Rig::in_dir(dir);
    rig.open_games(dir, "s.pgn", &GAMES);
    rig.open_window(AdvancedKind::Position);
    rig
}

fn saved_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir.join("searches"))
        .map(|rd| rd.map(|e| e.unwrap().path()).collect())
        .unwrap_or_default();
    files.sort();
    files
}

/// Every file of `dir` (not recursive) except the base and the folder.
fn stray_files(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != "s.pgn" && n != "searches")
        .collect()
}

fn saved_effect(effects: &[Effect]) -> Option<(&RequestId, &PathBuf, &SavedCommand)> {
    effects.iter().find_map(|e| match e {
        Effect::AdvancedSaved {
            request,
            dir,
            command,
        } => Some((request, dir, command)),
        _ => None,
    })
}

// ------------------------------------------------------------- saving

#[test]
fn saving_runs_in_perform_then_lists_by_itself() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let effects = rig.raw(adv(AdvancedSearchMessage::SaveAs("Open game".into())));
    assert_eq!(effects.len(), 1, "{effects:?}");
    let (_, folder, command) = saved_effect(&effects).expect("a saved effect");
    assert_eq!(folder, &dir.path().join("searches"));
    assert!(matches!(command, SavedCommand::Save(s) if s.name == "Open game"));
    // `update` wrote nothing.
    assert!(saved_files(dir.path()).is_empty());
    let mut effects = effects;
    let answer = rig.perform(effects.remove(0));
    assert_eq!(saved_files(dir.path()).len(), 1);
    // The answer tells and asks for the list without the front-end asking.
    let next = rig.raw(answer);
    assert_eq!(
        rig.issue(),
        Some(AdvancedSearchNotice::Saved("Open game".into()))
    );
    let (_, _, command) = saved_effect(&next).expect("an automatic List");
    assert_eq!(command, &SavedCommand::List);
    assert!(names(&rig).is_empty(), "not listed before the List answer");
    let mut next = next;
    let listed = rig.perform(next.remove(0));
    rig.raw(listed);
    assert_eq!(names(&rig), ["Open game"]);
}

#[test]
fn saving_with_an_existing_name_replaces_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(&mut rig, AdvancedSearchMessage::SaveAs("Same".into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SetFen(
            "rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b KQkq - 0 1".into(),
        ),
    );
    set(&mut rig, AdvancedSearchMessage::SaveAs("Same".into()));
    assert_eq!(names(&rig), ["Same"]);
    assert_eq!(saved_files(dir.path()).len(), 1);
    set(&mut rig, AdvancedSearchMessage::UseCurrentPosition);
    set(&mut rig, AdvancedSearchMessage::LoadSaved("Same".into()));
    let fen = rig
        .state
        .advanced_search_view()
        .unwrap()
        .position
        .fen
        .unwrap();
    assert!(fen.starts_with("rnbqkbnr/pppppppp/8/8/3P4"), "{fen}");
}

#[test]
fn names_that_differ_only_by_case_or_symbols_are_two_searches() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    let other = "rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b KQkq - 0 1";
    for (name, fen) in [
        ("Foo", AFTER_E4_E5),
        ("foo", other),
        ("A/B", AFTER_E4_E5),
        ("A_B", other),
    ] {
        set(&mut rig, AdvancedSearchMessage::SetFen(fen.into()));
        set(&mut rig, AdvancedSearchMessage::SaveAs(name.into()));
    }
    let mut listed = names(&rig);
    listed.sort();
    assert_eq!(listed, ["A/B", "A_B", "Foo", "foo"]);
    assert_eq!(saved_files(dir.path()).len(), 4);
    for (name, want_other) in [("Foo", false), ("foo", true), ("A/B", false), ("A_B", true)] {
        set(&mut rig, AdvancedSearchMessage::UseCurrentPosition);
        set(&mut rig, AdvancedSearchMessage::LoadSaved(name.into()));
        let fen = rig
            .state
            .advanced_search_view()
            .unwrap()
            .position
            .fen
            .unwrap();
        assert_eq!(fen == other, want_other, "{name}: {fen}");
    }
}

#[test]
fn the_name_is_trimmed() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SaveAs("  Trim me \t".into()),
    );
    assert_eq!(names(&rig), ["Trim me"]);
    set(&mut rig, AdvancedSearchMessage::UseCurrentPosition);
    set(
        &mut rig,
        AdvancedSearchMessage::LoadSaved(" Trim me ".into()),
    );
    assert_eq!(
        rig.state
            .advanced_search_view()
            .unwrap()
            .position
            .fen
            .as_deref(),
        Some(AFTER_E4_E5)
    );
}

#[rstest]
#[case("")]
#[case("   ")]
#[case("\t\n")]
fn a_blank_name_is_invalid_and_nothing_is_written(#[case] name: &str) {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let effects = rig.raw(adv(AdvancedSearchMessage::SaveAs(name.into())));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::InvalidSavedName));
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::AdvancedSearch(
            AdvancedSearchNotice::InvalidSavedName
        ))
    );
    assert!(saved_files(dir.path()).is_empty());
}

#[rstest]
#[case("../escape")]
#[case("..\\escape")]
#[case("../../escape")]
#[case("/abs/escape")]
#[case("C:\\escape")]
#[case("a:b")]
#[case("CON")]
#[case("aux")]
#[case("nul.txt")]
#[case("COM1")]
#[case("lpt9")]
#[case("...")]
#[case("name\u{0}with nul")]
#[case("<>:\"/\\|?*")]
fn dangerous_names_never_escape_the_folder(#[case] name: &str) {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(&mut rig, AdvancedSearchMessage::SaveAs(name.into()));
    // Either refused with the notice, or saved inside `searches/`.
    assert!(
        stray_files(dir.path())
            .iter()
            .all(|n| n == "preferences.toml" || n == "searches")
    );
    assert!(!dir.path().parent().unwrap().join("escape").exists());
    for file in saved_files(dir.path()) {
        assert!(file.starts_with(dir.path().join("searches")));
        let stem = file.file_stem().unwrap().to_string_lossy().to_lowercase();
        // The reserved names of Windows get a prefix (D640).
        for reserved in ["con", "prn", "aux", "nul", "com1", "lpt9"] {
            assert_ne!(stem, reserved, "{file:?}");
        }
        let text = file.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            !text.contains(['<', '>', ':', '"', '|', '?', '*']),
            "{text}"
        );
    }
    if rig.issue() == Some(AdvancedSearchNotice::Saved(name.trim().into())) {
        // What was saved can be loaded back by the same name.
        set(&mut rig, AdvancedSearchMessage::UseCurrentPosition);
        set(&mut rig, AdvancedSearchMessage::LoadSaved(name.into()));
        assert!(
            rig.state
                .advanced_search_view()
                .unwrap()
                .position
                .fen
                .is_some(),
            "{:?}",
            rig.issue()
        );
    }
}

#[test]
fn a_very_long_name_is_refused_or_saved_safely() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let name = "x".repeat(5_000);
    set(&mut rig, AdvancedSearchMessage::SaveAs(name.clone()));
    for file in saved_files(dir.path()) {
        assert!(file.file_name().unwrap().len() <= 255);
    }
    if names(&rig) == [name.clone()] {
        set(&mut rig, AdvancedSearchMessage::DeleteSaved(name));
        assert!(names(&rig).is_empty());
    }
}

#[test]
fn an_invalid_form_is_not_saved_whatever_the_tab() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(&mut rig, AdvancedSearchMessage::SetFen("not a fen".into()));
    set(&mut rig, AdvancedSearchMessage::SaveAs("a".into()));
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::InvalidFen));
    set(&mut rig, AdvancedSearchMessage::SetMoveRange(9, 2));
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Material),
    );
    set(&mut rig, AdvancedSearchMessage::SaveAs("b".into()));
    assert_eq!(
        rig.issue(),
        Some(AdvancedSearchNotice::InvalidMoveRange { min: 9, max: 2 })
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Moves),
    );
    set(&mut rig, AdvancedSearchMessage::SetMovesText("? e4".into()));
    set(&mut rig, AdvancedSearchMessage::SaveAs("c".into()));
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::FirstMoveIsWildcard));
    assert!(saved_files(dir.path()).is_empty());
}

// ---------------------------------------------------- loading by tab

#[test]
fn loading_a_moves_search_shows_its_tab_and_leaves_the_other_forms_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesText("e4 ? Nf3".into()),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesSide(SideKind::White),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesCheck(CheckKind::Check),
    );
    set(&mut rig, AdvancedSearchMessage::SetMovesVariations(true));
    set(
        &mut rig,
        AdvancedSearchMessage::SetOp(AdvancedKind::Moves, SearchOp::And),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Moves),
    );
    set(&mut rig, AdvancedSearchMessage::SaveAs("seq".into()));
    // Spoil everything the load must restore, then show another tab.
    set(&mut rig, AdvancedSearchMessage::SetMovesText("d4".into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesSide(SideKind::Both),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesCheck(CheckKind::No),
    );
    set(&mut rig, AdvancedSearchMessage::SetMovesVariations(false));
    set(
        &mut rig,
        AdvancedSearchMessage::SetOp(AdvancedKind::Moves, SearchOp::Reset),
    );
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Material),
    );
    let before = rig.state.advanced_search_view().unwrap();
    set(&mut rig, AdvancedSearchMessage::LoadSaved("seq".into()));
    let after = rig.state.advanced_search_view().unwrap();
    assert_eq!(after.tab, AdvancedKind::Moves);
    assert_eq!(after.moves.text, "e4 ? Nf3");
    assert_eq!(after.moves.side, SideKind::White);
    assert_eq!(after.moves.check, CheckKind::Check);
    assert!(after.moves.variations);
    assert_eq!(after.moves.op, SearchOp::And);
    // The other tabs were not touched.
    assert_eq!(after.position, before.position);
    assert_eq!(after.material, before.material);
}

#[test]
fn loading_a_position_search_shows_its_tab() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SetBoardKind(BoardKind::Files),
    );
    set(&mut rig, AdvancedSearchMessage::SetBoardVariations(true));
    set(
        &mut rig,
        AdvancedSearchMessage::SetOp(AdvancedKind::Position, SearchOp::Or),
    );
    set(&mut rig, AdvancedSearchMessage::SaveAs("pos".into()));
    set(&mut rig, AdvancedSearchMessage::UseCurrentPosition);
    set(
        &mut rig,
        AdvancedSearchMessage::SetBoardKind(BoardKind::Exact),
    );
    set(&mut rig, AdvancedSearchMessage::SetBoardVariations(false));
    set(
        &mut rig,
        AdvancedSearchMessage::SetOp(AdvancedKind::Position, SearchOp::Reset),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Moves),
    );
    set(&mut rig, AdvancedSearchMessage::LoadSaved("pos".into()));
    let view = rig.state.advanced_search_view().unwrap();
    assert_eq!(view.tab, AdvancedKind::Position);
    assert_eq!(view.position.fen.as_deref(), Some(AFTER_E4_E5));
    assert_eq!(view.position.kind, BoardKind::Files);
    assert!(view.position.variations);
    assert_eq!(view.position.op, SearchOp::Or);
    // A loaded search runs exactly like the saved one.
    set(&mut rig, AdvancedSearchMessage::Start);
    let slot = rig.state.bases.current();
    let numbers = rig.filter_numbers(slot);
    assert!(numbers.contains(&1) && numbers.contains(&2));
}

#[test]
fn loading_a_material_search_pads_the_rows_to_ten() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(
        &mut rig,
        AdvancedSearchMessage::SetRange {
            color: Color::White,
            piece: MaterialPiece::Rook,
            min: 1,
            max: 1,
        },
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetBishops(mychess_app::BishopsKind::Same),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Material),
    );
    set(&mut rig, AdvancedSearchMessage::SaveAs("mat".into()));
    set(&mut rig, AdvancedSearchMessage::MaterialZero);
    set(
        &mut rig,
        AdvancedSearchMessage::SetBishops(mychess_app::BishopsKind::Either),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Position),
    );
    set(&mut rig, AdvancedSearchMessage::LoadSaved("mat".into()));
    let view = rig.state.advanced_search_view().unwrap();
    assert_eq!(view.tab, AdvancedKind::Material);
    let rook = view.material.white.get(MaterialPiece::Rook);
    assert_eq!((rook.min, rook.max), (1, 1));
    assert_eq!(view.material.bishops, mychess_app::BishopsKind::Same);
    assert_eq!(view.material.patterns.len(), mychess_app::PATTERN_ROWS);
}

// ------------------------------------------------------------ deleting

#[test]
fn deleting_removes_the_file_and_refreshes_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(&mut rig, AdvancedSearchMessage::SaveAs("keep".into()));
    set(&mut rig, AdvancedSearchMessage::SaveAs("drop".into()));
    assert_eq!(saved_files(dir.path()).len(), 2);
    set(&mut rig, AdvancedSearchMessage::DeleteSaved("drop".into()));
    assert_eq!(names(&rig), ["keep"]);
    assert_eq!(saved_files(dir.path()).len(), 1);
    // The notice of the deletion is the last one (the List is silent).
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::AdvancedSearch(AdvancedSearchNotice::Deleted(
            "drop".into()
        )))
    );
}

#[test]
fn deleting_a_search_that_does_not_exist_says_so_with_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(&mut rig, AdvancedSearchMessage::SaveAs("keep".into()));
    set(&mut rig, AdvancedSearchMessage::DeleteSaved("ghost".into()));
    assert_eq!(
        rig.issue(),
        Some(AdvancedSearchNotice::SavedNotFound("ghost".into()))
    );
    assert_eq!(names(&rig), ["keep"]);
    assert_eq!(saved_files(dir.path()).len(), 1);
    assert!(!AdvancedSearchNotice::SavedNotFound("ghost".into()).is_form_error());
}

#[test]
fn loading_a_search_that_does_not_exist_says_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(&mut rig, AdvancedSearchMessage::LoadSaved("ghost".into()));
    assert!(matches!(
        rig.issue(),
        Some(AdvancedSearchNotice::SavedNotFound(_))
    ));
}

#[test]
fn loading_or_deleting_by_a_path_never_reaches_outside_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(&mut rig, AdvancedSearchMessage::SaveAs("real".into()));
    let file = saved_files(dir.path()).remove(0);
    let victim = dir.path().join("victim.toml");
    std::fs::copy(&file, &victim).unwrap();
    let before = std::fs::read(&victim).unwrap();
    set(&mut rig, AdvancedSearchMessage::UseCurrentPosition);
    for name in [
        "../victim",
        "../victim.toml",
        "..\\victim",
        "searches/../victim",
    ] {
        set(&mut rig, AdvancedSearchMessage::LoadSaved(name.into()));
        assert!(
            rig.state
                .advanced_search_view()
                .unwrap()
                .position
                .fen
                .is_none(),
            "{name} loaded something"
        );
        set(&mut rig, AdvancedSearchMessage::DeleteSaved(name.into()));
        assert_eq!(std::fs::read(&victim).unwrap(), before, "{name}");
    }
    assert_eq!(names(&rig), ["real"]);
}

// ------------------------------------------------- hostile folder

#[test]
fn damaged_files_in_the_folder_do_not_break_the_list() {
    let dir = tempfile::tempdir().unwrap();
    // Save one good search first with a normal session.
    {
        let mut rig = rig_in(dir.path());
        set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
        set(&mut rig, AdvancedSearchMessage::SaveAs("good".into()));
    }
    let searches = dir.path().join("searches");
    let good = std::fs::read(saved_files(dir.path()).remove(0)).unwrap();
    std::fs::write(searches.join("garbage.toml"), [0u8, 159, 146, 150, 255, 0]).unwrap();
    std::fs::write(searches.join("truncated.toml"), &good[..good.len() / 2]).unwrap();
    std::fs::write(searches.join("empty.toml"), b"").unwrap();
    std::fs::write(searches.join("kind.toml"), "kind = 42\nname = \"k\"\n").unwrap();
    std::fs::write(searches.join("huge.toml"), vec![b'a'; 3 << 20]).unwrap();
    std::fs::write(searches.join("notes.txt"), "not a search").unwrap();
    std::fs::create_dir(searches.join("folder.toml")).unwrap();
    let mut rig = rig_in(dir.path());
    // The window listed on opening: the good one is there, nothing else.
    assert_eq!(names(&rig), ["good"]);
    set(&mut rig, AdvancedSearchMessage::RefreshSaved);
    assert_eq!(names(&rig), ["good"]);
    let before = rig.state.advanced_search_view().unwrap();
    for name in [
        "garbage",
        "truncated",
        "empty",
        "kind",
        "huge",
        "folder",
        "notes",
    ] {
        set(&mut rig, AdvancedSearchMessage::LoadSaved(name.into()));
        assert!(
            matches!(
                rig.issue(),
                Some(AdvancedSearchNotice::SavedNotFound(_) | AdvancedSearchNotice::SavedFailed)
            ),
            "{name}: {:?}",
            rig.issue()
        );
        let after = rig.state.advanced_search_view().unwrap();
        assert_eq!(after.position, before.position, "{name}");
        assert_eq!(after.material, before.material, "{name}");
        assert_eq!(after.moves, before.moves, "{name}");
    }
}

#[test]
fn a_folder_that_cannot_be_written_tells_and_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    // `searches` is a file: nothing can be created inside.
    std::fs::write(dir.path().join("searches"), "i am a file").unwrap();
    let mut rig = rig_in(dir.path());
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(&mut rig, AdvancedSearchMessage::SaveAs("x".into()));
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::SavedFailed));
    assert!(!rig.state.advanced_search().is_running());
}

// ------------------------------------------------------- no folder

#[test]
fn without_a_preferences_file_the_commands_that_need_a_folder_say_so() {
    let mut rig = Rig::new();
    rig.open_games(tempfile::tempdir().unwrap().path(), "s.pgn", &GAMES);
    rig.open_window(AdvancedKind::Position);
    assert!(!rig.state.advanced_search_view().unwrap().can_save);
    for message in [
        AdvancedSearchMessage::SaveAs("x".into()),
        AdvancedSearchMessage::LoadSaved("x".into()),
        AdvancedSearchMessage::DeleteSaved("x".into()),
    ] {
        rig.state.notice = None;
        set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
        let effects = rig.raw(adv(message));
        assert!(effects.is_empty(), "{effects:?}");
        assert_eq!(rig.issue(), Some(AdvancedSearchNotice::NoSavedFolder));
        assert_eq!(
            rig.state.notice,
            Some(AppNotice::AdvancedSearch(
                AdvancedSearchNotice::NoSavedFolder
            ))
        );
    }
    // Listing is silent: the window just has no list.
    rig.state.notice = None;
    let effects = rig.raw(adv(AdvancedSearchMessage::RefreshSaved));
    assert!(effects.is_empty());
    assert!(rig.state.notice.is_none());
    assert!(names(&rig).is_empty());
}

// ---------------------------------------------------- stale answers

#[test]
fn saved_operations_are_serialized_and_each_answer_counts() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    let mut first = rig.raw(adv(AdvancedSearchMessage::LoadSaved("ghost".into())));
    // D3390: the second waits for the first instead of replacing it.
    let second = rig.raw(adv(AdvancedSearchMessage::RefreshSaved));
    assert!(second.is_empty());
    let first_answer = rig.perform(first.remove(0));
    let mut next = rig.raw(first_answer);
    assert_eq!(
        rig.issue(),
        Some(AdvancedSearchNotice::SavedNotFound("ghost".into()))
    );
    assert_eq!(next.len(), 1, "the waiting listing goes now");
    let second_answer = rig.perform(next.remove(0));
    rig.raw(second_answer);
    assert!(names(&rig).is_empty());
}

#[test]
fn an_unknown_saved_answer_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    let before = rig.state.advanced_search_view().unwrap();
    let effects = rig.raw(adv(AdvancedSearchMessage::SavedDone {
        request: RequestId(987_654),
        result: Err(SavedFailure::Io),
    }));
    assert!(effects.is_empty());
    assert_eq!(rig.state.advanced_search_view().unwrap(), before);
}

#[rstest]
#[case(SavedFailure::NotFound, true)]
#[case(SavedFailure::InvalidName, false)]
#[case(SavedFailure::InvalidFile, false)]
#[case(SavedFailure::Io, false)]
#[case(SavedFailure::Internal, false)]
fn every_failure_becomes_a_notice(#[case] failure: SavedFailure, #[case] not_found: bool) {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    let effects = rig.raw(adv(AdvancedSearchMessage::RefreshSaved));
    let (request, _, _) = saved_effect(&effects).expect("effect");
    let request = *request;
    rig.raw(adv(AdvancedSearchMessage::SavedDone {
        request,
        result: Err(failure.clone()),
    }));
    let notice = rig.issue().expect("a notice");
    match failure {
        SavedFailure::NotFound => assert!(matches!(notice, AdvancedSearchNotice::SavedNotFound(_))),
        SavedFailure::InvalidName => assert_eq!(notice, AdvancedSearchNotice::InvalidSavedName),
        _ => assert_eq!(notice, AdvancedSearchNotice::SavedFailed),
    }
    let _ = not_found;
    assert!(!notice.is_form_error());
}

// -------------------------------------------------------------- .sso

const SSO: &str = "# Scid vs PC search options file\r\nset searchType Material\r\n\
    set pMin(wq) 1\r\nset pMax(wq) 1\r\nset pMin(bp) 2\r\nset pMax(bp) 6\r\n\
    set ignoreColors 1\r\nset matchEnd 0\r\nset minMoveNum 3\r\nset maxMoveNum 40\r\n\
    set minHalfMoves 5\r\nset oppBishops Opposite\r\nset ::search::filter::operation 1\r\n";

fn import(rig: &mut Rig, dir: &Path, name: &str, bytes: &[u8]) {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    set(rig, AdvancedSearchMessage::ImportSso(path));
}

#[test]
fn a_sso_file_with_windows_line_endings_reads_like_one_with_unix_ones() {
    let dir = tempfile::tempdir().unwrap();
    let mut crlf = rig_in(dir.path());
    import(&mut crlf, dir.path(), "crlf.sso", SSO.as_bytes());
    assert!(
        matches!(
            crlf.issue(),
            Some(AdvancedSearchNotice::Imported { warnings: 0 })
        ),
        "{:?}",
        crlf.issue()
    );
    let mut lf = Rig::new();
    lf.open_window(AdvancedKind::Material);
    import(
        &mut lf,
        dir.path(),
        "lf.sso",
        SSO.replace("\r\n", "\n").as_bytes(),
    );
    assert_eq!(
        crlf.state.advanced_search_view().unwrap().material,
        lf.state.advanced_search_view().unwrap().material
    );
    // And the old Mac ending.
    let mut cr = Rig::new();
    cr.open_window(AdvancedKind::Material);
    import(
        &mut cr,
        dir.path(),
        "cr.sso",
        SSO.replace("\r\n", "\r").as_bytes(),
    );
    assert!(
        matches!(cr.issue(), Some(AdvancedSearchNotice::Imported { .. })),
        "{:?}",
        cr.issue()
    );
}

#[test]
fn importing_needs_no_saved_folder() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_window(AdvancedKind::Position);
    import(&mut rig, dir.path(), "a.sso", SSO.as_bytes());
    assert!(matches!(
        rig.issue(),
        Some(AdvancedSearchNotice::Imported { .. })
    ));
    let view = rig.state.advanced_search_view().unwrap();
    assert_eq!(view.tab, AdvancedKind::Material);
    assert!(!view.can_save);
    // The ranges are the file's, the pieces it does not mention keep the
    // default of the Scid.
    assert_eq!(view.material.white.get(MaterialPiece::Queen).min, 1);
    assert_eq!(view.material.black.get(MaterialPiece::Pawn).max, 6);
}

#[test]
fn a_sso_with_a_bad_pattern_is_imported_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    let text = "set searchType Material\nset pattPiece(1) wp\nset pattFyle(1) ?\n\
                set pattRank(1) ?\nset pattBool(1) Yes\n";
    import(&mut rig, dir.path(), "w.sso", text.as_bytes());
    assert_eq!(
        rig.issue(),
        Some(AdvancedSearchNotice::Imported { warnings: 1 })
    );
}

#[test]
fn a_sso_with_a_utf8_bom_or_latin_1_text_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let mut bom = rig_in(dir.path());
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend_from_slice(SSO.replace("\r\n", "\n").as_bytes());
    import(&mut bom, dir.path(), "bom.sso", &bytes);
    assert!(
        matches!(bom.issue(), Some(AdvancedSearchNotice::Imported { .. })),
        "BOM: {:?}",
        bom.issue()
    );
    let mut latin = Rig::new();
    latin.open_window(AdvancedKind::Material);
    let mut bytes = b"# B\xFCchner \xE9tude\n".to_vec();
    bytes.extend_from_slice(SSO.replace("\r\n", "\n").as_bytes());
    import(&mut latin, dir.path(), "latin1.sso", &bytes);
    assert!(
        matches!(latin.issue(), Some(AdvancedSearchNotice::Imported { .. })),
        "Latin-1: {:?}",
        latin.issue()
    );
}

#[test]
fn nothing_in_a_sso_is_executed() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    let pwned = dir.path().join("pwned");
    let text = format!(
        "set searchType Material\nset pMin(wq) [file mkdir {0}]\nexec touch {0}\nfile mkdir {0}\n",
        pwned.display()
    );
    import(&mut rig, dir.path(), "evil.sso", text.as_bytes());
    assert!(!pwned.exists());
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::ImportFailed));
}

#[rstest]
#[case::empty(b"".to_vec())]
#[case::binary(vec![0u8, 1, 2, 3, 255, 254, 0, 0, 7])]
#[case::only_comments(b"# one\n# two\n".to_vec())]
#[case::truncated_value(b"set searchType Material\nset pMin(wq)".to_vec())]
#[case::unclosed_brace(b"set searchType Material\nset pMin(wq) {1\n".to_vec())]
#[case::unclosed_quote(b"set searchType Material\nset pMin(wq) \"1\n".to_vec())]
#[case::unclosed_index(b"set searchType Material\nset pMin(wq 1\n".to_vec())]
#[case::huge_numbers(b"set searchType Material\nset pMin(wq) 99999999999999999999\nset pMax(wq) -5\n".to_vec())]
#[case::negative(b"set searchType Material\nset pMin(wq) -1\nset maxMoveNum -9\n".to_vec())]
#[case::words_for_numbers(b"set searchType Material\nset pMin(wq) many\nset oppBishops Purple\n".to_vec())]
#[case::reversed_ranges(b"set searchType Material\nset pMin(wq) 5\nset pMax(wq) 1\nset minMoveNum 50\nset maxMoveNum 2\n".to_vec())]
#[case::unknown_type(b"set searchType Zebra\nset pMin(wq) 1\n".to_vec())]
#[case::no_type(b"set pMin(wq) 1\nset pMax(wq) 1\n".to_vec())]
#[case::very_long_line({ let mut v = b"set searchType Material\nset pMin(wq) ".to_vec(); v.extend(vec![b'9'; 100_000]); v })]
#[case::deep_braces({ let mut v = b"set searchType Material\nset x ".to_vec(); v.extend(vec![b'{'; 50_000]); v })]
fn a_hostile_sso_never_panics_and_leaves_a_usable_window(#[case] bytes: Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    let before = rig.state.advanced_search_view().unwrap();
    import(&mut rig, dir.path(), "x.sso", &bytes);
    let notice = rig.issue().expect("a notice");
    assert!(
        matches!(
            notice,
            AdvancedSearchNotice::Imported { .. } | AdvancedSearchNotice::ImportFailed
        ),
        "{notice:?}"
    );
    let view = rig.state.advanced_search_view().unwrap();
    if notice == AdvancedSearchNotice::ImportFailed {
        // A refused file changes nothing.
        assert_eq!(view.material, before.material);
        assert_eq!(view.tab, before.tab);
    } else {
        assert_eq!(view.tab, AdvancedKind::Material);
        assert_eq!(view.material.patterns.len(), mychess_app::PATTERN_ROWS);
    }
    assert!(view.material.move_max <= 999 && view.material.min_plies <= 99);
    // The window keeps working: the material tab either searches or says
    // why not.
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Material),
    );
    let effects = rig.raw(adv(AdvancedSearchMessage::Start));
    let searched = effects
        .iter()
        .any(|e| matches!(e, Effect::AdvancedSearch { .. }));
    assert!(
        searched ^ rig.issue().is_some_and(|i| i.is_form_error()),
        "{effects:?} {:?}",
        rig.issue()
    );
}

#[test]
fn a_folder_or_a_missing_path_is_an_import_failure() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(
        &mut rig,
        AdvancedSearchMessage::ImportSso(dir.path().to_path_buf()),
    );
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::ImportFailed));
    set(
        &mut rig,
        AdvancedSearchMessage::ImportSso(dir.path().join("nothing").join("here.sso")),
    );
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::ImportFailed));
    set(&mut rig, AdvancedSearchMessage::ImportSso(PathBuf::new()));
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::ImportFailed));
}

#[test]
fn a_huge_sso_is_refused_or_read_but_does_not_hang_or_panic() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    let mut bytes = Vec::new();
    while bytes.len() < (6 << 20) {
        bytes.extend_from_slice(b"# padding padding padding padding padding padding\n");
    }
    bytes.extend_from_slice(b"set searchType Material\n");
    import(&mut rig, dir.path(), "huge.sso", &bytes);
    assert!(matches!(
        rig.issue(),
        Some(AdvancedSearchNotice::Imported { .. } | AdvancedSearchNotice::ImportFailed)
    ));
}

#[test]
fn an_imported_search_can_be_saved_and_loaded_back_equal() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    import(&mut rig, dir.path(), "a.sso", SSO.as_bytes());
    let imported = rig.state.advanced_search_view().unwrap().material;
    set(&mut rig, AdvancedSearchMessage::SaveAs("from sso".into()));
    assert_eq!(names(&rig), ["from sso"]);
    set(&mut rig, AdvancedSearchMessage::MaterialZero);
    set(&mut rig, AdvancedSearchMessage::SetMoveRange(1, 2));
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Position),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::LoadSaved("from sso".into()),
    );
    let view = rig.state.advanced_search_view().unwrap();
    assert_eq!(view.tab, AdvancedKind::Material);
    assert_eq!(view.material, imported);
    let _ = File::A;
}

#[test]
fn the_not_found_notice_of_a_load_carries_the_name_asked() {
    // The Fluent message `advsearch-notice-saved-not-found` has a `{name}`
    // argument (spec, "API para a GUI"): the delete path fills it, so must
    // the load path.
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(&mut rig, AdvancedSearchMessage::LoadSaved("ghost".into()));
    assert_eq!(
        rig.issue(),
        Some(AdvancedSearchNotice::SavedNotFound("ghost".into()))
    );
}
