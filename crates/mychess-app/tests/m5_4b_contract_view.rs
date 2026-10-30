//! Extra contract tests of the mask laid over the tree, of the lock of the
//! assistance during an online game and of the texts (spec M5-4, slice
//! M5-4b: D1827, D1828, D1835, D1860, D1864, D1875, D1836).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::large_futures, // the rig owns the whole `AppState`
    clippy::cloned_ref_to_slice_refs
)]

mod m5_4b_common;

use std::collections::BTreeSet;
use std::path::Path;

use m5_4b_common::*;
use mychess_app::online::{MarkerInfo, OnlineMessage, OnlineNotice};
use mychess_app::{FillSide, Marker, MaskColor, MaskKey, MaskNag, TreeBody};
use mychess_core::Position;
use tempfile::TempDir;

fn dir() -> TempDir {
    tempfile::tempdir().expect("temp dir")
}

fn view(rig: &Rig) -> mychess_app::MaskView {
    rig.state.tree_view().mask.expect("a mask is open")
}

fn rig_with_mask(dir: &Path) -> Rig {
    let mut rig = Rig::std_with_tree(dir);
    m(&mut rig, MaskMessage::New);
    rig
}

fn lock(rig: &mut Rig) {
    rig.send(Message::Online(OnlineMessage::MarkerRead(Some(
        MarkerInfo {
            game_id: "abcd1234".to_owned(),
            date: "2026-10-26".to_owned(),
        },
    ))));
    assert!(rig.state.assist_locked());
}

fn unlock(rig: &mut Rig) {
    rig.send(Message::Online(OnlineMessage::MarkerRead(None)));
    assert!(!rig.state.assist_locked());
}

// ---- what the mask says about the table ----

#[test]
fn the_strip_of_the_mask_keeps_the_order_of_the_mask_and_lists_illegal_entries_last() {
    let dir = dir();
    let mut rig = Rig::new();
    // Two games, so that the editor is not filled by the base.
    rig.open_pgn(
        dir.path(),
        "e4.pgn",
        "[Result \"*\"]\n\n1. e4 e5 *\n\n[Result \"*\"]\n\n1. e4 c5 *\n\n",
    );
    rig.send(TreeMessage::Open);
    m(&mut rig, MaskMessage::New);
    // Insertion order: Nc3, Ke5 (illegal), c4, d4 and e4 (in the tree).
    for text in ["Nc3", "Ke5", "c4", "d4", "e4"] {
        edit(&mut rig, text, MaskEdit::Add);
    }
    edit(&mut rig, "Ke5", MaskEdit::Comment("no way".to_owned()));
    let v = view(&rig);
    let got: Vec<(&str, bool, bool)> = v
        .only_rows
        .iter()
        .map(|r| (r.san.as_str(), r.legal, r.clickable))
        .collect();
    assert_eq!(
        got,
        [
            ("Nc3", true, true),
            ("Ke5", false, false),
            ("c4", true, true),
            ("d4", true, true)
        ]
    );
    assert_eq!(v.only_rows[1].cell.comment, "no way");
    assert!(v.only_rows.iter().enumerate().all(|(i, r)| r.index == i));
    // The tree row is not repeated in the strip, and has its cell.
    assert!(rig.row("e4").mask.expect("cell").in_mask);
    // A click on the illegal one and on an index out of range play nothing.
    for index in [1, 9, usize::MAX] {
        m(&mut rig, MaskMessage::PlayOnlyRow(index));
        assert_eq!(rig.ply(), 0, "index {index}");
    }
    // A click on a legal one plays it; then the table is the next position.
    m(&mut rig, MaskMessage::PlayOnlyRow(2));
    assert_eq!(rig.ply(), 1);
    assert_eq!(
        rig.state.session.position().fen().split(' ').next(),
        Some("rnbqkbnr/pppppppp/8/8/2P5/8/PP1PPPPP/RNBQKBNR")
    );
}

#[test]
fn a_move_with_no_nag_color_or_marker_has_a_plain_cell_and_one_with_all_has_them() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    edit(&mut rig, "e4", MaskEdit::Add);
    let plain = rig.row("e4").mask.expect("cell");
    assert!(plain.in_mask);
    assert_eq!(plain.nag, None);
    assert_eq!(plain.color, MaskColor::White);
    assert_eq!(plain.markers, [None, None]);
    assert_eq!(plain.comment, "");
    assert!(!plain.comment_more);
    edit(&mut rig, "e4", MaskEdit::Nag(Some(MaskNag::Poor)));
    edit(&mut rig, "e4", MaskEdit::Color(MaskColor::Red));
    edit(
        &mut rig,
        "e4",
        MaskEdit::Marker {
            slot: mychess_app::MarkerSlot::First,
            marker: Some(Marker::Exclude),
        },
    );
    let cell = rig.row("e4").mask.expect("cell");
    assert_eq!(cell.nag, Some(MaskNag::Poor));
    assert_eq!(cell.color, MaskColor::Red);
    assert_eq!(cell.markers, [Some(Marker::Exclude), None]);
}

#[test]
fn the_strip_follows_the_table_in_the_position_of_the_cursor_only() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    edit(&mut rig, "c4", MaskEdit::Add);
    assert_eq!(view(&rig).only_rows.len(), 1);
    rig.play("e2e4");
    assert!(view(&rig).only_rows.is_empty(), "another position");
    rig.nav(mychess_app::Nav::Start);
    assert_eq!(view(&rig).only_rows.len(), 1);
}

#[test]
fn a_transposition_shares_the_data_of_the_position() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    for uci in ["e2e4", "e7e5", "g1f3"] {
        rig.play(uci);
    }
    m(
        &mut rig,
        MaskMessage::SetPositionComment("shared".to_owned()),
    );
    edit(&mut rig, "Nc6", MaskEdit::Color(MaskColor::Green));
    // Another game reaches the same position by another order of moves.
    rig.state.session.replace_game(mychess_core::Game::new());
    m(&mut rig, MaskMessage::CloseSearch);
    for uci in ["g1f3", "e7e5", "e2e4"] {
        rig.play(uci);
    }
    assert_eq!(rig.state.mask_comment(None).as_deref(), Some("shared"));
    assert_eq!(view(&rig).position_comment, "shared");
    let only = view(&rig).only_rows;
    assert_eq!(only.len(), 1);
    assert_eq!(only[0].san, "Nc6");
    assert_eq!(only[0].cell.color, MaskColor::Green);
}

#[test]
fn the_cells_are_not_drawn_while_the_first_search_runs_or_the_base_is_not_usable() {
    let dir = dir();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    m(&mut rig, MaskMessage::New);
    edit(&mut rig, "c4", MaskEdit::Add);
    // The panel opens and its first search is still running.
    let effects = rig.raw(TreeMessage::Open);
    assert_eq!(rig.view().body, TreeBody::Loading);
    assert!(view(&rig).only_rows.is_empty(), "no strip while loading");
    assert!(rig.view().rows.iter().all(|r| r.mask.is_none()));
    // The search answers.
    for effect in effects {
        if matches!(effect, Effect::TreeSearch { .. }) {
            let answer = rig.perform(effect);
            rig.send(answer);
        }
    }
    assert_eq!(rig.view().body, TreeBody::Rows);
    assert_eq!(view(&rig).only_rows.len(), 1);
    // The panel closed: no cells either (nothing to annotate).
    rig.send(TreeMessage::Close);
    assert!(view(&rig).only_rows.is_empty());
}

// ---- standard chess only (D1835, D1875) ----

#[test]
fn in_chess960_the_mask_is_there_but_draws_nothing_and_files_still_work() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::std_with_tree(dir.path());
    m(&mut rig, MaskMessage::Open(Some(a.clone())));
    assert!(rig.row("e4").mask.is_some());
    let pos = Position::chess960_start(519).unwrap();
    rig.state
        .session
        .replace_game(mychess_core::Game::from_position(pos));
    // Any message makes `update` refresh what the mask says.
    m(&mut rig, MaskMessage::CloseSearch);
    let v = view(&rig);
    assert!(!v.enabled);
    assert!(v.only_rows.is_empty());
    assert!(rig.view().rows.iter().all(|r| r.mask.is_none()));
    assert!(rig.state.mask_legal_moves().is_empty());
    assert_eq!(v.file_name.as_deref(), Some("a.stm"), "still open");
    // Everything that edits is refused with the notice.
    for message in [
        MaskMessage::Edit {
            target: san("Nb3"),
            edit: MaskEdit::Add,
            scope: MaskScope::Line,
        },
        MaskMessage::SetPositionComment("x".to_owned()),
        MaskMessage::FillLine(FillSide::All),
        MaskMessage::FillGame,
        MaskMessage::FillFilter(FillSide::All),
    ] {
        rig.state.notice = None;
        let out = rig.raw(TreeMessage::Mask(message.clone()));
        assert!(out.is_empty(), "{message:?}: {out:?}");
        assert_eq!(notice(&rig), Some(MaskNotice::NotStandard), "{message:?}");
    }
    assert!(!dirty(&rig));
    // Files and options still work: save as, open another, close.
    let host = m(&mut rig, MaskMessage::SaveAs);
    pick(&mut rig, &host, Some(dir.path().join("b.stm")));
    assert!(dir.path().join("b.stm").is_file());
    m(&mut rig, MaskMessage::SetShowMarkers(false));
    assert!(!rig.state.prefs.mask.show_markers);
    m(&mut rig, MaskMessage::Close);
    assert!(!rig.state.mask_open());
    // Going to a position of Chess960 is not possible either: an opaque key.
    let out = m(&mut rig, MaskMessage::GoTo(MaskKey::from_text("x y z w")));
    assert!(out.iter().all(|e| !matches!(e, Effect::Exit)));
    assert_eq!(notice(&rig), Some(MaskNotice::CannotShowPosition));
}

#[test]
fn a_standard_game_brings_the_cells_back_after_chess960() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    edit(&mut rig, "e4", MaskEdit::Color(MaskColor::Green));
    let pos = Position::chess960_start(100).unwrap();
    rig.state
        .session
        .replace_game(mychess_core::Game::from_position(pos));
    m(&mut rig, MaskMessage::CloseSearch);
    assert!(!view(&rig).enabled);
    rig.state.session.replace_game(mychess_core::Game::new());
    m(&mut rig, MaskMessage::CloseSearch);
    assert!(view(&rig).enabled);
    assert_eq!(rig.row("e4").mask.expect("cell").color, MaskColor::Green);
}

// ---- the lock of the assistance during an online game (D1860) ----

#[test]
fn during_the_lock_what_shows_lines_is_dropped_with_the_notice() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    edit(&mut rig, "e4", MaskEdit::Color(MaskColor::Green));
    edit(&mut rig, "c4", MaskEdit::Add);
    lock(&mut rig);
    let key = MaskKey::from_position(&Position::new());
    for message in [
        MaskMessage::Search(Box::default()),
        MaskMessage::BuildOutline,
        MaskMessage::GoTo(key),
        MaskMessage::PlayOnlyRow(0),
    ] {
        rig.state.notice = None;
        let out = rig.raw(TreeMessage::Mask(message.clone()));
        assert!(out.is_empty(), "{message:?}: {out:?}");
        assert_eq!(
            rig.state.notice,
            Some(AppNotice::Online(OnlineNotice::AssistanceDisabled)),
            "{message:?}"
        );
    }
    assert!(rig.state.mask_search().is_none());
    assert!(rig.state.mask_outline().is_none());
    assert!(rig.state.discard_prompt.is_none());
    assert_eq!(rig.ply(), 0);
}

#[test]
fn during_the_lock_the_view_has_no_cells_no_strip_and_no_legal_moves() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    edit(&mut rig, "e4", MaskEdit::Color(MaskColor::Green));
    edit(&mut rig, "c4", MaskEdit::Add);
    m(
        &mut rig,
        MaskMessage::SetPositionComment("secret plan".to_owned()),
    );
    assert!(rig.row("e4").mask.is_some());
    assert_eq!(rig.state.mask_legal_moves().len(), 20);
    lock(&mut rig);
    let v = view(&rig);
    assert!(v.only_rows.is_empty(), "no line of the repertoire");
    assert_eq!(v.position_comment, "", "nor the comment of the position");
    assert!(rig.view().rows.iter().all(|r| r.mask.is_none()));
    assert!(rig.state.mask_legal_moves().is_empty());
    // The mask itself is not lost.
    assert_eq!(v.positions, 1);
    assert!(v.dirty);
    // The lock ends: everything comes back.
    unlock(&mut rig);
    // The lock closed the panel of the tree; the user opens it again.
    assert_eq!(rig.view().body, TreeBody::Closed);
    rig.send(TreeMessage::Open);
    let v = view(&rig);
    assert_eq!(v.only_rows.len(), 1);
    assert_eq!(v.position_comment, "secret plan");
    assert_eq!(rig.row("e4").mask.expect("cell").color, MaskColor::Green);
}

#[test]
fn during_the_lock_files_options_and_the_answers_of_the_effects_pass() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    let b = dir.path().join("b.stm");
    mask_with(&a, "e4");
    mask_with(&b, "d4");
    let mut rig = Rig::std_with_tree(dir.path());
    m(&mut rig, MaskMessage::Open(Some(a.clone())));
    lock(&mut rig);
    // Options.
    m(&mut rig, MaskMessage::SetShowMarkers(false));
    assert!(!rig.state.prefs.mask.show_markers);
    assert_ne!(
        rig.state.notice,
        Some(AppNotice::Online(OnlineNotice::AssistanceDisabled))
    );
    // Edits by text pass (they need no table).
    rig.state.notice = None;
    edit(&mut rig, "c4", MaskEdit::Add);
    assert!(dirty(&rig));
    assert_ne!(
        rig.state.notice,
        Some(AppNotice::Online(OnlineNotice::AssistanceDisabled))
    );
    // Save and open another: the question, then the read.
    m(&mut rig, MaskMessage::Save);
    assert!(!dirty(&rig));
    m(&mut rig, MaskMessage::Open(Some(b.clone())));
    assert_eq!(view(&rig).path, Some(b));
    // Save as, with the dialog.
    let host = m(&mut rig, MaskMessage::SaveAs);
    pick(&mut rig, &host, Some(dir.path().join("c.stm")));
    assert!(dir.path().join("c.stm").is_file());
    // A search and an outline that were asked before the lock may answer.
    unlock(&mut rig);
    m(&mut rig, MaskMessage::Search(Box::default()));
    assert!(rig.state.mask_search().is_some_and(|s| s.hits.is_some()));
    lock(&mut rig);
    m(&mut rig, MaskMessage::CloseSearch);
    m(&mut rig, MaskMessage::Close);
    assert!(!rig.state.mask_open());
    m(&mut rig, MaskMessage::New);
    assert!(rig.state.mask_open());
}

#[test]
fn during_the_lock_a_fill_with_the_game_and_the_line_still_work_on_the_mask() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    rig.play("e2e4");
    lock(&mut rig);
    // Filling from the game being played is the user's own work, not help.
    m(&mut rig, MaskMessage::FillGame);
    assert!(dirty(&rig));
    assert_eq!(view(&rig).positions, 1);
}

#[test]
fn the_lock_does_not_cancel_a_fill_that_was_already_running() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    let job = rig
        .raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All)))
        .into_iter()
        .find(|e| matches!(e, Effect::Mask(_)))
        .expect("job");
    lock(&mut rig);
    let answer = rig.perform(job);
    rig.send(answer);
    assert!(rig.state.mask_busy().is_none());
    assert!(matches!(notice(&rig), Some(MaskNotice::Filled { .. })));
    // `CancelFill` is not help either.
    let out = rig.raw(TreeMessage::Mask(MaskMessage::CancelFill));
    assert!(out.is_empty());
    assert_ne!(
        rig.state.notice,
        Some(AppNotice::Online(OnlineNotice::AssistanceDisabled))
    );
}

// ---- search and outline answers ----

#[test]
fn an_outline_answer_of_another_request_is_dropped_and_a_new_one_replaces_the_old() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    rig.play("e2e4");
    m(&mut rig, MaskMessage::FillGame);
    rig.nav(mychess_app::Nav::Start);
    // Two asks in a row: the first answer must not be taken for the second.
    let first = rig.raw(TreeMessage::Mask(MaskMessage::BuildOutline));
    let second = rig.raw(TreeMessage::Mask(MaskMessage::BuildOutline));
    let first_answer = rig.perform(first.into_iter().next().unwrap());
    let second_answer = rig.perform(second.into_iter().next().unwrap());
    rig.raw(first_answer);
    assert!(
        rig.state
            .mask_outline()
            .is_some_and(|o| o.outline.is_none()),
        "the old answer is dropped"
    );
    rig.raw(second_answer);
    assert!(
        rig.state
            .mask_outline()
            .is_some_and(|o| o.outline.is_some())
    );
    // Closing the outline while it runs: the answer finds nothing to fill.
    let ask = rig.raw(TreeMessage::Mask(MaskMessage::BuildOutline));
    rig.raw(TreeMessage::Mask(MaskMessage::CloseOutline));
    let answer = rig.perform(ask.into_iter().next().unwrap());
    rig.raw(answer);
    assert!(rig.state.mask_outline().is_none());
}

#[test]
fn a_search_answer_after_the_mask_was_closed_or_replaced_is_dropped() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    let ask = rig.raw(TreeMessage::Mask(MaskMessage::Search(Box::default())));
    let answer = rig.perform(ask.into_iter().next().unwrap());
    m(&mut rig, MaskMessage::Close);
    rig.raw(answer);
    assert!(rig.state.mask_search().is_none());
    assert!(!rig.state.mask_open());
}

#[test]
fn a_search_on_a_mask_that_is_not_open_says_so() {
    let mut rig = Rig::new();
    let out = rig.raw(TreeMessage::Mask(MaskMessage::Search(Box::default())));
    assert!(out.is_empty());
    assert_eq!(notice(&rig), Some(MaskNotice::NoMask));
    let out = rig.raw(TreeMessage::Mask(MaskMessage::BuildOutline));
    assert!(out.is_empty());
    assert_eq!(notice(&rig), Some(MaskNotice::NoMask));
    assert_eq!(rig.state.mask_comment(None), None);
    assert!(rig.state.mask_legal_moves().is_empty());
    assert!(rig.state.mask_busy().is_none());
    assert!(rig.state.tree_view().mask.is_none());
}

// ---- the texts (D1836) ----

/// `key -> set of $variables` of the messages `tree-mask-*` of a file.
fn mask_messages(text: &str) -> std::collections::BTreeMap<String, BTreeSet<String>> {
    let mut out = std::collections::BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("tree-mask-") {
            let key = rest.split('=').next().unwrap_or("").trim().to_owned();
            if rest.contains('=') {
                current = Some(format!("tree-mask-{key}"));
                out.entry(current.clone().unwrap())
                    .or_insert_with(BTreeSet::new);
            }
        } else if !line.starts_with(' ') && !line.is_empty() {
            current = None;
        }
        if let Some(key) = &current {
            let mut rest = line;
            while let Some(at) = rest.find("{ $") {
                let tail = &rest[at + 3..];
                let name: String = tail
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                    .collect();
                out.get_mut(key).unwrap().insert(name);
                rest = tail;
            }
        }
    }
    out
}

#[test]
fn the_mask_texts_have_the_same_keys_and_variables_in_both_languages() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/i18n");
    let read = |lang: &str| {
        let mut all = String::new();
        let dir = root.join(lang);
        let mut names: Vec<_> = std::fs::read_dir(&dir)
            .expect("the language directory")
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "ftl"))
            .collect();
        names.sort();
        for path in names {
            all.push_str(&std::fs::read_to_string(&path).unwrap());
            all.push('\n');
        }
        all
    };
    let en = mask_messages(&read("en-US"));
    let pt = mask_messages(&read("pt-BR"));
    assert!(!en.is_empty(), "the keys tree-mask-* exist");
    let en_keys: Vec<&String> = en.keys().collect();
    let pt_keys: Vec<&String> = pt.keys().collect();
    assert_eq!(en_keys, pt_keys, "the same keys in both languages");
    for (key, vars) in &en {
        assert_eq!(vars, &pt[key], "{key}: the same variables");
    }
    // Every notice and issue key the code can give is there.
    for key in [
        "tree-mask-open-failed",
        "tree-mask-save-failed",
        "tree-mask-warnings",
        "tree-mask-filled",
    ] {
        assert!(en.contains_key(key), "{key}");
    }
    // The arguments the spec names (D1866, D1876).
    assert!(
        en["tree-mask-open-failed"]
            .is_superset(&BTreeSet::from(["path".to_owned(), "reason".to_owned()]))
    );
    assert!(en["tree-mask-warnings"].contains("count"));
}

#[test]
fn the_mask_texts_never_say_variacao_and_are_not_empty() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/i18n");
    for lang in ["en-US", "pt-BR"] {
        for entry in std::fs::read_dir(root.join(lang)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "ftl") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for line in text.lines().filter(|l| l.starts_with("tree-mask-")) {
                let (key, value) = line.split_once('=').expect("a message");
                // A message may start on the next (indented) lines.
                let multiline = text
                    .lines()
                    .skip_while(|l| *l != line)
                    .nth(1)
                    .is_some_and(|next| next.starts_with(' '));
                assert!(
                    !value.trim().is_empty() || multiline,
                    "{lang}: {key} is empty"
                );
                if lang == "pt-BR" {
                    let lower = value.to_lowercase();
                    assert!(
                        !lower.contains("variaç") && !lower.contains("variac"),
                        "{key}: use `variante`: {value}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_table_that_is_still_on_screen_under_the_lock_gets_no_cells() {
    // The marker of a game in progress read at the start (D1332) locks the
    // help while the panel of the tree is open (the table is still there).
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    edit(&mut rig, "e4", MaskEdit::Color(MaskColor::Green));
    edit(&mut rig, "c4", MaskEdit::Add);
    m(&mut rig, MaskMessage::SetPositionComment("plan".to_owned()));
    assert!(rig.row("e4").mask.is_some());
    rig.state.online.marker = Some(MarkerInfo {
        game_id: "abcd1234".to_owned(),
        date: "2026-10-26".to_owned(),
    });
    assert!(rig.state.assist_locked());
    // Any message of the mask refreshes what the mask says (D1864).
    m(&mut rig, MaskMessage::CloseSearch);
    let v = view(&rig);
    assert!(v.only_rows.is_empty(), "{:?}", v.only_rows);
    assert_eq!(v.position_comment, "");
    assert!(
        rig.view().rows.iter().all(|r| r.mask.is_none()),
        "the cells of the mask are help on the position of the game"
    );
    assert!(rig.state.mask_legal_moves().is_empty());
    // And a click on a strip row that was drawn before the lock does nothing.
    m(&mut rig, MaskMessage::PlayOnlyRow(0));
    assert_eq!(rig.ply(), 0);
}
