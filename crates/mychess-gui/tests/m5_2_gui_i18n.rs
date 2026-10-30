//! M5-2a GUI contract: texts of the opening tree panel in both languages
//! (spec `docs/specs/m5-2-janela-da-arvore.md`, D971, D983, "Todo texto vem do
//! Fluent"): parity of `tree.ftl`, the same variables, the keys the code
//! uses, the menu and action texts, and no text fixed in the Rust modules.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

mod m5_2_gui_common;

use std::collections::{BTreeMap, BTreeSet};

use fluent_syntax::ast::Entry;
use fluent_syntax::parser;
use m5_2_gui_common::{en, ftl_text, pt};

const LANGUAGES: [&str; 2] = ["en-US", "pt-BR"];

fn ids_of(language: &str, file: &str) -> BTreeSet<String> {
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

fn tree_source() -> String {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut text = std::fs::read_to_string(dir.join("windows/tree.rs")).expect("tree.rs");
    text.push_str(&std::fs::read_to_string(dir.join("theme/tree_colors.rs")).expect("colors"));
    text
}

/// The source without the test module and without comments.
fn production_code(source: &str) -> String {
    let cut = source.find("#[cfg(test)]").unwrap_or(source.len());
    source[..cut]
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn tree_ftl_parses_without_errors_in_both_languages() {
    for language in LANGUAGES {
        let text = ftl_text(language, "tree.ftl");
        assert!(parser::parse(text.as_str()).is_ok(), "{language}");
    }
}

#[test]
fn both_languages_define_exactly_the_same_tree_keys() {
    let en = ids_of("en-US", "tree.ftl");
    let pt = ids_of("pt-BR", "tree.ftl");
    assert_eq!(
        en.difference(&pt).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "only in en-US"
    );
    assert_eq!(
        pt.difference(&en).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "only in pt-BR"
    );
    assert!(en.len() >= 30, "{} keys", en.len());
}

#[test]
fn every_key_uses_the_same_variables_in_both_languages() {
    let en = placeholders("en-US", "tree.ftl");
    let pt = placeholders("pt-BR", "tree.ftl");
    for (id, vars) in &en {
        assert_eq!(pt.get(id), Some(vars), "{id}");
    }
    // The variables the code passes (D983) are the ones the texts use.
    assert_eq!(
        en["tree-percent"],
        BTreeSet::from(["whole".to_owned(), "tenth".to_owned()])
    );
    assert_eq!(
        en["tree-percent-whole"],
        BTreeSet::from(["value".to_owned()])
    );
    assert_eq!(en["tree-issue-read"], BTreeSet::from(["reason".to_owned()]));
    assert_eq!(en["tree-unreadable"], BTreeSet::from(["count".to_owned()]));
}

#[test]
fn every_tree_key_resolves_to_a_text_in_both_languages() {
    for i18n in [en(), pt()] {
        for id in ids_of("en-US", "tree.ftl") {
            assert!(i18n.has(&id), "{id}");
            if !placeholders("en-US", "tree.ftl")[&id].is_empty() {
                continue;
            }
            let text = i18n.tr(&id);
            assert!(!text.trim().is_empty() && text != id, "{id}");
        }
    }
}

#[test]
fn the_menu_action_and_pane_keys_exist_in_both_languages() {
    for (file, key) in [
        ("menu.ftl", "menu-view-pane-tree"),
        ("menu.ftl", "menu-tools-tree"),
        ("tree.ftl", "action-toggle-tree-pane"),
        ("tree.ftl", "pane-tree"),
    ] {
        for language in LANGUAGES {
            assert!(ids_of(language, file).contains(key), "{language}: {key}");
        }
    }
    assert_ne!(
        pt().tr("action-toggle-tree-pane"),
        en().tr("action-toggle-tree-pane")
    );
}

#[test]
fn no_tree_key_is_defined_twice_across_the_files_of_a_language() {
    for language in LANGUAGES {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/i18n")
            .join(language);
        let mut owner: BTreeMap<String, String> = BTreeMap::new();
        for entry in std::fs::read_dir(dir).unwrap().filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if std::path::Path::new(&name)
                .extension()
                .is_none_or(|e| e != "ftl")
            {
                continue;
            }
            for id in ids_of(language, &name) {
                let ours = id.starts_with("tree-")
                    || [
                        "action-toggle-tree-pane",
                        "menu-tools-tree",
                        "menu-view-pane-tree",
                        "pane-tree",
                    ]
                    .contains(&id.as_str());
                if ours && let Some(other) = owner.insert(id.clone(), name.clone()) {
                    panic!("{language}: {id} is in {other} and {name}");
                }
            }
        }
    }
}

#[test]
fn every_key_the_panel_code_asks_for_exists() {
    let source = production_code(&tree_source());
    let mut used = BTreeSet::new();
    let mut rest = source.as_str();
    while let Some(at) = rest.find("\"tree-") {
        rest = &rest[at + 1..];
        let key: String = rest
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || *c == '-')
            .collect();
        // Ids of widgets (`tree-row-`, `tree-total`, `tree-pane`) are not
        // texts.
        if !key.ends_with('-') && !["tree-total-id", "tree-pane"].contains(&key.as_str()) {
            used.insert(key);
        }
    }
    let known = ids_of("en-US", "tree.ftl");
    let missing: Vec<&String> = used
        .iter()
        .filter(|k| !known.contains(*k) && k.as_str() != "tree-row")
        .collect();
    assert!(missing.is_empty(), "keys without a text: {missing:?}");
    assert!(used.len() >= 20, "{used:?}");
}

#[test]
fn the_panel_modules_fix_no_visible_text_in_the_code() {
    let source = production_code(&tree_source());
    // A literal given to a widget that shows it.
    for pattern in ["text(\"", ".label(\"", "placeholder(\"", "tooltip(\""] {
        for (at, _) in source.match_indices(pattern) {
            let literal: String = source[at + pattern.len()..]
                .chars()
                .take_while(|c| *c != '"')
                .collect();
            assert!(
                literal.is_empty(),
                "fixed text {literal:?} after `{pattern}`"
            );
        }
    }
    // Nothing like a sentence in a plain string either.
    for line in source.lines() {
        let t = line.trim();
        if t.starts_with("#[") || t.starts_with("use ") {
            continue;
        }
        for literal in t.split('"').skip(1).step_by(2) {
            let looks_like_prose = literal.contains(' ')
                && literal.chars().any(char::is_alphabetic)
                && !literal.contains('{');
            assert!(!looks_like_prose, "prose in code: {literal:?} ({t})");
        }
    }
}

#[test]
fn the_panel_is_in_portuguese_for_a_portuguese_session() {
    use m5_2_gui_common::*;
    let dir = tempfile::tempdir().unwrap();
    let mut rig = TreeRig::with_language(mychess_app::LanguagePref::PtBr);
    rig.open_pgn(dir.path(), "std.pgn", STD);
    rig.open_panel();
    for text in [
        "Lance",
        "Partidas",
        "Empates",
        "Atualizar",
        "Ajustar filtro",
        "Ordenar",
        "Busca",
        "Exibição curta",
    ] {
        assert!(pane_shows(&rig, text), "{text}");
    }
    for text in ["Move", "Games", "Refresh", "Adjust filter", "Short display"] {
        assert!(!pane_shows(&rig, text), "{text} leaked from en-US");
    }
}
