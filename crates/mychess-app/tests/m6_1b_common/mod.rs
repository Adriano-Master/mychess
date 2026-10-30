//! Shared helpers of the `m6_1b_*` contract tests (spec
//! `m6-1-buscas-avancadas.md`, slice M6-1b).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub use mychess_app::{
    AdvancedKind, AdvancedSearchMessage, AdvancedSearchNotice, AppNotice, AppState, BasesMessage,
    BasesNotice, Effect, Message, SearchOp, SlotId, perform, update,
};

/// The sample games, in database order.
pub const GAMES: [&str; 6] = [
    "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 *",
    "1. e4 e5 2. Nc3 Nc6 *",
    "1. d4 d5 2. c4 e6 *",
    "1. e4 c5 2. Nf3 d6 3. d4 cxd4 4. Nxd4 Nf6 *",
    "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 *",
    "1. Nf3 d5 *",
];

/// FEN after 1.e4 e5.
pub const AFTER_E4_E5: &str = "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2";

/// PGN text with one game per movetext.
pub fn pgn(games: &[&str]) -> String {
    let mut out = String::new();
    for (i, moves) in games.iter().enumerate() {
        let _ = write!(
            out,
            "[Event \"E{i}\"]\n[Site \"?\"]\n[Date \"2020.01.01\"]\n[Round \"1\"]\n\
             [White \"W{i}\"]\n[Black \"B{i}\"]\n[Result \"*\"]\n\n{moves}\n\n"
        );
    }
    out
}

pub struct Rig {
    pub state: AppState,
    rt: tokio::runtime::Runtime,
}

impl Rig {
    pub fn new() -> Rig {
        let mut state = AppState::default();
        state.open_pgn_in_slots = true;
        Rig::from(state)
    }

    /// A state that keeps its preferences (and so its saved searches) in
    /// `dir`.
    pub fn in_dir(dir: &Path) -> Rig {
        let mut state = AppState::with_preferences_path(Some(dir.join("preferences.toml")));
        state.open_pgn_in_slots = true;
        Rig::from(state)
    }

    fn from(state: AppState) -> Rig {
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

    /// `update` plus every non-host effect, answers fed back.
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

    pub fn adv(&mut self, message: AdvancedSearchMessage) -> Vec<Effect> {
        self.send(Message::AdvancedSearch(message))
    }

    /// Opens a PGN with these movetexts in a file of `dir`.
    pub fn open_games(&mut self, dir: &Path, name: &str, games: &[&str]) -> SlotId {
        let path = dir.join(name);
        std::fs::write(&path, pgn(games)).expect("write");
        self.send(BasesMessage::Open(path));
        self.state.bases.current()
    }

    /// The numbers in the filter of a slot, in database order.
    pub fn filter_numbers(&self, slot: SlotId) -> Vec<u32> {
        self.state
            .bases
            .slot(slot)
            .expect("slot")
            .filter()
            .iter_games()
            .collect()
    }

    /// The value (ply + 1) the filter holds for a game.
    pub fn filter_value(&self, slot: SlotId, game: u32) -> u8 {
        self.state
            .bases
            .slot(slot)
            .expect("slot")
            .filter()
            .get(game)
    }

    /// The ply of the cursor in the main line of the game in the editor.
    pub fn cursor_ply(&self) -> usize {
        let ply = self
            .state
            .session
            .game()
            .ply(self.state.session.cursor())
            .expect("ply");
        usize::try_from(ply).expect("small")
    }

    /// The effect of `Start` for the tab on show (the search is not run).
    pub fn start_effect(&mut self, kind: AdvancedKind) -> Effect {
        self.raw(Message::AdvancedSearch(AdvancedSearchMessage::SelectTab(
            kind,
        )));
        let mut effects = self.raw(Message::AdvancedSearch(AdvancedSearchMessage::Start));
        effects.retain(|e| matches!(e, Effect::AdvancedSearch { .. }));
        assert_eq!(effects.len(), 1, "one search effect: {effects:?}");
        effects.remove(0)
    }

    pub fn issue(&self) -> Option<AdvancedSearchNotice> {
        self.state
            .advanced_search_view()
            .and_then(|view| view.issue)
    }

    pub fn open_window(&mut self, kind: AdvancedKind) {
        self.send(Message::AdvancedSearch(AdvancedSearchMessage::Open(kind)));
    }
}

/// A state with the sample games open in one slot.
pub fn sample() -> (Rig, SlotId, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let slot = rig.open_games(dir.path(), "s.pgn", &GAMES);
    assert_eq!(rig.state.bases.slot(slot).expect("slot").total(), 6);
    rig.open_window(AdvancedKind::Position);
    (rig, slot, dir)
}
