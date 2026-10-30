//! M8-3c (GUI): the Fluent files of the online play (en-US and pt-BR) have
//! the same keys, every text the code asks for exists, and every remark
//! and failure the application can report has a text in both languages
//! (spec `docs/specs/m8-3-lichess-online.md`, D1328, D1473).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::path::Path;

use mychess_app::online::{OnlineError, OnlineNotice, RejectReason};
use mychess_gui::i18n::{I18n, Language};

fn keys(language: &str) -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join("online.ftl");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with(' '))
        .filter_map(|l| l.split_once(" = "))
        .map(|(k, _)| k.trim().to_owned())
        .collect()
}

#[test]
fn both_languages_have_the_same_online_keys() {
    let en = keys("en-US");
    let pt = keys("pt-BR");
    assert!(en.len() > 150, "the file was read: {}", en.len());
    let only_en: Vec<_> = en.difference(&pt).collect();
    let only_pt: Vec<_> = pt.difference(&en).collect();
    assert!(only_en.is_empty(), "missing in pt-BR: {only_en:?}");
    assert!(only_pt.is_empty(), "missing in en-US: {only_pt:?}");
}

#[test]
fn every_key_the_code_uses_exists() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = [
        "windows/online_dialog.rs",
        "windows/online_panel.rs",
        "windows/online_text.rs",
        "windows/online_ui.rs",
        "windows/menu_bar.rs",
    ];
    let known = keys("en-US");
    let mut missing = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(src.join(file)).expect("source");
        let body = text.split("#[cfg(test)]").next().unwrap_or_default();
        for part in body.split('"').skip(1).step_by(2) {
            let looks_like_key = (part.starts_with("online-") || part.starts_with("menu-online"))
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            // Ids of widgets are not texts.
            let is_id = part.ends_with("-box")
                || part.ends_with("-banner")
                || matches!(
                    part,
                    "online-bar" | "online-token" | "online-seek-minutes" | "online-watch-input"
                );
            if looks_like_key && !is_id && !known.contains(part) {
                missing.push(format!("{file}: {part}"));
            }
        }
    }
    assert!(missing.is_empty(), "{missing:#?}");
}

#[test]
fn every_remark_and_failure_has_a_text_in_both_languages() {
    let rejects = [
        RejectReason::WrongState,
        RejectReason::NotConnected,
        RejectReason::Busy,
        RejectReason::GameInProgress,
        RejectReason::EventsNotReady,
        RejectReason::TimeControl,
        RejectReason::NotYourTurn,
        RejectReason::NoGame,
        RejectReason::IllegalMove,
        RejectReason::ChallengeNotPlayable,
        RejectReason::NotAvailable,
        RejectReason::BadUser,
        RejectReason::MovePending,
    ];
    let mut notices: Vec<OnlineNotice> = rejects.into_iter().map(OnlineNotice::Rejected).collect();
    notices.extend([
        OnlineNotice::AssistanceDisabled,
        OnlineNotice::AssistanceWillBeDisabled,
        OnlineNotice::Connected,
        OnlineNotice::SignedOut,
        OnlineNotice::Disconnected,
        OnlineNotice::AnotherGame,
        OnlineNotice::Resumed,
        OnlineNotice::UnsupportedVariant,
        OnlineNotice::MoveRejected {
            reason: "x".to_owned(),
        },
        OnlineNotice::MoveUncertain,
        OnlineNotice::Resynchronised,
        OnlineNotice::Desynced,
        OnlineNotice::GameLost,
        OnlineNotice::SearchTimedOut,
        OnlineNotice::ChallengeDeclined { reason: None },
        OnlineNotice::Imported,
        OnlineNotice::AlreadyImported,
        OnlineNotice::NotImported,
        OnlineNotice::ImportFailed,
        OnlineNotice::LockReleased,
        OnlineNotice::MarkerCleared,
        OnlineNotice::PreviousRestored,
        OnlineNotice::GameOver,
    ]);
    let errors = [
        OnlineError::Unauthorized,
        OnlineError::ScopeMissing,
        OnlineError::RateLimited { secs: 3 },
        OnlineError::Network,
        OnlineError::Timeout,
        OnlineError::NotFound,
        OnlineError::Rejected {
            reason: "x".to_owned(),
        },
        OnlineError::Protocol,
        OnlineError::BotAccount,
        OnlineError::InsecureUrl,
        OnlineError::NoToken,
        OnlineError::BadToken,
        OnlineError::TokenStore,
        OnlineError::TimeControl,
        OnlineError::Server,
        OnlineError::Other,
    ];
    notices.extend(errors.into_iter().map(OnlineNotice::Error));
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for notice in &notices {
            let key = notice.message_key();
            assert!(i18n.has(key), "{key} missing in {language:?}");
            let text = mychess_gui::windows::online_text::notice_text(&i18n, notice);
            assert!(!text.trim().is_empty() && text != key, "{key}: {text:?}");
            assert!(!text.contains("{$"), "{key} shows a placeholder: {text:?}");
        }
    }
}

#[test]
fn the_shortcut_actions_and_menu_labels_are_translated() {
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for key in [
            "menu-online",
            "menu-group-lichess",
            "menu-group-fics",
            "menu-online-connect",
            "menu-online-seek",
            "menu-online-challenge",
            "menu-online-incoming",
            "menu-online-disconnect",
            "menu-online-remove-lock",
            "action-online-connect",
            "action-online-seek",
        ] {
            assert!(i18n.has(key), "{key} missing in {language:?}");
        }
    }
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    assert_ne!(en.tr("online-resign"), pt.tr("online-resign"));
    assert_ne!(en.tr("online-abort"), pt.tr("online-abort"));
}
