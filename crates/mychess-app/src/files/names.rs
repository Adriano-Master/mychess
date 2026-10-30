//! File names offered and fixed when saving (spec M2-2, D130).

use std::ffi::OsString;
use std::path::PathBuf;

use mychess_core::Tags;

/// Longest suggested name before the extension, in characters.
pub const MAX_FILE_STEM_CHARS: usize = 100;

/// Longest suggested name before the extension, in UTF-8 bytes: it leaves
/// room for ` (2).html` in the 255 bytes most file systems allow.
const MAX_FILE_STEM_BYTES: usize = 200;

/// Names Windows reserves, with or without an extension.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Whether Windows maps `stem` to a device: `CON`, `COM1` to `COM9`, the
/// superscript forms `COM\u{b9}`, `COM\u{b2}`, `COM\u{b3}` (and `LPT`), any case.
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
/// characters (`< > : " / \ | ? *` and controls) become `_`, spaces and
/// dots at both ends go, a Windows reserved name gets a `_` prefix, and the
/// result is cut to [`MAX_FILE_STEM_CHARS`] characters and
/// 200 bytes (at a character boundary). Never returns an empty string
/// (`game` instead).
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
    // Windows ignores spaces and dots at the end of the part before the
    // extension: `CON .x` is the console device.
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
        .take(MAX_FILE_STEM_CHARS)
        .take_while(|c| {
            bytes += c.len_utf8();
            bytes <= MAX_FILE_STEM_BYTES
        })
        .collect();
    let name = trim(&cut);
    if name.is_empty() {
        "game".to_owned()
    } else {
        name
    }
}

/// The file name offered by "Save as": `White - Black.pgn` when both names
/// are known, `game.pgn` otherwise.
///
/// ```
/// use mychess_app::suggest_file_name;
/// use mychess_core::Tags;
///
/// let mut tags = Tags::default();
/// assert_eq!(suggest_file_name(&tags), "game.pgn");
/// tags.white = "Kasparov, G.".to_owned();
/// tags.black = "Karpov, A.".to_owned();
/// assert_eq!(suggest_file_name(&tags), "Kasparov, G. - Karpov, A.pgn");
/// ```
pub fn suggest_file_name(tags: &Tags) -> String {
    let known = |name: &str| {
        let name = name.trim();
        !name.is_empty() && name != "?"
    };
    let stem = if known(&tags.white) && known(&tags.black) {
        sanitize_file_name(&format!("{} - {}", tags.white.trim(), tags.black.trim()))
    } else {
        "game".to_owned()
    };
    format!("{stem}.pgn")
}

/// Keeps a `.pgn` or `.txt` extension (any case) and otherwise **adds**
/// `.pgn` (Scid: `x.foo` becomes `x.foo.pgn`).
///
/// ```
/// use std::path::PathBuf;
/// use mychess_app::ensure_pgn_extension;
///
/// assert_eq!(ensure_pgn_extension(PathBuf::from("a.PGN")), PathBuf::from("a.PGN"));
/// assert_eq!(ensure_pgn_extension(PathBuf::from("a.foo")), PathBuf::from("a.foo.pgn"));
/// ```
pub fn ensure_pgn_extension(path: PathBuf) -> PathBuf {
    let keeps = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("pgn") || e.eq_ignore_ascii_case("txt"));
    if keeps {
        return path;
    }
    let mut name: OsString = path.into_os_string();
    name.push(".pgn");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(white: &str, black: &str) -> Tags {
        let mut tags = Tags::default();
        tags.white = white.to_owned();
        tags.black = black.to_owned();
        tags
    }

    #[test]
    fn extension_rules() {
        let ext = |s: &str| ensure_pgn_extension(PathBuf::from(s));
        assert_eq!(ext("a"), PathBuf::from("a.pgn"));
        assert_eq!(ext("a.PGN"), PathBuf::from("a.PGN"));
        assert_eq!(ext("a.txt"), PathBuf::from("a.txt"));
        assert_eq!(ext("a.foo"), PathBuf::from("a.foo.pgn"));
        assert_eq!(ext("a.pgn.gz"), PathBuf::from("a.pgn.gz.pgn"));
        assert_eq!(ext("dir.pgn/a"), PathBuf::from("dir.pgn/a.pgn"));
    }

    #[test]
    fn suggestions_use_known_names() {
        assert_eq!(
            suggest_file_name(&named("Kasparov, G.", "Karpov, A.")),
            "Kasparov, G. - Karpov, A.pgn"
        );
        assert_eq!(suggest_file_name(&named("?", "B")), "game.pgn");
        assert_eq!(suggest_file_name(&named("", "")), "game.pgn");
        assert_eq!(
            suggest_file_name(&named("a:b", "c/d\\e")),
            "a_b - c_d_e.pgn"
        );
    }

    #[test]
    fn a_long_name_is_cut_by_bytes_at_a_character_boundary() {
        // 100 CJK characters are 300 bytes: more than a file name allows.
        let cjk = "\u{4e2d}".repeat(100);
        let cut = sanitize_file_name(&cjk);
        assert!(cut.len() <= MAX_FILE_STEM_BYTES, "{} bytes", cut.len());
        assert_eq!(cut.chars().count(), 66);
        assert!(cut.chars().all(|c| c == '\u{4e2d}'));
        // The batch adds ` (2).html`: the whole name stays under 255 bytes.
        assert!(format!("{cut} (2).html").len() <= 255);
        // A short name and a 100-character ASCII name are not touched.
        assert_eq!(sanitize_file_name(&"a".repeat(100)), "a".repeat(100));
        assert_eq!(sanitize_file_name("\u{4e2d}\u{6587}"), "\u{4e2d}\u{6587}");
    }

    #[test]
    fn reserved_and_long_names() {
        assert_eq!(sanitize_file_name("con"), "_con");
        assert_eq!(sanitize_file_name("NUL.txt"), "_NUL.txt");
        assert_eq!(sanitize_file_name("CON .x"), "_CON .x");
        assert_eq!(sanitize_file_name("nul ..txt"), "_nul ..txt");
        assert_eq!(sanitize_file_name("com\u{b9}"), "_com\u{b9}");
        assert_eq!(sanitize_file_name("LPT\u{b3}.pgn"), "_LPT\u{b3}.pgn");
        assert_eq!(sanitize_file_name("COM10"), "COM10");
        assert_eq!(sanitize_file_name("console"), "console");
        assert_eq!(sanitize_file_name("  .. "), "game");
        assert_eq!(sanitize_file_name("name. "), "name");
        let long = suggest_file_name(&named(&"x".repeat(300), "y"));
        assert!(long.chars().count() <= MAX_FILE_STEM_CHARS + 4, "{long}");
        let stem = long.trim_end_matches(".pgn");
        assert!(!stem.ends_with(' ') && !stem.ends_with('.'));
    }
}
