//! Contract tests of the local Syzygy source (spec m8-8, D2555, D2558,
//! D2559, D2561; acceptance criteria 3 and 4). The files here are synthetic;
//! the tests with real tables need `MYCHESS_TEST_SYZYGY`.

#![allow(clippy::assert_is_empty, clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use mychess_core::Position;
use mychess_tablebase::local::MAX_DIRECTORIES;
use mychess_tablebase::{
    Category, DirStatus, FileIssueKind, LocalTablebase, PositionProbe, TablebaseError,
};

fn write(dir: &Path, name: &str, size: usize) {
    fs::write(dir.join(name), vec![0u8; size]).unwrap();
}

#[test]
fn an_empty_folder_opens_with_no_tables() {
    let dir = tempfile::tempdir().unwrap();
    let (tb, report) = LocalTablebase::open(&[dir.path().to_path_buf()]).unwrap();
    assert!(tb.is_empty() && report.is_empty());
    assert_eq!(report.max_pieces, 0);
    assert_eq!(report.directories[0].status, DirStatus::Read);
    let pos = Position::from_fen("8/8/8/8/8/2k5/8/KQ6 w - - 0 1").unwrap();
    assert_eq!(tb.probe(&pos).unwrap_err(), TablebaseError::NoTables);
    assert_eq!(tb.max_pieces(), 0);
}

#[test]
fn no_folder_at_all_is_an_empty_set() {
    let (tb, report) = LocalTablebase::open(&[]).unwrap();
    assert!(tb.is_empty());
    assert!(report.directories.is_empty());
}

#[test]
fn a_missing_folder_and_a_file_in_place_of_a_folder_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope");
    let file = dir.path().join("file.txt");
    fs::write(&file, b"x").unwrap();
    let (_, report) = LocalTablebase::open(&[missing, file]).unwrap();
    assert_eq!(report.directories[0].status, DirStatus::NotADirectory);
    assert_eq!(report.directories[1].status, DirStatus::NotADirectory);
}

#[test]
fn too_many_folders_is_refused() {
    let dirs: Vec<PathBuf> = (0..=MAX_DIRECTORIES)
        .map(|i| PathBuf::from(format!("d{i}")))
        .collect();
    assert_eq!(
        LocalTablebase::open(&dirs).unwrap_err(),
        TablebaseError::TooManyDirectories {
            max: MAX_DIRECTORIES
        }
    );
}

#[test]
fn files_that_are_not_tables_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "readme.txt", 10);
    write(dir.path(), "KXvK.rtbw", 80); // not a material
    write(dir.path(), "notes.rtbw", 80);
    write(dir.path(), "KQvK.rtbq", 80); // not a table extension
    fs::create_dir(dir.path().join("KRvK.rtbw")).unwrap(); // a folder with a table name
    let (tb, report) = LocalTablebase::open(&[dir.path().to_path_buf()]).unwrap();
    assert!(tb.is_empty());
    assert_eq!(report.directories[0].wdl_files, 0);
}

#[test]
fn table_files_with_an_upper_case_extension_are_counted_not_used() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "KQvK.RTBW", 80);
    write(dir.path(), "KQvK.Rtbz", 80);
    write(dir.path(), "KRvK.rtbw", 80);
    write(dir.path(), "readme.TXT", 5);
    let (tb, report) = LocalTablebase::open(&[dir.path().to_path_buf()]).unwrap();
    let d = &report.directories[0];
    assert_eq!((d.wdl_files, d.dtz_files, d.uppercase_files), (1, 0, 2));
    assert_eq!(tb.max_pieces(), 3);
}

#[test]
fn a_file_with_the_wrong_size_is_reported_and_does_not_stop_the_others() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "KQvK.rtbw", 100); // truncated: size % 64 != 16
    write(dir.path(), "KRvK.rtbw", 80); // right size, junk content
    let (tb, report) = LocalTablebase::open(&[dir.path().to_path_buf()]).unwrap();
    let d = &report.directories[0];
    assert_eq!(d.wdl_files, 1);
    assert_eq!(d.max_pieces, 3);
    assert_eq!(d.issues.len(), 1);
    assert_eq!(d.issues[0].name, "KQvK.rtbw");
    assert_eq!(d.issues[0].kind, FileIssueKind::WrongSize);
    assert!(!tb.is_empty());
    assert_eq!(tb.max_pieces(), 3);
}

#[test]
fn a_corrupted_table_is_an_error_and_never_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "KRvK.rtbw", 80);
    write(dir.path(), "KRvK.rtbz", 80);
    let (tb, _) = LocalTablebase::open(&[dir.path().to_path_buf()]).unwrap();
    let pos = Position::from_fen("8/8/8/8/8/2k5/8/KR6 w - - 0 1").unwrap();
    let err = tb.probe(&pos).unwrap_err();
    assert_eq!(
        err,
        TablebaseError::Corrupt {
            material: "KRvK".to_owned()
        }
    );
    // And again: the failure is repeatable, not a poisoned state.
    assert_eq!(tb.probe(&pos).unwrap_err(), err);
}

#[test]
fn a_random_junk_table_never_panics() {
    let dir = tempfile::tempdir().unwrap();
    let mut junk = Vec::new();
    let mut x = 0x1234_5678_u32;
    for _ in 0..(64 * 40 + 16) {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        junk.push(u8::try_from(x >> 24).unwrap());
    }
    fs::write(dir.path().join("KQvK.rtbw"), &junk).unwrap();
    fs::write(dir.path().join("KQvK.rtbz"), &junk).unwrap();
    // The magic bytes of a WDL table followed by junk.
    junk[..4].copy_from_slice(&[0x71, 0xE8, 0x23, 0x5D]);
    fs::write(dir.path().join("KRvK.rtbw"), &junk).unwrap();
    fs::write(dir.path().join("KRvK.rtbz"), &junk).unwrap();
    let (tb, _) = LocalTablebase::open(&[dir.path().to_path_buf()]).unwrap();
    for fen in [
        "8/8/8/8/8/2k5/8/KQ6 w - - 0 1",
        "8/8/8/8/8/2k5/8/KR6 w - - 0 1",
        "8/8/8/8/8/2k5/8/KR6 b - - 0 1",
    ] {
        let pos = Position::from_fen(fen).unwrap();
        assert!(tb.probe(&pos).is_err(), "{fen}");
    }
}

#[test]
fn positions_outside_the_tables_are_typed_errors() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "KQvK.rtbw", 80);
    let (tb, _) = LocalTablebase::open(&[dir.path().to_path_buf()]).unwrap();
    let start = Position::new();
    assert!(matches!(
        tb.probe(&start).unwrap_err(),
        TablebaseError::Castling
    ));
    let many = Position::from_fen("8/8/8/8/3k4/8/2PPP3/KQ6 w - - 0 1").unwrap();
    assert!(matches!(
        tb.probe(&many).unwrap_err(),
        TablebaseError::TooManyPieces { pieces: 6, max: 3 }
    ));
    let kk = Position::from_fen("8/8/8/8/8/2k5/8/K7 w - - 0 1").unwrap();
    assert!(matches!(
        tb.probe(&kk).unwrap_err(),
        TablebaseError::TooFewPieces { pieces: 2 }
    ));
    // KRvK has no table in this set.
    let kr = Position::from_fen("8/8/8/8/8/2k5/8/KR6 w - - 0 1").unwrap();
    assert!(matches!(
        tb.probe(&kr).unwrap_err(),
        TablebaseError::MissingTable { .. }
    ));
}

#[test]
fn a_finished_game_needs_no_table() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "KQvK.rtbw", 80);
    let (tb, _) = LocalTablebase::open(&[dir.path().to_path_buf()]).unwrap();
    // Black is checkmated by the queen.
    let mate = Position::from_fen("k7/1Q6/1K6/8/8/8/8/8 b - - 0 1").unwrap();
    let r = tb.probe(&mate).unwrap();
    assert_eq!(r.evaluation.category, Category::Loss);
    assert!(r.moves.is_empty());
    // Black is stalemated.
    let stale = Position::from_fen("k7/2Q5/1K6/8/8/8/8/8 b - - 0 1").unwrap();
    assert_eq!(
        tb.probe(&stale).unwrap().evaluation.category,
        Category::Draw
    );
}

// ---- real tables (need the files; D2561) ----

fn real_dir() -> PathBuf {
    let dir = std::env::var_os("MYCHESS_TEST_SYZYGY")
        .map(PathBuf::from)
        .expect("set MYCHESS_TEST_SYZYGY to a folder with KQvK, KRvK and KPvK (.rtbw and .rtbz)");
    assert!(dir.is_dir(), "{} is not a folder", dir.display());
    dir
}

#[test]
#[ignore = "requires MYCHESS_TEST_SYZYGY"]
fn real_kqvk_is_a_win_with_a_coherent_dtz() {
    let (tb, report) = LocalTablebase::open(&[real_dir()]).unwrap();
    assert!(report.max_pieces >= 3);
    let pos = Position::from_fen("8/8/8/8/8/2k5/8/KQ6 w - - 0 1").unwrap();
    let r = tb.probe(&pos).unwrap();
    assert_eq!(r.evaluation.category, Category::Win);
    let dtz = r.evaluation.dtz.expect("the .rtbz file is there");
    assert!(dtz > 0);
    assert!(r.evaluation.dtm.is_none(), "Syzygy has no DTM (D2554)");
    // The best move keeps the win and is one ply closer.
    let best = r.best().unwrap();
    assert_eq!(best.evaluation.category, Category::Win);
    assert_eq!(best.evaluation.dtz, Some(dtz - 1));
}

#[test]
#[ignore = "requires MYCHESS_TEST_SYZYGY"]
fn real_krvk_lists_wins_and_losses_in_order() {
    let (tb, _) = LocalTablebase::open(&[real_dir()]).unwrap();
    let pos = Position::from_fen("8/8/8/8/8/2k5/8/KR6 w - - 0 1").unwrap();
    let r = tb.probe(&pos).unwrap();
    assert_eq!(r.evaluation.category, Category::Win);
    assert!(r.counts().wins > 0 && r.counts().draws > 0);
    let ranks: Vec<u8> = r
        .moves
        .iter()
        .map(|m| m.evaluation.category.rank())
        .collect();
    assert!(ranks.windows(2).all(|w| w[0] >= w[1]), "{ranks:?}");
    // The rook that is taken leaves K against K: a draw.
    let hang = Position::from_fen("8/8/8/8/8/1k6/1R6/7K b - - 0 1").unwrap();
    let r = tb.probe(&hang).unwrap();
    let take = r.moves.iter().find(|m| m.uci == "b3b2").unwrap();
    assert_eq!(take.evaluation.category, Category::Draw);
}

#[test]
#[ignore = "requires MYCHESS_TEST_SYZYGY"]
fn real_kpvk_gives_a_result_for_both_sides() {
    let (tb, _) = LocalTablebase::open(&[real_dir()]).unwrap();
    let a = Position::from_fen("8/8/8/4k3/8/4K3/4P3/8 b - - 0 1").unwrap();
    let b = Position::from_fen("8/8/8/8/8/7k/4P3/4K3 w - - 0 1").unwrap();
    for pos in [&a, &b] {
        let r = tb.probe(pos).unwrap();
        assert_ne!(r.evaluation.category, Category::Unknown);
        assert!(!r.moves.is_empty());
    }
}
