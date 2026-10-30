//! Shared helpers of the `m7_1c_*` tests (spec `docs/specs/m7-1-manutencao.md`,
//! slice M7-1c): a shell on a real `.si4` whose tasks run to completion, the
//! maintenance window drawn headless.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

#[path = "../m5_2_gui_common/mod.rs"]
pub mod base;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub use base::{
    AppAction, AppMessage, BasesMessage, Key, KeyInput, Message, Modifiers, NamedKey, SIZE, Shell,
    TreeRig, UiMessage,
};
use iced::{Settings, Size};
pub use iced_test::Simulator;
pub use mychess_app::maintenance::{
    FlagAction, FlagScope, MaintFlag, MaintNotice, MaintReport, MaintRequest, MaintenanceMessage,
    ScopeChoice, SortCriterion,
};
pub use mychess_gui::windows::maintenance::{
    MaintEdit, MaintForm, MaintUiMessage, NameUiKind, common_sort,
};

/// Six games with different years (so a sort has something to do).
pub fn six_pgn() -> String {
    let mut out = String::new();
    for (n, (year, white)) in [
        (2022, "Alfa, A"),
        (2020, "Beta, B"),
        (2021, "Gama, G"),
        (2019, "Alfa, A"),
        (2023, "Delta, D"),
        (2018, "Beta, B"),
    ]
    .into_iter()
    .enumerate()
    {
        let _ = write!(
            out,
            "[Event \"Aberto\"]\n[Site \"Rio\"]\n[Date \"{year}.01.10\"]\n[Round \"{}\"]\n\
             [White \"{white}\"]\n[Black \"Zeta, Z\"]\n[Result \"1-0\"]\n\n\
             1. e4 e5 2. Nf3 Nc6 1-0\n\n",
            n + 1
        );
    }
    out
}

/// The duplicates of the spec: games 1 and 2 (the second continues the first)
/// and 3 (other moves), same players, event, site, round and date.
pub fn twins_pgn() -> String {
    let mut out = String::new();
    for moves in [
        "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6",
        "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6",
        "1. d4 d5 2. c4 e6",
    ] {
        let _ = write!(
            out,
            "[Event \"Aberto\"]\n[Site \"Rio\"]\n[Date \"2020.01.10\"]\n[Round \"1\"]\n\
             [White \"Alfa, A\"]\n[Black \"Beta, B\"]\n[Result \"*\"]\n\n{moves} *\n\n"
        );
    }
    out
}

pub struct Maint {
    pub rig: TreeRig,
    pub dir: tempfile::TempDir,
    pub path: PathBuf,
}

impl Maint {
    /// A shell with the `.si4` of `pgn` open and the window closed.
    pub fn with(pgn: &str) -> Maint {
        let dir = tempfile::tempdir().expect("dir");
        let mut rig = TreeRig::new();
        let path = base::make_si4(dir.path(), "base", pgn);
        rig.app_msg(BasesMessage::Open(path.clone()));
        Maint { rig, dir, path }
    }

    pub fn six() -> Maint {
        Maint::with(&six_pgn())
    }

    pub fn shell(&self) -> &Shell {
        &self.rig.rig.shell
    }

    pub fn app(&self) -> &mychess_app::AppState {
        &self.rig.rig.shell.app
    }

    pub fn ui(&mut self, message: MaintUiMessage) {
        self.rig.msg(Message::Maint(message));
    }

    pub fn open(&mut self) {
        self.ui(MaintUiMessage::Open);
    }

    pub fn key(&mut self, key: Key) {
        self.rig.key(KeyInput {
            key,
            modifiers: Modifiers::NONE,
        });
    }

    pub fn form(&self) -> Option<MaintForm> {
        self.shell().ui.dialogs.maint.form
    }

    pub fn is_open(&self) -> bool {
        self.app().maintenance().is_open()
    }

    pub fn sim(&self) -> Simulator<'_, Message> {
        let element = mychess_gui::windows::main_window::view(self.shell());
        Simulator::with_size(Settings::default(), Size::new(1400.0, 1000.0), element)
    }

    pub fn deleted(&self) -> u32 {
        let stats = self.app().maintenance_view().stats.expect("stats");
        MaintFlag::Delete
            .counter()
            .and_then(|i| stats.flags.get(i).copied())
            .unwrap_or(0)
    }
}
