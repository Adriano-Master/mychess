//! Tests of the opening tree panel: bar geometry, texts of the states, the
//! table in the simulator, clicks and the pane in the shell (spec M5-2).
#![allow(clippy::float_cmp)]

use iced::{Settings, Size};
use iced_test::Simulator;
use mychess_app::{Action, BarSegments, TreeBody, TreeFastMode, TreeSort, bar_segments};
use mychess_core::Color;

use super::*;
use crate::i18n::Language;

fn en() -> I18n {
    I18n::new(Language::EnUs)
}

fn pt() -> I18n {
    I18n::new(Language::PtBr)
}

fn row_view(index: usize, san: &str) -> TreeRowView {
    TreeRowView {
        mask: None,
        index,
        san: san.to_owned(),
        hidden: false,
        is_end: false,
        clickable: true,
        is_next_move: false,
        games: 1234,
        share: (45, 3),
        results: [500, 300, 400, 34],
        score: (54, 7),
        draw_percent: 37,
        avg_elo: Some(2511),
        perf: Some(2600),
        avg_year: Some(2008),
        eco: Some("B20".to_owned()),
        bar: bar_segments(547, 37),
    }
}

fn view_of(rows: Vec<TreeRowView>) -> TreeView {
    TreeView {
        mask: None,
        body: TreeBody::Rows,
        total: Some(TreeTotalView {
            games: 2000,
            results: [800, 500, 650, 50],
            score: (51, 2),
            draw_percent: 32,
            avg_elo: Some(2400),
            perf: None,
            avg_year: Some(2005),
        }),
        rows,
        turn: Color::White,
        short: true,
        sort: TreeSort::Frequency,
        fast_mode: TreeFastMode::Slow,
        autorefresh: true,
        adjust: false,
        searching: false,
        unreadable: 0,
        from_cache: false,
        searched: 0,
        elapsed_ms: 0,
        training: false,
        training_available: true,
        locked: false,
        target_name: Some("games".to_owned()),
        target_is_clipbase: false,
        best: None,
        cache: mychess_app::CacheView::default(),
    }
}

fn sim<'a>(view: &'a TreeView, i18n: &I18n) -> Simulator<'a, Message> {
    Simulator::with_size(
        Settings::default(),
        Size::new(900.0, 500.0),
        content_with(view, i18n, (false, false)),
    )
}

#[test]
fn the_bar_has_a_grey_base_then_the_white_and_black_parts() {
    let parts = bar_parts(bar_segments(547, 37));
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], (Part::Grey, 0.0, BAR_WIDTH));
    assert_eq!(parts[1].0, Part::White);
    assert_eq!(parts[2].0, Part::Black);
    assert!(parts[1].2 < parts[2].1, "grey stays between");
    // All wins for White: no black part; the white one fills the bar.
    let all_white = bar_parts(bar_segments(1000, 0));
    assert_eq!(all_white.len(), 2);
    assert_eq!(all_white[1], (Part::White, 0.0, BAR_WIDTH));
    // Parts stay inside the bar whatever the segments say.
    let wild = bar_parts(BarSegments {
        white_end: Some(500.0),
        black_start: Some(-4.0),
    });
    assert!(wild.iter().all(|p| p.1 >= 0.0 && p.2 <= BAR_WIDTH));
}

#[test]
fn percentages_use_the_decimal_separator_of_the_language() {
    assert_eq!(percent_text(&en(), (45, 3)), "45.3%");
    assert_eq!(percent_text(&pt(), (45, 3)), "45,3%");
    assert_eq!(percent_text(&en(), (100, 0)), "100.0%");
}

#[test]
fn every_state_has_a_text_and_the_table_has_none() {
    let i18n = en();
    assert_eq!(body_message(&i18n, &TreeBody::Rows), None);
    assert_eq!(
        body_message(&i18n, &TreeBody::Unavailable).as_deref(),
        Some("Base not available")
    );
    assert_eq!(
        body_message(&i18n, &TreeBody::Empty).as_deref(),
        Some("No game reaches this position")
    );
    let read = body_message(
        &i18n,
        &TreeBody::Issue(TreeIssue::Read("disk gone".to_owned())),
    );
    assert_eq!(
        read.as_deref(),
        Some("The database could not be read: disk gone")
    );
    for issue in [
        TreeIssue::Cancelled,
        TreeIssue::TooManyMoves,
        TreeIssue::UnsupportedPosition,
        TreeIssue::FilterSize,
        TreeIssue::Internal,
    ] {
        let text = body_message(&i18n, &TreeBody::Issue(issue.clone())).expect("text");
        assert!(!text.is_empty() && !text.starts_with("tree-"), "{issue:?}");
    }
}

#[test]
fn the_short_table_hides_rating_performance_year_and_eco() {
    let i18n = en();
    let mut view = view_of(vec![row_view(0, "e4")]);
    let mut short = sim(&view, &i18n);
    assert!(short.find("e4").is_ok());
    assert!(short.find("Move").is_ok());
    assert!(short.find("TOTAL").is_ok());
    assert!(short.find("54.7%").is_ok());
    assert!(short.find("37%").is_ok());
    assert!(short.find("ECO").is_err());
    assert!(short.find("B20").is_err());
    assert!(short.find("2511").is_err());
    drop(short);
    view.short = false;
    let mut full = sim(&view, &i18n);
    assert!(full.find("ECO").is_ok());
    assert!(full.find("B20").is_ok());
    assert!(full.find("2511").is_ok());
    assert!(full.find("2008").is_ok());
}

#[test]
fn a_hidden_row_draws_no_move_and_no_eco() {
    let i18n = en();
    let mut hidden = row_view(0, "Nf3");
    hidden.hidden = true;
    hidden.clickable = false;
    let mut view = view_of(vec![hidden]);
    view.short = false;
    let mut s = sim(&view, &i18n);
    assert!(s.find("Nf3").is_err());
    assert!(s.find("B20").is_err());
    assert!(s.find("1234").is_ok());
}

#[test]
fn a_click_on_a_clickable_row_sends_the_index_and_others_send_nothing() {
    let i18n = en();
    let mut dead = row_view(1, "[end]");
    dead.clickable = false;
    dead.is_end = true;
    let view = view_of(vec![row_view(0, "e4"), dead]);
    let mut s = sim(&view, &i18n);
    s.click("e4").expect("row");
    s.click("[end]").expect("text found");
    let sent: Vec<Message> = s.into_messages().collect();
    assert!(
        matches!(
            sent.as_slice(),
            [Message::App(AppMessage::Tree(TreeMessage::RowClicked(0)))]
        ),
        "{sent:?}"
    );
}

#[test]
fn the_options_send_their_messages() {
    let i18n = en();
    let view = view_of(vec![row_view(0, "e4")]);
    let mut s = sim(&view, &i18n);
    s.click("Refresh").expect("box");
    s.click("Adjust filter").expect("box");
    s.click("Short display").expect("box");
    s.click(Icon::Close.file_stem()).ok();
    let sent: Vec<Message> = s.into_messages().collect();
    assert!(matches!(
        sent.as_slice(),
        [
            Message::App(AppMessage::Tree(TreeMessage::ToggleAutoRefresh)),
            Message::App(AppMessage::Tree(TreeMessage::SetAdjust(true))),
            Message::App(AppMessage::Tree(TreeMessage::SetShort(false))),
            ..
        ]
    ));
}

#[test]
fn the_unreadable_warning_and_the_empty_text_show() {
    let i18n = en();
    let mut view = view_of(Vec::new());
    view.body = TreeBody::Empty;
    view.total = None;
    view.unreadable = 3;
    let mut s = sim(&view, &i18n);
    assert!(s.find("No game reaches this position").is_ok());
    assert!(s.find("3 games could not be read").is_ok());
    assert!(s.find("Move").is_err(), "no table");
}

#[test]
fn searching_keeps_the_table_and_shows_the_indicator() {
    let i18n = en();
    let mut view = view_of(vec![row_view(0, "e4")]);
    view.searching = true;
    view.rows[0].clickable = false;
    let mut s = sim(&view, &i18n);
    assert!(s.find("e4").is_ok());
    assert!(s.find("Searching...").is_ok());
}

#[test]
fn the_pane_opens_with_the_action_tells_the_app_and_closes() {
    let mut shell = Shell::new(None);
    shell.i18n = en();
    assert!(!shell.ui.is_open(PaneKind::Tree) && !shell.app.tree().is_open());
    drop(shell.update(Message::Ui(UiMessage::Action(Action::ToggleTreePane))));
    assert!(shell.ui.is_open(PaneKind::Tree) && shell.app.tree().is_open());
    assert!(shell.ui.layout().panes().contains(&PaneKind::Tree));
    assert!(shell.ui.layout().is_valid());
    // No base with games: the panel says so.
    assert_eq!(shell.app.tree_view().body, TreeBody::Unavailable);
    let mut s = Simulator::with_size(Settings::default(), Size::new(900.0, 500.0), view(&shell));
    assert!(s.find("Base not available").is_ok());
    drop(s);
    drop(shell.update(Message::Ui(UiMessage::Action(Action::ToggleTreePane))));
    assert!(!shell.ui.is_open(PaneKind::Tree) && !shell.app.tree().is_open());
}

#[test]
fn the_pane_survives_other_panes_and_goes_with_the_reset() {
    let mut shell = Shell::new(None);
    drop(shell.update(Message::Ui(UiMessage::TogglePane(PaneKind::Tree))));
    for kind in [PaneKind::Info, PaneKind::Analysis, PaneKind::Graph] {
        drop(shell.update(Message::Ui(UiMessage::TogglePane(kind))));
        assert!(
            shell.ui.layout().panes().contains(&PaneKind::Tree),
            "{kind:?}"
        );
        assert!(shell.ui.layout().is_valid());
    }
    assert!(shell.app.tree().is_open());
    drop(shell.update(Message::Ui(UiMessage::ResetLayout)));
    assert!(!shell.ui.is_open(PaneKind::Tree));
    assert!(!shell.app.tree().is_open());
}

#[test]
fn the_close_button_of_the_panel_toggles_the_pane_in_the_shell() {
    let mut shell = Shell::new(None);
    shell.i18n = en();
    drop(shell.update(Message::Ui(UiMessage::TogglePane(PaneKind::Tree))));
    drop(shell.update(Message::Ui(UiMessage::TogglePane(PaneKind::Tree))));
    assert!(!shell.app.tree().is_open());
}

#[test]
fn the_menu_entries_exist_and_show_the_shortcut() {
    let shell = Shell::new(None);
    let specs = crate::windows::menu_bar::menu_specs(&shell.app, &shell.ui, shell.options);
    let tools = specs
        .iter()
        .find(|s| s.label_key == "menu-tools")
        .expect("tools");
    let found = tools.entries.iter().any(|e| {
        matches!(
            e,
            crate::windows::menu_bar::MenuEntry::Item {
                label: crate::windows::menu_bar::Label::Key("menu-tools-tree"),
                shortcut: Some(_),
                ..
            }
        )
    });
    assert!(found);
}

#[test]
fn contract_bar_parts_for_zero_even_and_all_draw_scores() {
    // 0 % and no draws: Black owns the bar (D969); the white part is the one
    // unit of the Scid offset.
    let zero = bar_parts(bar_segments(0, 0));
    assert_eq!(zero.last().copied(), Some((Part::Black, 1.0, BAR_WIDTH)));
    assert_eq!(zero[1], (Part::White, 0.0, 1.0));
    // 50 % with no draws: White and Black split the bar, no grey between.
    let even = bar_parts(bar_segments(500, 0));
    assert_eq!(even.len(), 3);
    assert_eq!(even[1].2, even[2].1, "a tie leaves no grey");
    assert!((even[1].2 - 31.0).abs() < 1e-3, "{even:?}");
    // 50 % with only draws: the bar is grey except the one-unit offset.
    let drawn = bar_parts(bar_segments(500, 100));
    assert!(
        drawn.iter().all(|p| p.0 != Part::Black),
        "no black part: {drawn:?}"
    );
    let white = drawn.iter().find(|p| p.0 == Part::White).expect("white");
    assert!(white.2 <= 1.0 + 1e-3, "{drawn:?}");
    // Mirror: the white part of s equals the black width of 1000 - s.
    for score in [100_u32, 250, 333, 500, 667, 900] {
        let a = bar_parts(bar_segments(score, 20));
        let b = bar_parts(bar_segments(1000 - score, 20));
        let white = a.iter().find(|p| p.0 == Part::White).map_or(0.0, |p| p.2);
        let black = b
            .iter()
            .find(|p| p.0 == Part::Black)
            .map_or(0.0, |p| BAR_WIDTH - p.1);
        assert!(
            ((white - 1.0).max(0.0) - black).abs() < 0.2,
            "{score}: {white} vs {black}"
        );
    }
}
