//! Shared helpers of the `m6_2c_*` contract tests (spec
//! `m6-2-taticas-e-padroes.md`, slice M6-2c).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty
)]

use std::path::Path;

pub use mychess_app::pattern_search::model::{Params, Who};
pub use mychess_app::{
    AppNotice, AppState, BasesMessage, Effect, Message, PatternSearchMessage as Pat,
    PatternSearchNotice, PatternTab, SlotId, perform, update,
};

/// Eight games written for the tests (the same positions as the fixture of
/// the database slice): 1 scholar's mate, 2 fool's mate, 3 knight fork
/// (from a FEN), 4 absolute pin, 5 Legal's mate, 6 quiet draw, 7 queen
/// promotion, 8 underpromotion.
pub const TACTICS_PGN: &str = "\
[Event \"1\"]\n[Site \"?\"]\n[Date \"2026.10.27\"]\n[Round \"1\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\n\
1. e4 e5 2. Bc4 Nc6 3. Qh5 Nf6 4. Qxf7# 1-0\n\n\
[Event \"2\"]\n[Site \"?\"]\n[Date \"2026.10.27\"]\n[Round \"2\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"0-1\"]\n\n\
1. f3 e5 2. g4 Qh4# 0-1\n\n\
[Event \"3\"]\n[Site \"?\"]\n[Date \"2026.10.27\"]\n[Round \"3\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\
[SetUp \"1\"]\n[FEN \"r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1\"]\n\n\
1. Nc7+ Kd7 2. Nxa8 1-0\n\n\
[Event \"4\"]\n[Site \"?\"]\n[Date \"2026.10.27\"]\n[Round \"4\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1/2-1/2\"]\n\n\
1. e4 e5 2. Nf3 Nc6 3. Bb5 d6 4. O-O Bd7 5. Re1 Nf6 1/2-1/2\n\n\
[Event \"5\"]\n[Site \"?\"]\n[Date \"2026.10.27\"]\n[Round \"5\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\n\
1. e4 e5 2. Nf3 d6 3. Bc4 Bg4 4. Nc3 g6 5. Nxe5 Bxd1 6. Bxf7+ Ke7 7. Nd5# 1-0\n\n\
[Event \"6\"]\n[Site \"?\"]\n[Date \"2026.10.27\"]\n[Round \"6\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1/2-1/2\"]\n\n\
1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 1/2-1/2\n\n\
[Event \"7\"]\n[Site \"?\"]\n[Date \"2026.10.27\"]\n[Round \"7\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\
[SetUp \"1\"]\n[FEN \"7k/P7/8/8/8/8/8/K7 w - - 0 1\"]\n\n\
1. a8=Q+ Kg7 1-0\n\n\
[Event \"8\"]\n[Site \"?\"]\n[Date \"2026.10.27\"]\n[Round \"8\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1/2-1/2\"]\n\
[SetUp \"1\"]\n[FEN \"7k/P7/8/8/8/8/8/K7 w - - 0 1\"]\n\n\
1. a8=N Kg7 1/2-1/2\n\n";

pub struct Rig {
    pub state: AppState,
    rt: tokio::runtime::Runtime,
}

impl Rig {
    /// A state whose patterns live in `dir/patterns` (never the real one).
    pub fn new(dir: &Path) -> Rig {
        let mut state = AppState::default();
        state.open_pgn_in_slots = true;
        state.set_pattern_dir(Some(dir.join("patterns")));
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

    pub fn pat(&mut self, message: Pat) -> Vec<Effect> {
        self.send(Message::PatternSearch(message))
    }

    pub fn open_text(&mut self, dir: &Path, name: &str, text: &str) -> SlotId {
        let path = dir.join(name);
        std::fs::write(&path, text).expect("write");
        self.send(BasesMessage::Open(path));
        self.state.bases.current()
    }

    /// The tactics sample open in the current slot, window open.
    pub fn tactics(dir: &Path) -> (Rig, SlotId) {
        let mut rig = Rig::new(dir);
        let slot = rig.open_text(dir, "t.pgn", TACTICS_PGN);
        assert_eq!(rig.state.bases.slot(slot).unwrap().total(), 8);
        rig.pat(Pat::Open);
        (rig, slot)
    }

    pub fn filter_values(&self, slot: SlotId) -> Vec<(u32, u8)> {
        let filter = self.state.bases.slot(slot).expect("slot").filter();
        filter.iter_games().map(|n| (n, filter.get(n))).collect()
    }

    pub fn cursor_ply(&self) -> u32 {
        self.state
            .session
            .game()
            .ply(self.state.session.cursor())
            .expect("ply")
    }

    pub fn view(&self) -> mychess_app::PatternSearchView {
        self.state.pattern_search_view().expect("window open")
    }

    /// The effect that `Start` plans (the search is not run).
    pub fn start_effect(&mut self) -> Effect {
        let mut effects = self.raw(Message::PatternSearch(Pat::Start));
        effects.retain(|e| matches!(e, Effect::PatternSearch { .. }));
        assert_eq!(effects.len(), 1, "one search effect: {effects:?}");
        effects.remove(0)
    }

    /// Picks the preset and sets the colour and N for a mate search.
    pub fn pick_mate(&mut self, by: Who, n: u8) {
        self.pat(Pat::SelectPreset("mate_in".into()));
        self.pat(Pat::SetParam(mychess_app::ParamValue::By(by)));
        self.pat(Pat::SetParam(mychess_app::ParamValue::N(n)));
    }
}
