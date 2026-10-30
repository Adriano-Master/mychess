//! Texts of the graph panel in both languages (spec M4-7, "Textos de
//! interface", D874): parity of `graph.ftl`, no duplicated ids across the
//! files, the keys the code uses, placeholders and the hover texts.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

mod m4_7_gui_common;

use std::collections::{BTreeMap, BTreeSet};

use fluent_bundle::FluentArgs;
use fluent_syntax::ast::Entry;
use fluent_syntax::parser;
use m4_7_gui_common::*;

const LANGUAGES: [&str; 2] = ["en-US", "pt-BR"];

fn ids_of(language: &str, file: &str) -> Vec<String> {
    let text = ftl_text(language, file);
    let resource = parser::parse(text.as_str()).unwrap_or_else(|(r, _)| r);
    resource
        .body
        .iter()
        .filter_map(|e| match e {
            Entry::Message(m) => Some(m.id.name.to_owned()),
            _ => None,
        })
        .collect()
}

fn placeholders(language: &str, file: &str) -> BTreeMap<String, BTreeSet<String>> {
    let text = ftl_text(language, file);
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(' ')
            && let Some((id, _)) = line.split_once(" = ")
        {
            current = Some(id.to_owned());
            out.entry(id.to_owned()).or_default();
        }
        if let Some(id) = &current {
            let set = out.get_mut(id).unwrap();
            let mut rest = line;
            while let Some(at) = rest.find('$') {
                rest = &rest[at + 1..];
                set.insert(
                    rest.chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect(),
                );
            }
        }
    }
    out
}

#[test]
fn graph_ftl_parses_without_errors_in_both_languages() {
    for language in LANGUAGES {
        let text = ftl_text(language, "graph.ftl");
        assert!(parser::parse(text.as_str()).is_ok(), "{language}");
    }
}

#[test]
fn graph_ftl_has_the_same_ids_and_placeholders_in_en_us_and_pt_br() {
    let en = placeholders("en-US", "graph.ftl");
    let pt = placeholders("pt-BR", "graph.ftl");
    assert_eq!(en, pt);
    assert!(en.len() >= 15, "{}", en.len());
}

#[test]
fn no_message_id_is_defined_twice_across_the_ftl_files() {
    for language in LANGUAGES {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/i18n")
            .join(language);
        let mut files: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| {
                std::path::Path::new(n)
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("ftl"))
            })
            .collect();
        files.sort();
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        for file in files {
            for id in ids_of(language, &file) {
                if let Some(first) = seen.insert(id.clone(), file.clone()) {
                    panic!("{language}: `{id}` is in {first} and in {file}");
                }
            }
        }
    }
}

#[test]
fn the_menu_and_pane_keys_exist_in_both_languages() {
    for key in [
        "menu-tools-graph",
        "menu-view-pane-graph",
        "pane-graph",
        "action-toggle-graph-pane",
    ] {
        for language in [Language::EnUs, Language::PtBr] {
            let i18n = I18n::new(language);
            assert!(i18n.has(key), "{key} in {language:?}");
            assert_ne!(i18n.tr(key), key, "{key} in {language:?} is not translated");
        }
    }
}

use mychess_gui::i18n::{I18n, Language};

/// Every `"graph-..."` literal in the sources of the panel must be a key.
#[test]
fn every_graph_key_the_code_uses_exists() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut keys = BTreeSet::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir).unwrap().filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                for part in text.split('"').skip(1).step_by(2) {
                    // `graph-pane` is the widget id of the body, not a key.
                    if part.starts_with("graph-")
                        && part != "graph-pane"
                        && part.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                    {
                        keys.insert(part.to_owned());
                    }
                }
            }
        }
    }
    assert!(keys.len() >= 10, "found {keys:?}");
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for key in &keys {
            assert!(
                i18n.has(key),
                "`{key}` is used by the code but missing in {language:?}"
            );
        }
    }
}

#[test]
fn the_five_kind_labels_are_distinct_in_each_language() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        let labels: BTreeSet<String> = [
            "graph-kind-auto",
            "graph-kind-score",
            "graph-kind-time",
            "graph-kind-score-combo",
            "graph-kind-time-combo",
        ]
        .iter()
        .map(|k| i18n.tr(k))
        .collect();
        assert_eq!(labels.len(), 5, "{language:?}");
    }
}

#[test]
fn the_hover_texts_fill_their_arguments_and_survive_hostile_values() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for (key, value) in [
            ("graph-hover-score", "+0.35"),
            ("graph-hover-time", "12"),
            ("graph-hover-score", "{ $x } \u{2067}"),
            ("graph-hover-time", ""),
        ] {
            let mut args = FluentArgs::new();
            args.set("move", "3...");
            args.set("value", value);
            let out = i18n.tr_args(key, &args);
            assert!(out.contains("3..."), "{key}: {out}");
            assert!(!out.contains("{$"), "{key}: {out}");
        }
    }
}

#[test]
fn the_two_invert_labels_keep_the_scid_names_and_have_tips() {
    let en = I18n::new(Language::EnUs);
    assert_eq!(en.tr("graph-invert-white"), "Invert White");
    assert_eq!(en.tr("graph-invert-black"), "Invert Black");
    assert_eq!(en.tr("graph-highlight"), "Highlight current move");
    assert_eq!(en.tr("graph-empty"), "No scores or times in this game");
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for key in [
            "graph-invert-white-tip",
            "graph-invert-black-tip",
            "graph-highlight-tip",
        ] {
            assert!(i18n.tr(key).len() > 20, "{key} {language:?}");
        }
    }
    assert_eq!(
        pt().tr("graph-empty"),
        "Sem pontuações nem tempos nesta partida"
    );
}

#[test]
fn the_panel_does_not_depend_on_the_system_language() {
    // `Rig` forces the language through the preferences: whatever the
    // machine says, the English wording is found.
    let rig = rig_with_graph(evals_game());
    assert!(rig.sim(SIZE).find("Invert White").is_ok());
}
