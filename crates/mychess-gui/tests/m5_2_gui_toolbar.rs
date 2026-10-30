//! M5-2a GUI contract: the options bar of the panel (spec
//! `docs/specs/m5-2-janela-da-arvore.md`, D983, behaviours 3 and 4):
//! Refresh, Adjust filter, Short display, Sort and Search send the right
//! messages and reflect the state.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m5_2_gui_common;

use m5_2_gui_common::common;
use m5_2_gui_common::*;
use mychess_app::LanguagePref;
use tempfile::tempdir;

fn panel(dir: &std::path::Path) -> TreeRig {
    let mut rig = TreeRig::with_std(dir);
    rig.open_panel();
    rig
}

fn press_at(sim: &mut iced_test::Simulator<'_, Message>, x: f32, y: f32) {
    sim.point_at(iced::Point::new(x, y));
    // The menu of a list selects what the last `CursorMoved` hovered.
    drop(sim.simulate([
        iced::Event::Mouse(iced::mouse::Event::CursorMoved {
            position: iced::Point::new(x, y),
        }),
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
            iced::mouse::Button::Left,
        )),
    ]));
}

/// The list that follows the label `label` (a `pick_list` draws its text
/// without a text widget, so it is found by position): opens it, then
/// presses lower and lower until each option has sent its message. Returns
/// the distinct messages in the order of the options, top to bottom.
fn options_of(rig: &TreeRig, label: &str) -> Vec<String> {
    let mut sim = pane_sim(rig);
    let bounds = sim.find(label).expect("label").bounds();
    drop(sim);
    let (x, y) = (
        bounds.x + bounds.width + 24.0,
        bounds.y + bounds.height / 2.0,
    );
    let mut found: Vec<String> = Vec::new();
    let mut dy = 8.0;
    while dy < 130.0 {
        let mut sim = pane_sim(rig);
        press_at(&mut sim, x, y);
        press_at(&mut sim, x, y + dy);
        for m in sim.into_messages() {
            let text = dbg(&m);
            if text.contains("Tree(Set") && !found.contains(&text) {
                found.push(text);
            }
        }
        dy += 3.0;
    }
    found
}

/// Chooses the option that sends `wanted` in the list after `label`.
fn choose(rig: &mut TreeRig, label: &str, wanted: &Message) {
    let mut sim = pane_sim(rig);
    let bounds = sim.find(label).expect("label").bounds();
    drop(sim);
    let (x, y) = (
        bounds.x + bounds.width + 24.0,
        bounds.y + bounds.height / 2.0,
    );
    let mut dy = 8.0;
    while dy < 130.0 {
        let messages: Vec<Message> = {
            let mut sim = pane_sim(rig);
            press_at(&mut sim, x, y);
            press_at(&mut sim, x, y + dy);
            sim.into_messages().collect()
        };
        if sent(&messages, wanted) {
            for m in messages {
                rig.msg(m);
            }
            return;
        }
        dy += 3.0;
    }
    panic!("no option of the list after {label:?} sends {wanted:?}");
}

fn searches(rig: &TreeRig) -> u64 {
    rig.app().tree().searches_started()
}

#[test]
fn every_label_of_the_bar_is_there_in_english() {
    let dir = tempdir().unwrap();
    let rig = panel(dir.path());
    for text in [
        "Refresh",
        "Adjust filter",
        "Sort",
        "Search",
        "Short display",
    ] {
        assert!(pane_shows(&rig, text), "{text}");
    }
}

#[test]
fn every_label_of_the_bar_is_there_in_portuguese() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_language(LanguagePref::PtBr);
    rig.open_pgn(dir.path(), "std.pgn", STD);
    rig.open_panel();
    for text in [
        "Atualizar",
        "Ajustar filtro",
        "Ordenar",
        "Busca",
        "Exibição curta",
    ] {
        assert!(pane_shows(&rig, text), "{text}");
    }
}

#[test]
fn the_refresh_box_sends_its_message_and_the_state_follows() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    assert!(rig.view().autorefresh);
    let sent = click_pane(&mut rig, "Refresh");
    assert!(
        sent.iter()
            .any(|m| dbg(m) == dbg(&tree_msg(TreeMessage::ToggleAutoRefresh))),
        "{sent:?}"
    );
    assert!(!rig.view().autorefresh);
    click_pane(&mut rig, "Refresh");
    assert!(rig.view().autorefresh);
}

#[test]
fn turning_refresh_back_on_brings_the_table_up_to_date() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    click_pane(&mut rig, "Refresh");
    rig.play("e2e4");
    assert!(
        rig.sans().contains(&"Nf3".to_owned()),
        "still the old table"
    );
    click_pane(&mut rig, "Refresh");
    assert!(!rig.sans().contains(&"Nf3".to_owned()));
    assert!(rig.sans().contains(&"e5".to_owned()), "{:?}", rig.sans());
}

#[test]
fn the_adjust_box_asks_for_the_opposite_of_the_state() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    assert!(!rig.view().adjust);
    let sent = click_pane(&mut rig, "Adjust filter");
    assert!(
        sent.iter()
            .any(|m| dbg(m) == dbg(&tree_msg(TreeMessage::SetAdjust(true)))),
        "{sent:?}"
    );
    assert!(rig.view().adjust);
    let sent = click_pane(&mut rig, "Adjust filter");
    assert!(
        sent.iter()
            .any(|m| dbg(m) == dbg(&tree_msg(TreeMessage::SetAdjust(false)))),
        "{sent:?}"
    );
    assert!(!rig.view().adjust);
}

#[test]
fn adjusting_the_filter_narrows_the_game_list_to_the_games_of_the_position() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    rig.play("e2e4");
    let slot = rig.app().bases.current();
    assert_eq!(rig.app().bases.slot(slot).unwrap().filtered(), 5);
    click_pane(&mut rig, "Adjust filter");
    assert_eq!(
        rig.app().bases.slot(slot).unwrap().filtered(),
        3,
        "only the three games that reach 1.e4"
    );
    // Closing the panel gives the filter back.
    rig.open_panel();
    assert_eq!(rig.app().bases.slot(slot).unwrap().filtered(), 5);
}

#[test]
fn the_short_display_box_changes_the_columns_and_never_searches() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    let before = searches(&rig);
    let sent = click_pane(&mut rig, "Short display");
    assert!(
        sent.iter()
            .any(|m| dbg(m) == dbg(&tree_msg(TreeMessage::SetShort(false)))),
        "{sent:?}"
    );
    assert!(!rig.view().short);
    assert!(pane_shows(&rig, "ECO"));
    assert_eq!(searches(&rig), before);
    click_pane(&mut rig, "Short display");
    assert!(rig.view().short);
    assert!(!pane_shows(&rig, "ECO"));
    assert_eq!(searches(&rig), before);
}

#[test]
fn the_sort_list_offers_the_four_orders_and_choosing_one_searches_again() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    let before = searches(&rig);
    let options = options_of(&rig, "Sort");
    let expected: Vec<String> = [
        TreeSort::Alphabetical,
        TreeSort::Eco,
        TreeSort::Frequency,
        TreeSort::Score,
    ]
    .into_iter()
    .map(|s| dbg(&tree_msg(TreeMessage::SetSort(s))))
    .collect();
    assert_eq!(
        options, expected,
        "the four orders, in the order of the list"
    );
    choose(
        &mut rig,
        "Sort",
        &tree_msg(TreeMessage::SetSort(TreeSort::Alphabetical)),
    );
    assert_eq!(rig.view().sort, TreeSort::Alphabetical);
    assert!(searches(&rig) > before, "the order refreshes the search");
    // Alphabetical puts the capital letter first: Nf3, d4, e4.
    assert_eq!(rig.sans(), ["Nf3", "d4", "e4"]);
    let mut sim = pane_sim(&rig);
    let ys: Vec<f32> = rig.sans().iter().map(|s| y_of(&mut sim, s)).collect();
    assert!(ys.windows(2).all(|w| w[0] < w[1]), "{ys:?}");
}

#[test]
fn the_selection_of_the_lists_is_drawn_for_each_value() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    let mut seen = std::collections::BTreeSet::new();
    for sort in [
        TreeSort::Alphabetical,
        TreeSort::Eco,
        TreeSort::Frequency,
        TreeSort::Score,
    ] {
        rig.app_msg(TreeMessage::SetSort(sort));
        assert_eq!(rig.view().sort, sort);
        let mut sim = pane_sim(&rig);
        // The table may change too; the closed list text is part of it.
        let hash = common::fingerprint(&mut sim);
        seen.insert((sort as u8, hash));
    }
    let hashes: std::collections::BTreeSet<_> = seen.iter().map(|(_, h)| h.clone()).collect();
    assert_eq!(hashes.len(), 4, "each order drew a different panel");
    let mut modes = std::collections::BTreeSet::new();
    for mode in [
        TreeFastMode::Slow,
        TreeFastMode::Fast,
        TreeFastMode::FastThenSlow,
    ] {
        rig.app_msg(TreeMessage::SetFastMode(mode));
        assert_eq!(rig.view().fast_mode, mode);
        let mut sim = pane_sim(&rig);
        modes.insert(common::fingerprint(&mut sim));
    }
    assert_eq!(modes.len(), 3, "each mode drew a different list text");
}

#[test]
fn the_search_list_offers_slow_fast_and_both_and_the_choice_is_applied() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    let options = options_of(&rig, "Search");
    let expected: Vec<String> = [
        TreeFastMode::Slow,
        TreeFastMode::Fast,
        TreeFastMode::FastThenSlow,
    ]
    .into_iter()
    .map(|m| dbg(&tree_msg(TreeMessage::SetFastMode(m))))
    .collect();
    assert_eq!(options, expected);
    choose(
        &mut rig,
        "Search",
        &tree_msg(TreeMessage::SetFastMode(TreeFastMode::FastThenSlow)),
    );
    assert_eq!(rig.view().fast_mode, TreeFastMode::FastThenSlow);
    // Both phases ran for real; the final table is complete.
    assert_eq!(rig.view().body, TreeBody::Rows);
    assert!(!rig.view().searching);
    assert_eq!(rig.view().rows.len(), 3);
}

#[test]
fn the_lists_are_reachable_in_portuguese() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_language(LanguagePref::PtBr);
    rig.open_pgn(dir.path(), "std.pgn", STD);
    rig.open_panel();
    assert_eq!(options_of(&rig, "Ordenar").len(), 4);
    assert_eq!(options_of(&rig, "Busca").len(), 3);
}

#[test]
fn the_chosen_options_are_saved_in_the_preferences() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    rig.app_msg(TreeMessage::SetSort(TreeSort::Score));
    rig.app_msg(TreeMessage::SetShort(false));
    rig.app_msg(TreeMessage::SetFastMode(TreeFastMode::Fast));
    let text = mychess_app::render_preferences(&rig.app().prefs);
    let loaded = mychess_app::load_preferences(&text);
    assert_eq!(loaded.prefs.tree, rig.app().prefs.tree);
    assert!(!loaded.prefs.tree.short);
}
