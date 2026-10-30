//! Progress of opening, menus, translations and the M2 regression of the
//! game list slice (spec M3-4 slice 4b: D543 to D548). Headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::float_cmp,
    clippy::assert_is_empty
)]

mod m3_4b_common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use iced::Size;
use m3_4b_common::*;
use mychess_app::{
    BasesMessage, Command, LanguagePref, Message as AppMessage, PrefChange, PrefsMessage, perform,
};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::layout::PaneKind;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::windows::status_bar::{app_notice_text, opening_text};
use mychess_gui::{Message, UiMessage};
use rstest::rstest;

// ---- progress of opening ----

#[test]
fn a_subscription_ticks_only_while_a_database_is_opening() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let idle = rig.shell.subscription().units();
    let path = numbered(dir.path(), "a.pgn", "A", 5);
    // The effect is not run: the open stays in flight.
    rig.app_msg(AppMessage::Bases(BasesMessage::Open(path)));
    assert!(rig.shell.app.base_opening().is_some());
    assert_eq!(rig.shell.subscription().units(), idle + 1, "a 150 ms timer");
    rig.app_msg(AppMessage::Bases(BasesMessage::CancelOpen));
    // Cancelled but not answered yet: still in flight.
    assert!(rig.shell.app.base_opening().is_some());
}

#[test]
fn the_status_bar_shows_the_file_being_opened_and_a_cancel_button() {
    let dir = tempfile::tempdir().expect("dir");
    for (language, opening, cancel) in [
        (LanguagePref::EnUs, "Opening a.pgn...", "Cancel"),
        (LanguagePref::PtBr, "Abrindo a.pgn...", "Cancelar"),
    ] {
        let mut rig = Rig::with_language(language);
        rig.app_msg(AppMessage::Bases(BasesMessage::Open(numbered(
            dir.path(),
            "a.pgn",
            "A",
            5,
        ))));
        let mut sim = rig.sim(Size::new(1280.0, 800.0));
        assert!(sim.find(opening).is_ok(), "{opening}");
        assert!(sim.find(cancel).is_ok(), "{cancel}");
    }
}

#[test]
fn the_progress_text_has_the_percentage_within_zero_and_one_hundred() {
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    let strip = |s: String| s.replace(['\u{2068}', '\u{2069}'], "");
    let at = |i18n: &I18n, f: f32| strip(opening_text(i18n, "a.pgn", Some(f)));
    assert!(at(&en, 0.37).contains("a.pgn") && at(&en, 0.37).contains("37"));
    assert!(at(&en, 0.37).contains('%'));
    assert!(at(&pt, 0.37).starts_with("Abrindo"));
    assert!(at(&en, 7.0).contains("100"), "above 1 is clamped");
    assert!(at(&en, -3.0).contains('0') && !at(&en, -3.0).contains('-'));
    assert!(!at(&en, f32::NAN).is_empty());
    assert_eq!(
        strip(opening_text(&en, "a.pgn", None)),
        "Opening a.pgn...",
        "no percentage until the size is known"
    );
}

#[test]
fn cancelling_before_the_answer_leaves_no_slot_and_no_progress() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let path = numbered(dir.path(), "big.pgn", "A", 50_000);
    let effects = mychess_app::update(
        &mut rig.shell.app,
        AppMessage::Bases(BasesMessage::Open(path)),
    );
    assert_eq!(effects.len(), 1);
    rig.app_msg(AppMessage::Bases(BasesMessage::CancelOpen));
    let rt = runtime();
    for effect in effects {
        let answer = rt.block_on(perform(effect));
        rig.app_msg(answer);
    }
    assert!(rig.shell.app.base_opening().is_none());
    assert_eq!(rig.shell.app.bases.iter().count(), 1, "only the Clipbase");
    assert!(rig.shell.app.bases.current().is_clipbase());
    assert!(
        !rig.shell.ui.is_open(PaneKind::GameList),
        "no panel for a cancelled open"
    );
    let units = rig.shell.subscription().units();
    let idle = Rig::new().shell.subscription().units();
    assert_eq!(units, idle, "no timer left behind");
}

#[rstest]
#[case::missing("missing.pgn", None)]
#[case::empty("empty.pgn", Some(&b""[..]))]
#[case::binary_junk("junk.pgn", Some(&b"\x00\x01 not a chess file at all \xff\xfe"[..]))]
fn a_file_that_cannot_be_opened_names_the_file_in_the_status_bar(
    #[case] name: &str,
    #[case] content: Option<&[u8]>,
) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join(name);
    if let Some(content) = content {
        std::fs::write(&path, content).unwrap();
    }
    let mut rig = Rig::new();
    let rt = runtime();
    let effects = mychess_app::update(
        &mut rig.shell.app,
        AppMessage::Bases(BasesMessage::Open(path)),
    );
    for effect in effects {
        let answer = rt.block_on(perform(effect));
        rig.app_msg(answer);
    }
    assert_eq!(
        rig.shell.app.bases.iter().count(),
        1,
        "{name}: nothing opened"
    );
    let notice = rig
        .shell
        .ui
        .status_notice
        .clone()
        .or_else(|| rig.shell.app.notice.clone())
        .unwrap_or_else(|| panic!("{name}: a notice"));
    let i18n = I18n::new(Language::EnUs);
    let text = app_notice_text(&i18n, &notice);
    assert!(text.contains(name), "{text:?} should name {name}");
    assert!(!rig.shell.ui.is_open(PaneKind::GameList));
}

// ---- menus ----

struct Item {
    key: &'static str,
    action: Option<Message>,
    checked: Option<bool>,
}

fn flatten(entries: &[MenuEntry], out: &mut Vec<Item>) {
    for entry in entries {
        match entry {
            MenuEntry::Item {
                label,
                action,
                checked,
                ..
            } => out.push(Item {
                key: match label {
                    Label::Key(key) => key,
                    _ => "",
                },
                action: action.clone(),
                checked: *checked,
            }),
            MenuEntry::Submenu { entries, .. } => flatten(entries, out),
            MenuEntry::Separator | MenuEntry::Group(_) => {}
        }
    }
}

fn items(rig: &Rig) -> Vec<Item> {
    let mut out = Vec::new();
    for spec in menu_specs(&rig.shell.app, &rig.shell.ui, rig.shell.options) {
        flatten(&spec.entries, &mut out);
    }
    out
}

fn item<'a>(all: &'a [Item], key: &str) -> &'a Item {
    all.iter()
        .find(|i| i.key == key)
        .unwrap_or_else(|| panic!("{key} is in the menus"))
}

#[allow(clippy::ref_option)]
fn is(action: &Option<Message>, expected: &Message) -> bool {
    action
        .as_ref()
        .is_some_and(|a| format!("{a:?}") == format!("{expected:?}"))
}

fn bases(message: BasesMessage) -> Message {
    Message::App(AppMessage::Bases(message))
}

#[test]
fn the_file_menu_lists_nine_positions_marks_the_current_and_disables_the_empty() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 10));
    open(&mut rig, &numbered(dir.path(), "b.pgn", "B", 10));
    let mut out = Vec::new();
    for spec in menu_specs(&rig.shell.app, &rig.shell.ui, rig.shell.options) {
        for entry in &spec.entries {
            if let MenuEntry::Submenu {
                label: Label::Key("menu-file-databases"),
                entries,
                ..
            } = entry
            {
                flatten(entries, &mut out);
            }
        }
    }
    assert!(!out.is_empty(), "a Databases submenu in the File menu");
    let positions: Vec<&Item> = out.iter().filter(|i| i.checked.is_some()).collect();
    assert_eq!(positions.len(), 9, "8 slots and the Clipbase");
    // Occupied: 1, 2 and the Clipbase (last); the current is 2.
    let enabled: Vec<bool> = positions.iter().map(|i| i.action.is_some()).collect();
    assert_eq!(enabled.iter().filter(|e| **e).count(), 3, "{enabled:?}");
    let checked: Vec<bool> = positions.iter().map(|i| i.checked.unwrap()).collect();
    assert_eq!(checked.iter().filter(|c| **c).count(), 1, "one current");
    let current = positions
        .iter()
        .position(|i| i.checked == Some(true))
        .unwrap();
    assert!(is(
        &positions[current].action,
        &bases(BasesMessage::Select(slot(2)))
    ));
    assert!(is(
        &positions[0].action,
        &bases(BasesMessage::Select(slot(1)))
    ));
    // Next and previous.
    assert!(is(
        &item(&out, "menu-file-next-base").action,
        &bases(BasesMessage::Next)
    ));
    assert!(is(
        &item(&out, "menu-file-previous-base").action,
        &bases(BasesMessage::Previous)
    ));
}

#[test]
fn close_database_becomes_clear_the_clipbase_on_the_clipbase_and_sends_the_right_slot() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let all = items(&rig);
    assert!(all.iter().all(|i| i.key != "menu-file-close-base"));
    let clear = item(&all, "menu-file-clear-clipbase");
    assert!(is(
        &clear.action,
        &bases(BasesMessage::Close(mychess_app::SlotId::CLIPBASE))
    ));
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 10));
    let all = items(&rig);
    assert!(all.iter().all(|i| i.key != "menu-file-clear-clipbase"));
    assert!(is(
        &item(&all, "menu-file-close-base").action,
        &bases(BasesMessage::Close(slot(1)))
    ));
}

#[test]
fn the_edit_menu_clipbase_items_send_their_messages_with_coherent_states() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    // On the Clipbase: copy and paste off, clear on.
    let all = items(&rig);
    assert!(item(&all, "menu-edit-clipbase-copy").action.is_none());
    assert!(item(&all, "menu-edit-clipbase-paste").action.is_none());
    assert!(is(
        &item(&all, "menu-edit-clipbase-clear").action,
        &bases(BasesMessage::ClipbaseClear)
    ));
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 10));
    let all = items(&rig);
    assert!(is(
        &item(&all, "menu-edit-clipbase-copy").action,
        &bases(BasesMessage::ClipbaseCopy)
    ));
    assert!(
        item(&all, "menu-edit-clipbase-paste").action.is_none(),
        "empty Clipbase"
    );
    rig.send(BasesMessage::ClipbaseCopy);
    let all = items(&rig);
    assert!(is(
        &item(&all, "menu-edit-clipbase-paste").action,
        &bases(BasesMessage::ClipbasePaste)
    ));
    // Back on the Clipbase (which now has a game): paste stays off, there.
    rig.app_msg(AppMessage::Bases(BasesMessage::Select(
        mychess_app::SlotId::CLIPBASE,
    )));
    let all = items(&rig);
    assert!(item(&all, "menu-edit-clipbase-paste").action.is_none());
    assert!(item(&all, "menu-edit-clipbase-copy").action.is_none());
}

#[test]
fn the_view_menu_toggles_the_games_panel_and_shows_whether_it_is_open() {
    let mut rig = Rig::new();
    let all = items(&rig);
    let entry = item(&all, "menu-view-pane-games");
    assert_eq!(entry.checked, Some(false));
    assert!(is(
        &entry.action,
        &Message::Ui(UiMessage::TogglePane(PaneKind::GameList))
    ));
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::GameList)));
    assert_eq!(
        item(&items(&rig), "menu-view-pane-games").checked,
        Some(true)
    );
}

#[test]
fn the_menus_translate_in_both_languages() {
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    for key in [
        "menu-file-databases",
        "menu-file-next-base",
        "menu-file-previous-base",
        "menu-file-close-base",
        "menu-file-clear-clipbase",
        "menu-edit-clipbase-copy",
        "menu-edit-clipbase-paste",
        "menu-edit-clipbase-clear",
        "menu-view-pane-games",
    ] {
        let (e, p) = (en.tr(key), pt.tr(key));
        assert!(!e.is_empty() && e != key, "{key} in en-US: {e:?}");
        assert!(!p.is_empty() && p != key, "{key} in pt-BR: {p:?}");
        assert_ne!(e, p, "{key} is not translated: {e:?}");
    }
}

// ---- translations ----

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

fn ftl_ids(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter(|l| !l.starts_with(' ') && !l.starts_with('#'))
        .filter_map(|l| l.split_once('=').map(|(k, _)| k.trim().to_owned()))
        .filter(|k| !k.is_empty())
        .collect()
}

#[test]
fn games_ftl_has_the_same_keys_in_both_languages_and_none_is_empty() {
    let en = std::fs::read_to_string(repo("assets/i18n/en-US/games.ftl")).expect("en-US");
    let pt = std::fs::read_to_string(repo("assets/i18n/pt-BR/games.ftl")).expect("pt-BR");
    assert_eq!(ftl_ids(&en), ftl_ids(&pt));
    assert!(ftl_ids(&en).len() >= 28);
    for (name, text) in [("en-US", &en), ("pt-BR", &pt)] {
        for line in text
            .lines()
            .filter(|l| l.contains('=') && !l.starts_with(' '))
        {
            let value = line.split_once('=').unwrap().1.trim();
            assert!(!value.is_empty(), "{name}: empty value in {line:?}");
        }
    }
}

/// The keys the panel's sources ask for.
fn keys_used(source: &str) -> BTreeSet<String> {
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);
    let mut keys = BTreeSet::new();
    let mut rest = production;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('"') else { break };
        let literal = &after[..end];
        let widget_id = [
            "games-header",
            "games-list",
            "games-pane",
            "games-row-",
            "games-tab-",
        ]
        .iter()
        .any(|p| literal.starts_with(p));
        if !widget_id
            && literal.len() > 5
            && literal
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && [
                "games-",
                "menu-file-",
                "menu-edit-clipbase",
                "pane-games",
                "status-opening",
            ]
            .iter()
            .any(|p| literal.starts_with(p))
        {
            keys.insert(literal.to_owned());
        }
        rest = &after[end + 1..];
    }
    keys
}

#[test]
fn every_key_the_panel_asks_for_exists_in_both_languages() {
    let all = |lang: &str| -> BTreeSet<String> {
        let mut ids = BTreeSet::new();
        for entry in std::fs::read_dir(repo(&format!("assets/i18n/{lang}"))).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "ftl") {
                ids.extend(ftl_ids(&std::fs::read_to_string(path).unwrap()));
            }
        }
        ids
    };
    let (en, pt) = (all("en-US"), all("pt-BR"));
    let mut used = BTreeSet::new();
    let mut files = panel_sources();
    files.extend(
        [
            "src/shell/games_glue.rs",
            "src/windows/menu_bar.rs",
            "src/windows/status_bar.rs",
        ]
        .map(String::from),
    );
    for file in files {
        let source =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(&file)).unwrap();
        used.extend(keys_used(&source));
    }
    assert!(used.len() >= 20, "found only {used:?}");
    for key in used {
        assert!(en.contains(&key), "{key} missing in en-US");
        assert!(pt.contains(&key), "{key} missing in pt-BR");
    }
}

#[test]
fn the_panel_sources_have_no_visible_text_in_the_code() {
    let mut files = panel_sources();
    files.push("src/shell/games_glue.rs".to_owned());
    for file in files {
        let source =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(&file)).unwrap();
        let production = source.split("#[cfg(test)]").next().unwrap();
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
            "\"read-only\"",
            "\"Clipbase\"",
            "\"Unreadable",
            "\"(unreadable",
        ] {
            assert!(
                !code.contains(literal),
                "{file}: visible English {literal} in the code"
            );
        }
    }
}

// ---- M2 regression (D548) ----

fn single_game(dir: &Path, name: &str, tail: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(
        &path,
        format!(
            "[Event \"E\"]\n[White \"W\"]\n[Black \"B\"]\n[Result \"*\"]\n\n1. e4 e5 *\n\n{tail}"
        ),
    )
    .unwrap();
    path
}

/// Opens through the slots, loads game 1 when the panel list is there.
fn open_and_load(rig: &mut Rig, path: &Path) {
    rig.send(AppMessage::Bases(BasesMessage::Open(path.to_path_buf())));
    let slot = rig.shell.app.bases.current();
    if !slot.is_clipbase() {
        rig.send(BasesMessage::LoadGame { slot, number: 1 });
    }
}

#[test]
fn m2_contract_a_pgn_with_text_outside_the_games_is_flagged_as_partly_read_on_save_over_it() {
    // M2-2 (`GameOrigin::PgnFile.partial`): "limite atingido ou texto fora das
    // partidas". Replacing such a file erases that text, so the user is warned
    // with the "partly read" reason, whichever way the file was opened.
    let dir = tempfile::tempdir().expect("dir");
    let file = single_game(
        dir.path(),
        "stray.pgn",
        "this text sits between games and belongs to nobody\n\n[Event \"E2\"]\n[White \"X\"]\n[Black \"Y\"]\n[Result \"*\"]\n\n1. d4 *\n",
    );
    let mut rig = Rig::new();
    open_and_load(&mut rig, &file);
    rig.send(AppMessage::Command(Command::SaveAs));
    rig.pick(Some(&file));
    let prompt = rig
        .shell
        .app
        .overwrite_prompt
        .clone()
        .expect("replacing a multi-game file always asks");
    assert_eq!(
        prompt.reason,
        mychess_app::OverwriteReason::PartiallyRead,
        "text between games was not read; the warning must say so (M2-2)"
    );
}

#[test]
fn m2_contract_trailing_text_after_the_only_game_is_flagged_as_partly_read() {
    let dir = tempfile::tempdir().expect("dir");
    let file = single_game(
        dir.path(),
        "tail.pgn",
        "precious notes that are not a game\n",
    );
    let mut rig = Rig::new();
    open_and_load(&mut rig, &file);
    rig.send(AppMessage::Command(Command::SaveAs));
    rig.pick(Some(&file));
    assert!(
        rig.shell.app.overwrite_prompt.is_some(),
        "overwriting a file that holds text the app did not read must ask first"
    );
}

#[test]
fn m2_contract_a_clean_single_game_file_is_overwritten_without_a_prompt() {
    let dir = tempfile::tempdir().expect("dir");
    let file = single_game(dir.path(), "clean.pgn", "");
    let mut rig = Rig::new();
    open_and_load(&mut rig, &file);
    rig.send(AppMessage::Command(Command::SaveAs));
    rig.pick(Some(&file));
    assert!(rig.shell.app.overwrite_prompt.is_none());
}

#[test]
fn m2_contract_an_unsupported_variant_game_still_marks_the_file_partly_read() {
    let dir = tempfile::tempdir().expect("dir");
    let file = single_game(
        dir.path(),
        "variant.pgn",
        "[Event \"Odd\"]\n[Variant \"Crazyhouse\"]\n\n1. e4 *\n\n",
    );
    let mut rig = Rig::new();
    open_and_load(&mut rig, &file);
    rig.send(AppMessage::Command(Command::SaveAs));
    rig.pick(Some(&file));
    let prompt = rig.shell.app.overwrite_prompt.clone().expect("asked");
    assert_eq!(prompt.reason, mychess_app::OverwriteReason::PartiallyRead);
}

#[test]
fn language_switch_updates_the_status_bar_progress_text() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    rig.app_msg(AppMessage::Bases(BasesMessage::Open(numbered(
        dir.path(),
        "a.pgn",
        "A",
        5,
    ))));
    assert!(has_text(&rig, "Opening a.pgn..."));
    rig.app_msg(AppMessage::Prefs(PrefsMessage::Set(PrefChange::Language(
        LanguagePref::PtBr,
    ))));
    assert!(has_text(&rig, "Abrindo a.pgn..."));
}

/// The sources of the game list panel: `src/windows/game_list.rs` and every
/// file under `src/windows/game_list/`, except the test modules.
fn panel_sources() -> Vec<String> {
    fn walk(dir: &Path, out: &mut Vec<String>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, out);
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if path.extension().is_some_and(|e| e == "rs")
                && name != "tests.rs"
                && name != "drag_tests.rs"
            {
                let relative = path
                    .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(relative);
            }
        }
    }
    let mut out = vec!["src/windows/game_list.rs".to_owned()];
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/windows/game_list"),
        &mut out,
    );
    out
}
