//! Property tests: round-trips and robustness against hostile bytes.

#![allow(clippy::cast_possible_truncation, clippy::assert_is_empty)]

mod common;

use common::random_game;
use mychess_book::{Book, Entry, decode_move, encode_move, polyglot_key};
use proptest::prelude::*;

fn arb_entry() -> impl Strategy<Value = Entry> {
    (any::<u64>(), any::<u16>(), any::<u16>(), any::<u32>()).prop_map(|(key, mv, weight, learn)| {
        Entry {
            key,
            mv,
            weight,
            learn,
        }
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn to_bytes_from_bytes_round_trip(entries in proptest::collection::vec(arb_entry(), 1..80)) {
        let mut sorted = entries;
        sorted.sort_by_key(|e| e.key); // stable, as the book does
        let book = Book::from_entries(sorted.clone());
        let bytes = book.to_bytes();
        prop_assert_eq!(bytes.len(), sorted.len() * 16);
        let back = Book::from_bytes(&bytes).expect("parse");
        prop_assert_eq!(back.entries(), &sorted[..]);
        prop_assert!(back.was_sorted());
        prop_assert_eq!(back.to_bytes(), bytes);
    }

    #[test]
    fn unsorted_entries_sort_stably_and_keep_learn(entries in proptest::collection::vec(arb_entry(), 1..80)) {
        let book = Book::from_entries(entries.clone());
        let mut expected = entries;
        expected.sort_by_key(|e| e.key);
        prop_assert_eq!(book.entries(), &expected[..]);
        prop_assert!(Book::from_bytes(&book.to_bytes()).expect("parse").was_sorted());
    }

    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..600)) {
        let result = Book::from_bytes(&bytes);
        if bytes.len() < 16 {
            prop_assert!(result.is_err());
        } else {
            let book = result.expect("at least one record");
            prop_assert_eq!(book.len(), bytes.len() / 16);
            prop_assert_eq!(book.validate().trailing_bytes, (bytes.len() % 16) as u64);
            // Querying garbage must be safe too.
            let pos = mychess_core::Position::new();
            std::hint::black_box(book.moves(&pos));
            std::hint::black_box(book.leading_to_book(&pos));
            std::hint::black_box(book.pick(&pos, |b| b / 2));
            std::hint::black_box(book.best(&pos));
            std::hint::black_box(book.illegal_moves_in(&pos));
        }
    }

    #[test]
    fn truncation_at_any_position_never_panics(cut in 0usize..=161, tail in any::<u8>()) {
        let pos = mychess_core::Position::new();
        let key = polyglot_key(&pos);
        let mut entries = Vec::new();
        for (i, m) in pos.legal_moves().into_iter().take(10).enumerate() {
            entries.push(Entry {
                key,
                mv: encode_move(m).expect("encodable"),
                weight: 1 + i as u16,
                learn: 0xdead_beef,
            });
        }
        let mut bytes = Book::from_entries(entries).to_bytes();
        bytes.push(tail);
        let cut = cut.min(bytes.len());
        let result = Book::from_bytes(&bytes[..cut]);
        match result {
            Err(_) => prop_assert!(cut < 16),
            Ok(book) => {
                prop_assert_eq!(book.len(), cut / 16);
                std::hint::black_box(book.moves(&pos));
                std::hint::black_box(book.validate());
            }
        }
    }

    #[test]
    fn every_legal_move_round_trips_on_random_games(seeds in proptest::collection::vec(any::<u16>(), 0..90)) {
        for pos in random_game(&seeds) {
            for m in pos.legal_moves() {
                let code = encode_move(m).expect("legal moves are encodable");
                prop_assert_ne!(code, 0);
                prop_assert_eq!(decode_move(&pos, code), Some(m), "{}", pos.fen());
            }
        }
    }

    #[test]
    fn distinct_legal_moves_have_distinct_codes(seeds in proptest::collection::vec(any::<u16>(), 0..60)) {
        for pos in random_game(&seeds) {
            let mut codes: Vec<u16> = pos.legal_moves().into_iter().filter_map(encode_move).collect();
            let n = codes.len();
            codes.sort_unstable();
            codes.dedup();
            prop_assert_eq!(codes.len(), n);
        }
    }

    #[test]
    fn key_is_stable_through_fen_round_trip(seeds in proptest::collection::vec(any::<u16>(), 0..80)) {
        for pos in random_game(&seeds) {
            let again = mychess_core::Position::from_fen(&pos.fen()).expect("fen");
            prop_assert_eq!(polyglot_key(&pos), polyglot_key(&again));
        }
    }

    #[test]
    fn book_from_a_game_answers_every_position_of_it(seeds in proptest::collection::vec(any::<u16>(), 1..60)) {
        // Record each played move with weight 1; every position of the game
        // must then list exactly the move that was played.
        let mut pos = mychess_core::Position::new();
        let mut book = Book::new();
        let mut line = Vec::new();
        for s in &seeds {
            let legal = pos.legal_moves();
            if legal.is_empty() { break; }
            let m = legal[usize::from(*s) % legal.len()];
            line.push(m);
            pos.play(m).expect("play");
        }
        book.add_line(&line, &mychess_core::Position::new(), 1).expect("add");
        let mut pos = mychess_core::Position::new();
        for m in &line {
            let listed: Vec<_> = book.moves(&pos).into_iter().map(|b| b.mv).collect();
            // A repeated position may carry several moves, but the played one is there.
            prop_assert!(listed.contains(m));
            pos.play(*m).expect("play");
        }
    }
}
