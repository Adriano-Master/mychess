//! The schedule of the games: pairings and colours (spec M8-2, D1264,
//! D1265).
//!
//! Scid: `comp.tcl` `compOk` 349-407. Differences, on purpose: the colour of
//! the first game of a pair alternates with the index of the pair, so an odd
//! number of cycles stays balanced (the Scid always gives White to the
//! participant of lower index in the first game).

use std::collections::HashMap;

use super::config::Scheduling;

/// One game of the schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pairing {
    /// The number of the game, from 1, in the order of the schedule.
    pub number: u32,
    /// The cycle, from 1.
    pub cycle: u32,
    /// The round, from 1: the round of the carousel, or the index of the
    /// confrontation in the other formats.
    pub round: u32,
    /// The participant with the white pieces.
    pub white: usize,
    /// The participant with the black pieces.
    pub black: usize,
    /// The games of a pair (same confrontation, colours swapped) share a
    /// group, and so an opening when `pair_openings` is on (D1273). Groups
    /// are numbered from 0 in the order they first appear.
    pub group: u32,
}

impl Pairing {
    /// The `Round` tag: `cycle.round` (the `"$k.$l"` of the Scid).
    pub fn round_label(&self) -> String {
        format!("{}.{}", self.cycle, self.round)
    }

    /// Whether `participant` plays this game.
    pub fn involves(&self, participant: usize) -> bool {
        self.white == participant || self.black == participant
    }
}

/// A participant that does not play in a round of the carousel (odd number
/// of participants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bye {
    /// The cycle, from 1.
    pub cycle: u32,
    /// The round, from 1.
    pub round: u32,
    /// The participant that rests.
    pub participant: usize,
}

/// The whole schedule.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Schedule {
    /// The games, in the order they are played.
    pub pairings: Vec<Pairing>,
    /// The rests of the carousel.
    pub byes: Vec<Bye>,
}

/// A game before numbering: `(cycle, round, white, black)`.
type Raw = (u32, u32, usize, usize);

/// Builds the schedule of `n` participants (D1264).
///
/// `tested` is the participant of `Gauntlet`; the other formats ignore it.
/// `cycles` is clamped to at least 1.
pub fn build_schedule(n: usize, scheduling: Scheduling, cycles: u32, tested: usize) -> Schedule {
    let cycles = cycles.max(1);
    let mut byes = Vec::new();
    let raw = match scheduling {
        Scheduling::RoundRobin => round_robin(n, cycles),
        Scheduling::Carousel => carousel(n, cycles, &mut byes),
        Scheduling::Gauntlet => gauntlet(n, cycles, tested),
    };
    let mut groups: HashMap<(usize, usize, u32), u32> = HashMap::new();
    let pairings = raw
        .into_iter()
        .zip(1u32..)
        .map(|((cycle, round, white, black), number)| {
            // Two consecutive cycles of the same confrontation make a pair.
            let key = (white.min(black), white.max(black), (cycle - 1) / 2);
            let next = u32::try_from(groups.len()).unwrap_or(u32::MAX);
            let group = *groups.entry(key).or_insert(next);
            Pairing {
                number,
                cycle,
                round,
                white,
                black,
                group,
            }
        })
        .collect();
    Schedule { pairings, byes }
}

fn index_u32(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// "Normal" of the Scid: pairs (0,1), (0,2), (1,2)..., the games of a pair
/// in sequence, colours alternating (D1265).
fn round_robin(n: usize, cycles: u32) -> Vec<Raw> {
    let mut out = Vec::new();
    let mut pair = 0usize;
    for j in 1..n {
        for i in 0..j {
            for k in 0..cycles {
                let first_is_white = (pair + k as usize).is_multiple_of(2);
                let (white, black) = if first_is_white { (i, j) } else { (j, i) };
                out.push((k + 1, index_u32(pair) + 1, white, black));
            }
            pair += 1;
        }
    }
    out
}

/// The circle method (D1265): one fixed seat, the others rotate.
///
/// Colours (D1305): inside a cycle every participant has `|white - black| <= 1`.
/// With an even number of participants the fixed seat is participant 0, which
/// alternates its colour round by round. With an odd number the fixed seat is
/// the phantom (the rest), so nobody is the odd one out. The `m` rotating
/// seats are labelled `0..m` (`m` is odd) and the pair `(i, j)` gives White to
/// `i` when `(j - i) mod m` is in `1..=(m - 1) / 2`: a circulant tournament,
/// where each seat wins and loses the same number of colours.
fn carousel(n: usize, cycles: u32, byes: &mut Vec<Bye>) -> Vec<Raw> {
    let mut out = Vec::new();
    if n < 2 {
        return out;
    }
    // A phantom participant (index `n`) completes an odd number.
    let size = n + n % 2;
    let m = size - 1;
    let start: Vec<usize> = if n.is_multiple_of(2) {
        (0..size).collect()
    } else {
        std::iter::once(n).chain(0..n).collect()
    };
    // The label of each rotating seat, by participant.
    let mut label = vec![0usize; size + 1];
    for (i, &p) in start.iter().skip(1).enumerate() {
        if let Some(slot) = label.get_mut(p) {
            *slot = i;
        }
    }
    for cycle in 0..cycles {
        let mut circle = start.clone();
        for round in 0..size - 1 {
            for board in 0..size / 2 {
                let (Some(&a), Some(&b)) = (circle.get(board), circle.get(size - 1 - board)) else {
                    continue;
                };
                let (cycle_no, round_no) = (cycle + 1, index_u32(round) + 1);
                if a >= n || b >= n {
                    byes.push(Bye {
                        cycle: cycle_no,
                        round: round_no,
                        participant: a.min(b),
                    });
                    continue;
                }
                let first_is_white = if board == 0 {
                    // `a` is the fixed seat (even `n`): it alternates.
                    round.is_multiple_of(2)
                } else {
                    let at = |p: usize| label.get(p).copied().unwrap_or(0);
                    let (la, lb) = (at(a), at(b));
                    let forward = (lb + m - la) % m;
                    (1..=(m - 1) / 2).contains(&forward)
                };
                // The next cycle inverts every colour.
                let first_is_white = first_is_white != (cycle % 2 == 1);
                let (white, black) = if first_is_white { (a, b) } else { (b, a) };
                out.push((cycle_no, round_no, white, black));
            }
            if let Some(last) = circle.pop() {
                circle.insert(1, last);
            }
        }
    }
    out
}

/// "First plays others": the tested participant against every other, the
/// colour of the first game alternating from one opponent to the next.
fn gauntlet(n: usize, cycles: u32, tested: usize) -> Vec<Raw> {
    let mut out = Vec::new();
    if tested >= n {
        return out;
    }
    for (j, opponent) in (0..n).filter(|&o| o != tested).enumerate() {
        for k in 0..cycles {
            let tested_white = (j + k as usize).is_multiple_of(2);
            let (white, black) = if tested_white {
                (tested, opponent)
            } else {
                (opponent, tested)
            };
            out.push((k + 1, index_u32(j) + 1, white, black));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};

    use proptest::prelude::*;

    use super::*;

    fn pair_key(p: &Pairing) -> (usize, usize) {
        (p.white.min(p.black), p.white.max(p.black))
    }

    #[test]
    fn round_robin_orders_pairs_like_the_scid() {
        let s = build_schedule(3, Scheduling::RoundRobin, 2, 0);
        let order: Vec<(usize, usize)> = s.pairings.iter().map(pair_key).collect();
        assert_eq!(order, [(0, 1), (0, 1), (0, 2), (0, 2), (1, 2), (1, 2)]);
        assert_eq!(s.pairings[0].round_label(), "1.1");
        assert_eq!(s.pairings[1].round_label(), "2.1");
        assert_eq!(s.pairings[2].round_label(), "1.2");
        assert_eq!(s.pairings[5].number, 6);
    }

    #[test]
    fn round_robin_with_odd_cycles_balances_the_colours_of_the_first_games() {
        let s = build_schedule(4, Scheduling::RoundRobin, 1, 0);
        let first_as_white = s.pairings.iter().filter(|p| p.white < p.black).count();
        // 6 pairs: three with the lower index as White, three with it as Black.
        assert_eq!(first_as_white, 3);
    }

    #[test]
    fn carousel_with_an_odd_number_rests_one_per_round() {
        let s = build_schedule(5, Scheduling::Carousel, 1, 0);
        assert_eq!(s.pairings.len(), 10);
        assert_eq!(s.byes.len(), 5);
        let rested: HashSet<usize> = s.byes.iter().map(|b| b.participant).collect();
        assert_eq!(rested.len(), 5, "everybody rests once");
    }

    #[test]
    fn carousel_inverts_the_colours_in_the_next_cycle() {
        let s = build_schedule(4, Scheduling::Carousel, 2, 0);
        let (first, second) = s.pairings.split_at(6);
        for a in first {
            let b = second
                .iter()
                .find(|b| pair_key(b) == pair_key(a))
                .expect("pair in cycle 2");
            assert_eq!((a.white, a.black), (b.black, b.white));
        }
    }

    #[test]
    fn gauntlet_always_has_the_tested_participant() {
        let s = build_schedule(4, Scheduling::Gauntlet, 2, 2);
        assert_eq!(s.pairings.len(), 6);
        assert!(s.pairings.iter().all(|p| p.involves(2)));
        let whites = s.pairings.iter().filter(|p| p.white == 2).count();
        assert_eq!(whites, 3);
    }

    #[test]
    fn two_participants_make_a_match_in_every_format() {
        for format in [
            Scheduling::RoundRobin,
            Scheduling::Carousel,
            Scheduling::Gauntlet,
        ] {
            let s = build_schedule(2, format, 4, 0);
            assert_eq!(s.pairings.len(), 4, "{format:?}");
            let whites = s.pairings.iter().filter(|p| p.white == 0).count();
            assert_eq!(whites, 2, "{format:?}");
        }
    }

    #[test]
    fn the_games_of_a_pair_share_an_opening_group() {
        let s = build_schedule(3, Scheduling::RoundRobin, 4, 0);
        let groups: Vec<u32> = s.pairings.iter().take(4).map(|p| p.group).collect();
        assert_eq!(groups, [0, 0, 1, 1]);
    }

    #[test]
    fn degenerate_inputs_give_an_empty_schedule() {
        assert!(
            build_schedule(0, Scheduling::RoundRobin, 2, 0)
                .pairings
                .is_empty()
        );
        assert!(
            build_schedule(1, Scheduling::Carousel, 2, 0)
                .pairings
                .is_empty()
        );
        assert!(
            build_schedule(3, Scheduling::Gauntlet, 2, 9)
                .pairings
                .is_empty()
        );
    }

    proptest! {
        #[test]
        fn every_pair_meets_exactly_cycles_times_with_balanced_colours(
            n in 2usize..=12, cycles in 1u32..=6, carousel in any::<bool>(),
        ) {
            let format = if carousel { Scheduling::Carousel } else { Scheduling::RoundRobin };
            let s = build_schedule(n, format, cycles, 0);
            prop_assert_eq!(s.pairings.len(), n * (n - 1) / 2 * cycles as usize);
            let mut pairs: BTreeMap<(usize, usize), (u32, i32)> = BTreeMap::new();
            for p in &s.pairings {
                prop_assert!(p.white != p.black && p.white < n && p.black < n);
                let e = pairs.entry(pair_key(p)).or_default();
                e.0 += 1;
                e.1 += if p.white < p.black { 1 } else { -1 };
            }
            prop_assert_eq!(pairs.len(), n * (n - 1) / 2);
            for (count, balance) in pairs.values() {
                prop_assert_eq!(*count, cycles);
                prop_assert!(balance.abs() <= 1, "colour balance {}", balance);
            }
            // Everybody plays the same number of games per cycle.
            for participant in 0..n {
                for cycle in 1..=cycles {
                    let played = s.pairings.iter()
                        .filter(|p| p.cycle == cycle && p.involves(participant))
                        .count();
                    prop_assert_eq!(played, n - 1);
                }
            }
            let numbers: Vec<u32> = s.pairings.iter().map(|p| p.number).collect();
            prop_assert_eq!(numbers, (1u32..).take(s.pairings.len()).collect::<Vec<_>>());
        }

        #[test]
        fn the_carousel_never_repeats_a_participant_in_a_round(
            n in 2usize..=12, cycles in 1u32..=4,
        ) {
            let s = build_schedule(n, Scheduling::Carousel, cycles, 0);
            let mut per_round: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new();
            for p in &s.pairings {
                per_round.entry((p.cycle, p.round)).or_default().extend([p.white, p.black]);
            }
            for b in &s.byes {
                per_round.entry((b.cycle, b.round)).or_default().push(b.participant);
            }
            for (round, players) in &per_round {
                let unique: HashSet<usize> = players.iter().copied().collect();
                prop_assert_eq!(unique.len(), players.len(), "round {:?}", round);
                prop_assert_eq!(unique.len(), n, "everybody plays or rests in {:?}", round);
            }
            let expected_byes = if n % 2 == 1 { n * cycles as usize } else { 0 };
            prop_assert_eq!(s.byes.len(), expected_byes);
        }

        #[test]
        fn the_carousel_balances_the_colours_of_each_participant_in_each_cycle(
            n in 3usize..=24, cycles in 1u32..=3,
        ) {
            let s = build_schedule(n, Scheduling::Carousel, cycles, 0);
            for cycle in 1..=cycles {
                for participant in 0..n {
                    let (mut white, mut black) = (0i32, 0i32);
                    for p in s.pairings.iter().filter(|p| p.cycle == cycle) {
                        if p.white == participant { white += 1; }
                        if p.black == participant { black += 1; }
                    }
                    prop_assert_eq!(white + black, i32::try_from(n - 1).unwrap_or(0));
                    prop_assert!(
                        (white - black).abs() <= 1,
                        "participant {} in cycle {}: {} white, {} black", participant, cycle, white, black
                    );
                }
            }
        }

        #[test]
        fn the_gauntlet_has_n_minus_one_times_cycles_games_with_the_tested(
            n in 2usize..=12, cycles in 1u32..=6, tested in 0usize..12,
        ) {
            let tested = tested % n;
            let s = build_schedule(n, Scheduling::Gauntlet, cycles, tested);
            prop_assert_eq!(s.pairings.len(), (n - 1) * cycles as usize);
            prop_assert!(s.pairings.iter().all(|p| p.involves(tested)));
            let mut meets: BTreeMap<usize, (u32, i32)> = BTreeMap::new();
            for p in &s.pairings {
                let other = if p.white == tested { p.black } else { p.white };
                let e = meets.entry(other).or_default();
                e.0 += 1;
                e.1 += if p.white == tested { 1 } else { -1 };
            }
            for (count, balance) in meets.values() {
                prop_assert_eq!(*count, cycles);
                prop_assert!(balance.abs() <= 1);
            }
        }
    }
}
