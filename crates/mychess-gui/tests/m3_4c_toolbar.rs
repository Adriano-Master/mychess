//! The toolbar, the counters, the progress and the context menus of the game
//! list (spec M3-4, slice 4c: D556-D569; criterion "Barra/menus"), headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::redundant_closure_for_method_calls
)]

mod m3_4c_common;

use iced::{Point, Size, widget};
use iced_test::core::Event;
use iced_test::core::mouse;
use m3_4c_common::*;
use mychess_app::{BasesMessage, ListColumn, ListMessage, Message as AppMessage, SlotId};
use mychess_gui::Message;
use mychess_gui::windows::game_list::{Column, GameListMessage, ListAction, ListMenu, MenuTarget};

const FIND: usize = 0;
const FILTER: usize = 1;
const RESET: usize = 2;
const NEGATE: usize = 3;
const REMOVE: usize = 4;
const KEEP: usize = 5;
const COPY: usize = 6;
const DELETE: usize = 7;
const COLUMNS: usize = 8;

fn app_list(rig: &mut Rig, message: ListMessage) {
    rig.send(AppMessage::Bases(BasesMessage::List(message)));
}

fn press_tool(rig: &Rig, index: usize) -> Vec<String> {
    let boxes = tool_buttons(rig);
    assert!(boxes.len() > index, "toolbar has {} buttons", boxes.len());
    debug(&click_box(rig, boxes[index]))
}

fn mentions(messages: &[String], needle: &str) -> bool {
    messages.iter().any(|m| m.contains(needle))
}

// ---- buttons ----

#[test]
fn the_toolbar_has_eight_tools_and_the_columns_button() {
    let dir = tempfile::tempdir().unwrap();
    let rig = panel(dir.path(), 20);
    // Nine tools of the 4c plus the browser button of the 4e, last.
    assert_eq!(tool_buttons(&rig).len(), 10);
}

#[test]
fn with_nothing_selected_find_filter_reset_negate_and_columns_send_their_messages() {
    let dir = tempfile::tempdir().unwrap();
    let rig = panel(dir.path(), 20);
    assert!(mentions(&press_tool(&rig, FIND), "Act(Find)"));
    assert!(mentions(&press_tool(&rig, FILTER), "Act(FilterText)"));
    assert!(mentions(&press_tool(&rig, RESET), "Act(Reset)"));
    assert!(mentions(&press_tool(&rig, NEGATE), "Act(Negate)"));
    assert!(mentions(&press_tool(&rig, COLUMNS), "ColumnsButton"));
}

#[test]
fn with_nothing_selected_the_selection_tools_are_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let rig = panel(dir.path(), 20);
    for (index, name) in [
        (REMOVE, "remove"),
        (KEEP, "keep"),
        (COPY, "copy"),
        (DELETE, "delete"),
    ] {
        assert_eq!(press_tool(&rig, index), Vec::<String>::new(), "{name}");
    }
}

#[test]
fn with_a_selection_remove_keep_and_copy_work_and_delete_stays_off_on_a_pgn() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(1),
            number: 3,
        },
    );
    assert!(mentions(&press_tool(&rig, REMOVE), "Act(RemoveSelected)"));
    assert!(mentions(&press_tool(&rig, KEEP), "Act(Keep)"));
    assert!(mentions(&press_tool(&rig, COPY), "Act(CopySelected)"));
    assert_eq!(
        press_tool(&rig, DELETE),
        Vec::<String>::new(),
        "a PGN has no deleted flag to write (D560)"
    );
}

#[test]
fn in_the_clipbase_copy_and_delete_are_disabled_even_with_a_selection() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(1),
            number: 2,
        },
    );
    app_list(&mut rig, ListMessage::CopySelected { slot: slot(1) });
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    list(
        &mut rig,
        GameListMessage::Click {
            slot: SlotId::CLIPBASE,
            number: 1,
        },
    );
    assert_eq!(current(&rig).list.selection.len(), 1);
    assert!(mentions(&press_tool(&rig, REMOVE), "Act(RemoveSelected)"));
    assert_eq!(press_tool(&rig, COPY), Vec::<String>::new());
    assert_eq!(press_tool(&rig, DELETE), Vec::<String>::new());
}

#[test]
fn while_a_job_runs_every_tool_but_columns_is_disabled_and_cancel_is_offered() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(1),
            number: 3,
        },
    );
    // A sort in flight: the effect is not performed, so the job stays.
    let effects = mychess_app::update(
        &mut rig.shell.app,
        AppMessage::Bases(BasesMessage::List(ListMessage::SortBy {
            slot: slot(1),
            column: ListColumn::White,
        })),
    );
    assert_eq!(effects.len(), 1);
    assert!(rig.shell.app.list_job().is_some());
    let boxes = tool_buttons(&rig);
    for index in [FIND, FILTER, RESET, NEGATE, REMOVE, KEEP, COPY, DELETE] {
        assert!(
            debug(&click_box(&rig, boxes[index])).is_empty(),
            "tool {index} must be off during a job"
        );
    }
    assert!(mentions(
        &debug(&click_box(&rig, boxes[COLUMNS])),
        "ColumnsButton"
    ));
    // The progress text and the cancel button (the last 24x24 box).
    assert!(has_text(&rig, "Sorting: 0%"), "progress text");
    let last = *boxes.last().unwrap();
    assert!(mentions(&debug(&click_box(&rig, last)), "Act(Cancel)"));
}

#[test]
fn cancel_through_the_ui_clears_the_job_and_the_progress() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    let mut effects = mychess_app::update(
        &mut rig.shell.app,
        AppMessage::Bases(BasesMessage::List(ListMessage::SortBy {
            slot: slot(1),
            column: ListColumn::White,
        })),
    );
    list(&mut rig, GameListMessage::Act(ListAction::Cancel));
    // The effect now runs, sees the cancellation and answers.
    let answer = {
        let rt = b::runtime();
        rt.block_on(mychess_app::perform(effects.remove(0)))
    };
    rig.app_msg(answer);
    assert!(rig.shell.app.list_job().is_none());
    assert!(!has_text(&rig, "Sorting: 0%"));
}

// ---- counters ----

#[test]
fn the_counter_shows_filtered_over_total_and_follows_a_filter() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 40);
    assert!(has_text(&rig, "40/40 games"));
    list(&mut rig, GameListMessage::SearchInput("White 1".into()));
    list(&mut rig, GameListMessage::Act(ListAction::FilterText));
    // "White 1", "White 10".."White 19".
    assert!(has_text(&rig, "11/40 games"), "counter after Filter");
    list(&mut rig, GameListMessage::Act(ListAction::Reset));
    assert!(has_text(&rig, "40/40 games"));
}

#[test]
fn the_tab_counter_follows_the_filter_too() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 40);
    list(&mut rig, GameListMessage::SearchInput("White 3".into()));
    list(&mut rig, GameListMessage::Act(ListAction::FilterText));
    assert!(has_text(&rig, "11/40"), "tab shows 11/40");
}

#[test]
fn the_number_of_selected_games_is_shown_only_when_there_is_a_selection() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    assert!(!has_text(&rig, "2 selected"));
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(1),
            number: 1,
        },
    );
    rig.msg(Message::Games(GameListMessage::Modifiers {
        command: true,
        shift: false,
    }));
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(1),
            number: 4,
        },
    );
    assert!(has_text(&rig, "2 selected"));
}

#[test]
fn a_find_that_finds_nothing_leaves_a_status_remark() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    list(&mut rig, GameListMessage::SearchInput("nobody".into()));
    list(&mut rig, GameListMessage::Act(ListAction::Find));
    assert_eq!(
        rig.shell.app.notice,
        Some(mychess_app::AppNotice::Bases(
            mychess_app::BasesNotice::NotFound
        ))
    );
}

// ---- the search field ----

#[test]
fn typing_in_the_search_field_sends_the_text_and_enter_runs_find() {
    let dir = tempfile::tempdir().unwrap();
    let rig = panel(dir.path(), 20);
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    sim.click(widget::Id::new("glist-search"))
        .expect("the field");
    sim.typewrite("ab");
    sim.tap_key(iced::keyboard::Key::Named(
        iced::keyboard::key::Named::Enter,
    ));
    let messages = debug(&sim.into_messages().collect::<Vec<_>>());
    assert!(mentions(&messages, "SearchInput(\"a\")"), "{messages:?}");
    assert!(
        mentions(&messages, "Act(Find)"),
        "Enter in the field is Find (D565): {messages:?}"
    );
}

#[test]
fn the_ignore_case_box_sends_its_state() {
    let dir = tempfile::tempdir().unwrap();
    let rig = panel(dir.path(), 20);
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    sim.click("Ignore case").expect("the box");
    let messages = debug(&sim.into_messages().collect::<Vec<_>>());
    assert!(mentions(&messages, "IgnoreCase(true)"), "{messages:?}");
}

#[test]
fn the_search_text_and_the_case_box_are_per_slot() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    open_more(&mut rig, dir.path(), "b.pgn", 20);
    // Slot 2 is current now.
    list(&mut rig, GameListMessage::SearchInput("zzz".into()));
    list(&mut rig, GameListMessage::IgnoreCase(true));
    rig.send(BasesMessage::Select(slot(1)));
    let first = rig.shell.app.bases.slot(slot(1)).unwrap();
    assert_eq!(first.list.search, "");
    assert!(!first.list.ignore_case);
    rig.send(BasesMessage::Select(slot(2)));
    let second = rig.shell.app.bases.slot(slot(2)).unwrap();
    assert_eq!(second.list.search, "zzz");
    assert!(second.list.ignore_case);
}

// ---- headers ----

#[test]
fn clicking_a_sortable_header_sends_the_sort_and_a_fixed_header_sends_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let rig = panel(dir.path(), 20);
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    sim.click("Black").expect("the Black header");
    let messages = debug(&sim.into_messages().collect::<Vec<_>>());
    assert!(mentions(&messages, "Header(Black)"), "{messages:?}");
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    sim.click("Nº").expect("the Number header");
    let messages = debug(&sim.into_messages().collect::<Vec<_>>());
    assert!(
        !mentions(&messages, "Header"),
        "Number does not sort: {messages:?}"
    );
}

#[test]
fn a_header_click_sorts_the_list_and_a_second_one_flips_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 12);
    // "White 1" < "White 10" < "White 11" < "White 12" < "White 2" ...
    list(&mut rig, GameListMessage::Header(Column::White));
    let numbers = |rig: &Rig| -> Vec<u32> {
        rig.shell
            .app
            .bases
            .list_page(slot(1), 1, 12)
            .iter()
            .map(|r| r.number)
            .collect()
    };
    assert_eq!(numbers(&rig), [1, 10, 11, 12, 2, 3, 4, 5, 6, 7, 8, 9]);
    list(&mut rig, GameListMessage::Header(Column::White));
    assert_eq!(numbers(&rig), [9, 8, 7, 6, 5, 4, 3, 2, 12, 11, 10, 1]);
    // The Number column still shows the real numbers (the rows say so).
    let rows = rig.shell.app.bases.list_page(slot(1), 1, 3);
    assert_eq!(rows[0].white, "White 9");
    assert_eq!(rows[0].number, 9);
}

// ---- row clicks, right clicks and menus ----

#[test]
fn a_right_click_on_a_row_opens_its_menu_at_the_pointer() {
    let dir = tempfile::tempdir().unwrap();
    let rig = panel(dir.path(), 20);
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    let target = sim.find(widget::Id::new("games-row-3")).expect("row 3");
    let at = target.bounds().center();
    sim.point_at(at);
    drop(sim.simulate([Event::Mouse(mouse::Event::ButtonPressed(
        mouse::Button::Right,
    ))]));
    let messages = debug(&sim.into_messages().collect::<Vec<_>>());
    assert!(
        mentions(&messages, "Menu(ListMenu { target: Row(3)"),
        "{messages:?}"
    );
}

#[test]
fn a_right_click_on_a_header_opens_the_menu_of_that_column() {
    let dir = tempfile::tempdir().unwrap();
    let rig = panel(dir.path(), 20);
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    let target = sim.find("Black").expect("header");
    sim.point_at(target.bounds().center());
    drop(sim.simulate([Event::Mouse(mouse::Event::ButtonPressed(
        mouse::Button::Right,
    ))]));
    let messages = debug(&sim.into_messages().collect::<Vec<_>>());
    assert!(mentions(&messages, "target: Header(Black)"), "{messages:?}");
}

#[test]
fn a_plain_left_click_does_not_open_a_menu() {
    let dir = tempfile::tempdir().unwrap();
    let rig = panel(dir.path(), 20);
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    sim.click(widget::Id::new("games-row-3")).expect("row");
    let messages = debug(&sim.into_messages().collect::<Vec<_>>());
    assert!(!mentions(&messages, "Menu("), "{messages:?}");
    assert!(mentions(&messages, "Click"), "{messages:?}");
}

fn menu_at(target: MenuTarget) -> ListMenu {
    ListMenu {
        target,
        position: Point::new(300.0, 200.0),
    }
}

fn click_text(rig: &Rig, label: &str) -> Vec<String> {
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    sim.click(label)
        .unwrap_or_else(|e| panic!("{label}: {e:?}"));
    debug(
        &sim.into_messages()
            .filter(|m| !format!("{m:?}").contains("Hover("))
            .collect::<Vec<_>>(),
    )
}

#[test]
fn the_row_menu_lists_its_items_and_leaves_out_the_read_only_toggle_for_a_pgn() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    open_menu(&mut rig, menu_at(MenuTarget::Row(4)));
    for label in [
        "Load game",
        "Select all visible games",
        "Remove from the filter",
        "Remove the games above from the filter",
        "Remove the games below from the filter",
        "Keep only the selected games",
        "Copy to the Clipbase",
        "Toggle the deleted mark",
        "Reset the filter",
        "Negate the filter",
    ] {
        assert!(has_text(&rig, label), "row menu item {label:?}");
    }
    assert!(!has_text(&rig, "Mark the database read-only"), "SI4 only");
    assert!(!has_text(&rig, "Mark the database editable"));
}

#[test]
fn row_menu_items_send_the_right_messages_and_selection_items_follow_the_selection() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    // No selection: remove, keep, copy and delete are off; the others work.
    open_menu(&mut rig, menu_at(MenuTarget::Row(4)));
    for off in [
        "Remove from the filter",
        "Keep only the selected games",
        "Copy to the Clipbase",
        "Toggle the deleted mark",
    ] {
        assert_eq!(click_text(&rig, off), Vec::<String>::new(), "{off}");
    }
    assert!(mentions(&click_text(&rig, "Load game"), "Act(Load(4))"));
    assert!(mentions(
        &click_text(&rig, "Remove the games above from the filter"),
        "Act(RemoveAbove(4))"
    ));
    assert!(mentions(
        &click_text(&rig, "Remove the games below from the filter"),
        "Act(RemoveBelow(4))"
    ));
    assert!(mentions(
        &click_text(&rig, "Reset the filter"),
        "Act(Reset)"
    ));
    assert!(mentions(
        &click_text(&rig, "Negate the filter"),
        "Act(Negate)"
    ));
    assert!(mentions(
        &click_text(&rig, "Select all visible games"),
        "Act(SelectVisible)"
    ));
    // With a selection the selection items work (not delete: it is a PGN).
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(1),
            number: 4,
        },
    );
    open_menu(&mut rig, menu_at(MenuTarget::Row(4)));
    assert!(mentions(
        &click_text(&rig, "Remove from the filter"),
        "Act(RemoveSelected)"
    ));
    assert!(mentions(
        &click_text(&rig, "Keep only the selected games"),
        "Act(Keep)"
    ));
    assert!(mentions(
        &click_text(&rig, "Copy to the Clipbase"),
        "Act(CopySelected)"
    ));
    assert_eq!(
        click_text(&rig, "Toggle the deleted mark"),
        Vec::<String>::new(),
        "PGN: no flag"
    );
}

#[test]
fn the_right_click_message_selects_a_row_outside_the_selection_and_keeps_one_inside() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(1),
            number: 2,
        },
    );
    rig.msg(Message::Games(GameListMessage::Modifiers {
        command: true,
        shift: false,
    }));
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(1),
            number: 5,
        },
    );
    rig.msg(Message::Games(GameListMessage::Modifiers {
        command: false,
        shift: false,
    }));
    // Inside the selection: nothing changes.
    list(&mut rig, GameListMessage::Menu(menu_at(MenuTarget::Row(5))));
    assert_eq!(
        current(&rig)
            .list
            .selection
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [2, 5]
    );
    // Outside it: the row becomes the selection.
    list(&mut rig, GameListMessage::Menu(menu_at(MenuTarget::Row(9))));
    assert_eq!(
        current(&rig)
            .list
            .selection
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [9]
    );
    // A header menu leaves the selection alone.
    list(
        &mut rig,
        GameListMessage::Menu(menu_at(MenuTarget::Header(Column::White))),
    );
    assert_eq!(current(&rig).list.selection.len(), 1);
}

#[test]
fn a_menu_choice_closes_the_menu() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    open_menu(&mut rig, menu_at(MenuTarget::Row(2)));
    assert!(rig.shell.ui.games.menu.is_some());
    list(&mut rig, GameListMessage::Act(ListAction::Reset));
    assert!(rig.shell.ui.games.menu.is_none());
    assert!(!has_text(&rig, "Reset the filter"));
}

#[test]
fn the_header_menu_has_the_column_commands_and_sort_follows_the_column() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    open_menu(&mut rig, menu_at(MenuTarget::Header(Column::White)));
    for label in [
        "Sort by this column",
        "Align left",
        "Align center",
        "Align right",
        "Wider",
        "Narrower",
        "Move left",
        "Move right",
        "Hide this column",
        "Reset the columns",
    ] {
        assert!(has_text(&rig, label), "header menu item {label:?}");
    }
    assert!(mentions(
        &click_text(&rig, "Sort by this column"),
        "Header(White)"
    ));
    assert!(mentions(
        &click_text(&rig, "Wider"),
        "Columns(Wider(White))"
    ));
    assert!(mentions(
        &click_text(&rig, "Align center"),
        "Columns(Align(White, Center))"
    ));
    assert!(mentions(
        &click_text(&rig, "Hide this column"),
        "Columns(Toggle(White))"
    ));
    // A column that does not sort has the item off.
    open_menu(&mut rig, menu_at(MenuTarget::Header(Column::Number)));
    assert_eq!(
        click_text(&rig, "Sort by this column"),
        Vec::<String>::new()
    );
}

#[test]
fn the_columns_button_lists_the_22_columns_with_a_mark_on_the_shown_ones() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    list(
        &mut rig,
        GameListMessage::ColumnsButton(Point::new(300.0, 200.0)),
    );
    let shown = [
        "Nº", "White", "Black", "Result", "Moves", "Date", "Event", "WElo", "BElo", "ECO",
    ];
    for label in shown {
        assert!(
            has_text(&rig, &format!("\u{2713}  {label}")),
            "{label} is marked"
        );
    }
    for label in [
        "Round",
        "Site",
        "Del",
        "Var",
        "Com",
        "Ann",
        "Opening",
        "Flags",
        "Start",
        "Ctry",
        "Event date",
        "End material",
    ] {
        assert!(
            has_text(&rig, &format!("   {label}")),
            "{label} is listed unmarked"
        );
    }
    assert!(has_text(&rig, "Reset the columns"));
}

#[test]
fn choosing_a_column_in_the_list_adds_it_and_the_list_stays_open() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    list(
        &mut rig,
        GameListMessage::ColumnsButton(Point::new(300.0, 200.0)),
    );
    assert!(mentions(
        &click_text(&rig, "   Site"),
        "Columns(Toggle(Site))"
    ));
    list(
        &mut rig,
        GameListMessage::Columns(mychess_gui::windows::game_list::ColumnEdit::Toggle(
            Column::Site,
        )),
    );
    assert!(rig.shell.app.prefs.game_list.shows(ListColumn::Site));
    assert!(
        rig.shell.ui.games.menu.is_some(),
        "Toggle keeps the list open"
    );
    // The header now has the Site column.
    assert!(has_text(&rig, "Site"));
}
