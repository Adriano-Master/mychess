//! `list_page` against a custom `GameSource` (spec M3-4, D509, D521): only the
//! index and the names are read, unreadable entries become flagged rows, the
//! filter selects and orders the games.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_push_string
)]

mod m3_4a_common;
use std::cell::Cell;
use std::fs::File;

use m3_4a_common::*;
use mychess_app::list_page;
use mychess_core::Game;
use mychess_db::si4::{
    Header, IndexEntry, NameBase, RawGame, Result as DbResult, Si4Database, Si4Error,
};
use mychess_db::{Filter, GameSource};

/// An SI4 database whose chosen entries cannot be read, counting the reads of
/// records.
struct Probe {
    inner: Si4Database<File>,
    broken: Vec<u32>,
    entries_read: Cell<u32>,
    records_read: u32,
}

impl GameSource for Probe {
    fn header(&self) -> &Header {
        self.inner.header()
    }
    fn len(&self) -> u32 {
        self.inner.len()
    }
    fn entry(&self, n: u32) -> DbResult<IndexEntry> {
        self.entries_read.set(self.entries_read.get() + 1);
        if self.broken.contains(&n) {
            return Err(Si4Error::InvalidGameNumber {
                number: n,
                len: self.inner.len(),
            });
        }
        self.inner.entry(n)
    }
    fn names(&self) -> &NameBase {
        self.inner.names()
    }
    fn record_bytes(&mut self, _n: u32) -> DbResult<&[u8]> {
        self.records_read += 1;
        Err(Si4Error::InvalidGameNumber { number: 0, len: 0 })
    }
    fn raw_game(&mut self, _n: u32) -> DbResult<RawGame> {
        self.records_read += 1;
        Err(Si4Error::InvalidGameNumber { number: 0, len: 0 })
    }
    fn game(&mut self, _n: u32) -> DbResult<Game> {
        self.records_read += 1;
        Err(Si4Error::InvalidGameNumber { number: 0, len: 0 })
    }
}

fn probe(broken: &[u32], n: usize) -> (tempfile::TempDir, Probe) {
    let dir = tempfile::tempdir().unwrap();
    let mut pgn = String::new();
    for i in 1..=n {
        pgn.push_str(&format!("[White \"W{i}\"]\n[Black \"B\"]\n\n1. e4 *\n\n"));
    }
    let path = make_si4(dir.path(), "p", &pgn);
    let inner = Si4Database::open(&path).unwrap();
    (
        dir,
        Probe {
            inner,
            broken: broken.to_vec(),
            entries_read: Cell::new(0),
            records_read: 0,
        },
    )
}

#[test]
fn an_unreadable_entry_is_a_flagged_row_and_the_page_goes_on() {
    let (_d, p) = probe(&[2], 4);
    let rows = list_page(&p, &Filter::new(4), 1, 10);
    assert_eq!(
        rows.iter().map(|r| r.number).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert_eq!(
        rows.iter().map(|r| r.unreadable).collect::<Vec<_>>(),
        [false, true, false, false]
    );
    assert_eq!(rows[2].white, "W3");
    assert!(rows[1].white.is_empty() && rows[1].site.is_empty());
}

#[test]
fn every_entry_unreadable_still_returns_all_rows() {
    let (_d, p) = probe(&[1, 2, 3], 3);
    let rows = list_page(&p, &Filter::new(3), 1, 10);
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r.unreadable));
}

#[test]
fn a_page_reads_only_the_index_never_the_game_records() {
    let (_d, p) = probe(&[], 50);
    let rows = list_page(&p, &Filter::new(50), 10, 5);
    assert_eq!(rows.len(), 5);
    assert_eq!(p.records_read, 0, "the list must not decode games");
    assert_eq!(p.entries_read.get(), 5, "only the visible rows are read");
}

#[test]
fn the_filter_selects_the_games_and_positions_count_inside_it() {
    let (_d, p) = probe(&[], 6);
    let mut filter = Filter::new(6);
    filter.set(1, 0);
    filter.set(4, 0);
    assert_eq!(filter.count(), 4);
    let numbers = |first, count| {
        list_page(&p, &filter, first, count)
            .iter()
            .map(|r| r.number)
            .collect::<Vec<_>>()
    };
    assert_eq!(numbers(1, 10), [2, 3, 5, 6]);
    assert_eq!(numbers(2, 2), [3, 5]);
    assert_eq!(numbers(4, 10), [6]);
    assert!(numbers(5, 10).is_empty());
    let mut none = Filter::new(6);
    none.fill(0);
    assert!(list_page(&p, &none, 1, 10).is_empty());
}

#[test]
fn a_filter_of_another_length_gives_no_rows() {
    let (_d, p) = probe(&[], 3);
    assert!(list_page(&p, &Filter::new(4), 1, 10).is_empty());
    assert!(list_page(&p, &Filter::new(2), 1, 10).is_empty());
}

#[test]
fn a_huge_count_does_not_allocate_beyond_the_filter() {
    let (_d, p) = probe(&[], 3);
    assert_eq!(list_page(&p, &Filter::new(3), 1, u32::MAX).len(), 3);
    assert_eq!(list_page(&p, &Filter::new(3), u32::MAX, u32::MAX).len(), 0);
}
