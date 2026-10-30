//! The `[sound]` table of the preferences (spec M8-11, D2804).

use std::ops::RangeInclusive;
use std::path::PathBuf;

use serde::Serialize;

use super::pack::PackId;

/// The range of the volume, in percent.
pub const VOLUME_RANGE: RangeInclusive<u8> = 0..=100;
/// Longest folder path kept, in characters.
pub const MAX_FOLDER_CHARS: usize = 4096;

/// What the user chose about the sounds. Off by default, like the Scid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SoundPrefs {
    /// The master switch.
    pub enabled: bool,
    /// The volume, in percent (0 plays nothing).
    pub volume: u8,
    /// Sound for a move added to the game (played or received).
    pub on_new_move: bool,
    /// Sound when moving forward through the game.
    pub on_forward: bool,
    /// Sound when moving back.
    pub on_back: bool,
    /// Only the plain move sound for every move (the Scid "tick-tock").
    pub tick_only: bool,
    /// Say the move with the word files of the sounds folder.
    pub announce: bool,
    /// The sounds folder; empty means only the built-in sounds. Text, so the
    /// file can always be written.
    pub folder: String,
    /// The sound pack (D2902): `builtin:synth`, `bundled:<folder>` or
    /// `user:<folder>`. The folder above wins over it cue by cue.
    pub pack: PackId,
}

impl Default for SoundPrefs {
    fn default() -> SoundPrefs {
        SoundPrefs {
            enabled: false,
            volume: 70,
            on_new_move: true,
            on_forward: false,
            on_back: false,
            tick_only: false,
            announce: false,
            folder: String::new(),
            pack: PackId::BuiltinSynth,
        }
    }
}

impl SoundPrefs {
    /// The preferences with the volume limited and the folder trimmed (a
    /// path too long is dropped).
    #[must_use]
    pub fn sanitized(mut self) -> SoundPrefs {
        self.volume = self
            .volume
            .clamp(*VOLUME_RANGE.start(), *VOLUME_RANGE.end());
        self.folder = self.folder.trim().to_owned();
        if self.folder.chars().count() > MAX_FOLDER_CHARS {
            self.folder.clear();
        }
        if !self.pack.is_valid() {
            self.pack = PackId::BuiltinSynth;
        }
        self
    }

    /// The sounds folder, if one is set.
    #[must_use]
    pub fn folder_path(&self) -> Option<PathBuf> {
        (!self.folder.is_empty()).then(|| PathBuf::from(&self.folder))
    }

    /// Whether any sound can play at all.
    #[must_use]
    pub fn audible(&self) -> bool {
        self.enabled && self.volume > 0
    }
}

/// The linear gain of a volume in percent: `(volume / 100)²` (D2805).
#[must_use]
pub fn gain(volume: u8) -> f32 {
    let v = f32::from(volume.min(100)) / 100.0;
    v * v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_off_with_new_moves_only() {
        let p = SoundPrefs::default();
        assert!(!p.enabled && !p.audible());
        assert!(p.on_new_move && !p.on_forward && !p.on_back);
        assert_eq!(p.folder_path(), None);
    }

    #[test]
    fn sanitized_limits_the_volume_and_trims_the_folder() {
        let p = SoundPrefs {
            volume: 250,
            folder: "  /sons  ".to_owned(),
            ..SoundPrefs::default()
        }
        .sanitized();
        assert_eq!(p.volume, 100);
        assert_eq!(p.folder, "/sons");
        assert_eq!(p.folder_path(), Some(PathBuf::from("/sons")));
        let long = SoundPrefs {
            folder: "x".repeat(MAX_FOLDER_CHARS + 1),
            ..SoundPrefs::default()
        }
        .sanitized();
        assert!(long.folder.is_empty());
    }

    #[test]
    fn sanitized_resets_an_invalid_pack() {
        let bad = SoundPrefs {
            pack: PackId::Bundled("nope".into()),
            ..SoundPrefs::default()
        }
        .sanitized();
        assert_eq!(bad.pack, PackId::BuiltinSynth);
        let bad = SoundPrefs {
            pack: PackId::User("../x".into()),
            ..SoundPrefs::default()
        }
        .sanitized();
        assert_eq!(bad.pack, PackId::BuiltinSynth);
        let ok = SoundPrefs {
            pack: PackId::Bundled("sfx".into()),
            ..SoundPrefs::default()
        }
        .sanitized();
        assert_eq!(ok.pack, PackId::Bundled("sfx".into()));
    }

    #[test]
    fn audible_needs_the_switch_and_a_volume() {
        let on = SoundPrefs {
            enabled: true,
            ..SoundPrefs::default()
        };
        assert!(on.audible());
        assert!(!SoundPrefs { volume: 0, ..on }.audible());
    }

    #[test]
    fn gain_is_a_square_law() {
        assert!(gain(0).abs() < f32::EPSILON);
        assert!((gain(100) - 1.0).abs() < f32::EPSILON);
        assert!((gain(50) - 0.25).abs() < 1e-6);
        assert!((gain(255) - 1.0).abs() < f32::EPSILON);
    }
}
