//! M5-2c GUI contract: Training, Lock and "Open base as tree" in the panel
//! (spec `docs/specs/m5-2-janela-da-arvore.md`, D1620-D1642, behaviours 2-4,
//! and the Fluent texts of M5-2b/c).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_gui_common;

use std::collections::{BTreeMap, BTreeSet};

use m5_2_gui_common::*;
use mychess_app::{Effect, LanguagePref, PickKind, SlotId};
use mychess_gui::host::{HostWork, PickDialog, classify};
use tempfile::tempdir;

const OTHER: &str = "[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n";

fn panel(dir: &std::path::Path) -> TreeRig {
    let mut rig = TreeRig::with_std(dir);
    rig.open_panel();
    assert_eq!(rig.view().body, TreeBody::Rows);
    rig
}

fn two(dir: &std::path::Path) -> TreeRig {
    let mut rig = TreeRig::new();
    rig.open_pgn(dir, "a.pgn", STD);
    let a = rig.app().bases.current();
    rig.open_pgn(dir, "b.pgn", OTHER);
    rig.app_msg(BasesMessage::Select(a));
    rig.open_panel();
    rig
}

fn click_no_run(rig: &TreeRig, text: &str) -> Vec<Message> {
    let mut sim = pane_sim(rig);
    sim.click(text).unwrap_or_else(|e| panic!("{text}: {e:?}"));
    sim.into_messages().collect()
}

// ---- training ----

#[test]
fn the_training_box_is_there_and_sends_the_message() {
    let dir = tempdir().unwrap();
    let rig = panel(dir.path());
    assert!(pane_shows(&rig, "Training"));
    let messages = click_no_run(&rig, "Training");
    assert!(
        sent(&messages, &tree_msg(TreeMessage::SetTraining(true))),
        "{messages:?}"
    );
}

#[test]
fn the_training_box_is_disabled_when_training_cannot_start() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    rig.app_msg(TreeMessage::ToggleAutoRefresh);
    assert!(!rig.view().training_available);
    assert!(click_no_run(&rig, "Training").is_empty());
}

#[test]
fn turning_the_training_on_hides_the_moves_and_the_rows_stop_answering() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    assert!(pane_shows(&rig, "d4") && pane_shows(&rig, "Nf3"));
    click_pane(&mut rig, "Training");
    assert!(rig.app().tree().training());
    assert!(rig.app().session.in_trial());
    for san in ["e4", "d4", "Nf3"] {
        assert!(!pane_shows(&rig, san), "{san} is hidden in the training");
    }
    // The rows are still there with their numbers, but a click does nothing.
    let mut sim = pane_sim(&rig);
    let bounds = row_bounds(&mut sim, "tree-row-0");
    drop(sim);
    assert!(bounds.height > 0.0);
    let nodes = rig.nodes();
    let messages: Vec<Message> = {
        let mut sim = pane_sim(&rig);
        sim.click(iced::widget::Id::from("tree-row-0".to_owned()))
            .unwrap();
        sim.into_messages().collect()
    };
    assert_eq!(row_clicks(&messages), 0, "{messages:?}");
    assert_eq!(rig.nodes(), nodes);
}

#[test]
fn the_tree_answers_a_move_of_the_board_through_the_shell() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    click_pane(&mut rig, "Training");
    rig.play("e2e4");
    assert_eq!(rig.ply(), 2);
    assert!(rig.app().tree().training());
}

#[test]
fn the_tree_answers_a_move_of_the_keyboard_too() {
    // A move typed in the shell reaches the same flow as a drag on the board.
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    click_pane(&mut rig, "Training");
    rig.app_msg(GameMessage::PlayMove {
        mv: rig.app().session.position().parse_uci("d2d4").unwrap(),
        options: PlayOptions::default(),
    });
    assert_eq!(rig.ply(), 2);
}

#[test]
fn leaving_the_trial_turns_the_training_off_and_shows_the_moves_again() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    click_pane(&mut rig, "Training");
    rig.app_msg(GameMessage::ExitTrial);
    assert!(!rig.app().tree().training());
    assert!(pane_shows(&rig, "d4"));
}

#[test]
fn closing_the_panel_ends_the_training_and_the_trial() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    click_pane(&mut rig, "Training");
    rig.open_panel();
    assert!(!rig.panel_open());
    assert!(!rig.app().tree().training());
    assert!(!rig.app().session.in_trial());
}

#[test]
fn the_box_turns_the_training_off_again() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    click_pane(&mut rig, "Training");
    let messages = click_no_run(&rig, "Training");
    assert!(
        sent(&messages, &tree_msg(TreeMessage::SetTraining(false))),
        "{messages:?}"
    );
}

// ---- lock ----

#[test]
fn the_lock_box_sends_the_message_and_the_name_of_the_target_is_shown() {
    let dir = tempdir().unwrap();
    let mut rig = two(dir.path());
    assert!(pane_shows(&rig, "Lock"));
    assert!(pane_shows(&rig, "Database: a"));
    let messages = click_pane(&mut rig, "Lock");
    assert!(
        sent(&messages, &tree_msg(TreeMessage::SetLocked(true))),
        "{messages:?}"
    );
    assert!(rig.view().locked);
}

#[test]
fn a_locked_panel_keeps_the_name_and_the_table_when_the_base_changes() {
    let dir = tempdir().unwrap();
    let mut rig = two(dir.path());
    click_pane(&mut rig, "Lock");
    let b = rig
        .app()
        .bases
        .find_by_path(&dir.path().join("b.pgn"))
        .unwrap();
    rig.app_msg(BasesMessage::Select(b));
    assert!(pane_shows(&rig, "Database: a"));
    assert!(!pane_shows(&rig, "Database: b"));
    assert_eq!(rig.view().total.map(|t| t.games), Some(5));
    // Unlock: back to the current base.
    click_pane(&mut rig, "Lock");
    assert!(pane_shows(&rig, "Database: b"));
    assert_eq!(rig.view().total.map(|t| t.games), Some(1));
}

#[test]
fn the_clipbase_is_named_as_such() {
    let dir = tempdir().unwrap();
    let mut rig = two(dir.path());
    rig.app_msg(BasesMessage::Select(SlotId::CLIPBASE));
    assert!(pane_shows(&rig, "Database: Clipbase"));
}

#[test]
fn the_lock_falls_when_the_base_closes_and_the_box_shows_it() {
    let dir = tempdir().unwrap();
    let mut rig = two(dir.path());
    click_pane(&mut rig, "Lock");
    let a = rig.app().bases.current();
    let b = rig
        .app()
        .bases
        .find_by_path(&dir.path().join("b.pgn"))
        .unwrap();
    rig.app_msg(BasesMessage::Select(b));
    rig.app_msg(BasesMessage::Close(a));
    assert!(!rig.view().locked);
    let messages = click_no_run(&rig, "Lock");
    assert!(
        sent(&messages, &tree_msg(TreeMessage::SetLocked(true))),
        "an unchecked box asks to lock: {messages:?}"
    );
}

// ---- open base as tree ----

#[test]
fn the_button_asks_for_a_base() {
    let dir = tempdir().unwrap();
    let rig = panel(dir.path());
    assert!(pane_shows(&rig, "Open base as tree"));
    let messages = click_no_run(&rig, "Open base as tree");
    assert!(
        sent(&messages, &tree_msg(TreeMessage::OpenBaseAsTree)),
        "{messages:?}"
    );
}

#[test]
fn the_dialog_of_the_command_is_a_database_dialog_with_its_texts() {
    let en = en();
    let pt = pt();
    let mut state = mychess_app::AppState::default();
    let effects = mychess_app::update(&mut state, TreeMessage::OpenBaseAsTree.into());
    let spec = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::PickFile(spec) => Some(spec),
            _ => None,
        })
        .expect("a dialog is requested");
    assert_eq!(spec.kind, PickKind::OpenTreeBase);
    let HostWork::Pick(dialog) = classify(Effect::PickFile(spec.clone()), &en) else {
        panic!("a file dialog");
    };
    assert_eq!(dialog.title, "Open a database as a tree");
    assert!(!dialog.is_save());
    assert_eq!(dialog.filters[0].0, "Databases and PGN files");
    let extensions = &dialog.filters[0].1;
    for wanted in ["si4", "pgn", "gz"] {
        assert!(
            extensions.iter().any(|e| e.eq_ignore_ascii_case(wanted)),
            "{wanted} in {extensions:?}"
        );
    }
    assert_eq!(dialog.filters.last().unwrap().1, ["*"], "any file last");
    let dialog = PickDialog::new(&spec, &pt);
    assert_eq!(dialog.title, "Abrir uma base como árvore");
    assert_eq!(dialog.filters[0].0, "Bancos e arquivos PGN");
}

/// Asks for a base through the application of the rig and answers the
/// dialog with `path` (the dialog itself is not shown).
fn open_as_tree(rig: &mut TreeRig, path: &std::path::Path) {
    let effects = mychess_app::update(&mut rig.rig.shell.app, TreeMessage::OpenBaseAsTree.into());
    let spec = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::PickFile(spec) => Some(spec),
            _ => None,
        })
        .expect("a dialog is requested");
    rig.msg(Message::App(mychess_app::Message::FilePicked(
        spec.request,
        Some(path.to_path_buf()),
    )));
}

#[test]
fn the_shell_shows_the_panel_the_application_asked_for_and_keeps_it() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    let first = rig.app().bases.current();
    assert!(!rig.panel_open());
    let other = dir.path().join("b.pgn");
    std::fs::write(&other, OTHER).unwrap();
    open_as_tree(&mut rig, &other);
    assert!(rig.panel_open(), "the panel was opened for the user");
    assert!(rig.app().tree().is_open());
    assert!(rig.view().locked);
    assert_eq!(rig.app().bases.current(), first, "the current base is back");
    assert!(pane_shows(&rig, "Database: b"));
    // Later updates do not close it again.
    rig.nav(Nav::Start);
    rig.play("e2e4");
    assert!(rig.panel_open());
    assert!(rig.app().tree().is_open());
}

#[test]
fn the_request_is_consumed_so_closing_the_panel_sticks() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    let other = dir.path().join("b.pgn");
    std::fs::write(&other, OTHER).unwrap();
    open_as_tree(&mut rig, &other);
    assert!(rig.panel_open());
    rig.open_panel();
    assert!(!rig.panel_open(), "the shortcut closes it");
    rig.nav(Nav::Start);
    assert!(!rig.panel_open());
    assert!(!rig.app().tree().is_open());
}

#[test]
fn a_file_that_is_already_open_is_only_locked_and_shown() {
    let dir = tempdir().unwrap();
    let mut rig = two(dir.path());
    rig.open_panel();
    assert!(!rig.panel_open());
    let slots = SlotId::all()
        .filter(|s| rig.app().bases.slot(*s).is_some())
        .count();
    open_as_tree(&mut rig, &dir.path().join("b.pgn"));
    let after = SlotId::all()
        .filter(|s| rig.app().bases.slot(*s).is_some())
        .count();
    assert_eq!(slots, after);
    assert!(rig.panel_open());
    assert!(rig.view().locked);
    assert!(pane_shows(&rig, "Database: b"));
}

// ---- texts ----

const KEYS: [&str; 30] = [
    "tree-training",
    "tree-training-tip",
    "tree-lock",
    "tree-lock-tip",
    "tree-target",
    "tree-target-clipbase",
    "tree-open-base",
    "tree-best",
    "tree-best-result",
    "tree-best-sort",
    "tree-best-max",
    "tree-best-results-all",
    "tree-best-results-white",
    "tree-best-results-black",
    "tree-best-results-decisive",
    "tree-best-results-draw",
    "tree-best-results-unfinished",
    "tree-best-order-rating",
    "tree-best-order-database",
    "tree-best-columns",
    "tree-best-load",
    "tree-best-browse",
    "tree-best-copy",
    "tree-best-empty",
    "tree-best-col-moves",
    "file-dialog-open-tree-base",
    "file-filter-databases",
    "games-col-number",
    "games-col-event",
    "games-col-eco",
];

fn message_ids(language: &str, file: &str) -> BTreeSet<String> {
    ftl_text(language, file)
        .lines()
        .filter(|l| !l.starts_with(' ') && !l.starts_with('#'))
        .filter_map(|l| l.split_once(" = ").map(|(id, _)| id.trim().to_owned()))
        .collect()
}

#[test]
fn the_new_keys_exist_in_both_languages_and_translate() {
    for (name, i18n) in [("en-US", en()), ("pt-BR", pt())] {
        for key in KEYS {
            let text = i18n.tr(key);
            assert!(!text.is_empty(), "{name} {key}");
            assert_ne!(text, key, "{name}: {key} has no text");
        }
    }
}

#[test]
fn the_texts_of_the_tree_file_have_the_same_keys_and_variables_in_both_languages() {
    let ids = |lang| message_ids(lang, "tree.ftl");
    assert_eq!(ids("en-US"), ids("pt-BR"));
    let vars = |lang: &str| -> BTreeMap<String, BTreeSet<String>> {
        let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut current = String::new();
        for line in ftl_text(lang, "tree.ftl").lines() {
            if !line.starts_with(' ')
                && !line.starts_with('#')
                && let Some((id, _)) = line.split_once(" = ")
            {
                current = id.trim().to_owned();
                out.entry(current.clone()).or_default();
            }
            let mut rest = line;
            while let Some(at) = rest.find('$') {
                rest = &rest[at + 1..];
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                    .collect();
                out.entry(current.clone()).or_default().insert(name);
            }
        }
        out
    };
    assert_eq!(vars("en-US"), vars("pt-BR"));
}

#[test]
fn the_portuguese_texts_say_variante_and_never_variacao() {
    let text = ftl_text("pt-BR", "tree.ftl").to_lowercase();
    assert!(!text.contains("variação"));
    assert!(!text.contains("variações"));
}

#[test]
fn the_target_line_takes_the_name_of_the_base() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_language(LanguagePref::PtBr);
    rig.open_pgn(dir.path(), "minha.pgn", STD);
    rig.open_panel();
    assert!(pane_shows(&rig, "Base: minha"));
    assert!(pane_shows(&rig, "Treino"));
    assert!(pane_shows(&rig, "Travar"));
    assert!(pane_shows(&rig, "Abrir base como árvore"));
}

#[test]
fn the_new_code_has_no_text_fixed_in_rust_and_no_physical_pixels() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/windows");
    let mut text = std::fs::read_to_string(dir.join("tree.rs")).unwrap();
    text.push_str(&std::fs::read_to_string(dir.join("tree/best.rs")).unwrap());
    let cut = |t: &str| t.split("#[cfg(test)]").next().unwrap_or("").to_owned();
    let production = cut(&text);
    for fixed in [
        "\"Best games\"",
        "\"Training\"",
        "\"Lock\"",
        "\"Open base as tree\"",
        "\"Browse\"",
        "\"Load\"",
        "\"Copy\"",
        "\"Columns\"",
        "\"Melhores partidas\"",
        "\"Treino\"",
        "\"Travar\"",
    ] {
        assert!(!production.contains(fixed), "{fixed} is fixed in the code");
    }
    assert!(!production.contains(".unwrap()"));
    assert!(!production.contains("control()"), "shortcuts use command()");
}
