//! Extra contract tests of the jobs and of the edits of the tree masks (spec
//! M5-4, slice M5-4b: D1821, D1822, D1826, D1863, D1865, D1867, case 21): one
//! job at a time, the fill with the filter and its races, and `Edit` with the
//! scope "line".
#![allow(
    clippy::format_push_string, // building the text of a file by hand
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::large_futures, // the rig owns the whole `AppState`
    clippy::cloned_ref_to_slice_refs
)]

mod m5_4b_common;

use m5_4b_common::*;
use mychess_app::{
    BasesMessage, DiscardChoice, FillSide, Marker, MarkerSlot, MaskColor, MaskEffect, MaskJobKind,
    MaskNag, MaskQuery, Nav,
};
use tempfile::TempDir;

fn dir() -> TempDir {
    tempfile::tempdir().expect("temp dir")
}

fn view(rig: &Rig) -> mychess_app::MaskView {
    rig.state.tree_view().mask.expect("a mask is open")
}

fn rig_with_mask(dir: &std::path::Path) -> Rig {
    let mut rig = Rig::std_with_tree(dir);
    m(&mut rig, MaskMessage::New);
    rig
}

fn fill_job(effects: Vec<Effect>) -> Effect {
    effects
        .into_iter()
        .find(|e| matches!(e, Effect::Mask(MaskEffect::Fill { .. })))
        .expect("a fill job")
}

fn play_line(rig: &mut Rig, ucis: &[&str]) {
    for uci in ucis {
        rig.play(uci);
    }
}

fn found(rig: &mut Rig, query: MaskQuery) -> mychess_app::MaskHits {
    m(rig, MaskMessage::Search(Box::new(query)));
    let state = rig.state.mask_search().expect("a search");
    (**state.hits.as_ref().expect("the answer")).clone()
}

fn sans(hits: &mychess_app::MaskHits) -> Vec<String> {
    hits.rows.iter().filter_map(|h| h.san.clone()).collect()
}

// ---- one job at a time (D1863) ----

#[test]
fn while_a_file_is_read_everything_that_changes_the_mask_is_refused() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    let b = dir.path().join("b.stm");
    mask_with(&a, "e4");
    mask_with(&b, "d4");
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::Open(Some(a.clone())));
    // A second read is in flight.
    let effects = rig.raw(TreeMessage::Mask(MaskMessage::Open(Some(b.clone()))));
    let read = effects
        .into_iter()
        .find(|e| matches!(e, Effect::Mask(MaskEffect::Read { .. })))
        .expect("read");
    assert_eq!(
        rig.state.mask_busy().map(|b| b.kind),
        Some(MaskJobKind::Open)
    );
    let refused = [
        MaskMessage::New,
        MaskMessage::Close,
        MaskMessage::Open(Some(a.clone())),
        MaskMessage::Open(None),
        MaskMessage::FillGame,
        MaskMessage::FillLine(FillSide::All),
        MaskMessage::FillFilter(FillSide::All),
        MaskMessage::SetPositionComment("x".to_owned()),
    ];
    for message in refused {
        let out = rig.raw(TreeMessage::Mask(message.clone()));
        assert!(out.is_empty(), "{message:?}: {out:?}");
        assert_eq!(notice(&rig), Some(MaskNotice::Busy), "{message:?}");
        rig.state.notice = None;
    }
    let out = rig.raw(TreeMessage::Mask(MaskMessage::Edit {
        target: san("c4"),
        edit: MaskEdit::Add,
        scope: MaskScope::One,
    }));
    assert!(out.is_empty());
    assert_eq!(notice(&rig), Some(MaskNotice::Busy));
    assert!(!dirty(&rig), "nothing changed under the read");
    // The read ends: the mask of the file is the open one.
    let answer = rig.perform(read);
    rig.send(answer);
    assert_eq!(view(&rig).path, Some(b));
    assert!(rig.state.mask_busy().is_none());
}

#[test]
fn a_save_is_allowed_while_a_fill_runs_and_the_fill_leaves_the_mask_altered() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "c4");
    let mut rig = Rig::std_with_tree(dir.path());
    m(&mut rig, MaskMessage::Open(Some(a.clone())));
    let job = fill_job(rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All))));
    let write = rig
        .raw(TreeMessage::Mask(MaskMessage::Save))
        .into_iter()
        .find(|e| matches!(e, Effect::Mask(MaskEffect::Write { .. })))
        .expect("a save is allowed during a fill");
    // The fill ends first, then the write (of the mask as it was) answers: the
    // fill's moves are not in the file, so the mask is still altered.
    let fill_answer = rig.perform(job);
    rig.send(fill_answer);
    assert!(dirty(&rig));
    let write_answer = rig.perform(write);
    rig.send(write_answer);
    assert!(dirty(&rig), "the fill is not in the file that was written");
    // Another save writes it all.
    m(&mut rig, MaskMessage::Save);
    assert!(!dirty(&rig));
    let (read, _) = mychess_db::tree::mask::stm::read(&a).unwrap();
    assert!(read.len() > 1, "{} positions", read.len());
}

#[test]
fn the_answer_of_a_fill_that_was_given_up_is_dropped_and_the_next_fill_is_kept() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    edit(&mut rig, "e4", MaskEdit::Add);
    // The first fill ran to the end, but its answer is held back.
    let job = fill_job(rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All))));
    let first = rig.perform(job);
    // The quit gives the job up; the user cancels the quit.
    let quit = rig.raw(Message::Quit);
    let generation = quit
        .iter()
        .find_map(|e| match e {
            Effect::Mask(MaskEffect::Deadline { generation, .. }) => Some(*generation),
            _ => None,
        })
        .expect("timer");
    rig.raw(TreeMessage::Mask(MaskMessage::Deadline(generation)));
    rig.raw(TreeMessage::Mask(MaskMessage::Answer(
        DiscardChoice::Cancel,
    )));
    assert!(rig.state.mask_busy().is_none());
    // A second fill starts; the old answer arrives in the middle.
    let job = fill_job(rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All))));
    assert_eq!(view(&rig).positions, 1);
    rig.raw(first);
    assert_eq!(view(&rig).positions, 1, "the old answer changed nothing");
    assert_eq!(
        rig.state.mask_busy().map(|b| b.kind),
        Some(MaskJobKind::Fill),
        "the second job is still the one waited for"
    );
    let second = rig.perform(job);
    rig.raw(second);
    assert!(view(&rig).positions > 1);
    assert!(rig.state.mask_busy().is_none());
}

#[test]
fn a_fill_that_was_given_up_leaves_the_mask_of_a_quit_alone() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    let job = fill_job(rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All))));
    let answer = rig.perform(job);
    // Quit gives the job up (the mask is clean, so the quit goes on), then the
    // user closes the mask.
    let quit = rig.raw(Message::Quit);
    let generation = quit
        .iter()
        .find_map(|e| match e {
            Effect::Mask(MaskEffect::Deadline { generation, .. }) => Some(*generation),
            _ => None,
        })
        .expect("timer");
    rig.raw(TreeMessage::Mask(MaskMessage::Deadline(generation)));
    // The quit decided about the mask (D1877): closing it is refused now,
    // and the answer of the job that was given up must not touch it.
    rig.raw(TreeMessage::Mask(MaskMessage::Close));
    rig.raw(answer);
    if rig.state.mask_open() {
        assert_eq!(view(&rig).positions, 0, "a late answer changes nothing");
        assert!(!dirty(&rig));
    }
}

#[test]
fn a_fill_survives_the_base_being_closed_or_replaced_while_it_runs() {
    for replace in [false, true] {
        let dir = dir();
        let mut rig = rig_with_mask(dir.path());
        let slot = rig.state.bases.current();
        let job = fill_job(rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All))));
        if replace {
            rig.open_pgn(dir.path(), "other.pgn", "[Result \"*\"]\n\n1. c4 *\n\n");
        } else {
            rig.send(BasesMessage::Close(slot));
        }
        let answer = rig.perform(job);
        rig.send(answer);
        assert!(rig.state.mask_busy().is_none(), "replace={replace}");
        assert!(rig.state.mask_open(), "replace={replace}");
        assert!(
            matches!(
                notice(&rig),
                Some(MaskNotice::Filled { .. } | MaskNotice::FillUnavailable)
            ),
            "replace={replace}: {:?}",
            rig.state.notice
        );
    }
}

#[test]
fn a_fill_with_a_base_that_has_no_games_says_so_without_a_job() {
    let dir = dir();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "empty.pgn", "");
    m(&mut rig, MaskMessage::New);
    let effects = rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All)));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(notice(&rig), Some(MaskNotice::FillUnavailable));
    assert!(rig.state.mask_busy().is_none());
}

#[test]
fn a_fill_that_was_cancelled_before_it_ran_reports_it_and_marks_nothing() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    let job = fill_job(rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All))));
    rig.raw(TreeMessage::Mask(MaskMessage::CancelFill));
    // Cancelling twice is harmless.
    rig.raw(TreeMessage::Mask(MaskMessage::CancelFill));
    let answer = rig.perform(job);
    rig.send(answer);
    assert!(matches!(
        notice(&rig),
        Some(MaskNotice::Filled {
            cancelled: true,
            ..
        })
    ));
    assert!(!dirty(&rig), "nothing was read, so nothing changed");
    // A cancel with no fill running does nothing at all.
    let out = rig.raw(TreeMessage::Mask(MaskMessage::CancelFill));
    assert!(out.is_empty());
    assert!(rig.state.mask_busy().is_none());
}

#[test]
fn the_fill_with_the_filter_uses_the_games_that_are_in_it_only() {
    let dir = dir();
    let mut rig = Rig::std_with_tree(dir.path());
    let slot = rig.state.bases.current();
    // Only the game 2 (`1.d4 d5`) stays in the filter.
    rig.keep(slot, &[2]);
    m(&mut rig, MaskMessage::New);
    let job = fill_job(rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All))));
    let answer = rig.perform(job);
    rig.send(answer);
    match notice(&rig) {
        Some(MaskNotice::Filled { games: 1, .. }) => {}
        other => panic!("{other:?}"),
    }
    let hits = found(&mut rig, MaskQuery::default());
    let mut got = sans(&hits);
    got.sort();
    assert_eq!(got, ["d4", "d5"]);
}

#[test]
fn the_fill_by_side_takes_only_the_moves_of_that_side() {
    let dir = dir();
    for (side, want) in [
        (FillSide::White, vec!["Nf3", "d4", "e4"]),
        (FillSide::Black, vec!["c5", "d5", "e5"]),
    ] {
        let mut rig = rig_with_mask(dir.path());
        let job = fill_job(rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(side))));
        let answer = rig.perform(job);
        rig.send(answer);
        let mut got = sans(&found(&mut rig, MaskQuery::default()));
        got.sort();
        got.dedup();
        // The first moves of the five games, plus Nf3 at move 2 for White.
        for san in &want {
            assert!(got.contains(&(*san).to_owned()), "{side:?}: {got:?}");
        }
        let other_side: &[&str] = if side == FillSide::White {
            &["d5", "c5", "e5"]
        } else {
            &["e4", "d4", "Nf3"]
        };
        for san in other_side {
            assert!(
                !got.contains(&(*san).to_owned()),
                "{side:?} must not take {san}: {got:?}"
            );
        }
    }
}

// ---- Edit and the scope "line" (D1822, D1865) ----

#[test]
fn a_line_edit_goes_back_through_the_moves_that_led_to_the_cursor() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    play_line(&mut rig, &["e2e4", "e7e5", "g1f3", "b8c6"]);
    rig.nav(Nav::Back(2));
    m(
        &mut rig,
        MaskMessage::Edit {
            target: san("Nf3"),
            edit: MaskEdit::Color(MaskColor::Green),
            scope: MaskScope::Line,
        },
    );
    let hits = found(
        &mut rig,
        MaskQuery {
            color: Some(MaskColor::Green),
            ..MaskQuery::default()
        },
    );
    let mut got = sans(&hits);
    got.sort();
    // The cursor is after 1.e4 e5: the line is e4, e5 and the clicked Nf3,
    // not the Nc6 that comes after the cursor.
    assert_eq!(got, ["Nf3", "e4", "e5"]);
}

#[test]
fn a_line_edit_that_changes_nothing_does_not_mark_the_mask() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    play_line(&mut rig, &["e2e4", "e7e5"]);
    let line = |rig: &mut Rig, color| {
        m(
            rig,
            MaskMessage::Edit {
                target: san("Nf3"),
                edit: MaskEdit::Color(color),
                scope: MaskScope::Line,
            },
        );
    };
    line(&mut rig, MaskColor::Blue);
    assert!(dirty(&rig));
    let host = m(&mut rig, MaskMessage::Save);
    pick(&mut rig, &host, Some(dir.path().join("m.stm")));
    assert!(!dirty(&rig));
    // The same edit again: every move already has it.
    line(&mut rig, MaskColor::Blue);
    assert!(!dirty(&rig));
    // A different value marks it once for the whole line.
    line(&mut rig, MaskColor::Red);
    assert!(dirty(&rig));
}

#[test]
fn a_line_comment_reaches_every_move_of_the_line() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    play_line(&mut rig, &["e2e4", "e7e5"]);
    m(
        &mut rig,
        MaskMessage::Edit {
            target: san("Nf3"),
            edit: MaskEdit::Comment("main line".to_owned()),
            scope: MaskScope::Line,
        },
    );
    let hits = found(
        &mut rig,
        MaskQuery {
            move_text: Some("MAIN LINE".to_owned()),
            ..MaskQuery::default()
        },
    );
    assert_eq!(hits.moves_found, 3);
}

#[test]
fn a_line_marker_and_nag_apply_to_the_whole_line() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    play_line(&mut rig, &["e2e4", "e7e5"]);
    for edit in [
        MaskEdit::Marker {
            slot: MarkerSlot::First,
            marker: Some(Marker::ToTrain),
        },
        MaskEdit::Nag(Some(MaskNag::Interesting)),
    ] {
        m(
            &mut rig,
            MaskMessage::Edit {
                target: san("Nf3"),
                edit,
                scope: MaskScope::Line,
            },
        );
    }
    let by_marker = found(
        &mut rig,
        MaskQuery {
            marker1: Some(Some(Marker::ToTrain)),
            ..MaskQuery::default()
        },
    );
    assert_eq!(by_marker.moves_found, 3);
    let by_nag = found(
        &mut rig,
        MaskQuery {
            nag: Some(Some(MaskNag::Interesting)),
            ..MaskQuery::default()
        },
    );
    assert_eq!(by_nag.moves_found, 3);
}

#[test]
fn removing_along_the_line_from_the_middle_keeps_what_is_before_the_cursor() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    play_line(&mut rig, &["e2e4", "e7e5", "g1f3", "b8c6"]);
    m(&mut rig, MaskMessage::FillGame);
    assert_eq!(view(&rig).positions, 4);
    // The cursor goes after 1.e4; the clicked move stands for 1...e5.
    rig.nav(Nav::Start);
    rig.nav(Nav::Forward(1));
    m(
        &mut rig,
        MaskMessage::Edit {
            target: san("e5"),
            edit: MaskEdit::Remove,
            scope: MaskScope::Line,
        },
    );
    // e5, Nf3 and Nc6 are gone; 1.e4 stays (its position is the only one).
    assert_eq!(view(&rig).positions, 1);
    let hits = found(&mut rig, MaskQuery::default());
    assert_eq!(sans(&hits), ["e4"]);
}

#[test]
fn removing_along_the_line_of_moves_that_are_not_there_changes_nothing() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    play_line(&mut rig, &["e2e4", "e7e5"]);
    rig.nav(Nav::Start);
    m(
        &mut rig,
        MaskMessage::Edit {
            target: san("e4"),
            edit: MaskEdit::Remove,
            scope: MaskScope::Line,
        },
    );
    assert!(!dirty(&rig));
    assert_eq!(view(&rig).positions, 0);
}

#[test]
fn an_edit_by_the_row_of_the_total_or_out_of_range_is_ignored() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    for target in [
        MaskTarget::Row(999),
        MaskTarget::MaskOnly(0),
        MaskTarget::MaskOnly(usize::MAX),
        MaskTarget::Row(usize::MAX),
    ] {
        let out = rig.raw(TreeMessage::Mask(MaskMessage::Edit {
            target: target.clone(),
            edit: MaskEdit::Add,
            scope: MaskScope::Line,
        }));
        assert!(out.is_empty(), "{target:?}");
        assert!(!dirty(&rig), "{target:?}");
    }
    assert_eq!(rig.state.mask_comment(Some(&MaskTarget::Row(999))), None);
}

#[test]
fn an_edit_by_the_end_row_is_ignored() {
    let dir = dir();
    let mut rig = Rig::new();
    // `1.e4` ends in the game: the `[end]` row is there after 1.e4.
    rig.open_pgn(dir.path(), "end.pgn", "[Result \"*\"]\n\n1. e4 *\n\n");
    rig.send(TreeMessage::Open);
    m(&mut rig, MaskMessage::New);
    rig.play("e2e4");
    let end = rig
        .view()
        .rows
        .iter()
        .position(|r| r.is_end)
        .expect("an [end] row");
    assert!(rig.view().rows[end].mask.is_none());
    m(
        &mut rig,
        MaskMessage::Edit {
            target: MaskTarget::Row(end),
            edit: MaskEdit::Add,
            scope: MaskScope::One,
        },
    );
    assert!(!dirty(&rig));
}

#[test]
fn a_mask_only_row_is_edited_by_its_index_and_stays_where_it_is() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    edit(&mut rig, "c4", MaskEdit::Add);
    edit(&mut rig, "b3", MaskEdit::Add);
    m(
        &mut rig,
        MaskMessage::Edit {
            target: MaskTarget::MaskOnly(1),
            edit: MaskEdit::Color(MaskColor::Yellow),
            scope: MaskScope::One,
        },
    );
    let rows = view(&rig).only_rows;
    assert_eq!(
        rows.iter().map(|r| r.san.as_str()).collect::<Vec<_>>(),
        ["c4", "b3"],
        "the order of the mask does not change"
    );
    assert_eq!(rows[0].cell.color, MaskColor::White);
    assert_eq!(rows[1].cell.color, MaskColor::Yellow);
    // Removing by the index takes the entry away.
    m(
        &mut rig,
        MaskMessage::Edit {
            target: MaskTarget::MaskOnly(0),
            edit: MaskEdit::Remove,
            scope: MaskScope::One,
        },
    );
    let rows = view(&rig).only_rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].san, "b3");
}

#[test]
fn comments_are_cleaned_cut_and_previewed_in_the_view() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    // Spaces only: the same as empty, so nothing on the position changes.
    m(
        &mut rig,
        MaskMessage::SetPositionComment("  \n\t ".to_owned()),
    );
    assert!(!dirty(&rig));
    // CRLF becomes LF and the ends are trimmed.
    edit(
        &mut rig,
        "c4",
        MaskEdit::Comment("  first\r\nsecond \r\n".to_owned()),
    );
    assert_eq!(
        rig.state.mask_comment(Some(&san("c4"))).as_deref(),
        Some("first\nsecond")
    );
    // A huge comment is cut; the view only carries a short preview.
    let big = "é".repeat(100_000);
    edit(&mut rig, "b3", MaskEdit::Comment(big));
    let whole = rig.state.mask_comment(Some(&san("b3"))).expect("comment");
    assert!(whole.len() <= 64 * 1024, "{} bytes", whole.len());
    assert!(!whole.is_empty());
    let row = view(&rig)
        .only_rows
        .into_iter()
        .find(|r| r.san == "b3")
        .expect("b3");
    assert!(row.cell.comment.len() <= mychess_app::MAX_PREVIEW_BYTES);
    assert!(row.cell.comment.chars().all(|c| c == 'é'));
    assert!(row.cell.comment_more);
    // The position comment: the same.
    m(
        &mut rig,
        MaskMessage::SetPositionComment(format!("head\n{}", "x".repeat(300_000))),
    );
    let v = view(&rig);
    assert_eq!(v.position_comment, "head");
    assert!(v.position_comment_more);
    assert!(rig.state.mask_comment(None).expect("text").len() <= 64 * 1024 + 8);
}

#[test]
fn setting_the_same_marker_twice_is_one_change() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    let mark = |rig: &mut Rig, slot, marker| {
        edit(rig, "e4", MaskEdit::Marker { slot, marker });
    };
    mark(&mut rig, MarkerSlot::First, Some(Marker::Include));
    let host = m(&mut rig, MaskMessage::Save);
    pick(&mut rig, &host, Some(dir.path().join("m.stm")));
    assert!(!dirty(&rig));
    mark(&mut rig, MarkerSlot::First, Some(Marker::Include));
    mark(&mut rig, MarkerSlot::Second, None);
    assert!(!dirty(&rig));
    mark(&mut rig, MarkerSlot::Second, Some(Marker::Dubious));
    assert!(dirty(&rig));
    let cell = rig.row("e4").mask.expect("cell");
    assert_eq!(cell.markers, [Some(Marker::Include), Some(Marker::Dubious)]);
}

#[test]
fn an_edit_with_a_text_that_is_no_move_does_not_break_the_mask() {
    let dir = dir();
    let mut rig = rig_with_mask(dir.path());
    for text in ["", "   ", "zz9", "e4e4", "{}", "\u{0}", "O-O-O-O"] {
        edit(&mut rig, text, MaskEdit::Add);
        edit(&mut rig, text, MaskEdit::Comment("c".to_owned()));
        edit(&mut rig, text, MaskEdit::Remove);
    }
    // The mask is usable and saves.
    edit(&mut rig, "e4", MaskEdit::Add);
    let host = m(&mut rig, MaskMessage::Save);
    pick(&mut rig, &host, Some(dir.path().join("m.stm")));
    assert!(!dirty(&rig));
    let (read, _) = mychess_db::tree::mask::stm::read(&dir.path().join("m.stm")).unwrap();
    assert!(
        read.position(&mychess_core::Position::new())
            .is_some_and(|p| p.moves.iter().any(|m| m.san == "e4"))
    );
}

// ---- the limit of positions (D1850) ----

#[test]
fn a_mask_with_the_largest_number_of_positions_says_so_when_a_fill_has_no_room() {
    let dir = dir();
    let path = dir.path().join("big.stm");
    let mut text = String::from("set ::tree::mask::maskSerialized {");
    for n in 0..1_000_050 {
        text.push_str(&format!("{{k{n}}} {{{{}} c}} "));
    }
    text.push_str("}\n");
    std::fs::write(&path, text).unwrap();
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::Open(Some(path)));
    assert_eq!(view(&rig).positions, 1_000_000);
    assert!(matches!(
        notice(&rig),
        Some(MaskNotice::Warnings { count }) if count >= 1
    ));
    rig.play("e2e4");
    m(&mut rig, MaskMessage::FillGame);
    assert_eq!(notice(&rig), Some(MaskNotice::Full));
    assert_eq!(view(&rig).positions, 1_000_000);
}
