//! Shared helpers of the `m5_7b_*` contract tests (spec
//! `docs/specs/m5-7-eco-e-novidade.md`, M5-7b, D1700-D1725, app side).
//!
//! Only the public API of `mychess-app` is used. The jobs run for real, on
//! real (small) databases opened through `BasesMessage::Open`.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::cast_precision_loss,
    clippy::format_push_string
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub use mychess_app::{
    AppNotice, AppState, BrowserScope, ClassifyChoice, Command, DiscardChoice, EcoMessage,
    EcoNotice, EcoWindow, Effect, GameMessage, ListJobKind, ListMessage, Message, Nav,
    NoveltyOutcome, PlayOptions, SharedEco, SlotId, perform, update,
};
pub use mychess_app::{BasesMessage, CopyScope, PrefChange, PrefsMessage};
use mychess_core::{EcoBook, PartialDate};

/// The standard book of the tests.
pub const BOOK: &str = "A00a \"Start\" *\n\
    B20 \"Sicilian Defence\" 1.e4 c5 *\n\
    C20 \"King's Pawn Game\" 1.e4 e5 *\n\
    C40 \"King's Knight Opening\" 1.e4 e5 2.Nf3 *\n";

pub fn book(text: &str) -> SharedEco {
    SharedEco::new(EcoBook::parse(text).expect("book"))
}

pub fn sel(n: usize) -> SlotId {
    SlotId::from_number(n).expect("slot number 1..=9")
}

pub fn eco(message: EcoMessage) -> Message {
    Message::Eco(message)
}

/// One game of a synthetic PGN.
pub fn game_pgn(moves: &str, result: &str, date: &str, eco: Option<&str>) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "[Event \"T\"]\n[Site \"S\"]\n[Date \"{date}\"]\n[Round \"1\"]\n[White \"W\"]\n[Black \"B\"]\n[Result \"{result}\"]\n"
    );
    if let Some(eco) = eco {
        let _ = writeln!(out, "[ECO \"{eco}\"]");
    }
    let _ = write!(out, "\n{moves} {result}\n\n");
    out
}

/// Builds `<dir>/<name>.si4` from PGN text and returns the `.si4` path.
pub fn make_si4(dir: &Path, name: &str, pgn: &str) -> PathBuf {
    use mychess_db::si4::Si4Writer;
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

/// The ECO codes (as text, `""` for none) of a `.si4` file, read through an
/// independent `Si4Database::open`.
pub fn codes_on_disk(path: &Path) -> Vec<String> {
    use mychess_db::GameSource;
    let db = mychess_db::si4::Si4Database::open(path).expect("open");
    (1..=GameSource::len(&db))
        .map(|n| {
            db.entry(n)
                .expect("entry")
                .eco()
                .map(|e| e.to_string())
                .unwrap_or_default()
        })
        .collect()
}

/// Counts the effects of a kind.
pub fn count(effects: &[Effect], pick: impl Fn(&Effect) -> bool) -> usize {
    effects.iter().filter(|e| pick(e)).count()
}

pub fn is_stats(e: &Effect) -> bool {
    matches!(e, Effect::EcoStats { .. })
}

pub fn is_classify(e: &Effect) -> bool {
    matches!(e, Effect::EcoClassify { .. })
}

pub fn is_novelty(e: &Effect) -> bool {
    matches!(e, Effect::EcoNovelty { .. })
}

/// A state with the slot flow on and a runtime for `perform`.
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

    /// Feeds `effects` back (performing the non-host ones) and returns the
    /// host effects.
    pub fn settle(&mut self, effects: Vec<Effect>) -> Vec<Effect> {
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

    pub fn open_pgn(&mut self, dir: &Path, name: &str, text: &str) -> SlotId {
        let path = dir.join(name);
        std::fs::write(&path, text).expect("write");
        self.send(BasesMessage::Open(path));
        self.state.bases.current()
    }

    pub fn open_si4(&mut self, dir: &Path, name: &str, text: &str) -> (SlotId, PathBuf) {
        let path = make_si4(dir, name, text);
        self.send(BasesMessage::Open(path.clone()));
        (self.state.bases.current(), path)
    }

    pub fn set_book(&mut self, text: &str) {
        self.state.eco_book = Some(book(text));
    }

    /// A new empty game in the editor (opening a base loads its first game).
    pub fn fresh_game(&mut self) {
        self.send(Message::Command(Command::NewGame));
        assert_eq!(self.mainline_len(), 0);
    }

    pub fn notice(&self) -> Option<EcoNotice> {
        match &self.state.notice {
            Some(AppNotice::Eco(n)) => Some(n.clone()),
            _ => None,
        }
    }

    pub fn clear_notice(&mut self) {
        self.state.notice = None;
    }

    /// Plays `uci` at the cursor.
    pub fn play(&mut self, uci: &str) {
        let mv = self.state.session.position().parse_uci(uci).expect("uci");
        self.send(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        });
    }

    /// Plays `uci` as a variation of the move that follows the cursor.
    pub fn play_variation(&mut self, uci: &str) {
        let mv = self.state.session.position().parse_uci(uci).expect("uci");
        self.send(GameMessage::PlayMove {
            mv,
            options: PlayOptions {
                force_variation: true,
            },
        });
    }

    pub fn play_all(&mut self, ucis: &[&str]) {
        for uci in ucis {
            self.play(uci);
        }
    }

    pub fn ply(&self) -> u32 {
        let s = &self.state.session;
        s.game().ply(s.cursor()).expect("ply")
    }

    pub fn mainline_len(&self) -> usize {
        self.state.session.game().mainline().count()
    }

    /// Keeps only the games `numbers` in the filter of `slot`.
    pub fn keep(&mut self, slot: SlotId, numbers: &[u32]) {
        self.send(BasesMessage::List(ListMessage::SelectNumbers {
            slot,
            numbers: numbers.to_vec(),
        }));
        self.send(BasesMessage::List(ListMessage::KeepSelected { slot }));
    }

    pub fn browser(&self) -> mychess_app::EcoBrowserView<'_> {
        self.state.eco_browser_view().expect("browser open")
    }

    pub fn open_browser(&mut self) {
        self.send(eco(EcoMessage::OpenBrowser { glossary: vec![] }));
    }

    pub fn open_browser_with(&mut self, glossary: &[(&str, &str)]) {
        let glossary = glossary
            .iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
            .collect();
        self.send(eco(EcoMessage::OpenBrowser { glossary }));
    }

    /// Opens the classification dialog and answers the clock with `today`.
    pub fn open_classify(&mut self, today: Option<PartialDate>) {
        let effects = self.raw(eco(EcoMessage::OpenClassify));
        for effect in effects {
            if let Effect::QueryToday { request } = effect
                && let Some(date) = today
            {
                self.send(Message::Today(request, date));
            }
        }
    }
}

pub fn date(y: u16, m: u8, d: u8) -> PartialDate {
    PartialDate::new(Some(y), Some(m), Some(d)).expect("date")
}
