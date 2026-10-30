//! Shared helpers of the `m4_6_*` contract tests (spec
//! `m4-6-anotacao-automatica.md`, phase A, app side): a ready UCI engine run,
//! synthetic engine answers fed to `update`, and accessors for the game.
//!
//! Only the public API of `mychess-app` is used.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

use mychess_app::engines::{EngineDraft, EngineId, EngineMessage, RunId, StartMode};
use mychess_app::{
    AnnotateMessage, AnnotateNotice, AnnotateOptions, AppNotice, AppState, Effect, GameMessage,
    Message, Nav, update,
};
use mychess_core::{Color, Game, Move, Nag, NodeId, Position};
use mychess_engine::{
    BestMove, Bound, Capabilities, EngineCommand, EngineEvent, EngineIdentity, EngineOption,
    OptionKind, Protocol, Score, SearchId, SearchInfo, SearchRequest,
};

/// The high bits of the tags of the searches of the annotation (D880).
pub const ANNOTATE_TAG: u64 = 0x4E4E_0000_0000_0000;

/// What the fake engine says about a position (White's point of view).
#[derive(Clone)]
pub struct Reply {
    pub score: Score,
    pub pv: Vec<Move>,
    pub second: Option<Score>,
    pub depth: u32,
    pub bound: Bound,
    /// Report nothing at all before the best move.
    pub silent: bool,
}

impl Reply {
    pub fn cp(cp: i32, pv: Vec<Move>) -> Reply {
        Reply {
            score: Score::Cp(cp),
            pv,
            second: None,
            depth: 13,
            bound: Bound::Exact,
            silent: false,
        }
    }

    pub fn mate(n: i32, pv: Vec<Move>) -> Reply {
        Reply {
            score: Score::Mate(n),
            ..Reply::cp(0, pv)
        }
    }
}

pub fn identity(options: Vec<EngineOption>, multipv_max: Option<u8>) -> EngineIdentity {
    EngineIdentity {
        name: "Fake 1".to_owned(),
        author: None,
        options,
        capabilities: Capabilities {
            multipv: multipv_max.map(|m| 1..=m),
            search_moves: true,
            ..Capabilities::default()
        },
    }
}

pub fn own_book_option() -> EngineOption {
    EngineOption {
        name: "OwnBook".to_owned(),
        kind: OptionKind::Check { default: true },
    }
}

/// A state with a UCI engine that finished its handshake, plus a game.
pub struct Rig {
    pub state: AppState,
    pub run: RunId,
    pub engine: EngineId,
    next_search: u64,
}

impl Rig {
    pub fn new(game: Game) -> Rig {
        Rig::with(
            game,
            identity(vec![own_book_option()], Some(5)),
            EngineDraft::new("Fake", "/fake"),
        )
    }

    pub fn with(game: Game, identity: EngineIdentity, draft: EngineDraft) -> Rig {
        let mut state = AppState::default();
        let engine = state.engines.add(draft).expect("add engine");
        let fx = update(
            &mut state,
            Message::Engine(EngineMessage::Start(engine, StartMode::Analyze)),
        );
        let run = fx
            .iter()
            .find_map(|e| match e {
                Effect::StartEngine { run, .. } => Some(*run),
                _ => None,
            })
            .expect("StartEngine");
        update(
            &mut state,
            Message::Engine(EngineMessage::Started {
                run,
                identity: Box::new(identity),
                at: 1_700_000_000,
            }),
        );
        state.session.replace_game(game);
        Rig {
            state,
            run,
            engine,
            next_search: 100,
        }
    }

    pub fn send(&mut self, message: impl Into<Message>) -> Vec<Effect> {
        update(&mut self.state, message.into())
    }

    pub fn event(&mut self, event: EngineEvent) -> Vec<Effect> {
        let run = self.run;
        self.send(Message::Engine(EngineMessage::Event {
            run,
            event: Box::new(event),
        }))
    }

    pub fn start(&mut self) -> Vec<Effect> {
        self.send(AnnotateMessage::Start)
    }

    pub fn running(&self) -> bool {
        self.state.annotation_running()
    }

    pub fn game(&self) -> &Game {
        self.state.session.game()
    }

    pub fn notice(&self) -> Option<AnnotateNotice> {
        match &self.state.notice {
            Some(AppNotice::Annotate(n)) => Some(*n),
            _ => None,
        }
    }

    pub fn options(&mut self, f: impl FnOnce(&mut AnnotateOptions)) {
        f(&mut self.state.annotate_options);
    }

    /// Answers the last search of `effects`: label, start, reports and best
    /// move. Returns the effects of the best move and the request.
    pub fn answer(
        &mut self,
        effects: &[Effect],
        reply: &Reply,
    ) -> Option<(Vec<Effect>, SearchRequest)> {
        let request = last_search(effects)?;
        self.next_search += 1;
        let search = SearchId(self.next_search);
        self.event(EngineEvent::SearchTagged {
            search,
            tag: request.tag,
        });
        self.event(EngineEvent::SearchStarted(search));
        if !reply.silent {
            self.event(EngineEvent::Info(SearchInfo {
                search,
                multipv: 1,
                depth: Some(reply.depth),
                score: Some((reply.score, reply.bound)),
                pv: reply.pv.clone(),
                ..SearchInfo::default()
            }));
            if let Some(second) = reply.second {
                self.event(EngineEvent::Info(SearchInfo {
                    search,
                    multipv: 2,
                    depth: Some(reply.depth),
                    score: Some((second, Bound::Exact)),
                    pv: Vec::new(),
                    ..SearchInfo::default()
                }));
            }
        }
        let after = self.event(EngineEvent::BestMove(BestMove {
            search,
            mv: reply.pv.first().copied(),
            ponder: None,
        }));
        Some((after, request))
    }

    /// Answers up to `max` searches with `engine`, starting from `effects`.
    pub fn drive(
        &mut self,
        mut effects: Vec<Effect>,
        max: usize,
        engine: &mut dyn FnMut(&SearchRequest, &Position) -> Reply,
    ) -> Log {
        let mut log = Log::default();
        log.commands.extend(commands(&effects));
        for _ in 0..max {
            if !self.running() {
                break;
            }
            let Some(request) = last_search(&effects) else {
                break;
            };
            let reply = engine(&request, &position_of(&request));
            let Some((after, request)) = self.answer(&effects, &reply) else {
                break;
            };
            log.requests.push(request);
            log.commands.extend(commands(&after));
            effects = after;
        }
        log.last = effects;
        log
    }

    /// Starts and drives to the end (at most 5000 searches).
    pub fn run_all(&mut self, engine: &mut dyn FnMut(&SearchRequest, &Position) -> Reply) -> Log {
        let fx = self.start();
        self.drive(fx, 5000, engine)
    }
}

/// Everything a drive saw.
#[derive(Default)]
pub struct Log {
    pub requests: Vec<SearchRequest>,
    pub commands: Vec<EngineCommand>,
    /// The effects after the last answer.
    pub last: Vec<Effect>,
}

pub fn commands(effects: &[Effect]) -> Vec<EngineCommand> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::EngineCommand { command, .. } => Some((**command).clone()),
            _ => None,
        })
        .collect()
}

pub fn last_search(effects: &[Effect]) -> Option<SearchRequest> {
    commands(effects).into_iter().rev().find_map(|c| match c {
        EngineCommand::Search(request) => Some(request),
        _ => None,
    })
}

pub fn position_of(request: &SearchRequest) -> Position {
    let mut pos = request.position.start.clone();
    for m in &request.position.moves {
        pos.play(*m).expect("the run sends legal moves");
    }
    pos
}

/// Number of moves played from the start to reach the searched position.
pub fn ply_of(request: &SearchRequest) -> usize {
    request.position.moves.len()
}

/// A game from the standard position with the moves `ucis`.
pub fn game_of(ucis: &[&str]) -> Game {
    game_from(Game::new(), ucis)
}

/// `game` extended along its main line... from its root, with `ucis`.
pub fn game_from(mut game: Game, ucis: &[&str]) -> Game {
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

/// 20 plies; 3.Qxe5 (ply 5) hangs the queen.
pub const HANGING_QUEEN: [&str; 20] = [
    "e2e4", "e7e5", "d1h5", "b8c6", "h5e5", "c6e5", "g1f3", "e5f3", "g2f3", "d7d6", "f1c4", "g8f6",
    "d2d3", "f8e7", "c1e3", "e8g8", "b1c3", "c7c6", "e1c1", "b7b5",
];

/// The node of the move at `ply` (1-based) of the main line.
pub fn nth_main(game: &Game, ply: usize) -> NodeId {
    game.mainline().nth(ply - 1).expect("ply")
}

/// Adds `uci` as a child of `parent` and returns the node.
pub fn add(game: &mut Game, parent: NodeId, uci: &str) -> NodeId {
    let mv = game
        .position_at(parent)
        .expect("position")
        .parse_uci(uci)
        .expect("uci");
    game.add_move(parent, mv).expect("legal")
}

pub fn nags_of(game: &Game, node: NodeId) -> Vec<u16> {
    game.nags(node)
        .expect("node")
        .iter()
        .map(|n| u16::from(n.value()))
        .collect()
}

pub fn comment_of(game: &Game, node: NodeId) -> Option<String> {
    game.comment(node).expect("node").map(str::to_owned)
}

/// UCI moves played from `position`.
pub fn pv_of(position: &Position, ucis: &[&str]) -> Vec<Move> {
    let mut pos = position.clone();
    ucis.iter()
        .map(|u| {
            let m = pos.parse_uci(u).expect("uci");
            pos.play(m).expect("play");
            m
        })
        .collect()
}

fn material(pos: &Position) -> i32 {
    let fen = pos.fen();
    let board = fen.split(' ').next().unwrap_or_default();
    board
        .chars()
        .map(|c| {
            let v = match c.to_ascii_lowercase() {
                'p' => 1,
                'n' | 'b' => 3,
                'r' => 5,
                'q' => 9,
                _ => 0,
            };
            if c.is_ascii_uppercase() { v } else { -v }
        })
        .sum::<i32>()
        * 100
}

/// The best move for the side to move and the best reply, by material at
/// two plies; the score is from White's point of view.
pub fn material_reply(pos: &Position) -> Reply {
    let white = pos.turn() == Color::White;
    let better = |a: i32, b: i32| if white { a > b } else { a < b };
    let mut best: Option<(i32, Vec<Move>)> = None;
    for m in pos.legal_moves() {
        let mut after = pos.clone();
        after.play(m).expect("legal");
        let mut value = material(&after);
        let mut line = vec![m];
        let mut reply_best: Option<i32> = None;
        for r in after.legal_moves() {
            let mut then = after.clone();
            then.play(r).expect("legal");
            let v = material(&then);
            let reply_better = if white {
                v < reply_best.unwrap_or(i32::MAX)
            } else {
                v > reply_best.unwrap_or(i32::MIN)
            };
            if reply_best.is_none() || reply_better {
                reply_best = Some(v);
                line.truncate(1);
                line.push(r);
            }
        }
        if let Some(v) = reply_best {
            value = v;
        }
        if best.as_ref().is_none_or(|(b, _)| better(value, *b)) {
            best = Some((value, line));
        }
    }
    let (cp, pv) = best.unwrap_or((0, Vec::new()));
    Reply::cp(cp, pv)
}

pub fn material_engine(_: &SearchRequest, pos: &Position) -> Reply {
    material_reply(pos)
}
