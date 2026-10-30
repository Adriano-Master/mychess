//! Contract tests of the list, the record and the tools of the EPD window
//! (spec `m7-5-janela-epd.md`, rules 4, 7, 8, 10-15, criteria 7-11, 14, 15).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]
mod m7_5b_common;

use m7_5b_common::*;
use mychess_app::epd_window::{SortKey, UnwritableKind};
use mychess_app::online::{Family, family_of};
use mychess_app::{GameOrigin, Preferences, load_preferences, render_preferences};

fn opened() -> (Rig, tempfile::TempDir, EpdFileId) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_cinco(dir.path(), "cinco.epd");
    let mut rig = Rig::new();
    let id = rig.open(&path);
    (rig, dir, id)
}

// ---- sync with the board (criterion 7) ----

#[test]
fn the_record_of_the_board_position_is_selected_and_shown() {
    let (mut rig, _dir, _) = opened();
    let tab = rig.tab();
    assert_eq!(tab.selected_row, Some(0));
    assert_eq!(tab.editor_text, "bm e4\nid \"first\"\n");
    // After 1.e4 the second record is the one.
    rig.play("e4");
    let tab = rig.tab();
    assert_eq!(tab.selected_row, Some(1));
    assert_eq!(tab.editor_text, "am e5\n");
    // After 1.e4 e5 the position is not in the file: the editor is empty.
    rig.play("e5");
    let tab = rig.tab();
    assert_eq!(tab.selected, None);
    assert_eq!(tab.editor_text, "");
}

#[test]
fn the_moves_that_lead_into_the_file_are_listed_in_ascii_order() {
    let (mut rig, _dir, _) = opened();
    assert_eq!(rig.tab().book_moves, ["Nf3", "c4", "d4", "e4"]);
    rig.play("e4");
    assert!(rig.tab().book_moves.is_empty());
}

#[test]
fn leaving_a_record_with_a_typed_text_writes_it_first() {
    let (mut rig, _dir, _) = opened();
    rig.epd(EpdMessage::Edit("bm Nc3\n".into()));
    assert!(rig.tab().editor_draft);
    rig.play("e4");
    let tab = rig.tab();
    assert!(tab.dirty);
    rig.send(GameMessage::Undo);
    assert_eq!(rig.tab().editor_text, "bm Nc3\n");
}

// ---- editing (criterion 8) ----

#[test]
fn edit_and_commit_rewrite_the_record_and_mark_the_file() {
    let (mut rig, _dir, _) = opened();
    rig.epd(EpdMessage::Edit("bm d4\nc0 \"hi\"\n".into()));
    assert!(!rig.tab().dirty, "only a draft until the commit");
    rig.epd(EpdMessage::Commit);
    let tab = rig.tab();
    assert!(tab.dirty);
    assert!(!tab.editor_draft);
    assert_eq!(tab.editor_text, "bm d4\nc0 \"hi\"\n");
}

#[test]
fn the_same_text_does_not_mark_the_file() {
    let (mut rig, _dir, _) = opened();
    let text = rig.tab().editor_text;
    rig.epd(EpdMessage::Edit(text));
    rig.epd(EpdMessage::Commit);
    assert!(!rig.tab().dirty);
}

#[test]
fn a_non_standard_opcode_is_kept_with_a_warning() {
    let (mut rig, _dir, _) = opened();
    rig.epd(EpdMessage::Edit("1x foo\n".into()));
    rig.epd(EpdMessage::Commit);
    assert_eq!(
        rig.notice(),
        Some(EpdNotice::NonStandardOpcode {
            opcode: "1x".into()
        })
    );
    assert_eq!(rig.tab().editor_text, "1x foo\n");
    assert_eq!(rig.tab().nonstandard, ["1x"]);
}

#[test]
fn typing_where_the_file_has_no_record_creates_it() {
    let (mut rig, _dir, _) = opened();
    rig.play("e4");
    rig.play("e5");
    rig.epd(EpdMessage::Edit("id \"new\"\n".into()));
    rig.epd(EpdMessage::Commit);
    let tab = rig.tab();
    assert_eq!((tab.records, tab.rows), (6, 6));
    assert!(tab.dirty);
    assert_eq!(tab.selected_row, Some(5));
    assert_eq!(tab.editor_text, "id \"new\"\n");
}

// ---- add and remove (criterion 9) ----

#[test]
fn add_position_appends_and_selects_and_a_second_add_only_selects() {
    let (mut rig, _dir, _) = opened();
    rig.play("e4");
    rig.play("e5");
    rig.epd(EpdMessage::AddPosition);
    let tab = rig.tab();
    assert_eq!(tab.records, 6);
    assert_eq!(tab.selected_row, Some(5));
    assert!(tab.dirty);
    rig.epd(EpdMessage::AddPosition);
    assert_eq!(rig.tab().records, 6);
    // A position that is there: selected, nothing added.
    rig.send(GameMessage::Undo);
    rig.send(GameMessage::Undo);
    rig.epd(EpdMessage::AddPosition);
    assert_eq!(rig.tab().records, 6);
    assert_eq!(rig.tab().selected_row, Some(0));
}

#[test]
fn removing_a_record_closes_the_hole_and_selects_the_neighbour() {
    let (mut rig, _dir, _) = opened();
    rig.play("e4");
    assert_eq!(rig.tab().selected_row, Some(1));
    rig.epd(EpdMessage::RemovePosition);
    assert!(rig.tab().pending_remove.is_some());
    assert_eq!(rig.tab().records, 5, "it asks first");
    rig.epd(EpdMessage::ConfirmRemove(false));
    assert_eq!(rig.tab().records, 5);
    assert!(rig.tab().pending_remove.is_none());
    rig.epd(EpdMessage::RemovePosition);
    rig.epd(EpdMessage::ConfirmRemove(true));
    let tab = rig.tab();
    assert_eq!((tab.records, tab.rows), (4, 4));
    assert!(tab.dirty);
    // The row that was after it took its place.
    assert_eq!(tab.selected_row, Some(1));
    let fens = row_fens(&rig.rows());
    let removed = four_fields(&five_positions()[1]);
    assert!(!fens.contains(&removed));
    assert_eq!(fens.len(), 4);
}

#[test]
fn removing_with_nothing_selected_says_so() {
    let (mut rig, _dir, _) = opened();
    rig.play("e4");
    rig.play("e5");
    rig.epd(EpdMessage::RemovePosition);
    assert_eq!(rig.notice(), Some(EpdNotice::NoRecord));
}

// ---- go to the position (criterion 10) ----

#[test]
fn go_to_loads_the_position_as_a_new_game_and_turns_the_board() {
    let (mut rig, _dir, _) = opened();
    let black_to_move = rig.rows()[1].index;
    rig.epd(EpdMessage::GoTo(black_to_move));
    assert!(matches!(rig.state.origin, GameOrigin::EpdRecord { .. }));
    assert_eq!(rig.state.session.position(), &five_positions()[1]);
    assert!(rig.state.board_flipped, "Black to move is at the bottom");
    assert_eq!(rig.tab().selected_row, Some(1));
    let white_to_move = rig.rows()[0].index;
    rig.epd(EpdMessage::GoTo(white_to_move));
    assert!(!rig.state.board_flipped);
}

#[test]
fn the_board_stays_when_the_preference_is_off() {
    let (mut rig, _dir, _) = opened();
    let prefs = EpdPrefs {
        flip_to_side_to_move: false,
        ..EpdPrefs::default()
    };
    rig.send(PrefsMessage::Set(PrefChange::Epd(Box::new(prefs))));
    let black_to_move = rig.rows()[1].index;
    rig.epd(EpdMessage::GoTo(black_to_move));
    assert!(!rig.state.board_flipped);
}

#[test]
fn a_game_that_came_from_a_record_is_replaced_without_asking() {
    let (mut rig, _dir, _) = opened();
    let rows = rig.rows();
    rig.epd(EpdMessage::GoTo(rows[1].index));
    rig.epd(EpdMessage::GoTo(rows[2].index));
    assert!(rig.state.discard_prompt.is_none());
    assert_eq!(rig.state.session.position(), &five_positions()[2]);
}

#[test]
fn a_game_with_unsaved_changes_asks_before_it_is_replaced() {
    let (mut rig, _dir, _) = opened();
    rig.play("e4");
    rig.play("e5");
    assert!(rig.state.session.is_dirty());
    let rows = rig.rows();
    rig.epd(EpdMessage::GoTo(rows[3].index));
    assert!(rig.state.discard_prompt.is_some());
    assert_eq!(rig.state.session.position().fen(), {
        let mut p = Position::new();
        for san in ["e4", "e5"] {
            p = p.after(mv(&p, san)).unwrap();
        }
        p.fen()
    });
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(rig.state.session.position(), &five_positions()[3]);
    assert!(matches!(rig.state.origin, GameOrigin::EpdRecord { .. }));
}

use mychess_core::Position;

#[test]
fn navigation_walks_the_list_with_wrap_around() {
    let (mut rig, _dir, _) = opened();
    rig.epd(EpdMessage::First);
    assert_eq!(rig.tab().selected_row, Some(0));
    rig.epd(EpdMessage::Next);
    assert_eq!(rig.tab().selected_row, Some(1));
    rig.epd(EpdMessage::Last);
    assert_eq!(rig.tab().selected_row, Some(4));
    rig.epd(EpdMessage::Next);
    assert_eq!(rig.tab().selected_row, Some(0));
    rig.epd(EpdMessage::Prev);
    assert_eq!(rig.tab().selected_row, Some(4));
    rig.epd(EpdMessage::Random(7));
    assert_eq!(rig.tab().selected_row, Some(2));
    assert_eq!(rig.state.session.position(), &five_positions()[2]);
}

#[test]
fn deepest_goes_to_the_deepest_position_of_the_game_that_the_file_has() {
    let (mut rig, _dir, _) = opened();
    rig.play("e4");
    rig.play("e5");
    rig.send(GameMessage::Navigate(mychess_app::Nav::Start));
    rig.epd(EpdMessage::Deepest);
    assert_eq!(rig.state.session.position(), &five_positions()[1]);
    // A game that never meets the file.
    rig.send(Command::NewGame);
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    rig.play("g4");
    rig.send(GameMessage::Navigate(mychess_app::Nav::Start));
    rig.epd(EpdMessage::Deepest);
    assert_eq!(
        rig.state.session.position(),
        &Position::new(),
        "start is in the file"
    );
}

// ---- sort and remove opcodes (criterion 11) ----

#[test]
fn strip_removes_an_opcode_everywhere_and_reports_how_many() {
    let (mut rig, _dir, _) = opened();
    rig.epd(EpdMessage::StripOpcode("CE".into()));
    assert_eq!(
        rig.notice(),
        Some(EpdNotice::Stripped {
            opcode: "CE".into(),
            count: 1
        })
    );
    assert!(rig.tab().dirty);
}

#[test]
fn stripping_an_opcode_nobody_has_reports_zero_and_does_not_mark() {
    let (mut rig, _dir, _) = opened();
    rig.epd(EpdMessage::StripOpcode("zz".into()));
    assert_eq!(
        rig.notice(),
        Some(EpdNotice::Stripped {
            opcode: "zz".into(),
            count: 0
        })
    );
    assert!(!rig.tab().dirty);
}

#[test]
fn sort_all_counts_the_records_that_changed_and_marks_only_then() {
    let (mut rig, _dir, _) = opened();
    // Already sorted: `bm` before `id`; `ce` before `pv`.
    rig.epd(EpdMessage::SortAll);
    assert_eq!(rig.notice(), Some(EpdNotice::Sorted { count: 0 }));
    assert!(!rig.tab().dirty);
    rig.epd(EpdMessage::Edit("zz 1\naa 2\n".into()));
    rig.epd(EpdMessage::Commit);
    rig.epd(EpdMessage::Save);
    rig.epd(EpdMessage::Edit("zz 1\naa 2\n".into()));
    rig.epd(EpdMessage::Commit);
    rig.epd(EpdMessage::SortAll);
    assert_eq!(rig.notice(), Some(EpdNotice::Sorted { count: 1 }));
    assert_eq!(rig.tab().editor_text, "aa 2\nzz 1\n");
    assert!(rig.tab().dirty);
}

#[test]
fn sort_record_sorts_the_selected_one() {
    let (mut rig, _dir, _) = opened();
    rig.epd(EpdMessage::Edit("zz 1\naa 2\n".into()));
    rig.epd(EpdMessage::Commit);
    rig.epd(EpdMessage::Save);
    assert!(!rig.tab().dirty);
    rig.epd(EpdMessage::SortRecord);
    assert_eq!(rig.tab().editor_text, "aa 2\nzz 1\n");
    assert_eq!(rig.notice(), Some(EpdNotice::Sorted { count: 1 }));
    assert!(rig.tab().dirty);
    rig.epd(EpdMessage::SortRecord);
    assert_eq!(rig.notice(), Some(EpdNotice::Sorted { count: 0 }));
}

// ---- the list ----

#[test]
fn the_filter_and_the_order_shape_the_rows_without_touching_the_book() {
    let (mut rig, _dir, _) = opened();
    rig.epd(EpdMessage::SetFilter("PV nf6".into()));
    let tab = rig.tab();
    assert_eq!((tab.records, tab.rows), (5, 1));
    rig.epd(EpdMessage::SetFilter(String::new()));
    rig.epd(EpdMessage::SetSort(EpdSort {
        key: SortKey::Eval,
        descending: false,
    }));
    let rows = rig.rows();
    // The record with `ce` first, the others in file order after it.
    assert_eq!(rows[0].fen, four_fields(&five_positions()[3]));
    assert_eq!(rows.len(), 5);
    rig.epd(EpdMessage::SetSort(EpdSort {
        key: SortKey::Id,
        descending: false,
    }));
    let ids: Vec<Option<String>> = rig.rows().into_iter().map(|r| r.id).collect();
    assert_eq!(ids[0].as_deref(), Some("X;Y"));
    assert_eq!(ids[1].as_deref(), Some("first"));
    assert!(!rig.tab().dirty);
}

#[test]
fn a_page_of_rows_is_numbered_from_one() {
    let (rig, _dir, id) = opened();
    let page = rig.state.epd_rows(id, 1..3);
    assert_eq!(page.iter().map(|r| r.number).collect::<Vec<_>>(), [2, 3]);
    assert!(rig.state.epd_rows(id, 9..20).is_empty());
}

// ---- search ----

#[test]
fn find_selects_the_next_match_forward_and_backward() {
    let (mut rig, _dir, _) = opened();
    let effects = rig.raw(Message::Epd(EpdMessage::Find("CE 25".into(), true)));
    assert!(matches!(
        effects.as_slice(),
        [Effect::Epd(EpdEffect::Find { .. })]
    ));
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    let tab = rig.tab();
    assert_eq!(tab.selected_row, Some(3));
    assert!(tab.find_hit.is_some());
    // Nothing more after it: the search wraps and finds the same one.
    rig.epd(EpdMessage::Find("ce 25".into(), true));
    assert_eq!(rig.tab().selected_row, Some(3));
    rig.epd(EpdMessage::Find("id \"".into(), false));
    assert_eq!(rig.tab().selected_row, Some(2));
    rig.epd(EpdMessage::Find("id \"".into(), false));
    assert_eq!(rig.tab().selected_row, Some(0));
}

#[test]
fn find_that_matches_nothing_says_so() {
    let (mut rig, _dir, _) = opened();
    rig.epd(EpdMessage::Find("zzz".into(), true));
    assert_eq!(rig.notice(), Some(EpdNotice::NotFound));
    assert!(rig.tab().find_hit.is_none());
}

// ---- paste analysis, online (criteria 12, 15) ----

#[test]
fn paste_without_an_analysis_of_the_position_is_an_error_notice() {
    let (mut rig, _dir, _) = opened();
    rig.epd(EpdMessage::PasteAnalysis(None));
    assert_eq!(rig.notice(), Some(EpdNotice::NoAnalysis));
    assert!(!rig.tab().dirty);
}

#[test]
fn only_the_engine_messages_are_help_during_an_online_game() {
    let config = mychess_app::epd_window::AnnotateConfig {
        engine: None,
        seconds: 3,
        mode: mychess_app::epd_window::AnnotateMode::Annotate,
        scope: mychess_app::epd_window::RunScope::All,
    };
    let help = [
        Message::Epd(EpdMessage::Annotate(config)),
        Message::Epd(EpdMessage::PasteAnalysis(None)),
    ];
    for message in &help {
        assert_eq!(family_of(message), Some(Family::Engines), "{message:?}");
    }
    let free = [
        Message::Epd(EpdMessage::Open),
        Message::Epd(EpdMessage::OpenPath("x.epd".into())),
        Message::Epd(EpdMessage::Edit(String::new())),
        Message::Epd(EpdMessage::Commit),
        Message::Epd(EpdMessage::Find("a".into(), true)),
        Message::Epd(EpdMessage::StopAnnotate),
        Message::Epd(EpdMessage::SortAll),
    ];
    for message in &free {
        assert_eq!(family_of(message), None, "{message:?}");
    }
}

// ---- preferences (criterion 16) ----

#[test]
fn the_epd_preferences_round_trip_and_the_bad_ones_go_back_to_the_default() {
    let mut prefs = Preferences::default();
    *prefs.epd = EpdPrefs {
        annotate_seconds: 42,
        recent: vec!["/a/b.epd".into(), "/c/d.epd.gz".into()],
        ..EpdPrefs::default()
    };
    let back = load_preferences(&render_preferences(&prefs));
    assert_eq!(*back.prefs.epd, *prefs.epd);
    let bad = load_preferences("[epd]\nannotate_mode = 7\nannotate_seconds = 99999\n");
    assert_eq!(
        bad.prefs.epd.annotate_mode,
        EpdPrefs::default().annotate_mode
    );
    assert_eq!(bad.prefs.epd.annotate_seconds, 300);
    assert!(!bad.issues.is_empty());
}

#[test]
fn opening_files_updates_the_recent_list_to_ten() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    for i in 0..12 {
        let path = write_cinco(dir.path(), &format!("f{i}.epd"));
        let id = rig.open(&path);
        rig.epd(EpdMessage::Close(id));
    }
    let recent = rig.view().recent;
    assert_eq!(recent.len(), 10);
    assert!(recent[0].ends_with("f11.epd"));
    rig.epd(EpdMessage::ClearRecent);
    assert!(rig.view().recent.is_empty());
}

#[test]
fn the_unwritable_kinds_have_their_own_fluent_keys() {
    let keys: Vec<&str> = [
        UnwritableKind::Semicolon,
        UnwritableKind::UnbalancedQuote,
        UnwritableKind::TooLong,
        UnwritableKind::LineBreak,
        UnwritableKind::Other,
    ]
    .into_iter()
    .map(|kind| {
        EpdNotice::Unwritable {
            opcode: "x".into(),
            kind,
        }
        .message_key()
    })
    .collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 5);
}
