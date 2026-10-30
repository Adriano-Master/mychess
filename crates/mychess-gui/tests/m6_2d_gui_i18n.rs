//! M6-2d (GUI): every text the window of the search for tactics and
//! patterns asks for exists in both languages, the sentences of the cards
//! read as sentences, and the Search menu has the item that opens the window
//! (spec M6-2, "Textos de interface", D3288).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::time::Duration;

use mychess_app::pattern_search::model::{ParamKey, Region, Sq};
use mychess_app::pattern_search::{
    ConditionGroup, ConditionKind, EditRefusal, GroupKind, PatternIssue, PatternRunSummary,
    PatternSearchNotice, Sentence, SentenceArg, describe, describe_region, param_label_key,
};
use mychess_app::{Action, SlotId};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::pattern_search_text::{
    notice_text, region_text, sentence_text, summary_text,
};
use mychess_gui::{Message, Shell, UiMessage};

fn collect_keys(sentence: &Sentence, keys: &mut BTreeSet<String>) {
    keys.insert(sentence.key.to_owned());
    for (_, arg) in &sentence.args {
        collect_arg(arg, keys);
    }
}

fn collect_arg(arg: &SentenceArg, keys: &mut BTreeSet<String>) {
    match arg {
        SentenceArg::Key(key) => {
            keys.insert((*key).to_owned());
        }
        SentenceArg::List(items) => items.iter().for_each(|i| collect_arg(i, keys)),
        SentenceArg::Phrase(inner) => collect_keys(inner, keys),
        SentenceArg::Text(_) | SentenceArg::Number(_) => {}
    }
}

fn notices() -> Vec<PatternSearchNotice> {
    let issue = || PatternIssue::InvalidValue {
        path: "root.items[2]".to_owned(),
    };
    vec![
        PatternSearchNotice::InvalidPattern(issue()),
        PatternSearchNotice::InvalidPattern(PatternIssue::EmptyGroup {
            path: "root".to_owned(),
        }),
        PatternSearchNotice::InvalidPattern(PatternIssue::TooDeep { max: 6 }),
        PatternSearchNotice::InvalidPattern(PatternIssue::TooManyNodes { max: 48 }),
        PatternSearchNotice::InvalidPattern(PatternIssue::MoverNotAllowed {
            path: "root".to_owned(),
        }),
        PatternSearchNotice::InvalidParam("n".to_owned()),
        PatternSearchNotice::UnknownPreset("zzz".to_owned()),
        PatternSearchNotice::EditRefused(EditRefusal::BadPath),
        PatternSearchNotice::NoDatabase,
        PatternSearchNotice::Stale,
        PatternSearchNotice::Interrupted,
        PatternSearchNotice::Unreadable { count: 7 },
        PatternSearchNotice::UnknownPositions { count: 4 },
        PatternSearchNotice::Failed,
        PatternSearchNotice::HitsFailed,
        PatternSearchNotice::NoSavedFolder,
        PatternSearchNotice::Saved("mine".to_owned()),
        PatternSearchNotice::Deleted("mine".to_owned()),
        PatternSearchNotice::SavedNotFound("mine".to_owned()),
        PatternSearchNotice::InvalidSavedName,
        PatternSearchNotice::SavedFailed,
        PatternSearchNotice::Imported("mine".to_owned()),
        PatternSearchNotice::ImportFailed,
        PatternSearchNotice::Exported,
        PatternSearchNotice::ExportFailed,
        PatternSearchNotice::FileUnreadable,
        PatternSearchNotice::NewerVersion,
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
            let must: Vec<&str> = match &notice {
                PatternSearchNotice::InvalidPattern(PatternIssue::InvalidValue { .. }) => {
                    vec!["root.items[2]"]
                }
                PatternSearchNotice::InvalidParam(_) => vec!["n"],
                PatternSearchNotice::UnknownPreset(_) => vec!["zzz"],
                PatternSearchNotice::Unreadable { .. } => vec!["7"],
                PatternSearchNotice::UnknownPositions { .. } => vec!["4"],
                PatternSearchNotice::Saved(n)
                | PatternSearchNotice::Deleted(n)
                | PatternSearchNotice::SavedNotFound(n)
                | PatternSearchNotice::Imported(n) => vec![n.as_str()],
                _ => vec![],
            };
            for part in must {
                assert!(text.contains(part), "{key}: {text}");
            }
        }
    }
}

#[test]
fn every_refusal_of_an_edit_has_a_text() {
    for refusal in [
        EditRefusal::BadPath,
        EditRefusal::NotAGroup,
        EditRefusal::RootCannotBeRemoved,
        EditRefusal::TooDeep,
        EditRefusal::TooManyNodes,
        EditRefusal::GroupFull,
        EditRefusal::MoveIntoItself,
        EditRefusal::NeedsOneItem,
        EditRefusal::NoSuchRegion,
        EditRefusal::NoMarks,
    ] {
        for language in Language::ALL {
            assert!(
                I18n::new(language).has(refusal.message_key()),
                "{} in {language:?}",
                refusal.message_key()
            );
        }
    }
}

#[test]
fn the_keys_of_the_model_exist_in_both_languages() {
    let mut keys: BTreeSet<String> = BTreeSet::new();
    for kind in ConditionKind::ALL {
        keys.insert(kind.title_key().to_owned());
        keys.insert(kind.group().title_key().to_owned());
        collect_keys(&describe(&kind.default_condition()), &mut keys);
    }
    for group in GroupKind::ALL {
        keys.insert(group.title_key().to_owned());
    }
    for group in [
        ConditionGroup::Logic,
        ConditionGroup::Pieces,
        ConditionGroup::Relations,
        ConditionGroup::Tactics,
        ConditionGroup::Structure,
        ConditionGroup::Moves,
    ] {
        keys.insert(group.title_key().to_owned());
    }
    for region in [
        Region::Anywhere,
        Region::Squares(vec![Sq(mychess_core::Square::E4)]),
        Region::File('d'),
        Region::Rank(7),
        Region::OwnRank(7),
        Region::Rect {
            from: Sq(mychess_core::Square::C3),
            to: Sq(mychess_core::Square::F6),
        },
    ] {
        collect_keys(&describe_region(&region), &mut keys);
    }
    for key in [
        ParamKey::By,
        ParamKey::N,
        ParamKey::MateMode,
        ParamKey::ForcingOnly,
        ParamKey::MaxNodes,
        ParamKey::Roles,
        ParamKey::TargetRoles,
        ParamKey::AgainstKing,
        ParamKey::Discovery,
        ParamKey::MinNet,
        ParamKey::Outcome,
        ParamKey::Within,
        ParamKey::RequireWin,
        ParamKey::MinOwnRank,
    ] {
        keys.insert(param_label_key(key).to_owned());
    }
    let mut shell = Shell::new(None);
    drop(shell.update(Message::Ui(UiMessage::Action(Action::SearchPattern))));
    let view = shell.app.pattern_search_view().expect("open");
    assert_eq!(view.presets.len(), 21);
    for preset in &view.presets {
        keys.insert(preset.name_key.to_owned());
        keys.insert(format!("{}-desc", preset.name_key));
    }
    for key in [
        "menu-search-pattern",
        "action-pattern-search",
        "pattern-list-and",
        "pattern-list-or",
        "pattern-mate-forcing-only",
        "pattern-region-anywhere",
        "pattern-role-any",
        "pattern-role-minor",
        "pattern-flag-both",
    ] {
        keys.insert(key.to_owned());
    }
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for key in &keys {
            assert!(i18n.has(key), "{key} missing in {language:?}");
        }
    }
}

#[test]
fn a_pin_reads_as_a_sentence_in_portuguese_with_no_raw_keys() {
    let i18n = I18n::new(Language::PtBr);
    let text = sentence_text(&i18n, &describe(&ConditionKind::Pins.default_condition()));
    assert!(text.contains("crava"), "{text}");
    assert!(text.contains("contra rei"), "{text}");
    assert!(!text.contains("pattern-") && !text.contains('{'), "{text}");
    let en = sentence_text(
        &I18n::new(Language::EnUs),
        &describe(&ConditionKind::Pins.default_condition()),
    );
    assert!(en.contains("pins") && en.contains("against king"), "{en}");
}

#[test]
fn ranges_lists_and_regions_are_joined_in_the_language() {
    let pt = I18n::new(Language::PtBr);
    let mate = sentence_text(&pt, &describe(&ConditionKind::Mate.default_condition()));
    assert!(mate.contains("mate em 1 lance"), "{mate}");
    let results = sentence_text(&pt, &describe(&ConditionKind::Result.default_condition()));
    assert!(results.starts_with("Resultado da partida"), "{results}");
    let squares = region_text(
        &pt,
        &Region::Squares(vec![
            Sq(mychess_core::Square::E4),
            Sq(mychess_core::Square::D4),
        ]),
    );
    assert_eq!(squares, "em e4, d4");
    assert_eq!(
        region_text(&I18n::new(Language::EnUs), &Region::File('d')),
        "on file d"
    );
}

#[test]
fn the_summary_line_shows_the_counts_the_time_and_the_problems() {
    let summary = PatternRunSummary {
        slot: SlotId::all().next().expect("a slot"),
        matched: 12,
        start_count: 345,
        elapsed: Duration::from_millis(1500),
        unreadable: 2,
        unknown_positions: 5,
        positions_examined: 1000,
        games_folded_out: 0,
    };
    let en = summary_text(&I18n::new(Language::EnUs), &summary);
    assert!(en.contains("12 / 345") && en.contains("1.50"), "{en}");
    assert!(
        en.contains("2 unreadable") && en.contains("5 positions"),
        "{en}"
    );
    let pt = summary_text(&I18n::new(Language::PtBr), &summary);
    assert!(pt.contains("ileg") && pt.contains("complexas"), "{pt}");
}
