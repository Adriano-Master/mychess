//! Command-line tools for MyChess databases (milestone M3,
//! `docs/specs/m3-5-mychess-cli.md`).
//!
//! One binary, `mychess-cli`, replaces three programs of the Scid vs PC:
//!
//! | Subcommand | Scid |
//! |---|---|
//! | `import` | `pgnscid`: PGN into an SI4 database |
//! | `merge` | `scmerge`: several databases into a new one |
//! | `info`, `names`, `describe`, `sort`, `compact` | `scidt -i/-c/-n`, `-p/-e/-s/-r`, `-d`, `-S/-R`, `-C/-N` |
//! | `export` | the games of a database as PGN |
//!
//! The work is done by `mychess-db`; this crate parses the arguments, shows
//! progress and turns results into text and exit codes. [`run`] takes the
//! standard streams and the [`CancelToken`] as parameters, so everything can
//! be tested without a process; `main.rs` wires the real ones and Ctrl-C.
//!
//! Exit codes: `0` success, `1` failure (including a command stopped by the
//! first signal), `2` usage error, `130` second signal (see [`EXIT_FAILURE`],
//! [`EXIT_USAGE`] and [`EXIT_SIGNALED`]).
//!
//! Dependency rule: `mychess-core`, `mychess-pgn` and `mychess-db` only.

mod args;
mod compact;
mod export;
mod import;
mod info;
mod merge;
mod paths;
mod progress;
mod sort;

use std::ffi::OsString;
use std::io::{BufRead, Write};

use clap::Parser;
use mychess_db::CancelToken;

pub use args::{Cli, Command};

/// Exit code of a failed command.
pub const EXIT_FAILURE: u8 = 1;
/// Exit code of a usage error.
pub const EXIT_USAGE: u8 = 2;
/// Exit code of the process when a second signal arrives (128 + SIGINT); used
/// by `main`, because a command only ever sees the first signal.
pub const EXIT_SIGNALED: i32 = 130;

/// The streams and the cancellation flag of one run.
pub struct Io<'a> {
    /// Results (standard output).
    pub out: &'a mut dyn Write,
    /// Progress, warnings and errors (standard error).
    pub err: &'a mut dyn Write,
    /// Standard input, read by `merge ... -`.
    pub input: &'a mut dyn BufRead,
    /// Asks the running command to stop.
    pub cancel: &'a CancelToken,
}

/// What a command needs while it runs.
pub(crate) struct Ctx<'a> {
    pub(crate) out: &'a mut dyn Write,
    pub(crate) err: &'a mut dyn Write,
    pub(crate) input: &'a mut dyn BufRead,
    pub(crate) cancel: &'a CancelToken,
    pub(crate) quiet: bool,
}

/// Runs the tool with `args` (the first one is the program name) and returns
/// the exit code.
pub fn run<I, T>(args: I, io: Io<'_>) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let Io {
        out,
        err,
        input,
        cancel,
    } = io;
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            let text = error.render().to_string();
            let code = if error.use_stderr() {
                writeln!(err, "{}", text.trim_end()).ok();
                EXIT_USAGE
            } else {
                writeln!(out, "{}", text.trim_end()).ok();
                0
            };
            return code;
        }
    };
    let mut ctx = Ctx {
        out,
        err,
        input,
        cancel,
        quiet: cli.quiet,
    };
    let result = match &cli.command {
        Command::Import(a) => import::run(&mut ctx, a),
        Command::Merge(a) => merge::run(&mut ctx, a),
        Command::Info(a) => info::run_info(&mut ctx, a),
        Command::Names(a) => info::run_names(&mut ctx, a),
        Command::Describe(a) => compact::run_describe(&mut ctx, a),
        Command::Sort(a) => sort::run(&mut ctx, a),
        Command::Compact(a) => compact::run(&mut ctx, a),
        Command::Export(a) => export::run(&mut ctx, a),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            tracing::debug!(?error, "command failed");
            writeln!(ctx.err, "error: {error:#}").ok();
            EXIT_FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn run_args(args: &[&str]) -> (u8, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut input: &[u8] = b"";
        let cancel = CancelToken::new();
        let code = run(
            args,
            Io {
                out: &mut out,
                err: &mut err,
                input: &mut input,
                cancel: &cancel,
            },
        );
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    #[test]
    fn help_exits_zero_on_stdout() {
        let (code, out, _) = run_args(&["mychess-cli", "--help"]);
        assert_eq!(code, 0);
        assert!(out.contains("import") && out.contains("merge"), "{out}");
    }

    #[test]
    fn unknown_subcommand_is_a_usage_error() {
        let (code, out, err) = run_args(&["mychess-cli", "frobnicate"]);
        assert_eq!(code, EXIT_USAGE);
        assert_eq!(out.len(), 0);
        assert!(err.contains("frobnicate"), "{err}");
    }

    #[test]
    fn missing_arguments_are_a_usage_error() {
        let (code, _, _) = run_args(&["mychess-cli", "import"]);
        assert_eq!(code, EXIT_USAGE);
        let (code, _, _) = run_args(&["mychess-cli"]);
        assert_eq!(code, EXIT_USAGE);
    }

    #[test]
    fn invalid_sort_field_is_a_usage_error() {
        let (code, _, err) = run_args(&["mychess-cli", "sort", "nope", "somewhere"]);
        assert_eq!(code, EXIT_USAGE);
        assert!(err.contains("invalid sort field"), "{err}");
    }

    #[test]
    fn cancelled_import_keeps_the_database_and_fails() {
        let dir = tempfile::tempdir().unwrap();
        let pgn = dir.path().join("a.pgn");
        std::fs::write(
            &pgn,
            "[White \"A\"]\n[Black \"B\"]\n[Result \"*\"]\n\n1. e4 *\n",
        )
        .unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut input: &[u8] = b"";
        let cancel = CancelToken::new();
        cancel.cancel();
        let code = run(
            [
                OsStr::new("mychess-cli"),
                OsStr::new("-q"),
                OsStr::new("import"),
                pgn.as_os_str(),
            ],
            Io {
                out: &mut out,
                err: &mut err,
                input: &mut input,
                cancel: &cancel,
            },
        );
        assert_eq!(code, EXIT_FAILURE);
        assert!(String::from_utf8_lossy(&err).contains("interrupted"));
        assert!(dir.path().join("a.si4").is_file());
    }
}
