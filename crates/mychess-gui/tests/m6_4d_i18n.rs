//! M6-4d (GUI, spec D3392, D3413): every key the application or the panels
//! ask for exists in both languages, the words sent to the application are
//! translated, and the texts of the panels follow the language.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use mychess_app::charts::ChartsNotice;
use mychess_app::crosstable::{CrossWarning, CrosstableNotice};
use mychess_app::piece_tracker::{PieceTrackerNotice, TrackerScheme};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::crosstable_panel::labels;
use mychess_gui::windows::ptracker_panel::scheme_key;

fn both(key: &str) {
    for language in Language::ALL {
        assert!(
            I18n::new(language).has(key),
            "{key} missing in {language:?}"
        );
    }
}

#[test]
fn the_keys_the_application_emits_exist_in_both_languages() {
    for notice in [
        CrosstableNotice::NoGame,
        CrosstableNotice::NoTable,
        CrosstableNotice::InvalidGame,
        CrosstableNotice::Failed,
        CrosstableNotice::Busy,
        CrosstableNotice::Stale,
        CrosstableNotice::FilterSet,
        CrosstableNotice::FilterAdded,
        CrosstableNotice::Exported,
        CrosstableNotice::ExportFailed,
        CrosstableNotice::Copied,
        CrosstableNotice::Cancelled,
        CrosstableNotice::ExportStale,
    ] {
        both(notice.message_key());
    }
    for warning in [
        CrossWarning::PlayerLimit,
        CrossWarning::TooManyPlayers,
        CrossWarning::GameLimit,
        CrossWarning::Unreadable(3),
        CrossWarning::LargeOutput,
    ] {
        both(warning.message_key());
    }
    for notice in [
        ChartsNotice::StatsFailed,
        ChartsNotice::GraphFailed,
        ChartsNotice::RatingFailed,
        ChartsNotice::Cancelled,
        ChartsNotice::TooManyPlayers,
        ChartsNotice::NoPlayers,
    ] {
        both(notice.message_key());
    }
    for notice in [PieceTrackerNotice::Failed, PieceTrackerNotice::NotAPiece] {
        both(notice.message_key());
    }
    for scheme in TrackerScheme::ALL {
        both(scheme_key(*scheme));
    }
    for key in [
        "action-toggle-crosstable-pane",
        "action-toggle-stats-pane",
        "action-toggle-rating-pane",
        "action-toggle-piece-tracker-pane",
        "graph-title-relative",
        "graph-title-absolute",
        "menu-stats",
    ] {
        both(key);
    }
}

#[test]
fn the_action_keys_are_those_of_the_shortcut_table() {
    for action in [
        mychess_app::Action::ToggleCrosstablePane,
        mychess_app::Action::ToggleStatsPane,
        mychess_app::Action::ToggleRatingPane,
        mychess_app::Action::TogglePieceTrackerPane,
    ] {
        both(action.message_key());
    }
}

#[test]
fn the_labels_sent_to_the_application_are_translated() {
    let en = labels(&I18n::new(Language::EnUs));
    let pt = labels(&I18n::new(Language::PtBr));
    assert_eq!(en, mychess_app::crosstable::CrossLabels::default());
    assert_ne!(pt, en);
    assert_eq!(pt.player, "Jogador");
    assert_eq!(pt.games, "partidas");
    assert_eq!(pt.average_rating, "Rating médio");
    assert!(!pt.score.is_empty() && !pt.tie.is_empty());
}

#[test]
fn the_summary_line_has_the_scid_shape_in_english() {
    let en = I18n::new(Language::EnUs);
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("n", 3);
    args.set("w", 2);
    args.set("b", 0);
    args.set("d", 1);
    assert_eq!(
        en.tr_args("crosstab-games-total", &args),
        "3 games: +2 -0 =1"
    );
}

#[test]
fn portuguese_says_tabela_cruzada_and_never_variacao() {
    let pt = I18n::new(Language::PtBr);
    assert_eq!(pt.tr("pane-crosstable"), "Tabela cruzada");
    for key in ["ptracker-skipped", "stats-skipped", "crosstab-too-many"] {
        assert!(!pt.tr(key).to_lowercase().contains("variaç"));
    }
}
