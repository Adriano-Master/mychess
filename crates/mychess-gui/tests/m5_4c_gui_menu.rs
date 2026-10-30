//! M5-4c GUI contract: the "Mask" menu of the tree panel and its
//! preferences (spec `docs/specs/m5-4-mascaras-de-arvore.md`, D1830, D1831).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_4c_gui_common;

use m5_4c_gui_common::*;
use mychess_gui::windows::mask::cells::MaskCtx;
use mychess_gui::windows::mask::menu::entries;
use mychess_gui::windows::menu_bar::{Label, MenuEntry};
use tempfile::tempdir;

fn keys(entries: &[MenuEntry]) -> Vec<(String, bool)> {
    entries
        .iter()
        .filter_map(|entry| match entry {
            MenuEntry::Item { label, action, .. } => Some((
                match label {
                    Label::Key(key) => (*key).to_owned(),
                    Label::Text(text) => text.clone(),
                    _ => String::new(),
                },
                action.is_some(),
            )),
            MenuEntry::Submenu { label, .. } => Some((
                match label {
                    Label::Key(key) => (*key).to_owned(),
                    _ => String::new(),
                },
                true,
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn the_menu_has_every_item_of_the_spec_in_order() {
    let dir = tempdir().unwrap();
    let rig = new_mask_rig(dir.path());
    let view = rig.view();
    let ctx = MaskCtx::of(&rig.rig.shell, &view);
    let names: Vec<String> = keys(&entries(&ctx)).into_iter().map(|(k, _)| k).collect();
    assert_eq!(
        names,
        [
            "tree-mask-new",
            "tree-mask-open",
            "tree-mask-open-recent",
            "tree-mask-save",
            "tree-mask-save-as",
            "tree-mask-close",
            "tree-mask-display",
            "tree-mask-search",
            "tree-mask-fill-line",
            "tree-mask-fill-game",
            "tree-mask-fill-filter",
            "tree-mask-show-markers",
            "tree-mask-show-tooltips",
            "tree-mask-auto-load",
        ]
    );
}

#[test]
fn what_needs_a_mask_is_off_until_one_is_open() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    let ctx = |rig: &TreeRig| MaskCtx::of(&rig.rig.shell, &rig.view());
    let off = keys(&entries(&ctx(&rig)));
    let enabled = |list: &[(String, bool)], key: &str| {
        list.iter()
            .find(|(k, _)| k == key)
            .is_some_and(|(_, on)| *on)
    };
    assert!(enabled(&off, "tree-mask-new") && enabled(&off, "tree-mask-open"));
    for key in [
        "tree-mask-save",
        "tree-mask-save-as",
        "tree-mask-close",
        "tree-mask-display",
        "tree-mask-search",
        "tree-mask-fill-line",
        "tree-mask-fill-game",
        "tree-mask-fill-filter",
    ] {
        assert!(!enabled(&off, key), "{key}");
    }
    mask(&mut rig, MaskMessage::New);
    let on = keys(&entries(&ctx(&rig)));
    // Only the list of recent files is empty on a fresh install.
    assert!(
        on.iter()
            .all(|(key, on)| *on || key == "tree-mask-open-recent"),
        "{on:?}"
    );
}

#[test]
fn the_recent_files_come_from_the_preferences_even_without_a_mask() {
    let dir = tempdir().unwrap();
    let a = stm_file(dir.path(), "a", &[("", "e4")]);
    let b = stm_file(dir.path(), "b", &[("", "d4")]);
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    mask(&mut rig, MaskMessage::Open(Some(a)));
    mask(&mut rig, MaskMessage::Open(Some(b)));
    mask(&mut rig, MaskMessage::Close);
    assert!(!rig.app().mask_open());
    let ctx = MaskCtx::of(&rig.rig.shell, &rig.view());
    assert_eq!(ctx.recent.len(), 2, "newest first");
    assert!(ctx.recent[0].ends_with("b.stm"));
    let entries = entries(&ctx);
    let Some(MenuEntry::Submenu {
        entries: recent, ..
    }) = entries.iter().find(|entry| {
        matches!(
            entry,
            MenuEntry::Submenu {
                label: Label::Key("tree-mask-open-recent"),
                ..
            }
        )
    })
    else {
        panic!("the recent files are a submenu");
    };
    let files = recent
        .iter()
        .filter(|e| matches!(e, MenuEntry::Item { .. }))
        .count();
    assert_eq!(files, 2);
}

#[test]
fn the_option_items_show_their_state_and_toggle_it() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    let ctx = MaskCtx::of(&rig.rig.shell, &rig.view());
    assert!(ctx.show_markers, "on by default (D1831)");
    assert!(ctx.tooltips);
    assert!(!ctx.auto_load);
    let menu = entries(&ctx);
    let checked: Vec<(String, Option<bool>)> = menu
        .iter()
        .filter_map(|e| match e {
            MenuEntry::Item {
                label: Label::Key(k),
                checked: Some(c),
                ..
            } => Some(((*k).to_owned(), Some(*c))),
            _ => None,
        })
        .collect();
    assert_eq!(
        checked,
        [
            ("tree-mask-show-markers".to_owned(), Some(true)),
            ("tree-mask-show-tooltips".to_owned(), Some(true)),
            ("tree-mask-auto-load".to_owned(), Some(false)),
        ]
    );
    mask(&mut rig, MaskMessage::SetAutoLoad(true));
    assert!(MaskCtx::of(&rig.rig.shell, &rig.view()).auto_load);
}

#[test]
fn the_portuguese_labels_of_the_menu_have_no_variacao() {
    let en = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/i18n/pt-BR/tree.ftl"),
    )
    .unwrap();
    let mask_lines: Vec<&str> = en.lines().filter(|l| l.starts_with("tree-mask-")).collect();
    assert!(mask_lines.len() > 100);
    for line in mask_lines {
        assert!(
            !line.to_lowercase().contains("variaç"),
            "\"variante\", never \"variação\": {line}"
        );
    }
}
