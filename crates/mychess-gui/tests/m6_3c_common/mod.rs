//! Shared helpers of the `m6_3c_*` contract tests (spec
//! `docs/specs/m6-3-jogadores-torneios-spelling.md`, slice M6-3c), headless.
//! The base is a real `.si4` and the tasks the shell returns run to
//! completion, so the whole path is real.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_push_string
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
use iced_test::Simulator;
pub use mychess_app::LanguagePref;
use mychess_gui::windows::dialog_ui::ActiveDialog;

/// One game of a sample base.
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

pub fn g(
    (event, site, date): (&'static str, &'static str, &'static str),
    white: &'static str,
    black: &'static str,
    result: &'static str,
    (welo, belo): (u16, u16),
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

/// The six games of the spec (tables).
pub fn six() -> Vec<G> {
    let t1 = ("Aberto", "Rio BRA", "2020.01.10");
    let t2 = ("Aberto", "Rio BRA", "2020.01.11");
    let t3 = ("Aberto", "Rio BRA", "2020.01.12");
    let t4 = ("Aberto", "Rio BRA", "2020.05.20");
    let t5 = ("Solo", "Lima PER", "????.??.??");
    vec![
        g(t1, "Alfa, A", "Beta, B", "1-0", (2500, 2400)),
        g(t2, "Beta, B", "Gama, G", "1/2-1/2", (2400, 0)),
        g(t3, "Gama, G", "Alfa, A", "0-1", (0, 2510)),
        g(t4, "Alfa, A", "Beta, B", "1-0", (2520, 2410)),
        g(t5, "Alfa, A", "Delta, D", "1/2-1/2", (0, 0)),
        g(t5, "Delta, D", "Alfa, A", "*", (0, 0)),
    ]
}

/// Games with misspelled names (the alias `Alfa, Alf` of [`SSP`]) and no
/// Elo for Alfa.
pub fn misspelled() -> Vec<G> {
    let t = ("Aberto", "Rio BRA", "2013.03.01");
    vec![
        g(t, "Alfa, Alf", "Beta, B", "1-0", (0, 0)),
        g(t, "Alfa, Alf", "Gama, G", "0-1", (0, 0)),
        g(t, "Alfa, A", "Delta, D", "1/2-1/2", (0, 0)),
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

/// A `.si4` of `games` in `dir`.
pub fn make_si4(dir: &Path, name: &str, games: &[G]) -> PathBuf {
    base::make_si4(dir, name, &pgn(games))
}

/// The spelling file of the tests.
pub const SSP: &str = "# test file\n\
@PLAYER \"., -_*\"\n\
Alfa, A #GM BRA [2520] 1980.01.01\n\
  %Bio FIDEID 1234567\n\
  %Bio Champion\n\
  %Elo 2013:2300,2301,2302,2303,2304,2305,2306,2307,2308,2309,2310,2311\n\
  = Alfa, Alf\n\
Gama, G #- BRA [2050] 1970.??.??\n\
Delta, D #IM ARG [2100*] 1960.01.01\n\
  = Delt, D\n\
@EVENT \",. -_\"\n\
Aberto Brasileiro\n\
  = Aberto\n\
@SITE \"., -_()\"\n\
Rio BRA\n\
@ROUND \"\"\n\
1\n";

pub fn en() -> mychess_gui::i18n::I18n {
    mychess_gui::i18n::I18n::new(mychess_gui::i18n::Language::EnUs)
}

pub fn pt() -> mychess_gui::i18n::I18n {
    mychess_gui::i18n::I18n::new(mychess_gui::i18n::Language::PtBr)
}

/// The rig plus the helpers of these tests.
pub struct Tools {
    pub rig: TreeRig,
    pub dir: tempfile::TempDir,
}

impl Tools {
    pub fn new() -> Tools {
        Tools::with_language(LanguagePref::EnUs)
    }

    pub fn with_language(language: LanguagePref) -> Tools {
        let dir = tempfile::tempdir().expect("dir");
        let mut rig = TreeRig::with_language(language);
        rig.rig
            .shell
            .app
            .set_player_data_dir(Some(dir.path().join("data")));
        Tools { rig, dir }
    }

    pub fn app(&self) -> &mychess_app::AppState {
        &self.rig.rig.shell.app
    }

    pub fn shell(&self) -> &Shell {
        &self.rig.rig.shell
    }

    pub fn open_base(&mut self, games: &[G]) -> PathBuf {
        let path = make_si4(self.dir.path(), "base", games);
        self.rig.app_msg(BasesMessage::Open(path.clone()));
        path
    }

    pub fn load_ssp(&mut self) {
        let path = self.dir.path().join("spelling.ssp");
        std::fs::write(&path, SSP).expect("write");
        self.rig.app_msg(mychess_app::SpellingMessage::Load(path));
        assert!(self.app().spelling().is_loaded(), "the file loads");
    }

    pub fn action(&mut self, action: AppAction) {
        self.rig.ui(UiMessage::Action(action));
    }

    pub fn finder(&mut self, message: mychess_app::FinderMessage) {
        self.rig.app_msg(message);
    }

    pub fn named(&mut self, key: NamedKey) {
        self.rig.key(KeyInput {
            key: Key::Named(key),
            modifiers: Modifiers::NONE,
        });
    }

    pub fn top(&self) -> Option<ActiveDialog> {
        ActiveDialog::of(self.app(), &self.shell().ui.dialogs)
    }

    pub fn sim(&self, size: Size) -> Simulator<'_, Message> {
        let element = mychess_gui::windows::main_window::view(self.shell());
        Simulator::with_size(Settings::default(), size, element)
    }

    pub fn sim_default(&self) -> Simulator<'_, Message> {
        self.sim(Size::new(1300.0, 900.0))
    }
}

/// The messages a click produced, as `Debug` text.
pub fn clicked(tools: &Tools, id: &'static str) -> String {
    let mut sim = tools.sim_default();
    sim.click(iced::widget::Id::new(id))
        .unwrap_or_else(|e| panic!("{id}: {e:?}"));
    format!("{:?}", sim.into_messages().collect::<Vec<_>>())
}
