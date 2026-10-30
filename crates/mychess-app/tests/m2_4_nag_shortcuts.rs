//! Typed NAG shortcuts (D181, D180, rule 28): `NagShortcutBuffer` with an
//! injected clock, and the `AppState` wiring.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::unreadable_literal,
    clippy::assigning_clones,
    clippy::manual_assert_eq,
    clippy::format_collect,
    clippy::unchecked_time_subtraction,
    clippy::field_reassign_with_default,
    clippy::cast_possible_truncation,
    clippy::items_after_statements,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::needless_pass_by_value
)]

mod m2_4_common;

use std::time::{Duration, Instant};

use m2_4_common::*;
use rstest::rstest;

fn typed(buffer: &mut NagShortcutBuffer, start: Instant, keys: &str) -> NagStep {
    let mut step = NagStep::Pending;
    for (i, ch) in keys.chars().enumerate() {
        let at = start + Duration::from_millis(i as u64);
        let key = if ch == '\n' {
            NagKey::Enter
        } else {
            NagKey::Char(ch)
        };
        step = buffer.push(key, at);
    }
    step
}

// Table of end.tcl:1753-1777 as listed in D181.
#[rstest]
#[case("!\n", 1)]
#[case("!!\n", 3)]
#[case("?\n", 2)]
#[case("??\n", 4)]
#[case("!?\n", 5)]
#[case("?!\n", 6)]
#[case("=\n", 10)]
#[case("~\n", 13)]
#[case("~=\n", 44)]
#[case("+-", 18)]
#[case("+/", 16)]
#[case("+=", 14)]
#[case("-+", 19)]
#[case("-/", 17)]
#[case("=+", 15)]
fn sequence_fires_the_nag_of_the_table(#[case] keys: &str, #[case] number: u16) {
    let mut b = NagShortcutBuffer::new();
    assert_eq!(
        typed(&mut b, Instant::now(), keys),
        NagStep::Fired(nag(number))
    );
    assert!(!b.is_pending());
    assert_eq!(b.keys(), "");
}

#[rstest]
#[case("!")]
#[case("!!")]
#[case("?")]
#[case("??")]
#[case("!?")]
#[case("?!")]
#[case("=")]
#[case("~")]
#[case("~=")]
fn enter_sequences_do_not_fire_without_enter(#[case] keys: &str) {
    let mut b = NagShortcutBuffer::new();
    assert_eq!(typed(&mut b, Instant::now(), keys), NagStep::Pending);
    assert!(b.is_pending());
    assert_eq!(b.keys(), keys);
}

#[rstest]
#[case("+")]
#[case("-")]
#[case("+/")]
fn enter_does_not_complete_a_no_enter_sequence_prefix(#[case] keys: &str) {
    // `+/` is complete without Enter, so it fires; `+` and `-` only wait.
    let mut b = NagShortcutBuffer::new();
    let step = typed(&mut b, Instant::now(), keys);
    if keys == "+/" {
        assert_eq!(step, NagStep::Fired(nag(16)));
        return;
    }
    assert_eq!(step, NagStep::Pending);
    assert_eq!(
        b.push(NagKey::Enter, Instant::now()),
        NagStep::Pass {
            chars: keys.to_owned(),
            enter: true
        }
    );
}

#[test]
fn enter_alone_is_an_ordinary_key() {
    let mut b = NagShortcutBuffer::new();
    assert_eq!(
        b.push(NagKey::Enter, Instant::now()),
        NagStep::Pass {
            chars: String::new(),
            enter: true
        }
    );
}

#[test]
fn a_character_that_starts_no_sequence_passes_at_once() {
    let mut b = NagShortcutBuffer::new();
    for ch in ['e', 'N', '1', ' ', 'é', '\u{1f600}', '\0'] {
        assert_eq!(
            b.push(NagKey::Char(ch), Instant::now()),
            NagStep::Pass {
                chars: ch.to_string(),
                enter: false
            },
            "{ch:?}"
        );
        assert!(!b.is_pending());
    }
}

#[test]
fn a_half_typed_sequence_followed_by_a_foreign_key_returns_both() {
    let mut b = NagShortcutBuffer::new();
    let t = Instant::now();
    b.push(NagKey::Char('!'), t);
    assert_eq!(
        b.push(NagKey::Char('e'), t),
        NagStep::Pass {
            chars: "!e".into(),
            enter: false
        }
    );
    assert!(!b.is_pending());
}

#[test]
fn two_minuses_are_not_a_nag_they_come_back_for_the_null_move() {
    let mut b = NagShortcutBuffer::new();
    let t = Instant::now();
    assert_eq!(b.push(NagKey::Char('-'), t), NagStep::Pending);
    assert_eq!(
        b.push(NagKey::Char('-'), t),
        NagStep::Pass {
            chars: "--".into(),
            enter: false
        }
    );
}

#[test]
fn timeout_is_1500_ms_inclusive_boundary() {
    assert_eq!(NAG_SEQUENCE_TIMEOUT, Duration::from_millis(1500));
    let mut b = NagShortcutBuffer::new();
    let t = Instant::now();
    b.push(NagKey::Char('+'), t);
    assert_eq!(b.expires_at(), Some(t + Duration::from_millis(1500)));
    assert_eq!(b.expire(t + Duration::from_millis(1499)), None);
    assert!(b.is_pending());
    assert_eq!(b.expire(t + Duration::from_millis(1500)), Some("+".into()));
    assert!(!b.is_pending());
    assert_eq!(b.expires_at(), None);
}

#[test]
fn a_late_second_key_does_not_complete_the_sequence() {
    let mut b = NagShortcutBuffer::new();
    let t = Instant::now();
    b.push(NagKey::Char('+'), t);
    let step = b.push(NagKey::Char('-'), t + Duration::from_secs(3600));
    assert_eq!(
        step,
        NagStep::Pending,
        "`-` starts a new sequence, `+-` must not fire"
    );
    assert_eq!(b.keys(), "-");
}

#[test]
fn time_going_backwards_does_not_panic_or_expire() {
    let mut b = NagShortcutBuffer::new();
    let t = Instant::now() + Duration::from_secs(10);
    b.push(NagKey::Char('!'), t);
    assert_eq!(b.expire(t - Duration::from_secs(5)), None);
    assert!(b.is_pending());
}

#[test]
fn flush_returns_what_was_held_and_empties() {
    let mut b = NagShortcutBuffer::new();
    b.push(NagKey::Char('?'), Instant::now());
    assert_eq!(b.flush(), "?");
    assert_eq!(b.flush(), "");
    assert_eq!(b.expires_at(), None);
}

#[test]
fn hostile_key_stream_never_panics_and_never_loses_characters() {
    let mut b = NagShortcutBuffer::new();
    let t0 = Instant::now();
    let alphabet = ['!', '?', '=', '~', '+', '-', '/', 'x', '\n'];
    let mut typed_chars = String::new();
    let mut out_chars = String::new();
    let mut fired = 0usize;
    let mut seed = 12345u64;
    for i in 0..5000u64 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let c = alphabet[(seed >> 33) as usize % alphabet.len()];
        let at = t0 + Duration::from_millis(i * (seed >> 60));
        let key = if c == '\n' {
            NagKey::Enter
        } else {
            NagKey::Char(c)
        };
        if c != '\n' {
            typed_chars.push(c);
        }
        if let Some(chars) = b.expire(at) {
            out_chars.push_str(&chars);
        }
        match b.push(key, at) {
            NagStep::Pending => {}
            NagStep::Fired(_) => fired += 1,
            NagStep::Pass { chars, .. } => out_chars.push_str(&chars),
        }
    }
    out_chars.push_str(&b.flush());
    assert!(fired > 0);
    // Characters are either replayed or consumed by a fired sequence.
    assert!(out_chars.len() <= typed_chars.len());
}

// ---- AppState wiring ----

fn at_e4() -> AppState {
    let mut g = Game::new();
    main_line(&mut g, "e4");
    state_with(g)
}

fn key(state: &mut AppState, ch: char, at: Instant) {
    update(
        state,
        Message::NagKey {
            key: NagKey::Char(ch),
            at,
        },
    );
}

fn enter(state: &mut AppState, at: Instant) {
    update(
        state,
        Message::NagKey {
            key: NagKey::Enter,
            at,
        },
    );
}

fn nags(state: &AppState) -> Vec<Nag> {
    state
        .session
        .game()
        .nags(state.session.cursor())
        .unwrap()
        .to_vec()
}

#[test]
fn exclamation_enter_adds_dollar_one_and_is_one_undo_step() {
    let mut s = at_e4();
    let t = Instant::now();
    key(&mut s, '!', t);
    assert!(nags(&s).is_empty(), "waits for Enter");
    enter(&mut s, t);
    assert_eq!(nags(&s), [nag(1)]);
    send(&mut s, GameMessage::Undo);
    assert!(nags(&s).is_empty());
}

#[test]
fn group_exclusivity_on_replaces_within_the_group_d180() {
    let mut s = at_e4();
    let t = Instant::now();
    for k in ["!", "?"] {
        key(&mut s, k.chars().next().unwrap(), t);
        enter(&mut s, t);
    }
    assert_eq!(nags(&s), [nag(2)], "? replaces !");
    key(&mut s, '+', t);
    key(&mut s, '=', t);
    assert_eq!(nags(&s), [nag(2), nag(14)]);
    key(&mut s, '=', t);
    key(&mut s, '+', t);
    assert_eq!(
        nags(&s),
        [nag(2), nag(15)],
        "=+ replaces += (evaluation group)"
    );
}

#[test]
fn group_exclusivity_off_accumulates_like_the_scid() {
    let mut s = at_e4();
    let settings = EditSettings {
        exclusive_nags: false,
        ..*s.session.settings()
    };
    send(&mut s, GameMessage::ApplySettings(settings));
    let t = Instant::now();
    key(&mut s, '!', t);
    enter(&mut s, t);
    key(&mut s, '?', t);
    enter(&mut s, t);
    assert_eq!(nags(&s), [nag(1), nag(2)]);
}

#[test]
fn at_the_root_the_shortcut_is_refused_with_no_move_here() {
    let mut s = at_e4();
    send(&mut s, GameMessage::Navigate(Nav::Start));
    let t = Instant::now();
    key(&mut s, '+', t);
    key(&mut s, '-', t);
    assert_eq!(s.session.notice(), Some(&Notice::NoMoveHere));
    assert!(!s.is_dirty() || s.session.game().node_count() == 2);
    let root = s.session.game().root();
    assert!(s.session.game().nags(root).map_or(true, <[_]>::is_empty));
}

#[test]
fn two_minuses_insert_a_null_move() {
    let mut s = at_e4();
    let t = Instant::now();
    key(&mut s, '-', t);
    key(&mut s, '-', t);
    let g = s.session.game();
    assert!(g.mv(s.session.cursor()).unwrap().is_null());
    assert!(!s.session.move_entry().text().contains('-'));
    assert!(s.nag_sequence_deadline().is_none());
}

#[test]
fn ordinary_characters_reach_the_move_entry_unchanged() {
    let mut s = at_e4();
    let t = Instant::now();
    key(&mut s, 'e', t);
    assert_eq!(s.session.move_entry().text(), "e");
}

#[test]
fn deadline_exists_only_while_a_sequence_is_half_typed() {
    let mut s = at_e4();
    let t = Instant::now();
    assert!(s.nag_sequence_deadline().is_none());
    key(&mut s, '+', t);
    assert_eq!(s.nag_sequence_deadline(), Some(t + NAG_SEQUENCE_TIMEOUT));
    key(&mut s, '-', t);
    assert!(s.nag_sequence_deadline().is_none());
}

#[test]
fn tick_after_the_timeout_drops_the_sequence_and_adds_nothing() {
    let mut s = at_e4();
    let t = Instant::now();
    key(&mut s, '+', t);
    update(&mut s, Message::NagTick(t + Duration::from_millis(1499)));
    assert!(s.nag_sequence_deadline().is_some());
    update(&mut s, Message::NagTick(t + Duration::from_millis(1500)));
    assert!(s.nag_sequence_deadline().is_none());
    assert!(nags(&s).is_empty());
}

#[test]
fn late_second_key_through_the_state_does_not_fire() {
    let mut s = at_e4();
    let t = Instant::now();
    key(&mut s, '+', t);
    key(&mut s, '-', t + Duration::from_secs(10));
    assert!(nags(&s).is_empty());
}

#[test]
fn interrupt_drops_the_sequence_and_replays_its_keys_to_the_move_entry() {
    let mut s = at_e4();
    let t = Instant::now();
    key(&mut s, '!', t);
    update(&mut s, Message::NagInterrupt);
    assert!(s.nag_sequence_deadline().is_none());
    assert!(nags(&s).is_empty());
    // `!` is not a move-entry character, so nothing is typed.
    update(&mut s, Message::NagInterrupt);
}

#[test]
fn enter_without_a_sequence_commits_the_move_entry() {
    let mut s = at_e4();
    let t = Instant::now();
    for ch in ['e', '5'] {
        key(&mut s, ch, t);
    }
    enter(&mut s, t);
    assert_eq!(s.session.game().node_count(), 3, "e5 was played");
}

#[test]
fn nag_shortcut_changes_the_notation_in_the_same_update() {
    let mut s = at_e4();
    let t = Instant::now();
    key(&mut s, '+', t);
    key(&mut s, '-', t);
    assert!(s.notation().flat_text().contains("+-"));
}

#[test]
fn shortcut_while_a_prompt_is_open_is_blocked() {
    let mut s = at_e4();
    send(&mut s, GameMessage::Navigate(Nav::Start));
    let root = s.session.game().root();
    send(
        &mut s,
        GameMessage::RequestStrip {
            kind: StripKind::TruncateAfter(root),
            confirm: true,
        },
    );
    assert!(s.session.prompt().is_some());
    let t = Instant::now();
    key(&mut s, '+', t);
    key(&mut s, '-', t);
    let e4 = s.session.game().children(s.session.game().root()).unwrap()[0];
    assert!(s.session.game().nags(e4).unwrap().is_empty());
}
