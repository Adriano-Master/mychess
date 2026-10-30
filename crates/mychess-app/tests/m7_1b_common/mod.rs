//! Shared helpers of the `m7_1b_*` contract tests (spec
//! `m7-1-manutencao.md`, slice M7-1b).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_push_string
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub use mychess_app::maintenance::{
    CleanerPrefs, ConfirmKind, FlagAction, FlagScope, HeaderEdit, MaintFlag, MaintJobKind,
    MaintNotice, MaintReport, MaintRequest, MaintenanceEffect, MaintenanceMessage,
    MaintenancePrefs, NameKindEdit, NameRequest, NameRequestKind, NameScopeChoice, ScopeChoice,
    SortCriterion, StripChoice, TagRequest, TwinPrefs, TwinSide,
};
pub use mychess_app::{
    AppNotice, AppState, BaseError, BaseKind, BasesMessage, BasesNotice, Effect, ListMessage,
    Message, PrefChange, PrefsMessage, SlotId, SpellingMessage, TreeMessage, perform, update,
};
use mychess_db::GameSource;
use mychess_db::si4::{Si4Database, Si4Writer};

/// One game of a sample base.
#[derive(Clone)]
pub struct G {
    pub event: &'static str,
    pub site: &'static str,
    pub date: &'static str,
    pub white: &'static str,
    pub black: &'static str,
    pub result: &'static str,
    pub welo: u16,
    pub belo: u16,
    pub moves: &'static str,
}

pub const OPEN: &str = "1. e4 e5 2. Nf3 Nc6";

/// A game with the default moves.
#[allow(clippy::too_many_arguments)]
pub fn g(
    event: &'static str,
    site: &'static str,
    date: &'static str,
    white: &'static str,
    black: &'static str,
    result: &'static str,
    welo: u16,
    belo: u16,
) -> G {
    G {
        event,
        site,
        date,
        white,
        black,
        result,
        welo,
        belo,
        moves: OPEN,
    }
}

/// The six games of the spec (`m6-3`, "Critérios de aceite").
pub fn six() -> Vec<G> {
    vec![
        g(
            "Aberto",
            "Rio BRA",
            "2020.01.10",
            "Alfa, A",
            "Beta, B",
            "1-0",
            2500,
            2400,
        ),
        g(
            "Aberto",
            "Rio BRA",
            "2020.01.11",
            "Beta, B",
            "Gama, G",
            "1/2-1/2",
            2400,
            0,
        ),
        g(
            "Aberto",
            "Rio BRA",
            "2020.01.12",
            "Gama, G",
            "Alfa, A",
            "0-1",
            0,
            2510,
        ),
        g(
            "Aberto",
            "Rio BRA",
            "2020.05.20",
            "Alfa, A",
            "Beta, B",
            "1-0",
            2520,
            2410,
        ),
        g(
            "Solo",
            "Lima PER",
            "????.??.??",
            "Alfa, A",
            "Delta, D",
            "1/2-1/2",
            0,
            0,
        ),
        g(
            "Solo",
            "Lima PER",
            "????.??.??",
            "Delta, D",
            "Alfa, A",
            "*",
            0,
            0,
        ),
    ]
}

/// The duplicate base of the spec (A1..A5).
pub fn twins() -> Vec<G> {
    let base = |date, moves| G {
        moves,
        ..g("Aberto", "Rio", date, "Alfa, A", "Beta, B", "*", 0, 0)
    };
    vec![
        base("2020.01.10", "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6"),
        base("2020.01.10", "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6"),
        base("2020.01.10", "1. d4 d5 2. c4 e6"),
        base("2020.01.10", "1. e4 e5 2. Nf3 Nc6 {commented} 3. Bb5 a6"),
        base("2020.02.11", "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6"),
    ]
}

pub fn pgn(games: &[G]) -> String {
    let mut out = String::new();
    for x in games {
        let _ = write!(
            out,
            "[Event \"{}\"]\n[Site \"{}\"]\n[Date \"{}\"]\n[Round \"1\"]\n[White \"{}\"]\n\
             [Black \"{}\"]\n[Result \"{}\"]\n",
            x.event, x.site, x.date, x.white, x.black, x.result
        );
        if x.welo != 0 {
            let _ = writeln!(out, "[WhiteElo \"{}\"]", x.welo);
        }
        if x.belo != 0 {
            let _ = writeln!(out, "[BlackElo \"{}\"]", x.belo);
        }
        let _ = write!(out, "\n{} {}\n\n", x.moves, x.result);
    }
    out
}

/// A `.si4` database of `games` in `dir`; returns the path of the `.si4`.
pub fn make_si4(dir: &Path, name: &str, games: &[G]) -> PathBuf {
    use mychess_db::{CancelToken, ImportOptions, NoProgress, import_pgn};
    let base = dir.join(name);
    let mut writer = Si4Writer::create(&base, 0).expect("create si4");
    import_pgn(
        pgn(games).as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .expect("import");
    drop(writer);
    base.with_extension("si4")
}

pub struct Rig {
    pub state: AppState,
    rt: tokio::runtime::Runtime,
}

impl Rig {
    pub fn new() -> Rig {
        let mut state = AppState::default();
        state.open_pgn_in_slots = true;
        Rig {
            state,
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime"),
        }
    }

    /// `update` only; the caller answers the effects.
    pub fn raw(&mut self, message: impl Into<Message>) -> Vec<Effect> {
        update(&mut self.state, message.into())
    }

    pub fn perform(&self, effect: Effect) -> Message {
        self.rt.block_on(perform(effect))
    }

    /// `update` plus every non-host effect, answers fed back; returns the
    /// host effects.
    pub fn send(&mut self, message: impl Into<Message>) -> Vec<Effect> {
        let mut host = Vec::new();
        let mut queue = vec![message.into()];
        while let Some(message) = queue.pop() {
            for effect in update(&mut self.state, message) {
                if effect.runs_on_host() {
                    host.push(effect);
                } else if matches!(
                    effect,
                    Effect::ScheduleAutoplayTick { .. } | Effect::SavePreferences { .. }
                ) {
                } else {
                    queue.push(self.rt.block_on(perform(effect)));
                }
            }
        }
        host
    }

    /// Opens the `.si4` of `games` and returns its slot.
    pub fn open_si4(&mut self, dir: &Path, name: &str, games: &[G]) -> (SlotId, PathBuf) {
        let path = make_si4(dir, name, games);
        self.send(BasesMessage::Open(path.clone()));
        (self.state.bases.current(), path)
    }

    /// Opens a PGN of `games`.
    pub fn open_pgn(&mut self, dir: &Path, name: &str, games: &[G]) -> SlotId {
        let path = dir.join(name);
        std::fs::write(&path, pgn(games)).expect("write");
        self.send(BasesMessage::Open(path));
        self.state.bases.current()
    }

    /// Asks for an operation and performs what follows.
    pub fn ask(&mut self, request: MaintRequest) {
        self.send(MaintenanceMessage::Start(request));
    }

    /// The notice of the maintenance tools, if the last notice is one.
    pub fn maint_notice(&self) -> Option<MaintNotice> {
        match self.state.notice.as_ref() {
            Some(AppNotice::Maintenance(n)) => Some(n.clone()),
            _ => None,
        }
    }

    pub fn filter_numbers(&self, slot: SlotId) -> Vec<u32> {
        self.state
            .bases
            .slot(slot)
            .expect("slot")
            .filter()
            .iter_games()
            .collect()
    }

    pub fn set_prefs(&mut self, change: impl FnOnce(&mut MaintenancePrefs)) {
        let mut prefs = (*self.state.prefs.maintenance).clone();
        change(&mut prefs);
        self.send(MaintenanceMessage::SetPrefs(Box::new(prefs)));
    }

    pub fn select(&mut self, slot: SlotId, numbers: &[u32]) {
        self.send(BasesMessage::List(ListMessage::SelectNumbers {
            slot,
            numbers: numbers.to_vec(),
        }));
    }
}

/// The bytes of the three files of a database.
pub fn files_of(path: &Path) -> [Vec<u8>; 3] {
    ["si4", "sn4", "sg4"].map(|e| std::fs::read(path.with_extension(e)).expect("read"))
}

/// The `(white, black, event, date)` text of every game on disk.
pub fn disk_games(path: &Path) -> Vec<(String, String, String, String)> {
    let mut db = Si4Database::open(path).expect("open");
    (1..=db.len())
        .map(|n| {
            let g = db.game(n).expect("game");
            let t = g.tags();
            (
                t.white.clone(),
                t.black.clone(),
                t.event.clone(),
                t.date.to_string(),
            )
        })
        .collect()
}

/// The flags word of every game on disk.
pub fn disk_flags(path: &Path) -> Vec<u32> {
    let db = Si4Database::open(path).expect("open");
    (1..=db.len())
        .map(|n| db.entry(n).expect("entry").flags.bits())
        .collect()
}

/// The `D` flag of every game on disk.
pub fn disk_deleted(path: &Path) -> Vec<bool> {
    use mychess_db::si4::GameFlags;
    let db = Si4Database::open(path).expect("open");
    (1..=db.len())
        .map(|n| {
            db.entry(n)
                .expect("entry")
                .flags
                .contains(GameFlags::DELETE)
        })
        .collect()
}
