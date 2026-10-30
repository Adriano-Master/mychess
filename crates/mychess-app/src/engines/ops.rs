//! What `perform` does for the engine effects (spec M4-3). Every function
//! here runs off the update loop and answers with the [`Message`] to feed
//! back.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mychess_engine::{
    EngineCommand, EngineConfig, EngineError, EngineIdentity, Protocol, detect_protocol, probe_uci,
    probe_xboard,
};

use super::check::{FsProbe, check_command, path_dirs_from_env};
use super::config::RunDefaults;
use super::find::{find_installed_engines, usual_engine_dirs};
use super::message::EngineMessage;
use super::model::{EngineDraft, EngineEntry};
use super::run::{CommandError, EngineFailure, EngineHub, RunId, SHUTDOWN_WAIT};
use super::scid::{MAX_SCID_BYTES, parse_scid_engines_dat};
use crate::error::{FileError, FileErrorKind, RequestId};
use crate::message::Message;

/// How long a found engine gets to say `uciok` before XBoard is tried.
const FOUND_UCI_WAIT: Duration = Duration::from_secs(3);

fn engine(message: EngineMessage) -> Message {
    Message::Engine(message)
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

pub(crate) async fn start(
    hub: &EngineHub,
    run: RunId,
    entry: &EngineEntry,
    defaults: &RunDefaults,
) -> Message {
    let config = entry.to_config(defaults);
    let Some(launched) = hub.launch(run, config).await else {
        // Cancelled by a stop or a quit: the answer only closes the run.
        return engine(EngineMessage::StartFailed {
            run,
            failure: EngineFailure::from_error(&EngineError::Closed),
        });
    };
    match launched {
        Ok((started, events)) => {
            if let Some(identity) = started.identity().cloned() {
                hub.adopt(run, started, events);
                engine(EngineMessage::Started {
                    run,
                    identity: Box::new(identity),
                    at: unix_seconds(),
                })
            } else {
                started.shutdown().await;
                engine(EngineMessage::StartFailed {
                    run,
                    failure: EngineFailure::from_error(&EngineError::Closed),
                })
            }
        }
        Err(error) => {
            tracing::warn!(%error, run = run.0, "engine did not start");
            engine(EngineMessage::StartFailed {
                run,
                failure: EngineFailure::from_error(&error),
            })
        }
    }
}

pub(crate) async fn stop(hub: &EngineHub, run: RunId) -> Message {
    hub.stop(run).await;
    engine(EngineMessage::Stopped(run))
}

pub(crate) async fn shutdown(hub: &EngineHub) -> Message {
    hub.shutdown_all(SHUTDOWN_WAIT).await;
    engine(EngineMessage::ShutdownDone)
}

pub(crate) fn command(hub: &EngineHub, run: RunId, command: EngineCommand) -> Message {
    let kind = super::run::CommandKind::of(&command);
    match hub.send(run, command) {
        Err(error @ (CommandError::Busy | CommandError::Refused)) => {
            engine(EngineMessage::CommandFailed { run, kind, error })
        }
        result => engine(EngineMessage::CommandDone { run, result }),
    }
}

/// The entries of the protocol log of `run` from `since` on; nothing when
/// the engine is gone. The ring is locked for a copy only, so this is cheap
/// enough to run on the executor.
pub(crate) fn read_log(hub: &EngineHub, run: RunId, since: u64) -> Message {
    let (entries, next) = hub
        .log(run)
        .map_or((Vec::new(), since), |log| log.snapshot_since(since));
    Message::Analysis(crate::AnalysisMessage::LogRead {
        run,
        since,
        entries,
        next,
    })
}

/// Writes the protocol log of `run` to `path`.
pub(crate) async fn save_log(hub: &EngineHub, run: RunId, path: PathBuf) -> Message {
    let result = match hub.log(run) {
        Some(log) => {
            let target = path.clone();
            tokio::task::spawn_blocking(move || log.save_to(&target))
                .await
                .unwrap_or_else(|_| Err(std::io::Error::other("log writer panicked")))
                .map_err(|e| FileError::from_io(&e, Some(path)))
        }
        None => Err(FileError::new(FileErrorKind::NotFound, Some(path))),
    };
    Message::Analysis(crate::AnalysisMessage::LogSaved { run, result })
}

async fn probe(config: EngineConfig) -> Result<EngineIdentity, EngineError> {
    match config.protocol {
        Protocol::XBoard => probe_xboard(config).await,
        _ => probe_uci(config).await,
    }
}

pub(crate) async fn test(
    request: RequestId,
    draft: &EngineDraft,
    defaults: &RunDefaults,
) -> Message {
    let result = probe(draft.to_config(defaults))
        .await
        .map_err(|error| EngineFailure::from_error(&error));
    engine(EngineMessage::Tested {
        request,
        result: Box::new(result),
    })
}

pub(crate) async fn detect(
    request: RequestId,
    draft: &EngineDraft,
    defaults: &RunDefaults,
) -> Message {
    let result = detect_protocol(draft.to_config(defaults))
        .await
        .map_err(|error| EngineFailure::from_error(&error));
    engine(EngineMessage::Detected { request, result })
}

pub(crate) async fn check(
    request: RequestId,
    command: PathBuf,
    working_dir: Option<PathBuf>,
) -> Message {
    let warnings = tokio::task::spawn_blocking(move || {
        check_command(
            &command,
            working_dir.as_deref(),
            &path_dirs_from_env(),
            &FsProbe,
        )
    })
    .await
    .unwrap_or_default();
    engine(EngineMessage::Checked { request, warnings })
}

pub(crate) async fn find(request: RequestId) -> Message {
    let engines = tokio::task::spawn_blocking(|| {
        let mut dirs = path_dirs_from_env();
        dirs.extend(usual_engine_dirs());
        find_installed_engines(&dirs)
    })
    .await
    .unwrap_or_default();
    engine(EngineMessage::Found { request, engines })
}

/// Starts a found executable as UCI and, if that fails, as XBoard; the name
/// the engine gives itself names the entry.
pub(crate) async fn probe_found(
    request: RequestId,
    path: PathBuf,
    defaults: &RunDefaults,
) -> Message {
    let mut draft = EngineDraft::new("", path.clone());
    draft.low_priority = Some(false);
    let mut config = draft.to_config(defaults);
    config.timeouts.uciok = FOUND_UCI_WAIT;
    let result = match probe_uci(config.clone()).await {
        Ok(identity) => Ok((Protocol::Uci, identity.name)),
        Err(error @ EngineError::Spawn { .. }) => Err(EngineFailure::from_error(&error)),
        Err(_) => {
            config.protocol = Protocol::XBoard;
            match probe_xboard(config).await {
                Ok(identity) => Ok((Protocol::XBoard, identity.name)),
                Err(error) => Err(EngineFailure::from_error(&error)),
            }
        }
    };
    engine(EngineMessage::FoundProbed {
        request,
        path,
        result,
    })
}

/// Reads and parses an `engines.dat`; never evaluates it.
pub(crate) async fn read_scid(request: RequestId, path: PathBuf) -> Message {
    let fallback = path.clone();
    let result = tokio::task::spawn_blocking(move || read_scid_file(&path))
        .await
        .unwrap_or_else(|_| {
            Err(FileError::new(
                FileErrorKind::Io(std::io::ErrorKind::Other),
                Some(fallback),
            ))
        });
    engine(EngineMessage::ScidRead {
        request,
        result: Box::new(result),
    })
}

fn read_scid_file(path: &std::path::Path) -> Result<super::scid::ScidImport, FileError> {
    use std::io::Read;
    let fail = |e: &std::io::Error| FileError::from_io(e, Some(path.to_path_buf()));
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| fail(&e))?
        .take(MAX_SCID_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| fail(&e))?;
    // The Scid writes the file in the system encoding; what is not UTF-8
    // becomes replacement characters and the parser treats it as text.
    let text = String::from_utf8_lossy(&bytes);
    Ok(parse_scid_engines_dat(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_missing_program_fails_the_test_with_cannot_start() {
        let draft = EngineDraft::new("x", "/nonexistent/mychess-no-such-engine");
        let message = test(RequestId(1), &draft, &RunDefaults::default()).await;
        let Message::Engine(EngineMessage::Tested { request, result }) = message else {
            panic!("expected a test answer");
        };
        assert_eq!(request, RequestId(1));
        assert_eq!(
            result.expect_err("must fail").kind,
            super::super::run::FailureKind::CannotStart
        );
    }

    #[tokio::test]
    async fn reading_a_missing_dat_reports_a_file_error_and_a_real_one_parses() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = read_scid(RequestId(2), dir.path().join("none.dat")).await;
        let Message::Engine(EngineMessage::ScidRead { result, .. }) = missing else {
            panic!("expected an answer");
        };
        assert!(result.is_err());
        let path = dir.path().join("engines.dat");
        std::fs::write(
            &path,
            "engine {\n  Name Crafty\n  Cmd crafty\n  Dir .\n  UCI 0\n}\n",
        )
        .expect("write");
        let Message::Engine(EngineMessage::ScidRead { result, .. }) =
            read_scid(RequestId(3), path).await
        else {
            panic!("expected an answer");
        };
        assert_eq!(result.expect("parsed").engines.len(), 1);
    }

    /// The `mychess-fake-engine` binary next to the test executable, when
    /// it was built (`cargo build -p mychess-engine --features testing`;
    /// `cargo test --workspace` builds it too). With
    /// `MYCHESS_REQUIRE_FAKE_ENGINE` set (`cargo xtask test` does) a missing
    /// binary fails the test instead of skipping it.
    fn fake_binary() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let name = format!("mychess-fake-engine{}", std::env::consts::EXE_SUFFIX);
        let path = exe.parent()?.parent()?.join(name);
        let found = path.is_file().then_some(path);
        assert!(
            found.is_some() || std::env::var_os("MYCHESS_REQUIRE_FAKE_ENGINE").is_none(),
            "mychess-fake-engine is not built"
        );
        found
    }

    #[tokio::test]
    async fn a_real_process_starts_answers_the_probe_and_ends_on_shutdown() {
        let Some(binary) = fake_binary() else {
            eprintln!("mychess-fake-engine not built: skipping");
            return;
        };
        let defaults = RunDefaults::default();
        // The probe (UCI): who it is and how many options.
        let draft = EngineDraft::new("Fake", binary.clone());
        let Message::Engine(EngineMessage::Tested { result, .. }) =
            test(RequestId(1), &draft, &defaults).await
        else {
            panic!("expected a test answer");
        };
        let identity = (*result).expect("the fake answers the probe");
        assert!(!identity.options.is_empty());
        // The detection finds UCI.
        let Message::Engine(EngineMessage::Detected { result, .. }) =
            detect(RequestId(2), &draft, &defaults).await
        else {
            panic!("expected a detection");
        };
        assert_eq!(result, Ok(Protocol::Uci));
        // A run: started through the default launcher, then ended.
        let hub = EngineHub::new();
        let mut list = crate::engines::EngineList::new();
        let id = list.add(draft).expect("add");
        let entry = list.get(id).expect("entry").clone();
        let started = start(&hub, RunId(1), &entry, &defaults).await;
        assert!(matches!(
            started,
            Message::Engine(EngineMessage::Started { .. })
        ));
        assert_eq!(hub.alive(), 1);
        shutdown(&hub).await;
        assert_eq!(hub.alive(), 0);
    }
}
