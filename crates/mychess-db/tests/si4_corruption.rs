//! Corruption tests (spec M3-1, criterion "Arquivos corrompidos"): truncation
//! at every byte of each file and random damage must give `Err` or `Ok`, never a
//! panic, an endless loop or a huge allocation.
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation
)]

#[path = "common/si4_builder.rs"]
mod si4_builder;

use std::io::Cursor;
use std::time::{Duration, Instant};

use mychess_db::si4::Si4Database;
use proptest::prelude::*;
use si4_builder::{
    EntrySpec, IndexHeader, NameRec, Parts, default_names, index_file, names_file_with,
    parts_from_records, rec, rec_fen, rec_full, section, tag_literal,
};

/// A small valid database exercising tags, FEN, variations, NAGs and comments.
fn base() -> Parts {
    let mut tags = tag_literal(b"Note", b"x");
    tags.extend(tag_literal(b"Annotator", b"me"));
    parts_from_records(&[
        rec(&[0xCF, 0xCF, 0x67, 0x22, 0x0F], &[]),
        rec_full(
            &tags,
            0,
            None,
            &[12, 0xCF, 12, 13, 0xBF, 12, 14, 11, 1, 0xCF, 0x0F],
            &[b"start", b"after e4", b"var"],
        ),
        rec_fen(
            "4k3/8/8/8/8/8/8/R3K3 w - - 0 1",
            &[0x1C, 0x01, 0x1E, 0x0F],
            &[],
        ),
    ])
}

/// Opens and reads everything; the result is irrelevant, only no panic and time.
fn exercise(p: &Parts) {
    let started = Instant::now();
    if let Ok(mut db) = Si4Database::from_parts(
        p.index.clone(),
        &p.names,
        Cursor::new(p.sg4.clone()),
        "x.sg4".into(),
    ) {
        for n in 1..=db.len().min(1000) {
            drop(db.entry(n));
            drop(db.game(n));
            drop(db.raw_game(n));
            drop(db.decode_mainline(n));
            drop(db.record_bytes(n).map(<[u8]>::len));
        }
    }
    assert!(started.elapsed() < Duration::from_secs(5), "took too long");
}

#[test]
fn the_base_database_is_valid() {
    let p = base();
    let mut db = Si4Database::from_parts(
        p.index.clone(),
        &p.names,
        Cursor::new(p.sg4.clone()),
        "x.sg4".into(),
    )
    .unwrap();
    for n in 1..=3 {
        db.game(n).unwrap();
    }
}

#[test]
fn truncating_the_index_at_every_byte_is_an_error() {
    let p = base();
    for len in 0..p.index.len() {
        let r = Si4Database::from_parts(
            p.index[..len].to_vec(),
            &p.names,
            Cursor::new(p.sg4.clone()),
            "x.sg4".into(),
        );
        assert!(r.is_err(), "index cut at {len}");
    }
}

#[test]
fn truncating_the_names_at_every_byte_is_an_error() {
    let p = base();
    for len in 0..p.names.len() {
        let r = Si4Database::from_parts(
            p.index.clone(),
            &p.names[..len],
            Cursor::new(p.sg4.clone()),
            "x.sg4".into(),
        );
        assert!(r.is_err(), "names cut at {len}");
    }
}

#[test]
fn truncating_the_games_file_at_every_byte_fails_only_the_affected_games() {
    let p = base();
    let full: Vec<bool> = {
        let mut db = Si4Database::from_parts(
            p.index.clone(),
            &p.names,
            Cursor::new(p.sg4.clone()),
            "x.sg4".into(),
        )
        .unwrap();
        (1..=3).map(|n| db.game(n).is_ok()).collect()
    };
    assert_eq!(full, vec![true; 3]);
    let mut bounds = Vec::new();
    let mut end = 0usize;
    for n in 1..=3u32 {
        let probe = Si4Database::from_parts(
            p.index.clone(),
            &p.names,
            Cursor::new(p.sg4.clone()),
            "x.sg4".into(),
        )
        .unwrap();
        end = end.max((probe.entry(n).unwrap().offset + probe.entry(n).unwrap().length) as usize);
        bounds.push(end);
    }
    for len in 0..p.sg4.len() {
        let mut db = Si4Database::from_parts(
            p.index.clone(),
            &p.names,
            Cursor::new(p.sg4[..len].to_vec()),
            "x.sg4".into(),
        )
        .unwrap();
        for (i, bound) in bounds.iter().enumerate() {
            let n = u32::try_from(i + 1).unwrap();
            assert_eq!(db.game(n).is_ok(), len >= *bound, "len {len} game {n}");
        }
    }
}

#[test]
fn huge_counts_in_tiny_files_are_cheap_errors() {
    let started = Instant::now();
    // numGames at the maximum with a one-entry index.
    let h = IndexHeader {
        num_games: Some(0xFF_FFFF),
        ..IndexHeader::default()
    };
    let index = index_file(&h, &[EntrySpec::new(0, 5).encode()]);
    assert!(
        Si4Database::from_parts(index, &default_names(), Cursor::new(vec![0; 8]), "x".into())
            .is_err()
    );
    // numNames at the maximum in each section with a tiny body.
    for kind in 0..4usize {
        let mut counts = [1u32, 1, 1, 1];
        counts[kind] = 0xFF_FFFF;
        let sections: [Vec<NameRec>; 4] = [
            section(&["a"]),
            section(&["b"]),
            section(&["c"]),
            section(&["d"]),
        ];
        let names = names_file_with(&sections, [0xFF_FFFF; 4], Some(counts));
        assert!(
            Si4Database::from_parts(
                index_file(&IndexHeader::default(), &[]),
                &names,
                Cursor::new(Vec::new()),
                "x".into()
            )
            .is_err(),
            "kind {kind}"
        );
    }
    assert!(started.elapsed() < Duration::from_secs(2));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn random_byte_changes_in_the_index_never_panic(
        edits in proptest::collection::vec((0usize..400, any::<u8>()), 1..6),
    ) {
        let mut p = base();
        for (at, v) in edits {
            let i = at % p.index.len();
            p.index[i] = v;
        }
        exercise(&p);
    }

    #[test]
    fn random_byte_changes_in_the_names_never_panic(
        edits in proptest::collection::vec((0usize..400, any::<u8>()), 1..6),
    ) {
        let mut p = base();
        for (at, v) in edits {
            let i = at % p.names.len();
            p.names[i] = v;
        }
        exercise(&p);
    }

    #[test]
    fn random_byte_changes_in_the_records_never_panic(
        edits in proptest::collection::vec((0usize..400, any::<u8>()), 1..6),
    ) {
        let mut p = base();
        for (at, v) in edits {
            let i = at % p.sg4.len();
            p.sg4[i] = v;
        }
        exercise(&p);
    }

    #[test]
    fn hostile_header_fields_with_a_valid_body_never_panic(
        num_games in 0u32..=0xFF_FFFF,
        at in 8usize..182,
        v in any::<u8>(),
    ) {
        let mut p = base();
        p.index[14..17].copy_from_slice(&num_games.to_be_bytes()[1..]);
        p.index[at] = v;
        exercise(&p);
    }

    #[test]
    fn hostile_name_headers_never_panic(
        counts in proptest::array::uniform4(0u32..=0xFF_FFFF),
        freqs in proptest::array::uniform4(0u32..=0xFF_FFFF),
    ) {
        let mut p = base();
        for (i, c) in counts.iter().enumerate() {
            p.names[12 + 3 * i..15 + 3 * i].copy_from_slice(&c.to_be_bytes()[1..]);
        }
        for (i, f) in freqs.iter().enumerate() {
            p.names[24 + 3 * i..27 + 3 * i].copy_from_slice(&f.to_be_bytes()[1..]);
        }
        exercise(&p);
    }

    #[test]
    fn entry_offsets_and_lengths_never_panic(
        offset in any::<u32>(),
        length in 0u32..=131_071,
        spare in any::<bool>(),
    ) {
        let mut spec = EntrySpec::new(offset, length);
        spec.spare = spare;
        let p = Parts {
            index: index_file(&IndexHeader::default(), &[spec.encode()]),
            names: default_names(),
            sg4: base().sg4,
        };
        exercise(&p);
    }

    #[test]
    fn pure_random_files_never_panic(
        index in proptest::collection::vec(any::<u8>(), 0..600),
        names in proptest::collection::vec(any::<u8>(), 0..600),
        sg4 in proptest::collection::vec(any::<u8>(), 0..600),
    ) {
        exercise(&Parts { index, names, sg4 });
    }

    #[test]
    fn random_streams_after_a_valid_prelude_never_panic(
        stream in proptest::collection::vec(any::<u8>(), 0..300),
        comments in proptest::collection::vec(any::<u8>(), 0..40),
        fen in any::<bool>(),
    ) {
        let mut r = if fen {
            rec_fen("4k3/8/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1", &[], &[])
        } else {
            rec(&[], &[])
        };
        r.extend_from_slice(&stream);
        r.extend_from_slice(&comments);
        exercise(&parts_from_records(&[r]));
    }

    #[test]
    fn mostly_valid_streams_with_structure_tokens_never_panic(
        stream in proptest::collection::vec(
            prop_oneof![
                Just(11u8), Just(12), Just(13), Just(14), Just(15), Just(0),
                0x10u8..=0xFF
            ],
            0..200,
        ),
    ) {
        let r = rec(&stream, &[b"a", b"b", b"c"]);
        exercise(&parts_from_records(&[r]));
    }
}
