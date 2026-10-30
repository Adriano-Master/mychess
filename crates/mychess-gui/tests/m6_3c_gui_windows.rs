//! M6-3c (GUI): the five windows of the players, the tournaments and the
//! spelling file open from the actions and the menus, show the views of the
//! application, turn clicks and keys into its messages and go away on close
//! (spec M6-3, criteria 40 to 44).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m6_3c_common;

use iced::Size;
use iced::widget::Id;
use iced_test::selector::Candidate;
use m6_3c_common::*;
use mychess_app::{
    Action, FinderMessage, NameKind, PlayerField, PlayerSortKey, SpellingMessage, TournamentField,
};
use mychess_gui::windows::dialog_ui::ActiveDialog;
use mychess_gui::windows::player_tools::finder_view as fv;
use mychess_gui::windows::player_tools::spell_view as sv;
use mychess_gui::windows::player_tools::{info_view as iv, text};

fn rows(tools: &Tools) -> Vec<String> {
    tools
        .app()
        .player_finder_view()
        .expect("open")
        .rows
        .iter()
        .map(|r| r.name.clone())
        .collect()
}

/// Some text on screen contains `needle`.
fn has_text(tools: &Tools, needle: &str) -> bool {
    let mut sim = tools.sim_default();
    sim.find(|c: Candidate<'_>| match c {
        Candidate::Text { content, .. } if content.contains(needle) => Some(()),
        _ => None,
    })
    .is_ok()
}

#[test]
fn the_actions_open_and_close_the_finders_and_the_information() {
    let mut t = Tools::new();
    assert!(!t.shell().modal_layer_open());
    t.action(Action::PlayerFinder);
    assert_eq!(t.top(), Some(ActiveDialog::PlayerFinder));
    assert!(t.shell().modal_layer_open());
    assert!(t.sim_default().find(Id::new("player-finder-box")).is_ok());
    t.action(Action::PlayerFinder);
    assert_eq!(t.top(), None);

    t.action(Action::TournamentFinder);
    assert_eq!(t.top(), Some(ActiveDialog::TournamentFinder));
    assert!(
        t.sim_default()
            .find(Id::new("tournament-finder-box"))
            .is_ok()
    );
    t.named(NamedKey::Escape);
    assert_eq!(t.top(), None);

    // With no player to repeat, the action only says so (rule 81).
    t.action(Action::PlayerInfo);
    assert_eq!(t.top(), None);
    t.rig.app_msg(mychess_app::PlayerInfoMessage::Open {
        name: "Alfa, A".into(),
    });
    assert_eq!(t.top(), Some(ActiveDialog::PlayerInfo));
    assert!(t.sim_default().find(Id::new("player-info-box")).is_ok());
    t.named(NamedKey::Escape);
    assert_eq!(t.top(), None);
}

#[test]
fn the_player_finder_lists_the_players_with_their_columns() {
    let mut t = Tools::new();
    t.open_base(&six());
    t.action(Action::PlayerFinder);
    assert_eq!(rows(&t), ["Alfa, A", "Beta, B", "Delta, D", "Gama, G"]);
    for header in ["Games", "Oldest", "Newest", "Elo", "Name"] {
        assert!(has_text(&t, header), "{header}");
    }
    let country = Id::new(fv::player_header_id(PlayerSortKey::Country));
    assert!(
        t.sim_default().find(country.clone()).is_err(),
        "no column without the country"
    );
    assert!(has_text(&t, "Alfa, A") && has_text(&t, "2520"));
    // The country column appears when asked for.
    t.finder(FinderMessage::SetPlayerField(
        PlayerField::Country,
        "yes".into(),
    ));
    t.finder(FinderMessage::UpdatePlayers);
    assert!(t.sim_default().find(country).is_ok());
    assert!(has_text(&t, "~~~"), "no spelling file: unknown country");
}

#[test]
fn a_header_orders_and_a_second_click_turns_the_order_round() {
    let mut t = Tools::new();
    t.open_base(&six());
    t.action(Action::PlayerFinder);
    let first = clicked(&t, fv::player_header_id(PlayerSortKey::Games));
    assert!(first.contains("SortPlayers(Games)"), "{first}");
    t.finder(FinderMessage::SortPlayers(PlayerSortKey::Games));
    assert_eq!(rows(&t)[0], "Alfa, A", "most games first");
    let descending = t.app().player_finder_view().unwrap().descending;
    t.finder(FinderMessage::SortPlayers(PlayerSortKey::Games));
    assert_eq!(rows(&t)[0], "Delta, D", "the order turned round");
    assert_ne!(t.app().player_finder_view().unwrap().descending, descending);
}

#[test]
fn the_fields_send_what_is_typed_and_enter_updates() {
    let mut t = Tools::new();
    t.open_base(&six());
    t.action(Action::PlayerFinder);
    let mut sim = t.sim_default();
    sim.click(Id::new(fv::player_field_id(PlayerField::Prefix)))
        .expect("field");
    let _ = sim.typewrite("be");
    let sent = format!("{:?}", sim.into_messages().collect::<Vec<_>>());
    assert!(sent.contains("SetPlayerField(Prefix"), "{sent}");
    // The application keeps the text; the button and Enter apply it.
    t.finder(FinderMessage::SetPlayerField(
        PlayerField::Prefix,
        "be".into(),
    ));
    let update = clicked(&t, fv::PLAYER_UPDATE_ID);
    assert!(update.contains("UpdatePlayers"), "{update}");
    t.named(NamedKey::Enter);
    assert_eq!(rows(&t), ["Beta, B"]);
    let defaults = clicked(&t, fv::PLAYER_DEFAULTS_ID);
    assert!(defaults.contains("DefaultsPlayers"), "{defaults}");
}

#[test]
fn a_row_opens_the_player_information_over_the_finder() {
    let mut t = Tools::new();
    t.open_base(&six());
    t.load_ssp();
    t.action(Action::PlayerFinder);
    let mut sim = t.sim_default();
    sim.click(fv::player_row_id(0)).expect("row");
    let sent = format!("{:?}", sim.into_messages().collect::<Vec<_>>());
    assert!(sent.contains("ClickPlayer(0)"), "{sent}");
    t.rig.msg(Message::PlayerTools(
        mychess_gui::windows::player_tools::PlayerToolsMessage::ClickPlayer(0),
    ));
    assert_eq!(t.top(), Some(ActiveDialog::PlayerInfo));
    let view = t.app().player_info_view().expect("open");
    assert_eq!(view.name, "Alfa, A");
    let stats = view.stats.expect("counted");
    assert_eq!(stats.games, 5);
    assert!(has_text(&t, "Alfa, A") && has_text(&t, "Player information"));
    assert!(
        has_text(&t, "Grandmaster"),
        "the title from the spelling file"
    );
    assert!(has_text(&t, "5 games"));
    assert!(has_text(&t, "80.00%"), "the score of the spec");
    assert!(has_text(&t, "FIDE profile"));
    // Esc closes the information first, then the finder.
    t.named(NamedKey::Escape);
    assert_eq!(t.top(), Some(ActiveDialog::PlayerFinder));
    t.named(NamedKey::Escape);
    assert_eq!(t.top(), None);
}

#[test]
fn the_numbers_of_the_information_set_the_filter() {
    let mut t = Tools::new();
    t.open_base(&six());
    t.rig.app_msg(mychess_app::PlayerInfoMessage::Open {
        name: "Alfa, A".into(),
    });
    assert_eq!(t.top(), Some(ActiveDialog::PlayerInfo));
    let mut sim = t.sim_default();
    sim.click("5 games").expect("the link");
    let sent = format!("{:?}", sim.into_messages().collect::<Vec<_>>());
    assert!(sent.contains("FilterLink(\"-faA\")"), "{sent}");
    let buttons = clicked(&t, iv::INFO_LOOKUP_ID);
    assert!(buttons.contains("LookupName"), "{buttons}");
    let buttons = clicked(&t, iv::INFO_TOURNAMENTS_ID);
    assert!(buttons.contains("OpenTournaments"), "{buttons}");
    let buttons = clicked(&t, iv::INFO_UPDATE_ID);
    assert!(buttons.contains("Refresh"), "{buttons}");
}

#[test]
fn the_information_without_a_spelling_file_says_so_and_a_missing_name_too() {
    let mut t = Tools::new();
    t.open_base(&six());
    t.rig.app_msg(mychess_app::PlayerInfoMessage::Open {
        name: "Alfa, A".into(),
    });
    assert!(has_text(
        &t,
        "No spelling file is loaded, so there is no title, country or biography."
    ));
    t.rig.app_msg(mychess_app::PlayerInfoMessage::Open {
        name: "Nobody".into(),
    });
    assert!(has_text(&t, "This name is not in the database."));
}

#[test]
fn the_tournament_finder_shows_the_winner_and_the_runner_up() {
    let mut t = Tools::new();
    t.open_base(&six());
    t.action(Action::TournamentFinder);
    // The defaults ask for 4 players and 10 games: nothing is found.
    assert!(has_text(&t, "No matching tournaments were found."));
    for (field, value) in [
        (TournamentField::PlayersMin, "1"),
        (TournamentField::GamesMin, "1"),
    ] {
        t.finder(FinderMessage::SetTournamentField(field, value.into()));
    }
    let update = clicked(&t, fv::TOURNAMENT_UPDATE_ID);
    assert!(update.contains("UpdateTournaments"), "{update}");
    t.finder(FinderMessage::UpdateTournaments);
    let view = t.app().tournament_finder_view().unwrap();
    let found = view.rows.expect("rows");
    assert_eq!(found.len(), 2);
    let (winner, second) = fv::winner_texts(&found[1]);
    assert_eq!(winner, "1. Alfa (2510)");
    assert_eq!(second, "2. Beta (2400)");
    for header in ["Date", "Players", "Winner", "Runner-up", "Site"] {
        assert!(has_text(&t, header), "{header}");
    }
    assert!(has_text(&t, "Rio BRA") && has_text(&t, "2020.01.10"));
    let sorted = clicked(
        &t,
        fv::tournament_header_id(mychess_app::TournamentSortKey::Games),
    );
    assert!(sorted.contains("SortTournaments(Games)"), "{sorted}");
}

#[test]
fn a_tournament_is_selected_and_then_acted_on() {
    let mut t = Tools::new();
    t.open_base(&six());
    t.action(Action::TournamentFinder);
    for (field, value) in [
        (TournamentField::PlayersMin, "1"),
        (TournamentField::GamesMin, "1"),
    ] {
        t.finder(FinderMessage::SetTournamentField(field, value.into()));
    }
    t.finder(FinderMessage::UpdateTournaments);
    // Nothing is selected: the actions are off.
    let off = clicked(&t, fv::TOURNAMENT_FILTER_ID);
    assert!(!off.contains("ShowInFilter"), "{off}");
    t.rig.msg(Message::PlayerTools(
        mychess_gui::windows::player_tools::PlayerToolsMessage::SelectTournament(0),
    ));
    let on = clicked(&t, fv::TOURNAMENT_FILTER_ID);
    assert!(on.contains("ShowInFilter"), "{on}");
    let load = clicked(&t, fv::TOURNAMENT_LOAD_ID);
    assert!(load.contains("LoadFirstGame"), "{load}");
    let table = clicked(&t, fv::TOURNAMENT_CROSSTABLE_ID);
    assert!(table.contains("OpenCrosstable"), "{table}");
    // Enter does the default action of the selected row.
    t.finder(FinderMessage::ClickTournament {
        index: 0,
        action: mychess_app::TournamentAction::ShowInFilter,
    });
    let slot = t.app().bases.current();
    assert_eq!(
        t.app()
            .bases
            .slot(slot)
            .unwrap()
            .filter()
            .iter_games()
            .count(),
        1
    );
}

#[test]
fn the_keys_move_the_cursor_of_the_list_and_enter_opens_the_row() {
    let mut t = Tools::new();
    t.open_base(&six());
    t.action(Action::PlayerFinder);
    t.named(NamedKey::Down);
    assert_eq!(t.shell().ui.dialogs.player_tools.players.cursor, Some(0));
    t.named(NamedKey::Down);
    assert_eq!(t.shell().ui.dialogs.player_tools.players.cursor, Some(1));
    t.named(NamedKey::End);
    assert_eq!(t.shell().ui.dialogs.player_tools.players.cursor, Some(3));
    t.named(NamedKey::Up);
    t.named(NamedKey::Home);
    assert_eq!(t.shell().ui.dialogs.player_tools.players.cursor, Some(0));
    t.named(NamedKey::Enter);
    assert_eq!(t.top(), Some(ActiveDialog::PlayerInfo));
    assert_eq!(t.app().player_info_view().unwrap().name, "Alfa, A");
}

#[test]
fn the_name_checker_lists_ticks_edits_and_applies_with_a_confirmation() {
    let mut t = Tools::new();
    let path = t.open_base(&misspelled());
    t.load_ssp();
    t.rig.app_msg(SpellingMessage::OpenCheck(NameKind::Player));
    assert_eq!(t.top(), Some(ActiveDialog::SpellCheck));
    let view = t.app().spell_view().expect("open");
    assert_eq!(view.total, 1, "Alfa, Alf is the only misspelled name");
    assert_eq!(view.rows[0].row.new, "Alfa, A");
    assert_eq!(view.rows[0].row.games, 2);
    assert!(has_text(&t, "Spell-check: player names"));
    assert!(has_text(&t, "1 correction found (player names)"));
    assert!(has_text(&t, "Alfa, Alf"));
    // The box ticks, the search and the options reach the application.
    let ambiguous = clicked(&t, sv::SPELL_AMBIGUOUS_ID);
    assert!(ambiguous.contains("SetAmbiguous(true)"), "{ambiguous}");
    let none = clicked(&t, sv::SPELL_SELECT_NONE_ID);
    assert!(none.contains("SelectAll(false)"), "{none}");
    // Make corrections asks first.
    let apply = clicked(&t, sv::SPELL_APPLY_ID);
    assert!(apply.contains("Apply"), "{apply}");
    t.rig.app_msg(SpellingMessage::Apply);
    assert!(t.app().spell_view().unwrap().confirm);
    assert!(has_text(
        &t,
        "Please confirm making the corrections. This cannot be undone."
    ));
    // Esc is "no".
    t.named(NamedKey::Escape);
    assert!(!t.app().spell_view().unwrap().confirm);
    assert_eq!(t.top(), Some(ActiveDialog::SpellCheck));
    t.rig.app_msg(SpellingMessage::Apply);
    let yes = clicked(&t, sv::SPELL_CONFIRM_YES_ID);
    assert!(yes.contains("ConfirmApply(true)"), "{yes}");
    // Enter says yes; the write runs as a list job.
    t.named(NamedKey::Enter);
    let view = t.app().spell_view().expect("still open");
    assert!(view.result.is_some(), "{view:?}");
    assert!(has_text(&t, "occurring 2 times in total"));
    let names = disk_white_names(&path);
    assert!(
        names
            .iter()
            .all(|n| n == "Alfa, A" || n == "Beta, B" || n == "Gama, G" || n == "Delta, D"),
        "{names:?}"
    );
    t.named(NamedKey::Escape);
    assert_eq!(t.top(), None);
}

fn disk_white_names(path: &std::path::Path) -> Vec<String> {
    let mut db = mychess_db::si4::Si4Database::open(path).expect("open");
    (1..=db.len())
        .map(|n| db.game(n).expect("game").tags().white.clone())
        .collect()
}

#[test]
fn the_name_checker_edit_of_a_new_name_reaches_the_application() {
    let mut t = Tools::new();
    t.open_base(&misspelled());
    t.load_ssp();
    t.rig.app_msg(SpellingMessage::OpenCheck(NameKind::Player));
    let mut sim = t.sim_default();
    sim.click(sv::new_name_id(0)).expect("the field");
    let _ = sim.typewrite("X");
    let sent = format!("{:?}", sim.into_messages().collect::<Vec<_>>());
    assert!(sent.contains("EditNew(0"), "{sent}");
}

#[test]
fn the_name_checker_of_a_base_that_cannot_change_is_read_only() {
    let mut t = Tools::new();
    let path = t.dir.path().join("plain.pgn");
    std::fs::write(&path, pgn(&misspelled())).expect("write");
    t.rig.app_msg(BasesMessage::Open(path));
    t.load_ssp();
    t.rig.app_msg(SpellingMessage::OpenCheck(NameKind::Player));
    assert!(!t.app().spell_view().unwrap().writable);
    assert!(has_text(&t, "This database cannot be changed."));
    let off = clicked(&t, sv::SPELL_CLOSE_ID);
    assert!(off.contains("CloseCheck"), "{off}");
    let apply = clicked(&t, sv::SPELL_APPLY_ID);
    assert!(!apply.contains("Apply"), "{apply}");
}

#[test]
fn add_elo_checks_confirms_and_reports() {
    let mut t = Tools::new();
    let path = t.open_base(&misspelled_with_exact_names());
    t.load_ssp();
    t.rig.app_msg(SpellingMessage::OpenAddElo);
    assert_eq!(t.top(), Some(ActiveDialog::AddElo));
    assert!(has_text(&t, "Add Elo ratings"));
    let plan = clicked(&t, sv::ELO_PLAN_ID);
    assert!(plan.contains("PlanElo"), "{plan}");
    // The scope and the overwrite box reach the application.
    let all = clicked(&t, sv::ELO_ALL_ID);
    assert!(all.contains("SetEloOnlyFilter(false)"), "{all}");
    t.rig.app_msg(SpellingMessage::SetEloOnlyFilter(false));
    t.rig.app_msg(SpellingMessage::PlanElo);
    let view = t.app().add_elo_view().unwrap();
    assert!(view.planned.is_some(), "{view:?}");
    assert!(has_text(&t, "ratings would be added to"));
    let add = clicked(&t, sv::ELO_ADD_ID);
    assert!(add.contains("ConfirmElo"), "{add}");
    t.named(NamedKey::Enter);
    assert!(t.app().add_elo_view().unwrap().result.is_some());
    assert!(has_text(&t, "Elo ratings were added"));
    let elos = disk_elos(&path);
    assert!(elos.iter().any(|(w, _)| *w == 2302), "{elos:?}");
    // Enter closes the report.
    t.named(NamedKey::Enter);
    assert_eq!(t.top(), None);
}

/// Alfa with the exact name, so the ratings of its line apply.
fn misspelled_with_exact_names() -> Vec<G> {
    let t = ("Aberto", "Rio BRA", "2013.03.01");
    vec![
        g(t, "Alfa, A", "Beta, B", "1-0", (0, 0)),
        g(t, "Beta, B", "Alfa, A", "0-1", (0, 0)),
    ]
}

fn disk_elos(path: &std::path::Path) -> Vec<(u16, u16)> {
    let db = mychess_db::si4::Si4Database::open(path).expect("open");
    (1..=db.len())
        .map(|n| {
            let e = db.entry(n).expect("entry");
            (e.white_elo, e.black_elo)
        })
        .collect()
}

#[test]
fn add_elo_without_elo_data_says_so() {
    let mut t = Tools::new();
    t.open_base(&misspelled_with_exact_names());
    let path = t.dir.path().join("plain.ssp");
    std::fs::write(&path, "@PLAYER \"\"\nAlfa, A\n").expect("write");
    t.rig.app_msg(SpellingMessage::Load(path));
    t.rig.app_msg(SpellingMessage::OpenAddElo);
    // The dialog does not open: the status bar says why.
    assert_eq!(t.top(), None);
    let notice = t.shell().ui.status_notice.clone();
    assert_eq!(
        notice,
        Some(mychess_app::AppNotice::Spelling(
            mychess_app::SpellingNotice::NoEloData
        ))
    );
    assert!(has_text(&t, "no Elo ratings"));
    assert!(
        text::spelling_notice_text(&en(), &mychess_app::SpellingNotice::NoEloData)
            .contains("no Elo ratings")
    );
}

#[test]
fn quitting_with_a_window_open_does_not_hang() {
    let mut t = Tools::new();
    t.open_base(&six());
    t.action(Action::PlayerFinder);
    t.action(Action::TournamentFinder);
    t.rig.app_msg(mychess_app::PlayerInfoMessage::Open {
        name: "Alfa, A".into(),
    });
    t.rig.app_msg(AppMessage::Quit);
    // Nothing runs after the quit: the windows can still be drawn.
    drop(t.sim_default());
}

#[test]
fn every_window_draws_in_small_sizes_and_in_portuguese() {
    for language in [LanguagePref::EnUs, LanguagePref::PtBr] {
        let mut t = Tools::with_language(language);
        t.open_base(&misspelled());
        t.load_ssp();
        for size in [
            Size::new(640.0, 400.0),
            Size::new(1000.0, 600.0),
            Size::new(300.0, 200.0),
        ] {
            t.action(Action::PlayerFinder);
            assert!(t.sim(size).find(Id::new("player-finder-box")).is_ok());
            t.action(Action::PlayerFinder);
            t.action(Action::TournamentFinder);
            assert!(t.sim(size).find(Id::new("tournament-finder-box")).is_ok());
            t.action(Action::TournamentFinder);
            t.rig.app_msg(mychess_app::PlayerInfoMessage::Open {
                name: "Alfa, A".into(),
            });
            assert!(t.sim(size).find(Id::new("player-info-box")).is_ok());
            t.rig.app_msg(mychess_app::PlayerInfoMessage::Close);
            t.rig.app_msg(SpellingMessage::OpenCheck(NameKind::Player));
            assert!(t.sim(size).find(Id::new("spell-check-box")).is_ok());
            t.rig.app_msg(SpellingMessage::CloseCheck);
            t.rig.app_msg(SpellingMessage::OpenAddElo);
            assert!(t.sim(size).find(Id::new("add-elo-box")).is_ok());
            t.rig.app_msg(SpellingMessage::CloseAddElo);
            assert_eq!(t.top(), None);
        }
    }
}

#[test]
fn the_portuguese_texts_show_in_the_finder() {
    let mut t = Tools::with_language(LanguagePref::PtBr);
    t.open_base(&six());
    t.action(Action::PlayerFinder);
    assert!(has_text(&t, "Mais antiga") && has_text(&t, "Partidas") && has_text(&t, "Nome"));
    t.named(NamedKey::Escape);
    t.action(Action::TournamentFinder);
    assert!(has_text(&t, "Nenhum torneio encontrado."));
}

#[test]
fn the_spelling_file_loads_once_after_the_preferences() {
    let dir = tempfile::tempdir().expect("dir");
    let data = dir.path().join("data");
    std::fs::create_dir_all(&data).expect("data");
    std::fs::write(data.join("spelling.ssp"), SSP).expect("write");
    let mut t = Tools::new();
    t.rig.rig.shell.app.set_player_data_dir(Some(data.clone()));
    // The rig sent `PreferencesLoaded` when it was built (before the data
    // directory was set), so the flag is set and a second one is silent.
    assert!(t.shell().ui.dialogs.player_tools.started);
    assert!(!t.app().spelling().is_loaded());
    let message = AppMessage::PreferencesLoaded(mychess_app::LoadedPreferences {
        prefs: mychess_app::Preferences::default(),
        issues: Vec::new(),
    });
    t.rig.app_msg(message);
    assert!(!t.app().spelling().is_loaded(), "only once");
    // A fresh shell loads it on its first preferences.
    let mut fresh = Shell::new(None);
    assert!(!fresh.ui.dialogs.player_tools.started);
    fresh.app.set_player_data_dir(Some(data));
    drop(fresh.update(Message::App(AppMessage::PreferencesLoaded(
        mychess_app::LoadedPreferences {
            prefs: mychess_app::Preferences::default(),
            issues: Vec::new(),
        },
    ))));
    assert!(fresh.ui.dialogs.player_tools.started);
}

fn walk(entries: &[mychess_gui::windows::menu_bar::MenuEntry], out: &mut Vec<&'static str>) {
    use mychess_gui::windows::menu_bar::{Label, MenuEntry};
    for entry in entries {
        match entry {
            MenuEntry::Item {
                label: Label::Key(key),
                action,
                ..
            } if action.is_some() => out.push(key),
            MenuEntry::Submenu { entries, .. } => walk(entries, out),
            _ => {}
        }
    }
}

#[test]
fn the_menus_have_the_new_entries() {
    use mychess_gui::windows::menu_bar::menu_specs;
    let t = Tools::new();
    let specs = menu_specs(t.app(), &t.shell().ui, mychess_gui::ViewOptions::default());
    let mut keys = Vec::new();
    for spec in &specs {
        walk(&spec.entries, &mut keys);
    }
    for key in [
        "menu-file-load-spelling",
        "menu-stats-find-players",
        "menu-stats-find-tournaments",
        "menu-stats-player-info",
        "menu-stats-spell-player",
        "menu-stats-spell-event",
        "menu-stats-spell-site",
        "menu-stats-spell-round",
        "menu-stats-add-elo",
    ] {
        assert!(keys.contains(&key), "{key}");
    }
}

#[test]
fn the_shortcuts_open_the_finders() {
    let mut t = Tools::new();
    t.rig.key(KeyInput {
        key: Key::Named(NamedKey::F(5)),
        modifiers: Modifiers {
            command: true,
            shift: true,
            ..Modifiers::NONE
        },
    });
    assert_eq!(t.top(), Some(ActiveDialog::PlayerFinder));
    t.named(NamedKey::Escape);
    t.rig.key(KeyInput {
        key: Key::Named(NamedKey::F(4)),
        modifiers: Modifiers {
            command: true,
            shift: true,
            ..Modifiers::NONE
        },
    });
    assert_eq!(t.top(), Some(ActiveDialog::TournamentFinder));
}
