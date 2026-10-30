//! Shared helpers of the `m4_8_*` contract tests (spec
//! `m4-8-ameaca-shootout-automove.md`, app side): a ready UCI run, a scripted
//! engine fed with synthetic events, and accessors for the game.
//!
//! Only the public API of `mychess-app` is used.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::time::Duration;

use mychess_app::engines::{EngineDraft, EngineId, EngineMessage, RunId, StartMode};
use mychess_app::{
    AnalysisMessage, AppNotice, AppState, Effect, GameMessage, Message, ModeMessage, ModeNotice,
    Nav, PlayOptions, update,
};
use mychess_core::{Color, Game, Move, NodeId, Position};
use mychess_engine::{
    BestMove, Capabilities, EngineCommand, EngineEvent, EngineIdentity, EngineWarning, ExitInfo,
    ExitReason, SearchId, SearchRequest,
};

pub fn identity() -> EngineIdentity {
    EngineIdentity {
        name: "Fake".to_owned(),
        author: None,
        options: Vec::new(),
        capabilities: Capabilities {
            multipv: Some(1..=5),
            search_moves: true,
            ..Capabilities::default()
        },
    }
}

/// A UCI engine that finished its handshake, plus a game.
pub struct Rig {
    pub state: AppState,
    pub run: RunId,
    pub engine: EngineId,
    next_search: u64,
}

impl Rig {
    pub fn new(game: Game) -> Rig {
        let mut state = AppState::default();
        let engine = state
            .engines
            .add(EngineDraft::new("Fake", "/fake"))
            .expect("add engine");
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
                identity: Box::new(identity()),
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

    pub fn fen(fen: &str) -> Rig {
        Rig::new(Game::from_position(Position::from_fen(fen).expect("fen")))
    }

    pub fn send(&mut self, message: impl Into<Message>) -> Vec<Effect> {
        update(&mut self.state, message.into())
    }

    pub fn mode(&mut self, m: ModeMessage) -> Vec<Effect> {
        self.send(Message::Mode(m))
    }

    pub fn event(&mut self, event: EngineEvent) -> Vec<Effect> {
        let run = self.run;
        self.send(Message::Engine(EngineMessage::Event {
            run,
            event: Box::new(event),
        }))
    }

    pub fn game_msg(&mut self, m: GameMessage) -> Vec<Effect> {
        self.send(Message::Game(m))
    }

    pub fn play(&mut self, uci: &str) -> Vec<Effect> {
        let mv = self.state.session.position().parse_uci(uci).expect("uci");
        self.game_msg(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        })
    }

    pub fn position(&self) -> &Position {
        self.state.session.position()
    }

    pub fn game(&self) -> &Game {
        self.state.session.game()
    }

    pub fn notice(&self) -> Option<&AppNotice> {
        self.state.notice.as_ref()
    }

    pub fn mode_notice(&self) -> Option<ModeNotice> {
        match &self.state.notice {
            Some(AppNotice::Mode(n)) => Some(n.clone()),
            _ => None,
        }
    }

    /// The engine tells what it plays for the search of `effects`: label,
    /// start and best move. Returns the effects of the best move, or `None`
    /// when `effects` hold no search.
    pub fn answer(&mut self, effects: &[Effect], mv: Option<Move>) -> Option<Vec<Effect>> {
        self.answer_with(effects, mv, false)
    }

    pub fn answer_with(
        &mut self,
        effects: &[Effect],
        mv: Option<Move>,
        warn: bool,
    ) -> Option<Vec<Effect>> {
        let request = last_search(effects)?;
        self.next_search += 1;
        let search = SearchId(self.next_search);
        self.event(EngineEvent::SearchTagged {
            search,
            tag: request.tag,
        });
        self.event(EngineEvent::SearchStarted(search));
        if warn {
            self.event(EngineEvent::Warning(EngineWarning::IllegalBestMove(
                "bad".to_owned(),
            )));
        }
        Some(self.event(EngineEvent::BestMove(BestMove {
            search,
            mv,
            ponder: None,
        })))
    }

    /// Answers searches with `engine` until none is asked or `max` is
    /// reached. Returns every request seen.
    pub fn drive(
        &mut self,
        mut effects: Vec<Effect>,
        max: usize,
        engine: &mut dyn FnMut(&SearchRequest, &Position) -> Option<Move>,
    ) -> (Vec<SearchRequest>, Vec<Effect>) {
        let mut seen = Vec::new();
        for _ in 0..max {
            let Some(request) = last_search(&effects) else {
                break;
            };
            let mv = engine(&request, &position_of(&request));
            seen.push(request);
            effects = self.answer(&effects, mv).expect("search");
        }
        (seen, effects)
    }

    pub fn mainline(&self) -> Vec<String> {
        mainline_ucis(self.game())
    }
}

/// The first legal move: deterministic.
pub fn first_legal(_: &SearchRequest, pos: &Position) -> Option<Move> {
    pos.legal_moves().first().copied()
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

pub fn searches(effects: &[Effect]) -> Vec<SearchRequest> {
    commands(effects)
        .into_iter()
        .filter_map(|c| match c {
            EngineCommand::Search(r) => Some(r),
            _ => None,
        })
        .collect()
}

/// The searches of the modes (they always carry a `movetime`); the analysis
/// of the board is not one of them.
pub fn mode_searches(effects: &[Effect]) -> Vec<SearchRequest> {
    searches(effects)
        .into_iter()
        .filter(|s| s.limits.movetime.is_some())
        .collect()
}

pub fn last_search(effects: &[Effect]) -> Option<SearchRequest> {
    mode_searches(effects).pop()
}

pub fn count_stops(effects: &[Effect]) -> usize {
    commands(effects)
        .iter()
        .filter(|c| **c == EngineCommand::Stop)
        .count()
}

pub fn ticks(effects: &[Effect]) -> Vec<(RunId, u64, Duration)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::ScheduleModeTick {
                run,
                generation,
                delay,
            } => Some((*run, *generation, *delay)),
            _ => None,
        })
        .collect()
}

pub fn position_of(request: &SearchRequest) -> Position {
    let mut pos = request.position.start.clone();
    for m in &request.position.moves {
        pos.play(*m).expect("the run sends legal moves");
    }
    pos
}

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

pub fn mainline_ucis(game: &Game) -> Vec<String> {
    let mut pos = game.start_position().clone();
    game.mainline()
        .filter(|id| *id != game.root())
        .map(|id| {
            let mv = game.mv(id).expect("mv");
            let u = pos.uci(mv);
            pos.play(mv).expect("legal");
            u
        })
        .collect()
}

pub fn mv(pos: &Position, uci: &str) -> Move {
    pos.parse_uci(uci).expect("uci")
}

pub fn exited() -> EngineEvent {
    EngineEvent::Exited(ExitInfo {
        reason: ExitReason::Crashed,
        status: None,
        stderr_tail: Vec::new(),
    })
}
