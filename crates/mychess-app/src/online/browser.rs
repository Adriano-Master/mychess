//! Opens a web page in the default browser (spec M8-3, D3009). The only
//! platform-specific code of the online play; every platform has a command
//! that takes the address as a separate argument (never through a shell).

use std::process::{Command, Stdio};

/// Whether `url` is something this program may hand to the system: an
/// `https` address, or `http` on the loopback interface (the tests and a
/// local Lichess), with no spaces or control characters.
fn is_openable(url: &str) -> bool {
    let plain = url.strip_prefix("http://").is_some_and(|rest| {
        rest.starts_with("127.0.0.1:")
            || rest.starts_with("localhost:")
            || rest.starts_with("[::1]:")
    });
    (url.starts_with("https://") || plain) && !url.chars().any(|c| c.is_control() || c == ' ')
}

#[cfg(target_os = "linux")]
fn command(url: &str) -> Command {
    let mut c = Command::new("xdg-open");
    c.arg(url);
    c
}

#[cfg(target_os = "macos")]
fn command(url: &str) -> Command {
    let mut c = Command::new("open");
    c.arg(url);
    c
}

#[cfg(windows)]
fn command(url: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut c = Command::new("rundll32");
    c.args(["url.dll,FileProtocolHandler", url]);
    // No console window (rule multiplataforma).
    c.creation_flags(0x0800_0000);
    c
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn command(url: &str) -> Command {
    let mut c = Command::new("xdg-open");
    c.arg(url);
    c
}

fn spawn_reaped(mut command: Command) -> bool {
    let spawned = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match spawned {
        Ok(mut child) => {
            // Reap the helper without blocking the caller.
            std::thread::spawn(move || {
                child.wait().ok();
            });
            true
        }
        Err(_) => false,
    }
}

/// Asks the system to open `url`; `false` when it refuses the address or the
/// opener cannot be started (the caller then shows the address).
pub(crate) fn open_url(url: &str) -> bool {
    is_openable(url) && spawn_reaped(command(url))
}

/// Whether `path` is something this program may hand to the system: an
/// absolute path of an existing regular `.html`/`.htm` file (any case; the
/// opener of Windows would run an `.exe`), with a name the opener cannot
/// take for an option.
fn is_openable_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"))
        && path.is_absolute()
        && path.is_file()
        && path
            .to_str()
            .is_some_and(|p| !p.chars().any(char::is_control))
}

/// Asks the system to open the HTML file `path` with its default program
/// (the opening report); `false` when the path is refused or the
/// opener cannot be started. The path is a separate argument, never part
/// of a shell string.
pub(crate) fn open_path(path: &std::path::Path) -> bool {
    match path.to_str() {
        Some(text) if is_openable_path(path) => spawn_reaped(command(text)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{is_openable, is_openable_path};

    #[test]
    fn only_https_and_loopback_http_are_opened() {
        for ok in [
            "https://lichess.org/oauth?a=b",
            "http://127.0.0.1:8080/x",
            "http://localhost:9/",
        ] {
            assert!(is_openable(ok), "{ok}");
        }
        for bad in [
            "http://lichess.org/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://lichess.org/a b",
            "https://lichess.org/\nrm",
            "-oProxyCommand=x",
            "",
        ] {
            assert!(!is_openable(bad), "{bad:?}");
        }
    }

    #[test]
    fn only_absolute_paths_of_existing_files_are_opened() {
        let dir = tempfile::tempdir().expect("dir");
        let file = dir.path().join("r.html");
        assert!(!is_openable_path(&file), "does not exist yet");
        std::fs::write(&file, "x").expect("write");
        assert!(is_openable_path(&file));
        assert!(!is_openable_path(dir.path()), "a folder");
        assert!(
            !is_openable_path(std::path::Path::new("r.html")),
            "relative"
        );
        assert!(!super::open_path(std::path::Path::new("-oProxyCommand=x")));
    }

    #[test]
    fn only_html_files_are_opened() {
        let dir = tempfile::tempdir().expect("dir");
        for (name, ok) in [
            ("r.html", true),
            ("r.HTM", true),
            ("r.Html", true),
            ("r.exe", false),
            ("r.txt", false),
            ("r", false),
            ("r.html.exe", false),
        ] {
            let file = dir.path().join(name);
            std::fs::write(&file, "x").expect("write");
            assert_eq!(is_openable_path(&file), ok, "{name}");
        }
    }
}
