//! Contract tests of the player information window in the application
//! layer (spec `m6-3-jogadores-torneios-spelling.md`, slice M6-3b; section
//! 3 and rules 81 to 90, D3318 to D3323).
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

mod m6_3b_common;

use m6_3b_common::*;
use mychess_app::{PhotoFormat, PlayerInfoEffect};

fn setup() -> (Rig, SlotId, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "six", &six());
    (rig, slot, dir)
}

fn open(rig: &mut Rig, name: &str) {
    rig.send(PlayerInfoMessage::Open {
        name: name.to_owned(),
    });
}

fn info_notice(rig: &Rig) -> Option<PlayerInfoNotice> {
    match rig.state.notice.as_ref() {
        Some(AppNotice::PlayerInfo(n)) => Some(*n),
        _ => None,
    }
}

fn keep_games(rig: &mut Rig, slot: SlotId, remove: &[u32]) {
    rig.send(Message::Bases(BasesMessage::List(
        ListMessage::SelectNumbers {
            slot,
            numbers: remove.to_vec(),
        },
    )));
    rig.send(Message::Bases(BasesMessage::List(
        ListMessage::RemoveSelected { slot },
    )));
}

#[test]
fn the_figures_of_a_player_follow_the_scid() {
    let (mut rig, _, _dir) = setup();
    open(&mut rig, "Alfa, A");
    let view = rig.state.player_info_view().expect("window");
    assert!(view.found && !view.stale && view.progress.is_none());
    let stats = view.stats.expect("stats");
    assert_eq!(stats.games, 5);
    assert_eq!(stats.first.to_string(), "2020.01.10");
    assert_eq!(stats.last.to_string(), "2020.05.20");
    assert!(stats.last_shown);
    let all = &stats.all;
    assert_eq!(
        (
            all.total.wins,
            all.total.losses,
            all.total.draws,
            all.total.unfinished
        ),
        (3, 0, 1, 1)
    );
    assert_eq!(all.total.percent_hundredths, 8000, "80.00%");
    assert_eq!(all.total.points, "4.0");
    assert_eq!(
        (all.white.games, all.white.wins, all.white.draws),
        (3, 2, 1)
    );
    assert_eq!(
        (all.black.games, all.black.wins, all.black.unfinished),
        (2, 1, 1)
    );
    assert!(
        stats.filter.is_none() && stats.in_filter.is_none(),
        "the filter holds every game"
    );
    assert!(stats.versus.is_none(), "the game on screen is not Alfa's");
    assert_eq!(stats.ratings, [(2020, 1, 2510), (2020, 5, 2520)]);
    assert!(
        stats.openings[0].is_empty() && stats.openings[1].is_empty(),
        "no ECO codes"
    );
    assert!(
        view.header.is_none() && !view.spelling_loaded,
        "no spelling file"
    );
}

#[test]
fn the_filter_section_appears_when_the_filter_is_smaller() {
    let (mut rig, slot, _dir) = setup();
    keep_games(&mut rig, slot, &[5, 6]);
    open(&mut rig, "Alfa, A");
    let stats = rig.state.player_info_view().unwrap().stats.unwrap();
    assert_eq!(stats.in_filter, Some(3));
    let f = stats.filter.expect("filter section");
    assert_eq!(
        (f.total.wins, f.total.games, f.total.percent_hundredths),
        (3, 3, 10000)
    );
    assert_eq!(f.total.points, "3.0");
}

#[test]
fn the_opponent_of_the_game_on_screen_gets_a_section() {
    let (mut rig, slot, _dir) = setup();
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    open(&mut rig, "Alfa, A");
    let view = rig.state.player_info_view().unwrap();
    assert_eq!(view.opponent.as_deref(), Some("Beta, B"));
    let (name, split) = view.stats.unwrap().versus.expect("versus");
    assert_eq!(name, "Beta, B");
    assert_eq!(
        (split.total.games, split.white.wins, split.black.games),
        (2, 2, 0)
    );
    assert_eq!(split.total.percent_hundredths, 10000);
}

#[test]
fn a_name_that_is_not_in_the_database_has_only_the_header() {
    let (mut rig, _, _dir) = setup();
    rig.load_ssp(&SSP.replace(
        "Gama, G #-",
        "Zeta, Z #WGM POL [2300] 1990.01.01\nGama, G #-",
    ));
    open(&mut rig, "Zeta, Z");
    let view = rig.state.player_info_view().unwrap();
    assert!(!view.found && view.stats.is_none());
    let header = view.header.expect("the file knows Zeta");
    assert_eq!(header.title_codes, ["WGM"]);
    assert_eq!(header.country_names, ["Poland"]);
    open(&mut rig, "Nobody, N");
    let view = rig.state.player_info_view().unwrap();
    assert!(!view.found && view.stats.is_none() && view.header.is_none());
}

#[test]
fn the_header_comes_from_the_spelling_file() {
    let (mut rig, _, _dir) = setup();
    rig.load_ssp(SSP);
    open(&mut rig, "Alfa, A");
    let header = rig
        .state
        .player_info_view()
        .unwrap()
        .header
        .expect("header");
    assert_eq!(header.title_codes, ["GM"]);
    assert_eq!(header.title_names, ["Grandmaster"]);
    assert_eq!(header.country_names, ["Brazil"]);
    assert_eq!((header.peak, header.peak_is_estimate), (2520, false));
    assert_eq!(header.born, "1980.01.01");
    assert_eq!(header.bio, ["FIDEID 1234567", "Champion"]);
    assert_eq!(
        header.fide_url.as_deref(),
        Some("https://ratings.fide.com/profile/1234567")
    );
    assert!(!header.approximate && header.candidates.is_empty());
    // An estimated peak keeps its mark.
    open(&mut rig, "Delta, D");
    let header = rig.state.player_info_view().unwrap().header.unwrap();
    assert!(header.peak_is_estimate);
    assert!(header.fide_url.is_none());
}

#[test]
fn a_loose_match_is_marked_approximate_with_its_candidates() {
    let (mut rig, _, _dir) = setup();
    rig.load_ssp(SSP);
    open(&mut rig, "Alfa");
    let header = rig
        .state
        .player_info_view()
        .unwrap()
        .header
        .expect("loose header");
    assert!(header.approximate);
    assert!(
        header.candidates.contains(&"Alfa, A".to_owned()),
        "{:?}",
        header.candidates
    );
    assert_eq!(header.title_codes, ["GM"]);
}

#[test]
fn the_links_set_the_filter_the_way_the_scid_does() {
    let (mut rig, slot, _dir) = setup();
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    open(&mut rig, "Alfa, A");
    let cases: [(&str, &[u32]); 6] = [
        ("-faA", &[1, 3, 4, 5, 6]),
        ("-fwW", &[1, 3, 4]),
        ("-fw", &[1, 4]),
        ("-fW", &[3]),
        ("-fd", &[5]),
        ("-fA", &[3, 6]),
    ];
    for (code, want) in cases {
        rig.send(PlayerInfoMessage::FilterLink(code.to_owned()));
        assert_eq!(rig.filter_numbers(slot), want, "{code}");
    }
    // The opponent of the game on screen is Beta.
    rig.send(PlayerInfoMessage::FilterLink("-oaA".to_owned()));
    assert_eq!(rig.filter_numbers(slot), [1, 4]);
    assert_eq!(
        info_notice(&rig),
        Some(PlayerInfoNotice::FilterSet { games: 2 })
    );
    // "-F" alone keeps only the games of the player.
    rig.send(Message::Bases(BasesMessage::List(
        ListMessage::ResetFilter { slot },
    )));
    keep_games(&mut rig, slot, &[6]);
    rig.send(PlayerInfoMessage::FilterLink("-F".to_owned()));
    assert_eq!(
        rig.filter_numbers(slot),
        [1, 2, 3, 4, 5]
            .iter()
            .copied()
            .filter(|n| *n != 2)
            .collect::<Vec<_>>()
    );
    // The window counts again with the new filter.
    let stats = rig.state.player_info_view().unwrap().stats.unwrap();
    assert_eq!(stats.in_filter, Some(4));
    // A link that is not the window's is refused.
    let before = rig.filter_numbers(slot);
    let fx = rig.raw(PlayerInfoMessage::FilterLink("-fz".to_owned()));
    assert!(fx.is_empty());
    assert_eq!(info_notice(&rig), Some(PlayerInfoNotice::BadLink));
    rig.raw(PlayerInfoMessage::FilterLink("fw".to_owned()));
    assert_eq!(rig.filter_numbers(slot), before);
}

#[test]
fn a_change_of_the_filter_counts_again_after_a_short_wait() {
    let (mut rig, slot, _dir) = setup();
    open(&mut rig, "Alfa, A");
    rig.send(Message::Bases(BasesMessage::List(
        ListMessage::SelectNumbers {
            slot,
            numbers: vec![5, 6],
        },
    )));
    let fx = rig.raw(Message::Bases(BasesMessage::List(
        ListMessage::RemoveSelected { slot },
    )));
    let delay = fx
        .iter()
        .find_map(|e| match e {
            Effect::PlayerInfo(PlayerInfoEffect::Delay { request, ms }) => Some((*request, *ms)),
            _ => None,
        })
        .expect("the count waits");
    assert_eq!(delay.1, 200);
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::PlayerInfo(PlayerInfoEffect::Compute { .. }))),
        "no count before the wait ends"
    );
    let view = rig.state.player_info_view().unwrap();
    assert!(view.stale, "the old figures stay on show meanwhile");
    // A wait that is not the pending one does nothing.
    let none = rig.raw(PlayerInfoMessage::DelayElapsed(mychess_app::RequestId(
        delay.0.0 + 99,
    )));
    assert!(none.is_empty());
    let fx = rig.raw(PlayerInfoMessage::DelayElapsed(delay.0));
    let compute = fx
        .into_iter()
        .find(|e| matches!(e, Effect::PlayerInfo(PlayerInfoEffect::Compute { .. })))
        .expect("the count starts");
    let answer = rig.perform(compute);
    rig.raw(answer);
    let view = rig.state.player_info_view().unwrap();
    assert!(!view.stale);
    assert_eq!(view.stats.unwrap().in_filter, Some(3));
}

#[test]
fn update_counts_again_at_once() {
    let (mut rig, _, _dir) = setup();
    open(&mut rig, "Alfa, A");
    let fx = rig.raw(PlayerInfoMessage::Refresh);
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::PlayerInfo(PlayerInfoEffect::Compute { .. }))),
        "{fx:?}"
    );
}

#[test]
fn the_last_ten_players_are_remembered_newest_first() {
    let (mut rig, _, _dir) = setup();
    for i in 0..12 {
        open(&mut rig, &format!("Player {i}"));
    }
    let history = rig.state.player_info_view().unwrap().history;
    assert_eq!(history.len(), 10);
    assert_eq!(history[0], "Player 11");
    assert_eq!(history[9], "Player 2");
    open(&mut rig, "Player 5");
    let history = rig.state.player_info_view().unwrap().history;
    assert_eq!(history[0], "Player 5");
    assert_eq!(history.iter().filter(|n| *n == "Player 5").count(), 1);
    assert_eq!(rig.state.prefs.player_tools.history, history);
}

#[test]
fn an_empty_name_repeats_the_last_player_or_says_there_is_none() {
    let (mut rig, _, _dir) = setup();
    open(&mut rig, "");
    assert_eq!(info_notice(&rig), Some(PlayerInfoNotice::NoPlayer));
    assert!(rig.state.player_info_view().is_none());
    open(&mut rig, "Beta, B");
    rig.send(PlayerInfoMessage::Close);
    assert!(rig.state.player_info_view().is_none());
    open(&mut rig, "  ");
    assert_eq!(rig.state.player_info().name(), "Beta, B");
    assert!(rig.state.player_info_view().is_some());
}

#[test]
fn the_fide_link_opens_only_what_the_file_gives() {
    let (mut rig, _, _dir) = setup();
    rig.load_ssp(SSP);
    open(&mut rig, "Alfa, A");
    let fx = rig.raw(PlayerInfoMessage::OpenFide);
    assert_eq!(
        fx,
        [Effect::PlayerInfo(PlayerInfoEffect::OpenUrl {
            url: "https://ratings.fide.com/profile/1234567".to_owned()
        })]
    );
    open(&mut rig, "Gama, G");
    let fx = rig.raw(PlayerInfoMessage::OpenFide);
    assert!(fx.is_empty());
    assert_eq!(info_notice(&rig), Some(PlayerInfoNotice::NoFide));
    rig.raw(PlayerInfoMessage::UrlOpened { ok: false });
    assert_eq!(info_notice(&rig), Some(PlayerInfoNotice::FideFailed));
}

#[test]
fn the_photo_of_the_library_arrives_as_bytes() {
    let (mut rig, _, _dir) = setup();
    let photos = rig.data.path().join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    std::fs::write(photos.join("Alfa, A.gif"), b"GIF89a-photo-bytes").unwrap();
    rig.load_ssp(SSP);
    open(&mut rig, "Alfa, A");
    let photo = rig.state.player_info_view().unwrap().photo.expect("photo");
    assert_eq!(photo.format, PhotoFormat::Gif);
    assert_eq!(&*photo.bytes, b"GIF89a-photo-bytes");
    open(&mut rig, "Beta, B");
    assert!(
        rig.state.player_info_view().unwrap().photo.is_none(),
        "no photo, nothing shown"
    );
}

#[test]
fn a_photo_that_is_not_an_image_is_not_shown() {
    let (mut rig, _, _dir) = setup();
    let photos = rig.data.path().join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    std::fs::write(photos.join("Beta, B.gif"), b"not an image").unwrap();
    rig.load_ssp(SSP);
    open(&mut rig, "Beta, B");
    assert!(rig.state.player_info_view().unwrap().photo.is_none());
}

#[test]
fn the_window_follows_the_player_and_drops_late_answers() {
    let (mut rig, _, _dir) = setup();
    let first = rig.raw(PlayerInfoMessage::Open {
        name: "Alfa, A".to_owned(),
    });
    let Some(Effect::PlayerInfo(PlayerInfoEffect::Compute { control, .. })) = first.first() else {
        panic!("compute: {first:?}");
    };
    let second = rig.raw(PlayerInfoMessage::Open {
        name: "Beta, B".to_owned(),
    });
    assert!(control.is_cancelled(), "a new player stops the old count");
    let old = rig.perform(first[0].clone());
    rig.raw(old);
    assert!(
        rig.state.player_info_view().unwrap().stats.is_none(),
        "the old answer was dropped"
    );
    let new = rig.perform(second[0].clone());
    rig.raw(new);
    let view = rig.state.player_info_view().unwrap();
    assert_eq!(view.name, "Beta, B");
    assert_eq!(view.stats.unwrap().games, 3);
}

#[test]
fn closing_and_quitting_stop_the_count() {
    let (mut rig, _, _dir) = setup();
    let fx = rig.raw(PlayerInfoMessage::Open {
        name: "Alfa, A".to_owned(),
    });
    let Some(Effect::PlayerInfo(PlayerInfoEffect::Compute { control, .. })) = fx.first() else {
        panic!("compute");
    };
    rig.raw(Message::Quit);
    assert!(control.is_cancelled());
    let (mut rig, _, _dir) = setup();
    let fx = rig.raw(PlayerInfoMessage::Open {
        name: "Alfa, A".to_owned(),
    });
    let Some(Effect::PlayerInfo(PlayerInfoEffect::Compute { control, .. })) = fx.first() else {
        panic!("compute");
    };
    rig.raw(PlayerInfoMessage::Close);
    assert!(control.is_cancelled());
    rig.raw(PlayerInfoMessage::Cancel);
}

#[test]
fn a_new_spelling_file_counts_again() {
    let (mut rig, _, _dir) = setup();
    open(&mut rig, "Alfa, A");
    assert!(rig.state.player_info_view().unwrap().header.is_none());
    rig.load_ssp(SSP);
    assert!(rig.state.player_info_view().unwrap().header.is_some());
    rig.send(SpellingMessage::Unload);
    assert!(rig.state.player_info_view().unwrap().header.is_none());
}

#[test]
fn the_player_buttons_open_the_finders() {
    let (mut rig, _, _dir) = setup();
    open(&mut rig, "Alfa, A");
    rig.send(PlayerInfoMessage::LookupName);
    assert_eq!(rig.state.player_finder_view().unwrap().form.prefix, "Alfa,");
    rig.send(PlayerInfoMessage::OpenTournaments);
    assert_eq!(
        rig.state
            .tournament_finder_view()
            .unwrap()
            .player
            .as_deref(),
        Some("Alfa, A")
    );
}
