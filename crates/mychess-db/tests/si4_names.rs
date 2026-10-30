//! Contract tests of the `.sn4` name file (spec M3-1, criterion ".sn4";
//! `docs/formatos/si4.md` section 7).
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_lossless
)]

#[path = "common/si4_builder.rs"]
mod si4_builder;

use mychess_db::si4::{
    NAME_HEADER_LEN, NameBase, NameKind, NameSection, Si4Database, Si4Error, Si4File,
};
use pretty_assertions::assert_eq;
use rstest::rstest;
use si4_builder::{EntrySpec, IndexHeader, NameRec, index_file, names_file_with, rec, section};
use std::io::Cursor;

const K: [NameKind; 4] = NameKind::ALL;

fn empty() -> Vec<NameRec> {
    Vec::new()
}

fn corrupt_or_truncated(e: &Si4Error) -> bool {
    matches!(
        e,
        Si4Error::CorruptNames { .. }
            | Si4Error::Truncated {
                file: Si4File::Names,
                ..
            }
    )
}

/// `.sn4` assembled by hand: header plus the bytes of the four sections.
fn raw_sn4(counts: [u32; 4], max_freq: [u32; 4], body: &[u8]) -> Vec<u8> {
    let mut out = b"Scid.sn\0".to_vec();
    out.extend_from_slice(&0u32.to_be_bytes());
    for c in counts {
        out.extend_from_slice(&c.to_be_bytes()[1..]);
    }
    for m in max_freq {
        out.extend_from_slice(&m.to_be_bytes()[1..]);
    }
    out.extend_from_slice(body);
    out
}

#[test]
fn name_header_is_36_bytes_and_reads_time_stamp_and_max_frequencies() {
    assert_eq!(NAME_HEADER_LEN, 36);
    let data = names_file_with(
        &[empty(), empty(), empty(), empty()],
        [10, 300, 70_000, 0],
        None,
    );
    assert_eq!(data.len(), 36);
    let nb = NameBase::parse(&data).unwrap();
    assert_eq!(nb.time_stamp(), 1234);
    for (kind, expected) in K.iter().zip([10, 300, 70_000, 0]) {
        assert_eq!(nb.max_frequency(*kind), expected);
        assert_eq!(nb.num_names(*kind), 0);
    }
}

#[test]
fn name_file_with_wrong_magic_or_short_header_is_rejected() {
    let good = names_file_with(&[empty(), empty(), empty(), empty()], [0; 4], None);
    for len in 0..NAME_HEADER_LEN {
        assert!(
            matches!(
                NameBase::parse(&good[..len]),
                Err(Si4Error::Truncated {
                    file: Si4File::Names,
                    ..
                })
            ),
            "length {len}"
        );
    }
    let mut bad = good.clone();
    bad[7] = 1;
    assert!(matches!(
        NameBase::parse(&bad),
        Err(Si4Error::BadMagic {
            file: Si4File::Names
        })
    ));
    let mut bad = good;
    bad[..8].copy_from_slice(b"Scid.si\0");
    assert!(matches!(
        NameBase::parse(&bad),
        Err(Si4Error::BadMagic { .. })
    ));
}

#[test]
fn names_are_front_coded_and_ids_need_not_follow_the_file_order() {
    let players: Vec<NameRec> = vec![
        (4, 1, b"".to_vec()),
        (2, 7, b"a".to_vec()),
        (0, 2, b"ab".to_vec()),
        (1, 3, b"abc".to_vec()),
        (3, 4, b"abd".to_vec()),
        (5, 5, b"b".to_vec()),
        (6, 6, b"\xC9mile".to_vec()), // Latin-1 byte, kept raw
    ];
    let events: Vec<NameRec> = vec![(1, 1, b"Wijk".to_vec()), (0, 1, b"Wijk aan Zee".to_vec())];
    let data = names_file_with(
        &[players.clone(), events, empty(), empty()],
        [7, 1, 0, 0],
        None,
    );
    let nb = NameBase::parse(&data).unwrap();
    for (id, freq, name) in &players {
        assert_eq!(
            nb.get(NameKind::Player, *id),
            Some(name.as_slice()),
            "id {id}"
        );
        assert_eq!(nb.frequency(NameKind::Player, *id), Some(*freq));
    }
    assert_eq!(nb.get(NameKind::Event, 0), Some(&b"Wijk aan Zee"[..]));
    assert_eq!(nb.get(NameKind::Event, 1), Some(&b"Wijk"[..]));
    assert_eq!(nb.num_names(NameKind::Player), 7);
    assert_eq!(nb.get(NameKind::Player, 7), None);
    assert_eq!(nb.get(NameKind::Site, 0), None);
}

#[test]
fn names_of_255_bytes_and_with_high_bytes_are_kept_raw() {
    let long = vec![b'x'; 255];
    let mut long2 = long.clone();
    long2[254] = 0xFF;
    let players: Vec<NameRec> = vec![(0, 1, long.clone()), (1, 1, long2.clone())];
    let data = names_file_with(&[players, empty(), empty(), empty()], [1; 4], None);
    let nb = NameBase::parse(&data).unwrap();
    assert_eq!(nb.get(NameKind::Player, 0), Some(long.as_slice()));
    assert_eq!(nb.get(NameKind::Player, 1), Some(long2.as_slice()));
}

#[test]
fn sections_are_independent_for_front_coding() {
    // The first name of each section has no prefix byte.
    let data = names_file_with(
        &[
            section(&["Same"]),
            section(&["Same"]),
            section(&["Same"]),
            section(&["Same"]),
        ],
        [1; 4],
        None,
    );
    let nb = NameBase::parse(&data).unwrap();
    for kind in K {
        assert_eq!(nb.get(kind, 0), Some(&b"Same"[..]), "{kind:?}");
    }
}

#[rstest]
#[case(65_534)]
#[case(65_535)]
#[case(65_536)]
#[case(65_537)]
fn id_width_switches_from_two_to_three_bytes_at_65536_names(#[case] count: u32) {
    // IDs are a permutation of 0..count, so the record order differs from the ID order.
    let mut recs: Vec<NameRec> = (0..count)
        .map(|i| {
            let id = (i * 7919) % count;
            (id, 1, format!("n{id:06}").into_bytes())
        })
        .collect();
    recs.sort_by(|a, b| a.2.cmp(&b.2));
    let data = names_file_with(&[recs, empty(), empty(), empty()], [1; 4], None);
    let nb = NameBase::parse(&data).unwrap();
    assert_eq!(nb.num_names(NameKind::Player), count);
    for id in [0, 1, count / 2, count - 2, count - 1] {
        let expected = format!("n{id:06}");
        assert_eq!(
            nb.get(NameKind::Player, id),
            Some(expected.as_bytes()),
            "count {count}, id {id}"
        );
    }
    assert_eq!(nb.get(NameKind::Player, count), None);
}

#[rstest]
#[case(255)]
#[case(256)]
#[case(65_535)]
#[case(65_536)]
#[case(16_777_215)]
fn frequency_width_follows_max_frequency(#[case] max: u32) {
    // The same max frequency in each kind, so a wrong width in any section breaks the next one.
    let sec = |n: &str| -> Vec<NameRec> {
        vec![
            (1, max, format!("{n}1").into_bytes()),
            (0, 1, format!("{n}0").into_bytes()),
            (2, max / 2, format!("{n}2").into_bytes()),
        ]
    };
    let data = names_file_with(&[sec("p"), sec("e"), sec("s"), sec("r")], [max; 4], None);
    let nb = NameBase::parse(&data).unwrap();
    for (kind, tag) in K.iter().zip(["p", "e", "s", "r"]) {
        assert_eq!(nb.max_frequency(*kind), max);
        assert_eq!(nb.frequency(*kind, 1), Some(max), "{tag} {max}");
        assert_eq!(nb.frequency(*kind, 2), Some(max / 2));
        assert_eq!(nb.get(*kind, 2), Some(format!("{tag}2").as_bytes()));
    }
}

#[test]
fn widths_are_chosen_per_kind() {
    let data = names_file_with(
        &[
            section(&["a"]),
            section(&["b"]),
            section(&["c"]),
            section(&["d"]),
        ],
        [255, 256, 65_535, 65_536],
        None,
    );
    let nb = NameBase::parse(&data).unwrap();
    for (kind, name) in K.iter().zip(["a", "b", "c", "d"]) {
        assert_eq!(nb.get(*kind, 0), Some(name.as_bytes()));
    }
}

#[test]
fn id_out_of_range_and_repeated_id_are_errors() {
    // Valid boundary first: id == numNames - 1.
    let ok = names_file_with(
        &[
            vec![(0, 1, b"a".to_vec()), (1, 1, b"b".to_vec())],
            empty(),
            empty(),
            empty(),
        ],
        [1; 4],
        None,
    );
    assert!(NameBase::parse(&ok).is_ok());
    for (a, b) in [(0u32, 2u32), (2, 1), (0xFFFF, 0), (0, 0), (1, 1)] {
        let bad = names_file_with(
            &[
                vec![(a, 1, b"a".to_vec()), (b, 1, b"b".to_vec())],
                empty(),
                empty(),
                empty(),
            ],
            [1; 4],
            None,
        );
        let e = NameBase::parse(&bad).unwrap_err();
        assert!(
            matches!(e, Si4Error::CorruptNames { .. }),
            "ids {a},{b}: {e:?}"
        );
    }
}

#[test]
fn id_error_in_a_later_section_names_that_section() {
    let data = names_file_with(
        &[
            section(&["a"]),
            section(&["b"]),
            vec![(3, 1, b"c".to_vec())],
            empty(),
        ],
        [1; 4],
        None,
    );
    match NameBase::parse(&data).unwrap_err() {
        Si4Error::CorruptNames { section, .. } => {
            assert_eq!(section, NameSection::Names(NameKind::Site));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn prefix_longer_than_the_name_or_than_the_previous_name_is_an_error() {
    // record 0: id 0, freq 1, length 3 "abc"; record 1: id 1, freq 1, length L, prefix P, new bytes.
    let rec0 = [0u8, 0, 1, 3, b'a', b'b', b'c'];
    let mk = |length: u8, prefix: u8, new: &[u8]| {
        let mut body = rec0.to_vec();
        body.extend_from_slice(&[0, 1, 1, length, prefix]);
        body.extend_from_slice(new);
        raw_sn4([2, 0, 0, 0], [1; 4], &body)
    };
    // sanity: "abc" + "de" = "abcde" (length 5, prefix 3)
    let nb = NameBase::parse(&mk(5, 3, b"de")).unwrap();
    assert_eq!(nb.get(NameKind::Player, 1), Some(&b"abcde"[..]));
    // prefix 4 > previous name length 3 (but <= length 5)
    assert!(matches!(
        NameBase::parse(&mk(5, 4, b"e")),
        Err(Si4Error::CorruptNames { .. })
    ));
    // prefix 3 > length 2
    assert!(matches!(
        NameBase::parse(&mk(2, 3, b"")),
        Err(Si4Error::CorruptNames { .. })
    ));
    // prefix 255 with a short name
    assert!(NameBase::parse(&mk(255, 255, b"")).is_err());
}

#[test]
fn every_strict_prefix_of_a_name_file_is_an_error() {
    let players: Vec<NameRec> = vec![
        (1, 300, b"Anand".to_vec()),
        (0, 2, b"Anderssen".to_vec()),
        (2, 3, b"Karpov".to_vec()),
    ];
    let data = names_file_with(
        &[
            players,
            section(&["Wijk aan Zee", "Wijk aan Zee B"]),
            section(&["Hastings"]),
            section(&["1", "2"]),
        ],
        [300, 1, 1, 1],
        None,
    );
    assert!(NameBase::parse(&data).is_ok());
    for len in 0..data.len() {
        let e = NameBase::parse(&data[..len]).unwrap_err();
        assert!(
            matches!(e, Si4Error::Truncated { .. } | Si4Error::BadMagic { .. }),
            "length {len}: {e:?}"
        );
    }
}

#[test]
fn huge_name_counts_in_a_tiny_file_are_rejected_without_allocating() {
    let data = raw_sn4([16_777_215; 4], [16_777_215; 4], &[]);
    let started = std::time::Instant::now();
    let e = NameBase::parse(&data).unwrap_err();
    assert!(corrupt_or_truncated(&e), "{e:?}");
    // One kind only, with a body that has a few records.
    let data = raw_sn4([0, 0, 16_777_215, 0], [1; 4], &[0, 0, 1, 1, b'x']);
    assert!(NameBase::parse(&data).is_err());
    assert!(
        started.elapsed().as_secs() < 2,
        "must not scale with the declared count"
    );
}

#[test]
fn huge_frequency_and_count_headers_with_random_looking_body_do_not_panic() {
    for counts in [
        [1u32, 0, 0, 0],
        [0, 1, 0, 0],
        [65_536, 0, 0, 0],
        [0, 0, 0, 262_143],
    ] {
        for body in [&[][..], &[0xFF; 3][..], &[0xFF; 64][..], &[0; 64][..]] {
            let data = raw_sn4(counts, [0xFF_FFFF; 4], body);
            drop(NameBase::parse(&data));
        }
    }
}

#[test]
fn database_resolves_player_ids_above_65535_through_three_byte_name_ids() {
    // 70,001 players: ids need 3 bytes in the .sn4 and the 4 high bits in the entry.
    let players: Vec<NameRec> = (0..70_001u32)
        .map(|i| (i, 1, format!("P{i:06}").into_bytes()))
        .collect();
    let names = names_file_with(
        &[players, section(&["E"]), section(&["S"]), section(&["1"])],
        [1; 4],
        None,
    );
    let mut e = EntrySpec::new(0, 5);
    e.white = 70_000;
    e.black = 65_536;
    let index = index_file(&IndexHeader::default(), &[e.encode()]);
    let mut db = Si4Database::from_parts(
        index,
        &names,
        Cursor::new(rec(&[0xCF, 0xCF, 15], &[])),
        "x.sg4".into(),
    )
    .unwrap();
    let game = db.game(1).unwrap();
    assert_eq!(game.tags().white, "P070000");
    assert_eq!(game.tags().black, "P065536");
}
