//! M2-6 (GUI): `Ctrl+E` and `Ctrl+I` toggling (D1022), the comment draft when
//! its tab goes out of sight (D1021), and the texts (D1024).
#![allow(
    unused_qualifications,
    clippy::manual_midpoint,
    clippy::map_unwrap_or,
    clippy::large_stack_arrays,
    clippy::match_wildcard_for_single_variants,
    let_underscore_drop,
    clippy::needless_pass_by_value,
    clippy::redundant_closure_for_method_calls,
    clippy::items_after_statements,
    clippy::cast_precision_loss,
    clippy::used_underscore_binding,
    clippy::semicolon_if_nothing_returned,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

mod m2_6_gui_common;

use iced::widget::text_editor::{Action, Edit};
use m2_6_gui_common::common;
use m2_6_gui_common::*;
use mychess_app::{CommentTarget, Key};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::widgets::notation::NotationEvent;
use mychess_gui::windows::comment_editor::EditorMessage;
use mychess_gui::windows::notation::NotationMessage;
use pretty_assertions::assert_eq;

fn ctrl(rig: &mut Rig, c: char) {
    rig.key(command(Key::Char(c)), false, None);
}

fn type_char(rig: &mut Rig, c: char) {
    rig.msg(Message::Notation(NotationMessage::Editor(
        EditorMessage::Action(Action::Edit(Edit::Insert(c))),
    )));
}

fn with_layout(rig: &mut Rig, layout: &LayoutNode) {
    rig.shell.ui.set_layout(layout);
}

// ---- Ctrl+E ----

#[test]
fn ctrl_e_shows_the_comment_and_a_second_press_goes_back_to_the_moves() {
    let mut rig = rig();
    ctrl(&mut rig, 'e');
    assert_eq!(shown_in(&rig, PaneKind::Comment), Some(PaneKind::Comment));
    assert!(has(&rig, "comment-pane"));
    ctrl(&mut rig, 'e');
    assert_eq!(shown_in(&rig, PaneKind::Comment), Some(PaneKind::Notation));
    assert!(is_open(&rig, PaneKind::Comment), "hidden, not closed");
}

#[test]
fn ctrl_e_with_the_information_tab_in_front_brings_the_comment_forward() {
    let mut rig = rig();
    click_tab(&mut rig, PaneKind::Info);
    ctrl(&mut rig, 'e');
    assert_eq!(shown_in(&rig, PaneKind::Comment), Some(PaneKind::Comment));
}

#[test]
fn ctrl_e_with_the_comment_closed_opens_it_as_a_tab_in_front() {
    let mut rig = rig();
    click(&mut rig, close_id(PaneKind::Comment));
    ctrl(&mut rig, 'e');
    assert!(is_open(&rig, PaneKind::Comment));
    assert_eq!(shown_in(&rig, PaneKind::Comment), Some(PaneKind::Comment));
    assert_eq!(tabs(&rig, PaneKind::Comment).len(), 3);
}

#[test]
fn ctrl_e_hides_to_the_moves_first_whatever_the_tab_order() {
    let mut rig = rig();
    let odd = LayoutNode::default_layout()
        .close(PaneKind::Notation)
        .close(PaneKind::Info)
        .close(PaneKind::Comment)
        .open(PaneKind::Comment)
        .open(PaneKind::Info)
        .open(PaneKind::Notation)
        .select(PaneKind::Comment);
    assert_eq!(
        odd.group_of(PaneKind::Comment).map(|(t, _)| t.to_vec()),
        Some(vec![PaneKind::Comment, PaneKind::Info, PaneKind::Notation])
    );
    with_layout(&mut rig, &odd);
    ctrl(&mut rig, 'e');
    assert_eq!(shown_in(&rig, PaneKind::Comment), Some(PaneKind::Notation));
}

#[test]
fn ctrl_e_on_a_comment_alone_in_its_group_closes_it() {
    let mut rig = rig();
    ui(&mut rig, UiMessage::DetachTab(PaneKind::Comment));
    assert!(rig.shell.ui.comment_visible());
    ctrl(&mut rig, 'e');
    assert!(!is_open(&rig, PaneKind::Comment));
    ctrl(&mut rig, 'e');
    assert!(is_open(&rig, PaneKind::Comment) && rig.shell.ui.comment_visible());
}

#[test]
fn ctrl_e_works_when_the_comment_is_hidden_in_a_merged_group() {
    let mut rig = rig();
    toggle(&mut rig, PaneKind::GameList);
    let merged = layout(&rig).merge(PaneKind::GameList, PaneKind::Comment);
    with_layout(&mut rig, &merged);
    assert_eq!(shown_in(&rig, PaneKind::Comment), Some(PaneKind::GameList));
    ctrl(&mut rig, 'e');
    assert_eq!(shown_in(&rig, PaneKind::Comment), Some(PaneKind::Comment));
    ctrl(&mut rig, 'e');
    assert_eq!(shown_in(&rig, PaneKind::Comment), Some(PaneKind::Notation));
}

// ---- Ctrl+I ----

#[test]
fn ctrl_i_toggles_the_information_tab() {
    let mut rig = rig();
    ctrl(&mut rig, 'i');
    assert_eq!(shown_in(&rig, PaneKind::Info), Some(PaneKind::Info));
    assert!(has(&rig, "info-pane"));
    ctrl(&mut rig, 'i');
    assert_eq!(shown_in(&rig, PaneKind::Info), Some(PaneKind::Notation));
    assert!(is_open(&rig, PaneKind::Info));
}

#[test]
fn ctrl_i_opens_a_closed_information_panel_and_closes_a_lone_one() {
    let mut rig = rig();
    click(&mut rig, close_id(PaneKind::Info));
    ctrl(&mut rig, 'i');
    assert_eq!(shown_in(&rig, PaneKind::Info), Some(PaneKind::Info));
    ui(&mut rig, UiMessage::DetachTab(PaneKind::Info));
    ctrl(&mut rig, 'i');
    assert!(!is_open(&rig, PaneKind::Info), "alone: closed");
}

#[test]
fn ctrl_i_leaves_the_comment_visible_in_another_group_alone() {
    let mut rig = rig();
    ui(&mut rig, UiMessage::DetachTab(PaneKind::Comment));
    ctrl(&mut rig, 'i');
    assert!(rig.shell.ui.comment_visible());
    assert_eq!(shown_in(&rig, PaneKind::Info), Some(PaneKind::Info));
}

#[test]
fn the_shortcuts_are_command_e_and_command_i_in_the_default_map() {
    let rig = rig();
    let find = |action| {
        rig.shell
            .app
            .shortcuts
            .bindings(action)
            .first()
            .map(|b| format!("{b:?}"))
    };
    let e = find(mychess_app::Action::ShowCommentEditor).expect("binding");
    let i = find(mychess_app::Action::ToggleGameInfo).expect("binding");
    assert!(e.to_lowercase().contains('e'), "{e}");
    assert!(i.to_lowercase().contains('i'), "{i}");
}

// ---- the draft ----

fn draft_rig() -> Rig {
    let mut rig = rig();
    ui(
        &mut rig,
        UiMessage::SelectTab {
            kind: PaneKind::Comment,
        },
    );
    type_char(&mut rig, 'z');
    assert!(rig.shell.ui.notation.editor.is_dirty());
    rig
}

fn assert_saved(rig: &Rig, how: &str) {
    assert!(
        !rig.shell.ui.notation.editor.is_dirty(),
        "{how}: still dirty"
    );
    assert_eq!(
        rig.shell.app.session.game().initial_comment(),
        Some("z"),
        "{how}"
    );
}

#[test]
fn the_draft_is_saved_when_the_comment_goes_out_of_sight_by_any_route() {
    type Route = fn(&mut Rig);
    let routes: [(&str, Route); 7] = [
        ("tab click", |r| {
            ui(
                r,
                UiMessage::SelectTab {
                    kind: PaneKind::Notation,
                },
            );
        }),
        ("other tab click (info)", |r| {
            ui(
                r,
                UiMessage::SelectTab {
                    kind: PaneKind::Info,
                },
            );
        }),
        ("close button", |r| {
            ui(r, UiMessage::ClosePane(PaneKind::Comment))
        }),
        ("menu toggle", |r| menu_toggle(r, PaneKind::Comment)),
        ("ctrl+e", |r| ctrl(r, 'e')),
        ("show notation", |r| {
            ui(r, UiMessage::Action(mychess_app::Action::ShowNotation));
        }),
        ("reset layout", |r| ui(r, UiMessage::ResetLayout)),
    ];
    for (how, route) in routes {
        let mut rig = draft_rig();
        route(&mut rig);
        assert!(!rig.shell.ui.comment_visible(), "{how}: still visible");
        assert_saved(&rig, how);
    }
}

#[test]
fn closing_the_notation_group_around_a_visible_comment_keeps_the_draft() {
    // The comment stays on screen when a neighbour tab is closed.
    let mut rig = draft_rig();
    ui(&mut rig, UiMessage::ClosePane(PaneKind::Info));
    assert!(rig.shell.ui.comment_visible());
    assert!(
        rig.shell.ui.notation.editor.is_dirty(),
        "not saved, not lost"
    );
    assert_eq!(rig.shell.ui.notation.editor.text(), "z");
}

#[test]
fn bringing_the_comment_into_sight_opens_the_editor_for_the_cursor_comment() {
    let mut s = common::sample();
    s.game.set_comment(s.e4, "old").unwrap();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    ctrl(&mut rig, 'e');
    assert_eq!(
        rig.shell.ui.notation.editor.target(),
        CommentTarget::After(e4)
    );
    assert_eq!(rig.shell.ui.notation.editor.text(), "old");
}

#[test]
fn editing_a_comment_from_the_notation_shows_the_hidden_or_closed_comment_tab() {
    for closed in [false, true] {
        let mut s = common::sample();
        let e4 = s.e4;
        s.game.set_comment(e4, "c").unwrap();
        let mut rig = Rig::with_game(s.game);
        if closed {
            ui(&mut rig, UiMessage::ClosePane(PaneKind::Comment));
        }
        assert!(!rig.shell.ui.comment_visible());
        rig.msg(Message::Notation(NotationMessage::Event(
            NotationEvent::CommentClick(CommentTarget::After(e4)),
        )));
        assert!(rig.shell.ui.comment_visible(), "closed: {closed}");
        assert!(is_open(&rig, PaneKind::Comment));
        assert_eq!(
            rig.shell.ui.notation.editor.target(),
            CommentTarget::After(e4)
        );
    }
}

#[test]
fn the_draft_does_not_leak_into_the_next_game_after_the_tab_is_hidden() {
    let mut rig = draft_rig();
    ctrl(&mut rig, 'e');
    rig.app_msg(mychess_app::Message::Game(
        mychess_app::GameMessage::Navigate(mychess_app::Nav::Start),
    ));
    assert_eq!(rig.shell.app.session.game().initial_comment(), Some("z"));
    assert!(!rig.shell.ui.notation.editor.is_dirty());
}

// ---- texts ----

#[test]
fn the_new_keys_exist_in_both_languages_with_the_expected_texts() {
    let cases = [
        ("pane-close", "Close panel", "Fechar painel"),
        (
            "tab-detach",
            "Detach to a new panel",
            "Separar em um painel novo",
        ),
        ("pane-comment", "Comment", "Comentário"),
        ("menu-view-pane-notation", "Moves", "Lances"),
        ("menu-view-pane-info", "Information", "Informações"),
        ("menu-view-pane-comment", "Comment", "Comentário"),
        ("notation-tab-moves", "Moves", "Lances"),
        ("notation-tab-info", "Information", "Informações"),
        ("notation-tab-comment", "Comment", "Comentário"),
    ];
    let (en, pt) = (I18n::new(Language::EnUs), I18n::new(Language::PtBr));
    for (key, e, p) in cases {
        assert_eq!(en.tr(key), e, "{key} en-US");
        assert_eq!(pt.tr(key), p, "{key} pt-BR");
    }
}

#[test]
fn every_panel_label_resolves_in_both_languages() {
    let (en, pt) = (I18n::new(Language::EnUs), I18n::new(Language::PtBr));
    for kind in ALL {
        let key = mychess_gui::windows::pane_tabs::label_key(kind);
        for i18n in [&en, &pt] {
            let text = i18n.tr(key);
            assert!(!text.is_empty() && text != key, "{key}");
        }
    }
}

#[test]
fn the_notation_tab_submenu_is_gone_from_the_texts_in_both_languages() {
    for lang in ["en-US", "pt-BR"] {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/i18n")
            .join(lang);
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(
                !text.contains("menu-view-tabs"),
                "{} still defines menu-view-tabs",
                path.display()
            );
        }
    }
}

#[test]
fn the_labels_follow_the_language_in_the_window_and_in_the_menu() {
    let rig = rig_pt();
    assert!(has_text(&rig, "Informações"));
    assert!(!has_text(&rig, "Information"));
    let mut rig = rig_pt();
    ui(&mut rig, UiMessage::DetachTab(PaneKind::Info));
    assert!(has_text(&rig, "Informações"), "title of the lone panel");
}
