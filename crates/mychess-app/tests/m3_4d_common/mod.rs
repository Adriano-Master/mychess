//! Shared helpers of the `m3_4d_*` contract tests (spec
//! `m3-4-slots-clipbase-lista-de-partidas.md`, slice 4d: copying the filter
//! or the selection between databases).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_push_string,
    clippy::many_single_char_names
)]

#[path = "../m3_4c_common/mod.rs"]
pub mod c;

use std::path::{Path, PathBuf};

pub use c::{G, Rig, l, make_si4, numbered, pgn, sel, write};
pub use mychess_app::{
    AppNotice, BaseError, BaseKind, BasesMessage, BasesNotice, CopyEnd, CopyPrompt, CopyRefusal,
    CopyScope, Effect, ListColumn, ListJobKind, ListMessage, Message, PickMode, PrefChange,
    PrefsMessage, SlotId,
};
use mychess_db::GameSource;

/// Switches the "confirm before copying" preference.
pub fn confirm(rig: &mut Rig, on: bool) {
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::ConfirmCopy(
        on,
    ))));
}

pub fn copy_msg(from: SlotId, to: SlotId, scope: CopyScope) -> BasesMessage {
    l(ListMessage::CopyGames { from, to, scope })
}

/// `CopyGames` run to completion.
pub fn copy(rig: &mut Rig, from: SlotId, to: SlotId, scope: CopyScope) {
    rig.send(copy_msg(from, to, scope));
}

/// A name for a slot as the notices carry it.
pub fn end(slot: SlotId, name: &str) -> CopyEnd {
    CopyEnd {
        slot,
        name: name.to_owned(),
    }
}

/// White players of the list of `slot` in view order.
pub fn whites(rig: &Rig, slot: SlotId) -> Vec<String> {
    rig.state
        .bases
        .list_page(slot, 1, u32::MAX / 2)
        .iter()
        .map(|r| r.white.clone())
        .collect()
}

/// White players of a slot in database order (the view may be sorted).
pub fn db_whites(rig: &Rig, slot: SlotId) -> Vec<String> {
    let source = rig.state.bases.slot(slot).expect("slot").source.clone();
    let mut db = source.lock();
    (1..=GameSource::len(&*db))
        .map(|n| db.game(n).expect("game").tags().white.clone())
        .collect()
}

/// The PGN text of every game of a slot, in database order.
pub fn texts(rig: &Rig, slot: SlotId) -> Vec<String> {
    let source = rig.state.bases.slot(slot).expect("slot").source.clone();
    let mut db = source.lock();
    (1..=GameSource::len(&*db))
        .map(|n| {
            let game = db.game(n).expect("game");
            mychess_pgn::write::game_to_string(&game, &mychess_pgn::write::WriteOptions::default())
                .expect("pgn")
        })
        .collect()
}

pub fn flags_of(rig: &Rig, slot: SlotId) -> Vec<u32> {
    let source = rig.state.bases.slot(slot).expect("slot").source.clone();
    let db = source.read();
    (1..=GameSource::len(&*db))
        .map(|n| db.entry(n).expect("entry").flags.bits())
        .collect()
}

/// What an independent open of the `.si4` on disk says about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskGame {
    pub white: String,
    pub black: String,
    pub event: String,
    pub site: String,
    pub round: String,
    pub text: String,
    pub eco_code: u16,
    pub result: String,
    pub welo: u16,
    pub belo: u16,
    pub half_moves: u16,
    pub final_mat_sig: u32,
    pub flags: u32,
}

/// Opens `path` with a new `Si4Database`, reads every game and checks that
/// the three files agree (game count in the header, name frequencies, every
/// record decodes).
pub fn disk(path: &Path) -> Vec<DiskGame> {
    use mychess_db::si4::{NameKind, Si4Database};
    let mut db = Si4Database::open(path).expect("an independent open of the .si4");
    let len = GameSource::len(&db);
    let mut out = Vec::new();
    let (mut w, mut b, mut e, mut s, mut r) = (0u64, 0u64, 0u64, 0u64, 0u64);
    for n in 1..=len {
        let entry = db.entry(n).expect("entry");
        let game = db.game(n).expect("record decodes");
        let tags = game.tags();
        out.push(DiskGame {
            white: tags.white.clone(),
            black: tags.black.clone(),
            event: tags.event.clone(),
            site: tags.site.clone(),
            round: tags.round.clone(),
            text: mychess_pgn::write::game_to_string(
                &game,
                &mychess_pgn::write::WriteOptions::default(),
            )
            .expect("pgn"),
            eco_code: entry.eco_code,
            result: format!("{:?}", entry.result),
            welo: entry.white_elo,
            belo: entry.black_elo,
            half_moves: entry.num_half_moves,
            final_mat_sig: entry.final_mat_sig,
            flags: entry.flags.bits(),
        });
        let names = db.names();
        for (kind, id, acc) in [
            (NameKind::Player, entry.white_id, &mut w),
            (NameKind::Player, entry.black_id, &mut b),
            (NameKind::Event, entry.event_id, &mut e),
            (NameKind::Site, entry.site_id, &mut s),
            (NameKind::Round, entry.round_id, &mut r),
        ] {
            assert!(names.get(kind, id).is_some(), "game {n}: name {id} exists");
            *acc += 1;
        }
    }
    // Name frequencies add up to the games that use them.
    let names = db.names();
    let sum = |kind: NameKind| -> u64 {
        (0..names.num_names(kind))
            .map(|id| u64::from(names.frequency(kind, id).unwrap_or(0)))
            .sum()
    };
    assert_eq!(sum(NameKind::Player), w + b, "player frequencies");
    assert_eq!(sum(NameKind::Event), e, "event frequencies");
    assert_eq!(sum(NameKind::Site), s, "site frequencies");
    assert_eq!(sum(NameKind::Round), r, "round frequencies");
    out
}

/// A rich game: comments, a variation, NAGs, ratings, ECO.
pub fn rich(white: &str) -> G {
    G::new(white, "Opp")
        .event("Rich Open")
        .site("Paris FRA")
        .date("2021.03.04")
        .round("7")
        .result("1-0")
        .elo(2500, 2400)
        .eco("C20")
        .moves("1. e4 {Best by test} e5 (1... c5 2. Nf3 $1) 2. Nf3 $14 Nc6 3. Bb5 *")
}

/// A state with a PGN of `n` games in slot 1 and an `.si4` of `m` games in
/// slot 2; copy confirmation is off. Returns the slots and the `.si4` path.
pub struct Pair {
    pub rig: Rig,
    pub from: SlotId,
    pub to: SlotId,
    pub path: PathBuf,
    pub dir: tempfile::TempDir,
}

pub fn pair(source: &[G], target: &[G]) -> Pair {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let from = rig.open_games(dir.path(), "src.pgn", source);
    let path = make_si4(dir.path(), "dst", &pgn(target));
    let to = rig.open(&path);
    assert_ne!(from, to);
    confirm(&mut rig, false);
    Pair {
        rig,
        from,
        to,
        path,
        dir,
    }
}

pub fn notice_of(rig: &Rig) -> Option<BasesNotice> {
    rig.notice().cloned()
}

/// The refusal in the notice, if the notice is one.
pub fn refusal(rig: &Rig) -> Option<(CopyRefusal, CopyEnd, CopyEnd)> {
    match rig.notice() {
        Some(BasesNotice::CopyRefused { reason, from, to }) => {
            Some((*reason, from.clone(), to.clone()))
        }
        _ => None,
    }
}
