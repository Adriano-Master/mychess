//! M9-5c (GUI): opening the manual, the table of contents, the chapters,
//! the links, the history, the search and the keys (spec M9-5, criteria 2,
//! 12, 13 and 15).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m9_5_common;

use m9_5_common::*;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};

fn help_item_message(rig: &Rig, key: &str) -> Option<Message> {
    fn walk(entries: &[MenuEntry], key: &str) -> Option<Message> {
        entries.iter().find_map(|e| match e {
            MenuEntry::Item {
                label: Label::Key(k),
                action,
                ..
            } if *k == key => action.clone(),
            MenuEntry::Submenu { entries, .. } => walk(entries, key),
            _ => None,
        })
    }
    menu_specs(&rig.shell.app, &rig.shell.ui, rig.shell.options)
        .iter()
        .find(|s| s.label_key == "menu-help")
        .and_then(|s| walk(&s.entries, key))
}

#[test]
fn the_help_menu_item_opens_the_manual_and_about_stays_available() {
    let mut rig = Rig::pt();
    let open = help_item_message(&rig, "menu-help-manual").expect("the Manual item is enabled");
    let about = help_item_message(&rig, "menu-help-about").expect("the About item is kept");
    assert!(!rig.shell.app.manual().is_open());
    rig.msg(open);
    assert!(rig.shell.app.manual().is_open());
    assert!(!rig.shell.ui.show_about);
    assert!(has_id(&rig, MANUAL_WINDOW_ID), "the window is drawn");
    rig.ui(ManualUiMessage::Plain(ManualMessage::Close));
    rig.msg(about);
    assert!(rig.shell.ui.show_about);
    assert!(!rig.shell.app.manual().is_open());
}

#[test]
fn f1_opens_the_manual_instead_of_the_about_box_and_toggles_it() {
    let mut rig = Rig::pt();
    rig.named(NamedKey::F(1));
    assert!(rig.shell.app.manual().is_open(), "F1 opens the manual");
    assert!(!rig.shell.ui.show_about, "F1 no longer opens the About box");
    assert_eq!(rig.chapter(), 2, "no window in focus: the board chapter");
    rig.named(NamedKey::F(1));
    assert!(!rig.shell.app.manual().is_open(), "F1 again closes it");
}

#[test]
fn the_index_lists_the_fifteen_chapters_with_their_titles() {
    let mut rig = Rig::pt();
    rig.open();
    for chapter in 0..15 {
        assert!(
            has_id(&rig, &chapter_entry_id(chapter)),
            "chapter {chapter}"
        );
    }
    assert!(!has_id(&rig, &chapter_entry_id(15)));
    let index = rig.shell.app.manual().index().unwrap();
    assert_eq!(index.len(), 15);
    assert_eq!(index.title(4), Some("Buscas"));
    assert_eq!(index.title(12), Some("Atalhos de teclado"));
    let mut sim = rig.sim(1300.0, 900.0);
    assert!(
        sim.find("Atalhos de teclado").is_ok(),
        "the title is on the screen"
    );
}

#[test]
fn clicking_a_chapter_shows_it() {
    let mut rig = Rig::pt();
    rig.open();
    let messages = click_id(rig.sim(1300.0, 900.0), &chapter_entry_id(4));
    assert!(!messages.is_empty());
    for message in messages {
        rig.msg(message);
    }
    assert_eq!(rig.chapter(), 4);
    let rendered = rig
        .shell
        .ui
        .dialogs
        .manual
        .rendered()
        .expect("the chapter is parsed");
    assert_eq!(rendered.chapter, 4);
    assert!(
        has_id(&rig, &heading_id("buscas")),
        "the heading of the chapter is on the screen"
    );
}

#[test]
fn an_internal_link_with_an_anchor_navigates_and_the_heading_is_there() {
    let mut rig = Rig::pt();
    rig.open();
    rig.link("03-bancos-de-dados.md#os-slots-e-a-clipbase");
    assert_eq!(rig.chapter(), 3);
    assert_eq!(
        rig.shell.app.manual().place().anchor.as_deref(),
        Some("os-slots-e-a-clipbase")
    );
    assert!(has_id(&rig, &heading_id("os-slots-e-a-clipbase")));
    // A link inside the chapter changes only the heading.
    rig.link("#abrir-uma-base");
    assert_eq!(rig.chapter(), 3);
    assert_eq!(
        rig.shell.app.manual().place().anchor.as_deref(),
        Some("abrir-uma-base")
    );
}

#[test]
fn the_text_viewer_turns_a_clicked_link_into_the_navigation_message() {
    // The message the markdown widget produces for a link of the text.
    let message = mychess_gui::windows::manual::link_message("04-buscas.md".to_owned());
    let mut rig = Rig::pt();
    rig.open();
    rig.msg(message);
    assert_eq!(rig.chapter(), 4);
}

#[test]
fn back_and_forward_buttons_follow_the_history() {
    let mut rig = Rig::pt();
    rig.open();
    rig.link("04-buscas.md");
    rig.link("05-aberturas.md");
    assert_eq!(rig.chapter(), 5);
    for message in click_id(rig.sim(1300.0, 900.0), "manual-back") {
        rig.msg(message);
    }
    assert_eq!(rig.chapter(), 4);
    for message in click_id(rig.sim(1300.0, 900.0), "manual-forward") {
        rig.msg(message);
    }
    assert_eq!(rig.chapter(), 5);
    // At the end of the trail Forward is disabled: the click sends nothing.
    let mut sim = rig.sim(1300.0, 900.0);
    drop(sim.click(id("manual-forward")));
    assert_eq!(sim.into_messages().count(), 0);
}

#[test]
fn alt_arrows_and_backspace_walk_the_history_with_the_window_in_focus() {
    let mut rig = Rig::pt();
    rig.open();
    rig.link("04-buscas.md");
    rig.link("05-aberturas.md");
    let alt = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };
    rig.key(Key::Named(NamedKey::Left), alt);
    assert_eq!(rig.chapter(), 4);
    rig.key(Key::Named(NamedKey::Right), alt);
    assert_eq!(rig.chapter(), 5);
    rig.named(NamedKey::Backspace);
    assert_eq!(rig.chapter(), 4);
}

#[test]
fn the_search_finds_clipbase_and_atalhos_and_a_result_leads_there() {
    let mut rig = Rig::pt();
    rig.open();
    rig.ui(ManualUiMessage::Plain(ManualMessage::Query(
        "Clipbase".into(),
    )));
    assert_ne!(rig.shell.app.manual().results().len(), 0);
    assert!(has_id(&rig, &result_entry_id(0)));
    for message in click_id(rig.sim(1300.0, 900.0), &result_entry_id(0)) {
        rig.msg(message);
    }
    assert_eq!(rig.chapter(), 3);
    assert_eq!(
        rig.shell.app.manual().place().anchor.as_deref(),
        Some("os-slots-e-a-clipbase")
    );
    rig.ui(ManualUiMessage::Plain(ManualMessage::Query(
        "atalhos".into(),
    )));
    let hits = rig.shell.app.manual().results();
    assert!(
        hits.iter().any(|h| h.chapter == 12 && h.anchor.is_none()),
        "the chapter 'Atalhos de teclado' is a result: {hits:?}"
    );
    rig.ui(ManualUiMessage::Plain(ManualMessage::Query(
        "zzzzzz".into(),
    )));
    assert_eq!(rig.shell.app.manual().results().len(), 0);
    let mut sim = rig.sim(1300.0, 900.0);
    assert!(
        sim.find("Nenhum capítulo ou seção tem esse título.")
            .is_ok(),
        "the empty search says so"
    );
}

#[test]
fn escape_clears_the_search_first_and_then_closes() {
    let mut rig = Rig::pt();
    rig.open();
    rig.ui(ManualUiMessage::Plain(ManualMessage::Query("abrir".into())));
    rig.named(NamedKey::Escape);
    assert!(
        rig.shell.app.manual().is_open(),
        "the first Esc only clears the search"
    );
    assert_eq!(rig.shell.app.manual().query(), "");
    rig.named(NamedKey::Escape);
    assert!(!rig.shell.app.manual().is_open());
    assert!(
        !rig.shell.ui.dialogs.manual.search_focused(),
        "the focus is zeroed on close"
    );
    assert!(
        rig.shell.ui.dialogs.manual.rendered().is_none(),
        "the chapter is forgotten"
    );
}

#[test]
fn ctrl_f_asks_for_the_focus_of_the_search_field() {
    let mut rig = Rig::pt();
    rig.open();
    let task = rig.shell.update(Message::Dialog(
        mychess_gui::windows::dialog_ui::DialogMessage::Key {
            input: KeyInput {
                key: Key::Char('f'),
                modifiers: Modifiers {
                    command: true,
                    ..Modifiers::NONE
                },
            },
            text_focused: false,
            typed: None,
        },
    ));
    let operations = iced_test::runtime::task::into_stream(task).map_or(0, |stream| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            use iced_test::futures::futures::StreamExt;
            let mut stream = Box::pin(stream);
            let mut n = 0;
            while let Some(action) = stream.next().await {
                if matches!(action, iced_test::runtime::Action::Widget(_)) {
                    n += 1;
                }
            }
            n
        })
    });
    assert!(
        operations >= 1,
        "Ctrl/Cmd+F focuses the field (a widget operation)"
    );
}

#[test]
fn the_question_about_an_external_link_is_shown_and_answered() {
    let mut rig = Rig::pt();
    rig.open();
    rig.link("https://lichess.org");
    assert_eq!(
        rig.shell.app.manual().pending_external(),
        Some("https://lichess.org")
    );
    assert!(has_id(&rig, "manual-external-box"));
    // Esc answers "no" and the window stays.
    rig.named(NamedKey::Escape);
    assert_eq!(rig.shell.app.manual().pending_external(), None);
    assert!(rig.shell.app.manual().is_open());
    assert!(!has_id(&rig, "manual-external-box"));
}

#[test]
fn an_unknown_link_shows_a_notice_in_the_bar() {
    let mut rig = Rig::pt();
    rig.open();
    rig.link("#nao-existe");
    assert!(has_id(&rig, "manual-notice"));
    let mut sim = rig.sim(1300.0, 900.0);
    assert!(sim.find("Link não encontrado.").is_ok());
}

#[test]
fn text_size_buttons_change_the_preference() {
    let mut rig = Rig::pt();
    rig.open();
    let before = rig.shell.app.prefs.manual.text_scale;
    for message in click_id(rig.sim(1300.0, 900.0), "manual-larger") {
        rig.msg(message);
    }
    assert_eq!(rig.shell.app.prefs.manual.text_scale, before + 1);
    for message in click_id(rig.sim(1300.0, 900.0), "manual-smaller") {
        rig.msg(message);
    }
    assert_eq!(rig.shell.app.prefs.manual.text_scale, before);
}
