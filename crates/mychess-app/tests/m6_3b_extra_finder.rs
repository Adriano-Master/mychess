//! Extra contract tests of the player finder and the tournament finder in
//! the application layer (spec `m6-3-jogadores-torneios-spelling.md`, slice
//! M6-3b; sections 2 and 4, rules 35, 75 to 80 and 91 to 100): stale and
//! cancelled answers, closing or switching the database while working,
//! quitting, concurrent queries, a changing database and hostile fields.
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

fn field(rig: &mut Rig, f: PlayerField, text: &str) {
    rig.send(FinderMessage::SetPlayerField(f, text.to_owned()));
}

fn tfield(rig: &mut Rig, f: TournamentField, text: &str) {
    rig.send(FinderMessage::SetTournamentField(f, text.to_owned()));
}

fn few() -> Vec<G> {
    vec![g("X", "Y", "2001.01.01", "Zed, Z", "Yan, Y", "1-0", 0, 0)]
}

fn tour_len(rig: &Rig) -> Option<usize> {
    rig.state
        .tournament_finder_view()
        .expect("window")
        .rows
        .map(|r| r.len())
}

// ---- the player finder: stopping, closing, switching ----

#[test]
fn after_stopping_a_build_update_builds_the_table_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "six", &six());
    let fx = rig.raw(FinderMessage::OpenPlayers { prefix: None });
    rig.raw(FinderMessage::CancelPlayers);
    let answer = rig.perform(fx[0].clone());
    rig.raw(answer);
    assert!(rig.state.player_finder_view().unwrap().building.is_none());
    // The user presses "Update": the window must work again.
    rig.send(FinderMessage::UpdatePlayers);
    assert_eq!(
        names(&rig).len(),
        4,
        "the stopped build left the window with no table for good"
    );
}

#[test]
fn closing_the_database_while_the_table_builds_stops_it_and_shows_no_rows() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "six", &six());
    let fx = rig.raw(FinderMessage::OpenPlayers { prefix: None });
    let Effect::Finder(FinderEffect::Build { control, .. }) = &fx[0] else {
        panic!("build");
    };
    rig.raw(BasesMessage::Close(slot));
    assert!(control.is_cancelled());
    let answer = rig.perform(fx[0].clone());
    let more = rig.raw(answer);
    assert!(more.iter().all(|e| !matches!(e, Effect::Finder(_))));
    let view = rig.state.player_finder_view().unwrap();
    assert!(
        view.rows.is_empty(),
        "the Clipbase takes the place of the closed database"
    );
}

#[test]
fn the_table_of_another_database_is_never_shown_for_the_current_one() {
    for old_last in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut rig = Rig::new();
        let (six_slot, _) = rig.open_si4(dir.path(), "six", &six());
        rig.open_si4(dir.path(), "few", &few());
        let old = rig.raw(FinderMessage::OpenPlayers { prefix: None });
        let new = rig.raw(BasesMessage::Select(six_slot));
        let new_build = new
            .iter()
            .find(|e| matches!(e, Effect::Finder(FinderEffect::Build { .. })))
            .expect("a build for the other database")
            .clone();
        let old_answer = rig.perform(old[0].clone());
        let new_answer = rig.perform(new_build);
        if old_last {
            let queries = rig.raw(new_answer);
            for q in queries {
                let a = rig.perform(q);
                rig.raw(a);
            }
            rig.raw(old_answer);
        } else {
            rig.raw(old_answer);
            let queries = rig.raw(new_answer);
            for q in queries {
                let a = rig.perform(q);
                rig.raw(a);
            }
        }
        assert_eq!(
            names(&rig).len(),
            4,
            "old_last = {old_last}: {:?}",
            names(&rig)
        );
        assert!(!names(&rig).contains(&"Zed, Z".to_owned()));
    }
}

#[test]
fn two_queries_in_flight_the_last_one_asked_wins_in_any_arrival_order() {
    for newest_first in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut rig = Rig::new();
        rig.open_si4(dir.path(), "six", &six());
        rig.send(FinderMessage::OpenPlayers { prefix: None });
        let q1 = rig.raw(FinderMessage::SortPlayers(PlayerSortKey::Games));
        let q2 = rig.raw(FinderMessage::SortPlayers(PlayerSortKey::Games));
        assert_eq!((q1.len(), q2.len()), (1, 1));
        let a1 = rig.perform(q1[0].clone());
        let a2 = rig.perform(q2[0].clone());
        if newest_first {
            rig.raw(a2);
            rig.raw(a1);
        } else {
            rig.raw(a1);
            rig.raw(a2);
        }
        assert_eq!(
            names(&rig),
            ["Delta, D", "Gama, G", "Beta, B", "Alfa, A"],
            "newest_first = {newest_first}: the second click on Games turns the order round"
        );
    }
}

#[test]
fn quitting_while_the_table_builds_or_a_search_runs_stops_both() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "six", &six());
    let players = rig.raw(FinderMessage::OpenPlayers { prefix: None });
    let tournaments = rig.raw(FinderMessage::OpenTournaments { player: None });
    let Effect::Finder(FinderEffect::Tournaments { control, .. }) = &tournaments[0] else {
        panic!("search: {tournaments:?}");
    };
    rig.raw(Message::Quit);
    assert!(control.is_cancelled());
    // Late answers after quitting do not start anything.
    let a = rig.perform(players[0].clone());
    let b = rig.perform(tournaments[0].clone());
    for m in [a, b] {
        let more = rig.raw(m);
        assert!(
            more.iter().all(|e| !matches!(e, Effect::Finder(_))),
            "{more:?}"
        );
    }
}

// ---- the player finder: the database changing ----

#[test]
fn the_clipbase_growing_builds_the_table_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "six", &six());
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::ConfirmCopy(
        false,
    ))));
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    rig.send(FinderMessage::OpenPlayers { prefix: None });
    assert!(names(&rig).is_empty(), "the Clipbase starts empty");
    rig.send(Message::Bases(BasesMessage::List(ListMessage::CopyGames {
        from: slot,
        to: SlotId::CLIPBASE,
        scope: mychess_app::CopyScope::Filter,
    })));
    assert_eq!(rig.state.bases.slot(SlotId::CLIPBASE).unwrap().total(), 6);
    assert_eq!(
        names(&rig),
        ["Alfa, A", "Beta, B", "Delta, D", "Gama, G"],
        "rule 80: the table follows the base"
    );
}

#[test]
fn unloading_the_spelling_file_takes_the_estimated_elo_and_the_country_away() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "six", &six());
    rig.load_ssp(SSP);
    rig.send(FinderMessage::OpenPlayers { prefix: None });
    field(&mut rig, PlayerField::Country, "yes");
    rig.send(FinderMessage::UpdatePlayers);
    let rows = rig.state.player_finder_view().unwrap().rows;
    let delta = |rows: &[mychess_app::PlayerRowView]| {
        rows.iter().find(|r| r.name == "Delta, D").unwrap().clone()
    };
    assert_eq!(delta(&rows).elo, Some(2100));
    assert_eq!(delta(&rows).country.as_deref(), Some("ARG"));
    rig.send(SpellingMessage::Unload);
    let rows = rig.state.player_finder_view().unwrap().rows;
    assert_eq!(delta(&rows).elo, None, "no spelling, no estimate");
    assert_eq!(delta(&rows).country.as_deref(), Some("~~~"));
}

#[test]
fn reloading_a_changed_spelling_file_changes_the_countries_shown() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "six", &six());
    let path = rig.load_ssp(SSP);
    rig.send(FinderMessage::OpenPlayers { prefix: None });
    field(&mut rig, PlayerField::Country, "yes");
    rig.send(FinderMessage::UpdatePlayers);
    let country = |rig: &Rig| {
        rig.state
            .player_finder_view()
            .unwrap()
            .rows
            .iter()
            .find(|r| r.name == "Alfa, A")
            .unwrap()
            .country
            .clone()
    };
    assert_eq!(country(&rig).as_deref(), Some("BRA"));
    std::fs::write(&path, SSP.replace("GM BRA [2520]", "GM ARG [2520]")).unwrap();
    rig.send(SpellingMessage::Reload);
    assert_eq!(
        country(&rig).as_deref(),
        Some("ARG"),
        "D3441: no stale cache"
    );
}

// ---- the player finder: hostile fields ----

#[test]
fn hostile_form_texts_are_brought_inside_the_limits_of_the_scid() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "six", &six());
    rig.send(FinderMessage::OpenPlayers { prefix: None });
    let cases = [
        ("abc", "-5", "99999999999999999999", "zz", "\u{0}"),
        ("-1", "0", "18446744073709551616", "5000", "1e9"),
        ("", "", "", "", ""),
        ("500", "10", "7", "4001", "\u{1F600}\u{202e}"),
    ];
    for (a, b, c, d, e) in cases {
        field(&mut rig, PlayerField::GamesMin, a);
        field(&mut rig, PlayerField::GamesMax, b);
        field(&mut rig, PlayerField::Limit, c);
        field(&mut rig, PlayerField::EloMax, d);
        field(&mut rig, PlayerField::Country, e);
        rig.send(FinderMessage::UpdatePlayers);
        let form = rig.state.player_finder_view().unwrap().form;
        let n = |s: &str| s.parse::<u64>().unwrap_or_else(|_| panic!("number: {s:?}"));
        assert!(n(&form.games_min) <= 100_000, "{form:?}");
        assert!((1..=1_000_000).contains(&n(&form.games_max)), "{form:?}");
        assert!(n(&form.games_max) >= n(&form.games_min), "{form:?}");
        assert!(
            n(&form.elo_max) <= 4000 && n(&form.elo_min) <= n(&form.elo_max),
            "{form:?}"
        );
        assert!(n(&form.limit) <= 100_000, "{form:?}");
        assert!(
            form.country == "no"
                || form.country == "yes"
                || (form.country.len() <= 3
                    && form.country.chars().all(|c| c.is_ascii_uppercase())),
            "{form:?}"
        );
    }
}

#[test]
fn a_giant_or_odd_prefix_is_harmless() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "six", &six());
    rig.send(FinderMessage::OpenPlayers { prefix: None });
    for prefix in [
        "A".repeat(300_000),
        "\u{0}".into(),
        "\u{130}lfa".into(),
        "  ".into(),
    ] {
        field(&mut rig, PlayerField::Prefix, &prefix);
        rig.send(FinderMessage::UpdatePlayers);
        drop(names(&rig));
    }
    // Case is ignored (rule 4 of section 2).
    field(&mut rig, PlayerField::Prefix, "aLFa");
    rig.send(FinderMessage::UpdatePlayers);
    assert_eq!(names(&rig), ["Alfa, A"]);
    // Spaces are not ignored.
    field(&mut rig, PlayerField::Prefix, " alfa");
    rig.send(FinderMessage::UpdatePlayers);
    assert!(names(&rig).is_empty());
}

#[test]
fn clicking_a_row_that_is_not_there_does_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "six", &six());
    rig.send(FinderMessage::OpenPlayers { prefix: None });
    let before = rig.state.notice.clone();
    for i in [4, 5, 1_000_000, usize::MAX] {
        let fx = rig.raw(FinderMessage::ClickPlayer(i));
        assert!(fx.is_empty(), "{fx:?}");
    }
    assert!(rig.state.player_info_view().is_none());
    assert_eq!(rig.state.notice, before);
}

#[test]
fn a_prefix_given_by_the_player_window_survives_a_closed_base_and_a_new_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "six", &six());
    rig.send(FinderMessage::OpenPlayers {
        prefix: Some("Be".into()),
    });
    rig.send(BasesMessage::Close(slot));
    assert!(names(&rig).is_empty());
    rig.open_si4(dir.path(), "six2", &six());
    assert_eq!(names(&rig), ["Beta, B"]);
}

// ---- the tournament finder ----

fn open_tournaments(rig: &mut Rig) {
    rig.send(FinderMessage::OpenTournaments { player: None });
    tfield(rig, TournamentField::PlayersMin, "1");
    tfield(rig, TournamentField::GamesMin, "1");
    rig.send(FinderMessage::UpdateTournaments);
}

#[test]
fn closing_the_database_during_a_search_stops_it_and_drops_the_answer() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "six", &six());
    let fx = rig.raw(FinderMessage::OpenTournaments { player: None });
    let Effect::Finder(FinderEffect::Tournaments { control, .. }) = &fx[0] else {
        panic!("search");
    };
    rig.raw(BasesMessage::Close(slot));
    assert!(control.is_cancelled());
    let answer = rig.perform(fx[0].clone());
    rig.raw(answer);
    let view = rig.state.tournament_finder_view().unwrap();
    assert!(view.rows.is_none() || view.rows.unwrap().is_empty());
}

#[test]
fn the_search_of_another_database_is_never_shown_for_the_current_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (six_slot, _) = rig.open_si4(dir.path(), "six", &six());
    rig.open_si4(dir.path(), "few", &few());
    let old = rig.raw(FinderMessage::OpenTournaments { player: None });
    tfield(&mut rig, TournamentField::PlayersMin, "1");
    tfield(&mut rig, TournamentField::GamesMin, "1");
    let old_answer = rig.perform(old[0].clone());
    let new = rig.raw(BasesMessage::Select(six_slot));
    let Some(Effect::Finder(FinderEffect::Tournaments { .. })) = new.first() else {
        panic!("a new search: {new:?}");
    };
    rig.raw(old_answer);
    let rows = rig.state.tournament_finder_view().unwrap().rows;
    assert!(
        rows.is_none_or(|r| r.iter().all(|r| r.event != "X")),
        "the old answer was dropped"
    );
    let a = rig.perform(new[0].clone());
    rig.raw(a);
    assert!(tour_len(&rig).is_some());
}

#[test]
fn after_stopping_a_search_update_searches_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "six", &six());
    let fx = rig.raw(FinderMessage::OpenTournaments { player: None });
    rig.raw(FinderMessage::CancelTournaments);
    let answer = rig.perform(fx[0].clone());
    rig.raw(answer);
    tfield(&mut rig, TournamentField::PlayersMin, "1");
    tfield(&mut rig, TournamentField::GamesMin, "1");
    rig.send(FinderMessage::UpdateTournaments);
    assert!(
        tour_len(&rig).unwrap_or(0) >= 1,
        "a stopped search must not lock the window"
    );
}

#[test]
fn hostile_tournament_fields_never_panic() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "six", &six());
    rig.send(FinderMessage::OpenTournaments { player: None });
    let junk = [
        "",
        "abc",
        "-1",
        "99999999999999999999",
        "2020.13.45",
        "20200",
        "\u{0}",
        "\u{1F600}",
        "????.??.??",
        "---",
        "é☃x",
    ];
    let fields = [
        TournamentField::GamesMin,
        TournamentField::GamesMax,
        TournamentField::PlayersMin,
        TournamentField::PlayersMax,
        TournamentField::EloMin,
        TournamentField::EloMax,
        TournamentField::Start,
        TournamentField::End,
        TournamentField::Event,
        TournamentField::Site,
        TournamentField::Player,
        TournamentField::Country,
        TournamentField::Limit,
    ];
    for (i, f) in fields.iter().enumerate() {
        for j in &junk {
            tfield(&mut rig, *f, j);
            rig.send(FinderMessage::UpdateTournaments);
            let _ = i;
        }
    }
    tfield(&mut rig, TournamentField::Event, &"E".repeat(500_000));
    rig.send(FinderMessage::UpdateTournaments);
    assert!(rig.state.tournament_finder_view().is_some());
}

#[test]
fn clicks_on_missing_tournaments_do_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "six", &six());
    // No result yet.
    rig.raw(FinderMessage::OpenTournaments { player: None });
    let before = rig.filter_numbers(slot);
    for action in [
        TournamentAction::OpenCrosstable,
        TournamentAction::LoadFirstGame,
        TournamentAction::ShowInFilter,
    ] {
        let fx = rig.raw(FinderMessage::ClickTournament { index: 0, action });
        assert!(fx.is_empty());
    }
    tfield(&mut rig, TournamentField::PlayersMin, "1");
    tfield(&mut rig, TournamentField::GamesMin, "1");
    rig.send(FinderMessage::UpdateTournaments);
    for index in [99, usize::MAX] {
        let fx = rig.raw(FinderMessage::ClickTournament {
            index,
            action: TournamentAction::ShowInFilter,
        });
        assert!(fx.is_empty());
    }
    assert_eq!(rig.filter_numbers(slot), before);
}

#[test]
fn the_tournament_list_follows_a_clipbase_that_grows() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "six", &six());
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::ConfirmCopy(
        false,
    ))));
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    open_tournaments(&mut rig);
    assert_eq!(tour_len(&rig), Some(0));
    rig.send(Message::Bases(BasesMessage::List(ListMessage::CopyGames {
        from: slot,
        to: SlotId::CLIPBASE,
        scope: mychess_app::CopyScope::Filter,
    })));
    assert!(tour_len(&rig).unwrap_or(0) >= 1);
}

#[test]
fn sorting_tournaments_twice_quickly_keeps_the_last_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "six", &six());
    open_tournaments(&mut rig);
    let first = rig.raw(FinderMessage::SortTournaments(TournamentSortKey::Games));
    let second = rig.raw(FinderMessage::SortTournaments(TournamentSortKey::Event));
    let a = rig.perform(first[0].clone());
    let b = rig.perform(second[0].clone());
    rig.raw(b);
    rig.raw(a);
    let rows = rig.state.tournament_finder_view().unwrap().rows.unwrap();
    let events: Vec<_> = rows.iter().map(|r| r.event.clone()).collect();
    let mut sorted = events.clone();
    sorted.sort_by_key(|e| e.to_lowercase());
    assert_eq!(
        events, sorted,
        "ordered by event (ascending), the last click"
    );
}
