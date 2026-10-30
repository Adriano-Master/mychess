//! The `mychess-cli` binary: wires the real streams, Ctrl-C and `tracing`
//! to [`mychess_cli::run`].

use std::io::{self, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use mychess_cli::{EXIT_SIGNALED, Io, run};
use mychess_db::CancelToken;
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    let filter = EnvFilter::try_from_env("MYCHESS_LOG").unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .init();

    let cancel = CancelToken::new();
    let handler_token = cancel.clone();
    // First signal (Ctrl-C, SIGTERM, SIGHUP): ask the running command to stop.
    // Second signal: leave at once with 130. `sort` and `compact` are atomic
    // through the journal, so ending the process at any moment is safe (D629).
    let signals = Arc::new(AtomicU32::new(0));
    let handler = move || {
        if signals.fetch_add(1, Ordering::SeqCst) == 0 {
            handler_token.cancel();
        } else {
            std::process::exit(EXIT_SIGNALED);
        }
    };
    if let Err(error) = ctrlc::set_handler(handler) {
        tracing::warn!(%error, "cannot install the signal handler; Ctrl-C will end the process at once");
    }

    let stdout = io::stdout();
    let stderr = io::stderr();
    let stdin = io::stdin();
    let (mut out, mut err, mut input) = (stdout.lock(), stderr.lock(), stdin.lock());
    let code = run(
        std::env::args_os(),
        Io {
            out: &mut out,
            err: &mut err,
            input: &mut input,
            cancel: &cancel,
        },
    );
    out.flush().ok();
    ExitCode::from(code)
}
