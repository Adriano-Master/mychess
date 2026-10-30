//! Contract tests of the Clipbase (spec M3-4, rule 7, D507, D519, D522;
//! Scid `sc_clipbase` copy/paste/clear, `tkscid.cpp` 4035-4170).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_push_string
)]

mod m3_4a_common;
use m3_4a_common::*;
use mychess_pgn::write::{WriteOptions, game_to_string};

const RICH: &str = "[Event \"Rich\"]\n[Site \"Oslo NOR\"]\n[Date \"2024.05.??\"]\n\
    [White \"Wh\"]\n[Black \"Bl\"]\n[WhiteElo \"2500\"]\n[BlackElo \"2400\"]\n\
    [Round \"3\"]\n[Result \"1-0\"]\n\n\
    1. e4 {good} e5 (1... c5 2. Nf3) 2. Nf3 $1 Nc6 1-0\n";

fn clip(rig: &Rig) -> (u32, u32) {
    let c = rig.state.bases.slot(SlotId::CLIPBASE).unwrap();
    (c.total(), c.filtered())
}

fn pgn_of(rig: &Rig) -> String {
    game_to_string(rig.state.session.game(), &WriteOptions::default()).expect("pgn")
}

fn rig_one(dir: &std::path::Path) -> Rig {
    let mut rig = Rig::new();
    rig.open(&write(dir, "one.pgn", ONE));
    rig
}

#[test]
fn copy_appends_to_the_end_and_the_filter_grows() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "two.pgn", TWO));
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 1,
    });
    rig.send(BasesMessage::ClipbaseCopy);
    assert_eq!(clip(&rig), (1, 1));
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 2,
    });
    rig.send(BasesMessage::ClipbaseCopy);
    assert_eq!(clip(&rig), (2, 2));
    let rows = rig.state.bases.list_page(SlotId::CLIPBASE, 1, 10);
    let whites: Vec<_> = rows.iter().map(|r| (r.number, r.white.as_str())).collect();
    assert_eq!(
        whites,
        [(1, "A"), (2, "C")],
        "copies go to the end, in order"
    );
    assert_eq!(
        rig.state.bases.slot(SlotId::CLIPBASE).unwrap().current_game,
        Some(2)
    );
    assert_eq!(rig.current(), sel(1), "copying does not switch slots");
}

#[test]
fn copy_does_not_change_the_edited_game_or_its_altered_flag() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_one(dir.path());
    rig.play("g1f3");
    let before = pgn_of(&rig);
    rig.send(BasesMessage::ClipbaseCopy);
    assert_eq!(pgn_of(&rig), before);
    assert!(rig.state.is_dirty());
    assert!(rig.state.discard_prompt.is_none());
}

#[test]
fn copy_from_a_read_only_slot_is_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_one(dir.path());
    assert!(rig.state.bases.slot(sel(1)).unwrap().read_only);
    rig.send(BasesMessage::ClipbaseCopy);
    assert_eq!(clip(&rig), (1, 1));
    assert!(rig.notice().is_none());
}

#[test]
fn the_same_game_copied_twice_makes_two_entries() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_one(dir.path());
    for _ in 0..2 {
        rig.send(BasesMessage::ClipbaseCopy);
    }
    let rows = rig.state.bases.list_page(SlotId::CLIPBASE, 1, 10);
    assert_eq!(rows.iter().map(|r| r.number).collect::<Vec<_>>(), [1, 2]);
    assert!(rows.iter().all(|r| r.white == "Solo" && r.black == "Only"));
}

#[test]
fn copy_is_refused_inside_the_clipbase_and_changes_nothing() {
    let mut rig = Rig::new();
    rig.play("e2e4");
    rig.send(BasesMessage::ClipbaseCopy);
    assert_eq!(rig.notice(), Some(&BasesNotice::AlreadyInClipbase));
    assert_eq!(clip(&rig), (0, 0));
    assert_eq!(rig.plies(), 1, "the edited game is untouched");
}

#[test]
fn a_copy_round_trips_tags_variations_comments_and_nags() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "rich.pgn", RICH));
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 1,
    });
    let original = pgn_of(&rig);
    assert!(
        original.contains("(1... c5") && original.contains("{good}"),
        "{original}"
    );
    rig.send(BasesMessage::ClipbaseCopy);
    rig.send(BasesMessage::LoadGame {
        slot: SlotId::CLIPBASE,
        number: 1,
    });
    assert_eq!(rig.current(), SlotId::CLIPBASE);
    assert_eq!(pgn_of(&rig), original);
}

#[test]
fn the_copied_game_becomes_the_active_one_and_discards_older_clipbase_edits() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_one(dir.path());
    rig.select(9);
    rig.play("e2e4"); // an edit in the Clipbase editor
    rig.select(1);
    rig.send(BasesMessage::ClipbaseCopy);
    rig.select(9);
    // Scid: "clipbase changes get discarded"; the copy is the active game.
    assert_eq!(rig.white(), "Solo");
    assert!(!rig.state.is_dirty());
}

#[test]
fn paste_replaces_the_game_with_the_copy_and_counts_it_as_altered() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "rich.pgn", RICH));
    rig.open(&write(dir.path(), "one.pgn", ONE));
    rig.send(BasesMessage::LoadGame {
        slot: sel(2),
        number: 1,
    });
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 1,
    });
    let rich = pgn_of(&rig);
    rig.send(BasesMessage::ClipbaseCopy);
    rig.select(2);
    rig.send(BasesMessage::LoadGame {
        slot: sel(2),
        number: 1,
    });
    assert_eq!(rig.white(), "Solo");
    rig.send(BasesMessage::ClipbasePaste);
    assert_eq!(pgn_of(&rig), rich);
    assert!(rig.state.is_dirty());
    assert_eq!(rig.state.origin, GameOrigin::Clipboard);
    assert_eq!(rig.current(), sel(2));
}

#[test]
fn paste_uses_the_active_clipbase_game_not_always_the_last() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "two.pgn", TWO));
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 1,
    });
    rig.send(BasesMessage::ClipbaseCopy);
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 2,
    });
    rig.send(BasesMessage::ClipbaseCopy);
    // Make clipbase game 1 the active one, then go back and paste.
    rig.send(BasesMessage::LoadGame {
        slot: SlotId::CLIPBASE,
        number: 1,
    });
    rig.select(1);
    rig.send(BasesMessage::ClipbasePaste);
    assert_eq!(rig.white(), "A", "the active Clipbase game was game 1");
}

#[test]
fn paste_is_a_no_op_inside_the_clipbase() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_one(dir.path());
    rig.send(BasesMessage::ClipbaseCopy);
    rig.select(9);
    let before = pgn_of(&rig);
    rig.send(BasesMessage::ClipbasePaste);
    assert_eq!(pgn_of(&rig), before);
    assert!(!rig.state.is_dirty());
    assert!(rig.state.discard_prompt.is_none());
    assert_eq!(clip(&rig), (1, 1));
}

#[test]
fn paste_with_an_empty_clipbase_reports_it_and_keeps_the_game() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_one(dir.path());
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 1,
    });
    rig.send(BasesMessage::ClipbasePaste);
    assert_eq!(rig.notice(), Some(&BasesNotice::ClipbaseEmpty));
    assert_eq!(rig.white(), "Solo");
    assert!(!rig.state.is_dirty());
}

#[test]
fn paste_over_an_altered_game_asks_before_replacing() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "rich.pgn", RICH));
    rig.open(&write(dir.path(), "one.pgn", ONE));
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 1,
    });
    rig.send(BasesMessage::ClipbaseCopy);
    rig.select(2);
    rig.send(BasesMessage::LoadGame {
        slot: sel(2),
        number: 1,
    });
    rig.play("g1f3");
    rig.send(BasesMessage::ClipbasePaste);
    assert!(rig.state.discard_prompt.is_some());
    assert_eq!(rig.white(), "Solo");
    rig.send(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert_eq!(rig.white(), "Solo");
    assert!(rig.state.is_dirty());
}

#[test]
fn clear_empties_games_filter_and_list_state() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_one(dir.path());
    for _ in 0..3 {
        rig.send(BasesMessage::ClipbaseCopy);
    }
    rig.send(BasesMessage::SetListFirst {
        slot: SlotId::CLIPBASE,
        first: 3,
    });
    rig.send(BasesMessage::SelectRow {
        slot: SlotId::CLIPBASE,
        number: Some(2),
    });
    rig.send(BasesMessage::ClipbaseClear);
    assert_eq!(clip(&rig), (0, 0));
    assert!(
        rig.state
            .bases
            .list_page(SlotId::CLIPBASE, 1, 10)
            .is_empty()
    );
    let list = &rig.state.bases.slot(SlotId::CLIPBASE).unwrap().list;
    assert_eq!((list.first, list.selected), (1, None));
    // Paste now finds nothing; copy works again from number 1.
    rig.send(BasesMessage::ClipbasePaste);
    assert_eq!(rig.notice(), Some(&BasesNotice::ClipbaseEmpty));
    rig.send(BasesMessage::ClipbaseCopy);
    assert_eq!(clip(&rig), (1, 1));
}

#[test]
fn clear_of_an_empty_clipbase_is_harmless() {
    let mut rig = Rig::new();
    rig.send(BasesMessage::ClipbaseClear);
    rig.send(BasesMessage::ClipbaseClear);
    assert_eq!(clip(&rig), (0, 0));
    assert!(rig.notice().is_none());
}

#[test]
fn reset_while_in_the_clipbase_empties_the_edited_game() {
    // Scid: Edit > Reset runs `sc_clipbase clear` then `updateBoard -pgn`;
    // `sc_clipbase_clear` does `clipbase->game->Clear()`.
    let mut rig = Rig::new();
    rig.play("e2e4");
    rig.send(BasesMessage::ClipbaseClear);
    assert_eq!(
        rig.plies(),
        0,
        "the game shown while in the Clipbase is cleared"
    );
}

#[test]
fn the_limit_makes_the_third_copy_fail_with_the_message_and_stay_consistent() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_one(dir.path());
    rig.state.bases.set_clipbase_limit(2);
    rig.send(BasesMessage::ClipbaseCopy);
    rig.send(BasesMessage::ClipbaseCopy);
    assert!(rig.notice().is_none());
    rig.send(BasesMessage::ClipbaseCopy);
    assert_eq!(
        rig.notice(),
        Some(&BasesNotice::Failed(BaseError::ClipbaseLimit { limit: 2 }))
    );
    assert_eq!(
        rig.notice().unwrap().message_key(),
        "base-error-clipbase-limit"
    );
    assert_eq!(clip(&rig), (2, 2));
    let rows = rig.state.bases.list_page(SlotId::CLIPBASE, 1, 10);
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| !r.unreadable && r.white == "Solo"));
    // A fourth try fails the same way, and clearing frees room.
    rig.send(BasesMessage::ClipbaseCopy);
    assert_eq!(clip(&rig), (2, 2));
    rig.send(BasesMessage::ClipbaseClear);
    rig.send(BasesMessage::ClipbaseCopy);
    assert_eq!(clip(&rig), (1, 1));
}

#[test]
fn a_limit_of_zero_refuses_the_first_copy() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_one(dir.path());
    rig.state.bases.set_clipbase_limit(0);
    rig.send(BasesMessage::ClipbaseCopy);
    assert_eq!(
        rig.notice(),
        Some(&BasesNotice::Failed(BaseError::ClipbaseLimit { limit: 0 }))
    );
    assert_eq!(clip(&rig), (0, 0));
}

#[test]
fn lowering_the_limit_below_the_content_keeps_the_games_and_blocks_new_ones() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_one(dir.path());
    for _ in 0..3 {
        rig.send(BasesMessage::ClipbaseCopy);
    }
    rig.state.bases.set_clipbase_limit(2);
    assert_eq!(rig.state.bases.clipbase_limit(), 2);
    assert_eq!(clip(&rig), (3, 3), "games already in stay");
    assert_eq!(rig.state.bases.list_page(SlotId::CLIPBASE, 1, 10).len(), 3);
    rig.send(BasesMessage::ClipbaseCopy);
    assert_eq!(
        rig.notice(),
        Some(&BasesNotice::Failed(BaseError::ClipbaseLimit { limit: 2 }))
    );
    assert_eq!(clip(&rig), (3, 3));
}

#[test]
fn a_failed_copy_does_not_disturb_the_edited_game() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_one(dir.path());
    rig.state.bases.set_clipbase_limit(0);
    rig.play("g1f3");
    let before = pgn_of(&rig);
    rig.send(BasesMessage::ClipbaseCopy);
    assert_eq!(pgn_of(&rig), before);
    assert!(rig.state.is_dirty());
}

#[test]
fn the_default_limit_is_five_million() {
    let rig = Rig::new();
    assert_eq!(rig.state.bases.clipbase_limit(), 5_000_000);
    assert_eq!(mychess_app::DEFAULT_CLIPBASE_LIMIT, 5_000_000);
}

#[test]
fn the_clipbase_is_in_memory_with_the_scid_description() {
    let rig = Rig::new();
    let clip = rig.state.bases.slot(SlotId::CLIPBASE).unwrap();
    assert_eq!(clip.kind, BaseKind::Clipbase);
    assert_eq!(clip.path, None);
    assert_eq!(mychess_app::CLIPBASE_TYPE, 2);
    assert_eq!(
        mychess_app::CLIPBASE_DESCRIPTION,
        "Temporary database, not kept on disk."
    );
}

#[test]
fn clipbase_contents_survive_switching_slots() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_one(dir.path());
    rig.send(BasesMessage::ClipbaseCopy);
    rig.select(9);
    rig.select(1);
    rig.send(BasesMessage::Close(sel(1)));
    assert_eq!(clip(&rig), (1, 1));
}

#[test]
fn copying_a_game_read_from_si4_works() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(dir.path(), "db", RICH);
    let mut rig = Rig::new();
    rig.open(&si4);
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 1,
    });
    let original = pgn_of(&rig);
    rig.send(BasesMessage::ClipbaseCopy);
    rig.send(BasesMessage::LoadGame {
        slot: SlotId::CLIPBASE,
        number: 1,
    });
    assert_eq!(pgn_of(&rig), original);
    let row = &rig.state.bases.list_page(SlotId::CLIPBASE, 1, 1)[0];
    assert_eq!((row.white.as_str(), row.black.as_str()), ("Wh", "Bl"));
    assert_eq!((row.white_elo, row.black_elo), (2500, 2400));
}
