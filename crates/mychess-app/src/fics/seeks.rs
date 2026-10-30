//! The seek graph of FICS (spec m8-9, D2626): the advertisements of the
//! players who look for an opponent.

use mychess_online::fics::Seek;

/// Most advertisements kept (D2604); the oldest go first.
pub const MAX_SEEKS: usize = 2000;

/// The kinds of game the app plays (D2600).
pub const PLAYABLE_KINDS: [&str; 4] = ["untimed", "blitz", "lightning", "standard"];

/// The list of advertisements.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeekGraph {
    ads: Vec<Seek>,
}

impl SeekGraph {
    /// An empty graph.
    #[must_use]
    pub fn new() -> SeekGraph {
        SeekGraph::default()
    }

    /// Adds an advertisement; one with the same index is replaced.
    pub fn add(&mut self, seek: Seek) {
        if let Some(slot) = self.ads.iter_mut().find(|s| s.index == seek.index) {
            *slot = seek;
            return;
        }
        if self.ads.len() >= MAX_SEEKS {
            self.ads.remove(0);
        }
        self.ads.push(seek);
    }

    /// Removes the advertisements with these indexes.
    pub fn remove(&mut self, indexes: &[u32]) {
        self.ads.retain(|s| !indexes.contains(&s.index));
    }

    /// Removes everything.
    pub fn clear(&mut self) {
        self.ads.clear();
    }

    /// The advertisements, oldest first.
    #[must_use]
    pub fn ads(&self) -> &[Seek] {
        &self.ads
    }

    /// The advertisement with this index.
    #[must_use]
    pub fn get(&self, index: u32) -> Option<&Seek> {
        self.ads.iter().find(|s| s.index == index)
    }

    /// How many advertisements.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ads.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ads.is_empty()
    }
}

/// Whether the app plays this advertisement (D2600).
#[must_use]
pub fn is_playable(seek: &Seek) -> bool {
    PLAYABLE_KINDS.contains(&seek.kind.as_str())
}

/// The estimated duration of a game in minutes, the x axis of the graph:
/// `minutes + 2/3 * increment` (D2626, the rule of the FICS rating
/// classes: 40 moves).
#[must_use]
pub fn estimated_minutes(seek: &Seek) -> f32 {
    // The values are small: the float conversions are exact.
    #[allow(clippy::cast_precision_loss)]
    let (m, i) = (seek.minutes as f32, seek.increment as f32);
    m + i * 2.0 / 3.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_online::fics::{FicsEvent, parse_line};

    fn seek(index: u32, minutes: u32, kind: &str) -> Seek {
        let text = format!(
            "<s> {index} w=bob ti=00 rt=1500E t={minutes} i=0 r=u tp={kind} c=? rr=0-9999 a=t f=f"
        );
        let FicsEvent::SeekAdd(s) = parse_line(&text) else {
            panic!("not a seek");
        };
        *s
    }

    #[test]
    fn add_replace_remove_clear() {
        let mut g = SeekGraph::new();
        assert!(g.is_empty());
        g.add(seek(1, 5, "blitz"));
        g.add(seek(2, 1, "lightning"));
        g.add(seek(1, 7, "blitz"));
        assert_eq!(g.len(), 2);
        assert_eq!(g.get(1).map(|s| s.minutes), Some(7));
        g.remove(&[2, 99]);
        assert_eq!(g.len(), 1);
        g.clear();
        assert!(g.is_empty());
    }

    #[test]
    fn the_graph_is_bounded_and_drops_the_oldest() {
        let mut g = SeekGraph::new();
        let max = u32::try_from(MAX_SEEKS).expect("small");
        for i in 0..(max + 10) {
            g.add(seek(i, 5, "blitz"));
        }
        assert_eq!(g.len(), MAX_SEEKS);
        assert!(g.get(0).is_none());
        assert!(g.get(max + 9).is_some());
    }

    #[test]
    fn variants_are_not_playable_and_the_x_axis_counts_the_increment() {
        assert!(is_playable(&seek(1, 5, "blitz")));
        assert!(!is_playable(&seek(1, 5, "crazyhouse")));
        let mut s = seek(1, 5, "blitz");
        s.increment = 3;
        assert!((estimated_minutes(&s) - 7.0).abs() < 1e-6);
    }
}
