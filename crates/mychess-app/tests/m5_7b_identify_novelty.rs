//! M5-7b contract: "Identify opening" and "Find novelty" (spec
//! `docs/specs/m5-7-eco-e-novidade.md`, D1717-D1720, rule 5).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_7b_common;

use m5_7b_common::*;
use mychess_core::Color;
use tempfile::tempdir;

const RUY: [&str; 6] = ["e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6"];
const RUY_NF6: [&str; 6] = ["e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "g8f6"];

// ---- identify opening ----

#[test]
fn identify_goes_to_the_deepest_opening_of_the_main_line_and_tells_it() {
    let mut rig = Rig::new();
    rig.set_book(BOOK);
    rig.play_all(&RUY);
    assert_eq!(rig.ply(), 6);
    rig.send(eco(EcoMessage::Identify));
    assert_eq!(rig.ply(), 3, "1.e4 e5 2.Nf3 is the deepest entry");
    assert_eq!(rig.mainline_len(), 6, "the game is not changed");
    assert_eq!(
        rig.notice(),
        Some(EcoNotice::Opening {
            eco: "C40".to_owned(),
            name: "King's Knight Opening".to_owned(),
            ply: 3
        })
    );
}

#[test]
fn identify_follows_transpositions_by_position() {
    let mut rig = Rig::new();
    rig.set_book(BOOK);
    rig.play_all(&["g1f3", "e7e5", "e2e4", "b8c6"]);
    rig.send(eco(EcoMessage::Identify));
    assert_eq!(rig.ply(), 3);
    assert!(matches!(
        rig.notice(),
        Some(EcoNotice::Opening { ply: 3, .. })
    ));
}

#[test]
fn identify_with_no_match_tells_so_and_does_not_move() {
    let mut rig = Rig::new();
    rig.set_book("B20 \"Sicilian Defence\" 1.e4 c5 *\nC40 \"X\" 1.e4 e5 2.Nf3 *\n");
    rig.play_all(&["d2d4", "d7d5"]);
    rig.send(eco(EcoMessage::Identify));
    assert_eq!(rig.notice(), Some(EcoNotice::NoOpening));
    assert_eq!(rig.ply(), 2);
}

#[test]
fn identify_without_a_book_or_with_an_empty_one_tells_no_book() {
    let mut rig = Rig::new();
    rig.play_all(&["e2e4", "e7e5"]);
    rig.send(eco(EcoMessage::Identify));
    assert_eq!(rig.notice(), Some(EcoNotice::NoBook));
    assert_eq!(rig.ply(), 2);
    rig.clear_notice();
    rig.state.eco_book = Some(SharedEco::new(mychess_core::EcoBook::default()));
    rig.send(eco(EcoMessage::Identify));
    assert_eq!(rig.notice(), Some(EcoNotice::NoBook));
}

#[test]
fn identify_on_the_empty_game_matches_the_start_entry() {
    let mut rig = Rig::new();
    rig.set_book(BOOK);
    rig.send(eco(EcoMessage::Identify));
    assert!(matches!(
        rig.notice(),
        Some(EcoNotice::Opening { ply: 0, .. })
    ));
}

#[test]
fn identify_is_ignored_while_a_dialog_is_open() {
    let mut rig = Rig::new();
    rig.set_book(BOOK);
    rig.play_all(&RUY);
    rig.raw(eco(EcoMessage::OpenClassify));
    let effects = rig.raw(eco(EcoMessage::Identify));
    assert!(effects.is_empty());
    assert_eq!(rig.ply(), 6);
    assert_eq!(rig.notice(), None);
}

#[test]
fn identify_only_looks_at_the_main_line() {
    let mut rig = Rig::new();
    rig.set_book(BOOK);
    // Main line 1.d4; a variation 1.e4 e5 2.Nf3 does not count.
    rig.play("d2d4");
    rig.send(GameMessage::Navigate(Nav::Start));
    rig.play_variation("e2e4");
    rig.play_all(&["e7e5", "g1f3"]);
    assert_eq!(rig.mainline_len(), 1, "the moves went into the variation");
    rig.send(eco(EcoMessage::Identify));
    assert!(matches!(
        rig.notice(),
        Some(EcoNotice::Opening { ply: 0, .. })
    ));
}

// ---- novelty: the dialog ----

fn base_with(dir: &std::path::Path, name: &str, games: &[(&str, &str)]) -> (Rig, SlotId) {
    let mut rig = Rig::new();
    let slot = open_more(&mut rig, dir, name, games);
    (rig, slot)
}

fn open_more(rig: &mut Rig, dir: &std::path::Path, name: &str, games: &[(&str, &str)]) -> SlotId {
    let mut pgn = String::new();
    for (moves, date) in games {
        pgn.push_str(&game_pgn(moves, "1-0", date, None));
    }
    let slot = rig.open_pgn(dir, name, &pgn);
    rig.fresh_game();
    slot
}

fn found(rig: &Rig) -> Option<NoveltyOutcome> {
    rig.state.eco_novelty_view().unwrap().result.cloned()
}

#[test]
fn the_example_of_rule_5_finds_3_nf6() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = base_with(
        dir.path(),
        "ref.pgn",
        &[("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6", "2020.01.01")],
    );
    rig.play_all(&RUY_NF6);
    rig.raw(eco(EcoMessage::OpenNovelty));
    rig.send(eco(EcoMessage::FindNovelty));
    assert_eq!(
        found(&rig),
        Some(NoveltyOutcome::Found {
            ply: 6,
            san: "Nf6".to_owned(),
            move_number: 3,
            color: Color::Black
        })
    );
    // The cursor goes to the position before the move (D1720).
    assert_eq!(rig.ply(), 5);
    assert_eq!(
        rig.notice(),
        Some(EcoNotice::Novelty {
            move_number: 3,
            color: Color::Black,
            san: "Nf6".to_owned()
        })
    );
    assert!(rig.state.list_job().is_none());
    assert_eq!(
        rig.state.eco_window(),
        Some(EcoWindow::Novelty),
        "dialog stays"
    );
}

#[test]
fn the_same_example_with_a_book_loaded_gives_the_same_novelty() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = base_with(
        dir.path(),
        "ref.pgn",
        &[("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6", "2020.01.01")],
    );
    rig.set_book(BOOK);
    rig.play_all(&RUY_NF6);
    rig.raw(eco(EcoMessage::OpenNovelty));
    rig.send(eco(EcoMessage::FindNovelty));
    assert!(matches!(
        found(&rig),
        Some(NoveltyOutcome::Found { ply: 6, .. })
    ));
}

#[test]
fn a_novelty_by_white_is_reported_with_one_dot() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = base_with(
        dir.path(),
        "ref.pgn",
        &[("1. e4 e5 2. Nf3 Nc6", "2020.01.01")],
    );
    rig.play_all(&["e2e4", "e7e5", "g1f3", "b8c6", "f1c4"]);
    rig.raw(eco(EcoMessage::OpenNovelty));
    rig.send(eco(EcoMessage::FindNovelty));
    assert_eq!(
        found(&rig),
        Some(NoveltyOutcome::Found {
            ply: 5,
            san: "Bc4".to_owned(),
            move_number: 3,
            color: Color::White
        })
    );
    assert_eq!(rig.ply(), 4);
}

#[test]
fn a_game_already_in_the_base_has_no_novelty_and_the_cursor_stays() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = base_with(
        dir.path(),
        "ref.pgn",
        &[("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6", "2020.01.01")],
    );
    rig.play_all(&RUY);
    rig.raw(eco(EcoMessage::OpenNovelty));
    rig.send(eco(EcoMessage::FindNovelty));
    assert_eq!(found(&rig), Some(NoveltyOutcome::NoNovelty));
    assert_eq!(rig.ply(), 6);
    assert_eq!(rig.notice(), Some(EcoNotice::NoNovelty));
}

#[test]
fn a_game_without_moves_has_no_novelty_and_starts_no_job() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = base_with(dir.path(), "ref.pgn", &[("1. e4", "2020.01.01")]);
    rig.raw(eco(EcoMessage::OpenNovelty));
    let view = rig.state.eco_novelty_view().unwrap();
    assert!(!view.has_moves);
    let effects = rig.raw(eco(EcoMessage::FindNovelty));
    assert_eq!(count(&effects, is_novelty), 0);
    assert!(rig.state.list_job().is_none());
    assert_eq!(found(&rig), Some(NoveltyOutcome::NoNovelty));
}

#[test]
fn nothing_in_the_base_passes_through_the_start_so_the_first_move_is_new() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = base_with(dir.path(), "ref.pgn", &[("1. d4 d5", "2020.01.01")]);
    rig.play_all(&["e2e4"]);
    rig.raw(eco(EcoMessage::OpenNovelty));
    rig.send(eco(EcoMessage::FindNovelty));
    assert_eq!(
        found(&rig),
        Some(NoveltyOutcome::Found {
            ply: 1,
            san: "e4".to_owned(),
            move_number: 1,
            color: Color::White
        })
    );
    assert_eq!(rig.ply(), 0);
}

#[test]
fn a_game_that_starts_from_its_own_position_has_no_search() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let pgn = "[Event \"T\"]\n[Date \"2020.01.01\"]\n[Result \"*\"]\n[SetUp \"1\"]\n\
        [FEN \"4k3/8/8/8/8/8/4P3/4K3 w - - 0 1\"]\n\n1. e4 Kd7 *\n\n";
    let slot = rig.open_pgn(dir.path(), "fen.pgn", pgn);
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    assert_eq!(rig.mainline_len(), 2);
    rig.raw(eco(EcoMessage::OpenNovelty));
    rig.send(eco(EcoMessage::FindNovelty));
    assert_eq!(found(&rig), Some(NoveltyOutcome::NonStandardStart));
    assert_eq!(rig.notice(), Some(EcoNotice::NoveltyNonStandard));
}

// ---- reference database ----

#[test]
fn the_reference_defaults_to_the_current_base_and_can_be_any_open_slot_clipbase_included() {
    let dir = tempdir().unwrap();
    let (mut rig, first) = base_with(
        dir.path(),
        "ref.pgn",
        &[("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6", "2020.01.01")],
    );
    let second = open_more(
        &mut rig,
        dir.path(),
        "other.pgn",
        &[("1. d4 d5", "2020.01.01")],
    );
    assert_ne!(first, second);
    assert_eq!(rig.state.bases.current(), second);
    rig.play_all(&RUY_NF6);
    rig.raw(eco(EcoMessage::OpenNovelty));
    let view = rig.state.eco_novelty_view().unwrap();
    assert_eq!(view.reference, second);
    assert!(view.references.contains(&first));
    assert!(view.references.contains(&second));
    assert!(
        view.references.iter().any(|s| s.is_clipbase()),
        "Clipbase listed"
    );
    assert!(!view.older_only);
    // The current base does not know 1.e4: the first move is new.
    rig.send(eco(EcoMessage::FindNovelty));
    assert!(matches!(
        found(&rig),
        Some(NoveltyOutcome::Found { ply: 1, .. })
    ));
    // Choose the other base: the result is cleared and the answer changes.
    rig.raw(eco(EcoMessage::SetNoveltyReference(first)));
    assert_eq!(rig.state.eco_novelty_view().unwrap().reference, first);
    assert!(found(&rig).is_none());
    rig.send(eco(EcoMessage::FindNovelty));
    assert!(matches!(
        found(&rig),
        Some(NoveltyOutcome::Found { ply: 6, .. })
    ));
    assert_eq!(rig.ply(), 5);
}

#[test]
fn a_slot_that_is_not_in_use_cannot_be_chosen() {
    let dir = tempdir().unwrap();
    let (mut rig, first) = base_with(dir.path(), "ref.pgn", &[("1. e4", "2020.01.01")]);
    rig.raw(eco(EcoMessage::OpenNovelty));
    for n in 2..=8 {
        rig.raw(eco(EcoMessage::SetNoveltyReference(sel(n))));
        assert_eq!(rig.state.eco_novelty_view().unwrap().reference, first);
    }
}

#[test]
fn the_clipbase_can_be_the_reference() {
    let dir = tempdir().unwrap();
    let (mut rig, first) = base_with(
        dir.path(),
        "ref.pgn",
        &[("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6", "2020.01.01")],
    );
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::ConfirmCopy(
        false,
    ))));
    rig.send(BasesMessage::List(ListMessage::CopyGames {
        from: first,
        to: sel(9),
        scope: CopyScope::Filter,
    }));
    let clip = sel(9);
    assert_eq!(
        rig.state.bases.slot(clip).unwrap().total(),
        1,
        "the game was copied"
    );
    let _other = open_more(
        &mut rig,
        dir.path(),
        "other.pgn",
        &[("1. d4 d5", "2020.01.01")],
    );
    rig.play_all(&RUY_NF6);
    rig.raw(eco(EcoMessage::OpenNovelty));
    rig.raw(eco(EcoMessage::SetNoveltyReference(clip)));
    assert_eq!(rig.state.eco_novelty_view().unwrap().reference, clip);
    rig.send(eco(EcoMessage::FindNovelty));
    assert!(matches!(
        found(&rig),
        Some(NoveltyOutcome::Found { ply: 6, .. })
    ));
}

// ---- older only and the game itself ----

fn three_games() -> [(&'static str, &'static str); 3] {
    [
        ("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6", "2020.01.01"),
        ("1. e4 e5 2. Nf3 Nc6 3. Bb5 Nf6", "2021.01.01"),
        ("1. e4 e5 2. Nf3 Nc6 3. Bb5 Nf6 4. O-O", "2022.01.01"),
    ]
}

#[test]
fn only_older_games_ignores_the_games_of_the_same_date_or_later() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = base_with(dir.path(), "ref.pgn", &three_games());
    // The editor holds game 2 (2021), which came from this very base.
    rig.send(BasesMessage::LoadGame { slot, number: 2 });
    assert_eq!(rig.mainline_len(), 6);
    rig.raw(eco(EcoMessage::OpenNovelty));
    let view = rig.state.eco_novelty_view().unwrap();
    assert_eq!(view.game_date.year(), Some(2021));
    assert!(view.has_moves);
    // Game 3 (2022) plays the whole line: no novelty.
    rig.send(eco(EcoMessage::FindNovelty));
    assert_eq!(found(&rig), Some(NoveltyOutcome::NoNovelty));
    // Only older games: game 3 is out and 3...Nf6 is new against game 1.
    rig.raw(eco(EcoMessage::SetNoveltyOlderOnly(true)));
    assert!(
        found(&rig).is_none(),
        "changing the option clears the result"
    );
    rig.send(eco(EcoMessage::FindNovelty));
    assert!(matches!(
        found(&rig),
        Some(NoveltyOutcome::Found { ply: 6, .. })
    ));
}

#[test]
fn the_game_of_the_editor_is_not_found_played_before_by_itself() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = base_with(
        dir.path(),
        "ref.pgn",
        &[
            ("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6", "2020.01.01"),
            ("1. e4 e5 2. Nf3 Nc6 3. Bb5 Nf6", "2021.01.01"),
        ],
    );
    rig.send(BasesMessage::LoadGame { slot, number: 2 });
    rig.raw(eco(EcoMessage::OpenNovelty));
    rig.send(eco(EcoMessage::FindNovelty));
    assert!(
        matches!(found(&rig), Some(NoveltyOutcome::Found { ply: 6, .. })),
        "{:?}",
        found(&rig)
    );
}

#[test]
fn a_copy_of_the_game_in_another_base_is_not_excluded() {
    let dir = tempdir().unwrap();
    let (mut rig, first) = base_with(
        dir.path(),
        "a.pgn",
        &[("1. e4 e5 2. Nf3 Nc6 3. Bb5 Nf6", "2021.01.01")],
    );
    let second = open_more(
        &mut rig,
        dir.path(),
        "b.pgn",
        &[("1. e4 e5 2. Nf3 Nc6 3. Bb5 Nf6", "2021.01.01")],
    );
    rig.send(BasesMessage::LoadGame {
        slot: first,
        number: 1,
    });
    // The editor game came from `first`; the reference is `second`, whose
    // game 1 is another game with the same moves.
    rig.raw(eco(EcoMessage::OpenNovelty));
    rig.raw(eco(EcoMessage::SetNoveltyReference(second)));
    rig.send(eco(EcoMessage::FindNovelty));
    assert_eq!(found(&rig), Some(NoveltyOutcome::NoNovelty));
}

// ---- the job ----

#[test]
fn the_search_is_a_job_of_the_list_one_at_a_time_and_closing_the_dialog_stops_it() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = base_with(
        dir.path(),
        "ref.pgn",
        &[("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6", "2020.01.01")],
    );
    rig.play_all(&RUY_NF6);
    rig.raw(eco(EcoMessage::OpenNovelty));
    let effects = rig.raw(eco(EcoMessage::FindNovelty));
    assert_eq!(count(&effects, is_novelty), 1);
    let (kind, control) = rig.state.list_job().expect("a job");
    assert_eq!(kind, ListJobKind::EcoNovelty);
    assert!(!control.is_cancelled());
    assert!(rig.state.eco_novelty_view().unwrap().running);
    assert_eq!(count(&rig.raw(eco(EcoMessage::FindNovelty)), is_novelty), 0);
    // Closing the dialog cancels it and a late answer is absorbed.
    rig.raw(eco(EcoMessage::CloseNovelty));
    assert!(rig.state.list_job().unwrap().1.is_cancelled());
    let effect = effects.into_iter().find(is_novelty).unwrap();
    let answer = rig.perform(effect);
    rig.send(answer);
    assert!(rig.state.list_job().is_none());
    assert_eq!(rig.state.eco_window(), None);
    assert_eq!(
        rig.ply(),
        6,
        "the cursor does not move under a closed dialog"
    );
}

#[test]
fn a_search_answered_after_the_dialog_closed_does_not_move_the_cursor() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = base_with(
        dir.path(),
        "ref.pgn",
        &[("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6", "2020.01.01")],
    );
    rig.play_all(&RUY_NF6);
    rig.raw(eco(EcoMessage::OpenNovelty));
    let effects = rig.raw(eco(EcoMessage::FindNovelty));
    let effect = effects.into_iter().find(is_novelty).unwrap();
    // The result arrives, but the dialog was closed and the editor is free.
    let answer = rig.perform(effect);
    rig.raw(eco(EcoMessage::CloseNovelty));
    rig.send(answer);
    assert_eq!(rig.ply(), 6);
}

#[test]
fn stopping_the_search_reports_it_as_interrupted_or_says_nothing_and_leaves_the_game_alone() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = base_with(
        dir.path(),
        "ref.pgn",
        &[("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6", "2020.01.01")],
    );
    rig.play_all(&RUY_NF6);
    rig.raw(eco(EcoMessage::OpenNovelty));
    let effect = rig
        .raw(eco(EcoMessage::FindNovelty))
        .into_iter()
        .find(is_novelty)
        .unwrap();
    rig.raw(Message::Bases(BasesMessage::List(ListMessage::CancelJob)));
    let answer = rig.perform(effect);
    rig.send(answer);
    assert!(rig.state.list_job().is_none());
    assert!(!rig.state.eco_novelty_view().unwrap().running);
    assert_eq!(rig.ply(), 6);
    assert!(matches!(
        found(&rig),
        None | Some(NoveltyOutcome::Interrupted)
    ));
    // The dialog can search again.
    rig.send(eco(EcoMessage::FindNovelty));
    assert!(matches!(
        found(&rig),
        Some(NoveltyOutcome::Found { ply: 6, .. })
    ));
}

#[test]
fn novelty_messages_with_no_dialog_open_do_nothing() {
    let mut rig = Rig::new();
    rig.play("e2e4");
    for m in [
        EcoMessage::CloseNovelty,
        EcoMessage::SetNoveltyReference(sel(9)),
        EcoMessage::SetNoveltyOlderOnly(true),
        EcoMessage::FindNovelty,
    ] {
        assert!(rig.raw(eco(m)).is_empty());
    }
    assert_eq!(rig.state.eco_window(), None);
}
