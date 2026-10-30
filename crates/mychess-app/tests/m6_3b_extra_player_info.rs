//! Extra contract tests of the player information window in the application
//! layer (spec `m6-3-jogadores-torneios-spelling.md`, slice M6-3b; section 3
//! and rules 81 to 90, D3318 to D3323): hostile names and links, stale and
//! cancelled answers, quitting, closed or rewritten databases, the Clipbase,
//! a PGN and the FIDE link limits.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

mod m6_3b_common;

use std::fmt::Write as _;

use m6_3b_common::*;
use mychess_app::PlayerInfoEffect;

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

// ---- hostile names ----

#[test]
fn odd_names_open_a_window_with_no_games_and_never_panic() {
    let (mut rig, _, _dir) = setup();
    let names = [
        "?".to_owned(),
        "\u{0}".to_owned(),
        "\u{202e}Alfa".to_owned(),
        "Alfa, A\u{0}".to_owned(),
        "\u{1F600}".to_owned(),
        "%s %d {} \"quoted\"".to_owned(),
        "x".repeat(3_000),
    ];
    for name in &names {
        open(&mut rig, name);
        let view = rig.state.player_info_view().expect("window open");
        assert!(view.stats.is_none() || view.stats.as_ref().unwrap().games == 0);
    }
    // A name that is not in the database says "0 games" (rule 6 of 3.2):
    // no statistics rows, the window is still usable.
    open(&mut rig, "Alfa, A");
    assert_eq!(
        rig.state.player_info_view().unwrap().stats.unwrap().games,
        5
    );
}

#[test]
fn a_giant_name_does_not_fill_the_history_that_is_saved_in_the_preferences() {
    let (mut rig, _, _dir) = setup();
    open(&mut rig, &"N".repeat(5_000));
    let history = rig.state.prefs.player_tools.history.clone();
    assert!(
        history.iter().all(|n| n.chars().count() <= 4096),
        "a hostile name must not grow the preferences file without limit"
    );
}

#[test]
fn names_that_differ_only_by_surrounding_blanks_share_one_history_entry() {
    let (mut rig, _, _dir) = setup();
    open(&mut rig, "Alfa, A");
    open(&mut rig, "  Alfa, A \t");
    open(&mut rig, "Beta, B");
    let history = rig.state.prefs.player_tools.history.clone();
    assert_eq!(history, ["Beta, B", "Alfa, A"]);
    assert_eq!(rig.state.player_info().name(), "Beta, B");
}

#[test]
fn a_blank_name_with_no_last_player_and_an_unnamed_game_says_there_is_none() {
    let (mut rig, _, _dir) = setup();
    for blank in ["", " ", "\t\n"] {
        let fx = rig.raw(PlayerInfoMessage::Open {
            name: blank.to_owned(),
        });
        assert!(fx.is_empty(), "{fx:?}");
        assert_eq!(info_notice(&rig), Some(PlayerInfoNotice::NoPlayer));
    }
    assert!(rig.state.player_info_view().is_none());
}

#[test]
fn opening_with_only_the_empty_clipbase_finds_nothing_and_survives_every_button() {
    let mut rig = Rig::new();
    rig.send(PlayerInfoMessage::Open {
        name: "Alfa, A".to_owned(),
    });
    let view = rig.state.player_info_view().unwrap();
    assert!(view.stats.is_none());
    rig.send(PlayerInfoMessage::Refresh);
    rig.raw(PlayerInfoMessage::OpenFide);
    rig.send(PlayerInfoMessage::FilterLink("-faA".to_owned()));
    rig.send(PlayerInfoMessage::LookupName);
    rig.send(PlayerInfoMessage::OpenTournaments);
    assert!(rig.state.player_info_view().is_some());
}

// ---- hostile links ----

#[test]
fn malformed_link_codes_are_refused_and_leave_the_filter_alone() {
    let (mut rig, slot, _dir) = setup();
    open(&mut rig, "Alfa, A");
    let before = rig.filter_numbers(slot);
    let bad = [
        "",
        "-",
        "f",
        "fwW",
        "--f",
        "-x",
        "-fq",
        "-fwWq",
        "-fw W",
        "-fw\n",
        "-\u{ff46}w",
        "-oz",
        "-Fz",
        "- fw",
        "-fwW\u{0}",
    ];
    for code in bad {
        let fx = rig.raw(PlayerInfoMessage::FilterLink(code.to_owned()));
        assert!(fx.is_empty(), "{code:?}: {fx:?}");
        assert_eq!(
            info_notice(&rig),
            Some(PlayerInfoNotice::BadLink),
            "{code:?}"
        );
        assert_eq!(rig.filter_numbers(slot), before, "{code:?}");
    }
    let giant = format!("-f{}", "w".repeat(500_000));
    rig.raw(PlayerInfoMessage::FilterLink(giant));
}

#[test]
fn a_link_with_no_player_open_changes_nothing() {
    let (mut rig, slot, _dir) = setup();
    let before = rig.filter_numbers(slot);
    let fx = rig.raw(PlayerInfoMessage::FilterLink("-faA".to_owned()));
    assert!(fx.is_empty());
    assert_eq!(info_notice(&rig), Some(PlayerInfoNotice::NoPlayer));
    assert_eq!(rig.filter_numbers(slot), before);
}

#[test]
fn the_opponent_link_without_an_opponent_on_screen_empties_the_filter_like_the_scid() {
    // Rule 88: "-o" empties the filter and adds the games against the
    // opponent of the current game; with no game on screen there is none.
    let (mut rig, slot, _dir) = setup();
    open(&mut rig, "Alfa, A");
    rig.send(PlayerInfoMessage::FilterLink("-oaA".to_owned()));
    assert!(rig.filter_numbers(slot).is_empty());
}

#[test]
fn a_filter_answer_that_arrives_after_the_database_was_rewritten_is_refused() {
    let (mut rig, slot, _dir) = setup();
    open(&mut rig, "Alfa, A");
    let before = rig.filter_numbers(slot);
    let link = rig.raw(PlayerInfoMessage::FilterLink("-fw".to_owned()));
    let Effect::PlayerInfo(PlayerInfoEffect::SetFilter { .. }) = &link[0] else {
        panic!("filter effect: {link:?}");
    };
    let answer = rig.perform(link[0].clone());
    // The names of the slot are rewritten before the answer is handled.
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(mychess_app::NameKind::Event));
    rig.send(SpellingMessage::Apply);
    rig.send(SpellingMessage::ConfirmApply(true));
    rig.raw(answer);
    assert_eq!(
        info_notice(&rig),
        Some(PlayerInfoNotice::Stale),
        "{:?}",
        rig.state.notice
    );
    assert_eq!(rig.filter_numbers(slot), before.clone());
}

#[test]
fn a_filter_answer_for_a_closed_slot_is_harmless() {
    let (mut rig, slot, _dir) = setup();
    open(&mut rig, "Alfa, A");
    let link = rig.raw(PlayerInfoMessage::FilterLink("-fw".to_owned()));
    rig.raw(BasesMessage::Close(slot));
    let answer = rig.perform(link[0].clone());
    rig.raw(answer);
    assert!(rig.state.bases.slot(slot).is_none());
}

#[test]
fn quitting_cancels_the_filter_job_and_its_answer_changes_nothing() {
    let (mut rig, slot, _dir) = setup();
    open(&mut rig, "Alfa, A");
    let before = rig.filter_numbers(slot);
    let link = rig.raw(PlayerInfoMessage::FilterLink("-fw".to_owned()));
    let Effect::PlayerInfo(PlayerInfoEffect::SetFilter { control, .. }) = &link[0] else {
        panic!("filter effect");
    };
    rig.raw(Message::Quit);
    assert!(control.is_cancelled());
    let answer = rig.perform(link[0].clone());
    rig.raw(answer);
    assert_eq!(
        rig.filter_numbers(slot),
        before,
        "a cancelled link sets no filter"
    );
}

#[test]
fn a_second_link_replaces_the_first_and_the_old_answer_is_dropped() {
    let (mut rig, slot, _dir) = setup();
    open(&mut rig, "Alfa, A");
    let first = rig.raw(PlayerInfoMessage::FilterLink("-fw".to_owned()));
    let Effect::PlayerInfo(PlayerInfoEffect::SetFilter { control, .. }) = &first[0] else {
        panic!("effect");
    };
    let second = rig.raw(PlayerInfoMessage::FilterLink("-fd".to_owned()));
    assert!(control.is_cancelled());
    let a = rig.perform(first[0].clone());
    let b = rig.perform(second[0].clone());
    rig.raw(b);
    rig.raw(a);
    assert_eq!(rig.filter_numbers(slot), [5], "draws: the second link");
}

// ---- stale answers and the window life ----

#[test]
fn closing_the_window_drops_a_late_count_and_reopening_counts_again() {
    let (mut rig, _, _dir) = setup();
    let fx = rig.raw(PlayerInfoMessage::Open {
        name: "Alfa, A".to_owned(),
    });
    rig.raw(PlayerInfoMessage::Close);
    let answer = rig.perform(fx[0].clone());
    let more = rig.raw(answer);
    assert!(more.iter().all(|e| !matches!(e, Effect::PlayerInfo(_))));
    assert!(rig.state.player_info_view().is_none());
    open(&mut rig, "Alfa, A");
    assert_eq!(
        rig.state.player_info_view().unwrap().stats.unwrap().games,
        5
    );
}

#[test]
fn after_stopping_the_count_refresh_counts_again() {
    let (mut rig, _, _dir) = setup();
    let fx = rig.raw(PlayerInfoMessage::Open {
        name: "Alfa, A".to_owned(),
    });
    rig.raw(PlayerInfoMessage::Cancel);
    let answer = rig.perform(fx[0].clone());
    rig.raw(answer);
    rig.send(PlayerInfoMessage::Refresh);
    assert_eq!(
        rig.state.player_info_view().unwrap().stats.map(|s| s.games),
        Some(5),
        "a stopped count must not leave the window empty for good"
    );
}

#[test]
fn closing_the_database_under_the_window_leaves_it_without_statistics() {
    let (mut rig, slot, _dir) = setup();
    open(&mut rig, "Alfa, A");
    assert!(rig.state.player_info_view().unwrap().stats.is_some());
    rig.send(BasesMessage::Close(slot));
    let view = rig.state.player_info_view().unwrap();
    assert!(
        view.stats.is_none_or(|s| s.games == 0),
        "the Clipbase takes the place: no games of that player there"
    );
}

#[test]
fn a_late_count_of_the_previous_database_is_not_shown_after_switching() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (six_slot, _) = rig.open_si4(dir.path(), "six", &six());
    rig.open_si4(
        dir.path(),
        "few",
        &[g("X", "Y", "2001.01.01", "Alfa, A", "Yan, Y", "1-0", 0, 0)],
    );
    let old = rig.raw(PlayerInfoMessage::Open {
        name: "Alfa, A".to_owned(),
    });
    let new = rig.raw(BasesMessage::Select(six_slot));
    let new_count = new
        .iter()
        .find(|e| matches!(e, Effect::PlayerInfo(PlayerInfoEffect::Compute { .. })))
        .expect("a new count")
        .clone();
    let old_answer = rig.perform(old[0].clone());
    let new_answer = rig.perform(new_count);
    rig.raw(new_answer);
    rig.raw(old_answer);
    assert_eq!(
        rig.state.player_info_view().unwrap().stats.unwrap().games,
        5,
        "the count of the one-game base must not replace the right one"
    );
}

#[test]
fn rewriting_names_makes_the_count_follow_the_new_names() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, _) = rig.open_si4(
        dir.path(),
        "m",
        &[
            g("X", "Y", "2020.01.01", "Alfa, Alf", "Beta, B", "1-0", 0, 0),
            g("X", "Y", "2020.01.02", "Alfa, A", "Beta, B", "1-0", 0, 0),
        ],
    );
    rig.load_ssp(SSP);
    open(&mut rig, "Alfa, A");
    assert_eq!(
        rig.state.player_info_view().unwrap().stats.unwrap().games,
        1
    );
    rig.send(SpellingMessage::OpenCheck(mychess_app::NameKind::Player));
    rig.send(SpellingMessage::Apply);
    rig.send(SpellingMessage::ConfirmApply(true));
    assert_eq!(
        rig.state.player_info_view().unwrap().stats.unwrap().games,
        2,
        "rule 58: the information is invalidated after the correction"
    );
}

// ---- other kinds of base ----

#[test]
fn a_pgn_gives_the_same_figures_as_the_si4() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "six.pgn", &six());
    open(&mut rig, "Alfa, A");
    let stats = rig.state.player_info_view().unwrap().stats.unwrap();
    assert_eq!(stats.games, 5);
    assert_eq!(stats.all.total.games, 5);
}

#[test]
fn the_clipbase_counts_again_when_it_grows() {
    let (mut rig, slot, _dir) = setup();
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::ConfirmCopy(
        false,
    ))));
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    open(&mut rig, "Alfa, A");
    let games = |rig: &Rig| rig.state.player_info_view().unwrap().stats.map(|s| s.games);
    assert!(games(&rig).is_none_or(|g| g == 0));
    rig.send(Message::Bases(BasesMessage::List(ListMessage::CopyGames {
        from: slot,
        to: SlotId::CLIPBASE,
        scope: mychess_app::CopyScope::Filter,
    })));
    assert_eq!(games(&rig), Some(5));
}

// ---- the header and the FIDE link (rule 90) ----

fn ssp_with_fide(note: &str) -> String {
    format!(
        "@PLAYER \"., -_*\"\nAlfa, A #GM BRA [2520] 1980.01.01\n  %Bio {note}\n  %Bio second note\n"
    )
}

#[test]
fn the_fide_link_takes_only_one_to_twelve_digits() {
    let cases = [
        ("FIDEID 1", Some("https://ratings.fide.com/profile/1")),
        (
            "FIDEID 123456789012",
            Some("https://ratings.fide.com/profile/123456789012"),
        ),
        ("FIDEID 1234567890123", None),
        ("FIDEID abc", None),
        ("FIDEID 12ab", None),
        ("FIDEID ", None),
        ("FIDEID ../../x", None),
        ("FIDEID 12/../34", None),
    ];
    for (note, want) in cases {
        let (mut rig, _, _dir) = setup();
        rig.load_ssp(&ssp_with_fide(note));
        open(&mut rig, "Alfa, A");
        let header = rig
            .state
            .player_info_view()
            .unwrap()
            .header
            .expect("header");
        assert_eq!(header.fide_url.as_deref(), want, "{note:?}");
        let fx = rig.raw(PlayerInfoMessage::OpenFide);
        assert_eq!(fx.len(), usize::from(want.is_some()), "{note:?}");
    }
}

#[test]
fn a_hostile_spelling_comment_never_breaks_the_header() {
    let comments = [
        "",
        "#",
        "- - -",
        "GM",
        "gm+cim+zzz+ BRA/ARG/USA/XXX [99999*] ????",
        "- ENG/NZL [....] 1904.??.??--1979.08.15",
        "\u{1F600} \u{1F600} [\u{1F600}] \u{1F600}",
        "GM BRA [2520",
        "GM BRA ]2520[ 1980",
        "GM BRA [2520] 99999.99.99--",
    ];
    for c in comments {
        let (mut rig, _, _dir) = setup();
        rig.load_ssp(&format!("@PLAYER \"\"\nAlfa, A #{c}\n"));
        open(&mut rig, "Alfa, A");
        let view = rig.state.player_info_view().expect("window");
        assert_eq!(view.stats.unwrap().games, 5, "{c:?}");
    }
}

#[test]
fn a_huge_biography_is_capped_in_the_window() {
    let mut text = String::from("@PLAYER \"\"\nAlfa, A #GM BRA [2520]\n");
    for i in 0..1200 {
        let _ = writeln!(text, "%Bio note {i}");
    }
    let (mut rig, _, _dir) = setup();
    rig.load_ssp(&text);
    open(&mut rig, "Alfa, A");
    let header = rig.state.player_info_view().unwrap().header.unwrap();
    assert!(
        header.bio.len() <= 1000,
        "rule 14: at most 1000 notes per name"
    );
}

#[test]
fn a_name_of_100_thousand_characters_is_answered_in_seconds() {
    let (mut rig, _, _dir) = setup();
    let started = std::time::Instant::now();
    open(&mut rig, &"N".repeat(100_000));
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
}
