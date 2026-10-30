//! The score of the trainer: four counters per position of the user (the
//! ones of the Scid) plus a simple spaced repetition (spec M8-5, D2222-D2224).

/// The days between two reviews of a position, by box (D2223).
pub const BOX_INTERVALS: [u32; 7] = [0, 1, 2, 4, 8, 16, 32];
/// The last box.
pub const TOP_BOX: u8 = 6;
/// Boxes from which a position counts as learned (D2224).
pub const LEARNED_BOX: u8 = 3;
/// Need of a position that was never seen (D2223).
pub const NEED_UNSEEN: u32 = 30;
/// Need of a due position, before the days of delay (D2223).
pub const NEED_DUE: u32 = 20;

/// What is known about one position of the user (D2222).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PosStat {
    /// Times the position appeared (Scid: white counter).
    pub seen: u32,
    /// Times the right move was found at the first try (Scid: green).
    pub good: u32,
    /// Times a dubious move of the repertoire was played (Scid: yellow).
    pub dubious: u32,
    /// Times a move out of the repertoire was tried (Scid: red).
    pub out: u32,
    /// The Leitner box, `0..=TOP_BOX`.
    pub box_no: u8,
    /// The day (since 1970-01-01) the position is due.
    pub due: u32,
}

impl PosStat {
    /// The need of the position on `today` (D2223): never seen is the
    /// highest, due grows with the delay, not yet due is zero.
    pub fn need(&self, today: u32) -> u32 {
        if self.seen == 0 {
            return NEED_UNSEEN;
        }
        if today >= self.due {
            return NEED_DUE + (today - self.due).min(30) / 3;
        }
        0
    }

    /// A right move at the first try: up one box, due after its interval.
    /// A position that is not due yet stays where it is, so repeating a line
    /// the same day does not climb the boxes again (D2259).
    pub fn promote(&mut self, today: u32) {
        if today < self.due {
            return;
        }
        self.box_no = (self.box_no + 1).min(TOP_BOX);
        let days = BOX_INTERVALS
            .get(usize::from(self.box_no))
            .copied()
            .unwrap_or(0);
        self.due = today.saturating_add(days);
    }

    /// A mistake or help: back to box 0, due today.
    pub fn demote(&mut self, today: u32) {
        self.box_no = 0;
        self.due = today;
    }

    /// Whether the position counts as learned (D2224).
    pub fn learned(&self) -> bool {
        self.box_no >= LEARNED_BOX
    }
}

/// The need of a position that may not have a record yet.
pub fn need_of(stat: Option<&PosStat>, today: u32) -> u32 {
    stat.map_or(NEED_UNSEEN, |s| s.need(today))
}

/// The counters of a session (what the panel shows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionStats {
    /// Lines finished.
    pub lines: u32,
    /// Lines finished without any error or help.
    pub perfect_lines: u32,
    /// Positions of the user answered right at the first try.
    pub good: u32,
    /// Moves refused or accepted as dubious.
    pub dubious: u32,
    /// Moves out of the repertoire.
    pub out: u32,
}

/// The repertoire report (D2224, the `report` of the Scid).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReportView {
    /// Positions of the user in the repertoire.
    pub positions: u32,
    /// Positions never played.
    pub never: u32,
    /// Positions played at least once.
    pub played: u32,
    /// Positions due today (played, not learned for today).
    pub due: u32,
    /// Positions in box 3 or above.
    pub learned: u32,
    /// Sum of right moves at the first try.
    pub good: u32,
    /// Sum of dubious moves.
    pub dubious: u32,
    /// Sum of moves out of the repertoire.
    pub out: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_position_never_seen_has_the_highest_need() {
        let fresh = PosStat::default();
        assert_eq!(fresh.need(100), NEED_UNSEEN);
        assert_eq!(need_of(None, 100), NEED_UNSEEN);
    }

    #[test]
    fn promoting_climbs_the_boxes_and_sets_the_due_day() {
        let mut stat = PosStat {
            seen: 1,
            ..PosStat::default()
        };
        stat.promote(100);
        assert_eq!((stat.box_no, stat.due), (1, 101));
        stat.promote(101);
        assert_eq!((stat.box_no, stat.due), (2, 103));
        // One right answer per due day: the next one comes when it is due.
        let mut day = 200;
        for _ in 0..10 {
            stat.promote(day);
            day = stat.due;
        }
        assert_eq!(stat.box_no, TOP_BOX);
        assert_eq!(stat.due, day);
        assert!(stat.learned());
    }

    #[test]
    fn repeating_a_line_the_same_day_does_not_promote_again() {
        let mut stat = PosStat {
            seen: 1,
            ..PosStat::default()
        };
        stat.promote(100);
        let after_first = stat;
        for _ in 0..3 {
            stat.promote(100);
        }
        assert_eq!(stat, after_first, "three repetitions are one answer");
        assert!(!stat.learned());
    }

    #[test]
    fn a_mistake_sends_the_position_back_and_makes_it_due() {
        let mut stat = PosStat {
            seen: 3,
            box_no: 5,
            due: 500,
            ..PosStat::default()
        };
        stat.demote(100);
        assert_eq!((stat.box_no, stat.due), (0, 100));
        assert_eq!(stat.need(100), NEED_DUE);
        assert!(!stat.learned());
    }

    #[test]
    fn need_grows_with_the_delay_and_is_zero_before_the_due_day() {
        let stat = PosStat {
            seen: 2,
            box_no: 2,
            due: 100,
            ..PosStat::default()
        };
        assert_eq!(stat.need(99), 0);
        assert_eq!(stat.need(100), NEED_DUE);
        assert_eq!(stat.need(130), NEED_DUE + 10);
        assert_eq!(stat.need(10_000), NEED_DUE + 10, "the delay is capped");
    }

    #[test]
    fn dates_before_the_epoch_of_the_store_never_underflow() {
        let stat = PosStat {
            seen: 1,
            due: u32::MAX,
            ..PosStat::default()
        };
        assert_eq!(stat.need(0), 0);
        let mut near = PosStat {
            seen: 1,
            ..PosStat::default()
        };
        near.promote(u32::MAX);
        assert_eq!(near.due, u32::MAX);
    }
}
