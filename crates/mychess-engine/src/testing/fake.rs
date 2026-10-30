//! The logic of the fake engine: a deterministic, material-greedy "engine"
//! that speaks UCI over any async reader and writer.
//!
//! Evaluation of a move (spec m4-1, "Engine falsa"): the material balance
//! (P=100, N=B=300, R=500, Q=900) *of the side that makes the move* after the
//! move, with checkmate worth a million. Moves are ranked by evaluation,
//! highest first, ties broken by the UCI text, ascending. The PV of depth `d`
//! has `min(d, 3)` moves: the move, then greedy replies by each side.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mychess_core::{Color, Move, Position, Role};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::time::Instant;

const MAX_DEPTH: u32 = 99;
const TICK: Duration = Duration::from_millis(5);

/// How the fake engine behaves; the defaults are a well-behaved engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeBehavior {
    /// `id name`.
    pub name: String,
    /// Never answers `uci`.
    pub silent: bool,
    /// Waits this long before `uciok`.
    pub uciok_delay: Duration,
    /// Ignores `stop` (but still answers `isready`).
    pub hang_on_stop: bool,
    /// On `stop` ends the search without printing `bestmove`.
    pub no_bestmove_after_stop: bool,
    /// Ignores `quit` and the closing of its input.
    pub ignore_quit: bool,
    /// Dies at the first `go`.
    pub crash_after_go: bool,
    /// Mixes junk into its output.
    pub garbage: bool,
    /// Announces the `MultiPV` option.
    pub multipv: bool,
    /// Announces the `UCI_Chess960` option.
    pub chess960: bool,
    /// Ends lines with `\r\n`.
    pub crlf: bool,
    /// Which options of strength the engine declares (M8-7).
    pub strength: FakeStrength,
    /// Exit code of the process (binary only).
    pub exit_code: i32,
    /// The protocol spoken (`--protocol`).
    pub protocol: FakeProtocol,
    /// The XBoard dialect (used when `protocol` is [`FakeProtocol::XBoard`]).
    pub xboard: XBoardFake,
}

/// The options of strength a fake UCI engine declares (spec M8-7, D2452).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FakeStrength {
    /// `UCI_LimitStrength`, `UCI_Elo` (1320-3190) and `Skill Level` (0-20),
    /// like Stockfish.
    #[default]
    Full,
    /// Only `Skill Level` (`--no-limit-strength`): no `UCI_LimitStrength` and
    /// no `UCI_Elo`.
    SkillOnly,
    /// None of the three (`--no-strength`).
    Nothing,
}

/// The protocol a fake engine speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FakeProtocol {
    /// UCI.
    #[default]
    Uci,
    /// XBoard / WinBoard (CECP).
    XBoard,
}

/// How the XBoard fake engine behaves (spec m4-2, "Engine falsa").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XBoardFake {
    /// Does not know `protover` (answers an error): no `feature` lines.
    pub legacy: bool,
    /// The `key=value` pairs of the `feature` line (after `myname`).
    pub features: String,
    /// A start-up line printed at `xboard` (for the quirk table).
    pub banner: Option<String>,
    /// Reports the time in whole seconds instead of centiseconds.
    pub seconds: bool,
    /// Reports scores from White's point of view.
    pub white_pov: bool,
    /// Writes the variation in SAN with move numbers (else coordinates).
    pub san_pv: bool,
    /// Never answers `ping`.
    pub no_pong: bool,
    /// Sends `done=0`, waits this long and then sends `done=1`.
    pub slow_feature: Duration,
    /// Answers `go` with `resign` instead of a move (spec M8-2c, D2526).
    pub resign: bool,
}

impl Default for XBoardFake {
    fn default() -> Self {
        XBoardFake {
            legacy: false,
            features: "setboard=1 analyze=1 ping=1 done=1".to_owned(),
            banner: None,
            seconds: false,
            white_pov: false,
            san_pv: true,
            no_pong: false,
            slow_feature: Duration::ZERO,
            resign: false,
        }
    }
}

impl Default for FakeBehavior {
    fn default() -> Self {
        FakeBehavior {
            name: "FakeFish".to_owned(),
            silent: false,
            uciok_delay: Duration::ZERO,
            hang_on_stop: false,
            no_bestmove_after_stop: false,
            ignore_quit: false,
            crash_after_go: false,
            garbage: false,
            multipv: true,
            chess960: true,
            crlf: false,
            strength: FakeStrength::Full,
            exit_code: 0,
            protocol: FakeProtocol::Uci,
            xboard: XBoardFake::default(),
        }
    }
}

impl FakeBehavior {
    /// Reads the command line flags of the fake-engine binary. Returns the
    /// behavior and whether `--stderr-flood` was given.
    pub fn from_args<I: IntoIterator<Item = String>>(
        args: I,
    ) -> Result<(FakeBehavior, bool), String> {
        let mut b = FakeBehavior::default();
        let mut flood = false;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let mut value = |what: &str| args.next().ok_or_else(|| format!("{what} needs a value"));
            match arg.as_str() {
                "--name" => b.name = value("--name")?,
                "--silent" => b.silent = true,
                "--uciok-delay-ms" => {
                    let ms = value("--uciok-delay-ms")?
                        .parse::<u64>()
                        .map_err(|e| e.to_string())?;
                    b.uciok_delay = Duration::from_millis(ms);
                }
                "--hang-on-stop" => b.hang_on_stop = true,
                "--no-bestmove-after-stop" => b.no_bestmove_after_stop = true,
                "--ignore-quit" => b.ignore_quit = true,
                "--crash-after-go" => b.crash_after_go = true,
                "--garbage" => b.garbage = true,
                "--no-multipv" => b.multipv = false,
                "--chess960" => b.chess960 = true,
                "--no-chess960" => b.chess960 = false,
                "--crlf" => b.crlf = true,
                "--no-limit-strength" => b.strength = FakeStrength::SkillOnly,
                "--no-strength" => b.strength = FakeStrength::Nothing,
                "--stderr-flood" => flood = true,
                "--protocol" => {
                    b.protocol = match value("--protocol")?.as_str() {
                        "uci" => FakeProtocol::Uci,
                        "xboard" => FakeProtocol::XBoard,
                        other => return Err(format!("unknown protocol `{other}`")),
                    };
                }
                "--legacy" => b.xboard.legacy = true,
                "--features" => b.xboard.features = value("--features")?,
                "--banner" => b.xboard.banner = Some(value("--banner")?),
                "--seconds" => b.xboard.seconds = true,
                "--white-pov" => b.xboard.white_pov = true,
                "--san-pv" => b.xboard.san_pv = true,
                "--coord-pv" => b.xboard.san_pv = false,
                "--no-pong" => b.xboard.no_pong = true,
                "--slow-feature" => {
                    let ms = value("--slow-feature")?
                        .parse::<u64>()
                        .map_err(|e| e.to_string())?;
                    b.xboard.slow_feature = Duration::from_millis(ms);
                }
                "--exit-code" => {
                    b.exit_code = value("--exit-code")?
                        .parse::<i32>()
                        .map_err(|e| e.to_string())?;
                }
                other => return Err(format!("unknown flag `{other}`")),
            }
        }
        Ok((b, flood))
    }
}

/// How the fake ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeExit {
    /// The exit code to use.
    pub code: i32,
    /// A message for the standard error.
    pub stderr: Option<String>,
}

/// Collects every line the fake receives.
pub type Recorder = Arc<Mutex<Vec<String>>>;

fn value_of(role: Role) -> i64 {
    match role {
        Role::Pawn => 100,
        Role::Knight | Role::Bishop => 300,
        Role::Rook => 500,
        Role::Queen => 900,
        Role::King => 0,
    }
}

fn material(position: &Position, color: Color) -> i64 {
    position
        .pieces()
        .filter(|(_, piece)| piece.color == color)
        .map(|(_, piece)| value_of(piece.role))
        .sum()
}

#[derive(Debug, Clone)]
pub(super) struct Ranked {
    pub(super) mv: Move,
    pub(super) uci: String,
    pub(super) eval: i64,
    pub(super) mate: bool,
}

pub(super) fn ranked(position: &Position, restrict: &[Move]) -> Vec<Ranked> {
    let mover = position.turn();
    let mut out: Vec<Ranked> = position
        .legal_moves()
        .into_iter()
        .filter(|m| restrict.is_empty() || restrict.contains(m))
        .filter_map(|mv| {
            let after = position.after(mv).ok()?;
            let mate = after.is_checkmate();
            let eval = material(&after, mover) - material(&after, !mover)
                + if mate { 1_000_000 } else { 0 };
            Some(Ranked {
                mv,
                uci: position.uci(mv),
                eval,
                mate,
            })
        })
        .collect();
    out.sort_by(|a, b| b.eval.cmp(&a.eval).then_with(|| a.uci.cmp(&b.uci)));
    out
}

pub(super) fn principal_variation(position: &Position, first: Move, depth: u32) -> Vec<Move> {
    let mut moves = vec![first];
    let Ok(mut current) = position.after(first) else {
        return moves;
    };
    while moves.len() < depth.min(3) as usize {
        let Some(best) = ranked(&current, &[]).into_iter().next() else {
            break;
        };
        moves.push(best.mv);
        match current.after(best.mv) {
            Ok(next) => current = next,
            Err(_) => break,
        }
    }
    moves
}

#[derive(Debug, Clone, Copy)]
enum Mode {
    Depth(u32),
    Movetime(Duration),
    Infinite,
}

#[derive(Debug)]
struct Search {
    mode: Mode,
    next_depth: u32,
    started: Instant,
    root: Position,
    candidates: Vec<Ranked>,
}

struct Fake<W> {
    b: FakeBehavior,
    out: W,
    options: Vec<(String, String)>,
    position: Position,
    search: Option<Search>,
}

/// Runs the fake engine until `quit` or the end of its input. `recorder`
/// receives every line read.
pub async fn run_fake<R, W>(
    reader: R,
    writer: W,
    behavior: FakeBehavior,
    recorder: Option<Recorder>,
) -> FakeExit
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    if behavior.protocol == FakeProtocol::XBoard {
        return super::fake_xboard::run(reader, writer, behavior, recorder).await;
    }
    let mut fake = Fake {
        b: behavior,
        out: writer,
        options: Vec::new(),
        position: Position::new(),
        search: None,
    };
    let mut lines = BufReader::new(reader).lines();
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(line)) => {
                    if let Some(recorder) = &recorder {
                        recorder.lock().unwrap_or_else(PoisonError::into_inner).push(line.clone());
                    }
                    match fake.handle(line.trim()).await {
                        Ok(None) => {}
                        Ok(Some(exit)) => return exit,
                        Err(_) => break,
                    }
                }
                _ => break,
            },
            _ = ticker.tick(), if fake.search.is_some() => {
                if fake.step().await.is_err() {
                    break;
                }
            }
        }
    }
    if fake.b.ignore_quit {
        std::future::pending::<()>().await;
    }
    FakeExit {
        code: fake.b.exit_code,
        stderr: None,
    }
}

type Io = std::io::Result<()>;

impl<W: AsyncWrite + Unpin> Fake<W> {
    async fn say_bytes(&mut self, text: &[u8]) -> Io {
        self.out.write_all(text).await?;
        self.out
            .write_all(if self.b.crlf { b"\r\n" } else { b"\n" })
            .await?;
        self.out.flush().await
    }

    async fn say(&mut self, text: &str) -> Io {
        self.say_bytes(text.as_bytes()).await
    }

    async fn handle(&mut self, line: &str) -> std::io::Result<Option<FakeExit>> {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.first().copied() {
            Some("uci") if !self.b.silent => self.handshake().await?,
            Some("isready") => self.say("readyok").await?,
            Some("setoption") => self.set_option(&words),
            Some("position") => self.set_position(&words),
            Some("go") => {
                if self.b.crash_after_go {
                    return Ok(Some(FakeExit {
                        code: if self.b.exit_code == 0 {
                            1
                        } else {
                            self.b.exit_code
                        },
                        stderr: Some("fatal: crashing on purpose".to_owned()),
                    }));
                }
                self.go(&words).await?;
            }
            Some("stop") => self.stop().await?,
            Some("ponderhit") => {
                if self.search.is_some() {
                    self.finish_search().await?;
                }
            }
            Some("quit") if !self.b.ignore_quit => {
                return Ok(Some(FakeExit {
                    code: self.b.exit_code,
                    stderr: None,
                }));
            }
            _ => {}
        }
        Ok(None)
    }

    async fn handshake(&mut self) -> Io {
        if !self.b.uciok_delay.is_zero() {
            tokio::time::sleep(self.b.uciok_delay).await;
        }
        let name = format!("id name {}", self.b.name);
        self.say(&name).await?;
        self.say("id author MyChess tests").await?;
        let mut lines = vec![
            "option name Hash type spin default 16 min 1 max 1024".to_owned(),
            "option name Threads type spin default 1 min 1 max 64".to_owned(),
        ];
        if self.b.multipv {
            lines.push("option name MultiPV type spin default 1 min 1 max 256".to_owned());
        }
        lines.push("option name Ponder type check default false".to_owned());
        if self.b.strength == FakeStrength::Full {
            lines.extend(
                [
                    "option name UCI_LimitStrength type check default false",
                    "option name UCI_Elo type spin default 1500 min 1320 max 3190",
                ]
                .map(str::to_owned),
            );
        }
        lines.push("option name UCI_ShowWDL type check default false".to_owned());
        if self.b.chess960 {
            lines.push("option name UCI_Chess960 type check default false".to_owned());
        }
        lines.extend(
            [
                "option name UCI_AnalyseMode type check default false",
                "option name Style type combo default Normal var Solid var Normal var Risky",
            ]
            .map(str::to_owned),
        );
        if self.b.strength != FakeStrength::Nothing {
            lines.push("option name Skill Level type spin default 20 min 0 max 20".to_owned());
        }
        lines.extend(
            [
                "option name SyzygyPath type string default <empty>",
                "option name Clear Hash type button",
            ]
            .map(str::to_owned),
        );
        for line in lines {
            self.say(&line).await?;
        }
        self.say("uciok").await
    }

    fn set_option(&mut self, words: &[&str]) {
        let Some(name_at) = words.iter().position(|w| *w == "name") else {
            return;
        };
        let value_at = words.iter().position(|w| *w == "value");
        let name_end = value_at.unwrap_or(words.len());
        let name = words[name_at + 1..name_end].join(" ");
        let value = value_at
            .map(|v| words[v + 1..].join(" "))
            .unwrap_or_default();
        self.options.retain(|(n, _)| *n != name);
        self.options.push((name, value));
    }

    fn option(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn set_position(&mut self, words: &[&str]) {
        let moves_at = words.iter().position(|w| *w == "moves");
        let head_end = moves_at.unwrap_or(words.len());
        let head = &words[1..head_end];
        let mut position = match head.first().copied() {
            Some("startpos") => Position::new(),
            Some("fen") => match Position::from_fen(&head[1..].join(" ")) {
                Ok(p) => p,
                Err(_) => return,
            },
            _ => return,
        };
        if let Some(at) = moves_at {
            for text in &words[at + 1..] {
                match position.parse_uci(text) {
                    Ok(m) if position.play(m).is_ok() => {}
                    _ => break,
                }
            }
        }
        self.position = position;
    }

    async fn go(&mut self, words: &[&str]) -> Io {
        let root = self.position.clone();
        let mut mode = Mode::Infinite;
        let mut restrict = Vec::new();
        let mut pondering = false;
        let mut i = 1;
        while i < words.len() {
            match words[i] {
                "depth" => {
                    if let Some(n) = words.get(i + 1).and_then(|w| w.parse::<u32>().ok()) {
                        mode = Mode::Depth(n.clamp(1, MAX_DEPTH));
                    }
                    i += 1;
                }
                "movetime" => {
                    if let Some(ms) = words.get(i + 1).and_then(|w| w.parse::<u64>().ok()) {
                        mode = Mode::Movetime(Duration::from_millis(ms));
                    }
                    i += 1;
                }
                "searchmoves" => {
                    for text in &words[i + 1..] {
                        if let Ok(m) = root.parse_uci(text) {
                            restrict.push(m);
                        }
                    }
                    break;
                }
                "ponder" => pondering = true,
                _ => {}
            }
            i += 1;
        }
        if pondering {
            // A search of `go ponder` runs until `ponderhit` or `stop`, whatever
            // limits come with it (UCI: the engine must not move before).
            mode = Mode::Infinite;
        }
        let candidates = ranked(&root, &restrict);
        if candidates.is_empty() {
            let score = if root.is_checkmate() {
                "mate 0"
            } else {
                "cp 0"
            };
            self.say(&format!("info depth 0 score {score}")).await?;
            return self.say("bestmove (none)").await;
        }
        if self.b.garbage {
            let big = format!("info string {}", "A".repeat(2 * 1024 * 1024));
            self.say(&big).await?;
        }
        self.search = Some(Search {
            mode,
            next_depth: 1,
            started: Instant::now(),
            root,
            candidates,
        });
        if let Mode::Depth(n) = mode {
            for _ in 1..=n {
                self.emit_depth().await?;
            }
            self.finish_search().await?;
        }
        Ok(())
    }

    async fn step(&mut self) -> Io {
        let Some(search) = &self.search else {
            return Ok(());
        };
        if let Mode::Movetime(limit) = search.mode
            && search.started.elapsed() >= limit
        {
            return self.finish_search().await;
        }
        self.emit_depth().await
    }

    async fn emit_depth(&mut self) -> Io {
        let Some(search) = &mut self.search else {
            return Ok(());
        };
        let depth = search.next_depth.min(MAX_DEPTH);
        search.next_depth = (search.next_depth + 1).min(MAX_DEPTH);
        let root = search.root.clone();
        let candidates = search.candidates.clone();
        let lines = self
            .option("MultiPV")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(1)
            .max(1);
        let wdl = self.option("UCI_ShowWDL") == Some("true");
        if self.b.garbage {
            self.say("this is not a UCI line").await?;
            self.say("info depth").await?;
            self.say("info depth x score cp y").await?;
            self.say("info depth 3 score foo 12 pv e2e4").await?;
            self.say_bytes(b"info string \xff\xfe bad bytes").await?;
        }
        for (k, cand) in candidates.iter().take(lines).enumerate() {
            let score = if cand.mate {
                "mate 1".to_owned()
            } else {
                format!("cp {}", cand.eval)
            };
            let pv: Vec<String> = principal_variation(&root, cand.mv, depth)
                .into_iter()
                .scan(root.clone(), |pos, m| {
                    let text = pos.uci(m);
                    *pos = pos.after(m).ok()?;
                    Some(text)
                })
                .collect();
            let wdl_text = if wdl {
                let x = cand.eval.clamp(-600, 600);
                let (win, loss) = (300 + x / 2, 300 - x / 2);
                format!(" wdl {win} {} {loss}", 1000 - win - loss)
            } else {
                String::new()
            };
            let line = format!(
                "info depth {depth} seldepth {} multipv {} score {score}{wdl_text} nodes {} nps 1000000 time {} pv {}",
                depth + 1,
                k + 1,
                u64::from(depth) * 1000 * (k as u64 + 1),
                u64::from(depth) * 5,
                pv.join(" ")
            );
            self.say(&line).await?;
        }
        Ok(())
    }

    async fn finish_search(&mut self) -> Io {
        let Some(search) = self.search.take() else {
            return Ok(());
        };
        let Some(best) = search.candidates.first() else {
            return self.say("bestmove (none)").await;
        };
        let ponder = if self.option("Ponder") == Some("true") {
            principal_variation(&search.root, best.mv, 2)
                .get(1)
                .copied()
        } else {
            None
        };
        let mut line = format!("bestmove {}", search.root.uci(best.mv));
        if let Some(p) = ponder
            && let Ok(after) = search.root.after(best.mv)
        {
            line.push_str(" ponder ");
            line.push_str(&after.uci(p));
        }
        self.say(&line).await
    }

    async fn stop(&mut self) -> Io {
        if self.b.hang_on_stop || self.search.is_none() {
            return Ok(());
        }
        if self.b.no_bestmove_after_stop {
            self.search = None;
            return Ok(());
        }
        self.finish_search().await
    }
}
