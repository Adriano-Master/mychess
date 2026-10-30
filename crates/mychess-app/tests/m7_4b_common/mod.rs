//! Shared helpers of the `m7_4b_*` contract tests (spec
//! `docs/specs/m7-4-exportacao.md`, M7-4b). Only the public API of
//! `mychess-app` is used; the jobs run for real in a temporary directory.
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

#[path = "../m3_9_common/mod.rs"]
pub mod c9;

use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use c9::*;
pub use mychess_app::board_image::{
    BoardImage, BoardPalette, ImageError, PieceSvgs, board_png, board_svg, namespace_svg_ids,
};
pub use mychess_app::{
    ExportAssets, ExportDocsMessage, ExportEdit, ExportFormat, ExportPrefs, ExportProblem,
    ImageFormat, ImageOutcome, PrefChange, Preferences, PrefsMessage, load_preferences,
    render_preferences,
};

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The pieces of `assets/pieces/<set>`.
pub fn real_pieces(set: &str) -> Arc<PieceSvgs> {
    use mychess_core::{Color, Role};
    let dir = repo_root().join("assets/pieces").join(set);
    Arc::new(
        PieceSvgs::new(|color, role| {
            let c = if color == Color::White { 'w' } else { 'b' };
            let r = match role {
                Role::King => 'K',
                Role::Queen => 'Q',
                Role::Rook => 'R',
                Role::Bishop => 'B',
                Role::Knight => 'N',
                Role::Pawn => 'P',
            };
            std::fs::read_to_string(dir.join(format!("{c}{r}.svg"))).unwrap()
        })
        .unwrap(),
    )
}

pub fn font() -> Vec<u8> {
    std::fs::read(repo_root().join("assets/fonts/NotoSans-Regular.ttf")).unwrap()
}

pub fn assets() -> ExportAssets {
    ExportAssets {
        pieces: real_pieces("cburnett"),
        palette: BoardPalette::default(),
        font: Arc::new(font()),
        pieces_credit: Some("cburnett by Colin M.L. Burnett, GPL-2.0-or-later".to_owned()),
    }
}

/// Two games with a variation, comments, NAG `!` and the diagram NAG `D`
/// ($201), names with `& < > "` and `_ $ % #`.
pub fn rich_pgn() -> String {
    let mut text = String::new();
    for n in 1..=2 {
        text.push_str(&format!(
            "[Event \"Ev & <{n}> \\\"q\\\"\"]\n[Site \"A_B $ % #\"]\n[Date \"2020.01.0{n}\"]\n\
             [Round \"1\"]\n[White \"W{n} & Co\"]\n[Black \"B{n} <b>\"]\n[Result \"1-0\"]\n\n\
             1. e4 $1 {{note <i> & more [%clk 0:01:00]}} (1. d4 d5) 1... e5 $201 2. Nf3 Nc6 1-0\n\n"
        ));
    }
    text
}

/// A game that starts from a position other than the standard one.
pub fn fen_game() -> String {
    "[Event \"Fen\"]\n[Site \"S\"]\n[Date \"2020.01.03\"]\n[Round \"1\"]\n[White \"Wf\"]\n\
     [Black \"Bf\"]\n[Result \"*\"]\n[SetUp \"1\"]\n\
     [FEN \"8/8/8/4k3/8/8/4P3/4K3 w - - 0 1\"]\n\n1. e4 *\n\n"
        .to_owned()
}

impl Rig {
    pub fn set_assets(&mut self) {
        self.state.set_export_assets(Some(assets()));
    }

    /// Chooses a format (the dialog must be open).
    pub fn format(&mut self, format: ExportFormat) {
        self.raw(ExportDocsMessage::SetFormat(format));
    }

    pub fn edit(&mut self, edit: ExportEdit) {
        self.raw(ExportDocsMessage::Edit(edit));
    }

    /// Opens the dialog, picks the format, runs the job to its end. Returns
    /// the answer of `Message::FilePicked` already performed.
    pub fn export_as(&mut self, format: ExportFormat, dest: &Path) {
        self.gx(GamesExportMessage::Open);
        self.format(format);
        let (effects, _) = self.gx_choose(dest);
        self.gx_run(effects);
    }
}

pub fn docs_notice(rig: &Rig) -> Option<GamesExportNotice> {
    notice(rig)
}

/// The job of an export among `effects`.
pub fn docs_control(effects: &[Effect]) -> mychess_app::OpenControl {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::ExportDocs { control, .. } => Some(control.clone()),
            _ => None,
        })
        .expect("an ExportDocs job")
}
