//! M5-4c GUI contract: the dialogs of the masks (spec
//! `docs/specs/m5-4-mascaras-de-arvore.md`, D1817, D1826, D1830 to D1833).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_4c_gui_common;

use iced::widget::text_editor::{Action, Edit};
use m5_4c_gui_common::*;
use mychess_gui::windows::dialog_ui::ActiveDialog;
use mychess_gui::windows::mask::{MaskUiMessage, SearchEdit};
use tempfile::tempdir;

fn type_text(rig: &mut TreeRig, text: &str) {
    for c in text.chars() {
        let edit = if c == '\n' {
            Edit::Enter
        } else {
            Edit::Insert(c)
        };
        ui(rig, MaskUiMessage::CommentAction(Action::Edit(edit)));
    }
}

// ---- the question about saving ----

fn rig_with_named_dirty_mask(dir: &std::path::Path) -> (TreeRig, std::path::PathBuf) {
    let file = stm_file(dir, "rep", &[("", "e4")]);
    let mut rig = TreeRig::with_std(dir);
    rig.open_panel();
    mask(&mut rig, MaskMessage::Open(Some(file.clone())));
    edit_row(&mut rig, "d4", MaskEdit::Add);
    assert!(mask_view(&rig).dirty);
    (rig, file)
}

#[test]
fn closing_an_altered_mask_asks_and_the_question_is_a_modal_layer() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = rig_with_named_dirty_mask(dir.path());
    mask(&mut rig, MaskMessage::Close);
    assert_eq!(active(&rig), Some(ActiveDialog::MaskPrompt));
    assert!(rig.rig.shell.modal_layer_open());
    for text in [
        "Save the mask?",
        "The mask rep.stm was changed. Save it first?",
    ] {
        assert!(rig.shows(text), "{text}");
    }
    for button in ["Save", "Don't save", "Cancel"] {
        assert!(rig.shows(button), "{button}");
    }
}

#[test]
fn an_unnamed_mask_gets_the_text_without_a_name() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    edit_row(&mut rig, "e4", MaskEdit::Add);
    mask(&mut rig, MaskMessage::Close);
    assert!(rig.shows("The new mask was changed. Save it first?"));
}

#[test]
fn escape_cancels_the_question_and_keeps_the_mask_and_the_game_untouched() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = rig_with_named_dirty_mask(dir.path());
    let ply = rig.ply();
    mask(&mut rig, MaskMessage::Close);
    rig.key(named(NamedKey::Escape));
    assert_eq!(active(&rig), None);
    assert!(rig.app().mask_open() && mask_view(&rig).dirty);
    assert_eq!(rig.ply(), ply);
}

#[test]
fn keys_do_not_leak_from_the_question_to_the_game() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = rig_with_named_dirty_mask(dir.path());
    rig.play("e2e4");
    let nodes = rig.nodes();
    mask(&mut rig, MaskMessage::Close);
    // A move typed with the question open goes nowhere.
    rig.key(ch('e'));
    rig.key(ch('5'));
    assert_eq!(rig.nodes(), nodes);
    assert_eq!(active(&rig), Some(ActiveDialog::MaskPrompt));
}

#[test]
fn enter_saves_and_the_mask_closes_with_the_file_written() {
    let dir = tempdir().unwrap();
    let (mut rig, file) = rig_with_named_dirty_mask(dir.path());
    mask(&mut rig, MaskMessage::Close);
    rig.key(named(NamedKey::Enter));
    assert_eq!(active(&rig), None);
    assert!(!rig.app().mask_open());
    let text = std::fs::read_to_string(file).unwrap();
    assert!(text.contains("d4") && text.contains("e4"), "{text}");
}

#[test]
fn dont_save_closes_without_writing() {
    let dir = tempdir().unwrap();
    let (mut rig, file) = rig_with_named_dirty_mask(dir.path());
    mask(&mut rig, MaskMessage::Close);
    rig.click("Don't save");
    assert!(!rig.app().mask_open());
    let text = std::fs::read_to_string(file).unwrap();
    assert!(!text.contains("d4"), "{text}");
}

#[test]
fn the_question_is_in_portuguese() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_language(mychess_app::LanguagePref::PtBr);
    rig.open_pgn(dir.path(), "std.pgn", STD);
    rig.open_panel();
    mask(&mut rig, MaskMessage::New);
    edit_row(&mut rig, "e4", MaskEdit::Add);
    mask(&mut rig, MaskMessage::Close);
    for text in [
        "Salvar a máscara?",
        "A máscara nova foi alterada. Salvar antes?",
        "Não salvar",
    ] {
        assert!(rig.shows(text), "{text}");
    }
}

// ---- fill with the line ----

#[test]
fn fill_with_the_line_asks_which_side_and_fills_only_that_side() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    for uci in ["e2e4", "e7e5", "g1f3"] {
        rig.play(uci);
    }
    ui(&mut rig, MaskUiMessage::OpenFillLine);
    assert_eq!(active(&rig), Some(ActiveDialog::MaskFillLine));
    for text in [
        "Fill the mask with the line",
        "All moves",
        "White's moves",
        "Black's moves",
    ] {
        assert!(rig.shows(text), "{text}");
    }
    rig.click("White's moves");
    assert_eq!(active(&rig), None);
    mask(&mut rig, MaskMessage::Search(Box::default()));
    let hits = rig.app().mask_search().unwrap().hits.clone().unwrap();
    assert_eq!(hits.moves_total, 2, "e4 and Nf3 only");
}

#[test]
fn escape_closes_the_fill_question_and_enter_takes_all_moves() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    rig.play("e2e4");
    ui(&mut rig, MaskUiMessage::OpenFillLine);
    rig.key(named(NamedKey::Escape));
    assert_eq!(active(&rig), None);
    assert!(!mask_view(&rig).dirty);
    ui(&mut rig, MaskUiMessage::OpenFillLine);
    rig.key(named(NamedKey::Enter));
    assert_eq!(active(&rig), None);
    assert!(mask_view(&rig).dirty, "the line was added");
}

// ---- the comment editor ----

#[test]
fn the_editor_of_the_comment_of_a_move_is_multiline_and_saves_with_ok() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    open_context(&mut rig, "e4", false);
    rig.click("Comment the move");
    assert_eq!(active(&rig), Some(ActiveDialog::MaskComment));
    assert!(rig.shows("Comment on the move e4"));
    type_text(&mut rig, "first\nsecond");
    rig.click("OK");
    assert_eq!(active(&rig), None);
    assert_eq!(
        rig.app()
            .mask_comment(Some(&MaskTarget::Row(rig.row("e4").index)))
            .as_deref(),
        Some("first\nsecond")
    );
    assert!(
        rig.row("e4").mask.unwrap().in_mask,
        "commenting adds the move"
    );
}

#[test]
fn the_editor_starts_with_the_text_the_mask_has_and_escape_discards_the_draft() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    edit_row(&mut rig, "e4", MaskEdit::Comment("old".to_owned()));
    open_context(&mut rig, "e4", false);
    rig.click("Comment the move");
    let draft = rig.rig.shell.ui.dialogs.mask.comment.as_ref().unwrap();
    assert_eq!(draft.content.text().trim(), "old");
    type_text(&mut rig, " more");
    rig.key(named(NamedKey::Escape));
    assert_eq!(active(&rig), None);
    assert_eq!(
        rig.app()
            .mask_comment(Some(&MaskTarget::Row(rig.row("e4").index)))
            .as_deref(),
        Some("old")
    );
}

#[test]
fn an_unchanged_comment_does_not_mark_the_mask_altered() {
    let dir = tempdir().unwrap();
    let file = stm_file(dir.path(), "rep", &[("", "e4")]);
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    mask(&mut rig, MaskMessage::Open(Some(file)));
    open_context(&mut rig, "e4", false);
    rig.click("Comment the move");
    rig.click("OK");
    assert!(!mask_view(&rig).dirty);
}

#[test]
fn the_comment_of_the_position_is_edited_from_the_menu() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    open_context(&mut rig, "e4", false);
    rig.click("Comment the position");
    assert!(rig.shows("Comment on the position"));
    type_text(&mut rig, "Plan");
    rig.click("OK");
    assert_eq!(rig.app().mask_comment(None).as_deref(), Some("Plan"));
    assert!(pane_shows(&rig, "Plan"), "the strip shows it");
}

#[test]
fn control_enter_in_the_text_area_saves_and_plain_enter_is_a_line_break() {
    use iced::keyboard::{Key, Modifiers, key::Named};
    use iced::widget::text_editor::{Binding, KeyPress, Status};
    let press = |modifiers| KeyPress {
        key: Key::Named(Named::Enter),
        modified_key: Key::Named(Named::Enter),
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Enter),
        modifiers,
        text: None,
        status: Status::Focused { is_hovered: false },
    };
    let saved = mychess_gui::windows::mask::dialogs::comment_key_binding(press(Modifiers::COMMAND));
    assert!(matches!(
        saved,
        Some(Binding::Custom(Message::MaskUi(
            MaskUiMessage::CommentCommit
        )))
    ));
    assert!(matches!(
        mychess_gui::windows::mask::dialogs::comment_key_binding(press(Modifiers::empty())),
        Some(Binding::Enter)
    ));
}

// ---- the search ----

#[test]
fn the_search_dialog_lists_the_moves_that_match_every_filter_that_is_on() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    edit_row(&mut rig, "e4", MaskEdit::Nag(Some(MaskNag::Good)));
    edit_row(&mut rig, "e4", MaskEdit::Comment("sharp reply".to_owned()));
    edit_row(&mut rig, "d4", MaskEdit::Comment("quiet".to_owned()));
    ui(&mut rig, MaskUiMessage::OpenSearch);
    assert_eq!(active(&rig), Some(ActiveDialog::MaskSearch));
    for text in [
        "Search the mask",
        "Text in the comment of the move",
        "Search",
        "Close",
    ] {
        assert!(rig.shows(text), "{text}");
    }
    let listed = |rig: &TreeRig| -> Vec<String> {
        let hits = rig.app().mask_search().unwrap().hits.clone().unwrap();
        hits.rows.iter().filter_map(|hit| hit.san.clone()).collect()
    };
    ui(
        &mut rig,
        MaskUiMessage::Search(SearchEdit::MoveText("SHARP".to_owned())),
    );
    ui(&mut rig, MaskUiMessage::SearchRun);
    assert_eq!(listed(&rig), ["e4"], "case does not matter");
    assert!(
        rig.shows("Positions 0/1, moves 1/2"),
        "the summary of the Scid"
    );
    // A second filter narrows it: a NAG nobody has finds nothing.
    ui(
        &mut rig,
        MaskUiMessage::Search(SearchEdit::Nag(Some(MaskNag::Blunder))),
    );
    ui(&mut rig, MaskUiMessage::SearchRun);
    assert!(listed(&rig).is_empty());
    assert!(rig.shows("Nothing found."));
}

#[test]
fn a_double_click_on_a_result_opens_a_new_game_at_the_position_and_closes_the_dialog() {
    let dir = tempdir().unwrap();
    let file = stm_file(dir.path(), "rep", &[("e2e4", "e5")]);
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    mask(&mut rig, MaskMessage::Open(Some(file)));
    ui(&mut rig, MaskUiMessage::OpenSearch);
    ui(&mut rig, MaskUiMessage::SearchRun);
    assert!(rig.shows("e5"));
    ui(&mut rig, MaskUiMessage::SearchGo(0));
    assert_eq!(active(&rig), None, "the dialog closed");
    assert!(rig.app().mask_search().is_none());
    assert!(
        rig.app()
            .session
            .position()
            .fen()
            .starts_with("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b"),
        "a new game at the position of the row"
    );
}

#[test]
fn a_result_of_thousands_of_rows_builds_only_the_rows_in_view() {
    use mychess_db::tree::mask::{MaskKey, MaskMove, MaskPosition, TreeMask, stm};
    let dir = tempdir().unwrap();
    let mut big = TreeMask::new();
    for i in 0..5000 {
        let mut position = MaskPosition::default();
        position.moves.push(MaskMove {
            san: "e4".to_owned(),
            nag: None,
            color: MaskColor::White,
            comment: format!("note{i}"),
            markers: [None, None],
        });
        big.insert(MaskKey::Opaque(format!("k{i}")), position);
    }
    let file = dir.path().join("big.stm");
    stm::write(&file, &big).unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    mask(&mut rig, MaskMessage::Open(Some(file)));
    ui(&mut rig, MaskUiMessage::OpenSearch);
    ui(&mut rig, MaskUiMessage::SearchRun);
    let hits = rig.app().mask_search().unwrap().hits.clone().unwrap();
    assert_eq!(hits.rows.len(), 5000);
    assert!(rig.shows("note0"));
    assert!(!rig.shows("note4000"), "far rows are spacers, not widgets");
    // Scrolling moves the window of rows.
    ui(&mut rig, MaskUiMessage::SearchScrolled(24.0 * 4000.0));
    assert!(rig.shows("note4000") && !rig.shows("note0"));
}

#[test]
fn escape_closes_the_search_and_the_application_forgets_the_result() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    edit_row(&mut rig, "e4", MaskEdit::Add);
    ui(&mut rig, MaskUiMessage::OpenSearch);
    ui(&mut rig, MaskUiMessage::SearchRun);
    assert!(rig.app().mask_search().is_some());
    rig.key(named(NamedKey::Escape));
    assert_eq!(active(&rig), None);
    assert!(rig.app().mask_search().is_none());
}

// ---- the outline ----

fn outline_rig(dir: &std::path::Path) -> TreeRig {
    let file = stm_file(
        dir,
        "rep",
        &[
            ("", "e4"),
            ("e2e4", "e5"),
            ("e2e4", "c5"),
            ("e2e4 e7e5", "Nf3"),
        ],
    );
    let mut rig = TreeRig::with_std(dir);
    rig.open_panel();
    mask(&mut rig, MaskMessage::Open(Some(file)));
    mask(&mut rig, MaskMessage::BuildOutline);
    rig
}

#[test]
fn the_outline_shows_the_lines_merged_and_folds_and_unfolds() {
    let dir = tempdir().unwrap();
    let mut rig = outline_rig(dir.path());
    assert_eq!(active(&rig), Some(ActiveDialog::MaskOutline));
    for text in [
        "Mask",
        "Expand all",
        "Collapse all",
        "Refresh",
        "Show comments",
    ] {
        assert!(rig.shows(text), "{text}");
    }
    // One move with two answers: `e4`, then `e5 Nf3` (merged) and `c5`.
    assert!(rig.shows("e4"));
    assert!(rig.shows("e5 Nf3"), "a run of single moves is one line");
    assert!(rig.shows("c5"));
    ui(&mut rig, MaskUiMessage::OutlineToggle(0));
    assert!(!rig.shows("c5"), "folded");
    ui(&mut rig, MaskUiMessage::OutlineExpandAll);
    assert!(rig.shows("c5"));
    ui(&mut rig, MaskUiMessage::OutlineCollapseAll);
    assert!(!rig.shows("c5") && rig.shows("e4"));
}

#[test]
fn a_double_click_on_a_node_goes_to_the_position_after_its_last_move() {
    let dir = tempdir().unwrap();
    let mut rig = outline_rig(dir.path());
    // Row 0 is `e4`, row 1 is `e5 Nf3`.
    ui(&mut rig, MaskUiMessage::OutlineGo(1));
    assert_eq!(active(&rig), None, "the outline closed");
    assert!(rig.app().mask_outline().is_none());
    assert!(
        rig.app()
            .session
            .position()
            .fen()
            .starts_with("rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b"),
        "{}",
        rig.app().session.position().fen()
    );
}

#[test]
fn escape_closes_the_outline_and_the_empty_one_says_so() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    mask(&mut rig, MaskMessage::BuildOutline);
    assert!(rig.shows("The mask has no moves from this position."));
    rig.key(named(NamedKey::Escape));
    assert_eq!(active(&rig), None);
    assert!(rig.app().mask_outline().is_none());
}

// ---- progress and the status bar ----

#[test]
fn the_job_that_runs_shows_its_progress_and_a_fill_can_be_stopped() {
    use mychess_app::{MaskBusy, MaskJobKind};
    use mychess_gui::windows::mask::menu::progress;
    let i18n = en();
    let busy = MaskBusy {
        kind: MaskJobKind::Fill,
        done: 3,
        total: 10,
        cancelling: false,
    };
    let mut sim = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(600.0, 100.0),
        progress(&busy, &i18n),
    );
    assert!(sim.find("Filling the mask: game 3 of 10").is_ok());
    sim.click("Stop").unwrap();
    let sent: Vec<Message> = sim.into_messages().collect();
    assert!(
        sent.iter()
            .any(|m| dbg(m) == dbg(&Message::App(mask_msg(MaskMessage::CancelFill)))),
        "{sent:?}"
    );
    let stopping = MaskBusy {
        cancelling: true,
        ..busy
    };
    let mut sim = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(600.0, 100.0),
        progress(&stopping, &i18n),
    );
    assert!(sim.find("Stopping...").is_ok());
    assert!(sim.find("Stop").is_err(), "nothing to press while it stops");
}

#[test]
fn the_notices_of_the_masks_reach_the_status_bar_with_their_arguments() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    mask(
        &mut rig,
        MaskMessage::Open(Some(dir.path().join("missing.stm"))),
    );
    let notice = rig
        .rig
        .shell
        .ui
        .status_notice
        .clone()
        .expect("the remark reached the status bar");
    let text = mychess_gui::windows::status_bar::app_notice_text(&en(), &notice);
    assert!(
        text.contains("missing.stm") && text.contains("the file was not found"),
        "the path and the translated reason are filled in: {text}"
    );
    let pt = mychess_gui::windows::status_bar::app_notice_text(&pt(), &notice);
    assert!(pt.contains("não foi encontrado"), "{pt}");
}
