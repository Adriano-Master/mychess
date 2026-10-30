//! Texts of the game browser in both languages (spec M3-4, slice 4e, D595;
//! rule "interface text in Fluent"): key parity, same variables, nothing
//! hardcoded in the new files.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

mod m3_4e_common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mychess_app::{AppNotice, BasesNotice};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::status_bar::app_notice_text;

fn ftl(language: &str, file: &str) -> BTreeMap<String, Vec<String>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join(file);
    let text = std::fs::read_to_string(&path).expect("ftl file");
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

const KEYS: &[(&str, &str)] = &[
    ("games.ftl", "games-menu-browse"),
    ("games.ftl", "games-tip-browse"),
    ("games.ftl", "browser-title-single"),
    ("games.ftl", "browser-title-grid"),
    ("games.ftl", "browser-loading"),
    ("games.ftl", "browser-progress"),
    ("games.ftl", "browser-unreadable"),
    ("games.ftl", "browser-deleted"),
    ("games.ftl", "browser-load"),
    ("games.ftl", "browser-close"),
    ("games.ftl", "browser-first-game"),
    ("games.ftl", "browser-previous-game"),
    ("games.ftl", "browser-next-game"),
    ("games.ftl", "browser-last-game"),
    ("games.ftl", "browser-tip-start"),
    ("games.ftl", "browser-tip-back"),
    ("games.ftl", "browser-tip-forward"),
    ("games.ftl", "browser-tip-end"),
    ("games.ftl", "browser-tip-play"),
    ("games.ftl", "browser-tip-pause"),
    ("games.ftl", "browser-tip-flip"),
    ("games.ftl", "browser-tip-larger"),
    ("games.ftl", "browser-tip-smaller"),
    ("games.ftl", "browser-tip-load"),
    ("games.ftl", "browser-players"),
    ("games.ftl", "browser-game-line"),
    ("games.ftl", "browser-event-line"),
    ("games.ftl", "browser-ply-line"),
    ("status.ftl", "base-notice-browser-limit"),
];

#[test]
fn every_browser_key_exists_in_both_languages_with_the_same_variables() {
    for (file, key) in KEYS {
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
fn the_variables_are_the_ones_the_view_passes() {
    let en = ftl("en-US", "games.ftl");
    let vars = |key: &str| en[key].clone();
    assert_eq!(vars("browser-title-single"), ["base", "number"]);
    assert_eq!(vars("browser-title-grid"), ["base", "count"]);
    assert_eq!(vars("browser-players"), ["black", "white"]);
    assert_eq!(vars("browser-event-line"), ["date", "event"]);
    assert_eq!(vars("browser-ply-line"), ["max", "ply"]);
    assert_eq!(vars("browser-game-line"), ["number"]);
    assert_eq!(vars("browser-progress"), ["done", "total"]);
    let status = ftl("en-US", "status.ftl");
    assert_eq!(status["base-notice-browser-limit"], ["asked", "shown"]);
}

#[test]
fn the_files_have_the_same_keys_in_both_languages() {
    for file in ["games.ftl", "status.ftl"] {
        let en: Vec<String> = ftl("en-US", file).into_keys().collect();
        let pt: Vec<String> = ftl("pt-BR", file).into_keys().collect();
        assert_eq!(en, pt, "{file}");
    }
}

#[test]
fn the_limit_notice_has_its_numbers_in_both_languages() {
    let notice = AppNotice::Bases(BasesNotice::BrowserLimit {
        shown: 64,
        asked: 70,
    });
    let en = app_notice_text(&I18n::new(Language::EnUs), &notice);
    let pt = app_notice_text(&I18n::new(Language::PtBr), &notice);
    for text in [&en, &pt] {
        assert!(text.contains("64") && text.contains("70"), "{text}");
        assert!(!text.contains('$') && !text.contains("{ "), "{text}");
    }
    assert_ne!(en, pt);
}

#[test]
fn every_browser_message_renders_without_leftover_placeholders() {
    use fluent_bundle::FluentArgs;
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        let mut all = FluentArgs::new();
        all.set("base", "x.pgn");
        all.set("number", 3);
        all.set("count", 5);
        all.set("white", "W");
        all.set("black", "B");
        all.set("event", "E");
        all.set("date", "D");
        all.set("ply", 1);
        all.set("max", 2);
        all.set("done", 1);
        all.set("total", 2);
        for (_, key) in KEYS.iter().filter(|(f, _)| *f == "games.ftl") {
            let text = i18n.tr_args(key, &all);
            assert!(!text.is_empty(), "{key}");
            assert!(!text.contains('$') && !text.contains("{ "), "{key}: {text}");
            assert!(!text.contains("missing"), "{key}: {text}");
        }
    }
}

fn source(file: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(file)).unwrap()
}

#[test]
fn the_new_sources_have_no_visible_text_in_the_code() {
    for file in [
        "src/windows/browser.rs",
        "src/shell/browser_glue.rs",
        "src/windows/game_list/toolbar.rs",
        "src/windows/game_list/menus.rs",
    ] {
        let text = source(file);
        let production = text.split("#[cfg(test)]").next().unwrap();
        let mut rest = production;
        while let Some(at) = rest.find("text(\"") {
            let after = &rest[at + 6..];
            let end = after.find('"').unwrap();
            let literal = &after[..end];
            assert!(
                literal.is_empty() || literal == "×",
                "{file}: hardcoded text(\"{literal}\")"
            );
            rest = &after[end..];
        }
        let code: String = production
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for literal in [
            "\"Close\"",
            "\"Load game\"",
            "\"Browse\"",
            "\"deleted\"",
            "\"Game ",
            "\"Move ",
            "\"First game\"",
            "\"Next game\"",
            "\"Reading",
        ] {
            assert!(
                !code.contains(literal),
                "{file}: visible English {literal} in the code"
            );
        }
    }
}

#[test]
fn the_keys_the_browser_sources_ask_for_all_exist() {
    let en = ftl("en-US", "games.ftl");
    let en_status = ftl("en-US", "status.ftl");
    let pt = ftl("pt-BR", "games.ftl");
    let pt_status = ftl("pt-BR", "status.ftl");
    let mut used = BTreeSet::new();
    for file in ["src/windows/browser.rs", "src/shell/browser_glue.rs"] {
        let text = source(file);
        let production = text.split("#[cfg(test)]").next().unwrap();
        let mut rest = production;
        while let Some(start) = rest.find('"') {
            let after = &rest[start + 1..];
            let Some(end) = after.find('"') else { break };
            let literal = &after[..end];
            if (literal.starts_with("browser-") || literal.starts_with("base-notice-browser"))
                && literal != "browser-box"
                && literal.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                && !literal.starts_with("browser-moves")
            {
                used.insert(literal.to_owned());
            }
            rest = &after[end + 1..];
        }
    }
    assert!(used.len() >= 15, "found only {used:?}");
    for key in used {
        if key == "game-browser" {
            continue;
        }
        let found = |a: &BTreeMap<String, Vec<String>>, b: &BTreeMap<String, Vec<String>>| {
            a.contains_key(&key) || b.contains_key(&key)
        };
        assert!(found(&en, &en_status), "{key} missing in en-US");
        assert!(found(&pt, &pt_status), "{key} missing in pt-BR");
    }
}
