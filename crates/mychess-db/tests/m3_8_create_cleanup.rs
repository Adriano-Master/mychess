//! `Si4Writer::create` leaves no half-made database behind when the final
//! open fails (M3-8 review, D1992).
#![allow(clippy::unwrap_used, clippy::assertions_on_result_states)]

use mychess_db::si4::{Si4Error, Si4Writer};

#[test]
fn create_removes_the_files_it_made_when_the_final_open_fails() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("db");
    // A damaged journal of an earlier database of that name: the open
    // that ends `create` refuses it.
    let journal = dir.path().join("db.txn");
    std::fs::write(&journal, [0xFF, 0xFE, 0x00]).unwrap();
    let error = Si4Writer::create(&base, 0).expect_err("must fail");
    assert!(
        matches!(error, Si4Error::CorruptJournal { .. }),
        "{error:?}"
    );
    for ext in ["si4", "sn4", "sg4"] {
        assert!(!base.with_extension(ext).exists(), "{ext} left behind");
    }
    assert!(journal.exists(), "what create did not make stays");
}
