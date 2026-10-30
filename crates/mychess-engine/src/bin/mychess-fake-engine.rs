//! A fake UCI engine on standard input and output, for tests of the process
//! handling of `mychess-engine`. See `mychess_engine::testing::fake` for the
//! behavior and the flags.

use std::io::Write;
use std::process::ExitCode;

use mychess_engine::testing::{FakeBehavior, run_fake};

fn main() -> ExitCode {
    let (behavior, flood) = match FakeBehavior::from_args(std::env::args().skip(1)) {
        Ok(parsed) => parsed,
        Err(message) => {
            writeln!(std::io::stderr(), "mychess-fake-engine: {message}").ok();
            return ExitCode::from(2);
        }
    };
    if flood {
        // Far more than any pipe buffer: the parent must drain stderr.
        std::thread::spawn(|| {
            let line = "stderr flood ".repeat(8);
            let mut err = std::io::stderr().lock();
            for _ in 0..50_000 {
                if writeln!(err, "{line}").is_err() {
                    break;
                }
            }
        });
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            writeln!(std::io::stderr(), "mychess-fake-engine: {e}").ok();
            return ExitCode::from(3);
        }
    };
    let exit = runtime.block_on(run_fake(
        tokio::io::stdin(),
        tokio::io::stdout(),
        behavior,
        None,
    ));
    if let Some(message) = exit.stderr {
        writeln!(std::io::stderr(), "{message}").ok();
    }
    ExitCode::from(u8::try_from(exit.code).unwrap_or(1))
}
