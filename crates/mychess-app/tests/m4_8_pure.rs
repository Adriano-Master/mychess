//! Contract tests of the pure parts of the engine modes (spec M4-8): the
//! rules of a finished game, the reasons, the notices and the hook seam.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashSet;
use std::path::Path;

use mychess_app::{
    AUTOMOVE_RANGE, AUTOMOVE_RETRIES, AUTOMOVE_RETRY_DELAY, AppState, DEFAULT_AUTOMOVE, Effect,
    ModeKind, ModeNotice, StopReason, TrainingHook, TrainingOutcome, run_hooks, terminal_reason,
};
use mychess_core::{Position, ZobristHash};
use proptest::prelude::*;
use rstest::rstest;
use std::time::Duration;

fn pos(fen: &str) -> Position {
    Position::from_fen(fen).expect("fen")
}

const FOOLS_MATE: &str = "rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3";

#[rstest]
#[case::mate(FOOLS_MATE, StopReason::Checkmate)]
#[case::stalemate("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1", StopReason::Stalemate)]
#[case::kvk("8/8/4k3/8/8/3K4/8/8 w - - 0 1", StopReason::InsufficientMaterial)]
#[case::knvk("8/8/4k3/8/8/3KN3/8/8 w - - 0 1", StopReason::InsufficientMaterial)]
#[case::kbvk("8/8/4k3/8/8/3KB3/8/8 w - - 0 1", StopReason::InsufficientMaterial)]
#[case::kbvkb_same_colour("8/8/3bk3/8/8/3KB3/8/8 w - - 0 1", StopReason::InsufficientMaterial)]
#[case::fifty("4k3/8/8/8/8/8/4P3/4K3 w - - 100 80", StopReason::FiftyMoves)]
#[case::fifty_over("4k3/8/8/8/8/8/4P3/4K3 b - - 130 80", StopReason::FiftyMoves)]
fn terminal_positions_have_their_reason(#[case] fen: &str, #[case] reason: StopReason) {
    assert_eq!(terminal_reason(&pos(fen), &[]), Some(reason), "{fen}");
}

#[rstest]
#[case::start("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")]
#[case::kbvkb_opposite_colours("8/8/2b1k3/8/8/3KB3/8/8 w - - 0 1")]
#[case::knvkn("8/8/3nk3/8/8/3KN3/8/8 w - - 0 1")]
#[case::forty_nine_moves("4k3/8/8/8/8/8/4P3/4K3 w - - 99 80")]
#[case::check_but_not_mate("4k3/8/8/8/8/8/4r3/4K3 w - - 0 1")]
fn playable_positions_are_not_terminal(#[case] fen: &str) {
    assert_eq!(terminal_reason(&pos(fen), &[]), None, "{fen}");
}

#[test]
fn the_third_occurrence_is_a_repetition_but_the_second_is_not() {
    let p = Position::new();
    let h = p.zobrist();
    assert_eq!(terminal_reason(&p, &[]), None);
    assert_eq!(terminal_reason(&p, &[h]), None);
    assert_eq!(terminal_reason(&p, &[h, h]), Some(StopReason::Repetition));
}

#[test]
fn unrelated_history_never_repeats() {
    let p = Position::new();
    let other = pos("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").zobrist();
    assert_eq!(terminal_reason(&p, &[other; 50]), None);
}

#[test]
fn mate_wins_over_the_draw_rules() {
    let mate = pos(FOOLS_MATE);
    let h = mate.zobrist();
    assert_eq!(terminal_reason(&mate, &[h, h]), Some(StopReason::Checkmate));
    let mate_at_100 = pos("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 100 3");
    assert_eq!(
        terminal_reason(&mate_at_100, &[]),
        Some(StopReason::Checkmate)
    );
}

proptest! {
    #[test]
    fn terminal_reason_never_panics_on_any_history(hashes in proptest::collection::vec(any::<u64>(), 0..64)) {
        let p = Position::new();
        let history: Vec<ZobristHash> = hashes
            .iter()
            .map(|_| p.zobrist())
            .collect();
        let r = terminal_reason(&p, &history);
        // Two earlier equal positions make a threefold repetition.
        prop_assert_eq!(r.is_some(), history.len() >= 2);
    }
}

const ALL_REASONS: [StopReason; 9] = [
    StopReason::Checkmate,
    StopReason::Stalemate,
    StopReason::InsufficientMaterial,
    StopReason::FiftyMoves,
    StopReason::Repetition,
    StopReason::NoMove,
    StopReason::EngineError,
    StopReason::UserStopped,
    StopReason::GameChanged,
];

#[test]
fn every_stop_reason_has_its_own_stable_key() {
    let keys: Vec<&str> = ALL_REASONS.iter().map(|r| r.message_key()).collect();
    let unique: HashSet<&&str> = keys.iter().collect();
    assert_eq!(unique.len(), keys.len(), "{keys:?}");
    for key in keys {
        assert!(key.starts_with("analysis-mode-ended-"), "{key}");
    }
    assert_eq!(
        StopReason::Checkmate.message_key(),
        "analysis-mode-ended-checkmate"
    );
    assert_eq!(
        StopReason::UserStopped.message_key(),
        "analysis-mode-ended-user-stopped"
    );
}

#[test]
fn notice_keys_are_distinct_and_ended_uses_the_reason_key() {
    let kinds = [ModeKind::Threat, ModeKind::ShootOut, ModeKind::Automove];
    let simple = [
        ModeNotice::NullMoveInCheck,
        ModeNotice::Conflict,
        ModeNotice::TrialMode,
    ];
    let mut keys: Vec<&str> = simple.iter().map(ModeNotice::message_key).collect();
    for reason in ALL_REASONS {
        for kind in kinds {
            assert_eq!(
                ModeNotice::Ended { kind, reason }.message_key(),
                reason.message_key()
            );
        }
        keys.push(reason.message_key());
    }
    let unique: HashSet<&&str> = keys.iter().collect();
    assert_eq!(unique.len(), keys.len(), "{keys:?}");
}

fn ftl(locale: &str) -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(locale);
    let mut text = String::new();
    for entry in std::fs::read_dir(dir).expect("i18n dir") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "ftl") {
            text.push_str(&std::fs::read_to_string(path).expect("ftl"));
            text.push('\n');
        }
    }
    text
}

fn defines(text: &str, key: &str) -> bool {
    text.lines()
        .any(|l| l.split('=').next().is_some_and(|k| k.trim() == key))
}

#[test]
fn the_null_move_in_check_notice_is_translated() {
    let key = ModeNotice::NullMoveInCheck.message_key();
    for locale in ["en-US", "pt-BR"] {
        assert!(defines(&ftl(locale), key), "{key} missing in {locale}");
    }
}

#[test]
fn every_mode_key_is_translated_in_both_locales() {
    for locale in ["en-US", "pt-BR"] {
        let text = ftl(locale);
        for key in ALL_REASONS.iter().map(|r| r.message_key()).chain([
            ModeNotice::Conflict.message_key(),
            ModeNotice::TrialMode.message_key(),
        ]) {
            assert!(defines(&text, key), "{key} missing in {locale}");
        }
    }
}

#[test]
fn the_constants_follow_the_spec() {
    assert_eq!(DEFAULT_AUTOMOVE, Duration::from_secs(3));
    assert_eq!(*AUTOMOVE_RANGE.start(), Duration::from_millis(100));
    assert_eq!(*AUTOMOVE_RANGE.end(), Duration::from_secs(300));
    assert_eq!(AUTOMOVE_RETRY_DELAY, Duration::from_secs(1));
    assert_eq!(AUTOMOVE_RETRIES, 10);
}

struct Recorder<'a> {
    name: &'static str,
    outcome: TrainingOutcome,
    log: &'a std::cell::RefCell<Vec<&'static str>>,
}

impl TrainingHook for Recorder<'_> {
    fn after_user_move(&mut self, _: &mut AppState, _: &mut Vec<Effect>) -> TrainingOutcome {
        self.log.borrow_mut().push(self.name);
        self.outcome
    }
}

#[test]
fn hooks_run_in_order_automove_tablebase_tree_until_one_handles() {
    let log = std::cell::RefCell::new(Vec::new());
    let mut automove = Recorder {
        name: "automove",
        outcome: TrainingOutcome::Continue,
        log: &log,
    };
    let mut tablebase = Recorder {
        name: "tablebase",
        outcome: TrainingOutcome::Handled,
        log: &log,
    };
    let mut tree = Recorder {
        name: "tree",
        outcome: TrainingOutcome::Continue,
        log: &log,
    };
    let mut state = AppState::default();
    let handled = run_hooks(
        &mut [&mut automove, &mut tablebase, &mut tree],
        &mut state,
        &mut Vec::new(),
    );
    assert!(handled);
    assert_eq!(*log.borrow(), ["automove", "tablebase"]);
}

#[test]
fn all_hooks_run_when_nobody_handles_and_none_means_false() {
    let log = std::cell::RefCell::new(Vec::new());
    let mut a = Recorder {
        name: "a",
        outcome: TrainingOutcome::Continue,
        log: &log,
    };
    let mut b = Recorder {
        name: "b",
        outcome: TrainingOutcome::Continue,
        log: &log,
    };
    let mut state = AppState::default();
    assert!(!run_hooks(
        &mut [&mut a, &mut b],
        &mut state,
        &mut Vec::new()
    ));
    assert_eq!(*log.borrow(), ["a", "b"]);
    assert!(!run_hooks(&mut [], &mut state, &mut Vec::new()));
}
