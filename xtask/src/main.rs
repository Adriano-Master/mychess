//! Development task runner for the MyChess workspace (the `cargo xtask` pattern).
//!
//! A portable replacement for shell scripts and Makefiles: the same quality
//! gate runs on Linux, Windows and macOS. Run `cargo xtask help` for the list
//! of tasks.

// A command-line tool reports progress on the terminal by design.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

type Result<T = ()> = std::result::Result<T, String>;

const HELP: &str = "\
Usage: cargo xtask <task>

Tasks:
  ci       Full quality gate: fmt-check, lint, arch, test, doc, deny
  fmt      Format the whole workspace
  lint     Clippy on all targets and features, warnings as errors
  arch     Check crate dependencies against the layering rules
  test     Unit, integration and doc tests (uses cargo-nextest when installed)
  doc      Build the documentation, warnings as errors
  deny     Licenses, security advisories, banned crates (needs cargo-deny)
  compat   Compatibility tests against reference/scid_vs_pc-4.27
  help     Show this message
";

/// Internal (`mychess-*`) dependencies each crate may have, as normal or build
/// dependencies. Dev-dependencies are not restricted.
///
/// This table is the executable form of the layering rules in
/// `docs/arquitetura.md`; keep both in sync. The check fails both for a
/// workspace crate missing from the table and for a table entry that matches
/// no crate, so the two cannot drift apart silently.
const ALLOWED_DEPS: &[(&str, &[&str])] = &[
    ("xtask", &[]),
    ("mychess-core", &[]),
    ("mychess-pgn", &["mychess-core"]),
    ("mychess-db", &["mychess-core", "mychess-pgn"]),
    ("mychess-engine", &["mychess-core"]),
    ("mychess-book", &["mychess-core"]),
    ("mychess-online", &["mychess-core"]),
    ("mychess-tablebase", &["mychess-core", "mychess-online"]),
    ("mychess-eboard", &["mychess-core"]),
    (
        "mychess-app",
        &[
            "mychess-core",
            "mychess-pgn",
            "mychess-db",
            "mychess-engine",
            "mychess-book",
            "mychess-online",
            "mychess-tablebase",
            "mychess-eboard",
        ],
    ),
    ("mychess-gui", &["mychess-app", "mychess-core"]),
    (
        "mychess-cli",
        &["mychess-core", "mychess-pgn", "mychess-db"],
    ),
];

/// Location of the read-only Scid vs PC sources, relative to the workspace root.
const SCID_REFERENCE: &str = "reference/scid_vs_pc-4.27";

fn main() -> ExitCode {
    let task = env::args().nth(1).unwrap_or_else(|| "help".to_owned());
    let result = Workspace::locate().and_then(|ws| match task.as_str() {
        "ci" => ci(&ws),
        "fmt" => ws.cargo(&["fmt", "--all"]),
        "lint" => lint(&ws),
        "arch" => arch(&ws),
        "test" => test(&ws),
        "doc" => doc(&ws),
        "deny" => deny(&ws),
        "compat" => compat(&ws),
        "help" | "-h" | "--help" => {
            print!("{HELP}");
            Ok(())
        }
        other => Err(format!("unknown task `{other}`\n\n{HELP}")),
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

type Step = fn(&Workspace) -> Result;

/// Runs every check, even after a failure, and reports all failing steps.
fn ci(ws: &Workspace) -> Result {
    let steps: [(&str, Step); 6] = [
        ("fmt", |ws| ws.cargo(&["fmt", "--all", "--check"])),
        ("lint", lint),
        ("arch", arch),
        ("test", test),
        ("doc", doc),
        ("deny", deny_if_installed),
    ];
    let mut failed = Vec::new();
    for (name, step) in steps {
        if let Err(message) = step(ws) {
            eprintln!("xtask: {message}");
            failed.push(name);
        }
    }
    if failed.is_empty() {
        println!("\nxtask ci: all checks passed");
        Ok(())
    } else {
        Err(format!("ci failed in: {}", failed.join(", ")))
    }
}

fn lint(ws: &Workspace) -> Result {
    ws.cargo(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--all-features",
        "--",
        "-D",
        "warnings",
    ])
}

fn test(ws: &Workspace) -> Result {
    // The tests with a real engine process need the fake engine binary; with
    // the variable set they fail instead of skipping when it is missing.
    ws.cargo(&[
        "build",
        "-p",
        "mychess-engine",
        "--features",
        "testing",
        "--bin",
        "mychess-fake-engine",
    ])?;
    let with_fake = |args: &[&str]| {
        let mut cmd = ws.cargo_command(args);
        cmd.env("MYCHESS_REQUIRE_FAKE_ENGINE", "1");
        run(cmd)
    };
    if ws.has_cargo_subcommand("nextest") {
        with_fake(&["nextest", "run", "--workspace", "--all-features"])?;
        // nextest does not run doctests.
        with_fake(&["test", "--workspace", "--all-features", "--doc"])
    } else {
        println!(
            "(cargo-nextest not found, using cargo test: cargo install --locked cargo-nextest)"
        );
        with_fake(&["test", "--workspace", "--all-features"])
    }
}

fn doc(ws: &Workspace) -> Result {
    let mut cmd = ws.cargo_command(&[
        "doc",
        "--workspace",
        "--no-deps",
        "--document-private-items",
    ]);
    // Keep any flags the user already set.
    let flags = env::var("RUSTDOCFLAGS").map_or_else(
        |_| "-D warnings".to_owned(),
        |flags| format!("{flags} -D warnings"),
    );
    cmd.env("RUSTDOCFLAGS", flags);
    run(cmd)
}

fn deny(ws: &Workspace) -> Result {
    if ws.has_cargo_subcommand("deny") {
        ws.cargo(&["deny", "check"])
    } else {
        Err("cargo-deny is not installed: cargo install --locked cargo-deny".to_owned())
    }
}

/// `ci` keeps working on machines without cargo-deny; the CI server always has it.
fn deny_if_installed(ws: &Workspace) -> Result {
    if ws.has_cargo_subcommand("deny") {
        deny(ws)
    } else {
        println!("\n(cargo-deny not installed: skipping license and advisory checks)");
        Ok(())
    }
}

/// Compatibility tests read files from the Scid vs PC reference tree, which is
/// not versioned; they are `#[ignore]`d by default and named `compat_*`.
fn compat(ws: &Workspace) -> Result {
    let reference = ws.root.join(SCID_REFERENCE);
    if !reference.is_dir() {
        return Err(format!(
            "{} not found; see docs/referencia-scid.md",
            reference.display()
        ));
    }
    if !has_compat_tests(&ws.root.join("crates")) {
        println!("\n(no `compat_*` tests exist yet: nothing to run)");
        return Ok(());
    }
    ws.cargo(&[
        "test",
        "--workspace",
        "--all-features",
        "--",
        "--ignored",
        "compat_",
    ])
}

/// Whether any Rust file under `dir` defines a `compat_*` test function.
fn has_compat_tests(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            has_compat_tests(&path)
        } else {
            path.extension().is_some_and(|ext| ext == "rs")
                && std::fs::read_to_string(&path).is_ok_and(|src| src.contains("fn compat_"))
        }
    })
}

/// Checks every `mychess-*` edge of the dependency graph against [`ALLOWED_DEPS`].
///
/// Optional and platform-specific dependencies count too (`--all-features`,
/// `--target all`), so a forbidden edge cannot hide behind a feature or a
/// `cfg(windows)` section when the check runs on Linux. `--no-dedupe` is
/// essential: without it, a crate already shown as a dependency of another one
/// is printed later as a root marked `(*)` *without its own dependencies*,
/// and those edges would go unchecked.
fn arch(ws: &Workspace) -> Result {
    println!("\n$ cargo tree (architecture check)");
    let output = ws
        .cargo_command(&[
            "tree",
            "--workspace",
            "--no-dedupe",
            "--all-features",
            "--target",
            "all",
            "--edges",
            "normal,build",
            "--depth",
            "1",
            "--prefix",
            "depth",
            "--format",
            "{p}",
        ])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("could not run cargo tree: {e}"))?;
    if !output.status.success() {
        return Err("cargo tree failed".to_owned());
    }

    let tree = String::from_utf8_lossy(&output.stdout);
    let mut violations = Vec::new();
    let mut workspace_crates = Vec::new();
    let mut current: Option<(&str, &[&str])> = None;
    for line in tree.lines() {
        let Some((depth, name)) = parse_tree_line(line) else {
            continue;
        };
        match depth {
            0 => {
                workspace_crates.push(name);
                current = ALLOWED_DEPS
                    .iter()
                    .find(|(krate, _)| *krate == name)
                    .copied();
                if current.is_none() {
                    violations.push(format!(
                        "`{name}` has no layering rule: add it to ALLOWED_DEPS in \
                         xtask/src/main.rs and to docs/arquitetura.md"
                    ));
                }
            }
            1 if name.starts_with("mychess-") => {
                if let Some((krate, allowed)) = current
                    && !allowed.contains(&name)
                {
                    violations.push(format!("`{krate}` must not depend on `{name}`"));
                }
            }
            _ => {}
        }
    }

    if workspace_crates.is_empty() {
        return Err("cargo tree listed no workspace crates: nothing was checked".to_owned());
    }
    for (krate, _) in ALLOWED_DEPS {
        if !workspace_crates.contains(krate) {
            violations.push(format!(
                "`{krate}` is in ALLOWED_DEPS but is not a workspace crate: remove the stale entry"
            ));
        }
    }

    if violations.is_empty() {
        println!("architecture: all crate dependencies follow the layering rules");
        Ok(())
    } else {
        Err(format!(
            "architecture violations (see docs/arquitetura.md):\n  - {}",
            violations.join("\n  - ")
        ))
    }
}

/// Parses a `cargo tree --prefix depth --format {p}` line such as
/// `1mychess-core v0.1.0 (/path)` into its depth and package name.
fn parse_tree_line(line: &str) -> Option<(usize, &str)> {
    let digits = line.find(|c: char| !c.is_ascii_digit())?;
    let depth = line[..digits].parse().ok()?;
    let name = line[digits..].split_whitespace().next()?;
    Some((depth, name))
}

struct Workspace {
    cargo: OsString,
    root: PathBuf,
}

impl Workspace {
    fn locate() -> Result<Self> {
        // Cargo sets `CARGO` for processes started with `cargo run`.
        let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or("xtask must live in a subdirectory of the workspace root")?
            .to_path_buf();
        Ok(Self { cargo, root })
    }

    fn cargo_command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.cargo);
        cmd.current_dir(&self.root).args(args);
        cmd
    }

    fn cargo(&self, args: &[&str]) -> Result {
        run(self.cargo_command(args))
    }

    fn has_cargo_subcommand(&self, name: &str) -> bool {
        self.cargo_command(&[name, "--version"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
}

fn run(mut cmd: Command) -> Result {
    let shown = describe(&cmd);
    println!("\n$ {shown}");
    let status = cmd
        .status()
        .map_err(|e| format!("could not start `{shown}`: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{shown}` failed ({status})"))
    }
}

/// Renders a command as `cargo clippy --workspace ...` for progress output.
fn describe(cmd: &Command) -> String {
    let program = Path::new(cmd.get_program()).file_stem().map_or_else(
        || cmd.get_program().to_string_lossy(),
        |s| s.to_string_lossy(),
    );
    let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy()).collect();
    format!("{program} {}", args.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tree_lines() {
        assert_eq!(
            parse_tree_line("0mychess-app v0.1.0 (/ws/crates/mychess-app)"),
            Some((0, "mychess-app"))
        );
        assert_eq!(
            parse_tree_line("1mychess-core v0.1.0 (/ws/crates/mychess-core)"),
            Some((1, "mychess-core"))
        );
        assert_eq!(parse_tree_line(""), None);
        assert_eq!(parse_tree_line("mychess-core v0.1.0"), None);
    }

    #[test]
    fn every_allowed_dependency_is_a_known_crate() {
        for (krate, allowed) in ALLOWED_DEPS {
            for dep in *allowed {
                assert!(
                    ALLOWED_DEPS.iter().any(|(name, _)| name == dep),
                    "{krate} allows unknown crate {dep}"
                );
                assert_ne!(krate, dep, "{krate} must not depend on itself");
            }
        }
    }
}
