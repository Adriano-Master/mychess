//! M5-2b GUI contract: the *Best Games* section of the tree panel (spec
//! `docs/specs/m5-2-janela-da-arvore.md`, D1600-D1612, behaviour 1): the
//! button, the controls, the columns, the selection, load, browse and copy.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_gui_common;

use iced::widget::Id;
use m5_2_gui_common::*;
use mychess_app::{BestColumn, BestMessage, BestOrder, BestResults, LanguagePref, PickMode};
use tempfile::tempdir;

fn panel(dir: &std::path::Path) -> TreeRig {
    let mut rig = TreeRig::with_std(dir);
    rig.open_panel();
    assert_eq!(rig.view().body, TreeBody::Rows);
    rig
}

fn with_best(dir: &std::path::Path) -> TreeRig {
    let mut rig = panel(dir);
    click_pane(&mut rig, "Best games");
    assert!(rig.view().best.is_some(), "the section is open");
    rig
}

fn best_msg(message: BestMessage) -> Message {
    tree_msg(TreeMessage::Best(message))
}

fn press_at(sim: &mut iced_test::Simulator<'_, Message>, x: f32, y: f32) {
    sim.point_at(iced::Point::new(x, y));
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

/// The centre of the row of controls of the section.
fn controls_y(rig: &TreeRig) -> f32 {
    let mut sim = pane_sim(rig);
    let bounds = sim.find("Browse").expect("Browse").bounds();
    bounds.y + bounds.height / 2.0
}

/// Presses at `(x, y)` and then `dy` away (the first opens a list, the
/// second chooses an option) and returns what was sent.
fn press_pair(rig: &TreeRig, x: f32, y: f32, dy: f32) -> Vec<Message> {
    let mut sim = pane_sim(rig);
    press_at(&mut sim, x, y);
    press_at(&mut sim, x, y + dy);
    sim.into_messages().collect()
}

/// The x of the list of the section that sends messages with `needle`
/// (a `pick_list` draws its text without a text widget, so it is found by
/// position; the list opens up or down depending on the room).
fn list_x(rig: &TreeRig, needle: &str) -> f32 {
    let y = controls_y(rig);
    let mut x = 10.0;
    while x < 520.0 {
        for dy in [-20.0, 20.0] {
            if press_pair(rig, x, y, dy)
                .iter()
                .any(|m| dbg(m).contains(needle))
            {
                return x;
            }
        }
        x += 8.0;
    }
    panic!("no list that sends {needle}");
}

/// Messages (by `Debug` text) the options of the list that sends `needle`
/// give, in the order of the options.
fn options_of(rig: &TreeRig, needle: &str) -> Vec<String> {
    let (x, y) = (list_x(rig, needle), controls_y(rig));
    let mut found: Vec<(f32, String)> = Vec::new();
    let mut dy = -260.0;
    while dy < 260.0 {
        for m in press_pair(rig, x, y, dy) {
            let text = dbg(&m);
            if text.contains(needle) && !found.iter().any(|(_, t)| *t == text) {
                found.push((dy, text));
            }
        }
        dy += 6.0;
    }
    // Top to bottom on screen = by increasing `dy`.
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().map(|(_, t)| t).collect()
}

fn choose(rig: &mut TreeRig, wanted: &Message) {
    let needle = dbg(wanted);
    let needle = needle.split('(').nth(3).unwrap_or("").to_owned();
    let (x, y) = (list_x(rig, &needle), controls_y(rig));
    let mut dy = -260.0;
    while dy < 260.0 {
        let messages = press_pair(rig, x, y, dy);
        if sent(&messages, wanted) {
            for m in messages {
                rig.msg(m);
            }
            return;
        }
        dy += 6.0;
    }
    panic!("no option sends {wanted:?}");
}

fn click_row(rig: &mut TreeRig, number: u32) -> Vec<Message> {
    let messages: Vec<Message> = {
        let mut sim = pane_sim(rig);
        sim.click(Id::from(format!("tree-best-row-{number}")))
            .unwrap_or_else(|e| panic!("row {number}: {e:?}"));
        sim.into_messages().collect()
    };
    for m in messages.clone() {
        rig.msg(m);
    }
    messages
}

fn selected(rig: &TreeRig) -> Vec<u32> {
    rig.view()
        .best
        .map(|b| b.selected.iter().copied().collect())
        .unwrap_or_default()
}

fn numbers(rig: &TreeRig) -> Vec<u32> {
    rig.view()
        .best
        .map(|b| b.rows.iter().map(|r| r.game.number).collect())
        .unwrap_or_default()
}

fn pane_has_id(rig: &TreeRig, id: &str) -> bool {
    pane_sim(rig).find(Id::from(id.to_owned())).is_ok()
}

// ---- the button and the section ----

#[test]
fn the_button_opens_and_closes_the_section_below_the_table() {
    let dir = tempdir().unwrap();
    let mut rig = panel(dir.path());
    assert!(pane_shows(&rig, "Best games"));
    assert!(!pane_shows(&rig, "Browse"));
    let sent_ = click_pane(&mut rig, "Best games");
    assert!(sent(&sent_, &best_msg(BestMessage::Toggle)));
    assert!(pane_shows(&rig, "Browse"));
    // Below the table: the total row is above the section.
    let mut sim = pane_sim(&rig);
    let table = row_bounds(&mut sim, "tree-total");
    let controls = sim.find("Browse").unwrap().bounds();
    assert!(controls.y > table.y, "{controls:?} under {table:?}");
    drop(sim);
    click_pane(&mut rig, "Best games");
    assert!(!pane_shows(&rig, "Browse"));
    assert!(rig.view().best.is_none());
}

#[test]
fn every_label_of_the_section_is_there_in_english() {
    let dir = tempdir().unwrap();
    let rig = with_best(dir.path());
    for text in [
        "Result", "Sort", "Games", "Columns", "Load", "Browse", "Copy",
    ] {
        assert!(pane_shows(&rig, text), "{text}");
    }
}

#[test]
fn every_label_of_the_section_is_there_in_portuguese() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_language(LanguagePref::PtBr);
    rig.open_pgn(dir.path(), "std.pgn", STD);
    rig.open_panel();
    click_pane(&mut rig, "Melhores partidas");
    for text in [
        "Resultado",
        "Ordem",
        "Partidas",
        "Colunas",
        "Carregar",
        "Navegar",
        "Copiar",
    ] {
        assert!(pane_shows(&rig, text), "{text}");
    }
    assert!(!pane_shows(&rig, "Browse"));
}

#[test]
fn one_row_per_game_with_the_default_columns() {
    let dir = tempdir().unwrap();
    let rig = with_best(dir.path());
    for n in 1..=5 {
        assert!(pane_has_id(&rig, &format!("tree-best-row-{n}")), "row {n}");
    }
    assert!(!pane_has_id(&rig, "tree-best-row-6"));
    // Headers of the default columns; the event is not one of them.
    for header in ["White", "Black", "Result", "Date", "ECO"] {
        assert!(pane_shows(&rig, header), "{header}");
    }
    assert!(!pane_shows(&rig, "Event"));
}

#[test]
fn an_empty_list_says_so() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    rig.play("a2a3");
    assert!(numbers(&rig).is_empty());
    assert!(pane_shows(&rig, "No games to show"));
}

#[test]
fn the_list_follows_the_game_on_the_board() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    rig.play("e2e4");
    assert_eq!(numbers(&rig).len(), 3);
    assert!(!pane_has_id(&rig, "tree-best-row-2"));
    assert!(pane_has_id(&rig, "tree-best-row-1"));
}

// ---- controls ----

#[test]
fn the_result_list_offers_every_choice_in_order() {
    let dir = tempdir().unwrap();
    let rig = with_best(dir.path());
    let found = options_of(&rig, "SetResults");
    let wanted: Vec<String> = BestResults::ALL
        .into_iter()
        .map(|r| dbg(&best_msg(BestMessage::SetResults(r))))
        .collect();
    assert_eq!(found, wanted);
}

#[test]
fn the_order_list_offers_both_choices() {
    let dir = tempdir().unwrap();
    let rig = with_best(dir.path());
    let found = options_of(&rig, "SetOrder");
    let wanted: Vec<String> = BestOrder::ALL
        .into_iter()
        .map(|o| dbg(&best_msg(BestMessage::SetOrder(o))))
        .collect();
    assert_eq!(found, wanted);
}

#[test]
fn the_games_list_offers_the_six_maximums() {
    let dir = tempdir().unwrap();
    let rig = with_best(dir.path());
    let found = options_of(&rig, "SetMax");
    let wanted: Vec<String> = [20, 50, 100, 250, 500, 1000]
        .into_iter()
        .map(|m| dbg(&best_msg(BestMessage::SetMax(m))))
        .collect();
    assert_eq!(found, wanted);
}

#[test]
fn choosing_a_result_shortens_the_list() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    choose(
        &mut rig,
        &best_msg(BestMessage::SetResults(BestResults::Draw)),
    );
    assert_eq!(numbers(&rig), [3]);
    assert!(pane_has_id(&rig, "tree-best-row-3"));
    assert!(!pane_has_id(&rig, "tree-best-row-1"));
}

#[test]
fn choosing_a_maximum_cuts_the_list() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    choose(&mut rig, &best_msg(BestMessage::SetMax(20)));
    assert_eq!(rig.view().best.unwrap().max, 20);
    assert_eq!(numbers(&rig).len(), 5);
}

// ---- columns ----

#[test]
fn the_columns_button_shows_a_box_per_column_and_closes_again() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    assert!(!pane_shows(&rig, "Event"));
    let sent_ = click_pane(&mut rig, "Columns");
    assert!(sent(&sent_, &best_msg(BestMessage::ToggleColumnMenu)));
    assert!(rig.view().best.unwrap().columns_open);
    assert!(pane_shows(&rig, "Event"), "the box of the event column");
    click_pane(&mut rig, "Columns");
    assert!(!pane_shows(&rig, "Event"));
}

#[test]
fn a_box_turns_a_column_on_and_the_header_appears() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    click_pane(&mut rig, "Columns");
    let sent_ = click_pane(&mut rig, "Event");
    assert!(sent(
        &sent_,
        &best_msg(BestMessage::ToggleColumn(BestColumn::Event))
    ));
    assert!(
        rig.view()
            .best
            .unwrap()
            .columns
            .contains(&BestColumn::Event)
    );
    // The box and the header both say "Event".
    click_pane(&mut rig, "Columns");
    assert!(pane_shows(&rig, "Event"));
}

#[test]
fn the_last_column_stays_when_every_box_is_cleared() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    for column in BestColumn::ALL {
        rig.msg(best_msg(BestMessage::ToggleColumn(column)));
    }
    let columns = rig.view().best.unwrap().columns;
    assert_eq!(columns.len(), 1);
    // The rows are still drawn, with that column.
    assert!(pane_has_id(&rig, "tree-best-row-1"));
}

// ---- selection ----

#[test]
fn a_click_on_a_row_selects_it_and_only_it() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    let sent_ = click_row(&mut rig, 2);
    assert!(sent(
        &sent_,
        &best_msg(BestMessage::Pick {
            number: 2,
            mode: PickMode::Only
        })
    ));
    assert_eq!(selected(&rig), [2]);
    click_row(&mut rig, 4);
    assert_eq!(selected(&rig), [4]);
    assert!(
        sent_.iter().all(|m| !dbg(m).contains("Load")),
        "a single click loads nothing"
    );
}

#[test]
fn a_double_click_loads_the_game() {
    let dir = tempdir().unwrap();
    let rig = with_best(dir.path());
    let messages: Vec<Message> = {
        let mut sim = pane_sim(&rig);
        sim.click(Id::from("tree-best-row-4".to_owned())).unwrap();
        sim.click(Id::from("tree-best-row-4".to_owned())).unwrap();
        sim.into_messages().collect()
    };
    assert!(
        sent(&messages, &best_msg(BestMessage::Load(4))),
        "{messages:?}"
    );
}

#[test]
fn load_browse_and_copy_do_nothing_without_a_selection() {
    let dir = tempdir().unwrap();
    let rig = with_best(dir.path());
    for label in ["Load", "Browse", "Copy"] {
        let mut sim = pane_sim(&rig);
        sim.click(label).unwrap();
        let messages: Vec<Message> = sim.into_messages().collect();
        assert!(messages.is_empty(), "{label}: {messages:?}");
    }
}

#[test]
fn with_a_selection_the_buttons_send_their_messages() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    click_row(&mut rig, 3);
    let load = click_pane_without_running(&rig, "Load");
    assert!(sent(&load, &best_msg(BestMessage::Load(3))), "{load:?}");
    let browse = click_pane_without_running(&rig, "Browse");
    assert!(sent(&browse, &best_msg(BestMessage::Browse)), "{browse:?}");
    let copy = click_pane_without_running(&rig, "Copy");
    assert!(sent(&copy, &best_msg(BestMessage::Copy)), "{copy:?}");
}

fn click_pane_without_running(rig: &TreeRig, text: &str) -> Vec<Message> {
    let mut sim = pane_sim(rig);
    sim.click(text).unwrap();
    sim.into_messages().collect()
}

#[test]
fn load_takes_the_first_selected_game_in_the_order_of_the_list() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::new();
    let games: String = [
        rated_game("1. e4 e5", "1-0", 2000, 2000, "2001.01.01"),
        rated_game("1. e4 c5", "0-1", 2700, 2600, "2001.01.01"),
        rated_game("1. d4 d5", "1/2-1/2", 2500, 2500, "2001.01.01"),
    ]
    .concat();
    rig.open_si4(dir.path(), "rated", &games);
    rig.open_panel();
    click_pane(&mut rig, "Best games");
    assert_eq!(numbers(&rig), [2, 3, 1]);
    rig.msg(best_msg(BestMessage::Pick {
        number: 1,
        mode: PickMode::Only,
    }));
    rig.msg(best_msg(BestMessage::Pick {
        number: 3,
        mode: PickMode::Toggle,
    }));
    let load = click_pane_without_running(&rig, "Load");
    assert!(sent(&load, &best_msg(BestMessage::Load(3))), "{load:?}");
}

#[test]
fn copy_is_disabled_when_the_database_is_the_clipbase() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    rig.msg(best_msg(BestMessage::Pick {
        number: 1,
        mode: PickMode::Only,
    }));
    // Copy the games to the clipbase and go there.
    let slot = rig.app().bases.current();
    rig.app_msg(mychess_app::Message::Prefs(mychess_app::PrefsMessage::Set(
        mychess_app::PrefChange::ConfirmCopy(false),
    )));
    rig.app_msg(BasesMessage::List(mychess_app::ListMessage::CopyGames {
        from: slot,
        to: mychess_app::SlotId::CLIPBASE,
        scope: mychess_app::CopyScope::Filter,
    }));
    rig.app_msg(BasesMessage::Select(mychess_app::SlotId::CLIPBASE));
    assert!(rig.view().best.unwrap().in_clipbase);
    rig.msg(best_msg(BestMessage::Pick {
        number: 1,
        mode: PickMode::Only,
    }));
    let copy = click_pane_without_running(&rig, "Copy");
    assert!(copy.is_empty(), "{copy:?}");
    let browse = click_pane_without_running(&rig, "Browse");
    assert!(sent(&browse, &best_msg(BestMessage::Browse)));
}

#[test]
fn closing_the_panel_forgets_the_list() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    click_row(&mut rig, 2);
    rig.open_panel();
    assert!(!rig.panel_open());
    assert!(numbers(&rig).is_empty());
    assert!(selected(&rig).is_empty());
}
