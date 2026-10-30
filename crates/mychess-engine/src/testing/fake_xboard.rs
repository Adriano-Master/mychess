//! The XBoard (CECP) dialect of the fake engine (spec m4-2, "Engine falsa").
//!
//! It shares the material-greedy ranking of the UCI fake. Thinking lines are
//! `"%2d. %+5d %5d %8d  %s"`: depth, score (side to move, or White with
//! `white_pov`), time (depth x 10 centiseconds, or `depth` seconds), nodes
//! (depth x 1000) and the variation. A mate scores `100001`.

use std::sync::PoisonError;
use std::time::Duration;

use mychess_core::{Color, Position, SanMode};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::time::Instant;

use super::fake::{FakeBehavior, FakeExit, Ranked, Recorder, principal_variation, ranked};

const TICK: Duration = Duration::from_millis(5);
const MAX_DEPTH: u32 = 99;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Analyze,
    Go,
}

#[derive(Debug, Clone, Copy)]
enum Limit {
    Depth(u32),
    Seconds(u64),
    Infinite,
}

#[derive(Debug)]
struct Search {
    kind: Kind,
    limit: Limit,
    next_depth: u32,
    started: Instant,
    root: Position,
    candidates: Vec<Ranked>,
}

struct XFake<W> {
    b: FakeBehavior,
    out: W,
    position: Position,
    history: Vec<Position>,
    st: u64,
    sd: u32,
    search: Option<Search>,
    last: (u32, u64),
    /// `time` arrived since the last `go`: the search follows the clock.
    clocked: bool,
}

type Io = std::io::Result<()>;

pub(super) async fn run<R, W>(
    reader: R,
    writer: W,
    behavior: FakeBehavior,
    recorder: Option<Recorder>,
) -> FakeExit
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut fake = XFake {
        b: behavior,
        out: writer,
        position: Position::new(),
        history: Vec::new(),
        st: 120_000,
        sd: 50,
        search: None,
        last: (0, 0),
        clocked: false,
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

impl<W: AsyncWrite + Unpin> XFake<W> {
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

    fn apply_move(&mut self, text: &str) -> bool {
        let found = self
            .position
            .parse_uci(text)
            .ok()
            .or_else(|| {
                self.position
                    .parse_san(text, SanMode::Tolerant)
                    .ok()
                    .map(|p| p.mv)
            })
            .filter(|m| self.position.is_legal(*m));
        let Some(mv) = found else { return false };
        let before = self.position.clone();
        if self.position.play(mv).is_err() {
            return false;
        }
        self.history.push(before);
        true
    }

    async fn handle(&mut self, line: &str) -> std::io::Result<Option<FakeExit>> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some(first) = words.first().copied() else {
            return Ok(None);
        };
        if self.b.silent && first != "quit" {
            return Ok(None);
        }
        match first {
            "xboard" => {
                if let Some(banner) = self.b.xboard.banner.clone() {
                    self.say(&banner).await?;
                }
            }
            "protover" => {
                if self.b.xboard.legacy {
                    self.say("Error (unknown command): protover").await?;
                } else {
                    self.announce().await?;
                }
            }
            "new" => {
                self.position = Position::new();
                self.history.clear();
                self.search = None;
            }
            "force" => {
                if self.search.as_ref().is_some_and(|s| s.kind == Kind::Go) {
                    self.search = None;
                }
            }
            "setboard" => {
                if let Ok(p) = Position::from_fen(&words[1..].join(" ")) {
                    self.position = p;
                    self.history.clear();
                } else {
                    self.say("Error (bad FEN): setboard").await?;
                }
            }
            "undo" => {
                if let Some(p) = self.history.pop() {
                    self.position = p;
                }
            }
            "st" => self.st = words.get(1).and_then(|w| w.parse().ok()).unwrap_or(self.st),
            "sd" => self.sd = words.get(1).and_then(|w| w.parse().ok()).unwrap_or(self.sd),
            "analyze" => {
                if self.features_say("analyze=0") {
                    self.say("Error (unknown command): analyze").await?;
                } else {
                    self.start(Kind::Analyze).await?;
                }
            }
            "time" => self.clocked = true,
            "go" => self.start(Kind::Go).await?,
            "exit" => self.search = None,
            "?" => {
                if !self.b.hang_on_stop && self.search.as_ref().is_some_and(|s| s.kind == Kind::Go)
                {
                    self.finish_go().await?;
                }
            }
            "ping" => {
                if !self.b.xboard.no_pong {
                    let n = words.get(1).copied().unwrap_or("0");
                    self.say(&format!("pong {n}")).await?;
                }
            }
            "." => self.stat01().await?,
            "quit" if !self.b.ignore_quit => {
                return Ok(Some(FakeExit {
                    code: self.b.exit_code,
                    stderr: None,
                }));
            }
            "usermove" => {
                let ok = words.get(1).is_some_and(|m| self.apply_move(m));
                if !ok {
                    self.say(&format!("Illegal move: {}", words.get(1).unwrap_or(&"")))
                        .await?;
                }
            }
            "accepted" | "rejected" | "easy" | "hard" | "post" | "nopost" | "random"
            | "computer" | "level" | "otim" | "name" | "rating" | "result" | "draw" | "white"
            | "black" | "log" | "noise" | "egtb" | "resign" | "quit" => {}
            other => {
                // A bare move, or an unknown command.
                if words.len() > 1 || !self.apply_move(other) {
                    self.say(&format!("Error (unknown command): {other}"))
                        .await?;
                }
            }
        }
        Ok(None)
    }

    async fn stat01(&mut self) -> Io {
        if self.search.is_none() {
            return Ok(());
        }
        let (depth, _) = self.last;
        let text = format!(
            "stat01: {} {} {depth} 20 20 e2e4",
            depth * 10,
            u64::from(depth) * 1000
        );
        self.say(&text).await
    }

    /// Whether the `--features` string contains `token`.
    fn features_say(&self, token: &str) -> bool {
        self.b
            .xboard
            .features
            .split_whitespace()
            .any(|t| t == token)
    }

    async fn announce(&mut self) -> Io {
        let slow = self.b.xboard.slow_feature;
        let name = &self.b.name;
        let features: Vec<&str> = self
            .b
            .xboard
            .features
            .split_whitespace()
            .filter(|t| !(slow > Duration::ZERO && t.starts_with("done=")))
            .collect();
        let mut line = format!("feature myname=\"{name}\" {}", features.join(" "));
        if slow > Duration::ZERO {
            line.push_str(" done=0");
        }
        self.say(line.trim_end()).await?;
        if slow > Duration::ZERO {
            tokio::time::sleep(slow).await;
            self.say("feature done=1").await?;
        }
        Ok(())
    }

    async fn start(&mut self, kind: Kind) -> Io {
        if self.b.crash_after_go {
            return Err(std::io::Error::other("crash"));
        }
        let root = self.position.clone();
        let candidates = ranked(&root, &[]);
        if candidates.is_empty() {
            return Ok(());
        }
        // With a clock the fake plays to a fixed depth: quick and the same
        // every time (spec M8-2c, D2527).
        let clocked = std::mem::take(&mut self.clocked);
        let limit = match kind {
            Kind::Go if clocked => Limit::Depth(3),
            Kind::Go if self.sd < 50 => Limit::Depth(self.sd.clamp(1, MAX_DEPTH)),
            Kind::Go if self.st < 120_000 => Limit::Seconds(self.st),
            Kind::Go | Kind::Analyze => Limit::Infinite,
        };
        self.search = Some(Search {
            kind,
            limit,
            next_depth: 1,
            started: Instant::now(),
            root,
            candidates,
        });
        if let Limit::Depth(n) = limit {
            for _ in 1..=n {
                self.emit_depth().await?;
            }
            self.finish_go().await?;
        }
        Ok(())
    }

    async fn step(&mut self) -> Io {
        let Some(search) = &self.search else {
            return Ok(());
        };
        if let Limit::Seconds(s) = search.limit
            && search.started.elapsed() >= Duration::from_secs(s)
        {
            return self.finish_go().await;
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
        let Some(best) = search.candidates.first().cloned() else {
            return Ok(());
        };
        if self.b.garbage {
            self.say("this is not a thinking line").await?;
            self.say("3. x y z w").await?;
            self.say_bytes(b"\xff\xfe bad bytes").await?;
        }
        let mut score = if best.mate { 100_001 } else { best.eval };
        if self.b.xboard.white_pov && root.turn() == Color::Black {
            score = -score;
        }
        let time = if self.b.xboard.seconds {
            u64::from(depth)
        } else {
            u64::from(depth) * 10
        };
        let nodes = u64::from(depth) * 1000;
        let pv = self.pv_text(&root, &best, depth);
        self.last = (depth, nodes);
        let line = format!("{depth:>2}. {score:+5} {time:5} {nodes:8}  {pv}");
        self.say(&line).await
    }

    fn pv_text(&self, root: &Position, best: &Ranked, depth: u32) -> String {
        let moves = principal_variation(root, best.mv, depth);
        let mut position = root.clone();
        let mut parts = Vec::new();
        for (i, m) in moves.iter().enumerate() {
            let text = if self.b.xboard.san_pv {
                position.san(*m).unwrap_or_else(|_| position.uci(*m))
            } else {
                position.uci(*m)
            };
            if self.b.xboard.san_pv {
                let n = position.fullmove_number();
                match position.turn() {
                    Color::White => parts.push(format!("{n}.")),
                    Color::Black if i == 0 => parts.push(format!("{n}...")),
                    Color::Black => {}
                }
            }
            parts.push(text);
            match position.after(*m) {
                Ok(next) => position = next,
                Err(_) => break,
            }
        }
        parts.join(" ")
    }

    async fn finish_go(&mut self) -> Io {
        let Some(search) = self.search.take() else {
            return Ok(());
        };
        if self.b.xboard.resign {
            return self.say("resign").await;
        }
        let Some(best) = search.candidates.first() else {
            return self.say("resign").await;
        };
        let text = search.root.uci(best.mv);
        self.apply_move(&text);
        self.say(&format!("move {text}")).await
    }
}
