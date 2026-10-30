//! M5-3b GUI contract: the cache bar of the opening tree panel (spec
//! `docs/specs/m5-3-cache-stc.md`, D1760-D1770): texts in both languages,
//! the messages the controls send, the job end to end and no modal layer.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_gui_common;

use std::collections::{BTreeMap, BTreeSet};

use fluent_bundle::FluentArgs;
use fluent_syntax::ast::Entry;
use fluent_syntax::parser;
use m5_2_gui_common::*;
use mychess_app::{CacheFill, CacheStatus};
use tempfile::tempdir;

fn cache_keys(language: &str) -> BTreeSet<String> {
    let text = ftl_text(language, "tree.ftl");
    let resource = parser::parse(text.as_str()).unwrap_or_else(|(r, _)| r);
    resource
        .body
        .iter()
        .filter_map(|e| match e {
            Entry::Message(m) if m.id.name.starts_with("tree-cache") => Some(m.id.name.to_owned()),
            _ => None,
        })
        .collect()
}

fn variables(language: &str) -> BTreeMap<String, BTreeSet<String>> {
    let text = ftl_text(language, "tree.ftl");
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if let Some((id, rest)) = line.split_once(" = ")
            && id.starts_with("tree-cache")
            && !id.contains(' ')
        {
            current = Some(id.to_owned());
            out.entry(id.to_owned()).or_default();
            collect(rest, out.get_mut(id).unwrap());
        } else if let Some(id) = &current
            && line.starts_with(' ')
        {
            collect(line, out.get_mut(id).unwrap());
        } else {
            current = None;
        }
    }
    out
}

fn collect(text: &str, into: &mut BTreeSet<String>) {
    let mut rest = text;
    while let Some(start) = rest.find("{ $") {
        let tail = &rest[start + 3..];
        let end = tail
            .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
            .unwrap_or(tail.len());
        into.insert(tail[..end].to_owned());
        rest = &tail[end..];
    }
}

#[test]
fn both_languages_have_the_same_cache_keys_and_variables() {
    let en_keys = cache_keys("en-US");
    let pt_keys = cache_keys("pt-BR");
    assert!(en_keys.len() >= 25, "{en_keys:?}");
    assert_eq!(en_keys, pt_keys);
    assert_eq!(variables("en-US"), variables("pt-BR"));
}

#[test]
fn the_portuguese_texts_say_variante_never_variacao() {
    let text = ftl_text("pt-BR", "tree.ftl").to_lowercase();
    assert!(!text.contains("variação") && !text.contains("variacao"));
}

#[test]
fn the_labels_are_translated() {
    assert_eq!(en().tr("tree-cache-save"), "Save cache");
    assert_eq!(pt().tr("tree-cache-save"), "Salvar cache");
    assert_eq!(en().tr("tree-cache-info"), "Cache information");
    assert_eq!(pt().tr("tree-cache-info"), "Informações do cache");
    assert_ne!(
        en().tr("tree-cache-fill-lines"),
        pt().tr("tree-cache-fill-lines")
    );
}

fn panel(dir: &std::path::Path) -> TreeRig {
    let mut rig = TreeRig::new();
    rig.open_si4(dir, "std", STD);
    rig.open_panel();
    rig
}

#[test]
fn the_panel_shows_the_size_and_the_auto_save_option() {
    let dir = tempdir().unwrap();
    let rig = panel(dir.path());
    assert!(pane_shows(&rig, "Cache size"));
    assert!(pane_shows(&rig, "Save cache automatically"));
}

#[test]
fn the_panel_is_translated_to_portuguese() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_language(mychess_app::LanguagePref::PtBr);
    rig.open_si4(dir.path(), "std", STD);
    rig.open_panel();
    assert!(pane_shows(&rig, "Tamanho do cache"));
    assert!(pane_shows(&rig, "Salvar o cache automaticamente"));
}

#[test]
fn the_auto_save_box_sends_its_message() {
    let dir = tempdir().unwrap();
    let rig = panel(dir.path());
    let mut sim = pane_sim(&rig);
    sim.click("Save cache automatically").unwrap();
    let messages: Vec<Message> = sim.into_messages().collect();
    assert!(sent(
        &messages,
        &tree_msg(TreeMessage::SetCacheAutosave(true))
    ));
}

#[test]
fn saving_shows_the_result_line_and_it_can_be_dismissed() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    rig.app_msg(TreeMessage::SaveCache);
    let CacheStatus::Saved { bytes, .. } = rig.view().cache.status else {
        panic!("not saved: {:?}", rig.view().cache.status);
    };
    assert!(dir.path().join("std.stc").exists());
    let mut args = FluentArgs::new();
    args.set("bytes", bytes.to_string());
    let line = en().tr_args("tree-cache-saved", &args);
    assert!(pane_shows(&rig, &line), "{line}");
    rig.app_msg(TreeMessage::DismissCacheStatus);
    assert!(!pane_shows(&rig, &line));
}

#[test]
fn a_fill_reports_what_it_did_and_leaves_no_layer_open() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    rig.app_msg(TreeMessage::FillCache(CacheFill::StandardLines));
    let CacheStatus::Filled { searches, hits } = rig.view().cache.status else {
        panic!("not filled: {:?}", rig.view().cache.status);
    };
    let mut args = FluentArgs::new();
    args.set("searches", i64::from(searches));
    args.set("hits", i64::from(hits));
    assert!(pane_shows(&rig, &en().tr_args("tree-cache-filled", &args)));
    assert!(
        !rig.rig.shell.modal_layer_open(),
        "the status is not a dialog (D1764)"
    );
}

#[test]
fn the_information_is_shown_as_a_line() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    rig.app_msg(TreeMessage::CacheInfo);
    let CacheStatus::Info(info) = rig.view().cache.status else {
        panic!("not the information");
    };
    let mut args = FluentArgs::new();
    args.set("used", i64::try_from(info.used).unwrap());
    args.set("capacity", i64::try_from(info.capacity).unwrap());
    args.set("backup", i64::try_from(info.backup_used).unwrap());
    args.set(
        "backup_capacity",
        i64::try_from(info.backup_capacity).unwrap(),
    );
    let head = en().tr_args("tree-cache-info-text", &args);
    let mut games = FluentArgs::new();
    games.set(
        "games",
        i64::from(info.lowest_total.expect("a tree is held")),
    );
    let line = format!(
        "{head} {} {}",
        en().tr_args("tree-cache-info-lowest", &games),
        en().tr("tree-cache-info-no-file")
    );
    assert!(pane_shows(&rig, &line), "{line}");
    assert!(!rig.rig.shell.modal_layer_open());
}

#[test]
fn a_job_in_flight_asks_the_shell_for_progress_ticks() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    assert!(!rig.app().tree_cache_busy());
    let task = rig.rig.shell.update(Message::App(
        TreeMessage::FillCache(CacheFill::StandardLines).into(),
    ));
    assert!(rig.app().tree_cache_busy());
    assert!(rig.view().cache.busy.is_some());
    rig.drain(task);
    assert!(!rig.app().tree_cache_busy());
}

#[test]
fn a_fill_in_flight_shows_the_cancel_button() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    let task = rig
        .rig
        .shell
        .update(Message::App(TreeMessage::FillCache(CacheFill::Base).into()));
    assert!(pane_shows(&rig, "Cancel"));
    let messages = {
        let mut sim = pane_sim(&rig);
        sim.click("Cancel").unwrap();
        sim.into_messages().collect::<Vec<_>>()
    };
    assert!(sent(&messages, &tree_msg(TreeMessage::CancelCacheJob)));
    for message in messages {
        rig.msg(message);
    }
    rig.drain(task);
    assert!(!rig.app().tree_cache_busy());
}
