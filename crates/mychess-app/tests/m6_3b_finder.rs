//! Contract tests of the player finder and the tournament finder in the
//! application layer (spec `m6-3-jogadores-torneios-spelling.md`, slice
//! M6-3b; sections 2 and 4, rules 35, 75 to 80 and 91 to 100).
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

mod m6_3b_common;

use m6_3b_common::*;
use mychess_app::{
    FinderEffect, PlayerField, PlayerSortKey, TournamentAction, TournamentField, TournamentSortKey,
};

fn names(rig: &Rig) -> Vec<String> {
    rig.state
        .player_finder_view()
        .expect("player finder open")
        .rows
        .iter()
        .map(|r| r.name.clone())
        .collect()
}

fn setup() -> (Rig, SlotId, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "six", &six());
    rig.send(FinderMessage::OpenPlayers { prefix: None });
    (rig, slot, dir)
}

fn field(rig: &mut Rig, f: PlayerField, text: &str) {
    rig.send(FinderMessage::SetPlayerField(f, text.to_owned()));
}

// ---- the player finder ----

#[test]
fn the_table_is_built_when_the_window_opens_and_lists_the_players_by_name() {
    let (rig, _, _dir) = setup();
    let view = rig.state.player_finder_view().expect("window");
    assert_eq!(view.players, 4);
    assert_eq!(view.file_name.as_deref(), Some("six.si4"));
    assert_eq!(names(&rig), ["Alfa, A", "Beta, B", "Delta, D", "Gama, G"]);
    let alfa = &view.rows[0];
    assert_eq!(
        (
            alfa.rank,
            alfa.games,
            alfa.first_year,
            alfa.last_year,
            alfa.elo
        ),
        (1, 5, Some(2020), Some(2020), Some(2520))
    );
    assert!(alfa.country.is_none() && !view.country_column);
    let delta = &view.rows[2];
    assert_eq!(
        (delta.first_year, delta.last_year, delta.elo),
        (None, None, None)
    );
    assert_eq!(view.rows[1].elo, Some(2410));
    assert_eq!(view.sort, PlayerSortKey::Name);
    assert!(view.building.is_none() && !view.no_base);
}

#[test]
fn the_orders_of_the_columns_and_the_second_click_turns_them_round() {
    let (mut rig, _, _dir) = setup();
    rig.send(FinderMessage::SortPlayers(PlayerSortKey::Games));
    assert_eq!(names(&rig), ["Alfa, A", "Beta, B", "Delta, D", "Gama, G"]);
    assert!(rig.state.player_finder_view().unwrap().descending);
    rig.send(FinderMessage::SortPlayers(PlayerSortKey::Games));
    assert_eq!(
        names(&rig),
        ["Delta, D", "Gama, G", "Beta, B", "Alfa, A"].map(String::from)
    );
    assert!(!rig.state.player_finder_view().unwrap().descending);
    rig.send(FinderMessage::SortPlayers(PlayerSortKey::Elo));
    assert_eq!(names(&rig), ["Alfa, A", "Beta, B", "Delta, D", "Gama, G"]);
    rig.send(FinderMessage::SortPlayers(PlayerSortKey::Oldest));
    assert_eq!(
        names(&rig),
        ["Alfa, A", "Beta, B", "Gama, G", "Delta, D"],
        "no date last (D3311)"
    );
    rig.send(FinderMessage::SortPlayers(PlayerSortKey::Newest));
    assert_eq!(names(&rig), ["Alfa, A", "Beta, B", "Gama, G", "Delta, D"]);
    // The order is a preference.
    assert_eq!(
        rig.state.prefs.player_tools.finder.sort,
        PlayerSortKey::Newest
    );
}

#[test]
fn the_filters_of_the_form_apply_when_updated() {
    let (mut rig, _, _dir) = setup();
    field(&mut rig, PlayerField::Prefix, "be");
    assert_eq!(names(&rig).len(), 4, "typing alone asks nothing");
    rig.send(FinderMessage::UpdatePlayers);
    assert_eq!(names(&rig), ["Beta, B"]);
    field(&mut rig, PlayerField::Prefix, "BE");
    rig.send(FinderMessage::UpdatePlayers);
    assert_eq!(names(&rig), ["Beta, B"], "case does not matter");
    field(&mut rig, PlayerField::Prefix, "");
    field(&mut rig, PlayerField::GamesMin, "3");
    rig.send(FinderMessage::UpdatePlayers);
    assert_eq!(names(&rig), ["Alfa, A", "Beta, B"]);
    field(&mut rig, PlayerField::GamesMin, "1");
    field(&mut rig, PlayerField::EloMin, "2450");
    rig.send(FinderMessage::UpdatePlayers);
    assert_eq!(names(&rig), ["Alfa, A"]);
    field(&mut rig, PlayerField::EloMin, "0");
    field(&mut rig, PlayerField::Limit, "2");
    rig.send(FinderMessage::UpdatePlayers);
    assert_eq!(names(&rig), ["Alfa, A", "Beta, B"]);
    field(&mut rig, PlayerField::Limit, "0");
    rig.send(FinderMessage::UpdatePlayers);
    assert_eq!(names(&rig).len(), 4, "0 is no limit");
    // The checks of the Scid rewrite the form.
    field(&mut rig, PlayerField::GamesMin, "50");
    field(&mut rig, PlayerField::GamesMax, "10");
    rig.send(FinderMessage::UpdatePlayers);
    let form = rig.state.player_finder_view().unwrap().form;
    assert_eq!(
        (form.games_min.as_str(), form.games_max.as_str()),
        ("50", "50")
    );
    assert!(names(&rig).is_empty());
    // The last values are kept in the preferences.
    let kept = &rig.state.prefs.player_tools.finder;
    assert_eq!((kept.games_min, kept.games_max, kept.limit), (50, 50, 0));
    // Defaults go back to the window's.
    rig.send(FinderMessage::DefaultsPlayers);
    rig.send(FinderMessage::UpdatePlayers);
    assert_eq!(names(&rig).len(), 4);
}

#[test]
fn the_country_comes_from_the_spelling_file() {
    let (mut rig, _, _dir) = setup();
    field(&mut rig, PlayerField::Country, "yes");
    rig.send(FinderMessage::UpdatePlayers);
    let view = rig.state.player_finder_view().unwrap();
    assert!(view.country_column);
    assert!(
        view.rows
            .iter()
            .all(|r| r.country.as_deref() == Some("~~~")),
        "no file, no country"
    );
    field(&mut rig, PlayerField::Country, "bra");
    rig.send(FinderMessage::UpdatePlayers);
    assert!(names(&rig).is_empty(), "no file: the filter matches nobody");
    rig.load_ssp(SSP);
    rig.send(FinderMessage::UpdatePlayers);
    assert_eq!(names(&rig), ["Alfa, A", "Gama, G"]);
    field(&mut rig, PlayerField::Country, "yes");
    rig.send(FinderMessage::UpdatePlayers);
    let rows = rig.state.player_finder_view().unwrap().rows;
    let by_name = |n: &str| rows.iter().find(|r| r.name == n).unwrap().country.clone();
    assert_eq!(by_name("Alfa, A").as_deref(), Some("BRA"));
    assert_eq!(by_name("Delta, D").as_deref(), Some("ARG"));
    assert_eq!(by_name("Beta, B").as_deref(), Some("~~~"));
    field(&mut rig, PlayerField::Country, "no");
    rig.send(FinderMessage::UpdatePlayers);
    assert!(!rig.state.player_finder_view().unwrap().country_column);
}

#[test]
fn loading_a_spelling_file_builds_the_table_again_with_the_estimated_elo() {
    let (mut rig, _, _dir) = setup();
    let gama = |rig: &Rig| {
        rig.state
            .player_finder_view()
            .unwrap()
            .rows
            .iter()
            .find(|r| r.name == "Gama, G")
            .unwrap()
            .elo
    };
    assert_eq!(gama(&rig), None);
    assert!(!rig.state.player_finder_view().unwrap().estimated);
    rig.load_ssp(SSP);
    assert_eq!(gama(&rig), Some(2050));
    assert!(rig.state.player_finder_view().unwrap().estimated);
    rig.send(SpellingMessage::Unload);
    assert_eq!(gama(&rig), None);
}

#[test]
fn another_database_or_a_changed_one_builds_the_table_again() {
    let (mut rig, slot, dir) = setup();
    let few = vec![g("X", "Y", "2001.01.01", "Zed, Z", "Yan, Y", "1-0", 0, 0)];
    rig.open_si4(dir.path(), "few", &few);
    assert_eq!(names(&rig), ["Yan, Y", "Zed, Z"]);
    assert_eq!(
        rig.state.player_finder_view().unwrap().file_name.as_deref(),
        Some("few.si4")
    );
    rig.send(BasesMessage::Select(slot));
    assert_eq!(names(&rig).len(), 4);
    // A write that renames players: the new database, the new table.
    rig.load_ssp(SSP);
    let dirty = vec![g(
        "X",
        "Y",
        "2020.01.01",
        "Alfa, Alf",
        "Beta, B",
        "1-0",
        0,
        0,
    )];
    rig.open_si4(dir.path(), "dirty", &dirty);
    assert!(names(&rig).contains(&"Alfa, Alf".to_owned()));
    rig.send(SpellingMessage::OpenCheck(mychess_app::NameKind::Player));
    rig.send(SpellingMessage::Apply);
    rig.send(SpellingMessage::ConfirmApply(true));
    assert!(
        names(&rig).contains(&"Alfa, A".to_owned()),
        "{:?}",
        names(&rig)
    );
    assert!(
        !names(&rig).contains(&"Alfa, Alf".to_owned()),
        "a name with no games is not listed"
    );
}

#[test]
fn clicking_a_row_opens_the_player_information() {
    let (mut rig, _, _dir) = setup();
    rig.send(FinderMessage::ClickPlayer(1));
    assert!(rig.state.player_info().is_open());
    assert_eq!(rig.state.player_info().name(), "Beta, B");
    // A click outside the list does nothing.
    rig.send(FinderMessage::ClickPlayer(99));
    assert_eq!(rig.state.player_info().name(), "Beta, B");
}

#[test]
fn opening_with_a_prefix_resets_the_other_fields() {
    let (mut rig, _, _dir) = setup();
    field(&mut rig, PlayerField::GamesMin, "99");
    rig.send(FinderMessage::OpenPlayers {
        prefix: Some("Gama,".to_owned()),
    });
    let form = rig.state.player_finder_view().unwrap().form;
    assert_eq!(form.prefix, "Gama,");
    assert_eq!(form.games_min, "1");
    assert_eq!(names(&rig), ["Gama, G"]);
}

#[test]
fn toggling_closes_and_a_late_answer_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "six", &six());
    let fx = rig.raw(FinderMessage::TogglePlayers);
    let Some(Effect::Finder(FinderEffect::Build { control, .. })) = fx.first() else {
        panic!("a build was expected: {fx:?}");
    };
    assert!(rig.state.player_finder_view().unwrap().building.is_some());
    rig.raw(FinderMessage::TogglePlayers);
    assert!(rig.state.player_finder_view().is_none());
    assert!(control.is_cancelled(), "closing stops the build");
    let late = rig.perform(fx[0].clone());
    let more = rig.raw(late);
    assert!(
        more.iter().all(|e| !matches!(e, Effect::Finder(_))),
        "{more:?}"
    );
    // Opened again, it works from scratch.
    rig.send(FinderMessage::TogglePlayers);
    assert_eq!(names(&rig).len(), 4);
}

#[test]
fn quitting_stops_the_build() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "six", &six());
    let fx = rig.raw(FinderMessage::OpenPlayers { prefix: None });
    let Some(Effect::Finder(FinderEffect::Build { control, .. })) = fx.first() else {
        panic!("build");
    };
    rig.raw(Message::Quit);
    assert!(control.is_cancelled());
}

#[test]
fn without_a_usable_database_the_window_says_so() {
    let mut rig = Rig::new();
    rig.send(FinderMessage::OpenPlayers { prefix: None });
    let view = rig.state.player_finder_view().unwrap();
    // The Clipbase is always there, and empty.
    assert!(view.rows.is_empty());
    assert_eq!(view.players, 0);
}

// ---- the tournament finder ----

fn tfield(rig: &mut Rig, f: TournamentField, text: &str) {
    rig.send(FinderMessage::SetTournamentField(f, text.to_owned()));
}

fn open_tournaments() -> (Rig, SlotId, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "six", &six());
    rig.send(FinderMessage::OpenTournaments { player: None });
    tfield(&mut rig, TournamentField::PlayersMin, "1");
    tfield(&mut rig, TournamentField::GamesMin, "1");
    rig.send(FinderMessage::UpdateTournaments);
    (rig, slot, dir)
}

fn tour_rows(rig: &Rig) -> Vec<(String, u32, u32)> {
    rig.state
        .tournament_finder_view()
        .expect("window")
        .rows
        .map(|rows| {
            rows.iter()
                .map(|r| (format!("{}/{}", r.event, r.site), r.games, r.players))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn the_window_starts_with_the_defaults_of_the_scid() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "six", &six());
    rig.send(FinderMessage::OpenTournaments { player: None });
    let view = rig.state.tournament_finder_view().unwrap();
    assert_eq!(view.form.start, "1960.??.??");
    assert_eq!(
        (
            view.form.players_min.as_str(),
            view.form.players_max.as_str()
        ),
        ("4", "1999")
    );
    assert_eq!(
        (view.form.games_min.as_str(), view.form.games_max.as_str()),
        ("10", "9999")
    );
    assert_eq!(view.form.limit, "1000");
    assert_eq!(view.sort, TournamentSortKey::Date);
    // Nothing has 10 games or 4 players: an empty list, not "no answer".
    assert_eq!(view.rows.map(|r| r.len()), Some(0));
}

#[test]
fn the_tournaments_of_the_six_games() {
    let (rig, _, _dir) = open_tournaments();
    let view = rig.state.tournament_finder_view().unwrap();
    let rows = view.rows.expect("rows");
    assert_eq!(
        rows.len(),
        2,
        "the undated games are out when a start is set"
    );
    // Newest first: B then A.
    let b = &rows[0];
    assert_eq!(
        (b.games, b.players, b.mean_elo, b.first_game),
        (1, 2, Some(2465), 4)
    );
    assert_eq!(b.date.to_string(), "2020.05.20");
    let a = &rows[1];
    assert_eq!(
        (a.games, a.players, a.mean_elo, a.first_game),
        (3, 3, Some(2453), 1)
    );
    assert_eq!(a.date.to_string(), "2020.01.10");
    assert_eq!((a.event.as_str(), a.site.as_str()), ("Aberto", "Rio BRA"));
    let winner = a.winner.as_ref().unwrap();
    assert_eq!(
        (
            winner.name.as_str(),
            winner.surname.as_str(),
            winner.elo,
            winner.points.as_str()
        ),
        ("Alfa, A", "Alfa", Some(2510), "2.0")
    );
    let second = a.runner_up.as_ref().unwrap();
    assert_eq!(
        (second.surname.as_str(), second.elo, second.points.as_str()),
        ("Beta", Some(2400), "0.5")
    );
    assert!(!a.tie, "2.0 against 0.5");
    assert_eq!(a.rank, 2);
}

#[test]
fn the_orders_and_the_filters_of_the_form() {
    let (mut rig, _, _dir) = open_tournaments();
    let events = |rig: &Rig| {
        tour_rows(rig)
            .iter()
            .map(|r| (r.1, r.2))
            .collect::<Vec<_>>()
    };
    rig.send(FinderMessage::SortTournaments(TournamentSortKey::Games));
    assert_eq!(events(&rig), [(3, 3), (1, 2)]);
    rig.send(FinderMessage::SortTournaments(TournamentSortKey::Elo));
    assert_eq!(events(&rig), [(1, 2), (3, 3)]);
    rig.send(FinderMessage::SortTournaments(TournamentSortKey::Players));
    assert_eq!(events(&rig), [(3, 3), (1, 2)]);
    rig.send(FinderMessage::SortTournaments(TournamentSortKey::Date));
    assert_eq!(events(&rig), [(1, 2), (3, 3)]);
    assert_eq!(
        rig.state.prefs.player_tools.tournaments.sort,
        TournamentSortKey::Date
    );

    tfield(&mut rig, TournamentField::Player, "gam");
    rig.send(FinderMessage::UpdateTournaments);
    assert_eq!(events(&rig), [(3, 3)]);
    tfield(&mut rig, TournamentField::Player, "");
    tfield(&mut rig, TournamentField::Site, "rio");
    rig.send(FinderMessage::UpdateTournaments);
    assert_eq!(events(&rig).len(), 2);
    tfield(&mut rig, TournamentField::Site, "");
    tfield(&mut rig, TournamentField::Country, "bra");
    rig.send(FinderMessage::UpdateTournaments);
    assert_eq!(events(&rig).len(), 2);
    assert_eq!(
        rig.state.tournament_finder_view().unwrap().form.country,
        "BRA"
    );
    tfield(&mut rig, TournamentField::Country, "PER");
    rig.send(FinderMessage::UpdateTournaments);
    assert!(events(&rig).is_empty(), "the Peru games have no date");
    tfield(&mut rig, TournamentField::Country, "");
    tfield(&mut rig, TournamentField::PlayersMin, "3");
    rig.send(FinderMessage::UpdateTournaments);
    assert_eq!(events(&rig), [(3, 3)]);
    tfield(&mut rig, TournamentField::PlayersMin, "1");
    tfield(&mut rig, TournamentField::EloMin, "2460");
    rig.send(FinderMessage::UpdateTournaments);
    assert_eq!(events(&rig), [(1, 2)]);
    tfield(&mut rig, TournamentField::EloMin, "0");
    tfield(&mut rig, TournamentField::Limit, "1");
    rig.send(FinderMessage::UpdateTournaments);
    assert_eq!(events(&rig).len(), 1);
}

#[test]
fn dates_are_completed_and_games_without_date_come_in_with_no_start() {
    let (mut rig, _, _dir) = open_tournaments();
    tfield(&mut rig, TournamentField::Start, "2020");
    tfield(&mut rig, TournamentField::End, "2020.02");
    rig.send(FinderMessage::UpdateTournaments);
    let form = rig.state.tournament_finder_view().unwrap().form;
    assert_eq!(
        (form.start.as_str(), form.end.as_str()),
        ("2020.??.??", "2020.02.31")
    );
    assert_eq!(tour_rows(&rig).len(), 1, "only the January games");
    tfield(&mut rig, TournamentField::Start, "");
    tfield(&mut rig, TournamentField::End, "");
    rig.send(FinderMessage::UpdateTournaments);
    let rows = tour_rows(&rig);
    assert_eq!(
        rows.len(),
        4,
        "two tournaments of one game without date: {rows:?}"
    );
    assert_eq!(rows.iter().filter(|r| r.0.starts_with("Solo")).count(), 2);
}

#[test]
fn the_player_button_fills_the_form_the_way_the_scid_does() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "six", &six());
    rig.send(FinderMessage::OpenTournaments {
        player: Some("Alfa, A".to_owned()),
    });
    let view = rig.state.tournament_finder_view().unwrap();
    assert_eq!(view.player.as_deref(), Some("Alfa, A"));
    assert_eq!(view.form.start, "1800.??.??");
    assert_eq!(
        (view.form.players_min.as_str(), view.form.games_min.as_str()),
        ("1", "1")
    );
    let rows = view.rows.expect("rows");
    assert_eq!(rows.len(), 2, "the dated tournaments of Alfa");
    // Delta only played the games without a date, which a start date leaves out.
    rig.send(FinderMessage::OpenTournaments {
        player: Some("Delta, D".to_owned()),
    });
    assert_eq!(tour_rows(&rig).len(), 0);
}

#[test]
fn the_player_information_opens_the_finders() {
    let (mut rig, _, _dir) = setup();
    rig.send(PlayerInfoMessage::Open {
        name: "Gama, G".to_owned(),
    });
    rig.send(PlayerInfoMessage::OpenTournaments);
    assert_eq!(
        rig.state
            .tournament_finder_view()
            .unwrap()
            .player
            .as_deref(),
        Some("Gama, G")
    );
    rig.send(PlayerInfoMessage::LookupName);
    assert_eq!(rig.state.player_finder_view().unwrap().form.prefix, "Gama,");
}

#[test]
fn a_click_on_a_tournament_does_what_the_front_end_asks() {
    let (mut rig, slot, _dir) = open_tournaments();
    // Newest first: row 1 is the January tournament (games 1 to 3).
    rig.send(FinderMessage::ClickTournament {
        index: 1,
        action: TournamentAction::ShowInFilter,
    });
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3]);
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Finder(FinderNotice::Filtered { games: 3 }))
    );
    rig.send(FinderMessage::ClickTournament {
        index: 0,
        action: TournamentAction::LoadFirstGame,
    });
    assert_eq!(rig.state.session.game().tags().white, "Alfa, A");
    assert_eq!(
        rig.state.session.game().tags().date.to_string(),
        "2020.05.20"
    );
    rig.send(FinderMessage::ClickTournament {
        index: 1,
        action: TournamentAction::OpenCrosstable,
    });
    assert!(rig.state.crosstable().is_open());
    assert!(
        !rig.state.crosstable_view().following,
        "the table of that tournament, not of the game on screen"
    );
    // A row that is not there does nothing.
    let before = rig.filter_numbers(slot);
    rig.send(FinderMessage::ClickTournament {
        index: 9,
        action: TournamentAction::ShowInFilter,
    });
    assert_eq!(rig.filter_numbers(slot), before);
}

#[test]
fn a_click_after_the_database_changed_is_refused() {
    let (mut rig, slot, _dir) = open_tournaments();
    // The names of the database are rewritten: the slot holds a new database
    // and the rows (which wait for their debounce) are of the old one.
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(mychess_app::NameKind::Event));
    rig.send(SpellingMessage::Apply);
    let fx = rig.raw(SpellingMessage::ConfirmApply(true));
    let answer = rig.perform(fx[0].clone());
    rig.raw(answer);
    let before = rig.state.bases.slot(slot).unwrap().filter().count();
    rig.raw(FinderMessage::ClickTournament {
        index: 0,
        action: TournamentAction::ShowInFilter,
    });
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Finder(FinderNotice::Stale))
    );
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter().count(), before);
}

#[test]
fn a_late_or_stopped_search_answer_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "six", &six());
    rig.send(FinderMessage::OpenTournaments { player: None });
    tfield(&mut rig, TournamentField::PlayersMin, "1");
    tfield(&mut rig, TournamentField::GamesMin, "1");
    let first = rig.raw(FinderMessage::UpdateTournaments);
    let Some(Effect::Finder(FinderEffect::Tournaments { control, .. })) = first.first() else {
        panic!("search: {first:?}");
    };
    tfield(&mut rig, TournamentField::Event, "Solo");
    let second = rig.raw(FinderMessage::UpdateTournaments);
    assert!(control.is_cancelled(), "a new query stops the old search");
    let old = rig.perform(first[0].clone());
    rig.raw(old);
    assert_eq!(tour_rows(&rig).len(), 0, "the old answer was dropped");
    let new = rig.perform(second[0].clone());
    rig.raw(new);
    assert!(
        tour_rows(&rig).is_empty(),
        "start still 1960: the Solo games have no date"
    );
    rig.send(FinderMessage::CancelTournaments);
    rig.raw(FinderMessage::CloseTournaments);
    assert!(rig.state.tournament_finder_view().is_none());
}

#[test]
fn the_tournament_form_is_a_preference() {
    let (rig, _, _dir) = open_tournaments();
    let t = &rig.state.prefs.player_tools.tournaments;
    assert_eq!((t.min_players, t.min_games, t.limit), (1, 1, 1000));
    assert_eq!(t.start, "1960.??.??");
}
