//! Checks on the Fluent files: valid syntax, the same ids in both
//! languages, every id referenced exists, and every key used in the code
//! exists.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use fluent_syntax::ast::{Entry, Expression, InlineExpression, PatternElement};
use fluent_syntax::parser;
use mychess_gui::i18n::{I18n, Language};

fn workspace_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn ftl_files(language: &str) -> Vec<PathBuf> {
    let dir = workspace_file(&format!("assets/i18n/{language}"));
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ftl")))
        .collect();
    files.sort();
    files
}

/// Message ids and the ids they reference.
fn ids(language: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut defined = BTreeSet::new();
    let mut referenced = BTreeSet::new();
    for file in ftl_files(language) {
        let source = fs::read_to_string(&file).expect("readable file");
        assert!(
            !source.starts_with('\u{feff}'),
            "{} has a BOM",
            file.display()
        );
        assert!(!source.contains('\r'), "{} has CR", file.display());
        let resource = parser::parse(source.as_str())
            .unwrap_or_else(|(_, errors)| panic!("{}: {errors:?}", file.display()));
        for entry in &resource.body {
            if let Entry::Message(message) = entry {
                defined.insert(message.id.name.to_owned());
                if let Some(pattern) = &message.value {
                    collect_references(&pattern.elements, &mut referenced);
                }
            }
        }
    }
    (defined, referenced)
}

fn collect_references(elements: &[PatternElement<&str>], out: &mut BTreeSet<String>) {
    for element in elements {
        if let PatternElement::Placeable { expression } = element {
            match expression {
                Expression::Inline(InlineExpression::MessageReference { id, .. }) => {
                    out.insert(id.name.to_owned());
                }
                Expression::Select { variants, .. } => {
                    for variant in variants {
                        collect_references(&variant.value.elements, out);
                    }
                }
                Expression::Inline(_) => {}
            }
        }
    }
}

#[test]
fn both_languages_define_the_same_ids() {
    let (en, _) = ids("en-US");
    let (pt, _) = ids("pt-BR");
    assert_eq!(
        en.difference(&pt).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "missing in pt-BR"
    );
    assert_eq!(
        pt.difference(&en).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "missing in en-US"
    );
}

#[test]
fn referenced_messages_exist() {
    for language in ["en-US", "pt-BR"] {
        let (defined, referenced) = ids(language);
        let missing: Vec<_> = referenced.difference(&defined).collect();
        assert!(missing.is_empty(), "{language}: {missing:?}");
    }
}

#[test]
fn every_literal_key_used_in_the_code_is_defined() {
    let (defined, _) = ids("en-US");
    let mut sources = Vec::new();
    collect_rs(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut sources,
    );
    assert_ne!(sources.len(), 0);
    let mut missing = Vec::new();
    for file in sources {
        let full = fs::read_to_string(&file).expect("readable source");
        // Unit tests may use made-up keys on purpose.
        let text = full.split("#[cfg(test)]").next().unwrap_or_default();
        for marker in [
            "tr(\"",
            "tr_args(\"",
            "Label::Key(\"",
            "label_key: \"",
            "item(\"",
            "toggle(\"",
            "submenu(\"",
            "pane_toggle(\"",
        ] {
            for (at, _) in text.match_indices(marker) {
                let before = text[..at].chars().next_back();
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let rest = &text[at + marker.len()..];
                if let Some(end) = rest.find('"') {
                    let key = &rest[..end];
                    if !defined.contains(key) {
                        missing.push(format!("{}: {key}", file.display()));
                    }
                }
            }
        }
    }
    assert!(missing.is_empty(), "{missing:#?}");
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir)
        .expect("readable directory")
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|x| x == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn the_keys_listed_by_the_spec_exist_in_both_languages() {
    let required = [
        "app-name",
        "window-title",
        "window-untitled",
        "menu-file",
        "menu-edit",
        "menu-game",
        "menu-view",
        "menu-help",
        "menu-view-theme-system",
        "menu-view-theme-light",
        "menu-view-theme-dark",
        "menu-view-coords-none",
        "menu-view-coords-outside",
        "menu-view-coords-both",
        "menu-view-coords-inside",
        "menu-view-material-off",
        "menu-view-material-difference",
        "menu-view-material-captured",
        "menu-view-language-system",
        "menu-view-language-en-us",
        "menu-view-language-pt-br",
        "pane-board",
        "pane-info",
        "pane-notation",
        "notation-placeholder",
        "board-promote-queen",
        "board-promote-rook",
        "board-promote-bishop",
        "board-promote-knight",
        "board-flip",
        "status-ready",
        "status-turn-white",
        "status-turn-black",
        "status-check",
        "status-checkmate",
        "status-stalemate",
        "status-modified",
        "info-unknown",
        "info-round",
        "dialog-ok",
        "dialog-cancel",
        "about-title",
        "about-license",
    ];
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for key in required {
            assert!(i18n.has(key), "{key} missing in {language:?}");
        }
    }
}

#[test]
fn a_missing_key_returns_itself() {
    assert_eq!(I18n::new(Language::PtBr).tr("no-such-key"), "no-such-key");
}
