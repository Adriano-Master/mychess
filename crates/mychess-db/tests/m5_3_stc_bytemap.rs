//! Contract tests of the filter compression of the `.stc` file (M5-3a, D1044
//! and D1059a of `docs/specs/m5-3-cache-stc.md`). The expected vectors were
//! worked out by hand from `packBytemap` / `unpackBytemap` in
//! `reference/scid_vs_pc-4.27/src/filter.cpp` (lines 320-520); the proptests
//! compare the production code with a direct port of that C code.
#![allow(
    clippy::needless_pass_by_value,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::too_many_lines
)]

use mychess_db::tree::stc::{StcError, pack_bytemap, unpack_bytemap};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;

fn rep(value: u8, n: usize) -> Vec<u8> {
    vec![value; n]
}

fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
    parts.concat()
}

// ------------------------------------------------------------ hand vectors

#[rstest]
// All zero: a single run (flag, control 0b10, value, length).
#[case::all_zero(rep(0, 100), vec![1, 0x02, 0, 100])]
// All one (the filter "everything in"): same shape.
#[case::all_one(rep(1, 100), vec![1, 0x02, 1, 100])]
// The long form: length 0 then 4 big-endian bytes (rle > 255).
#[case::run_300(rep(1, 300), vec![1, 0x02, 1, 0, 0, 0, 1, 44])]
#[case::run_255_is_short(rep(7, 255), vec![1, 0x02, 7, 255])]
#[case::run_256_is_long(rep(7, 256), vec![1, 0x02, 7, 0, 0, 0, 1, 0])]
#[case::run_70000(rep(9, 70000), vec![1, 0x02, 9, 0, 0, 1, 17, 112])]
// A run of 9 is the shortest run.
#[case::run_9(rep(5, 9), vec![1, 0x02, 5, 9])]
// Eight equal bytes are literals: new, then seven "same as previous". The
// last control byte is written even when empty.
#[case::run_8_literals(rep(5, 8), vec![1, 0x57, 5, 0x55, 0x00])]
#[case::eight_zeros(rep(0, 8), vec![1, 0, 0, 0])]
// Control bytes are interleaved with the data: zeros, new literal, three
// repeats, zeros; the empty last control byte is present.
#[case::mixed(
    cat(&[rep(0, 4), rep(2, 4), rep(0, 4)]),
    vec![1, 0x00, 0x57, 2, 0x00, 0x00]
)]
// A run followed by two zeros: the codes (2, 0, 0) leave the byte padded.
#[case::run_then_zeros(cat(&[rep(7, 10), rep(0, 2)]), vec![1, 0x02, 7, 10])]
// A run does not change the "previous literal": 5, 6 x 9, 5 -> prev code.
#[case::run_keeps_previous_literal(
    cat(&[vec![5], rep(6, 9), vec![5]]),
    vec![1, 0x1B, 5, 6, 9]
)]
// A run in the 3rd code, then a 4th code: a new control byte is reserved
// after the run data.
#[case::run_then_one_zero(
    cat(&[rep(0, 2), rep(4, 9), vec![0]]),
    vec![1, 0x20, 4, 9, 0x00]
)]
// Packed size equal to the input size is still packed (copy only when
// strictly bigger).
#[case::equal_size_is_packed(vec![1, 0, 0, 0], vec![1, 0x03, 1, 0x00])]
#[case::previous_literal_after_zero(vec![7, 0, 7], vec![1, 0x13, 7])]
#[case::five_ones(rep(1, 5), vec![1, 0x57, 1, 0x01])]
// Packing does not pay: flag 0 and CompressedLength = size + 1.
#[case::copy_when_bigger(vec![1, 2, 1, 2, 1, 2, 1, 2], vec![0, 1, 2, 1, 2, 1, 2, 1, 2])]
#[case::empty(vec![], vec![0])]
fn pack_matches_the_hand_computed_vector(#[case] input: Vec<u8>, #[case] packed: Vec<u8>) {
    assert_eq!(pack_bytemap(&input), packed);
    assert_eq!(
        unpack_bytemap(&packed, input.len()).unwrap(),
        input,
        "unpack of the vector"
    );
}

#[test]
fn copied_form_has_length_plus_one() {
    let input = vec![3u8, 1, 4, 1, 5, 9, 2, 6];
    let packed = pack_bytemap(&input);
    assert_eq!(packed[0], 0);
    assert_eq!(packed.len(), input.len() + 1);
    assert_eq!(&packed[1..], &input[..]);
}

// ------------------------------------------------------------ unpack errors

fn corrupt<T: std::fmt::Debug>(r: Result<T, StcError>) {
    assert!(matches!(r, Err(StcError::Corrupt { .. })), "got {r:?}");
}

#[rstest]
#[case::empty_input(vec![], 0)]
#[case::unknown_flag(vec![2, 0, 0], 2)]
#[case::copy_too_short(vec![0, 1, 2], 3)]
#[case::copy_too_long(vec![0, 1, 2], 1)]
#[case::copy_without_data_for_one(vec![0], 1)]
// D1059a: the Scid reads past the buffer here; this is Corrupt.
#[case::flag_only(vec![1], 0)]
#[case::flag_only_nonzero_out(vec![1], 5)]
#[case::run_longer_than_output(vec![1, 0x02, 0, 100], 99)]
#[case::input_with_trailing_byte(vec![1, 0x02, 0, 100, 0], 100)]
#[case::run_without_length(vec![1, 0x02, 0], 100)]
#[case::long_run_cut(vec![1, 0x02, 1, 0, 0, 0, 1], 300)]
#[case::huge_run_in_small_output(vec![1, 0x02, 1, 0, 255, 255, 255, 255], 10)]
#[case::new_literal_without_data(vec![1, 0x03], 1)]
#[case::missing_next_control_byte(vec![1, 0x00], 5)]
fn unpack_rejects(#[case] input: Vec<u8>, #[case] out_len: usize) {
    corrupt(unpack_bytemap(&input, out_len));
}

#[test]
fn unpack_requires_exact_output_length_for_a_copied_filter() {
    let packed = [0u8, 1, 2];
    assert_eq!(unpack_bytemap(&packed, 2).unwrap(), vec![1, 2]);
    corrupt(unpack_bytemap(&packed, 1));
    corrupt(unpack_bytemap(&packed, 3));
}

#[test]
fn unpack_accepts_a_slightly_longer_output_when_the_last_control_byte_has_room() {
    // D1059a: unused 2-bit codes of the last control byte read as zeros (the
    // Scid does the same). [1, 2, 0, 100] is a run of 100 zeros plus three
    // padding codes.
    let packed = [1u8, 0x02, 0, 100];
    assert_eq!(unpack_bytemap(&packed, 100).unwrap(), rep(0, 100));
    assert_eq!(unpack_bytemap(&packed, 101).unwrap(), rep(0, 101));
    assert_eq!(unpack_bytemap(&packed, 102).unwrap(), rep(0, 102));
    // The 4th code needs the next control byte, which does not exist.
    corrupt(unpack_bytemap(&packed, 103));
}

#[test]
fn unpack_with_an_absurd_output_length_is_an_error_not_an_abort() {
    corrupt(unpack_bytemap(&[1, 0x02, 0, 100], usize::MAX));
    corrupt(unpack_bytemap(&[0, 1], usize::MAX));
}

// ------------------------------------------------------------ the Scid model

/// A direct port of `packBytemap` (filter.cpp 320-407).
fn model_pack(input: &[u8]) -> Vec<u8> {
    struct Enc {
        out: Vec<u8>,
        control_idx: usize,
        control_data: u32,
        control_bits: u32,
        out_bytes: usize,
    }
    impl Enc {
        fn code(&mut self, bits: u32) {
            self.control_data >>= 2;
            self.control_data |= bits << 6;
            self.control_bits -= 2;
            if self.control_bits == 0 {
                self.out[self.control_idx] = (self.control_data & 0xFF) as u8;
                self.control_idx = self.out.len();
                self.out.push(0);
                self.out_bytes += 1;
                self.control_data = 0;
                self.control_bits = 8;
            }
        }
    }
    let mut e = Enc {
        out: vec![1, 0],
        control_idx: 1,
        control_data: 0,
        control_bits: 8,
        out_bytes: 2,
    };
    let mut prev = 0u8;
    let mut pos = 0usize;
    while pos < input.len() && e.out_bytes <= input.len() {
        let value = input[pos];
        let mut rle = 1usize;
        while pos + rle < input.len() && input[pos + rle] == value {
            rle += 1;
        }
        if rle >= 9 {
            e.code(2);
            pos += rle;
            e.out.push(value);
            if rle > 255 {
                e.out.push(0);
                e.out.extend_from_slice(&(rle as u32).to_be_bytes());
                e.out_bytes += 6;
            } else {
                e.out.push(rle as u8);
                e.out_bytes += 2;
            }
        } else if value == 0 {
            e.code(0);
            pos += 1;
        } else if value == prev {
            e.code(1);
            pos += 1;
        } else {
            e.code(3);
            pos += 1;
            prev = value;
            e.out.push(value);
            e.out_bytes += 1;
        }
    }
    e.control_data >>= e.control_bits;
    e.out[e.control_idx] = (e.control_data & 0xFF) as u8;
    if e.out_bytes > input.len() {
        let mut copy = vec![0];
        copy.extend_from_slice(input);
        return copy;
    }
    e.out.truncate(e.out_bytes);
    e.out
}

#[test]
fn the_model_agrees_with_the_hand_vectors() {
    assert_eq!(model_pack(&rep(5, 8)), vec![1, 0x57, 5, 0x55, 0x00]);
    assert_eq!(
        model_pack(&cat(&[rep(0, 4), rep(2, 4), rep(0, 4)])),
        vec![1, 0x00, 0x57, 2, 0x00, 0x00]
    );
    assert_eq!(model_pack(&rep(1, 300)), vec![1, 0x02, 1, 0, 0, 0, 1, 44]);
}

// ------------------------------------------------------------ proptests

/// Filter-like data: runs of a few values.
fn structured() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(
        (
            prop::sample::select(vec![0u8, 0, 0, 1, 2, 3, 255]),
            1usize..400,
        ),
        0..12,
    )
    .prop_map(|runs| runs.into_iter().flat_map(|(v, n)| vec![v; n]).collect())
}

/// Sparse filter: mostly zeros with a few non-zero values.
fn sparse() -> impl Strategy<Value = Vec<u8>> {
    (
        1usize..3000,
        prop::collection::vec((any::<u16>(), 1u8..=255), 0..40),
    )
        .prop_map(|(len, marks)| {
            let mut v = vec![0u8; len];
            for (i, value) in marks {
                let idx = usize::from(i) % len;
                v[idx] = value;
            }
            v
        })
}

fn any_bytemap() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        prop::collection::vec(any::<u8>(), 0..600),
        prop::collection::vec(0u8..3, 0..600),
        structured(),
        sparse(),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn unpack_of_pack_is_the_identity(input in any_bytemap()) {
        let packed = pack_bytemap(&input);
        prop_assert_eq!(unpack_bytemap(&packed, input.len()).unwrap(), input.clone());
        // Either a copy (flag 0, size + 1) or a packing that is not bigger.
        match packed[0] {
            0 => prop_assert_eq!(packed.len(), input.len() + 1),
            1 => prop_assert!(packed.len() <= input.len()),
            f => prop_assert!(false, "flag {f}"),
        }
    }

    #[test]
    fn pack_equals_the_port_of_the_scid_algorithm(input in any_bytemap()) {
        prop_assert_eq!(pack_bytemap(&input), model_pack(&input));
    }

    #[test]
    fn unpack_of_arbitrary_bytes_never_panics_and_keeps_the_length(
        input in prop::collection::vec(any::<u8>(), 0..64),
        out_len in 0usize..5000,
    ) {
        if let Ok(out) = unpack_bytemap(&input, out_len) {
            prop_assert_eq!(out.len(), out_len);
        }
    }

    #[test]
    fn unpack_of_a_corrupted_packing_never_panics(
        input in structured(),
        at in any::<prop::sample::Index>(),
        value in any::<u8>(),
    ) {
        let mut packed = pack_bytemap(&input);
        let i = at.index(packed.len());
        packed[i] = value;
        if let Ok(out) = unpack_bytemap(&packed, input.len()) {
            prop_assert_eq!(out.len(), input.len());
        }
    }
}
