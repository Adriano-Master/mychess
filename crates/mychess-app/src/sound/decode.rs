//! Decoding of sound files: WAV (own reader), OGG Vorbis and MP3
//! (`symphonia`) into one [`Clip`] (spec M8-11b, D2904, ADR 0015).
//!
//! A sound file is untrusted input (a pack of the user). The limits are the
//! same for every format: the file has at most [`MAX_FILE_BYTES`], the sound
//! at most [`MAX_SECONDS`] seconds, a rate of [`MIN_RATE`]..=[`MAX_RATE`] Hz
//! and one or two channels. The decoder works packet by packet and the
//! decoded samples are counted *before* they are kept, so a header that
//! promises billions of samples never makes an allocation: the decoding is
//! aborted when the ceiling is passed. The decoder runs inside
//! `catch_unwind`: a panic in it is a refused file, not a crash.
//!
//! The format is chosen by the *content* (magic bytes), not by the extension:
//! a Vorbis file named `move.wav` is still read. MP3 is decoded as the
//! decoder delivers it, with the encoder delay (about 25 ms of silence at
//! the start) left in; the bundled packs are OGG for that reason.

use std::io::{Cursor, ErrorKind, Read};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use thiserror::Error;

use super::bank::Clip;
use super::wav::{MAX_FILE_BYTES, MAX_RATE, MAX_SECONDS, MIN_RATE, WavError, parse_wav};

/// Most packets a decoder may refuse (corrupt) before the file is given up.
const MAX_BAD_PACKETS: u32 = 16;

/// Why a sound file was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum SoundFileError {
    /// The file could not be read.
    #[error("cannot read the file: {0:?}")]
    Io(ErrorKind),
    /// The file is larger than [`MAX_FILE_BYTES`].
    #[error("the file is larger than {MAX_FILE_BYTES} bytes")]
    TooLarge,
    /// The sound is longer than [`MAX_SECONDS`].
    #[error("the sound is longer than {MAX_SECONDS} seconds")]
    TooLong,
    /// The sound has no sample.
    #[error("the sound is empty")]
    Empty,
    /// The file ends before the sound does, or before a header is complete.
    #[error("the file is truncated")]
    Truncated,
    /// The content is not a supported sound format.
    #[error("unsupported sound format: {0}")]
    Unsupported(&'static str),
    /// The sample rate is outside `MIN_RATE..=MAX_RATE`.
    #[error("unsupported sample rate {0} Hz")]
    BadRate(u32),
    /// More than two channels, or none.
    #[error("unsupported channel count {0}")]
    BadChannels(u32),
    /// A WAV file refused for another reason.
    #[error("invalid WAV file: {0}")]
    Wav(WavError),
    /// The decoder failed (corrupt stream, or a panic inside it).
    #[error("decoder failure: {0}")]
    Decoder(String),
}

impl From<WavError> for SoundFileError {
    fn from(error: WavError) -> SoundFileError {
        match error {
            WavError::TruncatedData => SoundFileError::Truncated,
            WavError::TooLong => SoundFileError::TooLong,
            WavError::TooLarge => SoundFileError::TooLarge,
            WavError::Empty => SoundFileError::Empty,
            WavError::UnsupportedRate(rate) => SoundFileError::BadRate(rate),
            WavError::UnsupportedChannels(n) => SoundFileError::BadChannels(u32::from(n)),
            other => SoundFileError::Wav(other),
        }
    }
}

/// The format of a file, from its first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Wav,
    Ogg,
    Mp3,
}

/// Looks at the magic bytes (`RIFF....WAVE`, `OggS`, `ID3` or an MPEG frame
/// sync).
fn sniff(bytes: &[u8]) -> Option<Kind> {
    if bytes.get(0..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        Some(Kind::Wav)
    } else if bytes.get(0..4) == Some(b"OggS") {
        Some(Kind::Ogg)
    } else if bytes.get(0..3) == Some(b"ID3")
        || matches!(bytes.get(0..2), Some([0xFF, b]) if b & 0xE0 == 0xE0)
    {
        Some(Kind::Mp3)
    } else {
        None
    }
}

/// Whether the last complete Ogg page of `bytes` carries the end-of-stream
/// flag. A stream cut short has no such page; the demuxer alone cannot tell
/// (it simply ends), so the cut would pass as a shorter sound.
///
/// The chain of pages is walked from the start through `page_segments` and
/// the segment table, so a payload that happens to contain `OggS` is never
/// taken for a header. A page that does not fit in the file ends the walk
/// with "not clean"; bytes that are not a page after a complete one (junk
/// at the end) end it with the flag of the last page seen.
fn ogg_ends_cleanly(bytes: &[u8]) -> bool {
    const HEADER: usize = 27;
    let mut pos = 0_usize;
    let mut last_flags: Option<u8> = None;
    while bytes.get(pos..pos + 4) == Some(b"OggS") {
        let (Some(&flags), Some(&segments)) = (bytes.get(pos + 5), bytes.get(pos + 26)) else {
            return false;
        };
        let table_end = pos + HEADER + usize::from(segments);
        let Some(table) = bytes.get(pos + HEADER..table_end) else {
            return false;
        };
        let payload: usize = table.iter().map(|&n| usize::from(n)).sum();
        let end = table_end + payload;
        if end > bytes.len() {
            return false;
        }
        last_flags = Some(flags);
        pos = end;
    }
    // Byte 5 of the page header is the header type; 0x04 = last page.
    last_flags.is_some_and(|flags| flags & 0x04 != 0)
}

/// Reads and decodes the sound file at `path`.
///
/// The file is opened and read with a cap of [`MAX_FILE_BYTES`] + 1 bytes
/// (it may grow while it is read); a directory, a FIFO or a device is
/// refused before the open so the read cannot block.
///
/// # Errors
/// [`SoundFileError`] when the file cannot be read or is not a good sound.
pub fn decode_file(path: &Path) -> Result<Clip, SoundFileError> {
    let metadata = std::fs::metadata(path).map_err(|e| SoundFileError::Io(e.kind()))?;
    if !metadata.is_file() {
        return Err(SoundFileError::Io(ErrorKind::InvalidInput));
    }
    let file = std::fs::File::open(path).map_err(|e| SoundFileError::Io(e.kind()))?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| SoundFileError::Io(e.kind()))?;
    decode_owned(bytes)
}

/// Decodes a sound file already in memory (WAV, OGG Vorbis or MP3). The
/// bytes are copied; use [`decode_owned`] or [`decode_static`] to avoid it.
///
/// # Errors
/// [`SoundFileError`] for anything that is not a supported, well-formed
/// sound within the limits of this module. Never panics.
pub fn decode_clip(bytes: &[u8]) -> Result<Clip, SoundFileError> {
    decode_data(bytes.to_vec())
}

/// Like [`decode_clip`] for a buffer that is already owned: not copied.
///
/// # Errors
/// As [`decode_clip`].
pub fn decode_owned(bytes: Vec<u8>) -> Result<Clip, SoundFileError> {
    decode_data(bytes)
}

/// Like [`decode_clip`] for bytes that live for the whole program (the
/// embedded packs): the demuxer reads them in place.
///
/// # Errors
/// As [`decode_clip`].
pub fn decode_static(bytes: &'static [u8]) -> Result<Clip, SoundFileError> {
    decode_data(bytes)
}

/// Runs `f` (third-party decoder code on untrusted bytes) so that a panic
/// in it is a refused file, not a crash.
fn guarded<F>(f: F) -> Result<Clip, SoundFileError>
where
    F: FnOnce() -> Result<Clip, SoundFileError>,
{
    catch_unwind(AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err(SoundFileError::Decoder(PANIC_MESSAGE.to_owned())))
}

/// The text of the error that stands for a caught panic.
pub const PANIC_MESSAGE: &str = "the decoder panicked";

fn decode_data<S>(data: S) -> Result<Clip, SoundFileError>
where
    S: AsRef<[u8]> + Send + Sync + 'static,
{
    let bytes = data.as_ref();
    if u64::try_from(bytes.len()).map_or(true, |n| n > MAX_FILE_BYTES) {
        return Err(SoundFileError::TooLarge);
    }
    if bytes.is_empty() {
        return Err(SoundFileError::Empty);
    }
    match sniff(bytes) {
        Some(Kind::Wav) => parse_wav(bytes).map_err(SoundFileError::from),
        Some(kind) => {
            if kind == Kind::Ogg && !ogg_ends_cleanly(bytes) {
                return Err(SoundFileError::Truncated);
            }
            let extension = if kind == Kind::Ogg { "ogg" } else { "mp3" };
            guarded(move || decode_stream(data, extension))
        }
        None if bytes.len() < 12 => Err(SoundFileError::Truncated),
        None => Err(SoundFileError::Unsupported("unknown file type")),
    }
}

fn decoder_error(error: &SymphoniaError) -> SoundFileError {
    match error {
        SymphoniaError::IoError(e) if e.kind() == ErrorKind::UnexpectedEof => {
            SoundFileError::Truncated
        }
        SymphoniaError::IoError(e) => SoundFileError::Io(e.kind()),
        SymphoniaError::Unsupported(what) => SoundFileError::Unsupported(what),
        SymphoniaError::LimitError(_) => SoundFileError::TooLarge,
        other => SoundFileError::Decoder(other.to_string()),
    }
}

/// Decodes an OGG or MP3 stream packet by packet, counting the samples
/// before keeping them.
fn decode_stream<S>(data: S, extension: &str) -> Result<Clip, SoundFileError>
where
    S: AsRef<[u8]> + Send + Sync + 'static,
{
    let source = MediaSourceStream::new(
        Box::new(Cursor::new(data)),
        MediaSourceStreamOptions::default(),
    );
    let mut hint = Hint::new();
    hint.with_extension(extension);
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            source,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| decoder_error(&e))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or(SoundFileError::Unsupported("no audio track"))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or(SoundFileError::Unsupported("no audio parameters"))?;
    // The header is checked before decoding, but never trusted for sizes.
    let declared_rate = params
        .sample_rate
        .ok_or(SoundFileError::Unsupported("no sample rate"))?;
    check_format(
        declared_rate,
        params
            .channels
            .as_ref()
            .map_or(0, symphonia::core::audio::Channels::count),
    )?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| decoder_error(&e))?;

    let mut samples: Vec<f32> = Vec::new();
    let mut scratch: Vec<f32> = Vec::new();
    let mut spec: Option<(u32, usize)> = None;
    let mut bad_packets = 0_u32;
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            // Cut in the middle of a packet: what came before is the sound.
            Err(SymphoniaError::IoError(e)) if e.kind() == ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(decoder_error(&e)),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buffer = match decoder.decode(&packet) {
            Ok(buffer) => buffer,
            Err(SymphoniaError::DecodeError(_)) => {
                bad_packets += 1;
                if bad_packets > MAX_BAD_PACKETS {
                    return Err(SoundFileError::Decoder(
                        "too many corrupt packets".to_owned(),
                    ));
                }
                continue;
            }
            Err(e) => return Err(decoder_error(&e)),
        };
        let rate = buffer.spec().rate();
        let channels = buffer.spec().channels().count();
        match spec {
            None => {
                check_format(rate, channels)?;
                spec = Some((rate, channels));
            }
            Some(first) if first != (rate, channels) => {
                return Err(SoundFileError::Decoder(
                    "the format changed mid-stream".to_owned(),
                ));
            }
            Some(_) => {}
        }
        let ceiling = sample_ceiling(rate, channels);
        if samples.capacity() == 0 {
            // One allocation for the most the sound may be, so the vector
            // never grows by doubling (a transient twice the ceiling).
            samples
                .try_reserve_exact(ceiling)
                .map_err(|_| SoundFileError::TooLarge)?;
        }
        let count = buffer.samples_interleaved();
        // The ceiling is checked before anything is kept.
        if samples.len().checked_add(count).is_none_or(|n| n > ceiling) {
            return Err(SoundFileError::TooLong);
        }
        scratch.clear();
        scratch.resize(count, 0.0);
        buffer.copy_to_slice_interleaved(&mut scratch);
        samples.extend(scratch.iter().copied().map(clean_sample));
    }
    samples.shrink_to_fit();
    finish_clip(spec, samples)
}

/// A decoded sample limited to the valid range; a non-finite one is silence.
fn clean_sample(sample: f32) -> f32 {
    if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// The clip of the decoded samples, or why there is none.
fn finish_clip(spec: Option<(u32, usize)>, samples: Vec<f32>) -> Result<Clip, SoundFileError> {
    let Some((rate, channels)) = spec else {
        return Err(SoundFileError::Empty);
    };
    if samples.is_empty() {
        return Err(SoundFileError::Empty);
    }
    Ok(Clip {
        sample_rate: rate,
        channels: u16::try_from(channels).map_err(|_| SoundFileError::BadChannels(3))?,
        samples,
    })
}

fn check_format(rate: u32, channels: usize) -> Result<(), SoundFileError> {
    if !(MIN_RATE..=MAX_RATE).contains(&rate) {
        return Err(SoundFileError::BadRate(rate));
    }
    if !(1..=2).contains(&channels) {
        return Err(SoundFileError::BadChannels(
            u32::try_from(channels).unwrap_or(u32::MAX),
        ));
    }
    Ok(())
}

/// The most interleaved samples of a sound of [`MAX_SECONDS`] seconds.
fn sample_ceiling(rate: u32, channels: usize) -> usize {
    usize::try_from(u64::from(MAX_SECONDS) * u64::from(rate))
        .unwrap_or(0)
        .saturating_mul(channels)
}

#[cfg(test)]
mod tests {
    use super::super::wav::encode_pcm16;
    use super::*;

    #[test]
    fn wav_still_goes_through_the_own_reader() {
        let wav = encode_pcm16(8_000, 1, &[0, 16_384, -16_384]);
        let clip = decode_clip(&wav).unwrap();
        assert_eq!((clip.sample_rate, clip.channels), (8_000, 1));
        assert_eq!(clip.samples, parse_wav(&wav).unwrap().samples);
    }

    #[test]
    fn empty_tiny_and_unknown_files_are_errors() {
        assert_eq!(decode_clip(&[]), Err(SoundFileError::Empty));
        assert_eq!(decode_clip(b"Og"), Err(SoundFileError::Truncated));
        assert_eq!(
            decode_clip(b"this is not a sound at all"),
            Err(SoundFileError::Unsupported("unknown file type"))
        );
    }

    #[test]
    fn a_file_over_the_limit_is_refused_before_decoding() {
        let big = vec![0_u8; usize::try_from(MAX_FILE_BYTES).unwrap() + 1];
        assert_eq!(decode_clip(&big), Err(SoundFileError::TooLarge));
    }

    #[test]
    fn the_magic_bytes_choose_the_format() {
        assert_eq!(sniff(b"OggS\0\x02rest"), Some(Kind::Ogg));
        assert_eq!(sniff(b"ID3\x04\0"), Some(Kind::Mp3));
        assert_eq!(sniff(&[0xFF, 0xFB, 0x90]), Some(Kind::Mp3));
        assert_eq!(sniff(b"RIFF\0\0\0\0WAVEfmt "), Some(Kind::Wav));
        assert_eq!(sniff(&[0xFF, 0x10]), None);
        assert_eq!(sniff(b"RIFF\0\0\0\0AVI "), None);
    }

    #[test]
    fn garbage_with_a_stream_magic_is_an_error_not_a_panic() {
        let mut ogg = b"OggS".to_vec();
        ogg.extend_from_slice(&[0x55; 300]);
        assert!(decode_clip(&ogg).is_err());
        let mut mp3 = vec![0xFF, 0xFB];
        mp3.extend_from_slice(&[0x13; 300]);
        assert!(decode_clip(&mp3).is_err());
        assert!(decode_clip(b"ID3\x04\0\0\0\0\0\x10").is_err());
    }

    #[test]
    fn the_end_of_stream_flag_marks_a_clean_ogg() {
        let mut page = vec![0_u8; 40];
        page[..4].copy_from_slice(b"OggS");
        assert!(!ogg_ends_cleanly(&page));
        page[5] = 0x04;
        assert!(ogg_ends_cleanly(&page));
        assert!(!ogg_ends_cleanly(b""));
    }

    #[test]
    fn a_payload_with_the_page_magic_is_not_a_page_header() {
        // Last real page has no end flag; its payload ends with "OggS".
        let mut page = vec![0_u8; 27];
        page[..4].copy_from_slice(b"OggS");
        page[26] = 1; // one segment
        page.push(8); // of 8 bytes
        page.extend_from_slice(b"xxxxOggS");
        assert!(!ogg_ends_cleanly(&page));
        // A decoy header with the flag inside the payload does not count.
        let mut decoy = vec![0_u8; 27];
        decoy[..4].copy_from_slice(b"OggS");
        decoy[5] = 0x04;
        let mut real = vec![0_u8; 27];
        real[..4].copy_from_slice(b"OggS");
        real[26] = 1;
        real.push(u8::try_from(decoy.len()).unwrap());
        real.extend_from_slice(&decoy);
        assert!(!ogg_ends_cleanly(&real));
        // Junk after a last page does not make it truncated.
        let mut ended = vec![0_u8; 27];
        ended[..4].copy_from_slice(b"OggS");
        ended[5] = 0x04;
        ended.extend_from_slice(b"trailing junk");
        assert!(ogg_ends_cleanly(&ended));
        // A page cut in the middle is not clean.
        let mut cut = real.clone();
        cut.pop();
        assert!(!ogg_ends_cleanly(&cut));
    }

    #[test]
    fn a_panic_in_the_decoder_is_a_decoder_error() {
        let result = guarded(|| panic!("boom"));
        assert_eq!(
            result,
            Err(SoundFileError::Decoder(PANIC_MESSAGE.to_owned()))
        );
        assert_eq!(
            guarded(|| Err(SoundFileError::Empty)),
            Err(SoundFileError::Empty)
        );
    }

    #[test]
    fn the_ceiling_follows_the_rate_and_the_channels() {
        assert_eq!(sample_ceiling(8_000, 1), 240_000);
        assert_eq!(sample_ceiling(44_100, 2), 2_646_000);
    }

    #[test]
    fn formats_outside_the_limits_are_refused() {
        assert_eq!(check_format(4_000, 1), Err(SoundFileError::BadRate(4_000)));
        assert_eq!(
            check_format(200_000, 1),
            Err(SoundFileError::BadRate(200_000))
        );
        assert_eq!(check_format(44_100, 3), Err(SoundFileError::BadChannels(3)));
        assert_eq!(check_format(44_100, 0), Err(SoundFileError::BadChannels(0)));
        assert!(check_format(44_100, 2).is_ok());
    }

    #[test]
    fn wav_errors_map_to_the_shared_variants() {
        assert_eq!(
            SoundFileError::from(WavError::TruncatedData),
            SoundFileError::Truncated
        );
        assert_eq!(
            SoundFileError::from(WavError::UnsupportedRate(1)),
            SoundFileError::BadRate(1)
        );
        assert_eq!(
            SoundFileError::from(WavError::MissingData),
            SoundFileError::Wav(WavError::MissingData)
        );
    }

    #[test]
    fn a_missing_file_and_a_folder_are_io_errors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            decode_file(&dir.path().join("none.ogg")),
            Err(SoundFileError::Io(ErrorKind::NotFound))
        ));
        assert!(matches!(
            decode_file(dir.path()),
            Err(SoundFileError::Io(ErrorKind::InvalidInput))
        ));
    }
}
