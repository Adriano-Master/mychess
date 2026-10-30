//! Shared helpers of the `m3_8_import_*` contract tests (spec
//! `docs/specs/m3-8-importar-arquivo-pgn.md`, D1970-D1989, app side).
//!
//! Only the public API of `mychess-app` is used. The jobs run for real, on
//! real (small) databases and PGN files in a temporary directory.
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
    AppNotice, AppState, BaseError, BaseKind, BasesMessage, BasesNotice, Effect, FileImportMessage,
    FileImportNotice, ListJobKind, ListMessage, Message, PickKind, PickSpec, RequestId, SlotId,
    perform, update,
};
use mychess_db::GameSource;
use mychess_db::si4::{Si4Database, Si4Writer};

/// One game of a synthetic PGN.
pub fn game(n: usize) -> String {
    format!(
        "[Event \"Event {n}\"]\n[Site \"S\"]\n[Date \"2020.01.01\"]\n[Round \"1\"]\n[White \"White {n}\"]\n[Black \"Black {n}\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 Nc6 1-0\n\n"
    )
}

/// `count` games.
pub fn games(count: usize) -> String {
    (1..=count).map(game).collect()
}

pub fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write");
    path
}

pub fn write_bytes(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write");
    path
}

/// Builds `<dir>/<name>.si4` from PGN text.
pub fn make_si4(dir: &Path, name: &str, pgn: &str) -> PathBuf {
    use mychess_db::{CancelToken, ImportOptions, NoProgress, import_pgn};
    let base = dir.join(name);
    let mut writer = Si4Writer::create(&base, 0).expect("create si4");
    import_pgn(
        pgn.as_bytes(),
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

/// The number of games of a `.si4` file, read independently.
pub fn games_on_disk(path: &Path) -> u32 {
    let db = Si4Database::open(path).expect("open");
    GameSource::len(&db)
}

/// The name of the white player of game `n` of a `.si4` file.
pub fn white_of(path: &Path, n: u32) -> String {
    use mychess_db::si4::{NameKind, bytes_to_string};
    let db = Si4Database::open(path).expect("open");
    let entry = db.entry(n).expect("entry");
    let bytes = db
        .names()
        .get(NameKind::Player, entry.white_id)
        .expect("name");
    bytes_to_string(bytes).into_owned()
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

    /// `update` plus every non-host effect, answers fed back. Returns the
    /// host effects.
    pub fn send(&mut self, message: impl Into<Message>) -> Vec<Effect> {
        let mut host = Vec::new();
        let mut queue = vec![message.into()];
        while let Some(message) = queue.pop() {
            for effect in update(&mut self.state, message) {
                if effect.runs_on_host() {
                    host.push(effect);
                } else if matches!(effect, Effect::ScheduleAutoplayTick { .. }) {
                } else {
                    queue.push(self.rt.block_on(perform(effect)));
                }
            }
        }
        host
    }

    pub fn open_si4(&mut self, dir: &Path, name: &str, text: &str) -> (SlotId, PathBuf) {
        let path = make_si4(dir, name, text);
        self.send(BasesMessage::Open(path.clone()));
        (self.state.bases.current(), path)
    }

    pub fn open_pgn(&mut self, dir: &Path, name: &str, text: &str) -> SlotId {
        let path = write(dir, name, text);
        self.send(BasesMessage::Open(path));
        self.state.bases.current()
    }

    /// "Import PGN file..." and the answer of the dialog with `files`.
    /// Returns the effects of the answer (not performed).
    pub fn start_import(&mut self, files: &[PathBuf]) -> Vec<Effect> {
        let effects = self.raw(FileImportMessage::Open);
        let spec = pick_of(&effects, PickKind::ImportPgn);
        self.raw(Message::FilesPicked(spec.request, files.to_vec()))
    }

    /// Like [`Rig::start_import`], then performs the job and feeds back the
    /// answer. Returns the host effects.
    pub fn import(&mut self, files: &[PathBuf]) -> Vec<Effect> {
        let effects = self.start_import(files);
        let mut host = Vec::new();
        for effect in effects {
            if effect.runs_on_host() {
                host.push(effect);
            } else {
                let answer = self.perform(effect);
                host.extend(self.send(answer));
            }
        }
        host
    }

    pub fn current_path(&self) -> PathBuf {
        self.state
            .bases
            .slot(self.state.bases.current())
            .and_then(|s| s.path.clone())
            .expect("path")
    }

    pub fn current_total(&self) -> u32 {
        self.state
            .bases
            .slot(self.state.bases.current())
            .expect("slot")
            .total()
    }

    pub fn current_filtered(&self) -> u32 {
        self.state
            .bases
            .slot(self.state.bases.current())
            .expect("slot")
            .filtered()
    }

    pub fn notice(&self) -> Option<FileImportNotice> {
        match &self.state.notice {
            Some(AppNotice::FileImport(n)) => Some(n.clone()),
            _ => None,
        }
    }
}

/// The file dialog of `kind` among `effects`.
pub fn pick_of(effects: &[Effect], kind: PickKind) -> PickSpec {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::PickFile(spec) if spec.kind == kind => Some(spec.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no {kind:?} dialog among {effects:?}"))
}

pub fn is_import_job(effect: &Effect) -> bool {
    matches!(effect, Effect::ImportPgnFiles { .. })
}
