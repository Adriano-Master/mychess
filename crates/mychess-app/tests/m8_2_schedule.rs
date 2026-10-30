//! M8-2a contract tests of the schedule (spec D1264, D1265, D1305; Scid
//! `comp.tcl` 349-407): pairings, colours, rounds, byes and determinism.
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_2_common;

use std::collections::{BTreeMap, HashSet};

use mychess_app::tournament::{Pairing, Scheduling, build_schedule};
use proptest::prelude::*;
use rstest::rstest;

fn key(p: &Pairing) -> (usize, usize) {
    (p.white.min(p.black), p.white.max(p.black))
}

fn colours(pairings: &[Pairing], n: usize) -> Vec<(i64, i64)> {
    let mut out = vec![(0, 0); n];
    for p in pairings {
        out[p.white].0 += 1;
        out[p.black].1 += 1;
    }
    out
}

#[rstest]
#[case(Scheduling::RoundRobin)]
#[case(Scheduling::Carousel)]
fn two_participants_make_a_match_in_every_format(#[case] s: Scheduling) {
    let sched = build_schedule(2, s, 4, 0);
    assert_eq!(sched.pairings.len(), 4);
    let whites: Vec<usize> = sched.pairings.iter().map(|p| p.white).collect();
    // Colours alternate in the match.
    assert_eq!(whites.iter().filter(|&&w| w == 0).count(), 2);
    assert_ne!(whites[0], whites[1]);
}

#[test]
fn gauntlet_with_two_participants_is_also_a_match() {
    let sched = build_schedule(2, Scheduling::Gauntlet, 2, 1);
    assert_eq!(sched.pairings.len(), 2);
    assert!(
        sched
            .pairings
            .iter()
            .all(|p| p.involves(1) && p.involves(0))
    );
    assert_ne!(sched.pairings[0].white, sched.pairings[1].white);
}

#[test]
fn numbers_are_consecutive_from_one_and_round_label_is_cycle_dot_round() {
    let sched = build_schedule(4, Scheduling::Carousel, 2, 0);
    for (i, p) in sched.pairings.iter().enumerate() {
        assert_eq!(p.number as usize, i + 1);
        assert_eq!(p.round_label(), format!("{}.{}", p.cycle, p.round));
    }
}

#[test]
fn carousel_everybody_plays_each_round_and_nobody_twice() {
    let sched = build_schedule(6, Scheduling::Carousel, 1, 0);
    let mut per_round: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for p in &sched.pairings {
        per_round
            .entry(p.round)
            .or_default()
            .extend([p.white, p.black]);
    }
    assert_eq!(per_round.len(), 5);
    for players in per_round.values() {
        let set: HashSet<_> = players.iter().copied().collect();
        assert_eq!(set.len(), 6, "everybody once: {players:?}");
        assert_eq!(players.len(), 6);
    }
    assert!(sched.byes.is_empty());
}

#[test]
fn carousel_with_odd_count_has_exactly_one_bye_per_round_and_the_rester_does_not_play() {
    let sched = build_schedule(7, Scheduling::Carousel, 2, 0);
    for cycle in 1..=2 {
        for round in 1..=7 {
            let byes: Vec<_> = sched
                .byes
                .iter()
                .filter(|b| b.cycle == cycle && b.round == round)
                .collect();
            assert_eq!(byes.len(), 1, "cycle {cycle} round {round}");
            let resting = byes[0].participant;
            assert!(
                sched
                    .pairings
                    .iter()
                    .filter(|p| p.cycle == cycle && p.round == round)
                    .all(|p| !p.involves(resting))
            );
        }
    }
}

#[test]
fn gauntlet_always_includes_the_tested_participant_and_every_opponent_cycles_times() {
    let sched = build_schedule(5, Scheduling::Gauntlet, 3, 2);
    assert_eq!(sched.pairings.len(), 4 * 3);
    assert!(sched.pairings.iter().all(|p| p.involves(2)));
    let mut counts: BTreeMap<usize, u32> = BTreeMap::new();
    for p in &sched.pairings {
        let other = if p.white == 2 { p.black } else { p.white };
        *counts.entry(other).or_default() += 1;
    }
    assert_eq!(counts.len(), 4);
    assert!(counts.values().all(|&c| c == 3));
}

#[test]
fn gauntlet_alternates_the_first_colour_from_opponent_to_opponent() {
    let sched = build_schedule(4, Scheduling::Gauntlet, 1, 0);
    let tested_white: Vec<bool> = sched.pairings.iter().map(|p| p.white == 0).collect();
    assert_eq!(tested_white, [true, false, true]);
}

#[test]
fn gauntlet_with_unknown_tested_participant_makes_no_game_and_no_panic() {
    assert!(
        build_schedule(3, Scheduling::Gauntlet, 2, 9)
            .pairings
            .is_empty()
    );
}

#[test]
fn round_robin_with_odd_cycles_balances_first_colours_over_pairs() {
    // D1265: the Scid always gives White to the lower index; we alternate.
    for n in 3..=9usize {
        let sched = build_schedule(n, Scheduling::RoundRobin, 1, 0);
        let low_white = sched.pairings.iter().filter(|p| p.white < p.black).count();
        let pairs = n * (n - 1) / 2;
        assert!(
            low_white.abs_diff(pairs - low_white) <= 1,
            "n={n}: {low_white} of {pairs}"
        );
    }
}

#[test]
fn round_robin_orders_pairs_like_the_scid_with_the_games_of_a_pair_in_sequence() {
    let sched = build_schedule(4, Scheduling::RoundRobin, 2, 0);
    let order: Vec<_> = sched.pairings.iter().map(key).collect();
    let mut expected = Vec::new();
    for j in 1..4 {
        for i in 0..j {
            expected.extend([(i, j), (i, j)]);
        }
    }
    assert_eq!(order, expected);
}

#[test]
fn groups_join_two_consecutive_cycles_of_the_same_confrontation() {
    let sched = build_schedule(3, Scheduling::RoundRobin, 4, 0);
    let mut by_pair: BTreeMap<(usize, usize), Vec<u32>> = BTreeMap::new();
    for p in &sched.pairings {
        by_pair.entry(key(p)).or_default().push(p.group);
    }
    for groups in by_pair.values() {
        assert_eq!(groups[0], groups[1]);
        assert_eq!(groups[2], groups[3]);
        assert_ne!(groups[0], groups[2]);
    }
}

#[test]
fn schedule_is_deterministic() {
    for s in [
        Scheduling::RoundRobin,
        Scheduling::Carousel,
        Scheduling::Gauntlet,
    ] {
        assert_eq!(build_schedule(7, s, 3, 1), build_schedule(7, s, 3, 1));
    }
}

#[test]
fn zero_cycles_are_treated_as_one() {
    assert_eq!(
        build_schedule(3, Scheduling::RoundRobin, 0, 0)
            .pairings
            .len(),
        3
    );
}

proptest! {
    #[test]
    fn round_robin_and_carousel_cover_each_pair_cycles_times_with_balanced_colours(
        n in 2usize..=12, cycles in 1u32..=6, carousel in any::<bool>()
    ) {
        let s = if carousel { Scheduling::Carousel } else { Scheduling::RoundRobin };
        let sched = build_schedule(n, s, cycles, 0);
        prop_assert_eq!(sched.pairings.len(), n * (n - 1) / 2 * cycles as usize);
        let mut pairs: BTreeMap<(usize, usize), (i64, i64, u32)> = BTreeMap::new();
        for p in &sched.pairings {
            let e = pairs.entry(key(p)).or_default();
            e.2 += 1;
            if p.white < p.black { e.0 += 1 } else { e.1 += 1 }
        }
        prop_assert_eq!(pairs.len(), n * (n - 1) / 2);
        for (c_low, c_high, total) in pairs.values() {
            prop_assert_eq!(*total, cycles);
            prop_assert!((c_low - c_high).abs() <= 1);
        }
        for p in &sched.pairings { prop_assert!(p.white != p.black && p.white < n && p.black < n); }
    }

    #[test]
    fn carousel_colours_stay_balanced_inside_each_cycle_for_each_participant(
        n in 3usize..=24, cycles in 1u32..=3
    ) {
        let sched = build_schedule(n, Scheduling::Carousel, cycles, 0);
        for cycle in 1..=cycles {
            let in_cycle: Vec<Pairing> =
                sched.pairings.iter().copied().filter(|p| p.cycle == cycle).collect();
            for (w, b) in colours(&in_cycle, n) {
                prop_assert!((w - b).abs() <= 1, "n={} cycle={} w={} b={}", n, cycle, w, b);
            }
        }
    }

    #[test]
    fn carousel_nobody_plays_twice_in_a_round_and_byes_match_parity(
        n in 2usize..=16, cycles in 1u32..=2
    ) {
        let sched = build_schedule(n, Scheduling::Carousel, cycles, 0);
        let mut seen: HashSet<(u32, u32, usize)> = HashSet::new();
        for p in &sched.pairings {
            prop_assert!(seen.insert((p.cycle, p.round, p.white)));
            prop_assert!(seen.insert((p.cycle, p.round, p.black)));
        }
        let expected_byes = if n % 2 == 1 { n * cycles as usize } else { 0 };
        prop_assert_eq!(sched.byes.len(), expected_byes);
    }

    #[test]
    fn gauntlet_has_n_minus_one_times_cycles_games_all_with_the_tested(
        n in 2usize..=12, cycles in 1u32..=6, tested in 0usize..12
    ) {
        let tested = tested % n;
        let sched = build_schedule(n, Scheduling::Gauntlet, cycles, tested);
        prop_assert_eq!(sched.pairings.len(), (n - 1) * cycles as usize);
        prop_assert!(sched.pairings.iter().all(|p| p.involves(tested)));
        let mut per: BTreeMap<(usize, usize), (i64, u32)> = BTreeMap::new();
        for p in &sched.pairings {
            let e = per.entry(key(p)).or_default();
            e.1 += 1;
            e.0 += if p.white == tested { 1 } else { -1 };
        }
        for (diff, total) in per.values() {
            prop_assert_eq!(*total, cycles);
            prop_assert!(diff.abs() <= 1);
        }
    }
}
