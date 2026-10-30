//! The context menu of a tab of the strip of databases (spec M3-4,
//! "Seletor de bancos", D583; Scid `switcher.tcl` 868-940), headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::redundant_closure_for_method_calls
)]

mod m3_4d_common;

use iced::Settings;
use iced_test::Simulator;
use iced_test::core::mouse;
use m3_4d_common::*;
use mychess_app::{AppNotice, BasesNotice, PrefChange, PrefsMessage};
use mychess_gui::Message;
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::game_list::{GameListMessage, ListMenu, MenuTarget, TabAction, overlay};
use mychess_gui::windows::layout::PaneKind;

const ON: &str = "\u{2713}";

fn open_tab_menu(rig: &mut Rig, tab: SlotId) {
    // An open menu has a layer that takes the next click (it dismisses it).
    rig.shell.ui.games.menu = None;
    let at = tab_point(rig, tab);
    let out = pressed(rig, at, mouse::Button::Right);
    apply(rig, out);
}

/// Clicks the item with this text in the open menu; the messages it sent.
fn choose(rig: &Rig, label: &str) -> Vec<Message> {
    let element = overlay(&rig.shell).expect("a menu is open");
    let mut sim = Simulator::with_size(Settings::default(), SIZE, element);
    drop(
        sim.click(label)
            .unwrap_or_else(|_| panic!("item {label:?}")),
    );
    sim.into_messages().collect()
}

fn has_item(rig: &Rig, label: &str) -> bool {
    let element = overlay(&rig.shell).expect("a menu is open");
    Simulator::with_size(Settings::default(), SIZE, element)
        .find(label)
        .is_ok()
}

fn tab_acts(messages: &[Message]) -> Vec<(SlotId, TabAction)> {
    messages
        .iter()
        .filter_map(|m| match m {
            Message::Games(GameListMessage::TabAct(s, a)) => Some((*s, *a)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_right_button_on_a_tab_opens_its_menu_without_starting_a_drag_or_selecting() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    let before = rig.shell.app.bases.current();
    let at = tab_point(&rig, slot(2));
    let out = pressed(&rig, at, mouse::Button::Right);
    assert!(
        out.iter().any(|m| matches!(
            m,
            Message::Games(GameListMessage::Menu(ListMenu {
                target: MenuTarget::Tab(s),
                ..
            })) if *s == slot(2)
        )),
        "{out:?}"
    );
    assert!(
        !out.iter()
            .any(|m| matches!(m, Message::Games(GameListMessage::TabPress(_)))),
        "the right button is not a drag"
    );
    apply(&mut rig, out);
    assert!(rig.shell.ui.games.drag.is_none());
    assert_eq!(rig.shell.app.bases.current(), before);
    assert!(overlay(&rig.shell).is_some());
    // The Clipbase has one too.
    open_tab_menu(&mut rig, CLIP);
    assert!(has_item(&rig, "Clear the Clipbase"));
}

#[test]
fn the_menu_has_the_five_items_in_english_and_portuguese() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    open_tab_menu(&mut rig, slot(2));
    for label in [
        "Reset the filter",
        "Negate the filter",
        &format!("{ON}  Read-only"),
        &format!("{ON}  Confirm before copying"),
        "Close the database",
    ] {
        assert!(has_item(&rig, label), "{label}");
    }
    rig.shell.i18n = I18n::new(Language::PtBr);
    for label in [
        "Restaurar o filtro",
        "Inverter o filtro",
        &format!("{ON}  Somente leitura"),
        &format!("{ON}  Confirmar antes de copiar"),
        "Fechar o banco",
    ] {
        assert!(has_item(&rig, label), "{label}");
    }
    open_tab_menu(&mut rig, CLIP);
    assert!(has_item(&rig, "Limpar a Clipbase"));
}

#[test]
fn reset_and_negate_act_on_the_tab_that_was_clicked_not_on_the_current_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    assert_eq!(rig.shell.app.bases.current(), slot(1));
    rig.send(AppMessage::Bases(BasesMessage::List(
        ListMessage::NegateFilter { slot: slot(2) },
    )));
    assert_eq!(rig.shell.app.bases.slot(slot(2)).unwrap().filtered(), 0);
    open_tab_menu(&mut rig, slot(2));
    let out = choose(&rig, "Reset the filter");
    assert_eq!(tab_acts(&out), [(slot(2), TabAction::Reset)]);
    apply(&mut rig, out);
    assert_eq!(rig.shell.app.bases.slot(slot(2)).unwrap().filtered(), 8);
    assert!(
        rig.shell.ui.games.menu.is_none(),
        "choosing closes the menu"
    );
    open_tab_menu(&mut rig, slot(2));
    let out = choose(&rig, "Negate the filter");
    assert_eq!(tab_acts(&out), [(slot(2), TabAction::Negate)]);
    apply(&mut rig, out);
    assert_eq!(rig.shell.app.bases.slot(slot(2)).unwrap().filtered(), 0);
    assert_eq!(rig.shell.app.bases.slot(slot(1)).unwrap().filtered(), 12);
    assert_eq!(rig.shell.app.bases.current(), slot(1));
}

#[test]
fn close_closes_that_database_and_the_clipbase_entry_clears_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    open_tab_menu(&mut rig, slot(3));
    let out = choose(&rig, "Close the database");
    assert_eq!(tab_acts(&out), [(slot(3), TabAction::Close)]);
    apply(&mut rig, out);
    assert!(rig.shell.app.bases.slot(slot(3)).is_none());
    assert_eq!(
        rig.shell.app.bases.current(),
        slot(1),
        "closing another keeps the current"
    );
    // Clipbase: put games in, then clear.
    rig.send(AppMessage::Bases(BasesMessage::List(
        ListMessage::CopyGames {
            from: slot(2),
            to: CLIP,
            scope: CopyScope::Filter,
        },
    )));
    assert_eq!(rig.shell.app.bases.slot(CLIP).unwrap().total(), 8);
    open_tab_menu(&mut rig, CLIP);
    let out = choose(&rig, "Clear the Clipbase");
    assert_eq!(tab_acts(&out), [(CLIP, TabAction::Close)]);
    apply(&mut rig, out);
    assert!(
        rig.shell.app.bases.slot(CLIP).is_some(),
        "the Clipbase stays"
    );
    assert_eq!(rig.shell.app.bases.slot(CLIP).unwrap().total(), 0);
}

#[test]
fn read_only_is_marked_and_disabled_when_the_lock_is_structural() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    open_tab_menu(&mut rig, slot(2));
    // A PGN file is read-only for good: marked, and choosing does nothing.
    assert!(has_item(&rig, &format!("{ON}  Read-only")));
    let out = choose(&rig, &format!("{ON}  Read-only"));
    assert!(tab_acts(&out).is_empty(), "{out:?}");
    assert!(rig.shell.app.bases.slot(slot(2)).unwrap().read_only);
    // The Clipbase has no read-only mark to set either.
    open_tab_menu(&mut rig, CLIP);
    let out = choose(&rig, "   Read-only");
    assert!(tab_acts(&out).is_empty(), "{out:?}");
}

#[test]
fn confirm_before_copying_shows_the_preference_and_toggles_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    assert!(rig.shell.app.prefs.editing.confirm_copy);
    open_tab_menu(&mut rig, slot(2));
    assert!(has_item(&rig, &format!("{ON}  Confirm before copying")));
    let out = choose(&rig, &format!("{ON}  Confirm before copying"));
    assert_eq!(tab_acts(&out), [(slot(2), TabAction::ToggleConfirmCopy)]);
    // What the application receives.
    let sent = apply(&mut rig, out);
    assert!(sent.contains(&AppMessage::Prefs(PrefsMessage::Set(
        PrefChange::ConfirmCopy(false)
    ))));
    assert!(!rig.shell.app.prefs.editing.confirm_copy);
    // The mark is gone, from every tab's menu.
    open_tab_menu(&mut rig, slot(1));
    assert!(has_item(&rig, "   Confirm before copying"));
    assert!(!has_item(&rig, &format!("{ON}  Confirm before copying")));
    let out = choose(&rig, "   Confirm before copying");
    let sent = apply(&mut rig, out);
    assert!(sent.contains(&AppMessage::Prefs(PrefsMessage::Set(
        PrefChange::ConfirmCopy(true)
    ))));
    assert!(rig.shell.app.prefs.editing.confirm_copy);
}

#[test]
fn while_a_job_runs_the_filter_items_wait_but_confirm_and_close_do_not() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    let effects = mychess_app::update(
        &mut rig.shell.app,
        AppMessage::Bases(BasesMessage::List(ListMessage::CopyGames {
            from: slot(2),
            to: CLIP,
            scope: CopyScope::Filter,
        })),
    );
    assert_eq!(effects.len(), 1);
    open_tab_menu(&mut rig, slot(1));
    for label in ["Reset the filter", "Negate the filter"] {
        assert!(tab_acts(&choose(&rig, label)).is_empty(), "{label} waits");
    }
    assert_eq!(
        tab_acts(&choose(&rig, &format!("{ON}  Confirm before copying"))),
        [(slot(1), TabAction::ToggleConfirmCopy)]
    );
    assert_eq!(
        tab_acts(&choose(&rig, "Close the database")),
        [(slot(1), TabAction::Close)]
    );
}

#[test]
fn closing_the_database_of_the_job_from_the_menu_cancels_the_job() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    let effects = mychess_app::update(
        &mut rig.shell.app,
        AppMessage::Bases(BasesMessage::List(ListMessage::CopyGames {
            from: slot(2),
            to: CLIP,
            scope: CopyScope::Filter,
        })),
    );
    assert_eq!(effects.len(), 1);
    let control = rig.shell.app.list_job().map(|j| j.1.clone()).unwrap();
    open_tab_menu(&mut rig, slot(2));
    let out = choose(&rig, "Close the database");
    apply(&mut rig, out);
    assert!(control.is_cancelled());
    assert!(rig.shell.app.list_job().is_none());
}

#[test]
fn the_menu_is_gone_when_the_panel_closes() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    open_tab_menu(&mut rig, slot(2));
    assert!(rig.shell.ui.games.menu.is_some());
    rig.shell.ui.close_pane(PaneKind::GameList);
    rig.shell.ui.games.close_with_panel(false);
    assert!(overlay(&rig.shell).is_none());
}

#[test]
fn the_refusal_after_a_drop_is_readable_in_both_languages_and_names_both_ends() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = three(dir.path());
    press(&mut rig, Spot::Tab(slot(1)));
    hover(&mut rig, Spot::Tab(slot(2)));
    release(&mut rig, Spot::Tab(slot(2)));
    let Some(notice) = notice(&rig) else {
        panic!("no notice");
    };
    assert!(matches!(
        notice,
        AppNotice::Bases(BasesNotice::CopyRefused { .. })
    ));
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        let text = mychess_gui::windows::status_bar::app_notice_text(&i18n, &notice);
        assert!(text.contains("a.pgn") && text.contains("b.pgn"), "{text}");
    }
}
