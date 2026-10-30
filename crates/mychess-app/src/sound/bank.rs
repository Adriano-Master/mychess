//! The sounds: [`Clip`], the sounds folder of the user and the choice of the
//! clips for a cue (D2806, D2807).

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use thiserror::Error;

use super::announce::{SoundToken, announce_tokens};
use super::cue::SoundCue;
use super::pack::SoundPack;
use super::prefs::SoundPrefs;
use super::synth;
use super::wav::{MAX_FILE_BYTES, parse_wav};

/// Most files of the folder that are read; the rest is ignored.
pub const MAX_FOLDER_FILES: usize = 64;

/// Most memory the decoded samples of one folder may take (`f32` bytes). A
/// file that would pass it is refused and counted in [`SoundFolder::skipped`].
pub(crate) const MAX_FOLDER_BYTES: usize = 64 * 1024 * 1024;

/// A sound: interleaved samples in `-1.0..=1.0`.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    /// Frames per second.
    pub sample_rate: u32,
    /// 1 (mono) or 2 (stereo).
    pub channels: u16,
    /// The interleaved samples.
    pub samples: Vec<f32>,
}

impl Clip {
    /// The same clip with every sample multiplied by `gain`.
    #[must_use]
    pub fn scaled(&self, gain: f32) -> Clip {
        Clip {
            sample_rate: self.sample_rate,
            channels: self.channels,
            samples: self.samples.iter().map(|s| s * gain).collect(),
        }
    }
}

/// The default sound of `cue`, rendered once (all the cues together, on the
/// first call). The GUI warms it up in a thread when it installs the sink
/// (`install_sink`), so `update` normally finds it ready.
#[must_use]
pub fn builtin_clip(cue: SoundCue) -> Arc<Clip> {
    static BUILTIN: OnceLock<Vec<Arc<Clip>>> = OnceLock::new();
    let all = BUILTIN.get_or_init(|| {
        SoundCue::ALL
            .iter()
            .map(|c| Arc::new(synth::render(*c)))
            .collect()
    });
    let index = SoundCue::ALL.iter().position(|c| *c == cue).unwrap_or(0);
    Arc::clone(&all[index])
}

/// Why the sounds folder could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum SoundFolderError {
    /// The path is not a folder (or does not exist).
    #[error("not a folder: {0}")]
    NotAFolder(PathBuf),
    /// The folder could not be listed.
    #[error("cannot read the folder: {0:?}")]
    Io(std::io::ErrorKind),
}

/// The sound files found in the folder of the user.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundFolder {
    path: PathBuf,
    /// By lower case file name without the extension.
    clips: HashMap<String, Arc<Clip>>,
    skipped: usize,
}

impl SoundFolder {
    /// The folder that was read.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// How many sound files served.
    #[must_use]
    pub fn found(&self) -> usize {
        self.clips.len()
    }

    /// How many files with a known name were refused (not a good WAV).
    #[must_use]
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    /// The clip of the file `stem.wav`, case-insensitive.
    #[must_use]
    pub fn get(&self, stem: &str) -> Option<&Arc<Clip>> {
        self.clips.get(&stem.to_ascii_lowercase())
    }

    /// A folder with the given clips, for tests of the other modules.
    #[must_use]
    pub fn from_clips(path: PathBuf, clips: Vec<(&str, Clip)>) -> SoundFolder {
        SoundFolder {
            path,
            clips: clips
                .into_iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), Arc::new(v)))
                .collect(),
            skipped: 0,
        }
    }
}

/// The lower case names (without extension) that the folder may hold.
fn known_stems() -> Vec<String> {
    let mut stems: Vec<String> = SoundCue::ALL
        .iter()
        .map(|c| c.file_stem().to_ascii_lowercase())
        .chain(
            SoundToken::all()
                .into_iter()
                .map(|t| t.file_stem().to_ascii_lowercase()),
        )
        .collect();
    stems.sort();
    stems.dedup();
    stems
}

/// Reads the sound files of `dir` (blocking: run it off the interface).
///
/// Names are matched ignoring case; only regular `.wav` files (any case) with
/// a known name are read, at most [`MAX_FOLDER_FILES`] of them, in name
/// order, and at most `MAX_FOLDER_BYTES` of decoded samples in all. A
/// symlink is followed when it ends at a regular file; a FIFO, a device or a
/// folder (also behind a link) is ignored, because opening it could block for
/// ever. A file that is not a good sound is counted in
/// [`SoundFolder::skipped`] and the rest goes on.
///
/// # Errors
/// [`SoundFolderError`] when `dir` is not a folder or cannot be listed.
pub fn load_folder(dir: &Path) -> Result<SoundFolder, SoundFolderError> {
    load_folder_within(dir, MAX_FOLDER_BYTES, MAX_FOLDER_FILES)
}

fn load_folder_within(
    dir: &Path,
    budget: usize,
    max_files: usize,
) -> Result<SoundFolder, SoundFolderError> {
    if !dir.is_dir() {
        return Err(SoundFolderError::NotAFolder(dir.to_path_buf()));
    }
    let known = known_stems();
    let mut files: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .map_err(|e| SoundFolderError::Io(e.kind()))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let is_wav = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
            let stem = path.file_stem()?.to_str()?.to_ascii_lowercase();
            (is_wav && known.contains(&stem) && is_regular_file(&path)).then_some((stem, path))
        })
        .collect();
    // Case variants of one name: the file name order decides.
    files.sort_by(|a, b| a.1.cmp(&b.1));
    files.truncate(max_files);
    let mut clips = HashMap::new();
    let mut skipped = 0;
    let mut left = budget;
    for (stem, path) in files {
        if clips.contains_key(&stem) {
            continue;
        }
        let fits = read_clip(&path).and_then(|clip| {
            let bytes = clip.samples.len().checked_mul(size_of::<f32>())?;
            left = left.checked_sub(bytes)?;
            Some(clip)
        });
        match fits {
            Some(clip) => {
                clips.insert(stem, Arc::new(clip));
            }
            None => skipped += 1,
        }
    }
    Ok(SoundFolder {
        path: dir.to_path_buf(),
        clips,
        skipped,
    })
}

/// Whether `path` (links followed) is a regular file.
fn is_regular_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file())
}

/// One file, read with a size cap (the file may grow while it is read). The
/// kind is checked again before the open; a swap in between is left to the
/// watchdog of the load.
fn read_clip(path: &Path) -> Option<Clip> {
    if !is_regular_file(path) {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes).ok()?;
    parse_wav(&bytes).ok()
}

/// The clips to play for `cue`, in order (D2807).
///
/// With `prefs.announce` and a folder, the queue of the words of `san` (or
/// `Back` for [`SoundCue::Back`]): the words with no file are skipped, and if
/// none is left the sound of the cue plays. Otherwise the file of the cue in
/// the folder (the long castling uses `CastleQ.wav`), or the default sound.
#[must_use]
pub fn clips_for(
    cue: SoundCue,
    san: Option<&str>,
    prefs: &SoundPrefs,
    folder: Option<&SoundFolder>,
) -> Vec<Arc<Clip>> {
    clips_for_pack(cue, san, prefs, folder, None)
}

/// Like [`clips_for`], with a sound pack between the folder and the default
/// sounds (D2902): the file of the folder, else the clip of the pack
/// following its fallback chain (D2905), else the synthesized sound.
#[must_use]
pub fn clips_for_pack(
    cue: SoundCue,
    san: Option<&str>,
    prefs: &SoundPrefs,
    folder: Option<&SoundFolder>,
    pack: Option<&SoundPack>,
) -> Vec<Arc<Clip>> {
    if let (true, Some(folder)) = (prefs.announce, folder) {
        let tokens = match (cue, san) {
            (SoundCue::Back, _) => vec![SoundToken::Back],
            (
                SoundCue::Move
                | SoundCue::Capture
                | SoundCue::Check
                | SoundCue::Checkmate
                | SoundCue::Castle
                | SoundCue::Promotion,
                Some(san),
            ) => announce_tokens(san),
            _ => Vec::new(),
        };
        let spoken: Vec<Arc<Clip>> = tokens
            .into_iter()
            .filter_map(|t| folder.get(&t.file_stem()).cloned())
            .collect();
        if !spoken.is_empty() {
            return spoken;
        }
    }
    let stem = if cue == SoundCue::Castle && san.is_some_and(|s| s.starts_with("O-O-O")) {
        "CastleQ"
    } else {
        cue.file_stem()
    };
    let clip = folder
        .and_then(|f| f.get(stem))
        .or_else(|| pack.and_then(|p| p.resolve(cue)))
        .cloned()
        .unwrap_or_else(|| builtin_clip(cue));
    vec![clip]
}

#[cfg(test)]
mod tests {
    use super::super::wav::encode_pcm16;
    use super::*;

    fn tone(n: usize) -> Clip {
        Clip {
            sample_rate: 8_000,
            channels: 1,
            samples: vec![0.25; n],
        }
    }

    fn prefs(announce: bool) -> SoundPrefs {
        SoundPrefs {
            announce,
            ..SoundPrefs::default()
        }
    }

    #[test]
    fn builtin_clips_are_shared() {
        let a = builtin_clip(SoundCue::Move);
        let b = builtin_clip(SoundCue::Move);
        assert!(Arc::ptr_eq(&a, &b));
        assert_ne!(*a, *builtin_clip(SoundCue::Capture));
    }

    #[test]
    fn the_cue_uses_the_default_sound_without_a_folder() {
        let clips = clips_for(SoundCue::Check, Some("Qh5+"), &prefs(true), None);
        assert_eq!(clips.len(), 1);
        assert!(Arc::ptr_eq(&clips[0], &builtin_clip(SoundCue::Check)));
    }

    #[test]
    fn a_file_of_the_folder_replaces_the_default() {
        let folder = SoundFolder::from_clips("x".into(), vec![("CHECK", tone(5))]);
        let clips = clips_for(SoundCue::Check, None, &prefs(false), Some(&folder));
        assert_eq!(clips[0].samples.len(), 5);
        // Another cue has no file: the default.
        let other = clips_for(SoundCue::Move, None, &prefs(false), Some(&folder));
        assert!(Arc::ptr_eq(&other[0], &builtin_clip(SoundCue::Move)));
    }

    #[test]
    fn the_pack_sits_between_the_folder_and_the_default() {
        use super::super::pack::PackId;
        let pack = SoundPack::from_clips(
            PackId::User("p".into()),
            vec![(SoundCue::Move, tone(11)), (SoundCue::Check, tone(12))],
        );
        let folder = SoundFolder::from_clips("x".into(), vec![("Check", tone(5))]);
        let p = prefs(false);
        let pick = |cue, folder| clips_for_pack(cue, None, &p, folder, Some(&pack));
        // The folder wins cue by cue.
        assert_eq!(pick(SoundCue::Check, Some(&folder))[0].samples.len(), 5);
        // Then the pack, with its chain.
        assert_eq!(pick(SoundCue::Check, None)[0].samples.len(), 12);
        assert_eq!(
            pick(SoundCue::Checkmate, Some(&folder))[0].samples.len(),
            12
        );
        assert_eq!(pick(SoundCue::Capture, None)[0].samples.len(), 11);
        // Then the synthesized one.
        assert!(Arc::ptr_eq(
            &pick(SoundCue::LowTime, None)[0],
            &builtin_clip(SoundCue::LowTime)
        ));
    }

    #[test]
    fn announcing_plays_the_queue_and_skips_missing_words() {
        let folder = SoundFolder::from_clips(
            "x".into(),
            vec![("Knight", tone(1)), ("f", tone(2)), ("3", tone(3))],
        );
        let clips = clips_for(SoundCue::Move, Some("Nf3"), &prefs(true), Some(&folder));
        let sizes: Vec<_> = clips.iter().map(|c| c.samples.len()).collect();
        assert_eq!(sizes, [1, 2, 3]);
        // "Nf3+": the Check word has no file and is skipped.
        let clips = clips_for(SoundCue::Check, Some("Nf3+"), &prefs(true), Some(&folder));
        assert_eq!(clips.len(), 3);
        // Nothing of the move can be said: the cue sound plays.
        let clips = clips_for(SoundCue::Move, Some("e4"), &prefs(true), Some(&folder));
        assert!(Arc::ptr_eq(&clips[0], &builtin_clip(SoundCue::Move)));
        // Announcing is off: no queue.
        let clips = clips_for(SoundCue::Move, Some("Nf3"), &prefs(false), Some(&folder));
        assert_eq!(clips.len(), 1);
    }

    #[test]
    fn going_back_says_back_and_the_long_castling_uses_its_file() {
        let folder = SoundFolder::from_clips(
            "x".into(),
            vec![
                ("Back", tone(7)),
                ("CastleQ", tone(9)),
                ("CastleK", tone(8)),
            ],
        );
        let back = clips_for(SoundCue::Back, None, &prefs(true), Some(&folder));
        assert_eq!(back[0].samples.len(), 7);
        let long = clips_for(
            SoundCue::Castle,
            Some("O-O-O"),
            &prefs(false),
            Some(&folder),
        );
        assert_eq!(long[0].samples.len(), 9);
        let short = clips_for(SoundCue::Castle, Some("O-O"), &prefs(false), Some(&folder));
        assert_eq!(short[0].samples.len(), 8);
    }

    #[test]
    fn the_folder_is_read_ignoring_case_and_bad_files() {
        let dir = tempfile::tempdir().unwrap();
        let good = encode_pcm16(8_000, 1, &[100, 200, 300]);
        std::fs::write(dir.path().join("Check.WAV"), &good).unwrap();
        std::fs::write(dir.path().join("knight.wav"), &good).unwrap();
        std::fs::write(dir.path().join("e.wav"), &good).unwrap();
        std::fs::write(dir.path().join("Mate.wav"), b"not a wav").unwrap();
        std::fs::write(dir.path().join("notes.wav"), &good).unwrap();
        std::fs::write(dir.path().join("move.mp3"), &good).unwrap();
        let folder = load_folder(dir.path()).unwrap();
        assert_eq!(folder.found(), 3);
        assert_eq!(folder.skipped(), 1);
        assert!(folder.get("check").is_some());
        assert!(folder.get("Knight").is_some());
        assert!(folder.get("mate").is_none());
        assert!(folder.get("notes").is_none());
        assert_eq!(folder.path(), dir.path());
    }

    #[test]
    fn a_missing_folder_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope");
        assert_eq!(
            load_folder(&missing),
            Err(SoundFolderError::NotAFolder(missing.clone()))
        );
        let file = dir.path().join("f.wav");
        std::fs::write(&file, b"x").unwrap();
        assert!(matches!(
            load_folder(&file),
            Err(SoundFolderError::NotAFolder(_))
        ));
    }

    fn good_file(dir: &Path, name: &str) {
        let good = encode_pcm16(8_000, 1, &[100, 200, 300, 400]);
        std::fs::write(dir.join(name), good).unwrap();
    }

    #[test]
    fn more_than_the_file_limit_reads_only_the_first_ones() {
        // Only ~33 names are known, so the real limit (64) is reached just
        // by case variants; a small limit exercises the same cut.
        let dir = tempfile::tempdir().unwrap();
        for stem in ["a", "b", "c", "d", "e"] {
            good_file(dir.path(), &format!("{stem}.wav"));
        }
        let folder = load_folder_within(dir.path(), MAX_FOLDER_BYTES, 3).unwrap();
        assert_eq!(folder.found(), 3);
        // Name order decides which ones are kept.
        assert!(folder.get("a").is_some() && folder.get("c").is_some());
        assert!(folder.get("d").is_none() && folder.get("e").is_none());
        let all = load_folder(dir.path()).unwrap();
        assert_eq!(all.found(), 5, "under the real limit everything is read");
    }

    #[test]
    fn mixed_case_variants_of_one_name_count_once() {
        let dir = tempfile::tempdir().unwrap();
        good_file(dir.path(), "MOVE.WAV");
        good_file(dir.path(), "move.wav");
        let folder = load_folder(dir.path()).unwrap();
        // On a case-insensitive file system there is a single file.
        assert_eq!(folder.found(), 1);
        assert_eq!(folder.skipped(), 0);
    }

    #[test]
    fn the_total_of_samples_is_limited() {
        let dir = tempfile::tempdir().unwrap();
        good_file(dir.path(), "Move.wav");
        good_file(dir.path(), "Check.wav");
        good_file(dir.path(), "Mate.wav");
        // Each file is 4 samples = 16 bytes; room for two.
        let folder = load_folder_within(dir.path(), 32, MAX_FOLDER_FILES).unwrap();
        assert_eq!(folder.found(), 2);
        assert_eq!(folder.skipped(), 1);
        let none = load_folder_within(dir.path(), 0, MAX_FOLDER_FILES).unwrap();
        assert_eq!(none.found(), 0);
        assert_eq!(none.skipped(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_file_is_followed_but_a_link_to_a_folder_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        good_file(other.path(), "real.dat");
        std::os::unix::fs::symlink(other.path().join("real.dat"), dir.path().join("Move.wav"))
            .unwrap();
        std::fs::create_dir(other.path().join("sub")).unwrap();
        std::os::unix::fs::symlink(other.path().join("sub"), dir.path().join("Check.wav")).unwrap();
        let folder = load_folder(dir.path()).unwrap();
        assert!(folder.get("move").is_some());
        assert!(folder.get("check").is_none());
        assert_eq!(folder.skipped(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_is_ignored_and_does_not_block() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("Move.wav");
        let made = std::process::Command::new("mkfifo").arg(&fifo).status();
        if !made.is_ok_and(|s| s.success()) {
            return; // no mkfifo on this machine
        }
        good_file(dir.path(), "Check.wav");
        let (tx, rx) = std::sync::mpsc::channel();
        let path = dir.path().to_path_buf();
        std::thread::spawn(move || {
            tx.send(load_folder(&path)).ok();
        });
        let folder = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the load must not block on a FIFO")
            .unwrap();
        assert_eq!(folder.found(), 1);
        assert!(folder.get("move").is_none());
    }

    #[test]
    fn scaled_multiplies_the_samples() {
        let c = tone(2).scaled(0.5);
        assert_eq!(c.samples, vec![0.125, 0.125]);
    }
}
