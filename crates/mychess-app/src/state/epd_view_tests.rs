//! Tests of the cheap questions the front-end asks about the EPD window:
//! which layer covers it, what would be lost on quitting, and what the
//! tabs compute (review of M7-5c, D3859-D3861).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use mychess_core::Position;
use mychess_pgn::epd::{EpdBook, EpdIndex};

use super::AppState;
use crate::epd_window::{BatchOp, EpdFile, EpdFileId, EpdSort, LoadReport, SortKey};

/// A book of `n` distinct positions: kings on e1/e8 and three more pieces.
fn book(n: usize) -> EpdBook {
    let mut book = EpdBook::new();
    let squares: Vec<usize> = (0..64).filter(|s| *s != 4 && *s != 60).collect();
    'all: for &a in &squares {
        for &b in &squares {
            for &c in &squares {
                if a == b || b == c || a == c {
                    continue;
                }
                if book.len() >= n {
                    break 'all;
                }
                let pieces = [(a, 'R'), (b, 'N'), (c, 'B'), (4, 'K'), (60, 'k')];
                let mut fen = String::new();
                for rank in (0..8).rev() {
                    let mut empty = 0;
                    for file in 0..8 {
                        if let Some((_, p)) = pieces.iter().find(|(s, _)| *s == rank * 8 + file) {
                            if empty > 0 {
                                fen.push_str(&empty.to_string());
                                empty = 0;
                            }
                            fen.push(*p);
                        } else {
                            empty += 1;
                        }
                    }
                    if empty > 0 {
                        fen.push_str(&empty.to_string());
                    }
                    if rank > 0 {
                        fen.push('/');
                    }
                }
                fen.push_str(" w - - 0 1");
                if let Ok(position) = Position::from_fen(&fen) {
                    book.insert(position, Vec::new());
                }
            }
        }
    }
    book
}

fn state_with(files: usize, records: usize) -> (AppState, Vec<EpdFileId>) {
    let mut state = AppState::default();
    let mut ids = Vec::new();
    for n in 0..files {
        let book = book(records);
        let rows: Vec<EpdIndex> = book.iter_indexed().map(|(i, _)| i).collect();
        let id = state.epd.new_id();
        state.epd.files.push(EpdFile::new(
            id,
            PathBuf::from(format!("/x/f{n}.epd")),
            Arc::new(book),
            false,
            Arc::new(rows),
            LoadReport::default(),
        ));
        ids.push(id);
    }
    state.epd.active = ids.first().copied();
    (state, ids)
}

#[test]
fn a_clean_window_has_nothing_unsaved_and_no_layer() {
    let (state, _) = state_with(2, 5);
    assert!(state.epd_unsaved_files().is_empty());
    assert!(!state.epd_layer_open());
}

#[test]
fn a_pending_removal_or_close_question_is_a_layer() {
    let (mut state, ids) = state_with(2, 5);
    let first = state.epd.files[1]
        .book
        .iter_indexed()
        .next()
        .map(|(i, _)| i);
    state.epd.files[1].pending_remove = first;
    assert!(state.epd_layer_open());
    state.epd.files[1].pending_remove = None;
    state.epd.close_prompt = Some(ids[0]);
    assert!(state.epd_layer_open());
}

#[test]
fn an_unconfirmed_draft_counts_as_unsaved() {
    let (mut state, ids) = state_with(2, 5);
    state.epd.files[1].draft = Some("bm e4".to_owned());
    assert_eq!(state.epd_unsaved_files(), vec![ids[1]]);
    // The marker of the file is still clean: only the draft tells.
    assert!(state.epd_dirty_files().is_empty());
}

#[test]
fn a_batch_edit_in_flight_counts_as_unsaved() {
    let (mut state, ids) = state_with(1, 5);
    state.epd.files[0].job = Some(crate::epd_window::Job {
        request: crate::error::RequestId(1),
        kind: crate::epd_window::JobKind::Batch(BatchOp::SortAll),
    });
    assert_eq!(state.epd_unsaved_files(), vec![ids[0]]);
}

#[test]
fn only_the_shown_tab_computes_the_row_and_the_book_moves() {
    let (mut state, ids) = state_with(2, 20);
    for file in &mut state.epd.files {
        file.sort = EpdSort {
            key: SortKey::Fen,
            descending: false,
        };
        file.selected = file.book.iter_indexed().map(|(i, _)| i).next();
    }
    let view = state.epd_view();
    assert!(view.tabs[0].selected_row.is_some(), "the active tab");
    assert!(view.tabs[1].selected_row.is_none(), "the other tab");
    assert_eq!(view.tabs[1].id, ids[1]);
    // With no active file the first tab is the one shown.
    state.epd.active = None;
    assert!(state.epd_view().tabs[0].selected_row.is_some());
}

/// 200 000 records ordered by position: the full view walks the rows for
/// the selected one, the question about the layer does not.
#[test]
#[ignore = "micro-benchmark: run with --ignored --nocapture"]
fn bench_layer_question_against_the_full_view() {
    let (mut state, _) = state_with(1, 200_000);
    let file = &mut state.epd.files[0];
    file.sort = EpdSort {
        key: SortKey::Fen,
        descending: false,
    };
    file.selected = file.rows.last().copied();
    let time = |f: &dyn Fn()| {
        let start = Instant::now();
        for _ in 0..20 {
            f();
        }
        start.elapsed() / 20
    };
    let full = time(&|| drop(std::hint::black_box(state.epd_view())));
    let cheap = time(&|| {
        std::hint::black_box(state.epd_layer_open());
    });
    eprintln!("epd_view: {full:?}, epd_layer_open: {cheap:?}");
    assert!(cheap < full);
}
