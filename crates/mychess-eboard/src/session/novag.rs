//! The session task for a Novag Citrine (spec m8-10, D2712-D2714).

use std::collections::VecDeque;
use tokio::sync::mpsc::error::TrySendError;
use tokio::time::{Instant, sleep_until};

use super::gate::LineGate;
use super::{Channels, send_last};
use crate::event::{Direction, EboardCommand, EboardEvent, Note, Reason, Warning};
use crate::novag::{self, INIT_COMMANDS, NovagLine, frame};
use crate::serial::{DeviceIo, DeviceThreads, IoEvent, spawn_io};

pub use crate::novag::NovagTimings;

struct Scheduled {
    at: Instant,
    line: String,
    /// The last start-up command: `Ready` follows it.
    ready_after: bool,
}

#[allow(clippy::too_many_lines)] // one flat select loop reads best
pub(super) async fn run(device: DeviceIo, timings: NovagTimings, ch: Channels) {
    let Channels {
        mut commands,
        events,
    } = ch;
    let mut io = spawn_io(device);
    let start = Instant::now();
    let mut outbox: VecDeque<Scheduled> = VecDeque::new();
    let mut busy_until = start;
    for (i, line) in INIT_COMMANDS.iter().enumerate() {
        let at = start + timings.init_gap * u32::try_from(i).unwrap_or(0);
        busy_until = at;
        outbox.push_back(Scheduled {
            at,
            line: (*line).to_owned(),
            ready_after: i + 1 == INIT_COMMANDS.len(),
        });
    }
    let mut warn_at = Some(start + timings.reply_warning);
    let mut gate = LineGate::new(start, crate::MAX_LINES_PER_SECOND);

    // The traffic shown in the console may be dropped when the application
    // is slow; only a closed channel ends the session.
    macro_rules! emit_log {
        ($event:expr) => {
            if let Err(TrySendError::Closed(_)) = events.try_send($event) {
                return;
            }
        };
    }

    macro_rules! emit {
        ($event:expr) => {
            if events.send($event).await.is_err() {
                return;
            }
        };
    }

    let reason = loop {
        let next_due = outbox.front().map(|s| s.at);
        tokio::select! {
            incoming = io.events.recv() => match incoming {
                Some(IoEvent::Line(line)) => {
                    warn_at = None;
                    if !gate.admit(Instant::now()) {
                        continue;
                    }
                    emit_log!(EboardEvent::Log { direction: Direction::In, text: line.text.clone() });
                    match novag::parse_line(&line.text) {
                        NovagLine::NewGame => {
                            emit!(EboardEvent::NewGame);
                            let at = Instant::now().max(busy_until);
                            busy_until = at;
                            outbox.push_back(Scheduled { at, line: "U ON".to_owned(), ready_after: false });
                        }
                        NovagLine::Move { mv, side } => {
                            emit!(EboardEvent::Move { mv, side: Some(side) });
                        }
                        NovagLine::TakeBack => emit!(EboardEvent::TakeBack),
                        NovagLine::UnreadableMove(text) => {
                            emit!(EboardEvent::Note(Note::UnreadableMove(text)));
                        }
                        NovagLine::Other => {}
                    }
                }
                Some(IoEvent::Lost(message)) => break Reason::DeviceLost(message),
                None => break Reason::DeviceLost("the reader ended".to_owned()),
            },
            command = commands.recv() => match command {
                None | Some(EboardCommand::Close) => break Reason::Requested,
                Some(EboardCommand::Move { uci, capture }) => {
                    let first = Instant::now().max(busy_until);
                    let gap = if capture { timings.capture_gap } else { timings.move_gap };
                    let line = novag::move_command(&uci);
                    outbox.push_back(Scheduled { at: first, line: line.clone(), ready_after: false });
                    busy_until = first + gap;
                    outbox.push_back(Scheduled { at: busy_until, line, ready_after: false });
                }
                Some(EboardCommand::Raw(text)) => {
                    if let Some(line) = crate::input::sanitize_raw(&text) {
                        let at = Instant::now().max(busy_until);
                        busy_until = at;
                        outbox.push_back(Scheduled { at, line, ready_after: false });
                    }
                }
                Some(_) => {
                    emit!(EboardEvent::Note(Note::InputEngineOnly));
                }
            },
            () = wait_until(next_due), if next_due.is_some() => {
                if let Some(item) = outbox.pop_front() {
                    if !io.write(frame(&item.line)) {
                        break Reason::DeviceLost("cannot write to the port".to_owned());
                    }
                    emit_log!(EboardEvent::Log { direction: Direction::Out, text: item.line });
                    if item.ready_after {
                        emit!(EboardEvent::Ready);
                    }
                }
            },
            () = wait_until(gate.report_due()), if gate.report_due().is_some() => {
                if let Some(n) = gate.flush(Instant::now()) {
                    emit!(EboardEvent::Note(Note::LinesOmitted(n)));
                }
            },
            () = wait_until(warn_at), if warn_at.is_some() => {
                warn_at = None;
                emit!(EboardEvent::Warning(Warning::NoReply));
            },
        }
    };
    finish(&events, io, reason).await;
}

async fn wait_until(at: Option<Instant>) {
    match at {
        Some(at) => sleep_until(at).await,
        None => std::future::pending::<()>().await,
    }
}

async fn finish(
    events: &tokio::sync::mpsc::Sender<EboardEvent>,
    io: DeviceThreads,
    reason: Reason,
) {
    // Windows keeps a port open as long as a handle exists: the threads end
    // (and release theirs) before the session says it is over, so a new
    // connection to the same port does not find it busy.
    io.shutdown().await;
    send_last(events, EboardEvent::Disconnected(reason)).await;
}
