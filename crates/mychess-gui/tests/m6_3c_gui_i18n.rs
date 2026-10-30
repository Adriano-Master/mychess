//! M6-3c (GUI): every text the player tools ask for exists in both
//! languages with its arguments, the notices of the application have their
//! text, and the Portuguese says "variante" never "variação" (spec M6-3,
//! "Textos de interface", criterion 45).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use mychess_app::{
    AppNotice, BaseError, FinderNotice, NameKind, PlayerInfoNotice, SpellingCounts,
    SpellingLoadError, SpellingNotice,
};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::player_tools::text::{
    finder_notice_text, info_notice_text, kind_key, life, percent, spelling_notice_text,
};
use mychess_gui::windows::status_bar::app_notice_text;

fn spelling_notices() -> Vec<SpellingNotice> {
    vec![
        SpellingNotice::Loaded(SpellingCounts {
            players: 11,
            events: 22,
            sites: 33,
            rounds: 44,
            ..SpellingCounts::default()
        }),
        SpellingNotice::LoadFailed(SpellingLoadError::NotAFile),
        SpellingNotice::LoadFailed(SpellingLoadError::TooLarge),
        SpellingNotice::LoadFailed(SpellingLoadError::Gzip),
        SpellingNotice::LoadFailed(SpellingLoadError::UnclosedQuote { line: 77 }),
        SpellingNotice::LoadFailed(SpellingLoadError::Cancelled),
        SpellingNotice::LoadFailed(SpellingLoadError::Internal),
        SpellingNotice::NotLoaded,
        SpellingNotice::NoEloData,
        SpellingNotice::ReadOnly,
        SpellingNotice::Stale,
        SpellingNotice::NoBase,
        SpellingNotice::NoneSelected,
        SpellingNotice::ScanInterrupted,
        SpellingNotice::Applied {
            kind: NameKind::Site,
            names: 5,
            times: 66,
            skipped_by_date: 3,
            not_found: 2,
            interrupted: true,
        },
        SpellingNotice::EloAdded {
            ratings: 8,
            games: 9,
        },
        SpellingNotice::EloNothing,
        SpellingNotice::GameNotRefreshed,
        SpellingNotice::Failed(BaseError::ReadOnly),
    ]
}

#[test]
fn every_spelling_notice_has_a_text_with_its_arguments_in_both_languages() {
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for notice in spelling_notices() {
            let key = notice.message_key();
            assert!(i18n.has(key), "{key} missing in {language:?}");
            let text = spelling_notice_text(&i18n, &notice);
            assert!(
                !text.is_empty() && text != key && !text.contains('{'),
                "{key}: {text}"
            );
            let must: Vec<&str> = match &notice {
                SpellingNotice::Loaded(_) => vec!["11", "22", "33", "44"],
                SpellingNotice::LoadFailed(SpellingLoadError::UnclosedQuote { .. }) => vec!["77"],
                SpellingNotice::Applied { .. } => vec!["5", "66", "3", "2"],
                SpellingNotice::EloAdded { .. } => vec!["8", "9"],
                _ => vec![],
            };
            for needle in must {
                assert!(text.contains(needle), "{language:?} {key}: {text}");
            }
        }
    }
}

#[test]
fn the_finder_and_information_notices_have_their_texts() {
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for notice in [
            FinderNotice::NoBase,
            FinderNotice::Stale,
            FinderNotice::Filtered { games: 17 },
            FinderNotice::Failed,
        ] {
            let text = finder_notice_text(&i18n, notice);
            assert!(
                i18n.has(notice.message_key()) && !text.contains('{'),
                "{text}"
            );
            if let FinderNotice::Filtered { .. } = notice {
                assert!(text.contains("17"), "{text}");
            }
        }
        for notice in [
            PlayerInfoNotice::NoBase,
            PlayerInfoNotice::NoPlayer,
            PlayerInfoNotice::BadLink,
            PlayerInfoNotice::NoFide,
            PlayerInfoNotice::FideFailed,
            PlayerInfoNotice::FilterSet { games: 23 },
        ] {
            let text = info_notice_text(&i18n, notice);
            assert!(
                i18n.has(notice.message_key()) && !text.contains('{'),
                "{text}"
            );
            if let PlayerInfoNotice::FilterSet { .. } = notice {
                assert!(text.contains("23"), "{text}");
            }
        }
    }
}

#[test]
fn the_status_bar_shows_the_notices_of_the_tools() {
    let i18n = I18n::new(Language::PtBr);
    let text = app_notice_text(&i18n, &AppNotice::Spelling(SpellingNotice::NotLoaded));
    assert!(text.contains("ortografia"), "{text}");
    let text = app_notice_text(
        &i18n,
        &AppNotice::Finder(FinderNotice::Filtered { games: 3 }),
    );
    assert!(text.contains('3') && text.contains("torneio"), "{text}");
    let text = app_notice_text(&i18n, &AppNotice::PlayerInfo(PlayerInfoNotice::NoPlayer));
    assert!(text.contains("jogador"), "{text}");
}

#[test]
fn the_keys_of_the_windows_exist_in_both_languages() {
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for key in [
            "menu-group-players",
            "menu-group-statistics",
            "menu-stats-find-players",
            "menu-stats-find-tournaments",
            "menu-stats-player-info",
            "menu-stats-spell",
            "menu-stats-spell-player",
            "menu-stats-spell-event",
            "menu-stats-spell-site",
            "menu-stats-spell-round",
            "menu-stats-add-elo",
            "menu-file-load-spelling",
            "menu-file-reload-spelling",
            "menu-file-unload-spelling",
            "menu-file-cancel-spelling-load",
            "action-player-finder",
            "action-tournament-finder",
            "action-player-info",
            "file-dialog-open-spelling",
            "file-filter-spelling",
            "pfinder-col-oldest",
            "pfinder-col-newest",
            "tmt-none",
            "tmt-col-winner",
            "pinfo-lookup",
            "pinfo-edit-ratings",
            "spell-confirm",
            "spell-elo-description",
            "games-job-spell-apply",
            "games-job-elo-add",
        ] {
            assert!(i18n.has(key), "{key} missing in {language:?}");
        }
        for kind in [
            NameKind::Player,
            NameKind::Event,
            NameKind::Site,
            NameKind::Round,
        ] {
            assert!(i18n.has(kind_key(kind)), "{kind:?}");
        }
    }
}

#[test]
fn the_helpers_of_the_texts_follow_the_spec() {
    assert_eq!(percent(&I18n::new(Language::EnUs), 8750), "87.50%");
    assert_eq!(percent(&I18n::new(Language::PtBr), 8750), "87,50%");
    let died = mychess_core::PartialDate::parse_lenient("1979.08.15");
    assert_eq!(
        life(mychess_core::PartialDate::UNKNOWN, died),
        "--1979.08.15"
    );
}

#[test]
fn the_portuguese_texts_of_the_tools_are_translated_and_never_say_variacao() {
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    for key in [
        "pfinder-col-games",
        "tmt-col-date",
        "pinfo-all",
        "pinfo-openings-white",
        "spell-title",
        "spell-add-elo",
        "menu-stats-find-players",
    ] {
        assert_ne!(en.tr(key), pt.tr(key), "{key}");
        assert!(!pt.tr(key).to_lowercase().contains("variaç"), "{key}");
    }
    assert_eq!(pt.tr("pfinder-col-oldest"), "Mais antiga");
    assert_eq!(pt.tr("tmt-none"), "Nenhum torneio encontrado.");
    assert_eq!(pt.tr("pinfo-edit-name"), "Editar nome");
}
