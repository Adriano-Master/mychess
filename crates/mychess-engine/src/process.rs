//! Launching an engine process and building its command line.
//!
//! The command is described by a plain [`CommandPlan`] first, so the
//! platform-specific parts (no console window on Windows, `nice` on Unix, the
//! working directory) can be tested without starting anything.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::engine::{EngineConfig, EngineError};
use crate::log::{LogHandle, LogKind};
use crate::wire::Wire;

/// Windows: do not open a console window for the engine.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Windows: run the process at idle priority.
pub const IDLE_PRIORITY_CLASS: u32 = 0x0000_0040;
/// The `nice` increment used for low-priority engines on Unix.
pub const NICE_INCREMENT: &str = "15";

/// How an engine process will be launched. Built by [`build_command`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPlan {
    /// The program to run (the engine itself, or `nice`).
    pub program: PathBuf,
    /// The arguments, one per element; never joined into a shell string.
    pub args: Vec<OsString>,
    /// The working directory, `None` to inherit.
    pub current_dir: Option<PathBuf>,
    /// Windows process creation flags (always 0 elsewhere).
    pub creation_flags: u32,
    /// A note for the protocol log (for example, `nice` was not found).
    pub note: Option<String>,
}

/// Plans the launch of `config`, looking for `nice` in the `PATH` when the
/// engine should run at low priority.
pub fn build_command(config: &EngineConfig) -> CommandPlan {
    let nice = if config.low_priority {
        platform::find_nice()
    } else {
        None
    };
    build_command_with(config, nice.as_deref())
}

/// Like [`build_command`], with the location of `nice` given.
///
/// - `program` and `args` stay separate (no shell);
/// - the working directory is `working_dir` or, if absent, the directory of
///   the executable (`multiplataforma.md`);
/// - a program path with directories is made absolute, because the child's
///   working directory differs from ours;
/// - Windows: `CREATE_NO_WINDOW`, plus `IDLE_PRIORITY_CLASS` for low priority;
/// - Unix with low priority: `nice -n 15 <program> <args...>` when `nice`
///   exists (it execs, so the pid stays the engine's), else a note.
pub fn build_command_with(config: &EngineConfig, nice: Option<&Path>) -> CommandPlan {
    let has_dir = config.command.components().count() > 1;
    let program = if has_dir {
        std::path::absolute(&config.command).unwrap_or_else(|_| config.command.clone())
    } else {
        config.command.clone()
    };
    let current_dir = config.working_dir.clone().or_else(|| {
        program
            .parent()
            .filter(|p| has_dir && !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
    });
    let mut plan = CommandPlan {
        program,
        args: config.args.iter().map(OsString::from).collect(),
        current_dir,
        creation_flags: 0,
        note: None,
    };
    if cfg!(windows) {
        plan.creation_flags = CREATE_NO_WINDOW;
    }
    if config.low_priority {
        if cfg!(windows) {
            plan.creation_flags |= IDLE_PRIORITY_CLASS;
        } else if let Some(nice) = nice {
            let engine = std::mem::replace(&mut plan.program, nice.to_path_buf());
            let mut args = vec![
                OsString::from("-n"),
                OsString::from(NICE_INCREMENT),
                engine.into_os_string(),
            ];
            args.append(&mut plan.args);
            plan.args = args;
        } else {
            plan.note = Some("low priority requested but `nice` was not found".to_owned());
        }
    }
    plan
}

impl CommandPlan {
    fn to_command(&self) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(&self.program);
        command.args(&self.args);
        if let Some(dir) = &self.current_dir {
            command.current_dir(dir);
        }
        platform::apply_flags(&mut command, self.creation_flags);
        command
    }
}

/// Starts the engine and connects its pipes. `stderr` is drained into the log.
pub(crate) fn spawn(config: &EngineConfig, log: &LogHandle) -> Result<Wire, EngineError> {
    let plan = build_command(config);
    if let Some(note) = &plan.note {
        log.push(LogKind::Note, note);
    }
    let mut command = plan.to_command();
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let spawn_error = |source| EngineError::Spawn {
        path: config.command.clone(),
        source,
    };
    let mut child = command.spawn().map_err(spawn_error)?;
    let missing = |what: &str| {
        spawn_error(std::io::Error::other(format!(
            "engine {what} is not a pipe"
        )))
    };
    let stdin = child.stdin.take().ok_or_else(|| missing("stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| missing("stdout"))?;
    let stderr = child.stderr.take().ok_or_else(|| missing("stderr"))?;
    let mut wire = Wire::from_io(stdout, stdin);
    wire.attach_child(child, stderr, log.clone());
    Ok(wire)
}

mod platform {
    //! The only platform-specific code of the crate.

    use std::path::PathBuf;

    /// Looks for `nice` in the `PATH`.
    #[cfg(unix)]
    pub(super) fn find_nice() -> Option<PathBuf> {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|dir| dir.join("nice"))
            .find(|candidate| {
                std::fs::metadata(candidate)
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
    }

    /// Windows has no `nice`; the priority class is a creation flag instead.
    #[cfg(not(unix))]
    pub(super) fn find_nice() -> Option<PathBuf> {
        None
    }

    #[cfg(windows)]
    pub(super) fn apply_flags(command: &mut tokio::process::Command, flags: u32) {
        command.creation_flags(flags);
    }

    #[cfg(not(windows))]
    pub(super) fn apply_flags(_command: &mut tokio::process::Command, _flags: u32) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(command: &str) -> EngineConfig {
        let mut c = EngineConfig::new(command);
        c.args = vec!["--threads".into(), "2 with space".into()];
        c
    }

    #[test]
    fn program_and_args_stay_separate_and_dir_defaults_to_the_executable_dir() {
        let exe = std::env::temp_dir().join("engines").join("sf");
        let plan = build_command_with(&config(exe.to_str().unwrap()), None);
        assert_eq!(plan.program, exe);
        assert_eq!(
            plan.args,
            [OsString::from("--threads"), OsString::from("2 with space")]
        );
        assert_eq!(plan.current_dir.as_deref(), exe.parent());
    }

    #[test]
    fn bare_program_name_inherits_the_directory_and_explicit_dir_wins() {
        let plan = build_command_with(&config("stockfish"), None);
        assert_eq!(plan.current_dir, None);
        let mut c = config("stockfish");
        c.working_dir = Some(PathBuf::from("work"));
        assert_eq!(
            build_command_with(&c, None).current_dir,
            Some(PathBuf::from("work"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn low_priority_wraps_the_engine_in_nice() {
        let mut c = config("stockfish");
        c.low_priority = true;
        let plan = build_command_with(&c, Some(Path::new("/usr/bin/nice")));
        assert_eq!(plan.program, Path::new("/usr/bin/nice"));
        assert_eq!(
            plan.args[..3],
            [
                OsString::from("-n"),
                OsString::from("15"),
                OsString::from("stockfish")
            ]
        );
        assert_eq!(plan.args.len(), 5);
        assert_eq!(plan.creation_flags, 0);
        let without = build_command_with(&c, None);
        assert_eq!(without.program, Path::new("stockfish"));
        assert!(without.note.is_some());
    }

    #[cfg(windows)]
    #[test]
    fn windows_flags_hide_the_console_and_lower_the_priority() {
        let plan = build_command_with(&config("stockfish.exe"), None);
        assert_eq!(plan.creation_flags, CREATE_NO_WINDOW);
        let mut c = config("stockfish.exe");
        c.low_priority = true;
        let plan = build_command_with(&c, None);
        assert_eq!(plan.creation_flags, CREATE_NO_WINDOW | IDLE_PRIORITY_CLASS);
    }
}
