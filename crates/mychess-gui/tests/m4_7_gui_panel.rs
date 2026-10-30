//! The graph panel in the window (spec M4-7, D870, D873-D875): opening,
//! menus, shortcut, toolbar, empty state, clicks and drawing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty,
    clippy::too_many_lines,
    clippy::float_cmp,
    clippy::cast_precision_loss
)]

mod m4_7_gui_common;

use m4_7_gui_common::*;
use mychess_app::{Action, Binding};
use mychess_gui::keymap::{Platform, binding_label};
use mychess_gui::windows::layout::LayoutNode;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};

fn menu_entries(rig: &Rig, key: &str) -> Vec<MenuEntry> {
    menu_specs(&rig.shell.app, &rig.shell.ui, rig.shell.options)
        .into_iter()
        .find(|s| s.label_key == key)
        .unwrap_or_else(|| panic!("menu {key}"))
        .entries
}

type Found<'a> = (Option<&'a Message>, Option<bool>, Option<&'a Binding>);

fn find_item<'a>(entries: &'a [MenuEntry], key: &str) -> Found<'a> {
    fn walk<'a>(entries: &'a [MenuEntry], key: &str) -> Option<Found<'a>> {
        entries.iter().find_map(|e| match e {
            MenuEntry::Item {
                label: Label::Key(k),
                action,
                checked,
                shortcut,
                ..
            } if *k == key => Some((action.as_ref(), *checked, shortcut.as_ref())),
            MenuEntry::Submenu { entries, .. } => walk(entries, key),
            _ => None,
        })
    }
    walk(entries, key).unwrap_or_else(|| panic!("menu item {key}"))
}

fn has(rig: &Rig, text: &str) -> bool {
    rig.sim(SIZE).find(text).is_ok()
}

fn open(rig: &Rig) -> bool {
    rig.shell.ui.is_open(PaneKind::Graph)
        && rig.shell.ui.layout().panes().contains(&PaneKind::Graph)
}

#[test]
fn the_panel_is_closed_by_default_and_the_app_builds_nothing() {
    let rig = Rig::with_game(evals_game());
    assert!(!open(&rig));
    assert!(!rig.shell.app.graph().is_open());
    assert!(rig.shell.app.graph().view().is_none());
}

#[test]
fn toggling_the_pane_opens_it_informs_the_app_and_closes_it() {
    let mut rig = rig_with_graph(evals_game());
    assert!(open(&rig));
    assert!(rig.shell.ui.layout().is_valid());
    assert!(rig.shell.app.graph().is_open());
    assert_eq!(rig.shell.app.graph().view().unwrap().bars.len(), 6);
    assert!(has(&rig, "Anna - Boris"));
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Graph)));
    assert!(!open(&rig));
    assert!(!rig.shell.app.graph().is_open());
    assert!(rig.shell.app.graph().view().is_none());
    assert!(!has(&rig, "Anna - Boris"));
}

#[test]
fn the_graph_pane_goes_to_the_bottom_with_the_board_above() {
    let rig = rig_with_graph(evals_game());
    let tree = rig.shell.ui.layout();
    match tree {
        LayoutNode::Split { ratio, b, .. } => {
            assert_eq!(*b, LayoutNode::pane(PaneKind::Graph), "graph at the bottom");
            assert!((ratio - 0.7).abs() < 0.02, "D874: 0.7, got {ratio}");
        }
        other @ LayoutNode::Group { .. } => panic!("{other:?}"),
    }
}

#[test]
fn the_pane_survives_opening_and_closing_the_other_panes() {
    let mut rig = rig_with_graph(evals_game());
    for kind in [PaneKind::Info, PaneKind::Analysis, PaneKind::GameList] {
        rig.msg(Message::Ui(UiMessage::TogglePane(kind)));
        assert!(open(&rig), "graph lost after toggling {kind:?} on");
        assert!(rig.shell.ui.layout().is_valid(), "{kind:?}");
    }
    for kind in [PaneKind::Info, PaneKind::Analysis, PaneKind::GameList] {
        rig.msg(Message::Ui(UiMessage::TogglePane(kind)));
        assert!(open(&rig), "graph lost after toggling {kind:?} off");
        assert!(rig.shell.ui.layout().is_valid(), "{kind:?}");
    }
    assert!(rig.shell.app.graph().is_open());
}

#[test]
fn the_pane_goes_into_the_saved_layout_and_a_new_session_restores_it() {
    let rig = rig_with_graph(evals_game());
    let saved = rig
        .shell
        .app
        .prefs
        .layout
        .clone()
        .expect("layout remembered");
    assert!(saved.panes().contains(&PaneKind::Graph));
    let text = mychess_app::render_preferences(&rig.shell.app.prefs);
    let loaded = mychess_app::load_preferences(&text);
    assert_eq!(loaded.prefs.layout, Some(saved));
}

#[test]
fn the_shortcut_command_shift_g_toggles_the_panel_and_shift_z_does_not() {
    let mut rig = Rig::with_game(evals_game());
    rig.press(command_shift(Key::Char('g')));
    assert!(open(&rig), "Command+Shift+G opens");
    rig.press(command_shift(Key::Char('g')));
    assert!(!open(&rig), "and closes");
    rig.press(command_shift(Key::Char('z')));
    assert!(!open(&rig), "Command+Shift+Z is Redo, not the graph (D870)");
}

#[test]
fn the_shortcut_works_with_text_focus_elsewhere_like_other_command_keys() {
    let mut rig = Rig::with_game(evals_game());
    rig.key(command_shift(Key::Char('g')), true, None);
    assert!(open(&rig));
}

#[test]
fn the_tools_menu_has_the_graph_item_with_state_and_shortcut() {
    let mut rig = Rig::with_game(evals_game());
    let entries = menu_entries(&rig, "menu-tools");
    let (action, checked, shortcut) = find_item(&entries, "menu-tools-graph");
    assert_eq!(checked, Some(false));
    assert!(matches!(
        action,
        Some(Message::Ui(UiMessage::TogglePane(PaneKind::Graph)))
    ));
    let b = shortcut.expect("a shortcut");
    assert_eq!(binding_label(b, Platform::Linux), "Ctrl+Shift+G");
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Graph)));
    let (_, checked, _) = find_item(&menu_entries(&rig, "menu-tools"), "menu-tools-graph");
    assert_eq!(checked, Some(true));
}

#[test]
fn the_view_menu_panes_submenu_has_the_checked_graph_entry() {
    let mut rig = Rig::with_game(evals_game());
    let entries = menu_entries(&rig, "menu-view");
    let (action, checked, _) = find_item(&entries, "menu-view-pane-graph");
    assert_eq!(checked, Some(false));
    assert!(matches!(
        action,
        Some(Message::Ui(UiMessage::TogglePane(PaneKind::Graph)))
    ));
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Graph)));
    let (_, checked, _) = find_item(&menu_entries(&rig, "menu-view"), "menu-view-pane-graph");
    assert_eq!(checked, Some(true));
}

#[test]
fn the_default_binding_for_the_action_is_the_documented_one() {
    let rig = Rig::new();
    let b = rig.shell.app.shortcuts.bindings(Action::ToggleGraphPane);
    assert_eq!(b.len(), 1);
    assert_eq!(binding_label(&b[0], Platform::MacOs), "⇧⌘G");
}

#[test]
fn the_toolbar_has_the_kind_list_and_the_three_checkboxes() {
    let rig = rig_with_graph(evals_game());
    // The `pick_list` draws its selection without a text widget, so the
    // kind list is checked through the drawing (see the next test).
    for text in ["Highlight current move", "Invert White", "Invert Black"] {
        assert!(has(&rig, text), "{text}");
    }
}

#[test]
fn the_kind_list_selection_is_drawn_for_each_of_the_five() {
    // Scores and times in the same game, so every kind draws differently
    // (and the list shows its label).
    let game = game_with(8, |i| {
        format!("[%eval {}] [%emt 0:00:{:02}]", i as f32 / 4.0, 3 + i)
    });
    let mut rig = rig_with_graph(game);
    let mut seen = std::collections::BTreeSet::new();
    for kind in [
        GraphKind::Auto,
        GraphKind::Score,
        GraphKind::Time,
        GraphKind::ScoreCombo,
        GraphKind::TimeCombo,
    ] {
        graph_msg(&mut rig, GraphMessage::SetKind(kind));
        assert_eq!(rig.shell.app.prefs.graph.kind, kind);
        assert!(
            seen.insert(shot(&rig, SIZE)),
            "{kind:?} drew like an earlier kind"
        );
    }
}

#[test]
fn each_checkbox_sends_its_message_and_the_state_follows() {
    let mut rig = rig_with_graph(evals_game());
    for (label, want) in [
        ("Highlight current move", GraphMessage::ToggleHighlight),
        ("Invert White", GraphMessage::ToggleInvertWhite),
        ("Invert Black", GraphMessage::ToggleInvertBlack),
    ] {
        let mut sim = rig.sim(SIZE);
        sim.click(label).expect(label);
        let sent: Vec<Message> = sim.into_messages().collect();
        let graph: Vec<_> = sent
            .iter()
            .filter_map(|m| match m {
                Message::App(AppMessage::Graph(g)) => Some(*g),
                _ => None,
            })
            .collect();
        assert_eq!(graph, vec![want], "{label}");
        for m in sent {
            rig.msg(m);
        }
    }
    let p = rig.shell.app.prefs.graph;
    assert!(!p.highlight_current && p.invert.white && p.invert.black);
}

#[test]
fn the_empty_game_says_there_is_nothing_to_show_and_a_game_with_data_does_not() {
    let rig = rig_with_graph(game_with(4, |_| String::new()));
    assert!(has(&rig, "No scores or times in this game"));
    let rig = rig_with_graph(evals_game());
    assert!(!has(&rig, "No scores or times in this game"));
}

#[test]
fn a_brand_new_game_with_zero_moves_does_not_break_the_panel() {
    let rig = rig_with_graph(Game::new());
    assert!(has(&rig, "No scores or times in this game"));
}

#[test]
fn switching_to_another_game_clears_the_old_graph() {
    let mut rig = rig_with_graph(evals_game());
    assert!(!has(&rig, "No scores or times in this game"));
    rig.set_game(game_with(2, |_| String::new()));
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(Nav::Start)));
    assert!(rig.shell.app.graph().view().unwrap().bars.is_empty());
    assert!(has(&rig, "No scores or times in this game"));
}

#[test]
fn clicks_along_the_plot_reach_the_app_in_order_and_cover_the_whole_game() {
    let mut rig = rig_with_graph(evals_game());
    // The bar-x click mapping is the Bars style (M4-9 D1400 made Line the
    // default; Line clicks go to GoToPly and are covered by the M4-9 tests).
    graph_msg(
        &mut rig,
        GraphMessage::SetStyle(mychess_app::graphs::GraphStyle::Bars),
    );
    let axis = rig.shell.app.graph().view().unwrap().x_axis;
    let xs = scan_row(&rig, SIZE, SIZE.height - 90.0, 3);
    assert!(xs.len() > 20, "clicks reached the canvas: {}", xs.len());
    assert!(
        xs.windows(2).all(|w| w[0] <= w[1] + 1e-4),
        "monotone: {xs:?}"
    );
    // Linear, from the start of the axis to its end (within one step).
    let d = (xs[1] - xs[0]).max(1e-6);
    assert!(
        xs.windows(2)
            .all(|w| ((w[1] - w[0]) - d).abs() < d * 0.2 + 1e-3)
    );
    assert!(
        xs[0] >= axis.min - 1e-4 && xs[0] - axis.min <= d * 2.0,
        "{} {}",
        xs[0],
        axis.min
    );
    let last = *xs.last().unwrap();
    assert!(
        last <= axis.max + 1e-4 && axis.max - last <= d * 2.0,
        "{last} {}",
        axis.max
    );
    // Applying them goes to plies 0 ..= 6, never beyond.
    let mut plies = Vec::new();
    for x in xs {
        rig.app_msg(AppMessage::Graph(GraphMessage::Click(x)));
        let p = rig.game().ply(rig.cursor()).unwrap();
        assert!(p <= 6, "{p}");
        plies.push(p);
    }
    assert_eq!(plies.first(), Some(&0));
    assert_eq!(plies.last(), Some(&6));
    assert!(plies.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn the_click_mapping_survives_a_resize_of_the_window() {
    let mut rig = rig_with_graph(evals_game());
    // Bars style: see the note in the test above.
    graph_msg(
        &mut rig,
        GraphMessage::SetStyle(mychess_app::graphs::GraphStyle::Bars),
    );
    let axis = rig.shell.app.graph().view().unwrap().x_axis;
    for size in [Size::new(800.0, 560.0), Size::new(1900.0, 1000.0)] {
        let xs = scan_row(&rig, size, size.height - 90.0, 3);
        assert!(xs.len() > 10, "{size:?}: {}", xs.len());
        let d = (xs[xs.len() - 1] - xs[0]) / xs.len() as f32;
        assert!(xs[0] - axis.min <= d * 3.0 + 1e-3, "{size:?}");
        assert!(axis.max - xs[xs.len() - 1] <= d * 3.0 + 1e-3, "{size:?}");
    }
}

use iced::Size;

#[test]
fn a_tiny_window_draws_and_clicks_without_panicking() {
    let rig = rig_with_graph(evals_game());
    for size in [
        Size::new(120.0, 90.0),
        Size::new(300.0, 200.0),
        Size::new(1.0, 1.0),
    ] {
        let xs = scan_row(&rig, size, size.height - 5.0, 7);
        for x in xs {
            assert!(x.is_finite(), "{size:?}");
        }
        drop(shot(&rig, size));
    }
}

#[test]
fn the_drawing_depends_on_the_kind_the_data_and_the_options() {
    let mut rig = rig_with_graph(evals_game());
    graph_msg(&mut rig, GraphMessage::SetKind(GraphKind::Score));
    let score = shot(&rig, SIZE);
    graph_msg(&mut rig, GraphMessage::SetKind(GraphKind::Auto));
    let bars = |rig: &Rig| rig.shell.app.graph().view().unwrap().bars.clone();
    let auto_bars = bars(&rig);
    graph_msg(&mut rig, GraphMessage::SetKind(GraphKind::Score));
    assert_eq!(auto_bars, bars(&rig), "Auto with 6 scores draws the scores");
    graph_msg(&mut rig, GraphMessage::SetKind(GraphKind::Time));
    assert_ne!(
        shot(&rig, SIZE),
        score,
        "no times: an empty graph is different"
    );
    graph_msg(&mut rig, GraphMessage::SetKind(GraphKind::Score));
    graph_msg(&mut rig, GraphMessage::ToggleInvertBlack);
    assert_ne!(shot(&rig, SIZE), score, "inverting changes the bars");
    graph_msg(&mut rig, GraphMessage::ToggleInvertBlack);
    assert_eq!(shot(&rig, SIZE), score, "and back");
}

#[test]
fn the_current_move_bar_follows_the_cursor_only_when_highlighting() {
    let mut rig = rig_with_graph(evals_game());
    let start = shot(&rig, SIZE);
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(
        Nav::GoToMainlinePly(3),
    )));
    let on3 = shot(&rig, SIZE);
    assert_ne!(on3, start, "the bar moved");
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(
        Nav::GoToMainlinePly(4),
    )));
    assert_ne!(shot(&rig, SIZE), on3);
    graph_msg(&mut rig, GraphMessage::ToggleHighlight);
    let off4 = shot(&rig, SIZE);
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(
        Nav::GoToMainlinePly(2),
    )));
    // The board changes with the cursor, so compare the graph through the
    // app: no bar is reported with the option off.
    assert_eq!(rig.shell.app.graph().current_x(), None);
    drop(off4);
}

#[test]
fn the_light_and_the_dark_theme_draw_the_graph_with_other_colors() {
    let rig = rig_with_graph(evals_game());
    let mut a = rig.sim(SIZE);
    let light = fingerprint_in(&mut a, &iced::Theme::Light);
    let mut b = rig.sim(SIZE);
    let dark = fingerprint_in(&mut b, &iced::Theme::Dark);
    assert_ne!(light, dark);
}

#[test]
fn extreme_data_renders_in_every_kind() {
    let hostile = game_with(6, |i| {
        [
            "[%emt 99999:59:59]",
            "[%eval 1e999]",
            "[%eval #]",
            "[%clk 0:00:00]",
            "[%eval 25]",
            "[%emt 0:00:00]",
        ][i]
            .to_owned()
    });
    let mut rig = rig_with_graph(hostile);
    for kind in GraphKind::ALL_FOR_TESTS {
        graph_msg(&mut rig, GraphMessage::SetKind(kind));
        drop(shot(&rig, SIZE));
    }
}

#[test]
fn a_game_of_six_thousand_plies_renders() {
    let rig = rig_with_graph(game_with(6000, |_| {
        "[%eval 0.20] [%clk 1:00:00]".to_owned()
    }));
    assert_eq!(rig.shell.app.graph().view().unwrap().plies, 6000);
    drop(shot(&rig, SIZE));
}

trait AllKinds {
    const ALL_FOR_TESTS: [GraphKind; 5];
}
impl AllKinds for GraphKind {
    const ALL_FOR_TESTS: [GraphKind; 5] = [
        GraphKind::Auto,
        GraphKind::Score,
        GraphKind::Time,
        GraphKind::ScoreCombo,
        GraphKind::TimeCombo,
    ];
}

#[test]
fn a_time_only_game_chooses_times_and_the_combo_draws_lines() {
    let mut rig = rig_with_graph(times_game());
    assert_eq!(rig.shell.app.graph().view().unwrap().kind, GraphKind::Time);
    graph_msg(&mut rig, GraphMessage::SetKind(GraphKind::ScoreCombo));
    let v = rig.shell.app.graph().view().unwrap();
    assert!(!v.white_line.is_empty() && !v.black_line.is_empty());
    assert!(!has(&rig, "No scores or times in this game"));
}

#[test]
fn the_panel_texts_are_in_portuguese_with_a_portuguese_session() {
    let mut rig = Rig::with_language(mychess_app::LanguagePref::PtBr);
    rig.set_game(evals_game());
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Graph)));
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(Nav::Start)));
    for text in ["Destacar o lance atual", "Inverter brancas"] {
        assert!(has(&rig, text), "{text}");
    }
    assert!(!has(&rig, "Highlight current move"));
}

#[test]
fn layout_helpers_insert_the_graph_once_and_remove_it() {
    let tree = LayoutNode::default_layout();
    let with = tree.open(PaneKind::Graph);
    assert!(with.is_valid());
    assert_eq!(
        with.panes()
            .iter()
            .filter(|k| **k == PaneKind::Graph)
            .count(),
        1
    );
    assert!(with.open(PaneKind::Graph).is_valid());
    let without = with.close(PaneKind::Graph);
    assert!(!without.panes().contains(&PaneKind::Graph));
    assert!(without.is_valid());
}

#[test]
fn a_toggle_through_the_app_path_never_leaves_the_app_out_of_step_with_the_layout() {
    let mut rig = Rig::with_game(evals_game());
    assert!(!rig.shell.ui.is_open(PaneKind::Graph));
    rig.app_msg(AppMessage::Graph(GraphMessage::Toggle));
    assert_eq!(
        rig.shell.app.graph().is_open(),
        rig.shell.ui.is_open(PaneKind::Graph),
        "the layout wins after the sync"
    );
    assert!(!rig.shell.app.graph().is_open());
    // The same with the pane open.
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Graph)));
    assert!(rig.shell.ui.is_open(PaneKind::Graph) && rig.shell.app.graph().is_open());
    rig.app_msg(AppMessage::Graph(GraphMessage::Toggle));
    assert_eq!(
        rig.shell.app.graph().is_open(),
        rig.shell.ui.is_open(PaneKind::Graph)
    );
    assert!(rig.shell.app.graph().is_open());
    // The shortcut action is the shell's own business: it toggles the layout.
    rig.app_msg(AppMessage::Action(Action::ToggleGraphPane));
    assert_eq!(
        rig.shell.app.graph().is_open(),
        rig.shell.ui.is_open(PaneKind::Graph)
    );
}
