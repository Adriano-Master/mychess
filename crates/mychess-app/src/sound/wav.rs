//! A small reader of PCM WAV files for the sounds folder of the user (D2806).
//!
//! Only what the Scid sounds need: PCM of 8, 16, 24 or 32 bits and float of
//! 32 bits, mono or stereo, between [`MIN_RATE`] and [`MAX_RATE`] Hz, up to
//! [`MAX_SECONDS`] seconds. Compressed formats are refused, not decoded. The
//! input is untrusted: every length is checked and nothing panics.

use thiserror::Error;

use super::bank::Clip;

/// The lowest sample rate accepted.
pub const MIN_RATE: u32 = 8_000;
/// The highest sample rate accepted.
pub const MAX_RATE: u32 = 192_000;
/// The longest sound accepted, in seconds.
pub const MAX_SECONDS: u32 = 30;
/// The largest file read, in bytes.
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// Why a WAV file was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum WavError {
    /// Not a RIFF/WAVE file, or cut before the end of a header.
    #[error("not a WAV file or truncated header")]
    NotWav,
    /// There is no `fmt ` chunk before the data.
    #[error("missing fmt chunk")]
    MissingFormat,
    /// There is no `data` chunk.
    #[error("missing data chunk")]
    MissingData,
    /// The encoding is not PCM or float (for example ADPCM or MP3 in WAV).
    #[error("unsupported encoding (format tag {0})")]
    UnsupportedEncoding(u16),
    /// The number of bits per sample is not supported.
    #[error("unsupported sample size of {0} bits")]
    UnsupportedBits(u16),
    /// More than two channels, or none.
    #[error("unsupported channel count {0}")]
    UnsupportedChannels(u16),
    /// The sample rate is outside the accepted range.
    #[error("unsupported sample rate {0} Hz")]
    UnsupportedRate(u32),
    /// The data chunk says it has more bytes than the file has.
    #[error("the data chunk is larger than the file")]
    TruncatedData,
    /// The sound is longer than [`MAX_SECONDS`].
    #[error("the sound is longer than {MAX_SECONDS} seconds")]
    TooLong,
    /// The file is larger than [`MAX_FILE_BYTES`].
    #[error("the file is larger than {MAX_FILE_BYTES} bytes")]
    TooLarge,
    /// The sound has no sample.
    #[error("the sound is empty")]
    Empty,
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let b = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

struct Format {
    float: bool,
    channels: u16,
    rate: u32,
    bits: u16,
}

fn parse_format(chunk: &[u8]) -> Result<Format, WavError> {
    let (Some(tag), Some(channels), Some(rate), Some(bits)) = (
        u16_at(chunk, 0),
        u16_at(chunk, 2),
        u32_at(chunk, 4),
        u16_at(chunk, 14),
    ) else {
        return Err(WavError::NotWav);
    };
    // WAVE_FORMAT_EXTENSIBLE: the real tag opens the sub-format GUID.
    let tag = if tag == 0xFFFE {
        u16_at(chunk, 24).ok_or(WavError::NotWav)?
    } else {
        tag
    };
    let float = match tag {
        1 => false,
        3 => true,
        other => return Err(WavError::UnsupportedEncoding(other)),
    };
    let bits_ok = if float {
        bits == 32
    } else {
        matches!(bits, 8 | 16 | 24 | 32)
    };
    if !bits_ok {
        return Err(WavError::UnsupportedBits(bits));
    }
    if !(1..=2).contains(&channels) {
        return Err(WavError::UnsupportedChannels(channels));
    }
    if !(MIN_RATE..=MAX_RATE).contains(&rate) {
        return Err(WavError::UnsupportedRate(rate));
    }
    Ok(Format {
        float,
        channels,
        rate,
        bits,
    })
}

fn decode(format: &Format, data: &[u8]) -> Vec<f32> {
    let width = usize::from(format.bits / 8);
    data.chunks_exact(width)
        .map(|b| match (format.float, format.bits) {
            (true, _) => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            (false, 8) => (f32::from(b[0]) - 128.0) / 128.0,
            (false, 16) => f32::from(i16::from_le_bytes([b[0], b[1]])) / 32_768.0,
            (false, 24) => {
                // Sign-extended through the top byte.
                let v = i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8;
                // 24-bit values fit the `f32` mantissa exactly.
                #[allow(clippy::cast_precision_loss)]
                let f = v as f32;
                f / 8_388_608.0
            }
            // Through `f64` (exact for 32 bits); the final narrowing is only
            // a loss of precision, in the range [-1, 1].
            #[allow(clippy::cast_possible_truncation)]
            _ => (f64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])) / 2_147_483_648.0) as f32,
        })
        .map(|v| {
            if v.is_finite() {
                v.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        })
        .collect()
}

/// Reads a WAV file already in memory.
///
/// # Errors
/// A [`WavError`] for anything that is not a supported, well-formed sound.
pub fn parse_wav(bytes: &[u8]) -> Result<Clip, WavError> {
    if u64::try_from(bytes.len()).map_or(true, |n| n > MAX_FILE_BYTES) {
        return Err(WavError::TooLarge);
    }
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(WavError::NotWav);
    }
    let mut format = None;
    let mut at = 12_usize;
    while let (Some(id), Some(size)) = (
        at.checked_add(4).and_then(|e| bytes.get(at..e)),
        at.checked_add(4).and_then(|p| u32_at(bytes, p)),
    ) {
        let body = at.checked_add(8).ok_or(WavError::NotWav)?;
        let size = usize::try_from(size).map_err(|_| WavError::TruncatedData)?;
        if id == b"data" {
            let format = format.ok_or(WavError::MissingFormat)?;
            let end = body.checked_add(size).ok_or(WavError::TruncatedData)?;
            let data = bytes.get(body..end).ok_or(WavError::TruncatedData)?;
            return finish(&format, data);
        }
        if id == b"fmt " {
            let end = body.checked_add(size).ok_or(WavError::NotWav)?;
            let chunk = bytes.get(body..end).ok_or(WavError::NotWav)?;
            format = Some(parse_format(chunk)?);
        }
        // Chunks are padded to an even size.
        at = body
            .checked_add(size)
            .and_then(|n| n.checked_add(size & 1))
            .ok_or(WavError::NotWav)?;
    }
    Err(if format.is_some() {
        WavError::MissingData
    } else {
        WavError::MissingFormat
    })
}

fn finish(format: &Format, data: &[u8]) -> Result<Clip, WavError> {
    let samples = decode(format, data);
    if samples.is_empty() {
        return Err(WavError::Empty);
    }
    let frames = samples.len() / usize::from(format.channels);
    let limit = usize::try_from(u64::from(MAX_SECONDS) * u64::from(format.rate)).unwrap_or(0);
    if frames > limit {
        return Err(WavError::TooLong);
    }
    // Whole frames only.
    let keep = frames * usize::from(format.channels);
    let mut samples = samples;
    samples.truncate(keep);
    if samples.is_empty() {
        return Err(WavError::Empty);
    }
    Ok(Clip {
        sample_rate: format.rate,
        channels: format.channels,
        samples,
    })
}

/// Test helper: a canonical WAV file of the given samples.
#[cfg(test)]
pub(crate) fn encode_pcm16(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
    let data_len = u32::try_from(samples.len() * 2).unwrap_or(0);
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(tag: u16, bits: u16, channels: u16, rate: u32, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(
            &(36 + u32::try_from(data.len()).expect("test data fits")).to_le_bytes(),
        );
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(
            &u32::try_from(data.len())
                .expect("test data fits")
                .to_le_bytes(),
        );
        out.extend_from_slice(data);
        out
    }

    #[test]
    fn reads_16_bit_mono_and_stereo() {
        let clip = parse_wav(&encode_pcm16(22_050, 1, &[0, 16_384, -16_384, 32_767])).unwrap();
        assert_eq!((clip.sample_rate, clip.channels), (22_050, 1));
        assert_eq!(clip.samples.len(), 4);
        assert!((clip.samples[1] - 0.5).abs() < 1e-4);
        assert!((clip.samples[2] + 0.5).abs() < 1e-4);
        let stereo = parse_wav(&encode_pcm16(44_100, 2, &[1, 2, 3, 4])).unwrap();
        assert_eq!(stereo.channels, 2);
        assert_eq!(stereo.samples.len(), 4);
    }

    #[test]
    fn reads_8_24_32_bit_and_float() {
        let c = parse_wav(&wav(1, 8, 1, 8_000, &[128, 255, 0])).unwrap();
        assert!(c.samples[0].abs() < 1e-6);
        assert!(c.samples[1] > 0.99 && c.samples[2] <= -1.0);
        // 24 bit: 0x400000 is one half; 0xC00000 is minus one half.
        let c = parse_wav(&wav(1, 24, 1, 8_000, &[0, 0, 0x40, 0, 0, 0xC0])).unwrap();
        assert!((c.samples[0] - 0.5).abs() < 1e-6);
        assert!((c.samples[1] + 0.5).abs() < 1e-6);
        let c = parse_wav(&wav(1, 32, 1, 8_000, &0x4000_0000i32.to_le_bytes())).unwrap();
        assert!((c.samples[0] - 0.5).abs() < 1e-6);
        let c = parse_wav(&wav(3, 32, 1, 8_000, &0.25f32.to_le_bytes())).unwrap();
        assert!((c.samples[0] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn float_garbage_becomes_silence_or_is_limited() {
        let mut data = f32::NAN.to_le_bytes().to_vec();
        data.extend_from_slice(&9.0f32.to_le_bytes());
        let c = parse_wav(&wav(3, 32, 1, 8_000, &data)).unwrap();
        assert_eq!(c.samples, vec![0.0, 1.0]);
    }

    #[test]
    fn refuses_what_it_does_not_understand() {
        assert_eq!(parse_wav(b""), Err(WavError::NotWav));
        assert_eq!(parse_wav(b"RIFF\0\0\0\0WAVE"), Err(WavError::MissingFormat));
        assert_eq!(
            parse_wav(&wav(2, 4, 1, 8_000, &[0, 0])),
            Err(WavError::UnsupportedEncoding(2))
        );
        assert_eq!(
            parse_wav(&wav(1, 12, 1, 8_000, &[0, 0])),
            Err(WavError::UnsupportedBits(12))
        );
        assert_eq!(
            parse_wav(&wav(1, 16, 3, 8_000, &[0, 0])),
            Err(WavError::UnsupportedChannels(3))
        );
        assert_eq!(
            parse_wav(&wav(1, 16, 1, 100, &[0, 0])),
            Err(WavError::UnsupportedRate(100))
        );
        assert_eq!(parse_wav(&wav(1, 16, 1, 8_000, &[])), Err(WavError::Empty));
    }

    #[test]
    fn refuses_truncated_files_without_panicking() {
        let good = encode_pcm16(8_000, 1, &[1, 2, 3, 4]);
        for cut in 0..good.len() {
            drop(parse_wav(&good[..cut]));
        }
        // The data chunk claims more than the file has.
        let mut lying = good.clone();
        lying[40..44].copy_from_slice(&1000u32.to_le_bytes());
        assert_eq!(parse_wav(&lying), Err(WavError::TruncatedData));
    }

    #[test]
    fn refuses_long_and_large_sounds() {
        let secs = (MAX_SECONDS + 1) * MIN_RATE;
        let data = vec![0u8; secs as usize];
        assert_eq!(
            parse_wav(&wav(1, 8, 1, MIN_RATE, &data)),
            Err(WavError::TooLong)
        );
        let huge = vec![0u8; usize::try_from(MAX_FILE_BYTES + 1).expect("fits")];
        assert_eq!(parse_wav(&huge), Err(WavError::TooLarge));
    }

    #[test]
    fn skips_unknown_chunks_and_pads_odd_ones() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF\0\0\0\0WAVE");
        bytes.extend_from_slice(b"LIST");
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&[1, 2, 3, 0]); // padded
        let rest = encode_pcm16(8_000, 1, &[100, 200]);
        bytes.extend_from_slice(&rest[12..]);
        let clip = parse_wav(&bytes).unwrap();
        assert_eq!(clip.samples.len(), 2);
    }

    #[test]
    fn extensible_format_uses_the_sub_format() {
        // fmt chunk of 40 bytes: tag 0xFFFE, sub-format tag 1 at offset 24.
        let mut fmt = vec![0u8; 40];
        fmt[0..2].copy_from_slice(&0xFFFEu16.to_le_bytes());
        fmt[2..4].copy_from_slice(&1u16.to_le_bytes());
        fmt[4..8].copy_from_slice(&8_000u32.to_le_bytes());
        fmt[14..16].copy_from_slice(&16u16.to_le_bytes());
        fmt[24..26].copy_from_slice(&1u16.to_le_bytes());
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF\0\0\0\0WAVEfmt ");
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(&fmt);
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&1000i16.to_le_bytes());
        assert_eq!(parse_wav(&bytes).unwrap().samples.len(), 1);
    }
}
