//! Sanity tests of the strip of databases: drag to copy, the context menu
//! of a tab (spec M3-4d). The full contract suite is the tester's.
#![allow(clippy::assert_is_empty)]

use iced::{Point, Settings, Size};
use iced_test::Simulator;
use mychess_app::{BasesMessage, CopyScope, ListMessage, Message as AppMessage, PickMode, SlotId};

use super::tests::{app_with, pgn_with};
use super::update::list;
use super::*;
use crate::shell::{Message, Shell};
use crate::widgets::context_menu::Entry;

fn slot(n: usize) -> SlotId {
    SlotId::from_number(n).expect("slot")
}

/// A shell with a.pgn (slot 1) and b.pgn (slot 2), 20 games each, and the
/// list shown.
fn shell(dir: &std::path::Path) -> Shell {
    let mut shell = Shell::new(None);
    shell.app = app_with(&pgn_with(dir, "a.pgn", 20));
    // Open the second one on top of the first.
    let mut queue = vec![AppMessage::Bases(BasesMessage::Open(pgn_with(
        dir, "b.pgn", 20,
    )))];
    while let Some(message) = queue.pop() {
        for effect in mychess_app::update(&mut shell.app, message) {
            if matches!(
                effect,
                mychess_app::Effect::OpenBase { .. } | mychess_app::Effect::LoadBaseGame { .. }
            ) {
                queue.push(crate::test_support::run_effect(effect));
            }
        }
    }
    shell
        .ui
        .show_pane(crate::windows::layout::PaneKind::GameList);
    // The test machine may speak another language.
    shell.i18n = crate::i18n::I18n::new(crate::i18n::Language::EnUs);
    shell
}

fn send(
    ui: &mut GameListUi,
    app: &mychess_app::AppState,
    message: GameListMessage,
) -> Vec<AppMessage> {
    ui.update(message, app)
}

#[test]
fn dropping_a_tab_on_another_copies_the_filter_and_clears_the_drag() {
    let dir = tempfile::tempdir().expect("dir");
    let shell = shell(dir.path());
    let mut ui = GameListUi::default();
    assert!(send(&mut ui, &shell.app, GameListMessage::TabPress(slot(1))).is_empty());
    assert!(
        ui.drag.is_some_and(|d| !d.is_dragging()),
        "not yet over another"
    );
    send(
        &mut ui,
        &shell.app,
        GameListMessage::TabHover(Some(slot(2))),
    );
    assert!(ui.drag.is_some_and(|d| d.is_dragging()));
    let out = send(&mut ui, &shell.app, GameListMessage::TabDrop(slot(2)));
    assert_eq!(
        out,
        [list(ListMessage::CopyGames {
            from: slot(1),
            to: slot(2),
            scope: CopyScope::Filter,
        })]
    );
    assert!(ui.drag.is_none());
}

#[test]
fn releasing_on_the_same_tab_only_selects_it_and_escape_cancels_the_drag() {
    let dir = tempfile::tempdir().expect("dir");
    let mut shell = shell(dir.path());
    let mut ui = GameListUi::default();
    send(&mut ui, &shell.app, GameListMessage::TabPress(slot(1)));
    let out = send(&mut ui, &shell.app, GameListMessage::TabDrop(slot(1)));
    assert_eq!(out, [AppMessage::Bases(BasesMessage::Select(slot(1)))]);
    // Escape, through the shell.
    shell.ui.games.drag = Some(TabDrag {
        from: slot(1),
        payload: DragPayload::Filter,
        over: Some(slot(2)),
        collapse: None,
    });
    drop(shell.update(Message::Dialog(
        crate::windows::dialog_ui::DialogMessage::Key {
            input: mychess_app::KeyInput {
                key: mychess_app::Key::Named(mychess_app::NamedKey::Escape),
                modifiers: mychess_app::Modifiers::NONE,
            },
            text_focused: false,
            typed: None,
        },
    )));
    assert!(
        shell.ui.games.drag_cancelled && shell.ui.games.drag.is_none(),
        "the release is remembered as dead"
    );
    // And a release anywhere ends it too.
    shell.ui.games.drag = Some(TabDrag {
        from: slot(1),
        payload: DragPayload::Filter,
        over: None,
        collapse: None,
    });
    drop(shell.update(Message::Notation(
        crate::windows::notation::NotationMessage::PointerReleased,
    )));
    assert!(shell.ui.games.drag.is_none());
}

#[test]
fn the_view_decides_what_a_release_on_a_tab_means() {
    let dir = tempfile::tempdir().expect("dir");
    let mut shell = shell(dir.path());
    // No drag: a click on the tab selects it.
    let mut sim = Simulator::with_size(Settings::default(), Size::new(1280.0, 800.0), view(&shell));
    sim.click("a.pgn").expect("the tab");
    let sent: Vec<Message> = sim.into_messages().collect();
    assert!(
        sent.iter().any(|m| matches!(
            m,
            Message::Games(GameListMessage::TabClick(s)) if *s == slot(1)
        )),
        "{sent:?}"
    );
    // The click became a selection through the update, which saw the press.
    let after = shell
        .ui
        .games
        .update(GameListMessage::TabPress(slot(1)), &shell.app);
    assert!(after.is_empty());
    let click = shell
        .ui
        .games
        .update(GameListMessage::TabClick(slot(1)), &shell.app);
    assert!(matches!(
        click.as_slice(),
        [AppMessage::Bases(BasesMessage::Select(s))] if *s == slot(1)
    ));
    // A release with no press on that tab selects nothing (D616).
    let lone = shell
        .ui
        .games
        .update(GameListMessage::TabClick(slot(1)), &shell.app);
    assert!(lone.is_empty());
    assert!(
        sent.iter()
            .any(|m| matches!(m, Message::Games(GameListMessage::TabPress(s)) if *s == slot(1)))
    );
    // A drag from the tab of b.pgn: the release on a.pgn drops.
    shell.ui.games.drag = Some(TabDrag {
        from: slot(2),
        payload: DragPayload::Filter,
        over: Some(slot(2)),
        collapse: None,
    });
    let mut sim = Simulator::with_size(Settings::default(), Size::new(1280.0, 800.0), view(&shell));
    sim.click("a.pgn").expect("the tab");
    let sent: Vec<Message> = sim.into_messages().collect();
    assert!(
        sent.iter()
            .any(|m| matches!(m, Message::Games(GameListMessage::TabDrop(s)) if *s == slot(1))),
        "{sent:?}"
    );
}

#[test]
fn the_hint_names_the_games_and_both_databases_only_over_another_tab() {
    let dir = tempfile::tempdir().expect("dir");
    let mut shell = shell(dir.path());
    let text = "Drop to copy 20 games from a.pgn to b.pgn";
    let find = |shell: &Shell| {
        let mut sim =
            Simulator::with_size(Settings::default(), Size::new(1280.0, 800.0), view(shell));
        sim.find(text).is_ok()
    };
    shell.ui.games.drag = Some(TabDrag {
        from: slot(1),
        payload: DragPayload::Filter,
        over: Some(slot(1)),
        collapse: None,
    });
    assert!(!find(&shell), "still over its own tab");
    shell.ui.games.drag = Some(TabDrag {
        from: slot(1),
        payload: DragPayload::Filter,
        over: Some(slot(2)),
        collapse: None,
    });
    assert!(find(&shell));
}

#[test]
fn rows_of_a_selection_can_be_dragged_and_a_plain_press_keeps_the_selection() {
    let dir = tempfile::tempdir().expect("dir");
    let mut shell = shell(dir.path());
    let id = shell.app.bases.current();
    for (n, mode) in [(3, PickMode::Only), (5, PickMode::Toggle)] {
        drop(mychess_app::update(
            &mut shell.app,
            list(ListMessage::Pick {
                slot: id,
                number: n,
                mode,
            }),
        ));
    }
    let mut ui = GameListUi::default();
    // A press on a selected row of a multiple selection sends nothing: the
    // selection waits for the release.
    let out = send(
        &mut ui,
        &shell.app,
        GameListMessage::Click {
            slot: id,
            number: 3,
        },
    );
    assert!(out.is_empty());
    assert_eq!(ui.drag.map(|d| d.payload), Some(DragPayload::Selection));
    send(
        &mut ui,
        &shell.app,
        GameListMessage::TabHover(Some(slot(1))),
    );
    let out = send(&mut ui, &shell.app, GameListMessage::TabDrop(slot(1)));
    assert_eq!(
        out,
        [list(ListMessage::CopyGames {
            from: id,
            to: slot(1),
            scope: CopyScope::Selection,
        })]
    );
    // Released on the same row: the selection shrinks to that row.
    send(
        &mut ui,
        &shell.app,
        GameListMessage::Click {
            slot: id,
            number: 5,
        },
    );
    let out = send(
        &mut ui,
        &shell.app,
        GameListMessage::RowRelease {
            slot: id,
            number: 5,
        },
    );
    assert_eq!(
        out,
        [list(ListMessage::Pick {
            slot: id,
            number: 5,
            mode: PickMode::Only,
        })]
    );
    // A press on a row outside the selection selects it at once.
    let out = send(
        &mut ui,
        &shell.app,
        GameListMessage::Click {
            slot: id,
            number: 9,
        },
    );
    assert_eq!(out.len(), 1);
    // And released on another row nothing is lost.
    let out = send(
        &mut ui,
        &shell.app,
        GameListMessage::RowRelease {
            slot: id,
            number: 10,
        },
    );
    assert!(out.is_empty() && ui.drag.is_none());
}

fn labels(entries: &[Entry<'static, Message>]) -> Vec<(String, bool)> {
    entries
        .iter()
        .filter_map(|e| match e {
            Entry::Item { label, message } => Some((label.clone(), message.is_some())),
            _ => None,
        })
        .collect()
}

#[test]
fn the_menu_of_a_tab_follows_the_scid_and_is_translated() {
    let dir = tempfile::tempdir().expect("dir");
    let mut shell = shell(dir.path());
    // A PGN file: read-only for good, so the entry is greyed.
    let entries = menus::tab_entries(&shell, slot(1));
    let items = labels(&entries);
    let names: Vec<&str> = items.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(
        names,
        [
            "Reset the filter",
            "Negate the filter",
            "\u{2713}  Read-only",
            "\u{2713}  Confirm before copying",
            "Copy the filter to b.pgn",
            "Copy the filter to Clipbase",
            "Close the database",
        ]
    );
    assert_eq!(
        items.iter().map(|(_, on)| *on).collect::<Vec<_>>(),
        [true, true, false, true, false, true, true]
    );
    // The Clipbase clears instead of closing.
    let clip = labels(&menus::tab_entries(&shell, SlotId::CLIPBASE));
    assert_eq!(
        clip.last().map(|(l, _)| l.as_str()),
        Some("Clear the Clipbase")
    );
    // The same in Portuguese.
    shell.i18n = crate::i18n::I18n::new(crate::i18n::Language::PtBr);
    let pt = labels(&menus::tab_entries(&shell, slot(1)));
    assert_eq!(pt[0].0, "Restaurar o filtro");
    assert!(
        pt.iter()
            .any(|(l, _)| l.contains("Confirmar antes de copiar"))
    );
    // The right click opens it.
    let out = shell.ui.games.update(
        GameListMessage::Menu(ListMenu {
            target: MenuTarget::Tab(slot(2)),
            position: Point::new(30.0, 30.0),
        }),
        &shell.app,
    );
    assert!(out.is_empty(), "no selection change for a tab");
    assert!(overlay(&shell).is_some());
}

#[test]
fn the_items_of_the_tab_menu_act_on_that_tab_and_the_preference_toggles() {
    let dir = tempfile::tempdir().expect("dir");
    let shell = shell(dir.path());
    let mut ui = GameListUi::default();
    let mut act = |slot_id, action| {
        send(
            &mut ui,
            &shell.app,
            GameListMessage::TabAct(slot_id, action),
        )
    };
    assert_eq!(
        act(slot(1), TabAction::Reset),
        [list(ListMessage::ResetFilter { slot: slot(1) })]
    );
    assert_eq!(
        act(slot(1), TabAction::Negate),
        [list(ListMessage::NegateFilter { slot: slot(1) })]
    );
    assert_eq!(
        act(slot(1), TabAction::Close),
        [AppMessage::Bases(BasesMessage::Close(slot(1)))]
    );
    assert!(matches!(
        &act(slot(1), TabAction::ToggleConfirmCopy)[..],
        [AppMessage::Prefs(mychess_app::PrefsMessage::Set(
            mychess_app::PrefChange::ConfirmCopy(false)
        ))]
    ));
}

#[test]
fn the_copy_question_is_a_dialog_with_both_names_that_answers_with_the_keys() {
    use crate::windows::dialog_ui::{ActiveDialog, DialogUi};
    let dir = tempfile::tempdir().expect("dir");
    let mut shell = shell(dir.path());
    // A PGN target is refused; use the Clipbase-free path: ask for the
    // prompt directly through the application with an editable target by
    // marking nothing: the question is only built for a writable target,
    // so check the refusal here and the box through a fabricated prompt.
    drop(mychess_app::update(
        &mut shell.app,
        list(ListMessage::CopyGames {
            from: slot(1),
            to: slot(2),
            scope: CopyScope::Filter,
        }),
    ));
    assert!(shell.app.copy_prompt.is_none(), "a PGN is not a target");
    shell.app.copy_prompt = Some(mychess_app::CopyPrompt {
        from: mychess_app::CopyEnd {
            slot: slot(1),
            name: "a.pgn".to_owned(),
        },
        to: mychess_app::CopyEnd {
            slot: slot(2),
            name: "b.si4".to_owned(),
        },
        scope: CopyScope::Filter,
        count: 12,
    });
    let ui = DialogUi::default();
    assert_eq!(
        ActiveDialog::of(&shell.app, &ui),
        Some(ActiveDialog::CopyGames)
    );
    let element = crate::windows::dialogs::view(&shell).expect("the box");
    let mut sim = Simulator::with_size(Settings::default(), Size::new(900.0, 600.0), element);
    assert!(sim.find("Copy 12 games from a.pgn to b.si4?").is_ok());
    sim.click("Copy").expect("the button");
    let sent: Vec<Message> = sim.into_messages().collect();
    assert!(matches!(
        sent.as_slice(),
        [Message::App(AppMessage::Bases(BasesMessage::List(
            ListMessage::AnswerCopy(true)
        )))]
    ));
    // Return copies, Escape does not.
    let mut ui = DialogUi::default();
    let key = |named| mychess_app::KeyInput {
        key: mychess_app::Key::Named(named),
        modifiers: mychess_app::Modifiers::NONE,
    };
    let enter = ui.key(
        ActiveDialog::CopyGames,
        &key(mychess_app::NamedKey::Enter),
        false,
        &shell.app,
    );
    let escape = ui.key(
        ActiveDialog::CopyGames,
        &key(mychess_app::NamedKey::Escape),
        false,
        &shell.app,
    );
    assert_eq!(
        enter.messages,
        [AppMessage::Bases(BasesMessage::List(
            ListMessage::AnswerCopy(true)
        ))]
    );
    assert_eq!(
        escape.messages,
        [AppMessage::Bases(BasesMessage::List(
            ListMessage::AnswerCopy(false)
        ))]
    );
}

#[test]
fn the_keyboard_copy_item_sends_the_copy_and_escape_keeps_the_release_dead() {
    let dir = tempfile::tempdir().expect("dir");
    let mut shell = shell(dir.path());
    let mut ui = GameListUi::default();
    let out = send(
        &mut ui,
        &shell.app,
        GameListMessage::TabCopy(slot(1), SlotId::CLIPBASE),
    );
    assert_eq!(
        out,
        [list(ListMessage::CopyGames {
            from: slot(1),
            to: SlotId::CLIPBASE,
            scope: CopyScope::Filter,
        })]
    );
    // A cancelled drag stays recorded until the release, and drops nothing.
    send(&mut ui, &shell.app, GameListMessage::TabPress(slot(1)));
    send(
        &mut ui,
        &shell.app,
        GameListMessage::TabHover(Some(slot(2))),
    );
    assert!(ui.escape_drag());
    assert!(ui.drag_cancelled && ui.drag.is_none());
    assert!(send(&mut ui, &shell.app, GameListMessage::TabDrop(slot(2))).is_empty());
    // A plain click in progress does not spend the key.
    send(&mut ui, &shell.app, GameListMessage::TabPress(slot(1)));
    assert!(!ui.escape_drag() && ui.drag.is_none() && !ui.drag_cancelled);
    // The menu of a tab that closed goes away.
    ui.menu = Some(ListMenu {
        target: MenuTarget::Tab(slot(2)),
        position: Point::ORIGIN,
    });
    drop(mychess_app::update(
        &mut shell.app,
        AppMessage::Bases(BasesMessage::Close(slot(2))),
    ));
    ui.drop_stale_menu(&shell.app);
    assert!(ui.menu.is_none());
}

#[test]
fn the_notes_of_a_finished_copy_are_spelled_out_in_both_languages() {
    use crate::i18n::{I18n, Language};
    use mychess_app::{AppNotice, BasesNotice, CopyEnd};
    let end = |n, name: &str| CopyEnd {
        slot: slot(n),
        name: name.to_owned(),
    };
    let notice = |truncated, reset| {
        AppNotice::Bases(BasesNotice::FilterCopiedNotes {
            count: 3,
            from: end(1, "a.pgn"),
            to: end(2, "b.si4"),
            truncated,
            reset,
        })
    };
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    let text = |i18n: &I18n, n| crate::windows::status_bar::app_notice_text(i18n, &n);
    assert_eq!(
        text(&en, notice(2, false)),
        "3 games copied from a.pgn to b.si4; 2 names were shortened to fit the format"
    );
    assert!(text(&en, notice(0, true)).contains("list of the target was reset"));
    assert_eq!(
        text(&pt, notice(1, false)),
        "3 partidas copiadas de a.pgn para b.si4; um nome foi encurtado para caber no formato"
    );
}

#[test]
fn a_stale_press_on_a_tab_does_not_turn_a_later_release_into_a_click() {
    let dir = tempfile::tempdir().expect("dir");
    let mut shell = shell(dir.path());
    drop(
        shell
            .ui
            .games
            .update(GameListMessage::TabPress(slot(1)), &shell.app),
    );
    // The button came up elsewhere: the press is forgotten (D619).
    drop(shell.update(Message::Notation(
        crate::windows::notation::NotationMessage::PointerReleased,
    )));
    let click = shell
        .ui
        .games
        .update(GameListMessage::TabClick(slot(1)), &shell.app);
    assert!(click.is_empty());
}
