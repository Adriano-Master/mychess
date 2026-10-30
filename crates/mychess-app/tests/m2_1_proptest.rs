//! Spec m2-1, acceptance criterion "Desfazer/refazer/alterada/tentativa":
//! random operation sequences and the invariants (a)-(e), plus hostile input.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m2_1_common;

use std::collections::HashSet;
use std::time::Duration;

use m2_1_common::*;
use mychess_app::{
    EditSettings, GameMessage, GameSession, MoveEntrySettings, Nav, Notice, PlayOptions, Prompt,
    ReplaceChoice, Revision, UNDO_LIMIT, VariationTarget,
};
use proptest::prelude::*;
use proptest::test_runner::{FileFailurePersistence, RngSeed};

// ---- operations ----

#[derive(Debug, Clone)]
enum Op {
    Play { pick: usize, force: bool },
    PlayNull,
    PlaySquares { from: u8, to: u8, promo: u8 },
    StartVariation,
    Answer(u8),
    AnswerPromotion(u8),
    Dismiss,
    Promote(usize),
    PromoteCurrent,
    MakeFirst(usize),
    Delete(usize),
    DeleteCurrent,
    TruncateEnd,
    TruncateFrom,
    TruncateStart,
    StripComments,
    StripVariations,
    StripNags,
    Comment(usize, u8),
    CommentBefore(usize, u8),
    AddNag(u8),
    ToggleNag(u8),
    RemoveNag(u8),
    ClearNags,
    SetNags(Vec<u8>),
    Undo,
    Redo,
    Nav(u8, u32, usize),
    EnterTrial(bool),
    ExitTrial,
    ToggleTrial(bool),
    MarkSaved,
    Autoplay(u8),
    Tick(i8),
    Delay(u64),
    Chooser(u8, i8),
    Entry(u8, char),
    Text(usize),
    Settings(u8),
}

const TEXTS: &[&str] = &[
    "e4", "Nf3", "--", "O-O", "e2e4", "zzz", "", "a8=Q", "0-0-0", "d5", "Qh5+", "e7e5", "é♞",
];
const COMMENTS: &[&str] = &["", "  ", "note", "[%clk 0:00:01] x", "é♞", "{}"];

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (any::<usize>(), any::<bool>()).prop_map(|(pick, force)| Op::Play { pick, force }),
        1 => Just(Op::PlayNull),
        1 => (0u8..64, 0u8..64, 0u8..6).prop_map(|(from, to, promo)| Op::PlaySquares { from, to, promo }),
        1 => Just(Op::StartVariation),
        2 => any::<u8>().prop_map(Op::Answer),
        1 => any::<u8>().prop_map(Op::AnswerPromotion),
        1 => Just(Op::Dismiss),
        1 => any::<usize>().prop_map(Op::Promote),
        1 => Just(Op::PromoteCurrent),
        1 => any::<usize>().prop_map(Op::MakeFirst),
        1 => any::<usize>().prop_map(Op::Delete),
        1 => Just(Op::DeleteCurrent),
        1 => Just(Op::TruncateEnd),
        1 => Just(Op::TruncateFrom),
        1 => Just(Op::TruncateStart),
        1 => Just(Op::StripComments),
        1 => Just(Op::StripVariations),
        1 => Just(Op::StripNags),
        2 => (any::<usize>(), any::<u8>()).prop_map(|(n, t)| Op::Comment(n, t)),
        1 => (any::<usize>(), any::<u8>()).prop_map(|(n, t)| Op::CommentBefore(n, t)),
        2 => any::<u8>().prop_map(Op::AddNag),
        1 => any::<u8>().prop_map(Op::ToggleNag),
        1 => any::<u8>().prop_map(Op::RemoveNag),
        1 => Just(Op::ClearNags),
        1 => proptest::collection::vec(any::<u8>(), 0..6).prop_map(Op::SetNags),
        3 => Just(Op::Undo),
        2 => Just(Op::Redo),
        4 => (0u8..12, 0u32..8, any::<usize>()).prop_map(|(k, n, i)| Op::Nav(k, n, i)),
        1 => any::<bool>().prop_map(Op::EnterTrial),
        1 => Just(Op::ExitTrial),
        1 => any::<bool>().prop_map(Op::ToggleTrial),
        1 => Just(Op::MarkSaved),
        1 => (0u8..3).prop_map(Op::Autoplay),
        1 => (-1i8..2).prop_map(Op::Tick),
        1 => any::<u64>().prop_map(Op::Delay),
        1 => (0u8..4, -3i8..4).prop_map(|(k, d)| Op::Chooser(k, d)),
        2 => (0u8..4, any::<char>()).prop_map(|(k, c)| Op::Entry(k, c)),
        1 => (0usize..TEXTS.len()).prop_map(Op::Text),
        1 => (0u8..8).prop_map(Op::Settings),
    ]
}

fn ops_strategy(max: usize) -> impl Strategy<Value = Vec<Op>> {
    proptest::collection::vec(op_strategy(), 0..max)
}

fn config() -> ProptestConfig {
    let cases = std::env::var("M2_1_PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(96);
    ProptestConfig {
        cases,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "proptest-regressions/m2_1_proptest.txt",
        ))),
        rng_seed: RngSeed::Fixed(0x4d32_2d31),
        ..ProptestConfig::default()
    }
}

// ---- driver ----

struct Driver {
    s: GameSession,
    pool: Vec<NodeId>,
    seen: HashSet<Revision>,
    saved: Option<Revision>,
}

impl Driver {
    fn new(game: Game) -> Driver {
        let s = GameSession::new(game);
        let mut d = Driver {
            saved: Some(s.revision()),
            seen: HashSet::new(),
            pool: Vec::new(),
            s,
        };
        d.refresh_pool();
        d.seen.insert(d.s.revision());
        d
    }

    fn refresh_pool(&mut self) {
        let root = self.s.game().root();
        if !self.pool.contains(&root) {
            self.pool.push(root);
        }
        for id in self.s.game().descendants(root).unwrap() {
            if !self.pool.contains(&id) {
                self.pool.push(id);
            }
        }
        if self.pool.len() > 200 {
            self.pool.drain(..self.pool.len() - 200);
        }
    }

    fn node(&self, i: usize) -> NodeId {
        self.pool[i % self.pool.len()]
    }

    fn category_nag(v: u8) -> Nag {
        Nag::new(u16::from(v % 30) + 1).unwrap()
    }

    #[allow(clippy::too_many_lines)]
    fn message(&self, op: &Op) -> Option<GameMessage> {
        let s = &self.s;
        Some(match op {
            Op::Play { pick, force } => {
                let legal = s.position().legal_moves();
                let mv = if legal.is_empty() {
                    Move::Null
                } else {
                    legal[pick % legal.len()]
                };
                GameMessage::PlayMove {
                    mv,
                    options: PlayOptions {
                        force_variation: *force,
                    },
                }
            }
            Op::PlayNull => GameMessage::PlayNullMove,
            Op::PlaySquares { from, to, promo } => GameMessage::PlaySquares {
                from: Square::new(u32::from(*from)),
                to: Square::new(u32::from(*to)),
                promotion: [
                    None,
                    Some(Role::Queen),
                    Some(Role::Rook),
                    Some(Role::Bishop),
                    Some(Role::Knight),
                    Some(Role::King),
                ][usize::from(*promo) % 6],
                options: PlayOptions::default(),
            },
            Op::StartVariation => GameMessage::StartVariation,
            Op::Answer(c) => GameMessage::AnswerReplace(
                [
                    ReplaceChoice::Replace,
                    ReplaceChoice::NewMainLine,
                    ReplaceChoice::AddVariation,
                    ReplaceChoice::Trial,
                    ReplaceChoice::Cancel,
                ][usize::from(*c) % 5],
            ),
            Op::AnswerPromotion(c) => GameMessage::AnswerPromotion(
                [
                    None,
                    Some(Role::Queen),
                    Some(Role::Knight),
                    Some(Role::Pawn),
                ][usize::from(*c) % 4],
            ),
            Op::Dismiss => GameMessage::DismissPrompt,
            Op::Promote(i) => GameMessage::PromoteVariation(VariationTarget::Node(self.node(*i))),
            Op::PromoteCurrent => GameMessage::PromoteVariation(VariationTarget::Current),
            Op::MakeFirst(i) => {
                GameMessage::MakeFirstVariation(VariationTarget::Node(self.node(*i)))
            }
            Op::Delete(i) => GameMessage::DeleteVariation(VariationTarget::Node(self.node(*i))),
            Op::DeleteCurrent => GameMessage::DeleteVariation(VariationTarget::Current),
            Op::TruncateEnd => GameMessage::TruncateEnd,
            Op::TruncateFrom => GameMessage::TruncateFrom,
            Op::TruncateStart => GameMessage::TruncateStart,
            Op::StripComments => GameMessage::StripComments,
            Op::StripVariations => GameMessage::StripVariations,
            Op::StripNags => GameMessage::StripNags,
            Op::Comment(i, t) => GameMessage::SetComment {
                node: self.node(*i),
                text: COMMENTS[usize::from(*t) % COMMENTS.len()].to_owned(),
            },
            Op::CommentBefore(i, t) => GameMessage::SetCommentBefore {
                node: self.node(*i),
                text: COMMENTS[usize::from(*t) % COMMENTS.len()].to_owned(),
            },
            Op::AddNag(v) => GameMessage::AddNag(Self::category_nag(*v)),
            Op::ToggleNag(v) => GameMessage::ToggleNag(Self::category_nag(*v)),
            Op::RemoveNag(v) => GameMessage::RemoveNag(Self::category_nag(*v)),
            Op::ClearNags => GameMessage::ClearNags,
            Op::SetNags(v) => {
                GameMessage::SetNags(v.iter().map(|x| Self::category_nag(*x)).collect())
            }
            Op::Undo => GameMessage::Undo,
            Op::Redo => GameMessage::Redo,
            Op::Nav(k, n, i) => GameMessage::Navigate(match k {
                0 => Nav::Start,
                1 => Nav::GameEnd,
                2 => Nav::Home,
                3 => Nav::End,
                4 => Nav::Back(*n),
                5 => Nav::Forward(*n),
                6 => Nav::ExitVariation,
                7 => Nav::EnterVariation(*i % 4),
                8 => Nav::GoToNode(self.node(*i)),
                9 => Nav::GoToMainlinePly(*n * 3),
                10 => Nav::GoToMoveNumber(*n),
                _ => Nav::GoToBlackMoveNumber(*n),
            }),
            Op::EnterTrial(b) => GameMessage::EnterTrial { null_move: *b },
            Op::ExitTrial => GameMessage::ExitTrial,
            Op::ToggleTrial(b) => GameMessage::ToggleTrial { null_move: *b },
            Op::MarkSaved => return None,
            Op::Autoplay(k) => match k {
                0 => GameMessage::StartAutoplay,
                1 => GameMessage::StopAutoplay,
                _ => GameMessage::ToggleAutoplay,
            },
            Op::Tick(d) => GameMessage::AutoplayTick {
                generation: s.autoplay().generation.wrapping_add_signed(i64::from(*d)),
            },
            Op::Delay(ms) => GameMessage::SetAutoplayDelay(Duration::from_millis(*ms)),
            Op::Chooser(k, d) => match k {
                0 => GameMessage::OpenVariationChooser,
                1 => GameMessage::ChooserMove(isize::from(*d)),
                2 => GameMessage::ChooserConfirm(usize::try_from(*d).ok()),
                _ => GameMessage::CloseVariationChooser,
            },
            Op::Entry(k, c) => match k {
                0 | 1 => GameMessage::MoveEntryChar(*c),
                2 => GameMessage::MoveEntryBackspace,
                _ => {
                    if u32::from(*c).is_multiple_of(2) {
                        GameMessage::MoveEntryCommit
                    } else {
                        GameMessage::MoveEntryClear
                    }
                }
            },
            Op::Text(t) => GameMessage::EnterMoveText(TEXTS[*t % TEXTS.len()].into()),
            Op::Settings(k) => GameMessage::ApplySettings(EditSettings {
                ask_to_replace_moves: k & 1 == 0,
                show_var_popup: k & 2 != 0,
                move_entry: MoveEntrySettings {
                    enabled: k & 4 == 0,
                    auto_expand: k & 1 != 0,
                    coordinate: k & 2 == 0,
                },
                autoplay_delay: Duration::from_secs(u64::from(*k)),
                exclusive_nags: k & 4 == 0,
            }),
        })
    }

    /// Applies one operation and checks the per-step invariants.
    fn apply(&mut self, op: &Op) {
        let had_prompt = self.s.prompt().is_some();
        let before_rev = self.s.revision();
        let Some(msg) = self.message(op) else {
            // In trial mode `revision()` is still the pre-trial one, which
            // is the state `mark_saved` records.
            self.s.mark_saved();
            self.saved = Some(self.s.revision());
            self.check(op, had_prompt, before_rev);
            return;
        };
        let stale_target = stale_node_in(&msg, self.s.game());
        let effects = self.s.update(msg.clone());
        for e in &effects {
            let mychess_app::Effect::ScheduleAutoplayTick { generation, delay } = e else {
                continue;
            };
            assert_eq!(*generation, self.s.autoplay().generation);
            assert!(*delay >= Duration::from_millis(100) && *delay <= Duration::from_secs(300));
        }
        if had_prompt && !allowed_with_prompt(&msg) {
            assert!(
                matches!(msg, GameMessage::AutoplayTick { .. })
                    || self.s.notice() == Some(&Notice::PromptPending),
                "{msg:?}"
            );
        } else if stale_target {
            assert_eq!(self.s.notice(), Some(&Notice::StaleNode), "{msg:?}");
        }
        self.check(op, had_prompt, before_rev);
    }

    fn check(&mut self, op: &Op, _had_prompt: bool, before_rev: Revision) {
        let s = &self.s;
        // (a) invariants of rule 6
        assert!(s.game().contains(s.cursor()), "cursor alive after {op:?}");
        match s.prompt() {
            Some(Prompt::ReplaceMove(p)) => assert!(s.game().contains(p.parent)),
            Some(Prompt::ChoosePromotion { .. } | Prompt::ConfirmStrip { .. }) | None => {}
        }
        if let Some(c) = s.chooser() {
            assert!(s.game().contains(c.parent));
            assert!(c.selected < s.game().children(c.parent).unwrap().len().max(1));
        }
        if let Some(p) = s.pending_variation() {
            assert!(s.game().contains(p));
        }
        assert!(
            !(s.autoplay().running && s.prompt().is_some()),
            "autoplay with prompt"
        );
        // (c)
        assert!(s.undo_depth() <= UNDO_LIMIT);
        // (d) dirty coherent with the saved revision
        assert_eq!(
            s.is_dirty(),
            Some(s.revision()) != self.saved,
            "dirty after {op:?}"
        );
        // revisions of new states are never reused
        let rev = s.revision();
        if rev != before_rev {
            let reverting = matches!(
                op,
                Op::Undo | Op::Redo | Op::ExitTrial | Op::ToggleTrial(_) | Op::Answer(_)
            );
            if !reverting {
                assert!(!self.seen.contains(&rev), "revision reused after {op:?}");
            }
        }
        self.seen.insert(rev);
        // trial mode never creates undo points or revisions
        self.refresh_pool();
    }
}

fn allowed_with_prompt(m: &GameMessage) -> bool {
    matches!(
        m,
        GameMessage::AnswerReplace(_)
            | GameMessage::AnswerPromotion(_)
            | GameMessage::DismissPrompt
            | GameMessage::ApplySettings(_)
    )
}

fn stale_node_in(m: &GameMessage, game: &Game) -> bool {
    match m {
        GameMessage::SetComment { node, .. } | GameMessage::SetCommentBefore { node, .. } => {
            !game.contains(*node)
        }
        GameMessage::PromoteVariation(VariationTarget::Node(n))
        | GameMessage::MakeFirstVariation(VariationTarget::Node(n))
        | GameMessage::DeleteVariation(VariationTarget::Node(n))
        | GameMessage::Navigate(Nav::GoToNode(n)) => !game.contains(*n),
        _ => false,
    }
}

fn start_game() -> Game {
    let (g, _) = nested_game();
    g
}

proptest! {
    #![proptest_config(config())]

    /// (a), (c), (d): the invariants hold after every message of a random run.
    #[test]
    fn invariants_hold_over_random_operation_sequences(ops in ops_strategy(60)) {
        let mut d = Driver::new(start_game());
        for (i, op) in ops.iter().enumerate() {
            // Resolve open prompts now and then so that the run keeps editing.
            if d.s.prompt().is_some() && i % 3 == 0 {
                let choice = [
                    ReplaceChoice::Replace,
                    ReplaceChoice::NewMainLine,
                    ReplaceChoice::AddVariation,
                    ReplaceChoice::Cancel,
                ][i / 3 % 4];
                d.s.update(GameMessage::AnswerReplace(choice));
                d.check(op, true, d.s.revision());
            }
            d.apply(op);
        }
    }

    /// The same, starting from a minimal game.
    #[test]
    fn invariants_hold_from_an_empty_game(ops in ops_strategy(40)) {
        let mut d = Driver::new(Game::new());
        for op in &ops {
            d.apply(op);
        }
    }

    /// (e): leaving trial mode gives back exactly the game of before.
    #[test]
    fn exiting_trial_restores_game_cursor_revision_and_dirty(
        prefix in ops_strategy(20),
        trial in ops_strategy(25),
        null_move in any::<bool>(),
    ) {
        let mut d = Driver::new(start_game());
        for op in &prefix {
            d.apply(op);
        }
        // Trial can be entered only without a prompt; close it first.
        if d.s.prompt().is_some() {
            d.s.update(GameMessage::DismissPrompt);
        }
        if d.s.in_trial() {
            d.s.update(GameMessage::ExitTrial);
        }
        let (game, cursor, rev, dirty, depth, can_redo) = (
            d.s.game().clone(), d.s.cursor(), d.s.revision(), d.s.is_dirty(),
            d.s.undo_depth(), d.s.can_redo(),
        );
        d.s.update(GameMessage::EnterTrial { null_move });
        prop_assert!(d.s.in_trial());
        for op in &trial {
            if matches!(op, Op::MarkSaved | Op::ExitTrial | Op::ToggleTrial(_)) {
                continue; // those would end or alter the trial by design
            }
            d.apply(op);
            prop_assert!(d.s.in_trial(), "{:?} left trial mode", op);
            prop_assert_eq!(d.s.undo_depth(), depth, "no undo point in trial after {:?}", op);
            prop_assert_eq!(d.s.is_dirty(), dirty, "dirty shows the old value after {:?}", op);
        }
        if d.s.prompt().is_some() {
            d.s.update(GameMessage::DismissPrompt);
        }
        d.s.update(GameMessage::ExitTrial);
        prop_assert!(!d.s.in_trial());
        prop_assert!(d.s.game().same_content(&game));
        prop_assert_eq!(d.s.cursor(), cursor);
        prop_assert_eq!(d.s.revision(), rev);
        prop_assert_eq!(d.s.is_dirty(), dirty);
        prop_assert_eq!(d.s.undo_depth(), depth);
        prop_assert_eq!(d.s.can_redo(), can_redo);
    }

    /// (b): n effective edits then n undos give the initial game; n redos the final one.
    #[test]
    fn undo_redo_round_trip(ops in ops_strategy(40)) {
        let mut s = GameSession::new(start_game());
        s.update(GameMessage::ApplySettings(EditSettings {
            ask_to_replace_moves: false,
            ..EditSettings::default()
        }));
        let mut d = Driver { saved: Some(s.revision()), seen: HashSet::new(), pool: Vec::new(), s };
        d.refresh_pool();
        let mut before: Vec<(Game, NodeId)> = Vec::new();
        for op in &ops {
            // Only edits and navigation: undo/redo/trial/settings are out.
            if matches!(
                op,
                Op::Undo | Op::Redo | Op::EnterTrial(_) | Op::ExitTrial | Op::ToggleTrial(_)
                    | Op::Settings(_) | Op::MarkSaved | Op::Answer(_)
            ) {
                continue;
            }
            if d.s.prompt().is_some() {
                d.s.update(GameMessage::DismissPrompt);
            }
            if before.len() >= UNDO_LIMIT {
                break;
            }
            let snapshot = (d.s.game().clone(), d.s.cursor());
            let depth = d.s.undo_depth();
            d.apply(op);
            if d.s.undo_depth() > depth {
                before.push(snapshot);
            }
        }
        if d.s.prompt().is_some() {
            d.s.update(GameMessage::DismissPrompt);
        }
        let (final_game, final_cursor) = (d.s.game().clone(), d.s.cursor());
        let n = d.s.undo_depth();
        prop_assert_eq!(n, before.len());
        for (game, cursor) in before.iter().rev() {
            d.s.update(GameMessage::Undo);
            prop_assert!(d.s.game().same_content(game));
            prop_assert_eq!(d.s.cursor(), *cursor);
        }
        prop_assert!(!d.s.can_undo());
        if let Some((first, _)) = before.first() {
            prop_assert!(d.s.game().same_content(first));
            prop_assert!(!d.s.is_dirty());
        }
        for _ in 0..n {
            d.s.update(GameMessage::Redo);
        }
        prop_assert!(d.s.game().same_content(&final_game));
        prop_assert_eq!(d.s.cursor(), final_cursor);
    }

    /// Navigation never changes the game, the revision or the history.
    #[test]
    fn navigation_is_cursor_only(ops in ops_strategy(25), navs in proptest::collection::vec((0u8..12, 0u32..8, any::<usize>()), 1..20)) {
        let mut d = Driver::new(start_game());
        for op in &ops {
            d.apply(op);
        }
        if d.s.prompt().is_some() {
            d.s.update(GameMessage::DismissPrompt);
        }
        let (game, rev, depth, dirty) = (d.s.game().clone(), d.s.revision(), d.s.undo_depth(), d.s.is_dirty());
        for (k, n, i) in navs {
            d.apply(&Op::Nav(k, n, i));
            prop_assert!(d.s.game().same_content(&game));
            prop_assert_eq!(d.s.revision(), rev);
            prop_assert_eq!(d.s.undo_depth(), depth);
            prop_assert_eq!(d.s.is_dirty(), dirty);
        }
    }

    /// Typing arbitrary characters never panics and the text only holds
    /// normalized characters.
    #[test]
    fn keyboard_entry_text_is_always_normalized(chars in proptest::collection::vec(any::<char>(), 0..40)) {
        let mut s = session_fen("r3k2r/pppp1ppp/8/8/8/8/PPPPPPPP/R3K2R w KQkq - 0 1");
        for c in chars {
            s.update(GameMessage::MoveEntryChar(c));
            let text = s.move_entry().text();
            prop_assert!(text.chars().all(|ch| matches!(ch, 'N'|'R'|'Q'|'K'|'O'|'B'|'a'..='h'|'1'..='8')));
            prop_assert!(s.game().contains(s.cursor()));
        }
    }
}

// ---- hostile input ----

#[test]
fn stale_ids_after_replace_game_never_panic() {
    let (mut s, n) = nested_session();
    s.replace_game(Game::new());
    let ids = [n.m1, n.a1, n.c2, n.b2];
    for id in ids {
        for msg in [
            GameMessage::Navigate(Nav::GoToNode(id)),
            GameMessage::SetComment {
                node: id,
                text: "x".into(),
            },
            GameMessage::SetCommentBefore {
                node: id,
                text: "x".into(),
            },
            GameMessage::PromoteVariation(VariationTarget::Node(id)),
            GameMessage::MakeFirstVariation(VariationTarget::Node(id)),
            GameMessage::DeleteVariation(VariationTarget::Node(id)),
        ] {
            s.update(msg);
            assert_eq!(s.notice(), Some(&Notice::StaleNode));
            assert_eq!(s.game().node_count(), 1);
        }
    }
}

#[test]
fn ids_from_another_game_with_the_same_shape_are_stale() {
    let (g1, n1) = nested_game();
    let (g2, _) = nested_game();
    let mut s = GameSession::new(g2);
    drop(g1);
    s.update(GameMessage::Navigate(Nav::GoToNode(n1.m3)));
    assert_eq!(s.notice(), Some(&Notice::StaleNode));
    assert_eq!(s.cursor(), s.game().root());
}

#[test]
fn hostile_texts_in_every_text_message_do_not_panic() {
    let hostile: Vec<String> = vec![
        "é".repeat(1 << 19),
        "\0".repeat(1 << 20),
        "e4 ".repeat(300_000),
        "\u{202e}\u{200b}\u{feff}".repeat(1000),
        "\r\n\t".repeat(1000),
        "♞".to_owned(),
        String::new(),
    ];
    let (mut s, n) = nested_session();
    for text in hostile {
        s.update(GameMessage::EnterMoveText(text.clone()));
        s.update(GameMessage::SetComment {
            node: n.m3,
            text: text.clone(),
        });
        s.update(GameMessage::SetCommentBefore {
            node: n.m3,
            text: text.clone(),
        });
        for ch in text.chars().take(2000) {
            s.update(GameMessage::MoveEntryChar(ch));
        }
        s.update(GameMessage::MoveEntryCommit);
        s.update(GameMessage::MoveEntryClear);
        assert!(s.game().contains(s.cursor()));
    }
    // Everything is undoable and the session is still consistent.
    while s.can_undo() {
        s.update(GameMessage::Undo);
    }
}

#[test]
fn extreme_numbers_do_not_panic() {
    let (mut s, n) = nested_session();
    for nav in [
        Nav::Back(u32::MAX),
        Nav::Forward(u32::MAX),
        Nav::EnterVariation(usize::MAX),
        Nav::GoToMainlinePly(u32::MAX),
        Nav::GoToMoveNumber(u32::MAX),
        Nav::GoToBlackMoveNumber(u32::MAX),
        Nav::GoToMoveNumber(0),
        Nav::GoToBlackMoveNumber(0),
    ] {
        s.update(GameMessage::Navigate(nav));
        assert!(s.game().contains(s.cursor()));
    }
    goto(&mut s, n.m1);
    s.update(GameMessage::OpenVariationChooser);
    s.update(GameMessage::ChooserMove(isize::MIN));
    s.update(GameMessage::ChooserMove(isize::MAX));
    s.update(GameMessage::ChooserConfirm(Some(usize::MAX)));
    s.update(GameMessage::SetAutoplayDelay(Duration::MAX));
    s.update(GameMessage::AutoplayTick {
        generation: u64::MAX,
    });
    assert!(s.game().contains(s.cursor()));
}

#[test]
fn a_thousand_edits_and_undos_stay_consistent_and_bounded() {
    let mut s = GameSession::new(line(&["e2e4", "e7e5"]));
    let m1 = s.game().children(s.game().root()).unwrap()[0];
    for i in 0..1000 {
        s.update(GameMessage::SetComment {
            node: m1,
            text: format!("c{i}"),
        });
        assert!(s.undo_depth() <= UNDO_LIMIT);
    }
    while s.can_undo() {
        s.update(GameMessage::Undo);
    }
    assert!(s.is_dirty());
    assert_eq!(s.game().comment(m1).unwrap(), Some("c979"));
}
