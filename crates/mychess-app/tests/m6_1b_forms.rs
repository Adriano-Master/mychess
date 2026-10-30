//! Contract tests of the forms of the advanced searches: validation,
//! presets, preferences and the online lock (spec `m6-1-buscas-avancadas.md`,
//! slice M6-1b: D3215, D3218, D3224-D3226, D3231, D3239; orchestrator
//! decisions 5 and 7).
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]
mod m6_1b_common;

use m6_1b_common::*;
use mychess_app::online::{MarkerInfo, OnlineMessage};
use mychess_app::{
    Action, AdvancedSearchPrefs, BishopsKind, BoardKind, CheckKind, MaterialPiece, Shortcuts,
    SideKind, load_preferences, render_preferences,
};
use mychess_core::{Color, File, Piece, Rank, Role};
use mychess_db::{AdvancedQuery, BishopColors, CheckTest, FilterOp, SearchError as E};

fn set(rig: &mut Rig, message: AdvancedSearchMessage) {
    rig.send(Message::AdvancedSearch(message));
}

/// The notice of `Start` on a tab (no effect must come out).
fn refused(rig: &mut Rig, kind: AdvancedKind) -> AdvancedSearchNotice {
    set(rig, AdvancedSearchMessage::SelectTab(kind));
    let effects = rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Start));
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::AdvancedSearch { .. })),
        "{effects:?}"
    );
    assert!(!rig.state.advanced_search().is_running());
    let notice = rig.issue().expect("an issue");
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::AdvancedSearch(notice.clone()))
    );
    notice
}

fn white(role: Role) -> Piece {
    Piece {
        color: Color::White,
        role,
    }
}

fn black(role: Role) -> Piece {
    Piece {
        color: Color::Black,
        role,
    }
}

#[test]
fn a_bad_fen_is_refused_before_searching() {
    let (mut rig, slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen("not a fen".into()));
    assert_eq!(
        refused(&mut rig, AdvancedKind::Position),
        AdvancedSearchNotice::InvalidFen
    );
    assert_eq!(rig.filter_numbers(slot).len(), 6);
    // A blank field stays a FEN field (D3392); the board is chosen apart.
    set(&mut rig, AdvancedSearchMessage::SetFen("   ".into()));
    assert!(
        rig.state
            .advanced_search_view()
            .unwrap()
            .position
            .fen
            .is_some()
    );
    set(&mut rig, AdvancedSearchMessage::UseCurrentPosition);
    assert!(
        rig.state
            .advanced_search_view()
            .unwrap()
            .position
            .fen
            .is_none()
    );
}

#[test]
fn the_move_list_is_validated() {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetMovesText("  ".into()));
    assert_eq!(
        refused(&mut rig, AdvancedKind::Moves),
        AdvancedSearchNotice::EmptyMoveList
    );
    set(&mut rig, AdvancedSearchMessage::SetMovesText("? e5".into()));
    assert_eq!(
        refused(&mut rig, AdvancedKind::Moves),
        AdvancedSearchNotice::FirstMoveIsWildcard
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesText("Ng1-f3".into()),
    );
    assert!(matches!(
        refused(&mut rig, AdvancedKind::Moves),
        AdvancedSearchNotice::BadMoveToken(t) if t == "Ng1-f3"
    ));
}

#[test]
fn the_notices_have_keys_and_form_errors_are_marked() {
    let errors = [
        AdvancedSearchNotice::InvalidFen,
        AdvancedSearchNotice::InvalidPattern("x"),
        AdvancedSearchNotice::InvalidMoveRange { min: 5, max: 1 },
        AdvancedSearchNotice::EmptyMoveList,
        AdvancedSearchNotice::FirstMoveIsWildcard,
        AdvancedSearchNotice::BadMoveToken("x".into()),
        AdvancedSearchNotice::CheckTestTooLow("Qh7+".into()),
        AdvancedSearchNotice::UnsupportedPosition,
    ];
    for notice in &errors {
        assert!(notice.is_form_error(), "{notice:?}");
        assert!(notice.message_key().starts_with("advsearch-notice-"));
    }
    assert!(!AdvancedSearchNotice::Interrupted.is_form_error());
    let mut keys: Vec<_> = errors
        .iter()
        .map(AdvancedSearchNotice::message_key)
        .collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), errors.len(), "one key each");
    // The engine's errors map one to one.
    assert_eq!(
        AdvancedSearchNotice::from(&E::CheckTestTooLow {
            token: "Qh7+".into()
        }),
        AdvancedSearchNotice::CheckTestTooLow("Qh7+".into())
    );
    assert_eq!(
        AdvancedSearchNotice::from(&E::UnsupportedPosition),
        AdvancedSearchNotice::UnsupportedPosition
    );
    assert_eq!(
        AdvancedSearchNotice::from(&E::FilterSize {
            filter: 1,
            games: 2
        }),
        AdvancedSearchNotice::Stale
    );
}

#[test]
fn a_pattern_needs_a_square_a_file_or_a_rank() {
    let (mut rig, _slot, _dir) = sample();
    set(
        &mut rig,
        AdvancedSearchMessage::SetPatternPiece {
            index: 0,
            piece: Some(white(Role::Pawn)),
        },
    );
    assert!(matches!(
        refused(&mut rig, AdvancedKind::Material),
        AdvancedSearchNotice::InvalidPattern(_)
    ));
    set(
        &mut rig,
        AdvancedSearchMessage::SetPatternFile {
            index: 0,
            file: Some(File::D),
        },
    );
    set(&mut rig, AdvancedSearchMessage::Start);
    assert!(!rig.state.advanced_search().is_running());
}

#[test]
fn the_range_of_move_numbers_is_validated() {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetMoveRange(10, 5));
    assert_eq!(
        refused(&mut rig, AdvancedKind::Material),
        AdvancedSearchNotice::InvalidMoveRange { min: 10, max: 5 }
    );
}

#[test]
fn a_missing_database_is_told() {
    let mut rig = Rig::new();
    rig.open_window(AdvancedKind::Position);
    // Only the (empty) Clipbase is open: its filter is empty but the search
    // runs; a reference to a closed slot is ignored.
    let closed = SlotId::from_number(3).unwrap();
    set(
        &mut rig,
        AdvancedSearchMessage::SetReference(AdvancedKind::Position, Some(closed)),
    );
    assert_eq!(
        rig.state.advanced_search_view().unwrap().position.reference,
        None
    );
}

#[test]
fn the_form_edits_are_clamped() {
    let (mut rig, _slot, _dir) = sample();
    set(
        &mut rig,
        AdvancedSearchMessage::SetRange {
            color: Color::Black,
            piece: MaterialPiece::Rook,
            min: 30,
            max: 200,
        },
    );
    set(&mut rig, AdvancedSearchMessage::SetDiff(Some((-500, 500))));
    set(&mut rig, AdvancedSearchMessage::SetMoveRange(2000, 3000));
    set(&mut rig, AdvancedSearchMessage::SetMinPlies(500));
    let view = rig.state.advanced_search_view().unwrap();
    let rook = view.material.black.get(MaterialPiece::Rook);
    assert_eq!((rook.min, rook.max), (9, 9));
    assert_eq!(view.material.diff, Some((-99, 99)));
    assert_eq!((view.material.move_min, view.material.move_max), (999, 999));
    assert_eq!(view.material.min_plies, 99);
    // Out of range rows are ignored, not a panic.
    set(
        &mut rig,
        AdvancedSearchMessage::SetPatternPiece {
            index: 99,
            piece: None,
        },
    );
    set(&mut rig, AdvancedSearchMessage::ApplyEnding(999));
    set(&mut rig, AdvancedSearchMessage::ApplyPatternPreset(999));
}

#[test]
fn the_window_starts_as_the_any_material_search() {
    let (mut rig, _slot, _dir) = sample();
    let view = rig.state.advanced_search_view().unwrap();
    for piece in MaterialPiece::ALL {
        let (w, b) = (
            view.material.white.get(piece),
            view.material.black.get(piece),
        );
        assert_eq!(w, b);
        assert_eq!(w.min, 0);
    }
    assert_eq!(view.material.white.get(MaterialPiece::Pawn).max, 8);
    assert_eq!(view.material.white.get(MaterialPiece::Minor).max, 4);
    assert_eq!(view.material.patterns.len(), mychess_app::PATTERN_ROWS);
    assert_eq!((view.material.move_min, view.material.move_max), (1, 999));
    assert_eq!(view.material.min_plies, 1);
    assert_eq!(view.material.diff, None);
    // Zero, then Current Board (no kings), then Any.
    set(&mut rig, AdvancedSearchMessage::MaterialZero);
    let view = rig.state.advanced_search_view().unwrap();
    assert_eq!(view.material.white.get(MaterialPiece::Queen).max, 0);
    set(&mut rig, AdvancedSearchMessage::MaterialCurrentBoard);
    let view = rig.state.advanced_search_view().unwrap();
    for side in [view.material.white, view.material.black] {
        assert_eq!(side.get(MaterialPiece::Pawn).min, 8);
        assert_eq!(side.get(MaterialPiece::Pawn).max, 8);
        assert_eq!(side.get(MaterialPiece::Minor).min, 4);
        assert_eq!(side.get(MaterialPiece::Queen).min, 1);
    }
    set(&mut rig, AdvancedSearchMessage::SetDiff(Some((1, 5))));
    set(&mut rig, AdvancedSearchMessage::MaterialAny);
    let view = rig.state.advanced_search_view().unwrap();
    assert_eq!(view.material.white.get(MaterialPiece::Pawn).max, 8);
    assert_eq!(view.material.diff, None);
}

#[test]
fn the_presets_are_listed_and_applied() {
    let (mut rig, _slot, _dir) = sample();
    let view = rig.state.advanced_search_view().unwrap();
    assert_eq!(view.endings.len(), 13);
    assert_eq!(view.patterns.len(), 14);
    assert!(view.endings.iter().all(|p| !p.key.is_empty()));
    let rooks_passed_a = view
        .endings
        .iter()
        .find(|p| p.key.contains("passed"))
        .expect("the ending with a passed a pawn")
        .index;
    set(&mut rig, AdvancedSearchMessage::ApplyEnding(rooks_passed_a));
    let view = rig.state.advanced_search_view().unwrap();
    // It turns "ignore colors" on and fills three patterns.
    assert!(view.material.ignore_colors);
    assert_eq!(
        view.material
            .patterns
            .iter()
            .filter(|p| p.piece.is_some())
            .count(),
        3
    );
    assert_eq!(view.material.patterns.len(), mychess_app::PATTERN_ROWS);
    assert_eq!(view.material.white.get(MaterialPiece::Rook).max, 1);
    // A common pattern keeps the counts it does not mention.
    let iqp = view
        .patterns
        .iter()
        .find(|p| p.key.contains("white-iqp"))
        .expect("an IQP")
        .index;
    set(&mut rig, AdvancedSearchMessage::ApplyPatternPreset(iqp));
    let view = rig.state.advanced_search_view().unwrap();
    assert!(
        view.material.patterns.iter().any(|p| {
            p.piece == Some(white(Role::Pawn)) && p.file == Some(File::D) && p.present
        })
    );
    set(&mut rig, AdvancedSearchMessage::ClearPatterns);
    let view = rig.state.advanced_search_view().unwrap();
    assert!(view.material.patterns.iter().all(|p| p.piece.is_none()));
}

#[test]
fn the_material_effect_carries_ranges_options_and_patterns() {
    let (mut rig, _slot, _dir) = sample();
    set(
        &mut rig,
        AdvancedSearchMessage::SetRange {
            color: Color::White,
            piece: MaterialPiece::Queen,
            min: 1,
            max: 1,
        },
    );
    set(&mut rig, AdvancedSearchMessage::SetDiff(Some((2, -2))));
    set(
        &mut rig,
        AdvancedSearchMessage::SetBishops(BishopsKind::Opposite),
    );
    set(&mut rig, AdvancedSearchMessage::SetMoveRange(0, 40));
    set(&mut rig, AdvancedSearchMessage::SetMinPlies(3));
    set(&mut rig, AdvancedSearchMessage::SetEndOnly(true));
    set(
        &mut rig,
        AdvancedSearchMessage::SetMaterialIgnoreColors(true),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetOp(AdvancedKind::Material, SearchOp::Or),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetPatternPiece {
            index: 2,
            piece: Some(black(Role::Bishop)),
        },
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetPatternRank {
            index: 2,
            rank: Some(Rank::Seventh),
        },
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetPatternPresent {
            index: 2,
            present: false,
        },
    );
    let Effect::AdvancedSearch { query, .. } = rig.start_effect(AdvancedKind::Material) else {
        panic!("not a search");
    };
    let AdvancedQuery::Material(q) = *query else {
        panic!("not a material search");
    };
    assert_eq!(q.white.queen.min, 1);
    assert_eq!(q.diff, Some((2, -2)));
    assert_eq!(q.bishops, BishopColors::Opposite);
    assert_eq!((q.moves, q.min_plies), ((0, 40), 3));
    assert!(q.end_only && q.ignore_colors);
    assert_eq!(q.op, FilterOp::Or);
    let pattern = q.patterns[2];
    assert_eq!(pattern.piece, Some(black(Role::Bishop)));
    assert_eq!(
        (pattern.file, pattern.rank, pattern.present),
        (None, Some(Rank::Seventh), false)
    );
}

#[test]
fn the_moves_effect_carries_the_options() {
    let (mut rig, _slot, _dir) = sample();
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesText("1.e4 ? Nf3".into()),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesSide(SideKind::White),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesCheck(CheckKind::Mate),
    );
    set(&mut rig, AdvancedSearchMessage::SetMovesVariations(true));
    let Effect::AdvancedSearch { query, .. } = rig.start_effect(AdvancedKind::Moves) else {
        panic!("not a search");
    };
    let AdvancedQuery::Moves(q) = *query else {
        panic!("not a moves search");
    };
    assert_eq!(q.tokens.len(), 3);
    assert_eq!(q.side, mychess_db::SideFilter::White);
    assert_eq!(q.check_test, CheckTest::Mate);
    assert!(q.in_variations);
}

#[test]
fn the_options_survive_the_preferences_file() {
    let (mut rig, _slot, _dir) = sample();
    set(
        &mut rig,
        AdvancedSearchMessage::SetBoardKind(BoardKind::Files),
    );
    set(&mut rig, AdvancedSearchMessage::SetBoardVariations(true));
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesCheck(CheckKind::Check),
    );
    set(&mut rig, AdvancedSearchMessage::SetMinPlies(4));
    for (kind, op) in [
        (AdvancedKind::Position, SearchOp::And),
        (AdvancedKind::Material, SearchOp::Or),
        (AdvancedKind::Moves, SearchOp::And),
    ] {
        set(&mut rig, AdvancedSearchMessage::SetOp(kind, op));
    }
    let prefs = rig.state.prefs.advanced_search;
    assert_ne!(prefs, AdvancedSearchPrefs::default());
    assert!(rig.state.prefs_dirty);
    let text = render_preferences(&rig.state.prefs);
    assert!(text.contains("[advanced_search]"), "{text}");
    let loaded = load_preferences(&text);
    assert_eq!(loaded.prefs.advanced_search, prefs);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    // A new session starts from them.
    let mut again = Rig::new();
    again.send(Message::PreferencesLoaded(loaded));
    again.open_window(AdvancedKind::Position);
    let view = again.state.advanced_search_view().unwrap();
    assert_eq!(
        (view.position.op, view.material.op, view.moves.op),
        (SearchOp::And, SearchOp::Or, SearchOp::And)
    );
    assert_eq!(view.position.kind, BoardKind::Files);
    assert!(view.position.variations);
    assert_eq!(view.moves.check, CheckKind::Check);
    assert_eq!(view.material.min_plies, 4);
}

#[test]
fn the_three_actions_open_their_tab_and_have_no_default_key() {
    let mut rig = Rig::new();
    for (action, kind) in [
        (Action::SearchBoard, AdvancedKind::Position),
        (Action::SearchMaterial, AdvancedKind::Material),
        (Action::SearchMoves, AdvancedKind::Moves),
    ] {
        rig.send(Message::Action(action));
        let view = rig.state.advanced_search_view().expect("open");
        assert_eq!(view.tab, kind);
        rig.send(Message::AdvancedSearch(AdvancedSearchMessage::Close));
        assert!(rig.state.advanced_search_view().is_none());
        assert!(
            Shortcuts::defaults().bindings(action).is_empty(),
            "{action:?}"
        );
        assert_eq!(Action::from_name(action.name()), Some(action));
    }
}

#[test]
fn searching_is_not_blocked_by_the_online_lock() {
    let (mut rig, slot, _dir) = sample();
    rig.send(Message::Online(OnlineMessage::MarkerRead(Some(
        MarkerInfo {
            game_id: "abcd1234".to_owned(),
            date: "2026-10-26".to_owned(),
        },
    ))));
    assert!(rig.state.assist_locked());
    rig.state.notice = None;
    // The family of the lock does not include the searches of local
    // databases (D3239).
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(&mut rig, AdvancedSearchMessage::Start);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 5]);
    // The first game is not loaded over the online game (the editor is
    // locked: that refusal is the lock of the editing, not of the search).
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, None);
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Online(
            mychess_app::online::OnlineNotice::AssistanceDisabled
        ))
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetBoardKind(BoardKind::Material),
    );
    assert_eq!(
        rig.state.prefs.advanced_search.board_kind,
        BoardKind::Material
    );
}

#[test]
fn bishops_kind_is_not_confused_with_check_kind() {
    // Guards the mirrors: each default is the first option of the Scid.
    assert_eq!(BishopsKind::default(), BishopsKind::Either);
    assert_eq!(CheckKind::default(), CheckKind::No);
    assert_eq!(SideKind::default(), SideKind::Both);
    assert_eq!(BoardKind::default(), BoardKind::Exact);
    assert_eq!(SearchOp::default(), SearchOp::Reset);
}

// ------------------------------------------- review fixes (D3392)

#[test]
fn an_emptied_fen_field_stays_on_show_and_is_invalid_on_start() {
    let mut rig = Rig::new();
    rig.open_window(AdvancedKind::Position);
    rig.send(Message::AdvancedSearch(AdvancedSearchMessage::SetFen(
        "8/8".into(),
    )));
    rig.send(Message::AdvancedSearch(AdvancedSearchMessage::SetFen(
        String::new(),
    )));
    let view = rig.state.advanced_search_view().unwrap();
    assert_eq!(view.position.fen.as_deref(), Some(""));
    rig.send(Message::AdvancedSearch(AdvancedSearchMessage::Start));
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::InvalidFen));
    rig.send(Message::AdvancedSearch(
        AdvancedSearchMessage::UseCurrentPosition,
    ));
    assert_eq!(rig.state.advanced_search_view().unwrap().position.fen, None);
}

#[test]
fn a_fen_with_text_after_its_six_fields_is_invalid() {
    let mut rig = Rig::new();
    rig.open_window(AdvancedKind::Position);
    let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1 extra".to_string();
    rig.send(Message::AdvancedSearch(AdvancedSearchMessage::SetFen(fen)));
    rig.send(Message::AdvancedSearch(AdvancedSearchMessage::Start));
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::InvalidFen));
}
