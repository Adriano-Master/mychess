// Derived from Scid vs PC 4.27, tcl/main.tcl (`photo`, `trimEngineName`,
// `updatePlayerPhotos`) and tcl/end.tcl (loading of the photo files).
// Copyright (c) 1999-2003 Shane Hudson; Scid vs PC contributors. Scid is
// distributed under the GNU General Public License (see its COPYING); this
// file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Player photos (rules 101 to 106; `docs/formatos/spf-fotos.md`).
//!
//! The `.spf` file of the Scid is a Tcl script. This reader **never runs
//! it**: it only recognises commands `photo <name> <base64 data>` with a
//! small tokenizer for Tcl words (quotes, braces, bare words, `;`, a
//! backslash before a newline, `#` comments); every other command is
//! ignored. An image is kept as bytes plus the kind found by its signature
//! (GIF, PNG, JPEG); decoding it is the interface's job.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::SpellChecker;
use crate::progress::CancelToken;

/// The largest `.spf` file read.
pub const MAX_SPF_BYTES: usize = 256 << 20;
/// The largest photo, decoded.
pub const MAX_PHOTO_BYTES: usize = 4 << 20;
/// The most photos of one `.spf` file or one library.
pub const MAX_PHOTOS: usize = 200_000;
/// The shortest prefix tried by the lookup of a name without a comma.
const MIN_PREFIX_CHARS: usize = 3;
/// The longest key kept (in characters): longer names are never photo keys.
const MAX_KEY_CHARS: usize = 128;

/// The kind of an image, from its first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhotoKind {
    /// `GIF87a` or `GIF89a`.
    Gif,
    /// PNG.
    Png,
    /// JPEG.
    Jpeg,
}

/// The kind of image in `bytes`, by signature.
///
/// ```
/// use mychess_db::spelling::photo::{PhotoKind, detect_kind};
///
/// assert_eq!(detect_kind(b"GIF89a...."), Some(PhotoKind::Gif));
/// assert_eq!(detect_kind(b"hello"), None);
/// ```
pub fn detect_kind(bytes: &[u8]) -> Option<PhotoKind> {
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(PhotoKind::Gif)
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(PhotoKind::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(PhotoKind::Jpeg)
    } else {
        None
    }
}

/// The key under which a photo is stored and looked for (rule 102): the name
/// of the correct spelling (`retrievename`) when a checker is given,
/// lowercase, without the prefix `deep ` and without any white space.
///
/// ```
/// use mychess_db::spelling::photo::photo_key;
///
/// assert_eq!(photo_key("Carlsen, Magnus", None), photo_key("carlsen,magnus", None));
/// assert_eq!(photo_key("Deep Fritz 11", None), "fritz11");
/// ```
pub fn photo_key(name: &str, spelling: Option<&SpellChecker>) -> String {
    let name = spelling.and_then(|s| s.correct(name)).unwrap_or(name);
    let lower: String = name.chars().map(simple_lower).collect();
    let lower = lower.strip_prefix("deep ").unwrap_or(&lower);
    lower.chars().filter(|c| !c.is_whitespace()).collect()
}

fn simple_lower(c: char) -> char {
    let mut l = c.to_lowercase();
    match (l.next(), l.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}

/// Where the bytes of a photo are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhotoSource {
    /// Inside a `.spf` file, already decoded from base64.
    Bytes(Arc<[u8]>),
    /// A loose image file.
    File(PathBuf),
}

/// What the reading of a `.spf` file found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpfReport {
    /// Photos accepted.
    pub photos: u32,
    /// Commands that are not `photo` (not run).
    pub ignored_commands: u32,
    /// `photo` commands dropped: bad base64, not an image, too big, or
    /// malformed.
    pub invalid: u32,
    /// The file or the number of photos exceeded a limit and the rest was
    /// not read.
    pub truncated: bool,
}

/// One Tcl word and where the next starts; `None` at the end of the command.
struct Words<'a> {
    b: &'a [u8],
    pos: usize,
}

enum Item {
    Word(Vec<u8>),
    /// `;` or a newline: end of the command.
    End,
    /// The text ended inside a quote or brace.
    Unterminated,
    Eof,
}

impl Words<'_> {
    fn skip_blanks(&mut self) {
        loop {
            match self.b.get(self.pos) {
                Some(b' ' | b'\t' | b'\r') => self.pos += 1,
                Some(b'\\') if self.b.get(self.pos + 1) == Some(&b'\n') => self.pos += 2,
                Some(b'\\')
                    if self.b.get(self.pos + 1) == Some(&b'\r')
                        && self.b.get(self.pos + 2) == Some(&b'\n') =>
                {
                    self.pos += 3;
                }
                _ => return,
            }
        }
    }

    fn skip_comment(&mut self) {
        while let Some(&c) = self.b.get(self.pos) {
            if c == b'\\' {
                self.pos += 2;
                continue;
            }
            self.pos += 1;
            if c == b'\n' {
                return;
            }
        }
        self.pos = self.pos.min(self.b.len());
    }

    fn next(&mut self) -> Item {
        self.skip_blanks();
        let Some(&c) = self.b.get(self.pos) else {
            return Item::Eof;
        };
        match c {
            b'\n' | b';' => {
                self.pos += 1;
                Item::End
            }
            b'"' => {
                self.pos += 1;
                let mut out = Vec::new();
                loop {
                    match self.b.get(self.pos) {
                        None => return Item::Unterminated,
                        Some(b'"') => {
                            self.pos += 1;
                            return Item::Word(out);
                        }
                        Some(b'\\') => {
                            match self.b.get(self.pos + 1) {
                                Some(b'\n') => out.push(b' '),
                                Some(&e) => out.push(e),
                                None => return Item::Unterminated,
                            }
                            self.pos += 2;
                        }
                        Some(&x) => {
                            out.push(x);
                            self.pos += 1;
                        }
                    }
                }
            }
            b'{' => {
                self.pos += 1;
                let mut depth = 1usize;
                let mut out = Vec::new();
                loop {
                    let Some(&x) = self.b.get(self.pos) else {
                        return Item::Unterminated;
                    };
                    self.pos += 1;
                    match x {
                        b'\\' => {
                            out.push(x);
                            if let Some(&e) = self.b.get(self.pos) {
                                out.push(e);
                                self.pos += 1;
                            }
                        }
                        b'{' => {
                            depth += 1;
                            out.push(x);
                        }
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                return Item::Word(out);
                            }
                            out.push(x);
                        }
                        _ => out.push(x),
                    }
                }
            }
            _ => {
                let mut out = Vec::new();
                while let Some(&x) = self.b.get(self.pos) {
                    if matches!(x, b' ' | b'\t' | b'\r' | b'\n' | b';') {
                        break;
                    }
                    if x == b'\\'
                        && let Some(&e) = self.b.get(self.pos + 1)
                    {
                        if e == b'\n' {
                            break;
                        }
                        out.push(e);
                        self.pos += 2;
                        continue;
                    }
                    out.push(x);
                    self.pos += 1;
                }
                Item::Word(out)
            }
        }
    }
}

/// Standard base64 (alphabet `A-Za-z0-9+/`, `=` padding); white space is
/// skipped. `None` for any other byte or for more than `limit` decoded
/// bytes.
fn base64_decode(text: &[u8], limit: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0u32;
    let mut padding = false;
    for &c in text {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                padding = true;
                continue;
            }
            b' ' | b'\t' | b'\r' | b'\n' => continue,
            _ => return None,
        };
        if padding {
            return None;
        }
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xFF).ok()?);
            if out.len() > limit {
                return None;
            }
        }
    }
    Some(out)
}

/// Reads the `photo` commands of a `.spf` file: `(name, image bytes)` in
/// file order, and a report. Nothing is ever executed.
pub fn read_spf(bytes: &[u8]) -> (Vec<(String, Vec<u8>)>, SpfReport) {
    let mut report = SpfReport::default();
    let mut photos = Vec::new();
    if bytes.len() > MAX_SPF_BYTES {
        report.truncated = true;
        return (photos, report);
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let mut words = Words { b: bytes, pos: 0 };
    'file: loop {
        let mut command: Vec<Vec<u8>> = Vec::new();
        let mut first = true;
        loop {
            if first {
                words.skip_blanks();
                if words.b.get(words.pos) == Some(&b'#') {
                    words.skip_comment();
                    continue 'file;
                }
                first = false;
            }
            match words.next() {
                Item::Word(w) => command.push(w),
                Item::End => break,
                Item::Eof => {
                    handle(&command, &mut photos, &mut report);
                    break 'file;
                }
                Item::Unterminated => {
                    report.invalid += 1;
                    break 'file;
                }
            }
        }
        handle(&command, &mut photos, &mut report);
        if photos.len() >= MAX_PHOTOS {
            report.truncated = true;
            break;
        }
    }
    (photos, report)
}

fn handle(command: &[Vec<u8>], photos: &mut Vec<(String, Vec<u8>)>, report: &mut SpfReport) {
    let Some(head) = command.first() else {
        return;
    };
    if head.as_slice() != b"photo" {
        report.ignored_commands += 1;
        return;
    }
    let (Some(name), Some(data), 3) = (command.get(1), command.get(2), command.len()) else {
        report.invalid += 1;
        return;
    };
    let image = base64_decode(data, MAX_PHOTO_BYTES).filter(|img| detect_kind(img).is_some());
    match image {
        Some(image) if !name.is_empty() => {
            photos.push((String::from_utf8_lossy(name).into_owned(), image));
            report.photos += 1;
        }
        _ => report.invalid += 1,
    }
}

/// The photos known to the program, by [`photo_key`].
#[derive(Debug, Clone, Default)]
pub struct PhotoLibrary {
    by_key: HashMap<Box<str>, PhotoSource>,
}

/// What loading a directory found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PhotoLoadReport {
    /// Photos added from `.spf` files.
    pub from_spf: u32,
    /// Loose images added.
    pub loose: u32,
    /// Files that could not be used (unreadable, not an image, too big).
    pub errors: u32,
    /// `.spf` commands that were not photos or were invalid.
    pub spf_ignored: u32,
}

impl PhotoLibrary {
    /// An empty library.
    pub fn new() -> PhotoLibrary {
        PhotoLibrary::default()
    }

    /// The number of photos.
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    /// Whether there are no photos.
    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// Adds (or replaces) the photo of a player.
    pub fn insert(&mut self, name: &str, source: PhotoSource) {
        if self.by_key.len() >= MAX_PHOTOS
            && !self.by_key.contains_key(photo_key(name, None).as_str())
        {
            return;
        }
        let key = photo_key(name, None);
        // A key longer than any real name is never looked up by prefix.
        if !key.is_empty() && key.chars().count() <= MAX_KEY_CHARS {
            self.by_key.insert(key.into_boxed_str(), source);
        }
    }

    /// The photo of `name` (rule 102): the key of the correct spelling; if
    /// there is none and the name has no comma and more than two
    /// characters, the longest prefix (of at least three characters) that
    /// is a key.
    pub fn lookup(&self, name: &str, spelling: Option<&SpellChecker>) -> Option<&PhotoSource> {
        if self.by_key.is_empty() {
            return None;
        }
        let key = photo_key(name, spelling);
        if let Some(found) = self.by_key.get(key.as_str()) {
            return Some(found);
        }
        if name.contains(',') {
            return None;
        }
        // Only the first characters can be a stored key (`insert` bounds
        // them), so the work does not grow with the length of the name.
        let chars: Vec<char> = key.chars().take(MAX_KEY_CHARS + 1).collect();
        if chars.len() <= 2 {
            return None;
        }
        let longest = if key.chars().nth(MAX_KEY_CHARS + 1).is_some() {
            MAX_KEY_CHARS
        } else {
            chars.len().saturating_sub(1)
        };
        (MIN_PREFIX_CHARS..=longest).rev().find_map(|n| {
            let prefix: String = chars.iter().take(n).collect();
            self.by_key.get(prefix.as_str())
        })
    }

    /// Loads the `.spf` files and the loose images of a directory: first the
    /// `.spf` files in alphabetical order (the last one wins), then the
    /// images (`name.gif`, `.png`, `.jpg`, `.jpeg`, any case), which replace
    /// them (rules 101 to 104). A missing directory adds nothing. Stops
    /// early if `cancel` is set.
    pub fn load_dir(&mut self, dir: &Path, cancel: &CancelToken) -> PhotoLoadReport {
        let mut report = PhotoLoadReport::default();
        let Ok(read) = fs::read_dir(dir) else {
            return report;
        };
        let mut files: Vec<(String, PathBuf)> = read
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
            .collect();
        files.sort();
        let ext = |name: &str| {
            Path::new(name)
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
        };
        for (name, path) in files
            .iter()
            .filter(|(n, _)| ext(n).as_deref() == Some("spf"))
        {
            if cancel.is_cancelled() {
                return report;
            }
            let _ = name;
            match fs::read(path) {
                Ok(bytes) => {
                    let (photos, spf) = read_spf(&bytes);
                    report.spf_ignored += spf.ignored_commands + spf.invalid;
                    for (player, image) in photos {
                        self.insert(&player, PhotoSource::Bytes(image.into()));
                        report.from_spf += 1;
                    }
                }
                Err(_) => report.errors += 1,
            }
        }
        for (name, path) in &files {
            if cancel.is_cancelled() {
                return report;
            }
            if !matches!(ext(name).as_deref(), Some("gif" | "png" | "jpg" | "jpeg")) {
                continue;
            }
            let stem = Path::new(name)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if loose_image_ok(path) {
                self.insert(&stem, PhotoSource::File(path.clone()));
                report.loose += 1;
            } else {
                report.errors += 1;
            }
        }
        report
    }
}

/// Whether a loose file is a readable image of a size we accept.
fn loose_image_ok(path: &Path) -> bool {
    use std::io::Read;
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if meta.len() > MAX_PHOTO_BYTES as u64 {
        return false;
    }
    let mut head = [0u8; 8];
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let n = file.read(&mut head).unwrap_or(0);
    head.get(..n).and_then(detect_kind).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIF_B64: &str = "R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";

    fn gif() -> Vec<u8> {
        base64_decode(GIF_B64.as_bytes(), 1000).unwrap()
    }

    #[test]
    fn a_key_of_128_characters_is_kept_and_one_of_129_is_not() {
        let mut lib = PhotoLibrary::new();
        let at_limit = "a".repeat(MAX_KEY_CHARS);
        lib.insert(&at_limit, PhotoSource::File(PathBuf::from("a.gif")));
        assert_eq!(lib.len(), 1);
        lib.insert(
            &"b".repeat(MAX_KEY_CHARS + 1),
            PhotoSource::File(PathBuf::from("b.gif")),
        );
        assert_eq!(lib.len(), 1, "the longer key is not stored");
        // The key at the limit is found by its exact name and by a longer
        // name that starts with it (no comma).
        assert!(lib.lookup(&at_limit, None).is_some());
        let longer = format!("{at_limit}zzz");
        assert!(lib.lookup(&longer, None).is_some());
    }

    #[test]
    fn base64_decodes_and_rejects() {
        assert_eq!(base64_decode(b"TWFu", 10).unwrap(), b"Man");
        assert_eq!(base64_decode(b"TWE=", 10).unwrap(), b"Ma");
        assert_eq!(base64_decode(b"TW\n Fu", 10).unwrap(), b"Man");
        assert!(base64_decode(b"TW*u", 10).is_none());
        assert!(base64_decode(b"TWFu", 2).is_none());
        assert!(base64_decode(b"TQ==TQ", 10).is_none());
        assert_eq!(&gif()[..6], b"GIF89a");
    }

    #[test]
    fn spf_accepts_both_word_styles_and_separators() {
        let text = format!(
            "# photos\nphoto \"Almeida, Beatriz Costa\" {{{GIF_B64}}}\n\
             photo {{Braga, Carlos Eduardo}} \"{GIF_B64}\"; photo Solo {GIF_B64}\n\
             photo \"A, B\" \\\n  {GIF_B64}\n"
        );
        let (photos, report) = read_spf(text.as_bytes());
        let names: Vec<&str> = photos.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "Almeida, Beatriz Costa",
                "Braga, Carlos Eduardo",
                "Solo",
                "A, B"
            ]
        );
        assert_eq!(report.photos, 4);
        assert_eq!(report.invalid, 0);
    }

    #[test]
    fn spf_never_runs_other_commands() {
        let text = format!(
            "exec touch /nonexistent/x\nset a [exec rm -rf /]\nsource evil.tcl\n\
             photo X {GIF_B64}\nphoto bad !!!!\nphoto Y {}\nphoto only\n",
            "QUJD"
        );
        let (photos, report) = read_spf(text.as_bytes());
        assert_eq!(photos.len(), 1);
        assert_eq!(report.ignored_commands, 3);
        assert_eq!(report.invalid, 3, "bad base64, not an image, wrong arity");
    }

    #[test]
    fn spf_unterminated_and_huge_input_do_not_panic() {
        let (p, r) = read_spf(b"photo \"abc");
        assert_eq!(p.len(), 0);
        assert_eq!(r.invalid, 1);
        let (p, r) = read_spf(b"photo {abc");
        assert_eq!((p.len(), r.invalid), (0, 1));
        for cut in 0..40 {
            let text = format!("photo {{A}} {GIF_B64}\n# c\nx y;z");
            drop(read_spf(text.as_bytes().get(..cut).unwrap_or(&[])));
        }
    }

    #[test]
    fn keys_and_lookup() {
        assert_eq!(photo_key("Carlsen, Magnus", None), "carlsen,magnus");
        assert_eq!(photo_key("Deep Fritz 11", None), "fritz11");
        let mut lib = PhotoLibrary::new();
        lib.insert("Fritz", PhotoSource::Bytes(gif().into()));
        lib.insert("Carlsen, Magnus", PhotoSource::File(PathBuf::from("x.gif")));
        assert!(lib.lookup("carlsen,magnus", None).is_some());
        assert!(
            lib.lookup("Deep Fritz 11", None).is_some(),
            "longest prefix"
        );
        assert!(lib.lookup("Fri", None).is_none(), "shorter than the key");
        assert!(
            lib.lookup("Carlsen, Magn", None).is_none(),
            "comma: no prefix search"
        );
        assert!(lib.lookup("", None).is_none());
    }

    #[test]
    fn directory_loading_prefers_loose_images() {
        let dir = tempfile::tempdir().unwrap();
        let spf = format!("photo \"Alfa, A\" {GIF_B64}\nphoto \"Beta, B\" {GIF_B64}\n");
        fs::write(dir.path().join("a.SPF"), spf).unwrap();
        fs::write(dir.path().join("Alfa, A.GIF"), gif()).unwrap();
        fs::write(dir.path().join("Gama, G.png"), b"not an image").unwrap();
        fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        let mut lib = PhotoLibrary::new();
        let report = lib.load_dir(dir.path(), &CancelToken::new());
        assert_eq!((report.from_spf, report.loose, report.errors), (2, 1, 1));
        assert!(matches!(
            lib.lookup("Alfa, A", None),
            Some(PhotoSource::File(_))
        ));
        assert!(matches!(
            lib.lookup("Beta, B", None),
            Some(PhotoSource::Bytes(_))
        ));
        assert!(lib.lookup("Gama, G", None).is_none());
        let mut empty = PhotoLibrary::new();
        assert_eq!(
            empty.load_dir(&dir.path().join("none"), &CancelToken::new()),
            PhotoLoadReport::default()
        );
    }
}
