//! Compatibility checks against a Scid database (spec
//! `docs/specs/m6-1-buscas-avancadas.md`, last criterion and decision 8 of the
//! orchestrator). There is no database with known answers in the repository:
//! these tests read the base named by `MYCHESS_TEST_SCID_BASE` (the path
//! without extension) and check the searches against each other and against
//! the opening tree, which counts the games of a position by itself.
#![allow(clippy::unwrap_used, clippy::field_reassign_with_default)]

use std::path::PathBuf;

use mychess_core::Position;
use mychess_db::si4::Si4Database;
use mychess_db::tree::{NoCache, TreeOptions, TreeQuery, TreeState, search as tree_search};
use mychess_db::{
    BoardMatch, BoardSearch, CancelToken, Filter, MaterialSearch, NoProgress, search_board,
    search_material,
};

fn scid_base() -> PathBuf {
    let Some(path) = std::env::var_os("MYCHESS_TEST_SCID_BASE") else {
        panic!("set MYCHESS_TEST_SCID_BASE to the path of a Scid database (without extension)");
    };
    PathBuf::from(path)
}

fn after(moves: &str) -> Position {
    let mut pos = Position::new();
    for san in moves.split_whitespace() {
        let parsed = pos.parse_san(san, mychess_core::SanMode::Tolerant).unwrap();
        pos.play(parsed.mv).unwrap();
    }
    pos
}

#[test]
#[ignore = "requires MYCHESS_TEST_SCID_BASE pointing to a Scid database"]
fn compat_exact_search_counts_the_games_the_tree_counts() {
    let mut db = Si4Database::open(&scid_base()).unwrap();
    for moves in ["e4", "e4 e5", "d4 d5 c4", "e4 c5 Nf3"] {
        let pos = after(moves);
        let mut filter = Filter::new(db.len());
        let report = search_board(
            &mut db,
            &BoardSearch::new(pos.clone()),
            &mut filter,
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
        let main = Filter::new(db.len());
        let tree = tree_search(
            &mut db,
            &TreeQuery {
                position: &pos,
                ply: 0,
                main_filter: &main,
            },
            &TreeOptions::default(),
            &mut TreeState::default(),
            &mut NoCache,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
        let counted = tree.total.map_or(0, |t| t.total);
        assert_eq!(report.matched, counted, "{moves}");
    }
}

#[test]
#[ignore = "requires MYCHESS_TEST_SCID_BASE pointing to a Scid database"]
fn compat_shortcuts_do_not_change_the_answers() {
    let mut db = Si4Database::open(&scid_base()).unwrap();
    let pos = after("e4 e5 Nf3 Nc6");
    for kind in [
        BoardMatch::Exact,
        BoardMatch::Pawns,
        BoardMatch::Files,
        BoardMatch::Material,
    ] {
        let mut q = BoardSearch::new(pos.clone());
        q.kind = kind;
        q.ignore_colors = true;
        let mut fast = Filter::new(db.len());
        search_board(&mut db, &q, &mut fast, &mut NoProgress, &CancelToken::new()).unwrap();
        q.shortcuts = false;
        let mut slow = Filter::new(db.len());
        search_board(&mut db, &q, &mut slow, &mut NoProgress, &CancelToken::new()).unwrap();
        assert_eq!(fast.as_bytes(), slow.as_bytes(), "{kind:?}");
    }
    let mut q = MaterialSearch::zero();
    q.white.pawn = mychess_db::PieceRange::new(0, 8);
    q.black.pawn = mychess_db::PieceRange::new(0, 8);
    q.white.rook = mychess_db::PieceRange::new(1, 2);
    q.black.rook = mychess_db::PieceRange::new(1, 2);
    let mut fast = Filter::new(db.len());
    search_material(&mut db, &q, &mut fast, &mut NoProgress, &CancelToken::new()).unwrap();
    q.shortcuts = false;
    let mut slow = Filter::new(db.len());
    search_material(&mut db, &q, &mut slow, &mut NoProgress, &CancelToken::new()).unwrap();
    assert_eq!(fast.as_bytes(), slow.as_bytes());
}
