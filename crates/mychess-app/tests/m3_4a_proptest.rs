//! Random sequences of slot operations against a small reference model
//! (spec M3-4, rules 1-6; invariants of the slots).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_push_string
)]

mod m3_4a_common;
use m3_4a_common::*;
use proptest::prelude::*;

const FILES: usize = 10;

#[derive(Debug, Clone)]
enum Op {
    Open(usize),
    Select(usize),
    Close(usize),
    Next,
    Previous,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (0..FILES).prop_map(Op::Open),
        3 => (0usize..=10).prop_map(Op::Select),
        3 => (0usize..=10).prop_map(Op::Close),
        1 => Just(Op::Next),
        1 => Just(Op::Previous),
    ]
}

/// The expected state: file index per position 0..=7, and the current position.
struct Model {
    slots: [Option<usize>; 8],
    current: usize,
}

impl Model {
    fn occupied(&self, pos: usize) -> bool {
        pos == 8 || self.slots.get(pos).is_some_and(Option::is_some)
    }
    fn select(&mut self, pos: usize) {
        if self.occupied(pos) {
            self.current = pos;
        }
    }
    fn cycle(&mut self, forward: bool) {
        for step in 1..9 {
            let pos = if forward {
                (self.current + step) % 9
            } else {
                (self.current + 9 - step) % 9
            };
            if self.occupied(pos) {
                self.current = pos;
                return;
            }
        }
    }
    fn apply(&mut self, op: &Op) {
        match *op {
            Op::Open(file) => {
                if let Some(pos) = self.slots.iter().position(|s| *s == Some(file)) {
                    self.current = pos;
                } else if let Some(pos) = self.slots.iter().position(Option::is_none) {
                    self.slots[pos] = Some(file);
                    self.current = pos;
                }
            }
            Op::Select(n) => {
                if (1..=9).contains(&n) {
                    self.select(n - 1);
                }
            }
            Op::Close(n) => {
                if (1..=8).contains(&n)
                    && self.slots[n - 1].take().is_some()
                    && self.current == n - 1
                {
                    self.current = 8;
                }
            }
            Op::Next => self.cycle(true),
            Op::Previous => self.cycle(false),
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    fn random_slot_operations_keep_the_invariants_and_match_the_model(ops in proptest::collection::vec(op(), 1..40)) {
        let dir = tempfile::tempdir().unwrap();
        let files: Vec<_> = (0..FILES)
            .map(|i| write(dir.path(), &format!("f{i}.pgn"), TWO))
            .collect();
        let mut rig = Rig::new();
        let mut model = Model { slots: [None; 8], current: 8 };
        for op in &ops {
            match op {
                Op::Open(i) => { rig.send(BasesMessage::Open(files[*i].clone())); }
                Op::Select(n) => {
                    // Number 0 and 10 do not name a slot: the message needs a SlotId.
                    if let Some(id) = SlotId::from_number(*n) { rig.send(BasesMessage::Select(id)); }
                }
                Op::Close(n) => {
                    if let Some(id) = SlotId::from_number(*n) { rig.send(BasesMessage::Close(id)); }
                }
                Op::Next => { rig.send(BasesMessage::Next); }
                Op::Previous => { rig.send(BasesMessage::Previous); }
            }
            model.apply(op);

            // Invariants.
            let bases = &rig.state.bases;
            let current = bases.slot(bases.current());
            prop_assert!(current.is_some(), "current points to an empty slot after {op:?}");
            let clip = bases.slot(SlotId::CLIPBASE);
            prop_assert!(clip.is_some_and(|c| c.kind == BaseKind::Clipbase));
            prop_assert!(bases.iter().filter(|(id, _)| id.is_clipbase()).count() == 1);
            for (id, slot) in bases.iter() {
                prop_assert_eq!(slot.kind == BaseKind::Clipbase, id.is_clipbase());
            }
            let paths = bases.open_paths();
            let mut unique = paths.clone();
            unique.sort();
            unique.dedup();
            prop_assert_eq!(paths.len(), unique.len(), "a path is in two slots");
            prop_assert!(bases.iter().count() <= 9);

            // Reference model.
            prop_assert_eq!(bases.current().index(), model.current, "current after {:?}", op);
            for pos in 0..8 {
                let id = SlotId::new(pos).unwrap();
                match (bases.slot(id), model.slots[pos]) {
                    (None, None) => {}
                    (Some(slot), Some(file)) => {
                        let canonical = std::fs::canonicalize(&files[file]).unwrap();
                        prop_assert_eq!(slot.path.as_deref(), Some(canonical.as_path()));
                    }
                    (got, want) => prop_assert!(false, "slot {pos}: got {:?}, want {want:?}", got.map(|s| s.path.clone())),
                }
            }
            prop_assert!(rig.state.discard_prompt.is_none());
        }
    }
}

#[derive(Debug, Clone)]
enum ClipOp {
    ToFile,
    ToClipbase,
    Copy,
    Paste,
    Clear,
    CloseClipbase,
    LoadFile,
    LoadClipbase(u32),
    Play,
}

fn clip_op() -> impl Strategy<Value = ClipOp> {
    prop_oneof![
        1 => Just(ClipOp::ToFile),
        1 => Just(ClipOp::ToClipbase),
        4 => Just(ClipOp::Copy),
        2 => Just(ClipOp::Paste),
        1 => Just(ClipOp::Clear),
        1 => Just(ClipOp::CloseClipbase),
        1 => Just(ClipOp::LoadFile),
        1 => (0u32..6).prop_map(ClipOp::LoadClipbase),
        1 => Just(ClipOp::Play),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    fn random_clipbase_operations_keep_it_consistent(
        limit in 0u32..5,
        ops in proptest::collection::vec(clip_op(), 1..40),
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mut rig = Rig::new();
        rig.state.bases.set_clipbase_limit(limit);
        rig.open(&write(dir.path(), "two.pgn", TWO));
        let mut count = 0u32;
        for op in &ops {
            let in_clip = rig.current().is_clipbase();
            match op {
                ClipOp::ToFile => { rig.send(BasesMessage::Select(sel(1))); }
                ClipOp::ToClipbase => { rig.send(BasesMessage::Select(SlotId::CLIPBASE)); }
                ClipOp::Copy => {
                    rig.send(BasesMessage::ClipbaseCopy);
                    if !in_clip && count < limit { count += 1; }
                }
                ClipOp::Paste => { rig.send(BasesMessage::ClipbasePaste); }
                ClipOp::Clear => { rig.send(BasesMessage::ClipbaseClear); count = 0; }
                ClipOp::CloseClipbase => { rig.send(BasesMessage::Close(SlotId::CLIPBASE)); count = 0; }
                ClipOp::LoadFile => { rig.send(BasesMessage::LoadGame { slot: sel(1), number: 1 }); }
                ClipOp::LoadClipbase(n) => { rig.send(BasesMessage::LoadGame { slot: SlotId::CLIPBASE, number: *n }); }
                ClipOp::Play => {
                    // Only when a legal move exists at the end of the main line.
                    rig.send(GameMessage::Navigate(Nav::GameEnd));
                    let uci = ["e2e4", "d2d4", "g1f3", "b1c3", "e7e5", "d7d5", "g8f6", "b8c6"];
                    if let Some(mv) = uci.iter().find_map(|u| rig.state.session.position().parse_uci(u).ok()) {
                        rig.send(GameMessage::PlayMove { mv, options: PlayOptions::default() });
                    }
                }
            }
            if rig.state.discard_prompt.is_some() {
                rig.send(Message::AnswerDiscard(DiscardChoice::DontSave));
            }
            // A load that was asked for a game number out of range does nothing, so
            // the clipbase count is tracked by the model only for copies.
            let clip = rig.state.bases.slot(SlotId::CLIPBASE).unwrap();
            prop_assert_eq!(clip.total(), count, "after {:?}", op);
            prop_assert_eq!(clip.filtered(), clip.total());
            let rows = rig.state.bases.list_page(SlotId::CLIPBASE, 1, 100);
            prop_assert_eq!(u32::try_from(rows.len()).unwrap(), count);
            prop_assert!(rows.iter().all(|r| !r.unreadable && r.number >= 1));
            prop_assert!(rig.state.bases.slot(rig.current()).is_some());
        }
    }
}
