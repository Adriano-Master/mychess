//! Move sounds and the announcement of moves (spec M8-11, D2800-D2899).
//!
//! The pure part of the feature, with no sound card involved:
//!
//! - `cue`: [`SoundCue`] and [`cue_for_move`], the map move to sound;
//! - `announce`: the queue of sound files the Scid speaks for a SAN move
//!   ([`announce_tokens`]) and its text in each language ([`announce_text`]);
//! - [`synth`]: the default sounds, rendered in code and deterministic;
//! - [`wav`]: a small reader of PCM WAV files;
//! - `decode`: [`decode_clip`], WAV, OGG Vorbis and MP3 into a [`Clip`] with
//!   the limits of D2806 (spec M8-11b);
//! - `pack`, `bundled`: the sound packs ([`PackId`], [`SoundPack`],
//!   [`list_packs`], [`scan_user_packs`], [`load_pack`]);
//! - `bank`: [`Clip`], the sounds of the user's folder and the choice of the
//!   clips to play;
//! - `prefs`: [`SoundPrefs`], the `[sound]` table;
//! - `sink`: [`SoundSink`], where the clips go (the front-end installs the
//!   real one; tests use [`FakeSink`]);
//! - `message`: [`SoundMessage`], [`SoundEffect`] and [`perform`].
//!
//! The watch of the cursor and the state live in `state/sound_flow.rs`.

mod announce;
mod bank;
mod bundled;
mod cue;
mod decode;
mod message;
mod pack;
mod prefs;
mod sink;
pub mod synth;
pub mod wav;

pub use announce::{AnnounceLang, SoundToken, announce_text, announce_tokens};
pub use bank::{
    Clip, MAX_FOLDER_FILES, SoundFolder, SoundFolderError, builtin_clip, clips_for, clips_for_pack,
    load_folder,
};
pub use bundled::{BUDGET_BYTES as BUNDLED_BUDGET_BYTES, embedded_bytes, embedded_files};
pub use cue::{SoundCue, cue_for_move};
pub use decode::{
    PANIC_MESSAGE, SoundFileError, decode_clip, decode_file, decode_owned, decode_static,
};
pub use message::{FolderStatus, PackStatus, SoundEffect, SoundMessage, perform};
pub use pack::{
    MAX_FOLDER_NAME, MAX_MANIFEST_BYTES, MAX_PACK_ENTRIES, MAX_USER_PACKS, PackError, PackId,
    PackInfo, PackOrigin, PackScan, PackWarning, SoundPack, is_valid_folder_name, list_packs,
    load_pack, scan_user_packs, user_packs_dir,
};
pub use prefs::{MAX_FOLDER_CHARS, SoundPrefs, VOLUME_RANGE, gain};
pub use sink::{FakeSink, NullSink, PlayRequest, SoundSink, install_sink, perform_play};
