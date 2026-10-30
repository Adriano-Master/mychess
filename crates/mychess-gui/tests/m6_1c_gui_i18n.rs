//! M6-1c (GUI): every text the window of the advanced searches asks for
//! exists in both languages, the notices carry their arguments and the
//! Search menu has the three items (spec M6-1, "Textos de interface").
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use mychess_app::{Action, AdvancedKind, AdvancedSearchNotice, CheckKind, SearchSummary, SlotId};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::advanced_search_text::{notice_text, summary_text};
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::{Message, Shell, UiMessage, ViewOptions};

fn notices() -> Vec<AdvancedSearchNotice> {
    vec![
        AdvancedSearchNotice::InvalidFen,
        AdvancedSearchNotice::InvalidPattern("reason"),
        AdvancedSearchNotice::InvalidMoveRange { min: 5, max: 2 },
        AdvancedSearchNotice::EmptyMoveList,
        AdvancedSearchNotice::FirstMoveIsWildcard,
        AdvancedSearchNotice::BadMoveToken("Zz9".to_owned()),
        AdvancedSearchNotice::CheckTestTooLow("Qh7+".to_owned()),
        AdvancedSearchNotice::CheckTestRaised(CheckKind::Mate),
        AdvancedSearchNotice::UnsupportedPosition,
        AdvancedSearchNotice::NoDatabase,
        AdvancedSearchNotice::Stale,
        AdvancedSearchNotice::Interrupted,
        AdvancedSearchNotice::Unreadable { count: 7 },
        AdvancedSearchNotice::Failed,
        AdvancedSearchNotice::NoSavedFolder,
        AdvancedSearchNotice::Saved("mine".to_owned()),
        AdvancedSearchNotice::Deleted("mine".to_owned()),
        AdvancedSearchNotice::SavedNotFound("mine".to_owned()),
        AdvancedSearchNotice::InvalidSavedName,
        AdvancedSearchNotice::SavedFailed,
        AdvancedSearchNotice::Imported { warnings: 3 },
        AdvancedSearchNotice::ImportFailed,
    ]
}

#[test]
fn every_notice_has_a_text_in_both_languages_with_its_arguments() {
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for notice in notices() {
            let key = notice.message_key();
            assert!(i18n.has(key), "{key} missing in {language:?}");
            let text = notice_text(&i18n, &notice);
            assert!(!text.is_empty() && text != key, "{key}: {text}");
            let must = match &notice {
                AdvancedSearchNotice::InvalidMoveRange { .. } => vec!["5", "2"],
                AdvancedSearchNotice::BadMoveToken(t)
                | AdvancedSearchNotice::CheckTestTooLow(t) => {
                    vec![t.as_str()]
                }
                AdvancedSearchNotice::Unreadable { .. } => vec!["7"],
                AdvancedSearchNotice::Saved(n)
                | AdvancedSearchNotice::Deleted(n)
                | AdvancedSearchNotice::SavedNotFound(n) => vec![n.as_str()],
                AdvancedSearchNotice::Imported { .. } => vec!["3"],
                _ => vec![],
            };
            for part in must {
                assert!(text.contains(part), "{key}: {text}");
            }
        }
    }
}

#[test]
fn the_summary_line_shows_the_counts_the_time_and_the_unreadable_games() {
    let summary = SearchSummary {
        kind: AdvancedKind::Position,
        slot: SlotId::all().next().expect("a slot"),
        matched: 12,
        start_count: 345,
        elapsed: Duration::from_millis(1500),
        unreadable: 2,
    };
    let en = summary_text(&I18n::new(Language::EnUs), &summary);
    assert!(en.contains("12 / 345") && en.contains("1.50"), "{en}");
    assert!(en.contains("2 unreadable"), "{en}");
    let pt = summary_text(&I18n::new(Language::PtBr), &summary);
    assert!(pt.contains("2 ileg"), "{pt}");
}

#[test]
fn the_texts_of_the_window_exist_in_both_languages() {
    let mut shell = Shell::new(None);
    drop(shell.update(Message::Ui(UiMessage::Action(Action::SearchMaterial))));
    let view = shell.app.advanced_search_view().expect("open");
    let mut keys: Vec<String> = Vec::new();
    for preset in view.endings.iter().chain(view.patterns.iter()) {
        keys.push(format!("search-{}", preset.key));
    }
    assert_eq!(view.endings.len(), 13);
    assert_eq!(view.patterns.len(), 14);
    for tip in ["exact", "pawns", "files", "material"] {
        keys.push(format!("advsearch-board-{tip}-tip"));
    }
    for piece in [
        "wk", "wq", "wr", "wb", "wn", "wp", "bk", "bq", "br", "bb", "bn", "bp",
    ] {
        keys.push(format!("advsearch-piece-{piece}"));
    }
    for piece in ["queen", "rook", "bishop", "knight", "minor", "pawn"] {
        keys.push(format!("advsearch-material-{piece}"));
    }
    for key in [
        "advsearch-check-no",
        "advsearch-check-check",
        "advsearch-check-mate",
    ] {
        keys.push(key.to_owned());
    }
    for key in [
        "action-search-board",
        "action-search-material",
        "action-search-moves",
        "menu-search",
        "advsearch-sso-dialog-title",
        "advsearch-sso-filter",
        "advsearch-progress",
        "advsearch-result",
    ] {
        keys.push(key.to_owned());
    }
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for key in &keys {
            assert!(i18n.has(key), "{key} missing in {language:?}");
        }
    }
}

#[test]
fn the_search_menu_has_the_items_that_open_the_windows() {
    let shell = Shell::new(None);
    let specs = menu_specs(&shell.app, &shell.ui, ViewOptions::default());
    let menu = specs
        .iter()
        .find(|s| s.label_key == "menu-search")
        .expect("the Search menu");
    let items: Vec<(&str, bool)> = menu
        .entries
        .iter()
        .map(|e| match e {
            MenuEntry::Item {
                label: Label::Key(k),
                shortcut,
                action,
                ..
            } => {
                assert!(shortcut.is_none(), "no default key (D3249)");
                (*k, action.is_some())
            }
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(
        items,
        [
            ("menu-search-board", true),
            ("menu-search-material", true),
            ("menu-search-moves", true),
            // Spec M6-2d.
            ("menu-search-pattern", true)
        ]
    );
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for (key, _) in &items {
            assert!(i18n.has(key), "{key} in {language:?}");
        }
    }
}
