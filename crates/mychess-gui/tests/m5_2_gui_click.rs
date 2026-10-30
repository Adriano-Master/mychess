//! M5-2a GUI contract: clicking a row of the table drives the game (spec
//! `docs/specs/m5-2-janela-da-arvore.md`, D965, D966, behaviour 3) and the
//! panel does not take the keys (D983, "O painel não trata teclado").
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_gui_common;

use m5_2_gui_common::*;
use mychess_app::Prompt;
use tempfile::tempdir;

fn panel(dir: &std::path::Path) -> TreeRig {
    let mut rig = TreeRig::with_std(dir);
    rig.open_panel();
    assert_eq!(rig.view().body, TreeBody::Rows);
    rig
}

#[test]
fn clicking_a_clickable_row_plays_the_move_on_the_board_and_the_table_follows() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    let nodes = rig.nodes();
    let sent = click_pane(&mut rig, "e4");
    assert_eq!(row_clicks(&sent), 1);
    assert_eq!(rig.ply(), 1);
    assert_eq!(rig.nodes(), nodes + 1);
    assert_eq!(
        rig.app().session.position().fen(),
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1"
    );
    // The panel shows the position after 1.e4.
    let mut sans = rig.sans();
    sans.sort();
    assert_eq!(sans, ["[end]", "c5", "e5"]);
    assert!(pane_shows(&rig, "c5") && !pane_shows(&rig, "Nf3"));
}

#[test]
fn clicking_the_next_move_of_the_line_only_advances() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    rig.play("e2e4");
    rig.play("e7e5");
    rig.nav(Nav::Start);
    let nodes = rig.nodes();
    click_pane(&mut rig, "e4");
    assert_eq!(rig.ply(), 1);
    assert_eq!(rig.nodes(), nodes, "no node was added");
    assert!(rig.app().session.prompt().is_none());
}

#[test]
fn clicking_a_move_of_the_middle_of_a_line_asks_before_changing_anything() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    rig.play("e2e4");
    rig.play("e7e5");
    rig.nav(Nav::Start);
    let nodes = rig.nodes();
    click_pane(&mut rig, "d4");
    assert!(
        matches!(rig.app().session.prompt(), Some(Prompt::ReplaceMove(_))),
        "{:?}",
        rig.app().session.prompt()
    );
    assert_eq!(rig.nodes(), nodes);
    assert_eq!(rig.ply(), 0);
}

#[test]
fn clicking_a_move_that_is_already_a_variation_enters_it() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    rig.play("e2e4");
    rig.nav(Nav::Start);
    let mv = rig.app().session.position().parse_uci("d2d4").unwrap();
    rig.app_msg(GameMessage::PlayMove {
        mv,
        options: PlayOptions {
            force_variation: true,
        },
    });
    rig.nav(Nav::Start);
    let nodes = rig.nodes();
    click_pane(&mut rig, "d4");
    assert_eq!(rig.nodes(), nodes, "the existing node is reused");
    assert_eq!(rig.ply(), 1);
    assert!(rig.app().session.prompt().is_none());
}

#[test]
fn the_end_row_and_the_total_line_do_not_change_the_game() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    rig.play("e2e4");
    assert!(rig.sans().contains(&"[end]".to_owned()));
    let (nodes, ply) = (rig.nodes(), rig.ply());
    for text in ["[end]", "TOTAL"] {
        let sent = click_pane(&mut rig, text);
        assert_eq!(row_clicks(&sent), 0, "{text}");
    }
    assert_eq!((rig.nodes(), rig.ply()), (nodes, ply));
    assert!(rig.app().session.prompt().is_none());
}

#[test]
fn clicks_are_ignored_while_the_search_runs() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    let mv = rig.app().session.position().parse_uci("a2a3").unwrap();
    send_without_running(
        &mut rig,
        GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        },
    );
    assert!(rig.view().searching);
    let (nodes, ply) = (rig.nodes(), rig.ply());
    let sent = click_pane(&mut rig, "e4");
    assert_eq!(row_clicks(&sent), 0, "the row is not a button now");
    assert_eq!((rig.nodes(), rig.ply()), (nodes, ply));
}

#[test]
fn clicks_are_ignored_with_refresh_off_and_the_table_stays() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    click_pane(&mut rig, "Refresh");
    assert!(!rig.view().autorefresh);
    let shown = rig.sans();
    let (nodes, ply) = (rig.nodes(), rig.ply());
    let sent = click_pane(&mut rig, "e4");
    assert_eq!(row_clicks(&sent), 0);
    assert_eq!((rig.nodes(), rig.ply()), (nodes, ply));
    // A move made elsewhere does not refresh the table either.
    rig.play("d2d4");
    assert_eq!(rig.sans(), shown, "the table stays as it is");
    assert!(pane_shows(&rig, "Nf3"));
}

#[test]
fn the_next_move_row_follows_the_cursor_and_nothing_is_highlighted_past_the_line() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    rig.play("e2e4");
    rig.play("e7e5");
    rig.nav(Nav::Start);
    let flagged = |rig: &TreeRig| -> Vec<String> {
        rig.view()
            .rows
            .iter()
            .filter(|r| r.is_next_move)
            .map(|r| r.san.clone())
            .collect()
    };
    assert_eq!(flagged(&rig), ["e4"]);
    rig.nav(Nav::Forward(1));
    assert_eq!(flagged(&rig), ["e5"]);
    rig.nav(Nav::End);
    assert!(flagged(&rig).is_empty());
}

// ---- keys ----

#[test]
fn the_arrows_keep_moving_the_game_with_the_panel_open_and_the_table_follows() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    rig.play("e2e4");
    rig.play("e7e5");
    assert_eq!(rig.ply(), 2);
    rig.key(named(NamedKey::Left));
    assert_eq!(rig.ply(), 1, "Left");
    assert!(rig.sans().contains(&"e5".to_owned()), "table of 1.e4");
    rig.key(named(NamedKey::Left));
    assert_eq!(rig.ply(), 0);
    assert!(rig.sans().contains(&"Nf3".to_owned()), "table of the start");
    rig.key(named(NamedKey::Right));
    assert_eq!(rig.ply(), 1, "Right");
    rig.key(named(NamedKey::End));
    assert_eq!(rig.ply(), 2, "End");
    rig.key(named(NamedKey::Home));
    assert_eq!(rig.ply(), 0, "Home");
    assert!(pane_shows(&rig, "d4"));
}

#[test]
fn clicking_the_panel_does_not_stop_the_arrows() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    rig.play("e2e4");
    click_pane(&mut rig, "e5");
    assert_eq!(rig.ply(), 2);
    rig.key(named(NamedKey::Left));
    assert_eq!(rig.ply(), 1);
}

#[test]
fn the_panel_toolbar_holds_no_text_field_that_would_take_the_keys() {
    // Spec D983: the panel does not handle the keyboard. With the panel
    // open no key reaches a text input: a letter key typed while the panel
    // is the last thing clicked still reaches the shortcut map.
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    rig.play("e2e4");
    click_pane(&mut rig, "Short display");
    rig.key(named(NamedKey::Left));
    assert_eq!(rig.ply(), 0);
}
