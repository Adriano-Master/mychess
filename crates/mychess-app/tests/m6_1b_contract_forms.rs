//! Contract tests of the forms of the advanced searches: presets, pattern
//! rows, validation notices and their keys, hostile input, and the
//! `[advanced_search]` preferences (spec `m6-1-buscas-avancadas.md`, slice
//! M6-1b: D3215, D3218, D3224-D3226, D3231, D3257, D3258, D3379; decisions
//! of the orchestrator 2, 3 and 5).
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]
mod m6_1b_common;

use m6_1b_common::*;
use mychess_app::{
    AdvancedSearchPrefs, BishopsKind, BoardKind, CheckKind, MaterialPiece, PATTERN_ROWS, SideKind,
    load_preferences, render_preferences,
};
use mychess_core::{Color, File, Piece, Rank, Role};
use mychess_db::{AdvancedQuery, SearchError};
use proptest::prelude::*;
use rstest::rstest;

fn set(rig: &mut Rig, message: AdvancedSearchMessage) {
    rig.send(Message::AdvancedSearch(message));
}

fn has_search(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|e| matches!(e, Effect::AdvancedSearch { .. }))
}

fn white(role: Role) -> Piece {
    Piece {
        color: Color::White,
        role,
    }
}

// ------------------------------------------------------------ presets

#[test]
fn the_presets_out_of_range_are_ignored_and_the_last_ones_work() {
    let (mut rig, _slot, _dir) = sample();
    let view = rig.state.advanced_search_view().unwrap();
    let (endings, patterns) = (view.endings.len(), view.patterns.len());
    assert_eq!((endings, patterns), (13, 14));
    // Indexes of the view are 0-based and contiguous.
    assert!(view.endings.iter().enumerate().all(|(i, p)| p.index == i));
    assert!(view.patterns.iter().enumerate().all(|(i, p)| p.index == i));
    let before = rig.state.advanced_search_view().unwrap();
    for message in [
        AdvancedSearchMessage::ApplyEnding(endings),
        AdvancedSearchMessage::ApplyEnding(usize::MAX),
        AdvancedSearchMessage::ApplyPatternPreset(patterns),
        AdvancedSearchMessage::ApplyPatternPreset(usize::MAX),
    ] {
        let effects = rig.raw(Message::AdvancedSearch(message));
        assert!(effects.is_empty(), "{effects:?}");
        assert_eq!(rig.state.advanced_search_view().unwrap(), before);
    }
    // The last valid indexes still apply.
    set(
        &mut rig,
        AdvancedSearchMessage::ApplyPatternPreset(patterns - 1),
    );
    assert_ne!(
        rig.state.advanced_search_view().unwrap().material,
        before.material
    );
    set(&mut rig, AdvancedSearchMessage::ClearPatterns);
    set(&mut rig, AdvancedSearchMessage::ApplyEnding(endings - 1));
    assert_ne!(
        rig.state.advanced_search_view().unwrap().material,
        before.material
    );
}

#[test]
fn every_preset_gives_a_valid_search_with_ten_rows() {
    let (mut rig, _slot, _dir) = sample();
    let view = rig.state.advanced_search_view().unwrap();
    for ending in &view.endings {
        set(&mut rig, AdvancedSearchMessage::MaterialAny);
        set(&mut rig, AdvancedSearchMessage::ClearPatterns);
        set(&mut rig, AdvancedSearchMessage::ApplyEnding(ending.index));
        let shown = rig.state.advanced_search_view().unwrap();
        assert_eq!(
            shown.material.patterns.len(),
            PATTERN_ROWS,
            "{}",
            ending.key
        );
        set(
            &mut rig,
            AdvancedSearchMessage::SelectTab(AdvancedKind::Material),
        );
        let effects = rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Start));
        assert!(has_search(&effects), "{}: {:?}", ending.key, rig.issue());
        // Do not leave a search running into the next iteration.
        rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Stop));
    }
    for pattern in &view.patterns {
        set(&mut rig, AdvancedSearchMessage::MaterialAny);
        set(&mut rig, AdvancedSearchMessage::ClearPatterns);
        set(
            &mut rig,
            AdvancedSearchMessage::ApplyPatternPreset(pattern.index),
        );
        let shown = rig.state.advanced_search_view().unwrap();
        assert_eq!(
            shown.material.patterns.len(),
            PATTERN_ROWS,
            "{}",
            pattern.key
        );
        assert!(
            shown.material.patterns.iter().any(|p| p.piece.is_some()),
            "{}",
            pattern.key
        );
        set(
            &mut rig,
            AdvancedSearchMessage::SelectTab(AdvancedKind::Material),
        );
        let effects = rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Start));
        assert!(has_search(&effects), "{}: {:?}", pattern.key, rig.issue());
        rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Stop));
    }
}

#[test]
fn an_ending_preset_sets_ignore_colors_in_the_preferences_too() {
    let (mut rig, _slot, _dir) = sample();
    let view = rig.state.advanced_search_view().unwrap();
    let before = rig.state.prefs.advanced_search.material_ignore_colors;
    // Some preset flips it; the value lives in the preferences (D3379).
    let mut seen_change = false;
    for ending in &view.endings {
        set(&mut rig, AdvancedSearchMessage::ApplyEnding(ending.index));
        let shown = rig.state.advanced_search_view().unwrap().material;
        assert_eq!(
            shown.ignore_colors,
            rig.state.prefs.advanced_search.material_ignore_colors
        );
        seen_change |= shown.ignore_colors != before;
    }
    assert!(seen_change, "no ending changes `ignore colors`");
}

// ------------------------------------------------------------ the rows

#[test]
fn the_window_has_ten_pattern_rows_and_an_eleventh_is_ignored() {
    let (mut rig, _slot, _dir) = sample();
    assert_eq!(PATTERN_ROWS, 10);
    let view = rig.state.advanced_search_view().unwrap();
    assert_eq!(view.material.patterns.len(), 10);
    // The tenth row (index 9) is editable.
    set(
        &mut rig,
        AdvancedSearchMessage::SetPatternPiece {
            index: 9,
            piece: Some(white(Role::Rook)),
        },
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetPatternFile {
            index: 9,
            file: Some(File::A),
        },
    );
    let view = rig.state.advanced_search_view().unwrap();
    assert_eq!(view.material.patterns[9].piece, Some(white(Role::Rook)));
    assert_eq!(view.material.patterns[9].file, Some(File::A));
    // The eleventh (index 10) is ignored, whatever the field.
    let before = rig.state.advanced_search_view().unwrap();
    for message in [
        AdvancedSearchMessage::SetPatternPiece {
            index: 10,
            piece: Some(white(Role::Pawn)),
        },
        AdvancedSearchMessage::SetPatternFile {
            index: 10,
            file: Some(File::B),
        },
        AdvancedSearchMessage::SetPatternRank {
            index: 10,
            rank: Some(Rank::Second),
        },
        AdvancedSearchMessage::SetPatternPresent {
            index: 10,
            present: false,
        },
        AdvancedSearchMessage::SetPatternPiece {
            index: usize::MAX,
            piece: None,
        },
    ] {
        let effects = rig.raw(Message::AdvancedSearch(message));
        assert!(effects.is_empty());
        assert_eq!(rig.state.advanced_search_view().unwrap(), before);
    }
    // And the query carries at most the rows of the window.
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Material),
    );
    let Effect::AdvancedSearch { query, .. } = rig.start_effect(AdvancedKind::Material) else {
        panic!("not a search");
    };
    let AdvancedQuery::Material(q) = *query else {
        panic!("not material");
    };
    assert!(q.patterns.len() <= 10, "{}", q.patterns.len());
}

#[test]
fn the_sense_of_a_row_reaches_the_view_and_the_query() {
    let (mut rig, _slot, _dir) = sample();
    set(
        &mut rig,
        AdvancedSearchMessage::SetPatternPiece {
            index: 2,
            piece: Some(white(Role::Pawn)),
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
    let view = rig.state.advanced_search_view().unwrap();
    assert!(!view.material.patterns[2].present);
    // Rows 0 and 1 are untouched.
    assert!(view.material.patterns[0].piece.is_none() && view.material.patterns[0].present);
    let Effect::AdvancedSearch { query, .. } = rig.start_effect(AdvancedKind::Material) else {
        panic!("not a search");
    };
    let AdvancedQuery::Material(q) = *query else {
        panic!("not material");
    };
    let row = q
        .patterns
        .iter()
        .find(|p| p.piece.is_some())
        .expect("the row");
    assert!(!row.present);
    assert_eq!(row.rank, Some(Rank::Seventh));
    assert_eq!(row.file, None);
}

// ----------------------------------------------------------- validation

fn refused_notice(rig: &mut Rig, kind: AdvancedKind) -> AdvancedSearchNotice {
    set(rig, AdvancedSearchMessage::SelectTab(kind));
    let effects = rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Start));
    assert!(!has_search(&effects), "{effects:?}");
    assert!(!rig.state.advanced_search().is_running());
    let notice = rig.issue().expect("an issue");
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::AdvancedSearch(notice.clone()))
    );
    notice
}

#[rstest]
#[case("")]
#[case("   \t  ")]
fn an_empty_move_list_is_refused(#[case] text: &str) {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetMovesText(text.into()));
    assert_eq!(
        refused_notice(&mut rig, AdvancedKind::Moves),
        AdvancedSearchNotice::EmptyMoveList
    );
}

#[rstest]
#[case("?")]
#[case("? e5")]
#[case("  ?   e4 e5")]
fn a_wildcard_first_move_is_refused(#[case] text: &str) {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetMovesText(text.into()));
    assert_eq!(
        refused_notice(&mut rig, AdvancedKind::Moves),
        AdvancedSearchNotice::FirstMoveIsWildcard
    );
}

#[rstest]
#[case("Ng1-f3")]
#[case("e9")]
#[case("Zz4")]
#[case("e4 hello")]
#[case("e4 e5 e4 ??? foo")]
#[case("Qh4++")]
#[case("Qh4+#")]
#[case("Qh4#+")]
#[case("\u{0}")]
#[case("♔e4")]
fn a_word_that_is_not_san_is_refused(#[case] text: &str) {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetMovesText(text.into()));
    let notice = refused_notice(&mut rig, AdvancedKind::Moves);
    assert!(
        matches!(notice, AdvancedSearchNotice::BadMoveToken(_)),
        "{notice:?}"
    );
    assert!(notice.is_form_error());
}

#[rstest]
#[case("0-0")]
#[case("O-O")]
#[case("1. e4 e5")]
#[case("1.e4 1...e5")]
#[case("e4! e5?!")]
#[case("e8Q")]
#[case("exd5e.p.")]
#[case("e4 ? Nf3")]
#[case("e4\ne5")]
#[case("e4\te5")]
fn tolerant_san_is_accepted(#[case] text: &str) {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetMovesText(text.into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Moves),
    );
    let effects = rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Start));
    assert!(has_search(&effects), "{text:?}: {:?}", rig.issue());
}

#[test]
fn a_check_marker_raises_the_test_and_the_search_runs_anyway() {
    let (mut rig, slot, _dir) = sample();
    for (text, want) in [("Qh7+", CheckKind::Check), ("Qh7#", CheckKind::Mate)] {
        set(
            &mut rig,
            AdvancedSearchMessage::SetMovesCheck(CheckKind::No),
        );
        set(&mut rig, AdvancedSearchMessage::SetMovesText(text.into()));
        set(
            &mut rig,
            AdvancedSearchMessage::SelectTab(AdvancedKind::Moves),
        );
        let effects = rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Start));
        assert!(has_search(&effects), "{text}");
        let notice = rig.issue().expect("raised");
        assert_eq!(notice, AdvancedSearchNotice::CheckTestRaised(want));
        assert!(!notice.is_form_error());
        assert_eq!(rig.state.notice, Some(AppNotice::AdvancedSearch(notice)));
        assert!(rig.state.advanced_search().is_running());
        rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Stop));
        // The user's own choice is not overwritten by the raise.
        assert_eq!(rig.state.prefs.advanced_search.moves_check, CheckKind::No);
    }
    // A test that is high enough raises nothing.
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesCheck(CheckKind::Mate),
    );
    set(&mut rig, AdvancedSearchMessage::SetMovesText("Qh7+".into()));
    let effects = rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Start));
    assert!(has_search(&effects));
    assert!(rig.issue().is_none(), "{:?}", rig.issue());
    assert_eq!(rig.filter_numbers(slot).len(), 6);
}

#[rstest]
#[case(10, 5)]
#[case(2, 1)]
fn a_reversed_range_of_move_numbers_is_refused(#[case] min: u16, #[case] max: u16) {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetMoveRange(min, max));
    let notice = refused_notice(&mut rig, AdvancedKind::Material);
    assert_eq!(notice, AdvancedSearchNotice::InvalidMoveRange { min, max });
    assert!(notice.is_form_error());
}

#[test]
fn the_range_of_move_numbers_accepts_its_limits() {
    let (mut rig, _slot, _dir) = sample();
    for (min, max) in [(0, 0), (1, 1), (0, 999), (999, 999)] {
        set(&mut rig, AdvancedSearchMessage::SetMoveRange(min, max));
        set(
            &mut rig,
            AdvancedSearchMessage::SelectTab(AdvancedKind::Material),
        );
        let effects = rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Start));
        assert!(has_search(&effects), "({min},{max}): {:?}", rig.issue());
        rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Stop));
    }
}

#[test]
fn a_pattern_row_with_a_piece_and_no_square_is_refused_and_a_square_fixes_it() {
    let (mut rig, _slot, _dir) = sample();
    set(
        &mut rig,
        AdvancedSearchMessage::SetPatternPiece {
            index: 4,
            piece: Some(white(Role::Knight)),
        },
    );
    let notice = refused_notice(&mut rig, AdvancedKind::Material);
    assert!(matches!(notice, AdvancedSearchNotice::InvalidPattern(_)));
    assert!(notice.is_form_error());
    // A rank alone is enough.
    set(
        &mut rig,
        AdvancedSearchMessage::SetPatternRank {
            index: 4,
            rank: Some(Rank::Fifth),
        },
    );
    let effects = rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Start));
    assert!(has_search(&effects), "{:?}", rig.issue());
}

#[test]
fn all_the_notices_have_distinct_keys_as_listed_in_the_spec() {
    use AdvancedSearchNotice as N;
    let all = [
        (N::InvalidFen, "advsearch-notice-invalid-fen", true),
        (
            N::InvalidPattern("x"),
            "advsearch-notice-invalid-pattern",
            true,
        ),
        (
            N::InvalidMoveRange { min: 2, max: 1 },
            "advsearch-notice-invalid-move-range",
            true,
        ),
        (N::EmptyMoveList, "advsearch-notice-empty-moves", true),
        (
            N::FirstMoveIsWildcard,
            "advsearch-notice-first-wildcard",
            true,
        ),
        (
            N::BadMoveToken("x".into()),
            "advsearch-notice-bad-move",
            true,
        ),
        (
            N::CheckTestTooLow("Qh7+".into()),
            "advsearch-notice-check-test-too-low",
            true,
        ),
        (
            N::CheckTestRaised(CheckKind::Check),
            "advsearch-notice-check-test-raised",
            false,
        ),
        (
            N::UnsupportedPosition,
            "advsearch-notice-unsupported-position",
            true,
        ),
        (N::NoDatabase, "advsearch-notice-no-database", false),
        (N::Stale, "advsearch-notice-stale", false),
        (N::Interrupted, "advsearch-notice-interrupted", false),
        (
            N::Unreadable { count: 2 },
            "advsearch-notice-unreadable",
            false,
        ),
        (N::Failed, "advsearch-notice-failed", false),
        (N::NoSavedFolder, "advsearch-notice-no-saved-folder", false),
        (N::Saved("a".into()), "advsearch-notice-saved", false),
        (N::Deleted("a".into()), "advsearch-notice-deleted", false),
        (
            N::SavedNotFound("a".into()),
            "advsearch-notice-saved-not-found",
            false,
        ),
        (
            N::InvalidSavedName,
            "advsearch-notice-invalid-saved-name",
            false,
        ),
        (N::SavedFailed, "advsearch-notice-saved-failed", false),
        (
            N::Imported { warnings: 1 },
            "advsearch-notice-imported",
            false,
        ),
        (N::ImportFailed, "advsearch-notice-import-failed", false),
    ];
    let mut keys = std::collections::HashSet::new();
    for (notice, key, form) in &all {
        assert_eq!(notice.message_key(), *key, "{notice:?}");
        assert_eq!(notice.is_form_error(), *form, "{notice:?}");
        assert!(keys.insert(*key), "duplicate key {key}");
    }
    assert_eq!(keys.len(), 22);
}

#[test]
fn the_engine_errors_map_to_their_own_notices() {
    use AdvancedSearchNotice as N;
    let cases = [
        (E::EmptyMoveList, N::EmptyMoveList),
        (E::FirstMoveIsWildcard, N::FirstMoveIsWildcard),
        (
            E::BadMoveToken { token: "z".into() },
            N::BadMoveToken("z".into()),
        ),
        (
            E::InvalidMoveRange { min: 3, max: 1 },
            N::InvalidMoveRange { min: 3, max: 1 },
        ),
        (E::UnsupportedPosition, N::UnsupportedPosition),
    ];
    for (error, notice) in cases {
        assert_eq!(N::from(&error), notice);
    }
    let a = N::from(&E::EmptyMoveList).message_key();
    let b = N::from(&E::FirstMoveIsWildcard).message_key();
    assert_ne!(a, b);
}
use SearchError as E;

// ------------------------------------------------------- hostile input

#[test]
fn a_huge_or_hostile_moves_text_never_panics() {
    let (mut rig, _slot, _dir) = sample();
    let long = "e4 ".repeat(50_000);
    let texts = [
        long,
        "?".repeat(10_000),
        "a".repeat(100_000),
        "\u{202e}e4\u{200b}".to_owned(),
        "e4 e5 \u{0} Nf3".to_owned(),
        "%%%% !!!! ???? ++++".to_owned(),
        "e4;e5,Nf3".to_owned(),
    ];
    for text in texts {
        set(&mut rig, AdvancedSearchMessage::SetMovesText(text));
        set(
            &mut rig,
            AdvancedSearchMessage::SelectTab(AdvancedKind::Moves),
        );
        let effects = rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Start));
        // Either it searches or says why not; never both nor neither.
        assert!(
            has_search(&effects) ^ rig.issue().is_some_and(|i| i.is_form_error()),
            "{effects:?} {:?}",
            rig.issue()
        );
        rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Stop));
        drop(rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Progress)));
    }
}

#[test]
fn a_huge_text_of_the_fen_never_panics() {
    let (mut rig, _slot, _dir) = sample();
    set(
        &mut rig,
        AdvancedSearchMessage::SetFen("8/".repeat(100_000)),
    );
    let effects = rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Start));
    assert!(effects.is_empty());
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::InvalidFen));
}

// ----------------------------------------------------- the clamps (D3379)

#[rstest]
#[case(0, 0, 0, 0)]
#[case(9, 9, 9, 9)]
#[case(10, 10, 9, 9)]
#[case(255, 255, 9, 9)]
#[case(3, 200, 3, 9)]
#[case(7, 2, 7, 2)]
fn the_count_of_a_piece_is_kept_in_zero_to_nine(
    #[case] min: u8,
    #[case] max: u8,
    #[case] want_min: u8,
    #[case] want_max: u8,
) {
    let (mut rig, _slot, _dir) = sample();
    set(
        &mut rig,
        AdvancedSearchMessage::SetRange {
            color: Color::White,
            piece: MaterialPiece::Pawn,
            min,
            max,
        },
    );
    let view = rig.state.advanced_search_view().unwrap();
    let range = view.material.white.get(MaterialPiece::Pawn);
    // A reversed pair is the user's to fix: the engine decides on search.
    let (got_min, got_max) = (range.min, range.max);
    assert!(got_min <= 9 && got_max <= 9);
    assert_eq!(
        (got_min.min(got_max), got_min.max(got_max)),
        (want_min.min(want_max), want_min.max(want_max))
    );
    // The other colour and the other rows did not move.
    let black = view.material.black.get(MaterialPiece::Pawn);
    assert_eq!((black.min, black.max), (0, 8));
}

#[test]
fn a_reversed_count_range_does_not_panic_the_search() {
    let (mut rig, _slot, _dir) = sample();
    set(
        &mut rig,
        AdvancedSearchMessage::SetRange {
            color: Color::White,
            piece: MaterialPiece::Queen,
            min: 5,
            max: 1,
        },
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Material),
    );
    set(&mut rig, AdvancedSearchMessage::Start);
    assert!(!rig.state.advanced_search().is_running());
    set(&mut rig, AdvancedSearchMessage::SetDiff(Some((5, -5))));
    set(&mut rig, AdvancedSearchMessage::Start);
    assert!(!rig.state.advanced_search().is_running());
}

// ------------------------------------------------------------- prefs

#[rstest]
#[case(AdvancedKind::Position, SearchOp::And)]
#[case(AdvancedKind::Position, SearchOp::Or)]
#[case(AdvancedKind::Material, SearchOp::And)]
#[case(AdvancedKind::Material, SearchOp::Or)]
#[case(AdvancedKind::Moves, SearchOp::And)]
#[case(AdvancedKind::Moves, SearchOp::Or)]
fn the_operation_of_one_tab_survives_the_file_without_touching_the_others(
    #[case] kind: AdvancedKind,
    #[case] op: SearchOp,
) {
    let mut rig = Rig::new();
    rig.open_window(AdvancedKind::Position);
    set(&mut rig, AdvancedSearchMessage::SetOp(kind, op));
    let text = render_preferences(&rig.state.prefs);
    let loaded = load_preferences(&text);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    let mut again = Rig::new();
    again.send(Message::PreferencesLoaded(loaded));
    again.open_window(AdvancedKind::Position);
    let view = again.state.advanced_search_view().unwrap();
    for other in AdvancedKind::ALL {
        let shown = match other {
            AdvancedKind::Position => view.position.op,
            AdvancedKind::Material => view.material.op,
            AdvancedKind::Moves => view.moves.op,
        };
        assert_eq!(shown, if other == kind { op } else { SearchOp::Reset });
    }
}

#[test]
fn the_last_options_of_every_tab_survive_the_file() {
    let mut rig = Rig::new();
    rig.open_window(AdvancedKind::Material);
    set(&mut rig, AdvancedSearchMessage::SetBoardIgnoreColors(true));
    set(&mut rig, AdvancedSearchMessage::SetBoardVariations(true));
    set(
        &mut rig,
        AdvancedSearchMessage::SetBoardKind(BoardKind::Material),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetBishops(BishopsKind::Opposite),
    );
    set(&mut rig, AdvancedSearchMessage::SetEndOnly(true));
    set(
        &mut rig,
        AdvancedSearchMessage::SetMaterialIgnoreColors(true),
    );
    set(&mut rig, AdvancedSearchMessage::SetMoveRange(4, 60));
    set(&mut rig, AdvancedSearchMessage::SetMinPlies(7));
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesSide(SideKind::Black),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesCheck(CheckKind::Mate),
    );
    set(&mut rig, AdvancedSearchMessage::SetMovesVariations(true));
    let want = rig.state.advanced_search_view().unwrap();
    let text = render_preferences(&rig.state.prefs);
    let mut again = Rig::new();
    again.send(Message::PreferencesLoaded(load_preferences(&text)));
    again.open_window(AdvancedKind::Material);
    let got = again.state.advanced_search_view().unwrap();
    assert_eq!(got.position, want.position);
    assert_eq!(got.moves, want.moves);
    assert_eq!(
        (
            got.material.bishops,
            got.material.end_only,
            got.material.ignore_colors,
            got.material.move_min,
            got.material.move_max,
            got.material.min_plies
        ),
        (BishopsKind::Opposite, true, true, 4, 60, 7)
    );
    // What was typed is not in the file (D3379).
    assert_eq!(got.moves.text, "");
    assert_eq!(got.position.fen, None);
}

#[test]
fn a_changed_option_is_saved_at_once_and_the_same_value_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::in_dir(dir.path());
    rig.open_window(AdvancedKind::Position);
    // The same value as before: nothing to save.
    for message in [
        AdvancedSearchMessage::SetOp(AdvancedKind::Moves, SearchOp::Reset),
        AdvancedSearchMessage::SetBoardKind(BoardKind::Exact),
        AdvancedSearchMessage::SetMoveRange(1, 999),
    ] {
        let effects = rig.raw(Message::AdvancedSearch(message));
        assert!(effects.is_empty(), "{effects:?}");
    }
    // A new value is persisted by the effect of the preferences.
    let mut effects = rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::SetOp(
        AdvancedKind::Moves,
        SearchOp::Or,
    )));
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert!(matches!(effects[0], Effect::SavePreferences { .. }));
    drop(rig.perform(effects.remove(0)));
    let text = std::fs::read_to_string(dir.path().join("preferences.toml")).unwrap();
    let loaded = load_preferences(&text);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(loaded.prefs.advanced_search.moves_op, SearchOp::Or);
    assert_eq!(loaded.prefs.advanced_search.board_op, SearchOp::Reset);
}

#[rstest]
#[case("[advanced_search]\nboard_op = 5\n")]
#[case("[advanced_search]\nboard_op = \"AND\"\n")]
#[case("[advanced_search]\nboard_op = \"\"\n")]
#[case("[advanced_search]\nboard_kind = [\"exact\"]\n")]
#[case("[advanced_search]\nmaterial_move_min = -3\n")]
#[case("[advanced_search]\nmaterial_move_max = 99999999999999999999\n")]
#[case("[advanced_search]\nmaterial_min_plies = 1.5e300\n")]
#[case("[advanced_search]\nmaterial_end_only = \"yes\"\n")]
#[case("[advanced_search]\nmoves_check = \"MATE\"\n")]
#[case("[advanced_search]\nmoves_side = { a = 1 }\n")]
#[case("[advanced_search]\nunknown_key = 1\nmoves_variations = true\n")]
#[case("advanced_search = [1, 2, 3]\n")]
#[case("advanced_search = \"nope\"\n")]
#[case("[advanced_search]\nmaterial_bishops = \"same\"\nmaterial_bishops = \"opposite\"\n")]
#[case("[advanced_search\nboard_op = \"or\"\n")]
fn bad_values_in_the_file_fall_back_without_panic(#[case] text: &str) {
    let loaded = load_preferences(text);
    let prefs = loaded.prefs.advanced_search;
    // Whatever was read is inside its limits.
    assert!(prefs.material_move_min <= 999 && prefs.material_move_max <= 999);
    assert!(prefs.material_min_plies <= 99);
    // A bad value is the default of that field; the good ones are kept.
    let default = AdvancedSearchPrefs::default();
    if text.contains("board_op") && !text.contains("\"or\"") {
        assert_eq!(prefs.board_op, default.board_op);
    }
    if text.contains("moves_check") {
        assert_eq!(prefs.moves_check, default.moves_check);
    }
    if text.contains("unknown_key") {
        assert!(prefs.moves_variations);
    }
    // The app can start from it and search.
    let mut rig = Rig::new();
    rig.send(Message::PreferencesLoaded(loaded));
    rig.open_window(AdvancedKind::Position);
    let view = rig.state.advanced_search_view().unwrap();
    assert!(view.material.move_max <= 999 && view.material.min_plies <= 99);
    set(&mut rig, AdvancedSearchMessage::SetMovesText("e4".into()));
    for kind in AdvancedKind::ALL {
        set(&mut rig, AdvancedSearchMessage::SelectTab(kind));
        drop(rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Start)));
        rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Stop));
    }
}

#[test]
fn a_file_with_a_bad_value_reports_an_issue_per_bad_field() {
    let loaded = load_preferences(
        "[advanced_search]\nboard_op = 5\nmoves_side = \"x\"\nmoves_variations = true\n",
    );
    assert_eq!(loaded.issues.len(), 2, "{:?}", loaded.issues);
    assert!(loaded.prefs.advanced_search.moves_variations);
}

// ------------------------------------------------------- no-panic fuzz

fn form_message() -> impl Strategy<Value = AdvancedSearchMessage> {
    let kind = prop_oneof![
        Just(AdvancedKind::Position),
        Just(AdvancedKind::Material),
        Just(AdvancedKind::Moves)
    ];
    let op = prop_oneof![
        Just(SearchOp::Reset),
        Just(SearchOp::And),
        Just(SearchOp::Or)
    ];
    let piece = prop::sample::select(MaterialPiece::ALL.to_vec());
    let color = prop_oneof![Just(Color::White), Just(Color::Black)];
    let text = prop_oneof![
        Just(String::new()),
        Just("e4".to_owned()),
        Just("e4 ? Nf3".to_owned()),
        Just("? e4".to_owned()),
        Just("Qh7+".to_owned()),
        Just(AFTER_E4_E5.to_owned()),
        Just("garbage".to_owned()),
        ".{0,12}",
    ];
    prop_oneof![
        kind.clone().prop_map(AdvancedSearchMessage::SelectTab),
        (kind, op).prop_map(|(k, o)| AdvancedSearchMessage::SetOp(k, o)),
        text.clone().prop_map(AdvancedSearchMessage::SetFen),
        text.prop_map(AdvancedSearchMessage::SetMovesText),
        Just(AdvancedSearchMessage::UseCurrentPosition),
        (color, piece, any::<u8>(), any::<u8>()).prop_map(|(color, piece, min, max)| {
            AdvancedSearchMessage::SetRange {
                color,
                piece,
                min,
                max,
            }
        }),
        prop::option::of((any::<i16>(), any::<i16>())).prop_map(AdvancedSearchMessage::SetDiff),
        (any::<u16>(), any::<u16>()).prop_map(|(a, b)| AdvancedSearchMessage::SetMoveRange(a, b)),
        any::<u16>().prop_map(AdvancedSearchMessage::SetMinPlies),
        any::<bool>().prop_map(AdvancedSearchMessage::SetEndOnly),
        any::<bool>().prop_map(AdvancedSearchMessage::SetBoardVariations),
        any::<bool>().prop_map(AdvancedSearchMessage::SetMovesVariations),
        any::<usize>().prop_map(AdvancedSearchMessage::ApplyEnding),
        (0usize..16).prop_map(AdvancedSearchMessage::ApplyEnding),
        (0usize..16).prop_map(AdvancedSearchMessage::ApplyPatternPreset),
        Just(AdvancedSearchMessage::ClearPatterns),
        Just(AdvancedSearchMessage::MaterialZero),
        Just(AdvancedSearchMessage::MaterialAny),
        Just(AdvancedSearchMessage::MaterialCurrentBoard),
        (0usize..14, any::<bool>()).prop_map(|(index, present)| {
            AdvancedSearchMessage::SetPatternPresent { index, present }
        }),
        (
            0usize..14,
            prop::option::of(prop::sample::select(vec![File::A, File::D, File::H]))
        )
            .prop_map(|(index, file)| AdvancedSearchMessage::SetPatternFile { index, file }),
        (0usize..14, any::<bool>()).prop_map(|(index, on)| {
            AdvancedSearchMessage::SetPatternPiece {
                index,
                piece: on.then(|| white(Role::Pawn)),
            }
        }),
        Just(AdvancedSearchMessage::Start),
        Just(AdvancedSearchMessage::Start),
        Just(AdvancedSearchMessage::Stop),
        Just(AdvancedSearchMessage::Progress),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn any_sequence_of_edits_and_searches_keeps_the_state_consistent(
        messages in prop::collection::vec(form_message(), 1..25)
    ) {
        let (mut rig, slot, _dir) = sample();
        for message in messages {
            rig.send(Message::AdvancedSearch(message));
            let view = rig.state.advanced_search_view().unwrap();
            prop_assert_eq!(view.material.patterns.len(), PATTERN_ROWS);
            // (The ranges may pass 9 after a preset: the Scid normalises
            // the minor pieces from the bishops and the knights.)
            prop_assert!(view.material.move_max <= 999 && view.material.min_plies <= 99);
            // `send` answers every effect, so no search stays in flight.
            prop_assert!(view.running.is_none());
            // The filter always has the size of the database.
            let filter = rig.state.bases.slot(slot).unwrap().filter();
            prop_assert_eq!(filter.len(), 6);
            // A refused form never leaves a summary of a search that did
            // not run, and the notice and the issue agree.
            if let Some(issue) = view.issue {
                prop_assert!(issue.message_key().starts_with("advsearch-notice-"));
            }
        }
    }
}
