//! Texts of the M4-5 overlays in both languages (spec "Textos de
//! interface", D852): parity, no duplicated ids, the keys the code uses
//! exist, and the keys resolve. Headless.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mychess_gui::i18n::{I18n, Language};

const LANGUAGES: [&str; 2] = ["en-US", "pt-BR"];

fn i18n_dir(language: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
}

/// `id -> (file, placeholders)` of every message of a language; ids found
/// twice are listed in the second vector.
fn all_messages(language: &str) -> (BTreeMap<String, BTreeSet<String>>, Vec<String>) {
    let mut files: Vec<PathBuf> = std::fs::read_dir(i18n_dir(language))
        .expect("i18n dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "ftl"))
        .collect();
    files.sort();
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut dup = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).expect("read");
        let mut current: Option<String> = None;
        for line in text.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if !line.starts_with(' ')
                && let Some((id, _)) = line.split_once(" = ")
            {
                let id = id.trim().to_owned();
                if out.contains_key(&id) {
                    dup.push(format!("{id} ({})", file.display()));
                }
                out.entry(id.clone()).or_default();
                current = Some(id);
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
    }
    (out, dup)
}

fn is_new(id: &str) -> bool {
    id.contains("arrow")
        || id.starts_with("menu-view-evalbar")
        || id.starts_with("menu-view-traffic")
        || id.starts_with("menu-options-evalbar")
        || id.starts_with("menu-options-arrows")
        || matches!(
            id,
            "action-toggle-eval-bar" | "action-toggle-traffic-lights"
        )
}

#[test]
fn every_new_key_exists_in_both_languages_with_the_same_placeholders() {
    let (en, _) = all_messages("en-US");
    let (pt, _) = all_messages("pt-BR");
    let new: BTreeSet<&String> = en.keys().chain(pt.keys()).filter(|k| is_new(k)).collect();
    assert!(new.len() >= 20, "found only {} new keys", new.len());
    for key in new {
        assert!(en.contains_key(key), "{key} missing in en-US");
        assert!(pt.contains_key(key), "{key} missing in pt-BR");
        assert_eq!(en[key], pt[key], "{key}: placeholders differ");
    }
}

#[test]
fn the_spec_table_keys_exist() {
    let (en, _) = all_messages("en-US");
    let (pt, _) = all_messages("pt-BR");
    for key in [
        "menu-view-evalbar",
        "menu-view-traffic-lights",
        "menu-view-arrow-numbers",
        "menu-options-evalbar",
        "menu-options-evalbar-scale",
        "menu-options-evalbar-ticks",
        "menu-options-evalbar-log",
        "menu-options-evalbar-label",
        "menu-options-arrows",
        "menu-options-arrows-click",
        "analysis-btn-arrows",
        "analysis-tip-arrows",
        "analysis-arrow-mode-arrows",
        "analysis-arrow-mode-sequence",
        "analysis-arrow-seq-both",
        "analysis-arrow-seq-white",
        "analysis-arrow-seq-black",
        "analysis-arrow-seq-len",
        "action-toggle-eval-bar",
        "action-toggle-traffic-lights",
        "action-toggle-arrow-numbers",
    ] {
        assert!(en.contains_key(key), "{key} (en-US)");
        assert!(pt.contains_key(key), "{key} (pt-BR)");
    }
}

#[test]
fn no_message_id_is_defined_twice_in_a_language() {
    for language in LANGUAGES {
        let (_, dup) = all_messages(language);
        assert!(dup.is_empty(), "{language}: {dup:?}");
    }
}

#[test]
fn new_keys_resolve_to_real_text_in_both_languages() {
    let (en_keys, _) = all_messages("en-US");
    for (language, tag) in [(Language::EnUs, "en"), (Language::PtBr, "pt")] {
        let i18n = I18n::new(language);
        for key in en_keys.keys().filter(|k| is_new(k)) {
            // Keys with placeholders are not resolved here.
            if !en_keys[key].is_empty() {
                continue;
            }
            let text = i18n.tr(key);
            assert!(i18n.has(key), "{tag}: {key}");
            assert_ne!(&text, key, "{tag}: {key} fell back to the id");
            assert!(!text.trim().is_empty(), "{tag}: {key} is blank");
            assert!(!text.contains('\u{FFFD}'), "{tag}: {key}");
        }
    }
}

#[test]
fn the_two_languages_differ_for_the_texts_that_have_a_translation() {
    // A copied English text in pt-BR is a forgotten translation. Texts that
    // read the same in both (a bare word "Arrows" vs "Setas" differ) are
    // few: only those two-word-or-longer messages must differ.
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    let (keys, _) = all_messages("en-US");
    for key in keys.keys().filter(|k| is_new(k) && keys[*k].is_empty()) {
        let (a, b) = (en.tr(key), pt.tr(key));
        if a.split_whitespace().count() >= 2 {
            assert_ne!(a, b, "{key} was not translated: {a:?}");
        }
    }
}

/// The string literals of the sources that look like keys of this slice.
fn literal_keys_in(dir: &Path, out: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir)
        .expect("src dir")
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.is_dir() {
            literal_keys_in(&path, out);
        } else if path.extension().is_some_and(|x| x == "rs") {
            let text = std::fs::read_to_string(&path).expect("read");
            let mut rest = text.as_str();
            while let Some(at) = rest.find('"') {
                rest = &rest[at + 1..];
                let Some(end) = rest.find('"') else { break };
                let lit = &rest[..end];
                rest = &rest[end + 1..];
                let looks_like_key = !lit.is_empty()
                    && lit
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                    && (lit.starts_with("menu-view-")
                        || lit.starts_with("menu-options-")
                        || lit.starts_with("analysis-arrow")
                        || lit.starts_with("analysis-btn-arrows")
                        || lit.starts_with("analysis-tip-arrows")
                        || matches!(
                            lit,
                            "action-toggle-eval-bar"
                                | "action-toggle-traffic-lights"
                                | "action-toggle-arrow-numbers"
                        ));
                if looks_like_key {
                    out.insert(lit.to_owned());
                }
            }
        }
    }
}

#[test]
fn every_key_the_gui_and_the_app_use_for_the_overlays_exists() {
    let mut used = BTreeSet::new();
    for krate in ["mychess-gui", "mychess-app"] {
        let src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(krate)
            .join("src");
        literal_keys_in(&src, &mut used);
    }
    let (en, _) = all_messages("en-US");
    let (pt, _) = all_messages("pt-BR");
    let mut missing = Vec::new();
    for key in &used {
        // A prefix such as "menu-view-" is built with `format!`: skip it.
        if key.ends_with('-') {
            continue;
        }
        if !en.contains_key(key) || !pt.contains_key(key) {
            missing.push(key.clone());
        }
    }
    assert!(
        missing.is_empty(),
        "keys used in code but not defined: {missing:?}"
    );
    assert!(
        used.contains("menu-view-evalbar"),
        "the scan finds the keys"
    );
}
