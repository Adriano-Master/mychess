//! The session task for an input engine (DGT driver): the handshake state
//! machine, the translation of its lines and the shutdown (spec m8-10,
//! D2715-D2718, D2720-D2722).

use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::Child;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use tokio::time::{Instant, sleep_until, timeout};

use super::gate::LineGate;
use super::{Channels, send_last};
use crate::error::EboardError;
use crate::event::{Direction, EboardCommand, EboardEvent, Note, Reason, Warning};
use crate::input::{
    self, CMD_ANNOUNCE, CMD_GET_POSITION, CMD_QUIT, CMD_STOP, CMD_XBOARD, DriverLine,
};
use crate::line::{Line, LineSplitter};

/// How many `stderr` lines of the driver are kept for the report.
const STDERR_TAIL_LINES: usize = 10;
/// The longest `stderr` line kept.
const STDERR_LINE_BYTES: usize = 4096;

/// The waits of the driver session (D2716).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// From the start to `Ready`.
    pub handshake: Duration,
    /// After `stop` and `quit`, before the process is killed.
    pub shutdown: Duration,
}

impl Default for Timeouts {
    fn default() -> Timeouts {
        Timeouts {
            handshake: Duration::from_secs(20),
            shutdown: Duration::from_secs(1),
        }
    }
}

/// The process behind a driver session.
#[derive(Debug)]
pub struct DriverProcess {
    child: Child,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
}

impl DriverProcess {
    fn tail(&self) -> Vec<String> {
        self.stderr_tail
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }
}

/// The command line of the driver (D2717): program, port and each word of
/// the parameters, never joined into a shell string.
pub(crate) fn driver_args(port: &str, params: &str) -> Vec<OsString> {
    let mut args = vec![OsString::from(port.trim())];
    args.extend(params.split_whitespace().map(OsString::from));
    args
}

/// The working directory: the directory of the driver when the command has
/// one (`multiplataforma.md`).
pub(crate) fn working_dir(command: &Path) -> Option<PathBuf> {
    if command.components().count() > 1 {
        let absolute = std::path::absolute(command).unwrap_or_else(|_| command.to_path_buf());
        absolute
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
    } else {
        None
    }
}

pub(crate) fn spawn_process(
    command: &Path,
    port: &str,
    params: &str,
) -> Result<
    (
        tokio::process::ChildStdout,
        tokio::process::ChildStdin,
        DriverProcess,
    ),
    EboardError,
> {
    let spawn_error = |message: String| EboardError::Spawn {
        path: command.to_path_buf(),
        message,
    };
    if command.as_os_str().is_empty() || command.to_string_lossy().contains('\0') {
        return Err(EboardError::InvalidDriver("empty command".to_owned()));
    }
    let program = if command.components().count() > 1 {
        std::path::absolute(command).unwrap_or_else(|_| command.to_path_buf())
    } else {
        command.to_path_buf()
    };
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(driver_args(port, params));
    if let Some(dir) = working_dir(command) {
        cmd.current_dir(dir);
    }
    crate::platform::hide_console(&mut cmd);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| spawn_error(e.to_string()))?;
    let missing = |what: &str| spawn_error(format!("the driver {what} is not a pipe"));
    let stdin = child.stdin.take().ok_or_else(|| missing("stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| missing("stdout"))?;
    let stderr = child.stderr.take().ok_or_else(|| missing("stderr"))?;
    let tail = Arc::new(Mutex::new(VecDeque::new()));
    tokio::spawn(drain_stderr(stderr, Arc::clone(&tail)));
    Ok((
        stdout,
        stdin,
        DriverProcess {
            child,
            stderr_tail: tail,
        },
    ))
}

async fn drain_stderr<R: AsyncRead + Unpin>(mut stderr: R, tail: Arc<Mutex<VecDeque<String>>>) {
    let mut splitter = LineSplitter::new();
    let mut buf = [0_u8; 1024];
    let mut lines = Vec::new();
    loop {
        let n = match stderr.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        splitter.feed(&buf[..n], &mut lines);
        let mut guard = tail.lock().unwrap_or_else(PoisonError::into_inner);
        for line in lines.drain(..) {
            if guard.len() == STDERR_TAIL_LINES {
                guard.pop_front();
            }
            let mut text = line.text;
            text.truncate(floor_boundary(&text, STDERR_LINE_BYTES));
            guard.push_back(text);
        }
    }
}

fn floor_boundary(text: &str, max: usize) -> usize {
    let mut at = max.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Starting,
    Initialising,
    Ready,
}

enum Wire {
    Line(Line),
    Eof,
}

#[allow(clippy::too_many_lines)] // one flat select loop reads best
pub(super) async fn run<R, W>(
    reader: R,
    writer: W,
    mut process: Option<DriverProcess>,
    timeouts: Timeouts,
    ch: Channels,
) where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let Channels {
        mut commands,
        events,
    } = ch;
    let (line_tx, mut line_rx) = mpsc::channel::<Wire>(crate::EVENT_QUEUE);
    let read_task = tokio::spawn(read_lines(reader, line_tx));
    let (write_tx, write_task) = spawn_writer(writer);

    let mut phase = Phase::Starting;
    let deadline = Instant::now() + timeouts.handshake;
    let mut gate = LineGate::new(Instant::now(), crate::MAX_LINES_PER_SECOND);

    // The traffic shown in the console may be dropped when the application
    // is slow; only a closed channel ends the session.
    macro_rules! emit_log {
        ($event:expr) => {
            if let Err(TrySendError::Closed(_)) = events.try_send($event) {
                read_task.abort();
                write_task.abort();
                return;
            }
        };
    }

    macro_rules! emit {
        ($event:expr) => {
            if events.send($event).await.is_err() {
                read_task.abort();
                write_task.abort();
                return;
            }
        };
    }
    macro_rules! say {
        ($text:expr) => {{
            let text: String = $text;
            if write_tx.try_send(text.clone()).is_ok() {
                emit_log!(EboardEvent::Log {
                    direction: Direction::Out,
                    text
                });
            } else {
                emit!(EboardEvent::Warning(Warning::Refused(text)));
            }
        }};
    }

    let reason = loop {
        tokio::select! {
            wire = line_rx.recv() => match wire {
                None | Some(Wire::Eof) => break Reason::DriverExited,
                Some(Wire::Line(line)) => {
                    if !gate.admit(Instant::now()) {
                        continue;
                    }
                    emit_log!(EboardEvent::Log { direction: Direction::In, text: line.text.clone() });
                    match input::parse_line(&line.text) {
                        DriverLine::UnreadableMove(text) => {
                            emit!(EboardEvent::Note(Note::UnreadableMove(text)));
                        }
                        DriverLine::Error(text) => {
                            emit!(EboardEvent::DriverError(text.clone()));
                            break Reason::DriverError(text);
                        }
                        // The two steps of the handshake count only in their
                        // order (D2715); out of order, the line is in the
                        // console and nothing else happens.
                        DriverLine::BoardFound if phase == Phase::Starting => {
                            say!(CMD_XBOARD.to_owned());
                            say!(CMD_ANNOUNCE.to_owned());
                            phase = Phase::Initialising;
                        }
                        DriverLine::XboardMode if phase == Phase::Initialising => {
                            say!(CMD_GET_POSITION.to_owned());
                            phase = Phase::Ready;
                            emit!(EboardEvent::Ready);
                        }
                        DriverLine::BoardFound | DriverLine::XboardMode => {}
                        other => {
                            if let Some(event) = plain_event(other) {
                                emit!(event);
                            }
                        }
                    }
                }
            },
            command = commands.recv() => match command {
                None | Some(EboardCommand::Close) => break Reason::Requested,
                Some(EboardCommand::Move { .. }) => {}
                Some(other) => {
                    if phase == Phase::Ready {
                        for line in input::commands_for(&other) {
                            say!(line);
                        }
                    } else {
                        emit!(EboardEvent::Warning(Warning::NotReady));
                    }
                }
            },
            () = sleep_until(deadline), if phase != Phase::Ready => {
                break Reason::HandshakeTimeout;
            },
            () = wait_report(gate.report_due()), if gate.report_due().is_some() => {
                if let Some(n) = gate.flush(Instant::now()) {
                    emit!(EboardEvent::Note(Note::LinesOmitted(n)));
                }
            },
        }
    };

    goodbye(
        &reason,
        write_tx,
        write_task,
        process.as_mut(),
        timeouts,
        &events,
    )
    .await;
    read_task.abort();
    send_last(&events, EboardEvent::Disconnected(reason)).await;
}

async fn wait_report(at: Option<Instant>) {
    match at {
        Some(at) => sleep_until(at).await,
        None => std::future::pending::<()>().await,
    }
}

/// Writes go through a queue and a task: a driver that stops reading its
/// input must not freeze the session.
fn spawn_writer<W>(mut writer: W) -> (mpsc::Sender<String>, tokio::task::JoinHandle<()>)
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (tx, mut rx) = mpsc::channel::<String>(64);
    let task = tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            let mut bytes = line.into_bytes();
            bytes.push(b'\n');
            if writer.write_all(&bytes).await.is_err() || writer.flush().await.is_err() {
                break;
            }
        }
        writer.shutdown().await.ok();
    });
    (tx, task)
}

/// Takes leave of the driver: `stop` and `quit` when it may still listen,
/// close its input, wait for the process, kill it when it lingers (D2716).
async fn goodbye(
    reason: &Reason,
    write_tx: mpsc::Sender<String>,
    write_task: tokio::task::JoinHandle<()>,
    process: Option<&mut DriverProcess>,
    timeouts: Timeouts,
    events: &mpsc::Sender<EboardEvent>,
) {
    if matches!(
        reason,
        Reason::Requested | Reason::HandshakeTimeout | Reason::DriverError(_)
    ) {
        for line in [CMD_STOP, CMD_QUIT] {
            if write_tx.try_send(line.to_owned()).is_ok() {
                // Console traffic: dropped when the application is slow.
                events
                    .try_send(EboardEvent::Log {
                        direction: Direction::Out,
                        text: line.to_owned(),
                    })
                    .ok();
            }
        }
    }
    drop(write_tx);
    // The writer task ends after flushing the goodbye and closes the input.
    timeout(timeouts.shutdown, write_task).await.ok();
    if let Some(process) = process {
        if timeout(timeouts.shutdown, process.child.wait())
            .await
            .is_err()
        {
            process.child.kill().await.ok();
        }
        let tail = process.tail();
        if !tail.is_empty() && *reason != Reason::Requested {
            send_last(
                events,
                EboardEvent::Note(Note::DriverStderr(tail.join(" | "))),
            )
            .await;
        }
    }
}

/// The events that need no state: a direct translation of the line.
fn plain_event(line: DriverLine) -> Option<EboardEvent> {
    Some(match line {
        DriverLine::Move(mv) => EboardEvent::Move { mv, side: None },
        DriverLine::StartupFen => EboardEvent::StartupFen,
        DriverLine::Fen(fen) => EboardEvent::Fen(fen),
        DriverLine::MovingPiece(c) => EboardEvent::MovingPiece(c),
        DriverLine::NewGame => EboardEvent::NewGame,
        DriverLine::GameEnd(result) => EboardEvent::GameEnd(result),
        DriverLine::SetupMode(on) => EboardEvent::SetupMode(on),
        DriverLine::ToMove(color) => EboardEvent::ToMove(color),
        DriverLine::NoClock => EboardEvent::NoClock,
        DriverLine::Clock { color, seconds } => EboardEvent::Clock { color, seconds },
        DriverLine::BoardIdentified(text) => EboardEvent::BoardIdentified(text),
        _ => return None,
    })
}

async fn read_lines<R: AsyncRead + Unpin>(mut reader: R, tx: mpsc::Sender<Wire>) {
    let mut splitter = LineSplitter::new();
    let mut buf = [0_u8; 1024];
    let mut lines = Vec::new();
    loop {
        match reader.read(&mut buf).await {
            Ok(0) | Err(_) => {
                if let Some(line) = splitter.finish() {
                    tx.send(Wire::Line(line)).await.ok();
                }
                tx.send(Wire::Eof).await.ok();
                return;
            }
            Ok(n) => {
                splitter.feed(&buf[..n], &mut lines);
                for line in lines.drain(..) {
                    if tx.send(Wire::Line(line)).await.is_err() {
                        return;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_separate_words() {
        let args = driver_args(" /dev/ttyUSB0 ", "la  extra");
        assert_eq!(
            args,
            [
                OsString::from("/dev/ttyUSB0"),
                OsString::from("la"),
                OsString::from("extra")
            ]
        );
    }

    #[test]
    fn working_directory_is_the_drivers_only_when_it_has_one() {
        assert_eq!(working_dir(Path::new("dgtdrv2")), None);
        let exe = std::env::temp_dir().join("drivers").join("dgtdrv2");
        assert_eq!(working_dir(&exe), exe.parent().map(Path::to_path_buf));
    }

    #[test]
    fn stderr_lines_are_cut_on_a_char_boundary() {
        let text = "é".repeat(10);
        assert_eq!(floor_boundary(&text, 5), 4);
        assert_eq!(floor_boundary(&text, 100), text.len());
    }
}
