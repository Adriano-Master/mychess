//! Contract tests of the `info` coalescer (spec m4-1, D412): at most one
//! emission per `(search, multipv)` per interval, the last value wins.

use std::collections::BTreeMap;
use std::time::Duration;

use proptest::prelude::*;
use tokio::time::Instant;

use super::*;

fn info(search: u64, multipv: u8, depth: u32) -> SearchInfo {
    SearchInfo {
        search: SearchId(search),
        multipv,
        depth: Some(depth),
        ..SearchInfo::default()
    }
}

proptest! {
    /// Whatever the arrival pattern, per line the delivered reports are at
    /// least one interval apart and the last one delivered after a final
    /// flush is the last one pushed.
    #[test]
    fn spaced_emissions_and_the_last_value_per_line_survives(
        steps in proptest::collection::vec((1_u8..=4, 0_u64..120), 1..300),
    ) {
        let interval = Duration::from_millis(50);
        let t0 = Instant::now();
        let mut c = Coalescer::new(interval);
        let mut now = t0;
        let mut depth = 0_u32;
        let mut last_pushed: BTreeMap<u8, u32> = BTreeMap::new();
        let mut delivered: Vec<(Instant, u8, u32)> = Vec::new();
        for (line, advance_ms) in steps {
            now += Duration::from_millis(advance_ms);
            for due in c.take_due(now) {
                delivered.push((now, due.multipv, due.depth.unwrap()));
            }
            depth += 1;
            last_pushed.insert(line, depth);
            if let Some(out) = c.push(info(1, line, depth), now) {
                delivered.push((now, out.multipv, out.depth.unwrap()));
            }
        }
        let final_flush = c.take_all(now);
        let flushed_at = now;
        for f in &final_flush {
            delivered.push((flushed_at, f.multipv, f.depth.unwrap()));
        }
        // Spacing (the final flush is exempt: it precedes a more important event).
        let regular = delivered.len() - final_flush.len();
        let mut last_at: BTreeMap<u8, Instant> = BTreeMap::new();
        for (at, line, _) in &delivered[..regular] {
            if let Some(prev) = last_at.get(line) {
                prop_assert!(*at - *prev >= interval, "line {line} emitted twice within {:?}", *at - *prev);
            }
            last_at.insert(*line, *at);
        }
        // Last value wins.
        for (line, depth) in &last_pushed {
            let last_delivered = delivered.iter().rev().find(|(_, l, _)| l == line).map(|d| d.2);
            prop_assert_eq!(last_delivered, Some(*depth), "line {}", line);
        }
        prop_assert!(c.next_due().is_none());
    }

    /// Delivered depths of one line never go backwards.
    #[test]
    fn delivery_order_per_line_follows_arrival_order(
        steps in proptest::collection::vec((1_u8..=3, 0_u64..80), 1..200),
    ) {
        let t0 = Instant::now();
        let mut c = Coalescer::new(Duration::from_millis(50));
        let mut now = t0;
        let mut depth = 0;
        let mut seen: BTreeMap<u8, u32> = BTreeMap::new();
        let mut check = |i: &SearchInfo| {
            let d = i.depth.unwrap();
            let prev = seen.insert(i.multipv, d);
            prev.is_none_or(|p| p < d)
        };
        for (line, ms) in steps {
            now += Duration::from_millis(ms);
            for due in c.take_due(now) {
                prop_assert!(check(&due));
            }
            depth += 1;
            if let Some(out) = c.push(info(1, line, depth), now) {
                prop_assert!(check(&out));
            }
        }
        for f in c.take_all(now) {
            prop_assert!(check(&f));
        }
    }
}

#[test]
fn a_new_search_is_not_held_back_by_the_old_one() {
    let t0 = Instant::now();
    let mut c = Coalescer::new(Duration::from_millis(50));
    assert!(c.push(info(1, 1, 1), t0).is_some());
    assert!(
        c.push(info(2, 1, 1), t0 + Duration::from_millis(1))
            .is_some()
    );
}

#[test]
fn clear_forgets_pending_reports() {
    let t0 = Instant::now();
    let mut c = Coalescer::new(Duration::from_millis(50));
    c.push(info(1, 1, 1), t0);
    c.push(info(1, 1, 2), t0 + Duration::from_millis(1));
    c.clear();
    assert_eq!(c.take_all(t0).len(), 0);
    assert!(c.next_due().is_none());
}
