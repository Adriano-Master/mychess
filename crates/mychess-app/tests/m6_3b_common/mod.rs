//! Shared helpers of the `m6_3b_*` contract tests (spec
//! `m6-3-jogadores-torneios-spelling.md`, slice M6-3b).
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

pub use mychess_app::{
    AppNotice, AppState, BaseError, BaseKind, BasesMessage, BasesNotice, Effect, FinderMessage,
    FinderNotice, ListMessage, Message, PickKind, PlayerInfoMessage, PlayerInfoNotice, PrefChange,
    PrefsMessage, SlotId, SpellingMessage, SpellingNotice, perform, update,
};
use mychess_db::si4::Si4Writer;

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
}

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
    }
}

/// The six games of the spec ("Critérios de aceite").
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
        let _ = write!(out, "\n1. e4 e5 2. Nf3 Nc6 {}\n\n", x.result);
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

/// The spelling file of the tests: Alfa has `%Elo` for 2020, a biography with
/// a FIDE id and an alias; Gama and Delta only a peak rating.
pub const SSP: &str = "# test file\n\
@PLAYER \"., -_*\"\n\
Alfa, A #GM BRA [2520] 1980.01.01\n\
  %Bio FIDEID 1234567\n\
  %Bio Champion\n\
  %Elo 2020:2500,2501,2502,2503,2504,2505,2506,2507,2508,2509,2510,2511\n\
  = Alfa, Alf\n\
Gama, G #- BRA [2050] 1970.??.??\n\
Delta, D #IM ARG [2100*] 1960.01.01\n\
  = Delt, D\n\
@EVENT \",. -_\"\n\
Aberto Brasileiro\n\
  = Aberto\n\
@SITE \"., -_()\"\n\
Rio BRA\n\
  = Rio de Janeiro BRA\n\
@ROUND \"\"\n\
1\n";

/// The same without the `%Elo` line.
pub fn ssp_without_elo() -> String {
    SSP.lines()
        .filter(|l| !l.contains("%Elo"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

pub struct Rig {
    pub state: AppState,
    rt: tokio::runtime::Runtime,
    /// The data directory of the program for the test (no real spelling file
    /// and no photos).
    pub data: tempfile::TempDir,
}

impl Rig {
    pub fn new() -> Rig {
        let data = tempfile::tempdir().expect("data dir");
        let mut state = AppState::default();
        state.open_pgn_in_slots = true;
        state.set_player_data_dir(Some(data.path().to_path_buf()));
        Rig {
            state,
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime"),
            data,
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

    /// Writes `text` as a spelling file in the data directory and loads it.
    pub fn load_ssp(&mut self, text: &str) -> PathBuf {
        let path = self.data.path().join("spelling.ssp");
        std::fs::write(&path, text).expect("write ssp");
        self.send(SpellingMessage::Load(path.clone()));
        path
    }

    pub fn notice(&self) -> Option<&AppNotice> {
        self.state.notice.as_ref()
    }

    pub fn spelling_notice(&self) -> Option<SpellingNotice> {
        match self.state.notice.as_ref() {
            Some(AppNotice::Spelling(n)) => Some(n.clone()),
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
}

/// The names of the `White` and `Black` of every game on disk.
pub fn disk_players(path: &Path) -> Vec<(String, String)> {
    use mychess_db::GameSource;
    let mut db = mychess_db::si4::Si4Database::open(path).expect("open");
    (1..=db.len())
        .map(|n| {
            let g = db.game(n).expect("game");
            (g.tags().white.clone(), g.tags().black.clone())
        })
        .collect()
}

/// The Elo of both sides of every game on disk.
pub fn disk_elos(path: &Path) -> Vec<(u16, u16)> {
    use mychess_db::GameSource;
    let db = mychess_db::si4::Si4Database::open(path).expect("open");
    (1..=db.len())
        .map(|n| {
            let e = db.entry(n).expect("entry");
            (e.white_elo, e.black_elo)
        })
        .collect()
}
