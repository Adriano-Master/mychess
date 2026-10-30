//! Property tests of the PGN import and export (spec M3-3, acceptance
//! criteria "Importação" and "Exportação"): random legal games survive
//! PGN -> SI4 -> PGN -> SI4 with the same moves, comments and NAGs.
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation
)]

#[path = "common/m3_3b_support.rs"]
mod support;

use mychess_core::{Game, GameResult, Nag, NodeId, Position};
use mychess_db::si4::Si4Database;
use mychess_db::{ExportItem, ExportOptions, ImportOptions};
use mychess_pgn::write::Encoding;
use proptest::prelude::*;
use support::{canon, import_bytes, import_str};

type Op = (u8, u8, u8, u8);

/// A random game: legal moves chosen by index from random nodes (so there
/// are variations), with comments and NAGs on some of them.
fn build(ops: &[Op], fen: bool, tag_seed: u8) -> Game {
    let mut game = if fen {
        Game::from_position(
            Position::from_fen("r3k2r/pppq1ppp/2n5/8/8/2N5/PPPQ1PPP/R3K2R w KQkq - 0 1").unwrap(),
        )
    } else {
        Game::new()
    };
    let mut nodes: Vec<NodeId> = vec![game.root()];
    for &(p, m, kind, val) in ops {
        let parent = nodes[usize::from(p) % nodes.len()];
        let legal = game.position_at(parent).unwrap().legal_moves();
        if legal.is_empty() {
            continue;
        }
        let mv = legal[usize::from(m) % legal.len()];
        let id = game.add_move(parent, mv).unwrap();
        if !nodes.contains(&id) {
            nodes.push(id);
        }
        if kind & 1 == 1 {
            game.set_comment(id, &format!("note {val} \u{e9}")).unwrap();
        }
        if kind & 2 == 2
            && game.nags(id).unwrap().len() < 3
            && let Ok(nag) = Nag::new(u16::from(val % 139) + 1)
        {
            game.add_nag(id, nag).unwrap();
        }
    }
    let names = ["Anand, V", "Carlsen", "\u{c5}sa", "Zed", "Kasparov, Garry"];
    let t = game.tags_mut();
    t.white = names[usize::from(tag_seed) % names.len()].into();
    t.black = names[usize::from(tag_seed / 7) % names.len()].into();
    t.event = format!("Ev{}", tag_seed % 5);
    t.result = [
        GameResult::Unknown,
        GameResult::White,
        GameResult::Black,
        GameResult::Draw,
    ][usize::from(tag_seed) % 4];
    if tag_seed.is_multiple_of(3) {
        game.set_initial_comment("opening remark");
    }
    game
}

fn mainline_moves(game: &Game) -> Vec<mychess_core::Move> {
    game.mainline().map(|id| game.mv(id).unwrap()).collect()
}

fn options_for(variant: u8) -> ExportOptions {
    let mut o = if variant.is_multiple_of(2) {
        ExportOptions::default()
    } else {
        ExportOptions::scid_dialog()
    };
    o.encoding = match variant % 3 {
        0 => Encoding::Utf8,
        1 => Encoding::Utf8Bom,
        _ => Encoding::Latin1,
    };
    o
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    /// PGN -> SI4 -> PGN -> SI4 keeps the game; the mainline is the same
    /// sequence of moves as the generated game.
    #[test]
    fn pgn_si4_pgn_si4_keeps_moves_comments_and_nags(
        ops in prop::collection::vec(any::<Op>(), 0..40),
        fen in any::<bool>(),
        tag_seed in any::<u8>(),
        variant in any::<u8>(),
    ) {
        let original = build(&ops, fen, tag_seed);
        let text = canon(&original);

        let first = import_str(&text, &ImportOptions::default());
        prop_assert_eq!(first.report.games_added, 1, "{}", first.report);
        let mut db1 = first.open();
        let g1 = db1.game(1).unwrap();
        prop_assert_eq!(mainline_moves(&g1), mainline_moves(&original));

        let out_dir = tempfile::tempdir().unwrap();
        let out = out_dir.path().join("out.pgn");
        let items = vec![ExportItem::stored(&mut db1, 1)];
        let report = mychess_db::export_pgn(
            items,
            &out,
            &options_for(variant),
            &mut mychess_db::NoProgress,
            &mychess_db::CancelToken::new(),
        )
        .unwrap();
        prop_assert_eq!(report.written, 1);
        prop_assert_eq!(report.unrepresentable, 0);
        prop_assert!(report.skipped.is_empty());

        let second = import_bytes(&std::fs::read(&out).unwrap(), &ImportOptions::default());
        prop_assert_eq!(second.report.games_added, 1, "{}", second.report);
        let mut db2 = second.open();
        let g2 = db2.game(1).unwrap();
        prop_assert!(
            g1.same_content(&g2),
            "first:\n{}\nsecond:\n{}",
            canon(&g1),
            canon(&g2)
        );
        prop_assert_eq!(mainline_moves(&g2), mainline_moves(&original));
        // the second generation is byte-identical in the record
        prop_assert_eq!(
            db1.record_bytes(1).unwrap().to_vec(),
            db2.record_bytes(1).unwrap().to_vec()
        );
    }

    /// Several games in one file keep their number and order.
    #[test]
    fn many_games_keep_their_order_through_export_and_import(
        specs in prop::collection::vec(
            (prop::collection::vec(any::<Op>(), 0..15), any::<u8>()),
            1..8,
        ),
    ) {
        let games: Vec<Game> = specs.iter().map(|(ops, seed)| build(ops, false, *seed)).collect();
        let text: String = games.iter().map(|g| canon(g) + "\n").collect();
        let first = import_str(&text, &ImportOptions::default());
        prop_assert_eq!(first.report.games_added as usize, games.len());
        let mut db1 = first.open();
        let items: Vec<ExportItem> = (1..=db1.len()).map(|n| ExportItem::stored(&mut db1, n)).collect();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("many.pgn");
        mychess_db::export_pgn(
            items,
            &out,
            &ExportOptions::scid_dialog(),
            &mut mychess_db::NoProgress,
            &mychess_db::CancelToken::new(),
        )
        .unwrap();
        let second = import_bytes(&std::fs::read(&out).unwrap(), &ImportOptions::default());
        prop_assert_eq!(second.report.games_added as usize, games.len());
        let mut db2: Si4Database<_> = second.open();
        for (i, g) in games.iter().enumerate() {
            let n = u32::try_from(i).unwrap() + 1;
            prop_assert_eq!(mainline_moves(&db2.game(n).unwrap()), mainline_moves(g), "game {}", n);
            prop_assert!(db1.game(n).unwrap().same_content(&db2.game(n).unwrap()), "game {}", n);
        }
    }
}
