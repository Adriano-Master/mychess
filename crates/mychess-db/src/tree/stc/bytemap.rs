// Derived from Scid vs PC 4.27, src/filter.cpp (`packBytemap`,
// `unpackBytemap`). Copyright (c) 2000-2002 Shane Hudson; Scid vs PC
// contributors. Scid is distributed under the GNU General Public License (see
// its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! The compression of the filters stored in the cache file (decision D1044).
//!
//! The first byte is a flag: `0` = the rest is a plain copy, `1` = packed.
//! A packed buffer is a stream of 2-bit codes, four per control byte (least
//! significant pair first), interleaved with the data they announce:
//!
//! | code | meaning | data after the control byte |
//! |---|---|---|
//! | 0 | a zero | none |
//! | 1 | the last non-zero literal again | none |
//! | 2 | a run of 9 or more equal bytes | value, length (`0` = 4 bytes follow, big endian) |
//! | 3 | a new non-zero literal | the value |
//!
//! A control byte is reserved in the output as soon as the previous one is
//! full, **before** the data of the code that filled it, so that data bytes
//! follow the reserved slot. The last control byte is always written (even if
//! empty), and the decoder reads the next control byte as soon as it has used
//! the fourth code, which is why it must exist.

use super::StcError;

const FLAG_COPIED: u8 = 0;
const FLAG_PACKED: u8 = 1;

const CODE_ZERO_LITERAL: u32 = 0;
const CODE_PREV_LITERAL: u32 = 1;
const CODE_RUN_LENGTH: u32 = 2;
const CODE_NEW_LITERAL: u32 = 3;

/// The shortest run coded as a run (`MIN_RLE_LENGTH`).
const MIN_RLE_LENGTH: usize = 9;

/// Longest run the encoder writes in one piece (its length is 4 bytes).
const MAX_RUN: usize = u32::MAX as usize;

/// Compresses a filter's bytes (`packBytemap`). The result is never longer
/// than `input.len() + 1`: when packing does not pay off the input is copied
/// behind a zero flag.
///
/// ```
/// use mychess_db::tree::stc::{pack_bytemap, unpack_bytemap};
///
/// let filter = vec![0u8; 100];
/// // flag, control byte (one run), value, length.
/// assert_eq!(pack_bytemap(&filter), [1, 0x02, 0, 100]);
/// assert_eq!(unpack_bytemap(&[1, 0x02, 0, 100], 100)?, filter);
/// # Ok::<(), mychess_db::tree::stc::StcError>(())
/// ```
pub fn pack_bytemap(input: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(input.len().min(1 << 16) + 2);
    // Flag and the first (still empty) control byte.
    out.extend_from_slice(&[FLAG_PACKED, 0]);
    let mut control_idx = 1usize;
    let mut control_data = 0u32;
    let mut control_bits = 8u32;
    let mut prev_literal = 0u8;
    let mut pos = 0usize;

    // `ENCODE_CONTROL_BITS` of the Scid.
    let mut encode = |out: &mut Vec<u8>, bits: u32| {
        control_data >>= 2;
        control_data |= bits << 6;
        control_bits -= 2;
        if control_bits == 0 {
            if let Some(slot) = out.get_mut(control_idx) {
                *slot = u8::try_from(control_data & 0xFF).unwrap_or(0);
            }
            control_idx = out.len();
            out.push(0);
            control_data = 0;
            control_bits = 8;
        }
    };

    while pos < input.len() && out.len() <= input.len() {
        let Some(&value) = input.get(pos) else { break };
        let mut run = 1usize;
        while run < MAX_RUN && input.get(pos + run) == Some(&value) {
            run += 1;
        }
        if run >= MIN_RLE_LENGTH {
            encode(&mut out, CODE_RUN_LENGTH);
            pos += run;
            out.push(value);
            if run > 255 {
                out.push(0);
                // `run` is capped at `u32::MAX` above.
                out.extend_from_slice(&u32::try_from(run).unwrap_or(u32::MAX).to_be_bytes());
            } else {
                out.push(u8::try_from(run).unwrap_or(u8::MAX));
            }
        } else if value == 0 {
            encode(&mut out, CODE_ZERO_LITERAL);
            pos += 1;
        } else if value == prev_literal {
            encode(&mut out, CODE_PREV_LITERAL);
            pos += 1;
        } else {
            encode(&mut out, CODE_NEW_LITERAL);
            pos += 1;
            prev_literal = value;
            out.push(value);
        }
    }

    // Flush the pending control bits (right-aligned in the byte, as the
    // decoder shifts them out from the bottom).
    control_data >>= control_bits;
    if let Some(slot) = out.get_mut(control_idx) {
        *slot = u8::try_from(control_data & 0xFF).unwrap_or(0);
    }

    if out.len() > input.len() {
        let mut copy = Vec::with_capacity(input.len() + 1);
        copy.push(FLAG_COPIED);
        copy.extend_from_slice(input);
        return copy;
    }
    out
}

/// Decompresses a buffer made by [`pack_bytemap`] into exactly `out_len`
/// bytes (`unpackBytemap`).
///
/// # Errors
///
/// [`StcError::Corrupt`] if the flag is unknown, the input ends early or has
/// bytes left over, or a run does not fit in `out_len`. As in the Scid, the
/// unused 2-bit codes at the end of the last control byte read as zeros, so
/// an `out_len` a little larger than the real size can still succeed when the
/// last control byte has room.
pub fn unpack_bytemap(input: &[u8], out_len: usize) -> Result<Vec<u8>, StcError> {
    let mut out = Vec::new();
    out.try_reserve_exact(out_len)
        .map_err(|_| StcError::corrupt("filter too large"))?;
    decode(input, out_len, Some(&mut out))?;
    Ok(out)
}

/// Validates a compressed buffer like [`unpack_bytemap`] without producing the
/// output; used when reading a file, so a corrupt filter is found at once and
/// no memory is spent on the bytes.
pub(super) fn check_bytemap(input: &[u8], out_len: usize) -> Result<(), StcError> {
    decode(input, out_len, None)
}

fn decode(input: &[u8], out_len: usize, mut out: Option<&mut Vec<u8>>) -> Result<(), StcError> {
    let corrupt = |reason| StcError::corrupt(reason);
    let (&flag, rest) = input
        .split_first()
        .ok_or_else(|| corrupt("empty compressed filter"))?;
    if flag == FLAG_COPIED {
        // `out_len` must be one shorter than the input.
        if rest.len() != out_len {
            return Err(corrupt("copied filter has the wrong length"));
        }
        if let Some(out) = out.as_mut() {
            out.extend_from_slice(rest);
        }
        return Ok(());
    }
    if flag != FLAG_PACKED {
        return Err(corrupt("unknown compressed filter flag"));
    }

    let truncated = || corrupt("compressed filter ends early");
    // Next unread byte of `input`; the first one is the first control byte.
    let mut at = 1usize;
    let mut control = u32::from(*input.get(at).ok_or_else(truncated)?);
    at += 1;
    let mut control_bits = 8u32;
    let mut prev_literal = 0u8;
    let mut left = out_len;

    while left > 0 {
        let code = control & 3;
        control >>= 2;
        control_bits -= 2;
        if control_bits == 0 {
            control = u32::from(*input.get(at).ok_or_else(truncated)?);
            at += 1;
            control_bits = 8;
        }
        match code {
            CODE_ZERO_LITERAL => {
                push(&mut out, 0, 1);
                left -= 1;
            }
            CODE_PREV_LITERAL => {
                push(&mut out, prev_literal, 1);
                left -= 1;
            }
            CODE_RUN_LENGTH => {
                let value = *input.get(at).ok_or_else(truncated)?;
                let short = *input.get(at + 1).ok_or_else(truncated)?;
                at += 2;
                let length = if short == 0 {
                    let long = input.get(at..at + 4).ok_or_else(truncated)?;
                    at += 4;
                    long.iter()
                        .fold(0usize, |acc, &b| (acc << 8) | usize::from(b))
                } else {
                    usize::from(short)
                };
                left = left
                    .checked_sub(length)
                    .ok_or_else(|| corrupt("run longer than the filter"))?;
                push(&mut out, value, length);
            }
            _ => {
                prev_literal = *input.get(at).ok_or_else(truncated)?;
                at += 1;
                push(&mut out, prev_literal, 1);
                left -= 1;
            }
        }
    }
    // Both buffers must be consumed exactly.
    if at != input.len() {
        return Err(corrupt("compressed filter has trailing bytes"));
    }
    Ok(())
}

fn push(out: &mut Option<&mut Vec<u8>>, value: u8, count: usize) {
    if let Some(out) = out.as_mut() {
        out.resize(out.len() + count, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(input: &[u8]) -> Vec<u8> {
        let packed = pack_bytemap(input);
        assert!(packed.len() <= input.len() + 1);
        assert_eq!(
            unpack_bytemap(&packed, input.len()).as_deref().ok(),
            Some(input)
        );
        assert!(check_bytemap(&packed, input.len()).is_ok());
        packed
    }

    #[test]
    fn all_zero_is_one_run() {
        // flag, control (run in the first code), value, length.
        assert_eq!(roundtrip(&[0; 100]), [1, 0x02, 0, 100]);
    }

    #[test]
    fn long_run_uses_four_length_bytes() {
        let packed = roundtrip(&vec![1u8; 300]);
        assert_eq!(packed, [1, 0x02, 1, 0, 0, 0, 1, 44]);
    }

    #[test]
    fn short_runs_are_literals_and_may_copy() {
        // Eight changing literals cost more than they save: plain copy.
        let packed = roundtrip(&[1, 2, 1, 2, 1, 2, 1, 2]);
        assert_eq!(packed[0], FLAG_COPIED);
        // Nine equal bytes are the shortest run.
        assert_eq!(roundtrip(&[5; 9]), [1, 0x02, 5, 9]);
        // Eight are literals: new, then seven "same as previous".
        // The last control byte is always written, even when empty.
        assert_eq!(roundtrip(&[5; 8]), [1, 0x57, 5, 0x55, 0x00]);
    }

    #[test]
    fn literals_interleave_with_control_bytes() {
        // zero, new(1), prev, new(3) fill the first control byte:
        // 0b11_01_11_00 = 0xDC. The second control byte is reserved *before*
        // the literal 3 of the fourth code, so it sits between 1 and 3.
        let input = [0, 1, 1, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(roundtrip(&input), [1, 0xDC, 1, 0x02, 3, 0, 12]);
    }

    #[test]
    fn empty_input_is_a_bare_flag() {
        assert_eq!(roundtrip(&[]), [FLAG_COPIED]);
    }

    #[test]
    fn rejects_wrong_lengths_and_flags() {
        let packed = pack_bytemap(&[0; 100]);
        assert!(unpack_bytemap(&packed, 99).is_err());
        // A copied buffer must be exactly one byte longer than the output.
        assert!(unpack_bytemap(&[FLAG_COPIED, 1, 2], 3).is_err());
        assert!(unpack_bytemap(&[FLAG_COPIED, 1, 2], 1).is_err());
        let mut extra = packed.clone();
        extra.push(0);
        assert!(unpack_bytemap(&extra, 100).is_err());
        assert!(unpack_bytemap(&packed[..packed.len() - 1], 100).is_err());
        assert!(unpack_bytemap(&[], 0).is_err());
        assert!(unpack_bytemap(&[2, 0, 0], 1).is_err());
        assert!(unpack_bytemap(&[FLAG_COPIED, 1, 2], 3).is_err());
    }

    #[test]
    fn mixed_filter_round_trips() {
        let mut input = vec![0u8; 5000];
        for (i, b) in input.iter_mut().enumerate() {
            if i % 7 == 0 || (1000..1100).contains(&i) {
                *b = u8::try_from(1 + i % 3).unwrap_or(1);
            }
        }
        roundtrip(&input);
    }
}
