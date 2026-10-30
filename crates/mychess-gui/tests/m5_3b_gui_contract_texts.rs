//! M5-3b additional GUI contract tests of the cache bar (D1764, D1768,
//! D1770): every status line in both languages, the controls and the Fluent
//! files (same variables, formatting without errors, "variante").
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_gui_common;

use std::collections::{BTreeMap, BTreeSet};

use fluent_bundle::FluentArgs;
use fluent_syntax::ast::{Entry, Expression, InlineExpression, Pattern, PatternElement};
use fluent_syntax::parser;
use m5_2_gui_common::*;
use mychess_app::{CacheFill, CacheIssue, CacheStatus, LanguagePref, ListMessage, SlotId};
use tempfile::tempdir;

fn walk_pattern(pattern: &Pattern<&str>, out: &mut BTreeSet<String>) {
    for element in &pattern.elements {
        if let PatternElement::Placeable { expression } = element {
            walk_expression(expression, out);
        }
    }
}

fn walk_inline(inline: &InlineExpression<&str>, out: &mut BTreeSet<String>) {
    match inline {
        InlineExpression::VariableReference { id } => {
            out.insert(id.name.to_owned());
        }
        InlineExpression::Placeable { expression } => walk_expression(expression, out),
        InlineExpression::FunctionReference { arguments, .. } => {
            for arg in &arguments.positional {
                walk_inline(arg, out);
            }
        }
        _ => {}
    }
}

fn walk_expression(expression: &Expression<&str>, out: &mut BTreeSet<String>) {
    match expression {
        Expression::Inline(inline) => walk_inline(inline, out),
        Expression::Select { selector, variants } => {
            walk_inline(selector, out);
            for variant in variants {
                walk_pattern(&variant.value, out);
            }
        }
    }
}

/// Message id -> variables it uses, for the whole file.
fn all_variables(language: &str) -> BTreeMap<String, BTreeSet<String>> {
    let text = ftl_text(language, "tree.ftl");
    let resource = parser::parse(text.as_str()).unwrap_or_else(|(r, _)| r);
    let mut out = BTreeMap::new();
    for entry in &resource.body {
        if let Entry::Message(m) = entry {
            let mut vars = BTreeSet::new();
            if let Some(pattern) = &m.value {
                walk_pattern(pattern, &mut vars);
            }
            out.insert(m.id.name.to_owned(), vars);
        }
    }
    out
}

#[test]
fn the_whole_tree_file_has_the_same_keys_and_variables_in_both_languages() {
    assert_eq!(all_variables("en-US"), all_variables("pt-BR"));
}

#[test]
fn the_tree_files_parse_without_errors() {
    for language in ["en-US", "pt-BR"] {
        let text = ftl_text(language, "tree.ftl");
        assert!(
            parser::parse(text.as_str()).is_ok(),
            "{language}: syntax errors in tree.ftl"
        );
    }
}

#[test]
fn every_cache_message_formats_with_its_variables_in_both_languages() {
    for (language, i18n) in [("en-US", en()), ("pt-BR", pt())] {
        for (id, vars) in all_variables(language) {
            if !id.starts_with("tree-cache") {
                continue;
            }
            let mut args = FluentArgs::new();
            for var in &vars {
                args.set(var.as_str(), 7_i64);
            }
            let text = i18n.tr_args(&id, &args);
            assert!(!text.is_empty(), "{language} {id}");
            assert_ne!(text, id, "{language} {id}: missing");
            assert!(
                !text.contains('{') && !text.contains('$'),
                "{language} {id}: unresolved placeable in {text:?}"
            );
            if !vars.is_empty() {
                assert!(text.contains('7'), "{language} {id}: {text:?}");
            }
        }
    }
}

#[test]
fn the_portuguese_cache_texts_differ_from_the_english_ones() {
    for (id, vars) in all_variables("en-US") {
        if !id.starts_with("tree-cache") {
            continue;
        }
        // "Cache" alone is the same word in both languages.
        if id == "tree-cache" {
            continue;
        }
        let mut args = FluentArgs::new();
        for var in &vars {
            args.set(var.as_str(), 3_i64);
        }
        assert_ne!(
            en().tr_args(&id, &args),
            pt().tr_args(&id, &args),
            "{id} is not translated"
        );
    }
}

#[test]
fn every_key_the_cache_bar_uses_exists_in_both_files() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("windows")
            .join("tree")
            .join("cache.rs"),
    )
    .unwrap();
    let keys: BTreeSet<&str> = source
        .split('"')
        .filter(|s| s.starts_with("tree-cache") && !s.contains(' '))
        .collect();
    assert!(keys.len() >= 10, "{keys:?}");
    let en_vars = all_variables("en-US");
    let pt_vars = all_variables("pt-BR");
    for key in keys {
        assert!(en_vars.contains_key(key), "en-US lacks {key}");
        assert!(pt_vars.contains_key(key), "pt-BR lacks {key}");
    }
}

fn panel(language: LanguagePref, dir: &std::path::Path, si4: bool) -> TreeRig {
    let mut rig = TreeRig::with_language(language);
    if si4 {
        rig.open_si4(dir, "std", STD);
    } else {
        rig.open_pgn(dir, "std.pgn", STD);
    }
    rig.open_panel();
    rig
}

#[test]
fn the_not_scid_failure_is_shown_in_both_languages_and_can_be_dismissed() {
    for (language, text) in [
        (
            LanguagePref::EnUs,
            "Only Scid databases (.si4) have a cache file",
        ),
        (
            LanguagePref::PtBr,
            "Só bases Scid (.si4) têm arquivo de cache",
        ),
    ] {
        let dir = tempdir().unwrap();
        let mut rig = panel(language, dir.path(), false);
        rig.app_msg(TreeMessage::SaveCache);
        assert_eq!(
            rig.view().cache.status,
            CacheStatus::Failed(CacheIssue::NotScidBase)
        );
        assert!(pane_shows(&rig, text), "{text}");
        assert!(!rig.rig.shell.modal_layer_open());
        rig.app_msg(TreeMessage::DismissCacheStatus);
        assert!(!pane_shows(&rig, text));
        assert_eq!(rig.view().cache.status, CacheStatus::None);
    }
}

#[test]
fn a_second_request_during_a_fill_keeps_the_progress_line_and_the_job() {
    let dir = tempdir().unwrap();
    let mut rig = panel(LanguagePref::EnUs, dir.path(), true);
    let task = rig
        .rig
        .shell
        .update(Message::App(TreeMessage::FillCache(CacheFill::Base).into()));
    send_without_running(&mut rig, TreeMessage::CacheInfo);
    assert_eq!(
        rig.view().cache.status,
        CacheStatus::Failed(CacheIssue::Busy)
    );
    // The bar shows the progress (and Cancel), not a second job.
    assert!(rig.view().cache.busy.is_some());
    assert!(pane_shows(&rig, "Cancel"));
    rig.drain(task);
    assert!(matches!(
        rig.view().cache.status,
        CacheStatus::Filled { .. }
    ));
}

#[test]
fn the_cancelled_fill_says_so_in_portuguese() {
    let dir = tempdir().unwrap();
    let mut rig = panel(LanguagePref::PtBr, dir.path(), true);
    let task = rig.rig.shell.update(Message::App(
        TreeMessage::FillCache(CacheFill::StandardLines).into(),
    ));
    assert!(pane_shows(&rig, "Cancelar"));
    send_without_running(&mut rig, TreeMessage::CancelCacheJob);
    rig.drain(task);
    assert_eq!(
        rig.view().cache.status,
        CacheStatus::Failed(CacheIssue::Cancelled)
    );
    assert!(pane_shows(&rig, "Cancelado"));
}

#[test]
fn the_information_with_a_file_shows_its_size_in_both_languages() {
    for (language, tr) in [(LanguagePref::EnUs, en()), (LanguagePref::PtBr, pt())] {
        let dir = tempdir().unwrap();
        let mut rig = panel(language, dir.path(), true);
        rig.app_msg(TreeMessage::SaveCache);
        rig.app_msg(TreeMessage::CacheInfo);
        let CacheStatus::Info(info) = rig.view().cache.status else {
            panic!("not the information");
        };
        let bytes = info.file_bytes.expect("the file exists");
        let mut args = FluentArgs::new();
        args.set("bytes", bytes.to_string());
        let file = tr.tr_args("tree-cache-info-file", &args);
        let head = tr.tr_args("tree-cache-info-text", &{
            let mut a = FluentArgs::new();
            a.set("used", i64::try_from(info.used).unwrap());
            a.set("capacity", i64::try_from(info.capacity).unwrap());
            a.set("backup", i64::try_from(info.backup_used).unwrap());
            a.set(
                "backup_capacity",
                i64::try_from(info.backup_capacity).unwrap(),
            );
            a
        });
        let mut games = FluentArgs::new();
        games.set("games", i64::from(info.lowest_total.expect("a tree")));
        let line = format!(
            "{head} {} {file}",
            tr.tr_args("tree-cache-info-lowest", &games)
        );
        assert!(pane_shows(&rig, &line), "{line}");
        assert!(!line.contains(&tr.tr("tree-cache-info-no-file")));
    }
}

#[test]
fn saving_in_the_clipbase_is_refused_with_the_not_scid_line() {
    let dir = tempdir().unwrap();
    let mut rig = panel(LanguagePref::EnUs, dir.path(), true);
    let slot = rig.app().bases.current();
    rig.app_msg(BasesMessage::List(ListMessage::SelectNumbers {
        slot,
        numbers: vec![1],
    }));
    rig.app_msg(BasesMessage::List(ListMessage::CopySelected { slot }));
    rig.app_msg(BasesMessage::Select(SlotId::CLIPBASE));
    rig.app_msg(TreeMessage::SaveCache);
    assert_eq!(
        rig.view().cache.status,
        CacheStatus::Failed(CacheIssue::NotScidBase)
    );
    assert!(pane_shows(
        &rig,
        "Only Scid databases (.si4) have a cache file"
    ));
}

#[test]
fn the_auto_save_box_shows_the_preference_and_the_tooltip_text_exists() {
    let dir = tempdir().unwrap();
    let mut rig = panel(LanguagePref::EnUs, dir.path(), true);
    rig.app_msg(TreeMessage::SetCacheAutosave(true));
    assert!(rig.view().cache.autosave);
    // Unchecking sends the opposite value.
    let mut sim = pane_sim(&rig);
    sim.click("Save cache automatically").unwrap();
    let messages: Vec<_> = sim.into_messages().collect();
    assert!(sent(
        &messages,
        &tree_msg(TreeMessage::SetCacheAutosave(false))
    ));
    assert!(en().has("tree-cache-autosave-tip") && pt().has("tree-cache-autosave-tip"));
}

#[test]
fn a_busy_save_or_info_shows_its_own_line() {
    let dir = tempdir().unwrap();
    let mut rig = panel(LanguagePref::EnUs, dir.path(), true);
    let task = rig
        .rig
        .shell
        .update(Message::App(TreeMessage::SaveCache.into()));
    assert!(pane_shows(&rig, "Saving the cache..."));
    rig.drain(task);
    let task = rig
        .rig
        .shell
        .update(Message::App(TreeMessage::CacheInfo.into()));
    assert!(pane_shows(&rig, "Reading the cache..."));
    rig.drain(task);
}

#[test]
fn opening_the_panel_does_not_open_a_layer_or_start_a_job() {
    let dir = tempdir().unwrap();
    let rig = panel(LanguagePref::EnUs, dir.path(), true);
    assert!(!rig.rig.shell.modal_layer_open());
    assert!(!rig.app().tree_cache_busy());
    assert_eq!(rig.view().cache.status, CacheStatus::None);
    assert!(rig.view().cache.has_file);
}
