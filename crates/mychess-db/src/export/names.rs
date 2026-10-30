//! Safe file names for generated exports (volumes of the HTML viewer).
//!
//! Same rules as `sanitize_file_name` of `mychess-app` (decision D130 of
//! `docs/specs/m2-2-arquivo-novo-abrir.md`), repeated here because the
//! database crate cannot depend on the application crate.

use std::path::{Path, PathBuf};

/// Longest name before the extension, in characters.
const MAX_STEM_CHARS: usize = 100;
/// Longest name before the extension, in UTF-8 bytes (room for `_5000.html`).
const MAX_STEM_BYTES: usize = 200;

/// Names Windows reserves, with or without an extension.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn is_reserved(stem: &str) -> bool {
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return true;
    }
    let mut chars = stem.chars();
    let prefix: String = chars.by_ref().take(3).collect();
    let rest: Vec<char> = chars.collect();
    (prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT"))
        && matches!(rest.as_slice(), ['\u{b9}' | '\u{b2}' | '\u{b3}'])
}

/// Makes `text` safe as a file name on Linux, Windows and macOS: forbidden
/// characters (`< > : " / \ | ? *` and controls) become `_`, spaces and dots
/// at both ends go, a Windows reserved name gets a `_` prefix and the result
/// is cut to 100 characters and 200 bytes. Never returns an empty string
/// (`export` instead).
///
/// ```
/// use mychess_db::export::names::sanitize_file_name;
///
/// assert_eq!(sanitize_file_name("a/b:c"), "a_b_c");
/// assert_eq!(sanitize_file_name("CON"), "_CON");
/// assert_eq!(sanitize_file_name(" .. "), "export");
/// ```
pub fn sanitize_file_name(text: &str) -> String {
    let replaced: String = text
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trim = |s: &str| s.trim_matches(|c: char| c == ' ' || c == '.').to_owned();
    let mut name = trim(&replaced);
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches([' ', '.']);
    if is_reserved(stem) {
        name.insert(0, '_');
    }
    let mut bytes = 0usize;
    let cut: String = name
        .chars()
        .take(MAX_STEM_CHARS)
        .take_while(|c| {
            bytes += c.len_utf8();
            bytes <= MAX_STEM_BYTES
        })
        .collect();
    let name = trim(&cut);
    if name.is_empty() {
        "export".to_owned()
    } else {
        name
    }
}

/// The file stem of `dest` without a `.html`/`.htm` extension (any letter
/// case), sanitized, plus the directory it lives in.
pub fn stem_and_dir(dest: &Path) -> Option<(String, PathBuf)> {
    let name = dest.file_name()?.to_string_lossy().into_owned();
    let lower = name.to_ascii_lowercase();
    let stem = ["html", "htm"]
        .iter()
        .find_map(|e| {
            lower
                .ends_with(&format!(".{e}"))
                .then(|| name.get(..name.len() - e.len() - 1))
        })
        .flatten()
        .unwrap_or(&name);
    let dir = match dest.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_owned(),
        _ => PathBuf::from("."),
    };
    Some((sanitize_file_name(stem), dir))
}

/// The name of volume `n` (1-based): `X.html`, `X_2.html`, ...
pub fn volume_file_name(stem: &str, n: u32) -> String {
    if n <= 1 {
        format!("{stem}.html")
    } else {
        format!("{stem}_{n}.html")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_names() {
        assert_eq!(volume_file_name("X", 1), "X.html");
        assert_eq!(volume_file_name("X", 3), "X_3.html");
    }

    #[test]
    fn stem_drops_the_html_extension_in_any_case() {
        let (stem, dir) = stem_and_dir(Path::new("/a/b/Game.HTML")).unwrap();
        assert_eq!((stem.as_str(), dir.as_path()), ("Game", Path::new("/a/b")));
        let (stem, _) = stem_and_dir(Path::new("plain")).unwrap();
        assert_eq!(stem, "plain");
        assert!(stem_and_dir(Path::new("/")).is_none());
    }

    #[test]
    fn reserved_and_forbidden_names_are_fixed() {
        assert_eq!(sanitize_file_name("con.txt"), "_con.txt");
        assert_eq!(sanitize_file_name("a<b>|c"), "a_b__c");
        assert_eq!(sanitize_file_name("\u{1F600}"), "\u{1F600}");
    }
}
