//! Shared helpers of the `m5_2_tree_*` contract tests (spec
//! `m5-2-janela-da-arvore.md`, sub-slice M5-2a, app side).
//!
//! Only the public API of `mychess-app` is used. Searches run for real, on
//! real (small) databases opened through `BasesMessage::Open`.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::vec_box,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub use mychess_app::{
    AppNotice, AppState, BaseError, BasesMessage, Effect, GameMessage, ListMessage, Message, Nav,
    PlayOptions, SlotId, TreeAnswer, TreeBody, TreeFastMode, TreeIssue, TreeJob, TreeMessage,
    TreePrefs, TreeSort, TreeView, bar_segments, perform, update,
};
use mychess_core::{Game, Position};

/// The standard five-game base: White to move at the start position sees
/// `e4` (3 games: 1-0, 1/2, no result), `d4` (0-1) and `Nf3` (1-0).
pub const STD: &str = "[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 1-0\n\n\
    [Result \"0-1\"]\n\n1. d4 d5 0-1\n\n\
    [Result \"1/2-1/2\"]\n\n1. e4 c5 1/2-1/2\n\n\
    [Result \"1-0\"]\n\n1. Nf3 d5 1-0\n\n\
    [Result \"*\"]\n\n1. e4 *\n\n";

/// One game of a synthetic PGN.
pub fn game_pgn(
    moves: &str,
    result: &str,
    welo: u16,
    belo: u16,
    date: &str,
    fen: Option<&str>,
) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "[Event \"T\"]\n[Site \"S\"]\n[Date \"{date}\"]\n[Round \"1\"]\n[White \"W\"]\n[Black \"B\"]\n[Result \"{result}\"]\n"
    );
    if welo > 0 {
        let _ = writeln!(out, "[WhiteElo \"{welo}\"]");
    }
    if belo > 0 {
        let _ = writeln!(out, "[BlackElo \"{belo}\"]");
    }
    if let Some(fen) = fen {
        let _ = writeln!(out, "[SetUp \"1\"]\n[FEN \"{fen}\"]");
    }
    let _ = write!(out, "\n{moves} {result}\n\n");
    out
}

/// A game from the standard position with the moves `ucis`.
pub fn game_of(ucis: &[&str]) -> Game {
    let mut game = Game::new();
    let mut node = game.root();
    for uci in ucis {
        let mv = game
            .position_at(node)
            .expect("position")
            .parse_uci(uci)
            .expect("uci");
        node = game.add_move(node, mv).expect("legal");
    }
    game
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

/// The tree searches among `effects`.
pub fn jobs(effects: Vec<Effect>) -> Vec<Box<TreeJob>> {
    effects
        .into_iter()
        .filter_map(|e| match e {
            Effect::TreeSearch { job } => Some(job),
            _ => None,
        })
        .collect()
}

pub fn tree_searches(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::TreeSearch { .. }))
        .count()
}

pub fn sel(n: usize) -> SlotId {
    SlotId::from_number(n).expect("slot number 1..=9")
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

    /// Runs one tree job and returns the message that carries its answer.
    pub fn run_job(&self, job: Box<TreeJob>) -> Message {
        self.perform(Effect::TreeSearch { job })
    }

    /// `update` plus every non-host effect, answers fed back (tree searches
    /// included). Returns the host effects.
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

    /// Opens a PGN text as a base (written to `dir`) and returns its slot.
    pub fn open_pgn(&mut self, dir: &Path, name: &str, text: &str) -> SlotId {
        let path = dir.join(name);
        std::fs::write(&path, text).expect("write");
        self.send(BasesMessage::Open(path));
        self.state.bases.current()
    }

    /// Opens an SI4 base built from PGN text and returns its slot.
    pub fn open_si4(&mut self, dir: &Path, name: &str, text: &str) -> SlotId {
        let path = make_si4(dir, name, text);
        self.send(BasesMessage::Open(path));
        self.state.bases.current()
    }

    /// Opens the std base and the tree; both searches done.
    pub fn std_with_tree(dir: &Path) -> Rig {
        let mut rig = Rig::new();
        rig.open_pgn(dir, "std.pgn", STD);
        rig.send(TreeMessage::Open);
        rig
    }

    pub fn open_tree(&mut self) {
        self.send(TreeMessage::Open);
    }

    /// Plays `uci` at the cursor (the normal move flow).
    pub fn play(&mut self, uci: &str) {
        let mv = self.state.session.position().parse_uci(uci).expect("uci");
        self.send(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        });
    }

    pub fn nav(&mut self, nav: Nav) {
        self.send(GameMessage::Navigate(nav));
    }

    pub fn view(&self) -> TreeView {
        self.state.tree_view()
    }

    pub fn sans(&self) -> Vec<String> {
        self.view().rows.iter().map(|r| r.san.clone()).collect()
    }

    pub fn sorted_sans(&self) -> Vec<String> {
        let mut sans = self.sans();
        sans.sort();
        sans
    }

    pub fn row(&self, san: &str) -> mychess_app::TreeRowView {
        self.view()
            .rows
            .into_iter()
            .find(|r| r.san == san)
            .unwrap_or_else(|| panic!("no row {san}"))
    }

    pub fn started(&self) -> u64 {
        self.state.tree().searches_started()
    }

    pub fn ply(&self) -> u32 {
        self.state
            .session
            .game()
            .ply(self.state.session.cursor())
            .expect("ply")
    }

    pub fn nodes(&self) -> usize {
        self.state.session.game().node_count()
    }

    pub fn filtered(&self, slot: SlotId) -> u32 {
        self.state.bases.slot(slot).expect("slot").filtered()
    }

    /// The numbers of the games in the main filter of `slot`.
    pub fn filter_numbers(&self, slot: SlotId) -> Vec<u32> {
        self.state
            .bases
            .slot(slot)
            .expect("slot")
            .filter()
            .iter_games()
            .collect()
    }

    /// Keeps only the games `numbers` in the filter of `slot` (user action).
    pub fn keep(&mut self, slot: SlotId, numbers: &[u32]) {
        self.send(BasesMessage::List(ListMessage::SelectNumbers {
            slot,
            numbers: numbers.to_vec(),
        }));
        self.send(BasesMessage::List(ListMessage::KeepSelected { slot }));
    }
}

/// The answer of a job carries its result; `Debug` shows `Cancelled`.
pub fn was_cancelled(message: &Message) -> bool {
    format!("{message:?}").contains("Cancelled")
}
