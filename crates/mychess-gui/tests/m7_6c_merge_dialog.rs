//! M7-6c (GUI): the "Merge game" dialog over the real application (spec
//! `docs/specs/m7-6-mesclar-partida.md`, criteria 16 and 17), headless. The
//! base is a PGN in a slot and the tasks the shell returns run to the end.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::fmt::Write as _;

#[path = "m5_2_gui_common/mod.rs"]
mod base;

use base::{
    AppMessage, BasesMessage, Key, KeyInput, Message, Modifiers, NamedKey, TreeRig, en, pt,
};
use iced::advanced::widget::operation::Scrollable;
use iced::widget::Id;
use iced::widget::scrollable::{AbsoluteOffset, RelativeOffset};
use iced::{Rectangle, Size, Vector};
use iced_test::Simulator;
use iced_test::futures::futures::StreamExt;
use iced_test::runtime::{Action as RtAction, task::into_stream};
use iced_test::selector::Candidate;
use mychess_app::{
    Action, AppNotice, GameMessage, LanguagePref, MergeMessage, MergeNotice, SlotId,
};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::dialog_ui::ActiveDialog;
use mychess_gui::windows::game_list::{GameListMessage, ListMenu, MenuTarget};
use mychess_gui::windows::layout::PaneKind;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::windows::merge_dialog as md;

const A: u32 = 1;
const B: u32 = 2;
const C: u32 = 3;
const D: u32 = 4;
const E: u32 = 5;
const OTHER_START: u32 = 6;

const PGN: &str = "[Event \"Evt\"]\n[Date \"2019.01.01\"]\n[White \"W1\"]\n[Black \"B1\"]\n[Result \"*\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 *\n\n\
[Event \"Aberto\"]\n[Date \"2020.05.05\"]\n[White \"Alfa, A\"]\n[Black \"Beta, B\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 4. c3 Nf6 1-0\n\n\
[Event \"Evt\"]\n[Result \"*\"]\n\n1. Nf3 e5 2. e4 Nc6 3. Bc4 Bc5 *\n\n\
[Event \"Evt\"]\n[Result \"*\"]\n\n1. d4 d5 2. c4 e6 *\n\n\
[Event \"Evt\"]\n[Result \"*\"]\n\n1. e4 e5 *\n\n\
[Event \"Evt\"]\n[Result \"*\"]\n[SetUp \"1\"]\n[FEN \"4k3/8/8/8/8/8/4P3/4K3 w - - 0 1\"]\n\n1. e4 Kd7 *\n\n";

struct T {
    rig: TreeRig,
    slot: SlotId,
    _dir: tempfile::TempDir,
}

impl T {
    fn new(language: LanguagePref) -> T {
        let t = T::with_pgn(language, PGN);
        assert_eq!(t.app().session.game().mainline().count(), 6);
        t
    }

    /// A rig with `pgn` open and its first game in the editor.
    fn with_pgn(language: LanguagePref, pgn: &str) -> T {
        let dir = tempfile::tempdir().expect("dir");
        let mut rig = TreeRig::with_language(language);
        rig.open_pgn(dir.path(), "s.pgn", pgn);
        let slot = rig.app().bases.current();
        rig.app_msg(BasesMessage::LoadGame { slot, number: A });
        T {
            rig,
            slot,
            _dir: dir,
        }
    }

    fn app(&self) -> &mychess_app::AppState {
        self.rig.app()
    }

    fn top(&self) -> Option<ActiveDialog> {
        ActiveDialog::of(self.app(), &self.rig.rig.shell.ui.dialogs)
    }

    fn open(&mut self, number: u32) {
        self.rig.app_msg(MergeMessage::Open {
            slot: self.slot,
            number,
        });
    }

    fn key(&mut self, key: NamedKey) {
        self.rig.key(KeyInput {
            key: Key::Named(key),
            modifiers: Modifiers::NONE,
        });
    }

    fn sim(&self) -> Simulator<'_, Message> {
        let _guard = base::common::render_lock();
        let element = mychess_gui::windows::main_window::view(&self.rig.rig.shell);
        Simulator::with_size(iced::Settings::default(), Size::new(1300.0, 900.0), element)
    }

    fn nodes(&self) -> usize {
        self.app().session.game().node_count()
    }

    fn status(&self) -> Option<String> {
        let shell = &self.rig.rig.shell;
        match self.rig.rig.shell.ui.status_notice.as_ref() {
            Some(AppNotice::Merge(n)) => Some(md::notice_text(&shell.i18n, n)),
            _ => None,
        }
    }
}

fn has_text(sim: &mut Simulator<'_, Message>, needle: &str) -> bool {
    sim.find(|c: Candidate<'_>| match c {
        Candidate::Text { content, .. } if content.contains(needle) => Some(()),
        _ => None,
    })
    .is_ok()
}

fn click_messages(t: &T, id: &str) -> Vec<Message> {
    let mut sim = t.sim();
    sim.click(Id::from(id.to_owned()))
        .unwrap_or_else(|e| panic!("{id}: {e:?}"));
    sim.into_messages().collect()
}

fn clicked(t: &T, id: &str) -> String {
    format!("{:?}", click_messages(t, id))
}

/// What the preview does with a scroll: the offsets it was asked for.
#[derive(Default)]
struct Probe {
    absolute: Vec<f32>,
}

impl Scrollable for Probe {
    fn snap_to(&mut self, _offset: RelativeOffset<Option<f32>>) {}

    fn scroll_to(&mut self, offset: AbsoluteOffset<Option<f32>>) {
        self.absolute.extend(offset.y);
    }

    fn scroll_by(&mut self, _offset: AbsoluteOffset, _bounds: Rectangle, _content: Rectangle) {}
}

/// Sends `message` to the shell and runs everything it starts, like the
/// real runtime; the widget operations are run against a probe standing in
/// for the scrollable of the preview. Returns the offsets asked of it.
fn preview_scrolls(t: &mut T, message: AppMessage) -> Vec<f32> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let id = Id::new(md::MERGE_PREVIEW_ID);
    let mut probe = Probe::default();
    let mut queue = vec![t.rig.rig.shell.update(Message::App(message))];
    let mut guard = 0;
    while let Some(task) = queue.pop() {
        guard += 1;
        assert!(guard < 200, "the tasks never end");
        let Some(stream) = into_stream(task) else {
            continue;
        };
        let actions: Vec<RtAction<Message>> = rt.block_on(async {
            let mut stream = Box::pin(stream);
            let mut out = Vec::new();
            while let Some(action) = stream.next().await {
                out.push(action);
            }
            out
        });
        for action in actions {
            match action {
                RtAction::Output(message) => queue.push(t.rig.rig.shell.update(message)),
                RtAction::Widget(mut operation) => operation.scrollable(
                    Some(&id),
                    Rectangle::default(),
                    Rectangle::default(),
                    Vector::ZERO,
                    &mut probe,
                ),
                _ => {}
            }
        }
    }
    probe.absolute
}

fn enabled(t: &T, key: &str) -> bool {
    let shell = &t.rig.rig.shell;
    menu_specs(&shell.app, &shell.ui, shell.options)
        .iter()
        .flat_map(|spec| spec.entries.iter())
        .find_map(|entry| match entry {
            MenuEntry::Item {
                label: Label::Key(k),
                action,
                ..
            } if *k == key => Some(action.is_some()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{key} is in the menu"))
}

#[test]
fn opening_shows_the_preview_with_the_buttons_and_closes_the_layer_behind() {
    let mut t = T::new(LanguagePref::EnUs);
    assert!(!t.rig.rig.shell.modal_layer_open());
    t.open(B);
    assert_eq!(t.top(), Some(ActiveDialog::Merge));
    assert!(t.rig.rig.shell.modal_layer_open());
    let mut sim = t.sim();
    assert!(sim.find(Id::new(md::MERGE_BOX_ID)).is_ok());
    assert!(sim.find(Id::new(md::MERGE_PREVIEW_ID)).is_ok());
    assert!(sim.find(Id::new(md::MERGE_APPLY_ID)).is_ok());
    assert!(sim.find(Id::new(md::MERGE_CANCEL_ID)).is_ok());
    assert!(sim.find(Id::new(md::MERGE_ALL_ID)).is_ok());
    for moves in [5, 10, 15, 20, 25, 30, 35, 40] {
        assert!(
            sim.find(Id::from(md::end_button_id(moves))).is_ok(),
            "{moves}"
        );
    }
    assert!(has_text(&mut sim, "Merge preview"));
    assert!(has_text(&mut sim, "Alfa, A -- Beta, B 1-0"));
    assert!(has_text(&mut sim, "Aberto, 2020"));
    assert!(!has_text(&mut sim, "will be added to the main line"));
    // The view of the application: 40 is the chosen button (ply 80).
    let view = t.app().merge_view().expect("view");
    assert_eq!(view.end_ply, Some(80));
    assert!(view.is_new(view.first().expect("first")));
}

#[test]
fn a_button_of_the_row_sends_its_ply_and_changes_the_preview() {
    let mut t = T::new(LanguagePref::EnUs);
    t.open(B);
    let before = t.app().merge_view().unwrap().preview.node_count();
    let text = clicked(&t, &md::end_button_id(5));
    assert!(text.contains("SetEndPly(Some(10))"), "{text}");
    let text = clicked(&t, md::MERGE_ALL_ID);
    assert!(text.contains("SetEndPly(None)"), "{text}");
    // Ply 6 cuts the game: two moves instead of four.
    t.rig.app_msg(MergeMessage::SetEndPly(Some(6)));
    let view = t.app().merge_view().unwrap();
    assert_eq!(view.end_ply, Some(6));
    assert!(view.preview.node_count() < before);
    assert!(view.truncated());
}

#[test]
fn enter_applies_in_one_undo_step_with_the_comment_in_the_interface_language() {
    for (language, tail) in [
        (LanguagePref::EnUs, "(4 moves)"),
        (LanguagePref::PtBr, "(4 lances)"),
    ] {
        let mut t = T::new(language);
        let before = t.nodes();
        let depth = t.app().session.undo_depth();
        t.open(B);
        t.rig.app_msg(MergeMessage::SetEndPly(Some(6)));
        t.key(NamedKey::Enter);
        assert_eq!(t.top(), None);
        assert!(!t.rig.rig.shell.modal_layer_open());
        assert_eq!(t.nodes(), before + 2);
        assert_eq!(t.app().session.undo_depth(), depth + 1);
        let game = t.app().session.game();
        let nc6 = game.mainline().nth(3).unwrap();
        let first = game.children(nc6).unwrap()[1];
        let comment = game.comment_before(first).unwrap().expect("comment");
        assert_eq!(
            comment,
            format!("Alfa, A -- Beta, B 1-0 {tail}\nAberto, 2020")
        );
        t.rig.app_msg(GameMessage::Undo);
        assert_eq!(t.nodes(), before);
    }
}

#[test]
fn the_ok_button_applies_like_enter() {
    let mut t = T::new(LanguagePref::EnUs);
    let before = t.nodes();
    t.open(B);
    // The real click makes the message, and that message is the one applied.
    let sent = click_messages(&t, md::MERGE_APPLY_ID);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(
        matches!(
            &sent[0],
            Message::App(AppMessage::Merge(MergeMessage::Apply { comment: None }))
        ),
        "{sent:?}"
    );
    for message in sent {
        t.rig.msg(message);
    }
    assert_eq!(t.top(), None);
    assert_eq!(t.nodes(), before + 4);
    // The comment was filled in on the way (`localize`), not left to the
    // neutral one.
    let game = t.app().session.game();
    let nc6 = game.mainline().nth(3).unwrap();
    let first = game.children(nc6).unwrap()[1];
    assert_eq!(
        game.comment_before(first).unwrap(),
        Some("Alfa, A -- Beta, B 1-0\nAberto, 2020")
    );
}

#[test]
fn localize_fills_the_comment_of_a_plain_ok_only() {
    let mut t = T::new(LanguagePref::PtBr);
    t.open(B);
    let i18n = &t.rig.rig.shell.i18n;
    let plain = md::apply_message_app();
    let filled = md::localize(t.app(), i18n, plain.clone());
    assert_ne!(filled, plain, "the plain OK gets a comment");
    assert!(
        matches!(
            &filled,
            AppMessage::Merge(MergeMessage::Apply { comment: Some(text) })
                if text == "Alfa, A -- Beta, B 1-0\nAberto, 2020"
        ),
        "{filled:?}"
    );
    // An OK that already has a comment, and other messages, go through.
    let own = AppMessage::Merge(MergeMessage::Apply {
        comment: Some(String::new()),
    });
    assert_eq!(md::localize(t.app(), i18n, own.clone()), own);
    let other = md::cancel_message_app();
    assert_eq!(md::localize(t.app(), i18n, other.clone()), other);
    // Without a dialog nothing is added.
    t.rig.app_msg(MergeMessage::Cancel);
    assert_eq!(
        md::localize(t.app(), &t.rig.rig.shell.i18n, plain.clone()),
        plain
    );
}

#[test]
fn escape_and_the_cancel_button_close_without_changing_the_game() {
    let mut t = T::new(LanguagePref::EnUs);
    let before = t.app().session.game().clone();
    let depth = t.app().session.undo_depth();
    t.open(B);
    t.key(NamedKey::Escape);
    assert_eq!(t.top(), None);
    assert!(!t.rig.rig.shell.modal_layer_open());
    assert!(t.app().session.game().same_content(&before));
    assert_eq!(t.app().session.undo_depth(), depth);
    assert!(!t.app().session.is_dirty());
    t.open(B);
    let text = clicked(&t, md::MERGE_CANCEL_ID);
    assert!(text.contains("Cancel"), "{text}");
    t.rig.app_msg(md::cancel_message_app());
    assert_eq!(t.top(), None);
    assert!(t.app().session.game().same_content(&before));
}

#[test]
fn keys_do_not_reach_the_game_behind_the_dialog() {
    let mut t = T::new(LanguagePref::EnUs);
    t.open(B);
    let cursor = t.app().session.cursor();
    let last = t.app().session.game().mainline().last().unwrap();
    assert_ne!(
        cursor, last,
        "the cursor is not at the end: Right and End move it"
    );
    t.key(NamedKey::Right);
    t.key(NamedKey::End);
    assert_eq!(t.app().session.cursor(), cursor);
    assert_eq!(t.top(), Some(ActiveDialog::Merge));
    // Control: without the dialog the same keys do move the cursor.
    t.key(NamedKey::Escape);
    assert_eq!(t.top(), None);
    t.key(NamedKey::End);
    assert_eq!(
        t.app().session.cursor(),
        last,
        "End reaches the game behind"
    );
}

#[test]
fn the_end_of_the_game_shows_the_note() {
    let mut t = T::new(LanguagePref::EnUs);
    t.rig.app_msg(BasesMessage::LoadGame {
        slot: t.slot,
        number: E,
    });
    t.open(B);
    assert!(t.app().merge_view().unwrap().extends_mainline());
    let mut sim = t.sim();
    assert!(has_text(&mut sim, "will be added to the main line"));
}

#[test]
fn a_batch_asks_with_the_count_and_one_enter_merges_it_in_one_step() {
    let mut t = T::new(LanguagePref::EnUs);
    let before = t.nodes();
    let depth = t.app().session.undo_depth();
    t.rig.app_msg(MergeMessage::OpenBatch {
        slot: t.slot,
        numbers: vec![A, B, C, D, E],
    });
    assert_eq!(t.top(), Some(ActiveDialog::Merge));
    let view = t.app().merge_view().unwrap();
    assert!(view.batch);
    assert_eq!(view.numbers.len(), 4, "the game itself is left out");
    let report = view.batch_report;
    assert_eq!(report.skipped_same, 1);
    assert!(report.merged >= 2, "{report:?}");
    let (merged, skipped) = (report.merged, report.skipped());
    {
        let mut sim = t.sim();
        assert!(has_text(&mut sim, "Merge games"));
        assert!(
            has_text(&mut sim, "Merge 4 games into the current game?"),
            "count"
        );
        assert!(
            sim.find(Id::new(md::MERGE_ALL_ID)).is_err(),
            "no row of buttons"
        );
    }
    t.key(NamedKey::Enter);
    assert_eq!(t.top(), None);
    assert!(t.nodes() > before);
    assert_eq!(t.app().session.undo_depth(), depth + 1);
    assert_eq!(
        t.rig.rig.shell.ui.status_notice,
        Some(AppNotice::Merge(MergeNotice::BatchDone { merged, skipped }))
    );
    assert_eq!(
        t.status().unwrap(),
        format!("Merge complete: {merged} merged, {skipped} skipped")
    );
    t.rig.app_msg(GameMessage::Undo);
    assert_eq!(t.nodes(), before);
}

#[test]
fn refusals_show_the_reason_and_open_no_dialog() {
    let mut t = T::new(LanguagePref::EnUs);
    t.open(A);
    assert_eq!(t.top(), None);
    assert_eq!(
        t.status().unwrap(),
        "Unable to merge the selected game: Game cannot be merged into itself."
    );
    t.open(OTHER_START);
    assert_eq!(t.top(), None);
    assert_eq!(
        t.status().unwrap(),
        "Unable to merge the selected game: No matching position found."
    );
    t.open(99);
    assert_eq!(
        t.status().unwrap(),
        "Unable to merge the selected game: Invalid game number."
    );
    // Portuguese.
    let mut p = T::new(LanguagePref::PtBr);
    p.open(A);
    assert_eq!(
        p.status().unwrap(),
        "Não foi possível mesclar a partida escolhida: A partida não pode ser mesclada nela mesma."
    );
}

#[test]
fn a_game_without_moves_is_refused_and_the_menu_is_disabled() {
    let mut t = T::new(LanguagePref::EnUs);
    t.rig
        .app_msg(AppMessage::Command(mychess_app::Command::NewGame));
    assert!(!md::can_merge(t.app()));
    assert!(!enabled(&t, "menu-edit-merge-game"));
    t.open(B);
    assert_eq!(t.top(), None);
    assert_eq!(
        t.status().unwrap(),
        "Unable to merge the selected game: The current game has no moves."
    );
}

#[test]
fn the_edit_menu_item_needs_a_selected_game_and_runs_the_action() {
    let mut t = T::new(LanguagePref::EnUs);
    t.rig.app_msg(BasesMessage::SelectRow {
        slot: t.slot,
        number: None,
    });
    assert!(!enabled(&t, "menu-edit-merge-game"), "nothing selected");
    t.rig.app_msg(BasesMessage::SelectRow {
        slot: t.slot,
        number: Some(B),
    });
    assert!(enabled(&t, "menu-edit-merge-game"));
    t.rig.ui(mychess_gui::UiMessage::Action(Action::MergeGame));
    assert_eq!(t.top(), Some(ActiveDialog::Merge));
}

#[test]
fn the_row_menu_of_the_list_offers_the_merge() {
    let mut t = T::new(LanguagePref::EnUs);
    t.rig.rig.shell.ui.show_pane(PaneKind::GameList);
    t.rig.msg(Message::Games(GameListMessage::Menu(ListMenu {
        target: MenuTarget::Row(B),
        position: iced::Point::new(100.0, 100.0),
    })));
    let mut sim = t.sim();
    assert!(has_text(&mut sim, "Merge into current game..."));
    sim.click("Merge into current game...").expect("click");
    let text = format!("{:?}", sim.into_messages().collect::<Vec<_>>());
    assert!(text.contains("Games(Act(Merge(2)))"), "{text}");
    t.rig.msg(Message::Games(GameListMessage::Act(
        mychess_gui::windows::game_list::ListAction::Merge(B),
    )));
    assert_eq!(t.top(), Some(ActiveDialog::Merge));
}

#[test]
fn the_texts_exist_in_both_languages_and_portuguese_says_variante() {
    let keys = [
        "menu-edit-merge-game",
        "action-merge-game",
        "merge-action",
        "merge-games",
        "merge-title",
        "merge-title-batch",
        "merge-source-label",
        "merge-up-to",
        "merge-all",
        "merge-note-extends",
        "merge-legend",
        "merge-preview-label",
        "merge-ok",
        "merge-cancel",
        "merge-comment-line",
        "merge-comment-moves",
        "merge-comment-event",
        "merge-batch-confirm",
        "merge-batch-summary",
        "merge-batch-skip-no-match",
        "merge-batch-skip-nothing",
        "merge-batch-skip-same",
        "merge-batch-skip-unreadable",
        "merge-batch-done",
        "merge-err-unable",
    ];
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for key in keys {
            assert!(i18n.has(key), "{key} missing in {language:?}");
        }
        for notice in [
            MergeNotice::NoGame,
            MergeNotice::Closed,
            MergeNotice::InvalidNumber,
            MergeNotice::SameGame,
            MergeNotice::EmptyGame,
            MergeNotice::Decode,
            MergeNotice::NoMatch,
            MergeNotice::NothingToAdd,
            MergeNotice::Failed,
            MergeNotice::Trial,
            MergeNotice::TooMany,
        ] {
            assert!(i18n.has(notice.message_key()), "{notice:?}");
            let text = md::notice_text(&i18n, &notice);
            assert!(!text.contains("merge-"), "{text}");
        }
        assert!(
            !md::notice_text(
                &i18n,
                &MergeNotice::BatchDone {
                    merged: 2,
                    skipped: 3
                }
            )
            .contains("merge-")
        );
    }
    let ftl = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/i18n/pt-BR/merge.ftl"),
    )
    .unwrap()
    .to_lowercase();
    assert!(!ftl.contains("variaç"), "pt-BR says variante");
    assert_eq!(en().tr("merge-all"), "All");
    assert_eq!(pt().tr("merge-all"), "Todos");
}

#[test]
fn the_dialog_is_drawn_in_a_small_and_a_large_window() {
    let mut t = T::new(LanguagePref::EnUs);
    t.open(B);
    for size in [Size::new(900.0, 600.0), Size::new(1600.0, 1000.0)] {
        let _guard = base::common::render_lock();
        let element = mychess_gui::windows::main_window::view(&t.rig.rig.shell);
        let mut sim = Simulator::with_size(iced::Settings::default(), size, element);
        assert!(sim.find(Id::new(md::MERGE_APPLY_ID)).is_ok(), "{size:?}");
    }
}

/// A game of `plies` plies that never repeats a position inside a cycle of
/// four, and a second game that only differs at the end.
fn long_pgn(cycles: u32) -> String {
    let mut main = String::new();
    for k in 0..cycles {
        let n = 2 * k + 1;
        let _ = write!(main, "{n}. Nf3 Nf6 {}. Ng1 Ng8 ", n + 1);
    }
    let tail = 2 * cycles + 1;
    format!(
        "[Event \"Long\"]\n[Result \"*\"]\n\n{main}*\n\n\
         [Event \"Tail\"]\n[Result \"*\"]\n\n1. Nf3 Nf6 2. Ng1 Ng8 3. e4 e5 4. Nf3 Nc6 *\n\n\
         [Event \"Other\"]\n[Result \"*\"]\n\n{tail}. e4 *\n\n"
    )
}

#[test]
fn the_preview_scrolls_to_the_first_merged_move_of_a_long_game() {
    // 75 cycles: 300 plies; the merge continues the line at its very end.
    let mut t = T::with_pgn(LanguagePref::EnUs, &long_pgn(75));
    assert_eq!(t.app().session.game().mainline().count(), 300);
    let slot = t.slot;
    let offsets = preview_scrolls(&mut t, MergeMessage::Open { slot, number: B }.into());
    assert_eq!(t.top(), Some(ActiveDialog::Merge));
    let view = t.app().merge_view().unwrap();
    assert!(view.extends_mainline());
    let (block, item) = view.first_position().expect("the first merged move");
    let y = md::position_y(view.preview_doc, block, item);
    assert_eq!(offsets.len(), 1, "one scroll on opening: {offsets:?}");
    let offset = offsets[0];
    assert!(offset > 0.0, "the new moves are far from the top: y = {y}");
    assert!(
        offset <= y && y < offset + md::PREVIEW_HEIGHT,
        "the first merged move ({y}) is inside the window [{offset}, +{}]",
        md::PREVIEW_HEIGHT
    );
    // Changing the ply scrolls again to the new moves.
    let offsets = preview_scrolls(&mut t, MergeMessage::SetEndPly(Some(2 * 300 + 2)).into());
    assert_eq!(offsets.len(), 1, "{offsets:?}");
    assert!(offsets[0] > 0.0);
}

#[test]
fn the_preview_of_a_short_game_stays_at_the_top() {
    let mut t = T::new(LanguagePref::EnUs);
    let slot = t.slot;
    let offsets = preview_scrolls(&mut t, MergeMessage::Open { slot, number: B }.into());
    assert_eq!(offsets, vec![0.0]);
}

#[test]
fn can_merge_is_false_in_trial_and_with_a_live_game_and_the_menu_follows() {
    let mut t = T::new(LanguagePref::EnUs);
    t.rig.app_msg(BasesMessage::SelectRow {
        slot: t.slot,
        number: Some(B),
    });
    assert!(md::can_merge(t.app()));
    assert!(enabled(&t, "menu-edit-merge-game"));
    // Trial mode.
    t.rig.app_msg(GameMessage::EnterTrial { null_move: false });
    assert!(t.app().session.in_trial());
    assert!(!md::can_merge(t.app()));
    assert!(!enabled(&t, "menu-edit-merge-game"));
    t.rig.app_msg(GameMessage::ExitTrial);
    assert!(md::can_merge(t.app()));
    // A live (online) game owns the board.
    t.rig.rig.shell.app.online.marker = Some(mychess_app::online::MarkerInfo {
        game_id: "g".to_owned(),
        date: "2026-01-01".to_owned(),
    });
    assert!(!md::can_merge(t.app()));
    assert!(!enabled(&t, "menu-edit-merge-game"));
}

#[test]
fn escape_with_a_batch_open_closes_it_without_changing_the_game() {
    let mut t = T::new(LanguagePref::EnUs);
    let before = t.app().session.game().clone();
    let depth = t.app().session.undo_depth();
    t.rig.app_msg(MergeMessage::OpenBatch {
        slot: t.slot,
        numbers: vec![B, C],
    });
    assert!(t.app().merge_view().unwrap().batch);
    t.key(NamedKey::Escape);
    assert_eq!(t.top(), None);
    assert!(!t.rig.rig.shell.modal_layer_open());
    assert!(t.app().session.game().same_content(&before));
    assert_eq!(t.app().session.undo_depth(), depth);
    assert_eq!(t.status(), None);
}

#[test]
fn the_base_cannot_be_closed_under_the_open_dialog() {
    let mut t = T::new(LanguagePref::EnUs);
    t.open(B);
    t.rig.app_msg(BasesMessage::Close(t.slot));
    assert!(t.app().bases.slot(t.slot).is_some(), "the base stays");
    assert_eq!(t.top(), Some(ActiveDialog::Merge));
    assert!(t.app().merge_view().is_some());
    t.key(NamedKey::Escape);
    assert_eq!(t.top(), None);
    t.rig.app_msg(BasesMessage::Close(t.slot));
    assert!(t.app().bases.slot(t.slot).is_none());
}
