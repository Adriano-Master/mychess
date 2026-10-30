//! Contract tests of the saved advanced searches and of the import of a
//! material `.sso` file (spec `m6-1-buscas-avancadas.md`, slice M6-1b: D3235,
//! D3250, D3251; orchestrator decision 6).
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]
mod m6_1b_common;

use m6_1b_common::*;
use mychess_app::{BishopsKind, CheckKind, MaterialPiece, SideKind};
use mychess_core::{Color, File, Piece, Role};

fn set(rig: &mut Rig, message: AdvancedSearchMessage) {
    rig.send(Message::AdvancedSearch(message));
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

fn rig_in(dir: &std::path::Path) -> Rig {
    let mut rig = Rig::in_dir(dir);
    rig.open_games(dir, "s.pgn", &GAMES);
    rig.open_window(AdvancedKind::Position);
    rig
}

#[test]
fn opening_the_window_lists_the_saved_searches_of_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let rig = rig_in(dir.path());
    assert!(names(&rig).is_empty());
    assert!(rig.state.advanced_search_view().unwrap().can_save);
}

#[test]
fn a_position_search_is_saved_listed_loaded_and_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SetBoardKind(mychess_app::BoardKind::Pawns),
    );
    set(&mut rig, AdvancedSearchMessage::SetBoardIgnoreColors(true));
    set(&mut rig, AdvancedSearchMessage::SaveAs("Open game".into()));
    assert_eq!(names(&rig), ["Open game"]);
    assert!(
        std::fs::read_dir(dir.path().join("searches"))
            .unwrap()
            .any(|e| e.unwrap().path().extension().is_some_and(|x| x == "toml"))
    );
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::AdvancedSearch(AdvancedSearchNotice::Saved(
            "Open game".into()
        )))
    );
    // Change the form, then load: it comes back.
    set(&mut rig, AdvancedSearchMessage::UseCurrentPosition);
    set(
        &mut rig,
        AdvancedSearchMessage::SetBoardKind(mychess_app::BoardKind::Exact),
    );
    set(&mut rig, AdvancedSearchMessage::SetBoardIgnoreColors(false));
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Moves),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::LoadSaved("Open game".into()),
    );
    let view = rig.state.advanced_search_view().unwrap();
    assert_eq!(view.tab, AdvancedKind::Position);
    assert_eq!(view.position.fen.as_deref(), Some(AFTER_E4_E5));
    assert_eq!(view.position.kind, mychess_app::BoardKind::Pawns);
    assert!(view.position.ignore_colors);
    set(
        &mut rig,
        AdvancedSearchMessage::DeleteSaved("Open game".into()),
    );
    assert!(names(&rig).is_empty());
    set(
        &mut rig,
        AdvancedSearchMessage::DeleteSaved("Open game".into()),
    );
    assert_eq!(
        rig.issue(),
        Some(AdvancedSearchNotice::SavedNotFound("Open game".into()))
    );
}

#[test]
fn a_material_search_round_trips_with_patterns_and_options() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Material),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetRange {
            color: Color::Black,
            piece: MaterialPiece::Pawn,
            min: 2,
            max: 6,
        },
    );
    set(&mut rig, AdvancedSearchMessage::SetDiff(Some((-3, 4))));
    set(
        &mut rig,
        AdvancedSearchMessage::SetBishops(BishopsKind::Same),
    );
    set(&mut rig, AdvancedSearchMessage::SetMoveRange(3, 40));
    set(&mut rig, AdvancedSearchMessage::SetMinPlies(5));
    set(&mut rig, AdvancedSearchMessage::SetEndOnly(true));
    set(
        &mut rig,
        AdvancedSearchMessage::SetOp(AdvancedKind::Material, SearchOp::And),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetPatternPiece {
            index: 1,
            piece: Some(Piece {
                color: Color::White,
                role: Role::Pawn,
            }),
        },
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetPatternFile {
            index: 1,
            file: Some(File::D),
        },
    );
    let before = rig.state.advanced_search_view().unwrap().material;
    set(&mut rig, AdvancedSearchMessage::SaveAs("Mine".into()));
    set(&mut rig, AdvancedSearchMessage::MaterialAny);
    set(&mut rig, AdvancedSearchMessage::ClearPatterns);
    set(&mut rig, AdvancedSearchMessage::SetEndOnly(false));
    set(&mut rig, AdvancedSearchMessage::LoadSaved("Mine".into()));
    let after = rig.state.advanced_search_view().unwrap().material;
    assert_eq!(after, before);
    assert_eq!(after.diff, Some((-3, 4)));
    assert!(after.end_only);
}

#[test]
fn a_moves_search_round_trips_as_text() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Moves),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesText("e4 ? Nf3 Qh7#".into()),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesSide(SideKind::White),
    );
    set(&mut rig, AdvancedSearchMessage::SetMovesVariations(true));
    set(&mut rig, AdvancedSearchMessage::SaveAs("Words".into()));
    set(&mut rig, AdvancedSearchMessage::SetMovesText(String::new()));
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesSide(SideKind::Both),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesCheck(CheckKind::No),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Position),
    );
    set(&mut rig, AdvancedSearchMessage::LoadSaved("Words".into()));
    let view = rig.state.advanced_search_view().unwrap();
    assert_eq!(view.tab, AdvancedKind::Moves);
    assert_eq!(view.moves.text, "e4 ? Nf3 Qh7#");
    assert_eq!(view.moves.side, SideKind::White);
    // The `#` raised the test when the search was built, and it was saved so.
    assert_eq!(view.moves.check, CheckKind::Mate);
    assert!(view.moves.variations);
}

#[test]
fn a_search_that_does_not_validate_is_not_saved() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Moves),
    );
    set(&mut rig, AdvancedSearchMessage::SaveAs("Empty".into()));
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::EmptyMoveList));
    assert!(names(&rig).is_empty());
    set(&mut rig, AdvancedSearchMessage::SetMovesText("e4".into()));
    set(&mut rig, AdvancedSearchMessage::SaveAs("  ".into()));
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::InvalidSavedName));
    set(&mut rig, AdvancedSearchMessage::SaveAs("a:b/c".into()));
    // Dangerous characters are the engine's to judge: either way nothing
    // escapes the folder.
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let path = entry.unwrap().path();
        assert!(path.starts_with(dir.path()));
    }
}

#[test]
fn without_a_folder_nothing_is_saved() {
    let mut rig = Rig::new();
    rig.open_window(AdvancedKind::Position);
    assert!(!rig.state.advanced_search_view().unwrap().can_save);
    let effects = rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::SaveAs(
        "x".into(),
    )));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::NoSavedFolder));
}

#[test]
fn loading_a_missing_search_says_so_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    let before = rig.state.advanced_search_view().unwrap();
    set(&mut rig, AdvancedSearchMessage::LoadSaved("nope".into()));
    assert!(matches!(
        rig.issue(),
        Some(AdvancedSearchNotice::SavedNotFound(_))
    ));
    let after = rig.state.advanced_search_view().unwrap();
    assert_eq!(after.position, before.position);
    assert_eq!(after.material, before.material);
}

const SSO: &str = "# Scid vs PC search options file\nset searchType Material\n\
    set pMin(wq) 1\nset pMax(wq) 1\nset pMin(bp) 2\nset pMax(bp) 6\n\
    set ignoreColors 1\nset matchEnd 0\nset minMoveNum 3\nset maxMoveNum 40\n\
    set minHalfMoves 5\nset oppBishops Opposite\nset ::search::filter::operation 1\n\
    set pattPiece(1) wp\nset pattFyle(1) d\nset pattRank(1) ?\nset pattBool(1) Yes\n";

#[test]
fn a_material_sso_file_fills_the_material_tab() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    let path = dir.path().join("endings.sso");
    std::fs::write(&path, SSO).unwrap();
    set(&mut rig, AdvancedSearchMessage::ImportSso(path));
    let view = rig.state.advanced_search_view().unwrap();
    assert_eq!(view.tab, AdvancedKind::Material);
    let m = &view.material;
    assert_eq!(
        (
            m.white.get(MaterialPiece::Queen).min,
            m.white.get(MaterialPiece::Queen).max
        ),
        (1, 1)
    );
    assert_eq!(
        (
            m.black.get(MaterialPiece::Pawn).min,
            m.black.get(MaterialPiece::Pawn).max
        ),
        (2, 6)
    );
    assert!(m.ignore_colors);
    assert_eq!((m.move_min, m.move_max, m.min_plies), (3, 40, 5));
    assert_eq!(m.bishops, BishopsKind::Opposite);
    assert_eq!(m.op, SearchOp::Or);
    // The window does not save the difference: no restriction.
    assert_eq!(m.diff, None);
    assert_eq!(m.patterns[0].file, Some(File::D));
    assert_eq!(m.patterns.len(), mychess_app::PATTERN_ROWS);
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::AdvancedSearch(AdvancedSearchNotice::Imported {
            warnings: 0
        }))
    );
}

#[test]
fn a_file_that_is_not_a_material_sso_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    let header = dir.path().join("header.sso");
    std::fs::write(
        &header,
        "# search\nset searchType Header\nset sWhite Carlsen\n",
    )
    .unwrap();
    set(&mut rig, AdvancedSearchMessage::ImportSso(header));
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::ImportFailed));
    set(
        &mut rig,
        AdvancedSearchMessage::ImportSso(dir.path().join("missing.sso")),
    );
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::ImportFailed));
    // The tab was not touched.
    assert_eq!(
        rig.state
            .advanced_search_view()
            .unwrap()
            .material
            .white
            .get(MaterialPiece::Queen)
            .max,
        2
    );
}

#[test]
fn an_imported_search_runs_like_the_window_would() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    // Material of the sample: at ply 6 of game 4 White has seven pawns.
    let text = SSO.replace(
        "set pMin(wq) 1\nset pMax(wq) 1\n",
        "set pMin(wp) 7\nset pMax(wp) 7\n",
    );
    let text = text
        .replace("set minMoveNum 3", "set minMoveNum 1")
        .replace("set minHalfMoves 5", "set minHalfMoves 1")
        .replace("set pMin(bp) 2\nset pMax(bp) 6\n", "")
        .replace("set ignoreColors 1", "set ignoreColors 0")
        .replace("set oppBishops Opposite", "set oppBishops Either")
        .replace("set pattPiece(1) wp", "set pattPiece(1) ?");
    let path = dir.path().join("w7.sso");
    std::fs::write(&path, text).unwrap();
    set(&mut rig, AdvancedSearchMessage::ImportSso(path));
    set(
        &mut rig,
        AdvancedSearchMessage::SetOp(AdvancedKind::Material, SearchOp::Reset),
    );
    set(&mut rig, AdvancedSearchMessage::Start);
    let slot = rig.state.bases.current();
    assert_eq!(rig.filter_numbers(slot), [4]);
}

// ------------------------------------------- review fixes (D3390, D3393)

/// Answers every effect of `effects`, and those the answers bring.
fn drain(rig: &mut Rig, effects: Vec<Effect>) {
    let mut queue = effects;
    while let Some(effect) = queue.pop() {
        let answer = rig.perform(effect);
        queue.extend(rig.raw(answer));
    }
}

#[test]
fn a_listing_asked_before_the_save_answers_does_not_drop_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let save = rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::SaveAs(
        "Open game".into(),
    )));
    assert_eq!(save.len(), 1);
    // Asked while the save is in flight: waits, no second effect yet.
    let refresh = rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::RefreshSaved));
    assert!(refresh.is_empty());
    drain(&mut rig, save);
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::AdvancedSearch(AdvancedSearchNotice::Saved(
            "Open game".into()
        )))
    );
    assert_eq!(names(&rig), ["Open game"]);
}

#[test]
fn saved_operations_run_in_the_order_they_were_asked() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let mut effects = rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::SaveAs(
        "One".into(),
    )));
    effects.extend(
        rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::SaveAs(
            "Two".into(),
        ))),
    );
    assert_eq!(effects.len(), 1, "the second waits");
    drain(&mut rig, effects);
    assert_eq!(names(&rig), ["One", "Two"]);
}

#[test]
fn loading_a_saved_search_clears_the_old_remark() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(&mut rig, AdvancedSearchMessage::SaveAs("Open game".into()));
    set(&mut rig, AdvancedSearchMessage::SetFen("garbage".into()));
    set(&mut rig, AdvancedSearchMessage::Start);
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::InvalidFen));
    set(
        &mut rig,
        AdvancedSearchMessage::LoadSaved("Open game".into()),
    );
    assert_eq!(rig.issue(), None);
}
