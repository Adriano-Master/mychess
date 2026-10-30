//! M5-4c regression tests of the review (spec
//! `docs/specs/m5-4-mascaras-de-arvore.md`, D1900 to D1909).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::float_cmp
)]

mod m5_4c_gui_common;

use iced::{Rectangle, Size};
use m5_4c_gui_common::*;
use mychess_app::DiscardChoice;
use mychess_gui::windows::dialog_ui::ActiveDialog;
use mychess_gui::windows::mask::SearchEdit;
use tempfile::tempdir;

fn answer(rig: &mut TreeRig, choice: DiscardChoice) {
    rig.app_msg(mychess_app::Message::AnswerDiscard(choice));
}

fn search_rig(dir: &std::path::Path) -> TreeRig {
    let file = stm_file(dir, "rep", &[("e2e4", "e5"), ("e2e4", "c5")]);
    let mut rig = TreeRig::with_std(dir);
    rig.open_panel();
    mask(&mut rig, MaskMessage::Open(Some(file)));
    rig.play("e2e4");
    ui(&mut rig, MaskUiMessage::OpenSearch);
    ui(&mut rig, MaskUiMessage::SearchRun);
    rig
}

// ---- 1: "go to" closes the dialog only when the game is replaced ----

#[test]
fn cancelling_the_question_of_go_to_keeps_the_search_and_its_result() {
    let dir = tempdir().unwrap();
    let mut rig = search_rig(dir.path());
    let rows = rig
        .app()
        .mask_search()
        .unwrap()
        .hits
        .clone()
        .unwrap()
        .rows
        .len();
    assert_eq!(rows, 2);
    // The game was played on (altered): going to a position asks first.
    ui(&mut rig, MaskUiMessage::SearchGo(0));
    assert_eq!(active(&rig), Some(ActiveDialog::Discard));
    answer(&mut rig, DiscardChoice::Cancel);
    assert_eq!(active(&rig), Some(ActiveDialog::MaskSearch), "still open");
    assert_eq!(
        rig.app()
            .mask_search()
            .unwrap()
            .hits
            .clone()
            .unwrap()
            .rows
            .len(),
        2,
        "the result is kept"
    );
    // Asking again and discarding the game does replace it, and closes.
    ui(&mut rig, MaskUiMessage::SearchGo(0));
    answer(&mut rig, DiscardChoice::DontSave);
    assert_eq!(active(&rig), None);
    assert!(rig.app().mask_search().is_none());
}

#[test]
fn cancelling_the_question_of_go_to_keeps_the_outline() {
    let dir = tempdir().unwrap();
    let file = stm_file(dir.path(), "rep", &[("", "e4"), ("e2e4", "e5")]);
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    mask(&mut rig, MaskMessage::Open(Some(file)));
    rig.play("d2d4");
    mask(&mut rig, MaskMessage::BuildOutline);
    ui(&mut rig, MaskUiMessage::OutlineSelect(0));
    ui(&mut rig, MaskUiMessage::OutlineGo(0));
    answer(&mut rig, DiscardChoice::Cancel);
    assert_eq!(active(&rig), Some(ActiveDialog::MaskOutline));
    assert!(rig.app().mask_outline().is_some());
}

// ---- 2: the selection of the outline follows the node ----

fn outline_rig(dir: &std::path::Path) -> TreeRig {
    let file = stm_file(
        dir,
        "rep",
        &[
            ("", "e4"),
            ("", "d4"),
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
fn folding_a_node_above_does_not_send_enter_to_another_position() {
    let dir = tempdir().unwrap();
    let mut rig = outline_rig(dir.path());
    // Rows: e4, e5 Nf3, c5, d4.
    ui(&mut rig, MaskUiMessage::OutlineSelect(3));
    ui(&mut rig, MaskUiMessage::OutlineToggle(0));
    rig.key(named(NamedKey::Enter));
    assert!(
        rig.app()
            .session
            .position()
            .fen()
            .starts_with("rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b"),
        "the position after d4, not after what the old row 3 became"
    );
}

#[test]
fn a_selected_node_that_is_folded_away_makes_enter_do_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = outline_rig(dir.path());
    ui(&mut rig, MaskUiMessage::OutlineSelect(2));
    ui(&mut rig, MaskUiMessage::OutlineToggle(0));
    rig.key(named(NamedKey::Enter));
    assert_eq!(active(&rig), Some(ActiveDialog::MaskOutline));
    ui(&mut rig, MaskUiMessage::OutlineSelect(1));
    ui(&mut rig, MaskUiMessage::OutlineCollapseAll);
    rig.key(named(NamedKey::Enter));
    assert_eq!(
        active(&rig),
        Some(ActiveDialog::MaskOutline),
        "selection cleared"
    );
}

// ---- 3: the timer of the progress ----

#[test]
fn a_timer_ticks_only_while_a_job_of_the_mask_runs() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    let idle = rig.rig.shell.subscription().units();
    let effects = mychess_app::update(
        &mut rig.rig.shell.app,
        mask_msg(MaskMessage::FillFilter(FillSide::All)),
    );
    assert!(rig.app().mask_busy().is_some());
    assert_eq!(
        rig.rig.shell.subscription().units(),
        idle + 1,
        "progress timer"
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let effect = effects
        .into_iter()
        .find(|e| matches!(e, mychess_app::Effect::Mask(_)))
        .expect("the job");
    let answer = runtime.block_on(mychess_app::perform(effect));
    rig.app_msg(answer);
    assert!(rig.app().mask_busy().is_none());
    // The remark "mask filled" has its own expiry timer; the progress one is
    // gone.
    let remark = usize::from(rig.rig.shell.has_temporary_notice());
    assert_eq!(
        rig.rig.shell.subscription().units(),
        idle + remark,
        "no job, no progress timer"
    );
}

// ---- 4: the dialogs fit the smallest window ----

fn inside(bounds: Rectangle, window: Size) -> bool {
    bounds.x >= 0.0
        && bounds.y >= 0.0
        && bounds.x + bounds.width <= window.width + 0.5
        && bounds.y + bounds.height <= window.height + 0.5
}

#[test]
fn the_search_and_outline_dialogs_keep_their_buttons_in_an_800_by_560_window() {
    let window = Size::new(800.0, 560.0);
    for language in [
        mychess_app::LanguagePref::EnUs,
        mychess_app::LanguagePref::PtBr,
    ] {
        let dir = tempdir().unwrap();
        let file = stm_file(dir.path(), "rep", &[("", "e4"), ("e2e4", "e5")]);
        let mut rig = TreeRig::with_language(language);
        rig.open_pgn(dir.path(), "std.pgn", STD);
        rig.open_panel();
        mask(&mut rig, MaskMessage::Open(Some(file)));
        rig.ui(UiMessage::WindowResized(window));
        ui(&mut rig, MaskUiMessage::OpenSearch);
        ui(&mut rig, MaskUiMessage::SearchRun);
        let labels = if language == mychess_app::LanguagePref::EnUs {
            ["Search", "Close", "Go to the position"]
        } else {
            ["Buscar", "Fechar", "Ir para a posição"]
        };
        {
            let mut sim = rig.sim_at(window);
            for label in labels {
                let bounds = sim.find(label).unwrap().bounds();
                assert!(inside(bounds, window), "{label}: {bounds:?}");
            }
            let card = sim
                .find(iced::widget::Id::new(
                    mychess_gui::windows::mask::dialogs::MASK_SEARCH_BOX_ID,
                ))
                .unwrap()
                .bounds();
            assert!(inside(card, window), "the card fits: {card:?}");
        }
        ui(&mut rig, MaskUiMessage::SearchClose);
        mask(&mut rig, MaskMessage::BuildOutline);
        let mut sim = rig.sim_at(window);
        let card = sim
            .find(iced::widget::Id::new(
                mychess_gui::windows::mask::dialogs::MASK_OUTLINE_BOX_ID,
            ))
            .unwrap()
            .bounds();
        assert!(inside(card, window), "the outline fits: {card:?}");
        let close = sim.find(labels[1]).unwrap().bounds();
        assert!(inside(close, window), "{close:?}");
    }
}

#[test]
fn a_card_is_never_wider_than_the_window_less_its_margins() {
    use mychess_gui::windows::mask::dialogs::fit_width;
    assert_eq!(fit_width(880.0, Size::new(1280.0, 800.0)), 880.0);
    assert_eq!(fit_width(880.0, Size::new(800.0, 560.0)), 752.0);
    assert!(fit_width(880.0, Size::new(100.0, 560.0)) >= 280.0);
    assert_eq!(fit_width(880.0, Size::new(f32::NAN, 560.0)), 880.0);
}

// ---- 5: cheap suggestions ----

#[test]
fn the_apply_line_box_does_not_stick_after_escape() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    open_context(&mut rig, "e4", false);
    rig.click("Apply to the line");
    assert!(rig.rig.shell.ui.dialogs.mask.apply_line);
    rig.key(named(NamedKey::Escape));
    assert!(!rig.rig.shell.ui.dialogs.mask.apply_line);
    open_context(&mut rig, "e4", false);
    assert!(!rig.rig.shell.ui.dialogs.mask.apply_line);
}

#[test]
fn collapse_all_keeps_the_scroll_the_list_clamps_it() {
    let dir = tempdir().unwrap();
    let mut rig = outline_rig(dir.path());
    ui(&mut rig, MaskUiMessage::OutlineScrolled(48.0));
    ui(&mut rig, MaskUiMessage::OutlineCollapseAll);
    assert_eq!(rig.rig.shell.ui.dialogs.mask.outline.scroll, 48.0);
}

#[test]
fn a_long_comment_has_a_short_tool_tip() {
    let long = "x".repeat(70_000);
    let cut = mychess_gui::windows::mask::cells::clip_tip(&long);
    assert!(cut.chars().count() <= 2001 && cut.ends_with('…'));
}

#[test]
fn the_notices_of_a_fill_use_the_singular() {
    use mychess_app::MaskNotice;
    let filled = |moves, games| {
        mychess_gui::windows::mask::notice_text(
            &en(),
            &MaskNotice::Filled {
                moves,
                games,
                skipped: 0,
                cancelled: false,
            },
        )
    };
    assert!(
        filled(1, 1).contains("1 move from 1 game "),
        "{}",
        filled(1, 1)
    );
    assert!(filled(2, 3).contains("2 moves from 3 games"));
    let pt = mychess_gui::windows::mask::notice_text(
        &pt(),
        &MaskNotice::Filled {
            moves: 1,
            games: 2,
            skipped: 0,
            cancelled: true,
        },
    );
    assert!(pt.contains("1 lance de 2 partidas"), "{pt}");
}

#[test]
fn the_context_menu_near_the_bottom_right_corner_stays_inside_the_window() {
    let window = Size::new(800.0, 560.0);
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    rig.ui(UiMessage::WindowResized(window));
    let row = rig.row("e4");
    ui(
        &mut rig,
        MaskUiMessage::OpenContext {
            target: MaskTarget::Row(row.index),
            san: "e4".to_owned(),
            in_mask: false,
            position: iced::Point::new(790.0, 550.0),
            command: false,
        },
    );
    {
        let mut sim = rig.sim_at(window);
        let bounds = sim.find("Comment the move").unwrap().bounds();
        assert!(inside(bounds, window), "{bounds:?}");
    }
    rig.click("Add this move to the mask  ▸");
    let mut sim = rig.sim_at(window);
    let bounds = sim.find("Nc3").unwrap().bounds();
    assert!(inside(bounds, window), "{bounds:?}");
}

#[test]
fn the_question_of_the_mask_is_below_the_question_about_the_books() {
    use mychess_gui::windows::book_panel::BookQuit;
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    edit_row(&mut rig, "e4", MaskEdit::Add);
    mask(&mut rig, MaskMessage::Close);
    assert_eq!(active(&rig), Some(ActiveDialog::MaskPrompt));
    rig.rig.shell.ui.dialogs.book.quit = Some(BookQuit::Asking);
    assert_eq!(active(&rig), Some(ActiveDialog::BookQuit));
    assert!(rig.rig.shell.modal_layer_open());
    rig.rig.shell.ui.dialogs.book.quit = None;
    assert_eq!(active(&rig), Some(ActiveDialog::MaskPrompt));
}

#[test]
fn quit_with_an_altered_mask_asks_and_cancel_aborts_the_exit() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    edit_row(&mut rig, "e4", MaskEdit::Add);
    rig.app_msg(mychess_app::Message::Quit);
    assert_eq!(active(&rig), Some(ActiveDialog::MaskPrompt));
    rig.key(named(NamedKey::Escape));
    assert_eq!(active(&rig), None);
    assert!(
        rig.app().mask_open() && mask_view(&rig).dirty,
        "nothing lost"
    );
    // The exit was dropped: asking again asks again.
    rig.app_msg(mychess_app::Message::Quit);
    assert_eq!(active(&rig), Some(ActiveDialog::MaskPrompt));
}

#[test]
fn a_search_with_the_result_of_a_closed_dialog_does_not_reopen_it() {
    let dir = tempdir().unwrap();
    let mut rig = search_rig(dir.path());
    ui(
        &mut rig,
        MaskUiMessage::Search(SearchEdit::MoveTextOn(false)),
    );
    ui(&mut rig, MaskUiMessage::SearchClose);
    assert_eq!(active(&rig), None);
    ui(&mut rig, MaskUiMessage::OpenSearch);
    assert_eq!(active(&rig), Some(ActiveDialog::MaskSearch));
}
