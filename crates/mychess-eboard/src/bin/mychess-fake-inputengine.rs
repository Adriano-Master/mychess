//! A fake input engine (DGT driver) on standard input and output, for the
//! process tests of `mychess-eboard`. See `mychess_eboard::testing`.

use std::io::Write;
use std::process::ExitCode;

use mychess_eboard::testing::{FakeDriver, run_fake_input_engine};

fn main() -> ExitCode {
    let fake = match FakeDriver::from_args(std::env::args().skip(1)) {
        Ok(fake) => fake,
        Err(message) => {
            writeln!(std::io::stderr(), "mychess-fake-inputengine: {message}").ok();
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            writeln!(std::io::stderr(), "mychess-fake-inputengine: {e}").ok();
            return ExitCode::from(3);
        }
    };
    runtime.block_on(run_fake_input_engine(
        tokio::io::stdin(),
        tokio::io::stdout(),
        fake,
    ));
    ExitCode::SUCCESS
}
