//! M8-2b (GUI): the Fluent files of the tournament (en-US and pt-BR) have
//! the same keys, and every text the code asks for exists.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::path::Path;

fn keys(language: &str) -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join("tournament.ftl");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with(' '))
        .filter_map(|l| l.split_once(" = "))
        .map(|(k, _)| k.trim().to_owned())
        .collect()
}

#[test]
fn both_languages_have_the_same_tournament_keys() {
    let en = keys("en-US");
    let pt = keys("pt-BR");
    assert!(en.len() > 200, "the file was read: {}", en.len());
    let only_en: Vec<_> = en.difference(&pt).collect();
    let only_pt: Vec<_> = pt.difference(&en).collect();
    assert!(only_en.is_empty(), "missing in pt-BR: {only_en:?}");
    assert!(only_pt.is_empty(), "missing in en-US: {only_pt:?}");
}

#[test]
fn every_key_the_code_uses_exists() {
    // The keys written in the sources of the tournament screens.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = [
        "windows/tournament_dialog.rs",
        "windows/tournament_panel.rs",
        "windows/tournament_text.rs",
        "windows/tournament_ui.rs",
        "host.rs",
        "windows/menu_bar.rs",
    ];
    let known = keys("en-US");
    let mut missing = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(src.join(file)).expect("source");
        for part in text.split('"').skip(1).step_by(2) {
            let looks_like_key = part.starts_with("tournament-")
                && part.len() > "tournament-".len()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            // Ids of widgets are not texts.
            let is_id = part.ends_with("-box") || part == "tournament-pairings";
            if looks_like_key && !is_id && !known.contains(part) {
                missing.push(format!("{file}: {part}"));
            }
        }
    }
    assert!(missing.is_empty(), "keys without a text: {missing:#?}");
}

#[test]
fn the_keys_of_the_application_have_texts() {
    use mychess_app::tournament::{ConfigError, TournamentNotice};
    let known = keys("en-US");
    let notices = [
        TournamentNotice::Started,
        TournamentNotice::Finished { winners: vec![] },
        TournamentNotice::Paused,
        TournamentNotice::Resumed,
        TournamentNotice::StoppedAfterGame,
        TournamentNotice::Aborted,
        TournamentNotice::NotApplicable,
        TournamentNotice::GameNotSaved,
        TournamentNotice::StateNotSaved,
        TournamentNotice::DestinationReadOnly,
        TournamentNotice::PlayBusy,
        TournamentNotice::AlreadyRunning,
        TournamentNotice::Busy,
        TournamentNotice::MissingEngines(vec![]),
        TournamentNotice::UnsupportedTime(String::new()),
        TournamentNotice::ConcurrencyReduced { from: 8, to: 4 },
        TournamentNotice::DestinationFailed,
        TournamentNotice::DestinationNotReopened,
        TournamentNotice::DestinationUnavailable,
        TournamentNotice::BookUnreadable,
        TournamentNotice::SuiteUnreadable,
        TournamentNotice::ResumeFailed,
        TournamentNotice::TablebaseUnavailable,
        TournamentNotice::Exported,
        TournamentNotice::ExportFailed,
        TournamentNotice::InvalidConfig(ConfigError::TooFewParticipants),
        TournamentNotice::InvalidConfig(ConfigError::TooManyParticipants),
        TournamentNotice::InvalidConfig(ConfigError::EmptyName(0)),
        TournamentNotice::InvalidConfig(ConfigError::DuplicateName(String::new())),
        TournamentNotice::InvalidConfig(ConfigError::BadCycles),
        TournamentNotice::InvalidConfig(ConfigError::BadTested),
        TournamentNotice::InvalidConfig(ConfigError::BadTime("")),
        TournamentNotice::InvalidConfig(ConfigError::BadWatchdog("")),
        TournamentNotice::InvalidConfig(ConfigError::MissingOpeningData("")),
        TournamentNotice::InvalidConfig(ConfigError::BadStartFen),
        TournamentNotice::InvalidConfig(ConfigError::BadBookLimit),
        TournamentNotice::InvalidConfig(ConfigError::BadSeed),
        TournamentNotice::InvalidConfig(ConfigError::TitleTooLong),
        TournamentNotice::InvalidConfig(ConfigError::BadAdjudication("")),
        TournamentNotice::InvalidConfig(ConfigError::BadFailureLimit),
        TournamentNotice::InvalidConfig(ConfigError::TooManyGames),
        TournamentNotice::InvalidConfig(ConfigError::BadConcurrency),
        TournamentNotice::InvalidConfig(ConfigError::BookNotFound(std::path::PathBuf::default())),
        TournamentNotice::InvalidConfig(
            ConfigError::SuiteUnreadable(std::path::PathBuf::default()),
        ),
    ];
    for notice in notices {
        assert!(known.contains(notice.message_key()), "{notice:?}");
    }
}
