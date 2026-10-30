//! Shared helpers of the `m3_9_export_*` contract tests (spec
//! `docs/specs/m3-9-exportar-partidas.md`, D2000-D2029, app side). Only the
//! public API of `mychess-app` is used; the jobs run for real.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_push_string,
    clippy::format_collect
)]

#[path = "../m3_8_common/mod.rs"]
pub mod c;

use std::path::{Path, PathBuf};

pub use c::{Rig, game, games, games_on_disk, make_si4, pick_of, white_of, write};
pub use mychess_app::{
    AppNotice, BaseError, BasesMessage, Effect, ExportScope, FileError, FileErrorKind,
    GamesExportMessage, GamesExportNotice, GamesExportStepView, ListJobKind, ListMessage, Message,
    PickKind, SlotId,
};
use mychess_db::si4::{FlagOp, GameFlags, Si4Writer};

/// `n` games with white players `P1..Pn`.
pub fn numbered(n: usize) -> String {
    (1..=n)
        .map(|i| {
            format!(
                "[Event \"E\"]\n[Site \"S\"]\n[Date \"2020.01.01\"]\n[Round \"1\"]\n[White \"P{i}\"]\n[Black \"B{i}\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 Nc6 1-0\n\n"
            )
        })
        .collect()
}

pub fn list(message: ListMessage) -> BasesMessage {
    BasesMessage::List(message)
}

/// Sets the Tactics flag of `games` in the `.si4` at `path`.
pub fn flag_tactics(path: &Path, games: &[u32]) {
    let mut w = Si4Writer::open(path).unwrap();
    for g in games {
        w.set_flag(*g, GameFlags::TACTICS, FlagOp::Set).unwrap();
    }
}

/// The white players written in a PGN text, in order.
pub fn whites_in(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.strip_prefix("[White \""))
        .map(|l| l.trim_end_matches("\"]").to_owned())
        .collect()
}

pub fn text_of(path: &Path) -> String {
    String::from_utf8(std::fs::read(path).unwrap()).unwrap()
}

/// Whether anything of an export is left next to `dest` (a temporary).
pub fn has_temp(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .unwrap()
        .any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".tmp"))
}

pub fn notice(rig: &Rig) -> Option<GamesExportNotice> {
    match &rig.state.notice {
        Some(AppNotice::GamesExport(n)) => Some(n.clone()),
        _ => None,
    }
}

impl Rig {
    pub fn gx(&mut self, message: GamesExportMessage) {
        self.raw(message);
    }

    /// "Choose destination..." and the answer of the dialog with `dest`.
    /// Returns the effects of the answer (the job, not performed) and the
    /// kind of dialog asked.
    pub fn gx_choose(&mut self, dest: &Path) -> (Vec<Effect>, PickKind) {
        let effects = self.raw(GamesExportMessage::Choose);
        let spec = effects
            .iter()
            .find_map(|e| match e {
                Effect::PickFile(spec) => Some(spec.clone()),
                _ => None,
            })
            .expect("a file dialog");
        let kind = spec.kind;
        (
            self.raw(Message::FilePicked(spec.request, Some(dest.to_path_buf()))),
            kind,
        )
    }

    /// Performs the effects and feeds the answers back.
    pub fn gx_run(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            assert!(!effect.runs_on_host(), "{effect:?}");
            let answer = self.perform(effect);
            self.send(answer);
        }
    }

    /// Open, choose to `dest` and run to the end.
    pub fn gx_all(&mut self, dest: &Path) {
        self.gx(GamesExportMessage::Open);
        let (effects, _) = self.gx_choose(dest);
        self.gx_run(effects);
    }
}
