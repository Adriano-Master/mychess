//! Contract of the game list panel (spec M3-4, slice 4b: rules 4, 5, 12,
//! decisions D536 to D542, acceptance criteria on the panel, tabs and list).
//! Derived from the spec and from the Scid (`tcl/windows/gamelist.tcl`
//! `glistFields`, `switcher.tcl`), headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::float_cmp,
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::redundant_closure_for_method_calls
)]

mod m3_4b_common;
mod render_common;

use iced::{Size, widget};
use m3_4b_common::*;
use mychess_app::{
    BasesMessage, DiscardChoice, GameMessage, LanguagePref, Message as AppMessage, PlayOptions,
};
use mychess_core::GameResult;
use mychess_gui::Message;
use mychess_gui::theme::{self, RowLook};
use mychess_gui::windows::game_list::{
    Column, GameListMessage, columns_for_width, max_first, visible_rows,
};
use mychess_gui::windows::layout::PaneKind;
use render_common::{SCALE, render};
use rstest::rstest;

// ---- virtualization ----

#[test]
fn rows_built_cost_the_same_for_ten_and_fifty_thousand_games() {
    let dir = tempfile::tempdir().expect("dir");
    let mut small = Rig::new();
    open(&mut small, &numbered(dir.path(), "s.pgn", "S", 30));
    viewport(&mut small, 900.0, 240.0);
    let ids = || (1..=200).chain(24_990..=25_010).chain(49_900..=50_000);
    let few = rows_built(&small, ids());
    assert_eq!(
        few.len(),
        10,
        "ten visible rows of 24 px in 240 px: {few:?}"
    );

    let mut big = Rig::new();
    open(&mut big, &numbered(dir.path(), "b.pgn", "B", 50_000));
    assert_eq!(
        big.shell.app.bases.current_slot().map(|s| s.total()),
        Some(50_000)
    );
    viewport(&mut big, 900.0, 240.0);
    let many = rows_built(&big, ids());
    assert_eq!(many.len(), 10, "same widgets for 50 000 games: {many:?}");
    assert_eq!(many, (1..=10).collect::<Vec<_>>());
}

#[rstest]
#[case::zero(0.0)]
#[case::one_pixel(1.0)]
#[case::less_than_a_row(23.0)]
#[case::negative(-40.0)]
#[case::nan(f32::NAN)]
#[case::infinite(f32::INFINITY)]
fn a_degenerate_viewport_height_still_shows_exactly_one_row_or_a_bounded_list(#[case] h: f32) {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 60));
    viewport(&mut rig, 900.0, h);
    let built = rows_built(&rig, 1..=60);
    if h.is_finite() {
        assert_eq!(built, vec![1], "at least one row, never more than fits");
    } else {
        assert!(!built.is_empty(), "no panic and something to show");
    }
    // The paging keys still move with a one row window.
    assert!(visible_rows(h) >= 1);
}

#[rstest]
#[case::zero(0.0)]
#[case::nan(f32::NAN)]
#[case::huge(1.0e9)]
fn a_degenerate_viewport_width_does_not_break_the_panel(#[case] w: f32) {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 20));
    viewport(&mut rig, w, 240.0);
    assert!(has_id(&rig, "games-header"));
    assert!(!rows_built(&rig, 1..=20).is_empty());
}

#[test]
fn an_enormous_viewport_shows_every_game_of_a_short_list_and_no_phantom_row() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 40));
    viewport(&mut rig, 5000.0, 1.0e6);
    assert_eq!(rows_built(&rig, 1..=45), (1..=40).collect::<Vec<_>>());
}

#[test]
fn an_empty_database_says_so_in_both_languages() {
    for (language, text) in [
        (LanguagePref::EnUs, "This database has no games"),
        (LanguagePref::PtBr, "Este banco não tem partidas"),
    ] {
        let mut rig = Rig::with_language(language);
        // The Clipbase is empty and the only database.
        rig.msg(Message::Ui(mychess_gui::UiMessage::TogglePane(
            PaneKind::GameList,
        )));
        assert!(has_text(&rig, text), "{text}");
        assert!(rows_built(&rig, 1..=5).is_empty());
    }
}

#[rstest]
#[case::far(10_000)]
#[case::max(u32::MAX)]
#[case::zero(0)]
fn a_list_position_outside_the_range_is_brought_back_and_never_blanks_the_list(#[case] first: u32) {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 100));
    viewport(&mut rig, 900.0, 240.0);
    let id = rig.shell.app.bases.current();
    rig.app_msg(AppMessage::Bases(BasesMessage::SetListFirst {
        slot: id,
        first,
    }));
    let at = current_first(&rig);
    assert!(
        (1..=max_first(100, 10)).contains(&at),
        "first={first} left the list at {at}"
    );
    assert!(!rows_built(&rig, 1..=100).is_empty());
    assert!(!has_text(&rig, "No games in the filter"));
}

// ---- header and columns ----

#[test]
fn the_header_names_the_scid_columns_and_the_elo_ones_need_760_px() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 20));
    viewport(&mut rig, 900.0, 240.0);
    for label in [
        "Nº", "White", "Black", "Result", "Date", "Event", "ECO", "Moves", "WElo", "BElo",
    ] {
        assert!(has_text(&rig, label), "header {label} at 900 px");
    }
    viewport(&mut rig, 759.0, 240.0);
    for label in [
        "Nº", "White", "Black", "Result", "Date", "Event", "ECO", "Moves",
    ] {
        assert!(has_text(&rig, label), "header {label} at 759 px");
    }
    assert!(!has_text(&rig, "WElo") && !has_text(&rig, "BElo"));
    viewport(&mut rig, 760.0, 240.0);
    assert!(has_text(&rig, "WElo") && has_text(&rig, "BElo"));
    assert!(columns_for_width(759.9).len() < columns_for_width(760.0).len());
}

#[test]
fn the_header_is_translated_to_portuguese() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::with_language(LanguagePref::PtBr);
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 20));
    viewport(&mut rig, 900.0, 240.0);
    for label in [
        "Brancas",
        "Pretas",
        "Resultado",
        "Data",
        "Evento",
        "Lances",
        "Elo B",
        "Elo P",
    ] {
        assert!(has_text(&rig, label), "{label}");
    }
    assert!(!has_text(&rig, "White"));
}

#[test]
fn the_first_columns_follow_the_scid_order_with_its_widths_and_alignments() {
    use iced::alignment::Horizontal::{Left, Right};
    let wide = columns_for_width(1000.0);
    assert_eq!(
        wide[..4],
        [Column::Number, Column::White, Column::Black, Column::Result]
    );
    // Scid glistFields: code, anchor (e = right, w = left), width in characters.
    let scid = [
        (Column::Number, Right, 7),
        (Column::White, Left, 14),
        (Column::Black, Left, 14),
        (Column::Result, Right, 5),
        (Column::Length, Right, 5),
        (Column::Date, Left, 10),
        (Column::Event, Left, 10),
        (Column::WhiteElo, Right, 5),
        (Column::BlackElo, Right, 5),
        (Column::Eco, Right, 5),
    ];
    for (column, align, chars) in scid {
        assert_eq!(column.align(), align, "{column:?}");
        assert_eq!(column.chars(), chars, "{column:?}");
        assert!(wide.contains(&column), "{column:?} is shown");
    }
}

#[test]
fn cells_print_the_fields_as_the_scid_list_does() {
    let dir = tempfile::tempdir().expect("dir");
    let games = [
        G::new("Capablanca, J", "Alekhine, A", "1-0")
            .tag("Date", "1927.09.21")
            .tag("ECO", "D30")
            .tag("WhiteElo", "2725"),
        G::new("B", "W", "0-1"),
        G::new("D", "E", "1/2-1/2"),
        G::new("F", "G", "*"),
    ];
    let mut rig = Rig::new();
    open(&mut rig, &write_games(dir.path(), "a.pgn", &games));
    let id = rig.shell.app.bases.current();
    let rows = rig.shell.app.bases.list_page(id, 1, 4);
    assert_eq!(rows.len(), 4);
    let text = |c: Column, i: usize| c.cell(&rows[i]);
    assert_eq!(text(Column::Number, 2), "3");
    assert_eq!(text(Column::White, 0), "Capablanca, J");
    assert_eq!(text(Column::Black, 0), "Alekhine, A");
    assert_eq!(text(Column::Date, 0), "1927.09.21");
    assert_eq!(text(Column::Eco, 0), "D30");
    assert_eq!(text(Column::WhiteElo, 0), "2725");
    assert_eq!(text(Column::BlackElo, 0), "", "no Elo: blank, not 0");
    assert_eq!(text(Column::Length, 0), "2", "4 plies are 2 moves");
    let results: Vec<String> = (0..4).map(|i| text(Column::Result, i)).collect();
    assert_eq!(results, ["1-0", "0-1", "=-=", "*"]);
    assert_eq!(rows[3].result, GameResult::Unknown);
    // The strings reach the screen.
    let rig_text = |t: &str| has_text(&rig, t);
    assert!(rig_text("Capablanca, J") && rig_text("=-=") && rig_text("1927.09.21"));
}

// ---- how a row is painted ----

#[test]
fn selected_current_and_zebra_rows_are_painted_differently() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 8));
    let id = rig.shell.app.bases.current();
    rig.send(BasesMessage::LoadGame {
        slot: id,
        number: 3,
    });
    rig.send(BasesMessage::SelectRow {
        slot: id,
        number: Some(4),
    });
    assert_eq!(
        rig.shell.app.bases.current_slot().unwrap().current_game,
        Some(3)
    );
    let size = Size::new(1280.0, 800.0);
    let shot = render(rig.shell.view(), size);
    let mut sim = rig.sim(size);
    let mut probe = |n: u32| {
        let target = sim
            .find(widget::Id::from(format!("games-row-{n}")))
            .expect("row");
        let b = target.bounds();
        shot.pixel(
            ((b.x + 2.0) * SCALE) as u32,
            ((b.y + b.height / 2.0) * SCALE) as u32,
        )
    };
    let (plain, zebra, current, selected, plain2) =
        (probe(1), probe(2), probe(3), probe(4), probe(5));
    assert_eq!(plain, plain2, "rows 1 and 5 are both plain");
    assert_ne!(zebra, plain, "zebra");
    assert_ne!(current, plain, "current game");
    assert_ne!(current, zebra);
    assert_ne!(selected, plain, "selection");
    assert_ne!(selected, zebra);
    assert_ne!(selected, current);
}

#[test]
fn a_deleted_or_unreadable_row_dims_its_text() {
    let theme = iced::Theme::Light;
    let normal = theme::list_row(RowLook::default())(&theme);
    let dimmed = theme::list_row(RowLook {
        dimmed: true,
        ..RowLook::default()
    })(&theme);
    assert_ne!(normal.text_color, dimmed.text_color);
    // Dimming keeps the other highlights.
    let both = theme::list_row(RowLook {
        dimmed: true,
        selected: true,
        ..RowLook::default()
    })(&theme);
    assert!(both.background.is_some());
    assert_eq!(both.text_color, dimmed.text_color);
}

// ---- clicks ----

#[test]
fn a_single_click_only_selects_and_a_double_click_loads_that_game_of_that_slot() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 10));
    open(&mut rig, &numbered(dir.path(), "b.pgn", "B", 10));
    rig.app_msg(AppMessage::Bases(BasesMessage::Select(slot(1))));
    let before = rig.game().tags().white.clone();

    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    sim.click(widget::Id::new("games-row-4")).expect("row 4");
    let single: Vec<Message> = sim.into_messages().collect();
    assert!(
        single
            .iter()
            .all(|m| !matches!(m, Message::Games(GameListMessage::Open { .. }))),
        "{single:?}"
    );
    for m in single {
        if let Message::Games(m) = m {
            list(&mut rig, m);
        }
    }
    assert_eq!(selected(&rig), Some(4));
    assert_eq!(rig.game().tags().white, before, "a click loads nothing");

    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    sim.click(widget::Id::new("games-row-6")).expect("row 6");
    sim.click(widget::Id::new("games-row-6")).expect("again");
    let double: Vec<Message> = sim.into_messages().collect();
    let open_msg = double
        .into_iter()
        .find_map(|m| match m {
            Message::Games(m @ GameListMessage::Open { .. }) => Some(m),
            _ => None,
        })
        .expect("an Open message");
    assert_eq!(
        open_msg,
        GameListMessage::Open {
            slot: slot(1),
            number: 6
        }
    );
    list(&mut rig, open_msg);
    assert_eq!(rig.game().tags().white, "A 6");
    assert_eq!(selected(&rig), Some(6));
    assert_eq!(
        rig.shell.app.bases.current_slot().unwrap().current_game,
        Some(6)
    );
}

fn play_e5(rig: &mut Rig) {
    rig.send(GameMessage::Navigate(mychess_app::Nav::GameEnd));
    let mv = rig
        .shell
        .app
        .session
        .position()
        .parse_uci("e7e5")
        .expect("e7e5 is legal after 1.e4");
    rig.send(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
}

#[test]
fn a_double_click_over_an_altered_game_asks_before_discarding() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 10));
    let id = rig.shell.app.bases.current();
    list(
        &mut rig,
        GameListMessage::Open {
            slot: id,
            number: 2,
        },
    );
    assert_eq!(rig.game().tags().white, "A 2");
    play_e5(&mut rig);
    assert!(rig.shell.app.session.is_dirty());
    let plies = rig.game().mainline().count();

    list(
        &mut rig,
        GameListMessage::Open {
            slot: id,
            number: 5,
        },
    );
    assert!(rig.shell.app.discard_prompt.is_some(), "must ask");
    assert_eq!(rig.game().tags().white, "A 2", "not replaced yet");
    rig.send(AppMessage::AnswerDiscard(DiscardChoice::Cancel));
    assert_eq!(rig.game().tags().white, "A 2");
    assert_eq!(rig.game().mainline().count(), plies, "the edit survives");
    assert!(rig.shell.app.session.is_dirty());

    list(
        &mut rig,
        GameListMessage::Open {
            slot: id,
            number: 5,
        },
    );
    rig.send(AppMessage::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(rig.game().tags().white, "A 5");
    assert!(!rig.shell.app.session.is_dirty());
}

#[test]
fn a_click_on_a_game_that_is_not_in_the_database_is_harmless() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 5));
    let id = rig.shell.app.bases.current();
    list(
        &mut rig,
        GameListMessage::Open {
            slot: id,
            number: 999,
        },
    );
    list(
        &mut rig,
        GameListMessage::Open {
            slot: id,
            number: 0,
        },
    );
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(8),
            number: 1,
        },
    );
    assert!(has_id(&rig, "games-pane"));
    assert!(!rig.shell.app.session.is_dirty());
}

// ---- tabs ----

#[test]
fn tabs_show_the_counter_the_read_only_mark_and_the_clipbase() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 12));
    assert!(has_text(&rig, "a.pgn"));
    assert!(has_text(&rig, "12/12"), "filtered/total");
    assert!(has_text(&rig, "read-only"));
    assert!(has_text(&rig, "Clipbase") && has_text(&rig, "0/0"));
    assert!(has_id(&rig, "games-tab-1") && has_id(&rig, "games-tab-9"));
    assert!(!has_id(&rig, "games-tab-2"));
    // The Clipbase tab comes last (D538).
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    let first = sim.find(widget::Id::new("games-tab-1")).unwrap().bounds();
    let clip = sim.find(widget::Id::new("games-tab-9")).unwrap().bounds();
    assert!(clip.x > first.x, "slot 1 before the Clipbase");
}

#[test]
fn the_tabs_are_translated() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::with_language(LanguagePref::PtBr);
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 12));
    assert!(has_text(&rig, "somente leitura"));
    assert!(!has_text(&rig, "read-only"));
}

#[test]
fn the_close_button_of_the_clipbase_tab_sends_close_for_slot_nine() {
    let mut rig = Rig::new();
    rig.msg(Message::Ui(mychess_gui::UiMessage::TogglePane(
        PaneKind::GameList,
    )));
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    sim.click("×").expect("the x button");
    let got: Vec<Message> = sim.into_messages().collect();
    assert!(
        got.iter().any(|m| matches!(
            m,
            Message::App(AppMessage::Bases(BasesMessage::Close(s))) if s.is_clipbase()
        )),
        "{got:?}"
    );
}

#[test]
fn clicking_a_tab_and_closing_one_work_and_closing_the_current_goes_to_the_clipbase() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 12));
    open(&mut rig, &numbered(dir.path(), "b.pgn", "B", 12));
    rig.app_msg(AppMessage::Bases(BasesMessage::Close(slot(1))));
    assert_eq!(rig.shell.app.bases.current(), slot(2), "another tab closed");
    assert!(!has_id(&rig, "games-tab-1") && has_id(&rig, "games-tab-2"));
    rig.app_msg(AppMessage::Bases(BasesMessage::Close(slot(2))));
    assert!(rig.shell.app.bases.current().is_clipbase());
    assert!(!has_id(&rig, "games-tab-2"));
}

#[test]
fn the_same_file_twice_activates_the_existing_tab_and_nine_positions_render() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let first = numbered(dir.path(), "f1.pgn", "F1", 12);
    open(&mut rig, &first);
    open(&mut rig, &numbered(dir.path(), "f2.pgn", "F2", 12));
    assert_eq!(rig.shell.app.bases.current(), slot(2));
    open(&mut rig, &first);
    assert_eq!(rig.shell.app.bases.current(), slot(1), "existing tab");
    assert!(!has_id(&rig, "games-tab-3"), "no third tab");
    for n in 3..=8 {
        open(
            &mut rig,
            &numbered(dir.path(), &format!("f{n}.pgn"), "X", 12),
        );
    }
    for n in 1..=9 {
        let id: &'static str = Box::leak(format!("games-tab-{n}").into_boxed_str());
        assert!(has_id(&rig, id), "tab {n}");
    }
    // A ninth file finds no free slot: the notice says so, nothing opens.
    open(&mut rig, &numbered(dir.path(), "f9.pgn", "Y", 12));
    assert!(has_text(
        &rig,
        "Too many databases are open; close one first"
    ));
    assert!(!has_id(&rig, "games-tab-10"));
    assert!(has_id(&rig, "games-pane"));
}

#[test]
fn a_long_file_name_is_cut_in_its_tab() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let name = "a_really_quite_long_database_name.pgn";
    open(&mut rig, &numbered(dir.path(), name, "A", 12));
    assert!(!has_text(&rig, name), "the full name is only the tooltip");
    assert!(has_text(&rig, "a_really_quite_lo…"), "18 chars at most");
}

// ---- opening ----

#[test]
fn a_one_game_file_opens_no_panel_and_a_multi_game_file_after_it_does() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    open(&mut rig, &numbered(dir.path(), "one.pgn", "One", 1));
    assert!(!rig.shell.ui.is_open(PaneKind::GameList));
    assert!(!has_id(&rig, "games-pane"));
    open(&mut rig, &numbered(dir.path(), "many.pgn", "Many", 5));
    assert!(rig.shell.ui.is_open(PaneKind::GameList));
    assert!(has_id(&rig, "games-pane"));
}

#[test]
fn a_panel_the_user_closed_stays_closed_when_another_tab_is_selected() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 12));
    rig.msg(Message::Ui(mychess_gui::UiMessage::TogglePane(
        PaneKind::GameList,
    )));
    assert!(!rig.shell.ui.is_open(PaneKind::GameList));
    rig.app_msg(AppMessage::Bases(BasesMessage::Select(
        mychess_app::SlotId::CLIPBASE,
    )));
    assert!(
        !rig.shell.ui.is_open(PaneKind::GameList),
        "only a new open reveals the panel"
    );
}

// ---- end to end, as the user does it ----

#[test]
fn two_databases_keep_their_own_list_selection_and_game() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let cap: Vec<G> = (1..=30)
        .map(|n| G::new(&format!("Capablanca {n}"), "Opp", "1-0"))
        .collect();
    let car: Vec<G> = (1..=25)
        .map(|n| G::new(&format!("Carlsen {n}"), "Opp", "0-1"))
        .collect();
    open(&mut rig, &write_games(dir.path(), "capablanca.pgn", &cap));
    open(&mut rig, &write_games(dir.path(), "carlsen.pgn", &car));
    viewport(&mut rig, 900.0, 240.0);
    assert!(has_id(&rig, "games-tab-1") && has_id(&rig, "games-tab-2"));
    assert!(has_text(&rig, "Carlsen 1"), "the newest tab is shown");
    assert!(!has_text(&rig, "Capablanca 1"));

    // Carlsen: scroll 7 rows and load game 9.
    let carlsen = rig.shell.app.bases.current();
    list(
        &mut rig,
        GameListMessage::Scroll {
            slot: carlsen,
            rows: 7,
        },
    );
    list(
        &mut rig,
        GameListMessage::Open {
            slot: carlsen,
            number: 9,
        },
    );
    assert_eq!(rig.game().tags().white, "Carlsen 9");

    // Capablanca: another tab, other rows; scroll 3 and load game 5.
    rig.app_msg(AppMessage::Bases(BasesMessage::Select(slot(1))));
    assert!(has_text(&rig, "Capablanca 1") && !has_text(&rig, "Carlsen 9"));
    let capablanca = rig.shell.app.bases.current();
    list(
        &mut rig,
        GameListMessage::Scroll {
            slot: capablanca,
            rows: 3,
        },
    );
    list(
        &mut rig,
        GameListMessage::Open {
            slot: capablanca,
            number: 5,
        },
    );
    assert_eq!(rig.game().tags().white, "Capablanca 5");

    // Back to Carlsen: list position, selection and game are as left.
    rig.app_msg(AppMessage::Bases(BasesMessage::Select(slot(2))));
    assert_eq!(current_first(&rig), 8);
    assert_eq!(selected(&rig), Some(9));
    assert!(has_text(&rig, "Carlsen 9") && !has_text(&rig, "Capablanca 5"));
    assert_eq!(rig.game().tags().white, "Carlsen 9");
    // And Capablanca likewise.
    rig.app_msg(AppMessage::Bases(BasesMessage::Select(slot(1))));
    assert_eq!(current_first(&rig), 4);
    assert_eq!(selected(&rig), Some(5));
    assert_eq!(rig.game().tags().white, "Capablanca 5");
    assert!(has_text(&rig, "Capablanca 5"));
}
