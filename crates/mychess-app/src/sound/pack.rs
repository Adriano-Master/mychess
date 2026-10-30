//! Sound packs: identity, `pack.toml`, discovery and loading (spec M8-11b,
//! D2900-D2909).
//!
//! A pack is a folder of sound files named by event. There are three
//! origins, with stable ids kept in the preferences: `builtin:synth` (the
//! synthesized sounds, the default), `bundled:<folder>` (compiled into the
//! binary, see [`super::bundled`]) and `user:<folder>` (a folder of the
//! user's data directory, see [`user_packs_dir`]).
//!
//! Everything here that touches the disk is blocking and must run off the
//! interface (the effects of [`super::message`] do that). A pack folder of
//! the user is untrusted: names are validated, a link is followed only when
//! its target stays inside the pack, the number of entries and the sizes are
//! capped, and a bad file only costs its own cue.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Serialize, Serializer};
use thiserror::Error;

use super::bank::Clip;
use super::bundled::{self, BundledPack};
use super::cue::SoundCue;
use super::decode::{SoundFileError, decode_file, decode_static};
use super::wav::MAX_FILE_BYTES;

/// Most packs of the user listed.
pub const MAX_USER_PACKS: usize = 64;
/// Most directory entries of one pack that are looked at.
pub const MAX_PACK_ENTRIES: usize = 256;
/// Largest `pack.toml` read, in bytes.
pub const MAX_MANIFEST_BYTES: usize = 16 * 1024;
/// Longest folder name of a pack, in characters.
pub const MAX_FOLDER_NAME: usize = 64;
/// Most memory the decoded samples of one pack may take (`f32` bytes).
const MAX_PACK_SAMPLE_BYTES: usize = 64 * 1024 * 1024;
/// Most entries of the packs directory that are looked at.
const MAX_SCANNED_FOLDERS: usize = 1024;
/// Longest metadata text kept (name, author, license, description).
const MAX_TEXT_CHARS: usize = 200;
/// Most entries of `[cues]` that are read.
const MAX_CUE_ENTRIES: usize = 256;
/// The file extensions of a sound, in order of preference.
const EXTENSIONS: [&str; 3] = ["wav", "ogg", "mp3"];

/// The identity of a pack, as kept in the preferences (D2901).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum PackId {
    /// The synthesized sounds (default).
    #[default]
    BuiltinSynth,
    /// A pack compiled into the binary: the name of its folder.
    Bundled(String),
    /// A pack in the user's folder: the name of its folder.
    User(String),
}

impl PackId {
    /// Reads `builtin:synth`, `bundled:<folder>` or `user:<folder>`. The
    /// folder must be a valid name and a bundled one must exist; `None` for
    /// anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<PackId> {
        let id = if text == "builtin:synth" {
            PackId::BuiltinSynth
        } else if let Some(folder) = text.strip_prefix("bundled:") {
            PackId::Bundled(folder.to_owned())
        } else {
            PackId::User(text.strip_prefix("user:")?.to_owned())
        };
        id.is_valid().then_some(id)
    }

    /// Whether the folder name is valid and a bundled pack exists.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        match self {
            PackId::BuiltinSynth => true,
            PackId::Bundled(folder) => bundled::find(folder).is_some(),
            PackId::User(folder) => is_valid_folder_name(folder),
        }
    }

    /// Where the pack comes from.
    #[must_use]
    pub fn origin(&self) -> PackOrigin {
        match self {
            PackId::BuiltinSynth => PackOrigin::Builtin,
            PackId::Bundled(_) => PackOrigin::Bundled,
            PackId::User(_) => PackOrigin::User,
        }
    }
}

impl fmt::Display for PackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackId::BuiltinSynth => f.write_str("builtin:synth"),
            PackId::Bundled(folder) => write!(f, "bundled:{folder}"),
            PackId::User(folder) => write!(f, "user:{folder}"),
        }
    }
}

impl Serialize for PackId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Where a pack comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackOrigin {
    /// The synthesized sounds.
    Builtin,
    /// Compiled into the binary.
    Bundled,
    /// A folder of the user.
    User,
}

/// Whether `name` is a safe folder name: 1 to 64 characters of
/// `[A-Za-z0-9._-]`, with no `..` (D2901).
#[must_use]
pub fn is_valid_folder_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MAX_FOLDER_NAME
        // "." and ".." name directories; a leading dot hides the folder and
        // a trailing dot is stripped by Windows.
        && !name.contains("..")
        && !name.starts_with('.')
        && !name.ends_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Something odd in a pack that did not stop it from working.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PackWarning {
    /// `pack.toml` is not valid TOML or not valid text; the folder name is
    /// the name of the pack.
    BadManifest,
    /// `pack.toml` is larger than [`MAX_MANIFEST_BYTES`]; it is ignored.
    ManifestTooLarge,
    /// A cue is mapped to a file that does not exist (or is not a sound
    /// file); the cue is absent.
    MissingFile {
        /// The cue.
        cue: SoundCue,
        /// The file name written in `pack.toml`.
        file: String,
    },
    /// A cue is mapped to a name that is not a plain file name.
    BadFileName(String),
    /// A sound file could not be decoded; its cues are absent.
    Undecodable {
        /// The file name.
        file: String,
        /// Why.
        error: SoundFileError,
    },
    /// The folder has more than [`MAX_PACK_ENTRIES`] entries; the rest was
    /// not looked at.
    TooManyEntries,
    /// A link points outside the pack (or cannot be resolved); ignored.
    LinkOutside(String),
    /// A sound file is larger than the limit; ignored.
    FileTooLarge(String),
    /// The decoded sounds of the pack passed the memory limit.
    OverBudget(String),
    /// A folder of the packs directory has an invalid name; ignored.
    BadFolderName(String),
    /// Two folders differ only by case; only the first is listed.
    DuplicateFolder(String),
    /// More than [`MAX_USER_PACKS`] folders; the rest is not listed.
    TooManyPacks,
    /// A folder could not be read; ignored.
    Unreadable(String),
}

/// Why a pack could not be loaded or the packs could not be listed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum PackError {
    /// There is no such pack (a bundled id that does not exist, or a folder
    /// that is gone).
    #[error("sound pack not found: {0}")]
    NotFound(PackId),
    /// The id is not valid.
    #[error("invalid sound pack id")]
    InvalidId,
    /// The directory could not be read.
    #[error("cannot read the sound packs: {0:?}")]
    Io(std::io::ErrorKind),
}

/// What is known about a pack without decoding it (D2907).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackInfo {
    /// The identity.
    pub id: PackId,
    /// The name to show: from `pack.toml`, or the folder name.
    pub name: String,
    /// The author, if `pack.toml` says.
    pub author: String,
    /// The license (SPDX), if `pack.toml` says.
    pub license: String,
    /// A line about the pack, if `pack.toml` says.
    pub description: String,
    /// How many of the [`SoundCue::ALL`] events have a file in the pack
    /// (without the fallback chain).
    pub covered: usize,
    /// Remarks about the pack.
    pub warnings: Vec<PackWarning>,
}

impl PackInfo {
    /// The origin of the pack.
    #[must_use]
    pub fn origin(&self) -> PackOrigin {
        self.id.origin()
    }

    /// The number of events there are (the denominator of "7 of 10").
    pub const TOTAL: usize = SoundCue::ALL.len();
}

/// A pack with its sounds decoded.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundPack {
    info: PackInfo,
    clips: BTreeMap<SoundCue, Arc<Clip>>,
}

impl SoundPack {
    /// What is known about the pack (warnings include the decoding ones).
    #[must_use]
    pub fn info(&self) -> &PackInfo {
        &self.info
    }

    /// The clip the pack has for `cue`, with no fallback.
    #[must_use]
    pub fn direct(&self, cue: SoundCue) -> Option<&Arc<Clip>> {
        self.clips.get(&cue)
    }

    /// The clip for `cue` following the chain of the pack (D2905):
    /// checkmate, check, move; castling, promotion, back and capture end in
    /// move. `None` means the pack has nothing in its timbre for the cue and
    /// the synthesized sound plays.
    #[must_use]
    pub fn resolve(&self, cue: SoundCue) -> Option<&Arc<Clip>> {
        fallback_chain(cue).iter().find_map(|c| self.clips.get(c))
    }

    /// A pack with the given clips, for tests.
    #[must_use]
    pub fn from_clips(id: PackId, clips: Vec<(SoundCue, Clip)>) -> SoundPack {
        let clips: BTreeMap<_, _> = clips.into_iter().map(|(c, k)| (c, Arc::new(k))).collect();
        SoundPack {
            info: PackInfo {
                name: id.to_string(),
                id,
                author: String::new(),
                license: String::new(),
                description: String::new(),
                covered: clips.len(),
                warnings: Vec::new(),
            },
            clips,
        }
    }
}

/// The cues to try for `cue`, in order (D2905).
fn fallback_chain(cue: SoundCue) -> &'static [SoundCue] {
    match cue {
        SoundCue::Checkmate => &[SoundCue::Checkmate, SoundCue::Check, SoundCue::Move],
        SoundCue::Check => &[SoundCue::Check, SoundCue::Move],
        SoundCue::Capture => &[SoundCue::Capture, SoundCue::Move],
        SoundCue::Castle => &[SoundCue::Castle, SoundCue::Move],
        SoundCue::Promotion => &[SoundCue::Promotion, SoundCue::Move],
        SoundCue::Back => &[SoundCue::Back, SoundCue::Move],
        SoundCue::Move => &[SoundCue::Move],
        SoundCue::GameStart => &[SoundCue::GameStart],
        SoundCue::GameEnd => &[SoundCue::GameEnd],
        SoundCue::LowTime => &[SoundCue::LowTime],
    }
}

/// The key of `cue` in `[cues]` of `pack.toml`.
const fn cue_key(cue: SoundCue) -> &'static str {
    match cue {
        SoundCue::Move => "move",
        SoundCue::Capture => "capture",
        SoundCue::Check => "check",
        SoundCue::Checkmate => "checkmate",
        SoundCue::Castle => "castle",
        SoundCue::Promotion => "promotion",
        SoundCue::Back => "back",
        SoundCue::GameStart => "start",
        SoundCue::GameEnd => "end",
        SoundCue::LowTime => "clock",
    }
}

/// The user's packs directory: `sounds/` in the data directory of the app,
/// or `None` when the system has none. Nothing is created.
#[must_use]
pub fn user_packs_dir() -> Option<PathBuf> {
    crate::start::data_dir().map(|dir| dir.join("sounds"))
}

// ---- pack.toml ----------------------------------------------------------

#[derive(Debug, Default)]
struct Manifest {
    name: String,
    author: String,
    license: String,
    description: String,
    /// Lower case key to file name. Unknown keys are kept and never looked
    /// at (the Lichess packs have `defeat`, `draw` and `notify`).
    cues: HashMap<String, String>,
}

/// Text for the interface: no control characters, trimmed and short.
/// Unicode `Cf` characters that matter for display spoofing.
fn is_format_char(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{E0000}'..='\u{E007F}'
    )
}

fn clean_text(text: &str) -> String {
    text.chars()
        // Control characters and format characters (Cf: bidirectional
        // overrides and isolates U+202A-202E and U+2066-2069, zero width...)
        // could disguise the name shown in the window.
        .filter(|c| !c.is_control() && !is_format_char(*c))
        .take(MAX_TEXT_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Reads `pack.toml`; `None` when it is not valid TOML.
fn parse_manifest(text: &str) -> Option<Manifest> {
    let table: toml::Table = text.parse().ok()?;
    let field = |key: &str| {
        table
            .get(key)
            .and_then(toml::Value::as_str)
            .map(clean_text)
            .unwrap_or_default()
    };
    let cues = table
        .get("cues")
        .and_then(toml::Value::as_table)
        .map(|t| {
            t.iter()
                .take(MAX_CUE_ENTRIES)
                .filter_map(|(k, v)| Some((k.to_ascii_lowercase(), v.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    Some(Manifest {
        name: field("name"),
        author: field("author"),
        license: field("license"),
        description: field("description"),
        cues,
    })
}

/// A plain file name: no separator, no drive, no `..`, no control character.
fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '\0'])
        && !name.chars().any(char::is_control)
}

// ---- the files of a pack -------------------------------------------------

#[derive(Debug)]
enum Source {
    Embedded(&'static [u8]),
    Path(PathBuf),
}

/// The sound files of a pack, by lower case file name.
#[derive(Debug, Default)]
struct Files {
    entries: BTreeMap<String, Source>,
}

impl Files {
    fn from_bundled(pack: &BundledPack) -> Files {
        Files {
            entries: pack
                .files
                .iter()
                .map(|(name, bytes)| (name.to_ascii_lowercase(), Source::Embedded(bytes)))
                .collect(),
        }
    }

    fn decode(&self, lower: &str) -> Result<Clip, SoundFileError> {
        match self.entries.get(lower) {
            Some(Source::Embedded(bytes)) => decode_static(bytes),
            Some(Source::Path(path)) => decode_file(path),
            None => Err(SoundFileError::Io(std::io::ErrorKind::NotFound)),
        }
    }
}

/// The result of looking into the folder of a user's pack.
struct Folder {
    files: Files,
    manifest: Option<String>,
    warnings: Vec<PackWarning>,
}

/// Resolves `path` to a regular file inside `root` (canonical): a link is
/// followed only when its target stays inside the pack (D2908).
fn file_inside(root: &Path, path: &Path) -> Result<(PathBuf, u64), PackWarning> {
    let label = || {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let canonical = path
        .canonicalize()
        .map_err(|_| PackWarning::LinkOutside(label()))?;
    if !canonical.starts_with(root) {
        return Err(PackWarning::LinkOutside(label()));
    }
    let meta = std::fs::metadata(&canonical).map_err(|_| PackWarning::LinkOutside(label()))?;
    if !meta.is_file() {
        return Err(PackWarning::LinkOutside(label()));
    }
    Ok((canonical, meta.len()))
}

fn read_folder(dir: &Path) -> Result<Folder, PackError> {
    let root = dir.canonicalize().map_err(|e| PackError::Io(e.kind()))?;
    let mut warnings = Vec::new();
    let mut names = Vec::new();
    for (seen, entry) in std::fs::read_dir(&root)
        .map_err(|e| PackError::Io(e.kind()))?
        .filter_map(Result::ok)
        .enumerate()
    {
        if seen >= MAX_PACK_ENTRIES {
            warnings.push(PackWarning::TooManyEntries);
            break;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let is_sound = Path::new(&name)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)));
        if is_sound {
            names.push(name);
        }
    }
    // Case variants of one name: the sorted order decides.
    names.sort();
    let mut files = Files::default();
    for name in names {
        let lower = name.to_ascii_lowercase();
        if files.entries.contains_key(&lower) {
            continue;
        }
        match file_inside(&root, &root.join(&name)) {
            Ok((path, len)) if len <= MAX_FILE_BYTES => {
                files.entries.insert(lower, Source::Path(path));
            }
            Ok(_) => warnings.push(PackWarning::FileTooLarge(name)),
            Err(warning) => warnings.push(warning),
        }
    }
    let manifest = read_manifest_file(&root, &mut warnings);
    Ok(Folder {
        files,
        manifest,
        warnings,
    })
}

fn read_manifest_file(root: &Path, warnings: &mut Vec<PackWarning>) -> Option<String> {
    let path = root.join("pack.toml");
    if std::fs::symlink_metadata(&path).is_err() {
        return None;
    }
    let Ok((path, len)) = file_inside(root, &path) else {
        warnings.push(PackWarning::BadManifest);
        return None;
    };
    if usize::try_from(len).map_or(true, |n| n > MAX_MANIFEST_BYTES) {
        warnings.push(PackWarning::ManifestTooLarge);
        return None;
    }
    let mut bytes = Vec::new();
    let read = std::fs::File::open(&path).and_then(|f| {
        f.take(MAX_MANIFEST_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
    });
    match (read, String::from_utf8(bytes)) {
        (Ok(_), Ok(text)) if text.len() <= MAX_MANIFEST_BYTES => Some(text),
        _ => {
            warnings.push(PackWarning::BadManifest);
            None
        }
    }
}

// ---- planning and loading ------------------------------------------------

/// What the pack has for a cue.
enum Plan {
    /// This file (lower case name).
    File(String),
    /// Nothing.
    Absent,
}

/// Which file serves each cue; the warnings of the mapping go to `warnings`.
fn plan(
    manifest: Option<&Manifest>,
    files: &Files,
    warnings: &mut Vec<PackWarning>,
) -> Vec<(SoundCue, Plan)> {
    SoundCue::ALL
        .iter()
        .map(|&cue| {
            let mapped = manifest.and_then(|m| m.cues.get(cue_key(cue)));
            let plan = match mapped {
                Some(file) if !is_plain_file_name(file) => {
                    warnings.push(PackWarning::BadFileName(clean_text(file)));
                    Plan::Absent
                }
                Some(file) => {
                    let lower = file.to_ascii_lowercase();
                    if files.entries.contains_key(&lower) {
                        Plan::File(lower)
                    } else {
                        warnings.push(PackWarning::MissingFile {
                            cue,
                            file: clean_text(file),
                        });
                        Plan::Absent
                    }
                }
                None => EXTENSIONS
                    .iter()
                    .flat_map(|ext| {
                        [
                            cue.file_stem().to_ascii_lowercase(),
                            cue_key(cue).to_owned(),
                        ]
                        .into_iter()
                        .map(move |stem| format!("{stem}.{ext}"))
                    })
                    .find(|name| files.entries.contains_key(name))
                    .map_or(Plan::Absent, Plan::File),
            };
            (cue, plan)
        })
        .collect()
}

/// Everything about a pack that needs no decoding.
fn inspect(
    id: PackId,
    folder_name: &str,
    manifest_text: Option<&str>,
    files: &Files,
    mut warnings: Vec<PackWarning>,
) -> (PackInfo, Vec<(SoundCue, Plan)>) {
    let manifest = manifest_text.and_then(|t| {
        let parsed = parse_manifest(t);
        if parsed.is_none() {
            warnings.push(PackWarning::BadManifest);
        }
        parsed
    });
    let plan = plan(manifest.as_ref(), files, &mut warnings);
    let covered = plan
        .iter()
        .filter(|(_, p)| matches!(p, Plan::File(_)))
        .count();
    let m = manifest.unwrap_or_default();
    let info = PackInfo {
        id,
        name: if m.name.is_empty() {
            clean_text(folder_name)
        } else {
            m.name
        },
        author: m.author,
        license: m.license,
        description: m.description,
        covered,
        warnings,
    };
    (info, plan)
}

fn load_planned(info: PackInfo, plan: &[(SoundCue, Plan)], files: &Files) -> SoundPack {
    let mut info = info;
    let mut decoded: HashMap<&str, Option<Arc<Clip>>> = HashMap::new();
    let mut clips = BTreeMap::new();
    let mut left = MAX_PACK_SAMPLE_BYTES;
    for (cue, plan) in plan {
        let Plan::File(name) = plan else { continue };
        let entry = decoded
            .entry(name.as_str())
            .or_insert_with(|| match files.decode(name) {
                Ok(clip) => {
                    let bytes = clip.samples.len().saturating_mul(size_of::<f32>());
                    if let Some(rest) = left.checked_sub(bytes) {
                        left = rest;
                        Some(Arc::new(clip))
                    } else {
                        info.warnings.push(PackWarning::OverBudget(name.clone()));
                        None
                    }
                }
                Err(error) => {
                    info.warnings.push(PackWarning::Undecodable {
                        file: name.clone(),
                        error,
                    });
                    None
                }
            });
        if let Some(clip) = entry {
            clips.insert(*cue, Arc::clone(clip));
        }
    }
    info.covered = clips.len();
    SoundPack { info, clips }
}

/// The synthesized sounds, as an entry of the list.
fn synth_info() -> PackInfo {
    PackInfo {
        id: PackId::BuiltinSynth,
        name: "Default (synthesized)".to_owned(),
        author: String::new(),
        license: String::new(),
        description: String::new(),
        covered: SoundCue::ALL.len(),
        warnings: Vec::new(),
    }
}

fn bundled_info(pack: &BundledPack) -> (PackInfo, Vec<(SoundCue, Plan)>, Files) {
    let files = Files::from_bundled(pack);
    let (info, plan) = inspect(
        PackId::Bundled(pack.folder.to_owned()),
        pack.folder,
        Some(pack.manifest),
        &files,
        Vec::new(),
    );
    (info, plan, files)
}

/// The packs that need no disk: the synthesized one and the embedded ones
/// (D2906). Pure.
#[must_use]
pub fn list_packs() -> Vec<PackInfo> {
    std::iter::once(synth_info())
        .chain(bundled::BUNDLED.iter().map(|p| bundled_info(p).0))
        .collect()
}

/// The packs found in the user's folder, and what was odd about the folder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackScan {
    /// The packs, by name without regard to case.
    pub packs: Vec<PackInfo>,
    /// Folders that were ignored and why.
    pub warnings: Vec<PackWarning>,
}

/// Lists the packs in `dir` without decoding anything (D2906): the
/// sub-folders with a valid name (at most [`MAX_USER_PACKS`], sorted without
/// regard to case), their `pack.toml` and how many events each serves. A
/// directory that does not exist is an empty list.
///
/// # Errors
/// [`PackError::Io`] when `dir` exists but cannot be listed.
pub fn scan_user_packs(dir: &Path) -> Result<PackScan, PackError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(PackScan::default()),
        Err(e) => return Err(PackError::Io(e.kind())),
    };
    let mut warnings = Vec::new();
    let mut names = Vec::new();
    for entry in entries.filter_map(Result::ok).take(MAX_SCANNED_FOLDERS) {
        // `is_dir` follows links: a link to a folder is a pack folder.
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_valid_folder_name(&name) {
            names.push(name);
        } else {
            warnings.push(PackWarning::BadFolderName(clean_text(&name)));
        }
    }
    names.sort_by_cached_key(|n| (n.to_ascii_lowercase(), n.clone()));
    let mut packs = Vec::new();
    let mut last = String::new();
    for name in names {
        let lower = name.to_ascii_lowercase();
        if lower == last {
            warnings.push(PackWarning::DuplicateFolder(name));
            continue;
        }
        last = lower;
        if packs.len() >= MAX_USER_PACKS {
            warnings.push(PackWarning::TooManyPacks);
            break;
        }
        match read_folder(&dir.join(&name)) {
            Ok(folder) => {
                let (info, _) = inspect(
                    PackId::User(name.clone()),
                    &name,
                    folder.manifest.as_deref(),
                    &folder.files,
                    folder.warnings,
                );
                packs.push(info);
            }
            Err(_) => warnings.push(PackWarning::Unreadable(name)),
        }
    }
    Ok(PackScan { packs, warnings })
}

/// Reads and decodes the pack `id` (blocking). `user_dir` is the packs
/// directory of the user, for `user:` ids. A file that cannot be decoded
/// leaves its cue out and adds a warning; the rest of the pack loads.
///
/// # Errors
/// [`PackError::NotFound`] when the pack does not exist,
/// [`PackError::InvalidId`] for a bad folder name, [`PackError::Io`] when
/// the folder cannot be read.
pub fn load_pack(id: &PackId, user_dir: Option<&Path>) -> Result<SoundPack, PackError> {
    match id {
        PackId::BuiltinSynth => Ok(SoundPack {
            info: synth_info(),
            clips: BTreeMap::new(),
        }),
        PackId::Bundled(folder) => {
            let pack = bundled::find(folder).ok_or_else(|| PackError::NotFound(id.clone()))?;
            let (info, plan, files) = bundled_info(pack);
            Ok(load_planned(info, &plan, &files))
        }
        PackId::User(folder) => {
            if !is_valid_folder_name(folder) {
                return Err(PackError::InvalidId);
            }
            let dir = user_dir
                .map(|d| d.join(folder))
                .filter(|d| d.is_dir())
                .ok_or_else(|| PackError::NotFound(id.clone()))?;
            let found = read_folder(&dir)?;
            let (info, plan) = inspect(
                id.clone(),
                folder,
                found.manifest.as_deref(),
                &found.files,
                found.warnings,
            );
            Ok(load_planned(info, &plan, &found.files))
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dot_names_and_dotted_edges_are_not_folder_names() {
        for bad in [".", "..", ".hidden", "trailing.", "a..b"] {
            assert!(!is_valid_folder_name(bad), "{bad}");
        }
        assert!(is_valid_folder_name("good-1.0_x"));
        assert!(!PackId::User(".".into()).is_valid());
    }

    #[test]
    fn clean_text_drops_bidi_and_format_characters() {
        let spoof = "a\u{202E}b\u{2066}c\u{2069}d\u{200B}e\u{FEFF}f\u{202A}\u{202D}";
        assert_eq!(clean_text(spoof), "abcdef");
        assert_eq!(clean_text("  ok\u{7} text "), "ok text");
    }

    use super::super::wav::encode_pcm16;
    use super::*;

    fn wav(n: usize) -> Vec<u8> {
        encode_pcm16(8_000, 1, &vec![1_000; n])
    }

    #[test]
    fn ids_round_trip_and_reject_bad_names() {
        for text in ["builtin:synth", "bundled:sfx", "user:My_Pack-1.0"] {
            assert_eq!(PackId::parse(text).unwrap().to_string(), text);
        }
        for text in [
            "",
            "synth",
            "bundled:nope",
            "user:",
            "user:..",
            "user:a/b",
            "user:a\\b",
            "user:a..b",
            "user:caf\u{e9}",
            &format!("user:{}", "x".repeat(65)),
            "Builtin:synth",
        ] {
            assert_eq!(PackId::parse(text), None, "{text}");
        }
        assert!(PackId::parse(&format!("user:{}", "x".repeat(64))).is_some());
    }

    #[test]
    fn the_chain_follows_the_spec() {
        let m = Clip {
            sample_rate: 8_000,
            channels: 1,
            samples: vec![0.1],
        };
        let c = Clip {
            samples: vec![0.2],
            ..m.clone()
        };
        let pack = SoundPack::from_clips(
            PackId::User("x".into()),
            vec![(SoundCue::Move, m.clone()), (SoundCue::Check, c.clone())],
        );
        assert_eq!(**pack.resolve(SoundCue::Checkmate).unwrap(), c);
        assert_eq!(**pack.resolve(SoundCue::Check).unwrap(), c);
        for cue in [
            SoundCue::Capture,
            SoundCue::Castle,
            SoundCue::Promotion,
            SoundCue::Back,
        ] {
            assert_eq!(**pack.resolve(cue).unwrap(), m, "{cue:?}");
        }
        for cue in [SoundCue::GameStart, SoundCue::GameEnd, SoundCue::LowTime] {
            assert!(pack.resolve(cue).is_none(), "{cue:?}");
        }
    }

    #[test]
    fn manifest_ignores_unknown_cue_keys_and_non_text_values() {
        let m = parse_manifest(
            "name = \"N\"\nauthor = \"A\"\n[cues]\nMove = \"a.wav\"\ndefeat = \"d.ogg\"\nx = 5\n",
        )
        .unwrap();
        assert_eq!(m.name, "N");
        assert_eq!(m.cues.get("move").map(String::as_str), Some("a.wav"));
        assert!(m.cues.contains_key("defeat"));
        assert!(!m.cues.contains_key("x"));
        assert!(parse_manifest("name = [").is_none());
        assert!(parse_manifest("name = 3").unwrap().name.is_empty());
    }

    #[test]
    fn plain_file_names() {
        assert!(is_plain_file_name("Move.ogg"));
        for bad in [
            "", "..", "../x.ogg", "a/b.ogg", "a\\b.ogg", "C:x.ogg", "a\0b",
        ] {
            assert!(!is_plain_file_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_listed_packs_are_the_synth_and_the_embedded_ones() {
        let list = list_packs();
        let ids: Vec<_> = list.iter().map(|p| p.id.to_string()).collect();
        assert_eq!(
            ids,
            [
                "builtin:synth",
                "bundled:sfx",
                "bundled:nes",
                "bundled:futuristic",
                "bundled:piano"
            ]
        );
        for p in &list[1..] {
            assert_eq!(p.covered, 6, "{}", p.name);
            assert!(p.warnings.is_empty(), "{:?}", p.warnings);
            assert_eq!(p.author, "Enigmahack");
        }
    }

    #[test]
    fn a_bundled_pack_loads_with_the_checkmate_sharing_the_check_clip() {
        let pack = load_pack(&PackId::Bundled("sfx".into()), None).unwrap();
        assert_eq!(pack.info().covered, 6);
        assert!(
            pack.info().warnings.is_empty(),
            "{:?}",
            pack.info().warnings
        );
        let check = pack.direct(SoundCue::Check).unwrap();
        assert!(Arc::ptr_eq(
            check,
            pack.direct(SoundCue::Checkmate).unwrap()
        ));
        assert!(pack.direct(SoundCue::Castle).is_none());
        assert!(Arc::ptr_eq(
            pack.resolve(SoundCue::Castle).unwrap(),
            pack.direct(SoundCue::Move).unwrap()
        ));
    }

    #[test]
    fn unknown_packs_are_not_found() {
        let id = PackId::Bundled("nope".into());
        assert_eq!(load_pack(&id, None), Err(PackError::NotFound(id)));
        let id = PackId::User("gone".into());
        assert_eq!(
            load_pack(&id, Some(Path::new("."))),
            Err(PackError::NotFound(id.clone()))
        );
        assert_eq!(load_pack(&id, None), Err(PackError::NotFound(id)));
        assert_eq!(
            load_pack(&PackId::User("..".into()), None),
            Err(PackError::InvalidId)
        );
    }

    #[test]
    fn a_user_pack_matches_names_without_case_and_prefers_wav() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("mine");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("MOVE.WAV"), wav(4)).unwrap();
        std::fs::write(dir.join("move.ogg"), b"junk").unwrap();
        std::fs::write(dir.join("Capture.wav"), wav(6)).unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        let pack = load_pack(&PackId::User("mine".into()), Some(root.path())).unwrap();
        assert_eq!(pack.info().name, "mine");
        assert_eq!(pack.info().covered, 2);
        assert_eq!(pack.direct(SoundCue::Move).unwrap().samples.len(), 4);
        assert!(pack.info().warnings.is_empty());
    }

    #[test]
    fn a_mapped_missing_file_is_a_warning_and_the_cue_is_absent() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("p");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.wav"), wav(3)).unwrap();
        // `move.wav` exists but the manifest maps move elsewhere.
        std::fs::write(dir.join("move.wav"), wav(9)).unwrap();
        std::fs::write(
            dir.join("pack.toml"),
            "name = \"Mine\"\n[cues]\nmove = \"gone.wav\"\ncapture = \"a.wav\"\ncheck = \"../a.wav\"\n",
        )
        .unwrap();
        let pack = load_pack(&PackId::User("p".into()), Some(root.path())).unwrap();
        assert_eq!(pack.info().name, "Mine");
        assert!(pack.direct(SoundCue::Move).is_none());
        assert!(pack.direct(SoundCue::Capture).is_some());
        let w = &pack.info().warnings;
        assert!(w.contains(&PackWarning::MissingFile {
            cue: SoundCue::Move,
            file: "gone.wav".into()
        }));
        assert!(w.contains(&PackWarning::BadFileName("../a.wav".into())));
    }

    #[test]
    fn a_bad_or_huge_manifest_falls_back_to_the_folder_name() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("p");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("pack.toml"), "name = [[[").unwrap();
        let scan = scan_user_packs(root.path()).unwrap();
        assert_eq!(scan.packs[0].name, "p");
        assert!(scan.packs[0].warnings.contains(&PackWarning::BadManifest));
        let big = format!("name = \"x\"\n# {}\n", "y".repeat(MAX_MANIFEST_BYTES));
        std::fs::write(dir.join("pack.toml"), big).unwrap();
        let scan = scan_user_packs(root.path()).unwrap();
        assert_eq!(scan.packs[0].name, "p");
        assert!(
            scan.packs[0]
                .warnings
                .contains(&PackWarning::ManifestTooLarge)
        );
    }

    #[test]
    fn the_scan_validates_names_sorts_and_limits() {
        let root = tempfile::tempdir().unwrap();
        for name in ["beta", "Alpha", "bad name", "gamma"] {
            std::fs::create_dir(root.path().join(name)).unwrap();
        }
        std::fs::write(root.path().join("file.txt"), b"x").unwrap();
        let scan = scan_user_packs(root.path()).unwrap();
        let names: Vec<_> = scan.packs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "beta", "gamma"]);
        assert_eq!(
            scan.warnings,
            [PackWarning::BadFolderName("bad name".into())]
        );
        let none = scan_user_packs(&root.path().join("missing")).unwrap();
        assert!(none.packs.is_empty() && none.warnings.is_empty());
    }

    #[test]
    fn the_scan_lists_at_most_sixty_four_packs() {
        let root = tempfile::tempdir().unwrap();
        for i in 0..70 {
            std::fs::create_dir(root.path().join(format!("p{i:02}"))).unwrap();
        }
        let scan = scan_user_packs(root.path()).unwrap();
        assert_eq!(scan.packs.len(), MAX_USER_PACKS);
        assert!(scan.warnings.contains(&PackWarning::TooManyPacks));
    }
}
