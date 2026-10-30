//! Texts of the copy between databases in both languages (spec M3-4, D587):
//! key parity, named ends, plurals, no leftover placeholders.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

mod m3_4d_common;

use std::collections::BTreeMap;
use std::path::PathBuf;

use iced::Settings;
use iced_test::Simulator;
use m3_4d_common::*;
use mychess_app::{AppNotice, BaseError, BasesNotice, CopyEnd, CopyRefusal, CopyScope, SlotId};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::status_bar::app_notice_text;

fn ftl(language: &str, file: &str) -> BTreeMap<String, Vec<String>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join(file);
    let text = std::fs::read_to_string(&path).expect("ftl file");
    // key -> variables used in its message (including its select lines).
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let is_key = line.split_once(" = ").filter(|(k, _)| {
            !k.is_empty()
                && k.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        });
        if let Some((key, _)) = is_key {
            current = Some(key.to_owned());
            out.entry(key.to_owned()).or_default();
        }
        if let Some(key) = &current {
            let vars = out.get_mut(key).unwrap();
            let mut rest = line;
            while let Some(at) = rest.find('$') {
                let name: String = rest[at + 1..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                    .collect();
                if !vars.contains(&name) {
                    vars.push(name);
                }
                rest = &rest[at + 1..];
            }
        }
    }
    for vars in out.values_mut() {
        vars.sort();
    }
    out
}

const NEW_KEYS: &[(&str, &str)] = &[
    ("status.ftl", "base-notice-copy-same"),
    ("status.ftl", "base-notice-copy-source-closed"),
    ("status.ftl", "base-notice-copy-target-closed"),
    ("status.ftl", "base-notice-copy-target-read-only"),
    ("status.ftl", "base-notice-copy-empty-filter"),
    ("status.ftl", "base-notice-copy-nothing-selected"),
    ("status.ftl", "base-notice-filter-copied"),
    ("status.ftl", "base-notice-copy-cancelled"),
    ("status.ftl", "base-notice-copy-failed"),
    ("status.ftl", "base-error-name-limit"),
    ("dialogs.ftl", "dialog-copy-title"),
    ("dialogs.ftl", "dialog-copy-message"),
    ("dialogs.ftl", "dialog-copy-selected-message"),
    ("dialogs.ftl", "dialog-copy-yes"),
    ("games.ftl", "games-drag-hint"),
    ("games.ftl", "games-slot-number"),
    ("games.ftl", "games-job-copy-games"),
    ("games.ftl", "games-menu-tab-read-only"),
    ("games.ftl", "games-menu-confirm-copy"),
    ("games.ftl", "games-menu-close-base"),
    ("games.ftl", "games-menu-clear-clipbase"),
    ("games.ftl", "games-clipbase"),
];

#[test]
fn every_new_key_exists_in_both_languages_with_the_same_variables() {
    for (file, key) in NEW_KEYS {
        let en = ftl("en-US", file);
        let pt = ftl("pt-BR", file);
        let (a, b) = (
            en.get(*key)
                .unwrap_or_else(|| panic!("{key} missing in en-US")),
            pt.get(*key)
                .unwrap_or_else(|| panic!("{key} missing in pt-BR")),
        );
        assert_eq!(a, b, "{key}: variables differ");
    }
}

#[test]
fn the_files_of_the_slice_have_the_same_keys_in_both_languages() {
    for file in ["status.ftl", "dialogs.ftl", "games.ftl"] {
        let en: Vec<String> = ftl("en-US", file).into_keys().collect();
        let pt: Vec<String> = ftl("pt-BR", file).into_keys().collect();
        assert_eq!(en, pt, "{file}");
    }
}

fn end(slot: usize, name: &str) -> CopyEnd {
    CopyEnd {
        slot: SlotId::from_number(slot).unwrap(),
        name: name.to_owned(),
    }
}

fn texts(notice: &BasesNotice) -> [String; 2] {
    [Language::EnUs, Language::PtBr]
        .map(|l| app_notice_text(&I18n::new(l), &AppNotice::Bases(notice.clone())))
}

fn clean(text: &str) {
    assert!(
        !text.contains('$') && !text.contains("{ ") && !text.contains('}'),
        "{text}"
    );
    assert!(!text.is_empty());
}

#[test]
fn every_refusal_names_the_source_and_the_target_in_both_languages() {
    let (from, to) = (end(1, "src.pgn"), end(2, "dst.si4"));
    for reason in [
        CopyRefusal::SourceNotOpen,
        CopyRefusal::TargetNotOpen,
        CopyRefusal::TargetReadOnly,
        CopyRefusal::EmptyFilter,
        CopyRefusal::NothingSelected,
    ] {
        let n = BasesNotice::CopyRefused {
            reason,
            from: from.clone(),
            to: to.clone(),
        };
        let [en, pt] = texts(&n);
        for text in [&en, &pt] {
            clean(text);
            assert!(
                text.contains("src.pgn") && text.contains("dst.si4"),
                "{reason:?}: {text}"
            );
        }
        assert_ne!(en, pt, "{reason:?} is translated");
    }
    // The same database is named once.
    let n = BasesNotice::CopyRefused {
        reason: CopyRefusal::SameBase,
        from: from.clone(),
        to: from,
    };
    for text in texts(&n) {
        clean(&text);
        assert!(text.contains("src.pgn"), "{text}");
    }
}

#[test]
fn an_end_without_a_file_is_the_clipbase_or_the_slot_number() {
    let n = BasesNotice::CopyRefused {
        reason: CopyRefusal::TargetNotOpen,
        from: end(9, ""),
        to: end(4, ""),
    };
    let [en, pt] = texts(&n);
    assert!(en.contains("Clipbase") && en.contains("database 4"), "{en}");
    assert!(pt.contains("Clipbase") && pt.contains("banco 4"), "{pt}");
}

#[test]
fn the_end_of_a_copy_says_how_many_and_between_which_databases() {
    let one = BasesNotice::FilterCopied {
        count: 1,
        from: end(1, "a.pgn"),
        to: end(9, ""),
    };
    let many = BasesNotice::FilterCopied {
        count: 7,
        from: end(1, "a.pgn"),
        to: end(2, "b.si4"),
    };
    let [en1, pt1] = texts(&one);
    let [en7, pt7] = texts(&many);
    assert!(
        en1.contains("One game") && en1.contains("Clipbase"),
        "{en1}"
    );
    assert!(
        pt1.contains("Uma partida") || pt1.contains("uma partida") || pt1.contains("1 "),
        "{pt1}"
    );
    assert!(
        en7.contains('7') && en7.contains("a.pgn") && en7.contains("b.si4"),
        "{en7}"
    );
    assert!(
        pt7.contains('7') && pt7.contains("a.pgn") && pt7.contains("b.si4"),
        "{pt7}"
    );
    for t in [&en1, &pt1, &en7, &pt7] {
        clean(t);
    }
}

#[test]
fn a_stopped_copy_tells_what_was_kept_and_why() {
    let cancelled = BasesNotice::CopyStopped {
        count: 51,
        error: BaseError::Cancelled,
        from: end(1, "a.pgn"),
        to: end(2, "b.si4"),
    };
    let [en, pt] = texts(&cancelled);
    assert!(
        en.contains("51") && en.contains("a.pgn") && en.contains("b.si4"),
        "{en}"
    );
    assert!(pt.contains("51") && pt.contains("a.pgn"), "{pt}");
    for error in [
        BaseError::NameLimit { limit: 7 },
        BaseError::Stale,
        BaseError::AlreadyOpen,
        BaseError::ClipbaseLimit { limit: 4 },
        BaseError::GameUnreadable { number: 3 },
        BaseError::TooManyGames { limit: 9 },
    ] {
        let n = BasesNotice::CopyStopped {
            count: 0,
            error: error.clone(),
            from: end(1, "a.pgn"),
            to: end(2, "b.si4"),
        };
        for (text, language) in texts(&n).iter().zip([Language::EnUs, Language::PtBr]) {
            clean(text);
            let reason = I18n::new(language).tr(error.message_key());
            assert!(
                !reason.is_empty() && text.contains(&reason),
                "{error:?}: {text}"
            );
        }
    }
    let [en, pt] = texts(&BasesNotice::CopyStopped {
        count: 0,
        error: BaseError::NameLimit { limit: 7 },
        from: end(1, "a.pgn"),
        to: end(2, "b.si4"),
    });
    assert!(en.contains("names") && pt.contains("nomes"), "{en} / {pt}");
}

fn dialog_text(language: Language, scope: CopyScope, count: u32) -> String {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    rig.shell.i18n = I18n::new(language);
    rig.shell.app.copy_prompt = Some(mychess_app::CopyPrompt {
        from: end(1, "a.pgn"),
        to: end(2, "b.si4"),
        scope,
        count,
    });
    // The dialog is a real element: find the text through the simulator.
    let element = mychess_gui::windows::dialogs::view(&rig.shell).expect("the box");
    let mut sim = Simulator::with_size(Settings::default(), SIZE, element);
    let candidates: Vec<String> = match language {
        Language::EnUs => vec![
            "Copy one game from a.pgn to b.si4?".into(),
            "Copy 12 games from a.pgn to b.si4?".into(),
            "Copy the selected game from a.pgn to b.si4?".into(),
            "Copy the 12 selected games from a.pgn to b.si4?".into(),
        ],
        Language::PtBr => vec![
            "Copiar uma partida de a.pgn para b.si4?".into(),
            "Copiar 12 partidas de a.pgn para b.si4?".into(),
            "Copiar a partida selecionada de a.pgn para b.si4?".into(),
            "Copiar as 12 partidas selecionadas de a.pgn para b.si4?".into(),
        ],
    };
    candidates
        .into_iter()
        .find(|c| sim.find(c.as_str()).is_ok())
        .unwrap_or_else(|| panic!("no known text for {scope:?} {count} in {language:?}"))
}

#[test]
fn the_question_has_singular_and_plural_forms_for_the_filter_and_for_the_selection() {
    assert_eq!(
        dialog_text(Language::EnUs, CopyScope::Filter, 1),
        "Copy one game from a.pgn to b.si4?"
    );
    assert_eq!(
        dialog_text(Language::EnUs, CopyScope::Filter, 12),
        "Copy 12 games from a.pgn to b.si4?"
    );
    assert_eq!(
        dialog_text(Language::EnUs, CopyScope::Selection, 1),
        "Copy the selected game from a.pgn to b.si4?"
    );
    assert_eq!(
        dialog_text(Language::EnUs, CopyScope::Selection, 12),
        "Copy the 12 selected games from a.pgn to b.si4?"
    );
    assert_eq!(
        dialog_text(Language::PtBr, CopyScope::Filter, 1),
        "Copiar uma partida de a.pgn para b.si4?"
    );
    assert_eq!(
        dialog_text(Language::PtBr, CopyScope::Selection, 12),
        "Copiar as 12 partidas selecionadas de a.pgn para b.si4?"
    );
}

#[test]
fn escape_answers_no_to_the_question_through_the_shell() {
    use mychess_app::{Key, KeyInput, Modifiers, NamedKey};
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    // A PGN is not a target, so fabricate the question as the app would.
    rig.shell.app.copy_prompt = Some(mychess_app::CopyPrompt {
        from: end(1, "a.pgn"),
        to: end(2, "b.si4"),
        scope: CopyScope::Filter,
        count: 12,
    });
    assert!(rig.shell.app.modal_open());
    let key = |named| KeyInput {
        key: Key::Named(named),
        modifiers: Modifiers::NONE,
    };
    // Escape answers "no": the question closes and nothing is copied.
    rig.key(key(NamedKey::Escape), false, None);
    assert!(rig.shell.app.copy_prompt.is_none());
    assert_eq!(rig.shell.app.bases.slot(slot(2)).unwrap().total(), 8);
}
