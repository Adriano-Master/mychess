//! D1055 of `docs/specs/m5-3-cache-stc.md`: the `BaseSource` wrapper of a slot
//! forwards `GameSource::is_scid_database`, so the priming of the tree cache
//! accepts an SI4 base and refuses a PGN file and the Clipbase.
#![allow(clippy::unwrap_used)]

use std::path::Path;

use mychess_app::{BaseSource, Clipbase};
use mychess_db::pgn_db::PgnDatabase;
use mychess_db::progress::NoProgress;
use mychess_db::si4::{Si4Database, Si4Writer};
use mychess_db::{CancelToken, GameSource, OpenOptions};

#[test]
fn an_si4_base_is_a_scid_database() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("games");
    drop(Si4Writer::create(&base, 1).unwrap());
    let si4 = Si4Database::open(&base).unwrap();
    assert!(BaseSource::Si4(Box::new(si4)).is_scid_database());
}

#[test]
fn a_pgn_base_is_not_a_scid_database() {
    let (pgn, _) = PgnDatabase::from_reader(
        &b"[Event \"?\"]\n\n1. e4 *\n\n"[..],
        Path::new("t.pgn"),
        &OpenOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert!(!BaseSource::Pgn(Box::new(pgn)).is_scid_database());
}

#[test]
fn the_clipbase_is_not_a_scid_database() {
    let clip = Clipbase::new(100).unwrap();
    assert!(!BaseSource::Clipbase(Box::new(clip)).is_scid_database());
}
