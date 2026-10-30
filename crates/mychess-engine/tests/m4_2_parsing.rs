//! M4-2 contract tests: thinking lines, principal variations, scores and
//! times as seen from the outside (spec `docs/specs/m4-2-protocolo-xboard.md`,
//! D420 and rules 1, 2, 4, 5, 9). The parsers are crate-private, so every
//! case goes through a session whose engine is played by hand.

#[path = "m4_1_common/helpers.rs"]
mod common;
#[path = "m4_2_common/xb.rs"]
mod xb;

use common::*;
use mychess_core::{Move, Position};
use mychess_engine::{Bound, EngineCommand, EngineEvent, ExitReason, Score, SearchInfo};
use proptest::prelude::*;
use xb::*;

const SENTINEL: &str = "1 0 0 777 zzz";

/// Feeds one line and returns the `Info` it produced, if any.
async fn feed(r: &mut Rig, text: &str) -> Option<SearchInfo> {
    r.peer.say(text).await;
    r.peer.say(SENTINEL).await;
    let mut found = None;
    loop {
        let info = next_info(&mut r.events).await;
        if info.nodes == Some(777) {
            return found;
        }
        assert!(found.is_none(), "one line produced two infos");
        found = Some(info);
    }
}

async fn analyzing(fen: &str) -> Rig {
    let mut r = full().await;
    r.start_fen(fen).await;
    r
}

// ---------------------------------------------------------------------------
// Thinking line
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn well_formed_thinking_lines_become_infos_with_every_field() {
    let mut r = analyzing(START_FEN).await;
    let i = feed(&mut r, "  9  +34  120  345678  Nf3 Nc6 e4")
        .await
        .unwrap();
    assert_eq!(i.multipv, 1);
    assert_eq!(i.depth, Some(9));
    assert_eq!(i.score, Some((Score::Cp(34), Bound::Exact)));
    assert_eq!(i.time_ms, Some(1200));
    assert_eq!(i.nodes, Some(345_678));
    assert_eq!(i.pv.len(), 3);
    assert!(!i.pv_truncated);
    assert_eq!(i.seldepth, None);
}

#[tokio::test(start_paused = true)]
async fn glued_character_after_the_ply_is_accepted() {
    let mut r = analyzing(START_FEN).await;
    for (line, ply) in [
        ("9. 10 5 100 e2e4", 9),
        ("10+ 10 5 100 e2e4", 10),
        ("11- 10 5 100 e2e4", 11),
        ("12? 10 5 100 e2e4", 12),
        ("13 10 5 100 e2e4", 13),
        ("  14.\t10\t5\t100\te2e4", 14),
    ] {
        let i = feed(&mut r, line)
            .await
            .unwrap_or_else(|| panic!("rejected: {line:?}"));
        assert_eq!(i.depth, Some(ply), "{line:?}");
    }
}

#[tokio::test(start_paused = true)]
async fn malformed_thinking_lines_are_ignored() {
    let mut r = analyzing(START_FEN).await;
    for line in [
        "",
        "   ",
        "e4 e5",
        "0 10 5 100 e2e4",
        "256 10 5 100 e2e4",
        "9 34 120",
        "9 34 120 345",
        "9 34 120 345   ",
        "9 x 120 345 e2e4",
        "9 - 120 345 e2e4",
        "9 1.5 120 345 e2e4",
        "9 34 -1 345 e2e4",
        "9 34 120 -5 e2e4",
        "9.. 34 1 2 e2e4",
        "9 99999999999 1 1 e2e4",
        "9 -99999999999 1 1 e2e4",
        "9 1 99999999999999999999999 1 e2e4",
        "9 1 1 99999999999999999999999 e2e4",
        "99999999999999999999 1 1 1 e2e4",
        "-3 1 1 1 e2e4",
        "ply 3 score 1 time 2 nodes 3 e2e4",
        "info depth 3 score cp 4 pv e2e4",
        "bestmove e2e4",
    ] {
        assert!(
            feed(&mut r, line).await.is_none(),
            "must be ignored: {line:?}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn boundary_numbers_are_accepted_without_overflow() {
    let mut r = analyzing(START_FEN).await;
    let i = feed(&mut r, "255 -0 0 0 e2e4").await.unwrap();
    assert_eq!(i.depth, Some(255));
    assert_eq!(i.score, Some((Score::Cp(0), Bound::Exact)));
    assert_eq!((i.time_ms, i.nodes), (Some(0), Some(0)));
    let i = feed(&mut r, "5 1 1 18446744073709551615 e2e4")
        .await
        .unwrap();
    assert_eq!(i.nodes, Some(u64::MAX));
    let i = feed(&mut r, "5 1 18446744073709551615 1 e2e4")
        .await
        .unwrap();
    assert!(
        i.time_ms.is_some(),
        "a huge time saturates instead of wrapping"
    );
    assert!(i.time_ms.unwrap() >= u64::MAX / 10);
    let i = feed(&mut r, "5 2147483647 1 1 e2e4").await.unwrap();
    assert!(matches!(i.score, Some((Score::Mate(n), _)) if n > 0));
    let i = feed(&mut r, "5 -2147483648 1 1 e2e4").await.unwrap();
    assert!(matches!(i.score, Some((Score::Mate(n), _)) if n < 0));
}

#[tokio::test(start_paused = true)]
async fn an_explicit_plus_sign_on_the_score_is_accepted() {
    // Deviation 9 of the implementation: the fake engine prints `%+5d`.
    let mut r = analyzing(START_FEN).await;
    assert_eq!(
        feed(&mut r, "5 +7 1 1 e2e4").await.unwrap().score,
        Some((Score::Cp(7), Bound::Exact))
    );
}

// ---------------------------------------------------------------------------
// Principal variations
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn pv_table_start_position() {
    // (pv text, number of moves kept, truncated)
    let cases: &[(&str, usize, bool)] = &[
        ("e2e4 e7e5 g1f3", 3, false),
        ("e4 e5 Nf3", 3, false),
        ("1. e4 e5 2. Nf3 Nc6 3. Bb5+", 5, false),
        ("1.e4 e5 2.Nf3 Nc6", 4, false),
        ("1. e4 1... e5", 2, false),
        ("1. e4 ... e5", 2, false),
        ("1. e4 1...e5 2. Nf3", 3, false),
        ("e4 {a comment} e5 (a variation) Nf3", 3, false),
        ("e4! e5? Nf3+ Nc6 Bb5#", 5, false),
        ("Nf3+", 1, false),
        ("e4 e5 Nf3 Nc6 Bb5 a6 Ba4 Nf6 O-O Be7", 10, false),
        ("e4 e5 Qxh8 Nc6", 2, true),
        ("e2e4 e7e5 e2e4", 2, true),
        ("Zz9 e4", 0, true),
        ("e2e5", 0, true),
        ("O-O", 0, true),
        ("0000 e4", 0, true),
        ("e4 e5 Nf3 xyzzy e4", 3, true),
        ("e4 e5 Nf3 e7e5 Nc6", 3, true),
    ];
    let mut r = analyzing(START_FEN).await;
    for (pv, len, truncated) in cases {
        let i = feed(&mut r, &format!("5 +1 1 1 {pv}"))
            .await
            .unwrap_or_else(|| panic!("no info for {pv}"));
        assert_eq!((i.pv.len(), i.pv_truncated), (*len, *truncated), "{pv}");
    }
}

#[tokio::test(start_paused = true)]
async fn pv_moves_are_the_legal_moves_in_order() {
    let mut r = analyzing(START_FEN).await;
    let i = feed(&mut r, "5 +1 1 1 1. e4 e5 2. Nf3").await.unwrap();
    let mut p = Position::new();
    let mut want: Vec<Move> = Vec::new();
    for t in ["e2e4", "e7e5", "g1f3"] {
        let m = mv(&p, t);
        p.play(m).unwrap();
        want.push(m);
    }
    assert_eq!(i.pv, want);
}

#[tokio::test(start_paused = true)]
async fn castling_in_san_and_in_coordinates() {
    let fen = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
    let mut r = analyzing(fen).await;
    for pv in ["O-O O-O-O", "e1g1 e8c8", "O-O-O O-O", "e1c1 e8g8"] {
        let i = feed(&mut r, &format!("5 +1 1 1 {pv}")).await.unwrap();
        assert_eq!((i.pv.len(), i.pv_truncated), (2, false), "{pv}");
    }
    let i = feed(&mut r, "5 +1 1 1 O-O O-O-O O-O").await.unwrap();
    assert_eq!(
        (i.pv.len(), i.pv_truncated),
        (2, true),
        "White has no castling right left"
    );
}

#[tokio::test(start_paused = true)]
async fn promotions_in_both_notations() {
    let mut r = analyzing("8/P6k/8/8/8/8/8/K7 w - - 0 1").await;
    for pv in ["a7a8q", "a8=Q", "a8=Q+", "a7a8n"] {
        let i = feed(&mut r, &format!("5 +1 1 1 {pv}")).await.unwrap();
        assert_eq!((i.pv.len(), i.pv_truncated), (1, false), "{pv}");
    }
}

#[tokio::test(start_paused = true)]
async fn unterminated_comment_and_odd_punctuation_never_panic() {
    let mut r = analyzing(START_FEN).await;
    for pv in [
        "e4 {oops e5",
        "e4 ((((( e5",
        "e4 ))) e5",
        "(((",
        "...",
        "1.",
        "e4 :",
        "+ + +",
        "e4;e5",
        "é\u{301}\u{0}",
    ] {
        let i = feed(&mut r, &format!("5 +1 1 1 {pv}")).await;
        if let Some(i) = i {
            assert!(i.pv.len() <= 2, "{pv}: {:?}", i.pv);
        }
    }
}

#[tokio::test(start_paused = true)]
async fn a_very_long_pv_is_bounded_and_does_not_slow_the_session() {
    let mut r = analyzing(START_FEN).await;
    let cycle = "g1f3 g8f6 f3g1 f6g8 ";
    let pv = cycle.repeat(5000); // 100 000 plies of knight shuffling
    let i = feed(&mut r, &format!("5 +1 1 1 {pv}")).await.unwrap();
    assert!(i.pv.len() <= 256 && i.pv.len() >= 100, "{}", i.pv.len());
}

#[tokio::test(start_paused = true)]
async fn a_one_mebibyte_line_is_survived() {
    let mut r = analyzing(START_FEN).await;
    let mut text = String::from("5 +1 1 1 e2e4 ");
    text.push_str(&"e7e5 ".repeat(300_000));
    r.peer.say(&text).await;
    drop(next_info(&mut r.events).await);
    let mut huge = vec![b'q'; 2 * 1024 * 1024];
    huge.push(b'\n');
    r.peer.say_raw(&huge).await;
    assert!(feed(&mut r, "6 +1 1 1 e2e4").await.is_some());
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "exit");
    next_best(&mut r.events).await;
}

#[tokio::test(start_paused = true)]
async fn invalid_utf8_inside_a_thinking_line_does_not_break_the_session() {
    let mut r = analyzing(START_FEN).await;
    r.peer.say_raw(b"7 +1 1 1 e2e4 \xff\xfe\n").await;
    let i = next_info(&mut r.events).await; // lossy decoding or drop: both fine
    assert!(i.pv.len() <= 1);
    assert!(feed(&mut r, "8 +1 1 1 e2e4").await.is_some());
}

// ---------------------------------------------------------------------------
// Scores
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn white_to_move_scores_are_kept_and_black_to_move_scores_are_negated() {
    let mut w = analyzing(START_FEN).await;
    let i = feed(&mut w, "5 +120 1 1 e2e4").await.unwrap();
    assert_eq!(i.score, Some((Score::Cp(120), Bound::Exact)));
    let i = feed(&mut w, "5 -80 1 1 e2e4").await.unwrap();
    assert_eq!(i.score, Some((Score::Cp(-80), Bound::Exact)));
    let mut b = analyzing(BLACK_WINS_QUEEN).await;
    let i = feed(&mut b, "6 +300 10 100 a3a1").await.unwrap();
    assert_eq!(
        i.score,
        Some((Score::Cp(-300), Bound::Exact)),
        "black is better: negative for White"
    );
    let i = feed(&mut b, "6 -50 10 100 a3a1").await.unwrap();
    assert_eq!(i.score, Some((Score::Cp(50), Bound::Exact)));
}

#[tokio::test(start_paused = true)]
async fn mate_scores_follow_the_100000_convention() {
    let mut w = analyzing(START_FEN).await;
    for (raw, want) in [
        (100_001_i32, Score::Mate(1)),
        (100_003, Score::Mate(3)),
        (-100_002, Score::Mate(-2)),
        (100_000, Score::Mate(1)),
        (-100_000, Score::Mate(-1)),
        (99_999, Score::Cp(99_999)),
        (-99_999, Score::Cp(-99_999)),
    ] {
        let i = feed(&mut w, &format!("5 {raw} 1 1 e2e4")).await.unwrap();
        assert_eq!(i.score, Some((want, Bound::Exact)), "{raw}");
    }
    let mut b = analyzing(BLACK_WINS_QUEEN).await;
    let i = feed(&mut b, "5 100002 1 1 a3a1").await.unwrap();
    assert_eq!(
        i.score,
        Some((Score::Mate(-2), Bound::Exact)),
        "Black mates: negative for White"
    );
}

#[tokio::test(start_paused = true)]
async fn crafty_18_and_later_report_scores_from_whites_point_of_view() {
    // (banner, engine score is already from White's view?)
    let cases = [
        ("Crafty v17.9 JA", false),
        ("Crafty v9.9", false),
        ("Crafty v18.0", true),
        ("Crafty v18.1 (1 cpus)", true),
        ("Crafty v23.4 JA (1 cpus)", true),
        ("Crafty v100.0", true),
        ("Crafty", false),
        ("Crafty vX.Y", false),
    ];
    for (banner, white_pov) in cases {
        let mut r = rig_with(xconfig(), &[banner], &[FULL]).await;
        r.start_fen(BLACK_WINS_QUEEN).await;
        let i = feed(&mut r, "6 +300 10 100 a3a1").await.unwrap();
        let want = if white_pov { 300 } else { -300 };
        assert_eq!(i.score, Some((Score::Cp(want), Bound::Exact)), "{banner}");
    }
}

#[tokio::test(start_paused = true)]
async fn crafty_version_from_myname_counts_too() {
    let mut r = rig(&[r#"feature myname="Crafty-23.4 JA" name=1 ping=1 done=1"#]).await;
    r.start_fen(BLACK_WINS_QUEEN).await;
    let i = feed(&mut r, "6 +300 10 100 a3a1").await.unwrap();
    assert_eq!(i.score, Some((Score::Cp(300), Bound::Exact)));
}

#[tokio::test(start_paused = true)]
async fn only_crafty_flips_the_score_convention() {
    for banner in [
        "Phalanx XXIII",
        "Stockfish 17",
        "Scorpio 2.7",
        "Baron 0.28a",
    ] {
        let mut r = rig_with(xconfig(), &[banner], &[FULL]).await;
        r.start_fen(BLACK_WINS_QUEEN).await;
        let i = feed(&mut r, "6 +300 10 100 a3a1").await.unwrap();
        assert_eq!(i.score, Some((Score::Cp(-300), Bound::Exact)), "{banner}");
    }
}

// ---------------------------------------------------------------------------
// Time
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn time_is_in_centiseconds_by_default() {
    let mut r = analyzing(START_FEN).await;
    for (cs, ms) in [
        (0_u64, 0_u64),
        (1, 10),
        (7, 70),
        (100, 1000),
        (123_456, 1_234_560),
    ] {
        let i = feed(&mut r, &format!("5 +1 {cs} 1 e2e4")).await.unwrap();
        assert_eq!(i.time_ms, Some(ms), "{cs} cs");
    }
}

#[tokio::test(start_paused = true)]
async fn engines_that_report_whole_seconds_are_scaled_by_1000() {
    for banner in ["Baron 0.28a", "Pharaon 2.61", "ZChess 2.22"] {
        let mut r = rig_with(xconfig(), &[banner], &[FULL]).await;
        r.start_fen(START_FEN).await;
        let i = feed(&mut r, "5 +1 3 1 e2e4").await.unwrap();
        assert_eq!(i.time_ms, Some(3000), "{banner}");
        let i = feed(&mut r, "5 +1 0 1 e2e4").await.unwrap();
        assert_eq!(i.time_ms, Some(0), "time 0 is accepted");
    }
}

// ---------------------------------------------------------------------------
// Proptests
// ---------------------------------------------------------------------------

fn run<F: Future<Output = ()>>(f: F) {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .expect("runtime")
        .block_on(f);
}

fn thinking_like() -> impl Strategy<Value = String> {
    prop_oneof![
        "[0-9]{1,4}[.+?!a-z-]? ?-?[0-9]{0,12} [0-9]{0,22} [0-9]{0,22} [ -~]{0,50}",
        "[ -~]{0,80}",
        "\\PC{0,60}",
        "[0-9]{1,3}[.+-?]? -?[0-9]{1,6} [0-9]{1,6} [0-9]{1,6} (([a-h][1-8]){2}[qrbn]? ?){0,10}",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]

    /// No engine line, however hostile, kills the session: afterwards a Stop
    /// still ends the search with a single BestMove.
    #[test]
    fn arbitrary_engine_lines_never_break_the_session(lines in proptest::collection::vec(thinking_like(), 0..12)) {
        run(async move {
            let mut r = full().await;
            r.start_fen(START_FEN).await;
            for l in &lines {
                // A line that happens to be `pong`/`move`/`feature` is legal protocol.
                if l.trim_start().starts_with("move") || l.trim_start().starts_with("My move")
                    || l.trim_start().starts_with("feature")
                { continue; }
                r.peer.say(l).await;
            }
            settle().await;
            let seen = drain(&mut r.events);
            assert!(seen.iter().all(|e| matches!(e, EngineEvent::Info(_) | EngineEvent::Warning(_))), "{seen:?}");
            r.engine.send(EngineCommand::Stop).unwrap();
            // Read until `exit` (the engine may have been sent other lines).
            let mut got = false;
            for _ in 0..8 { if r.next_line().await == "exit" { got = true; break; } }
            assert!(got);
            let b = next_best(&mut r.events).await;
            assert!(b.mv.is_none() || Position::new().is_legal(b.mv.unwrap()));
        });
    }

    /// A random legal variation, written in a random mix of notations with
    /// optional move numbers and an optional bad token, round-trips: all the
    /// moves before the bad token are kept, and only then `pv_truncated`.
    #[test]
    fn pv_round_trips_through_any_notation_and_truncates_at_the_bad_token(
        picks in proptest::collection::vec(any::<u8>(), 1..14),
        styles in proptest::collection::vec(0_u8..4, 14),
        bad_at in proptest::option::of(0_usize..14),
    ) {
        run(async move {
            let mut p = Position::new();
            let mut moves: Vec<Move> = Vec::new();
            let mut tokens: Vec<(usize, String)> = Vec::new();
            for (i, pick) in picks.iter().enumerate() {
                let legal = p.legal_moves();
                if legal.is_empty() { break; }
                let m = legal[usize::from(*pick) % legal.len()];
                let number = p.fullmove_number();
                let white = p.turn() == mychess_core::Color::White;
                let style = styles[i];
                let text = if style % 2 == 0 { p.uci(m) } else { p.san(m).unwrap() };
                let mut piece = String::new();
                if style >= 2 {
                    if white { piece = format!("{number}. "); }
                    else if i == 0 { piece = format!("{number}... "); }
                }
                piece.push_str(&text);
                tokens.push((i, piece));
                p.play(m).unwrap();
                moves.push(m);
            }
            let cut = bad_at.map(|k| k.min(moves.len()));
            let mut parts: Vec<String> = Vec::new();
            for (i, t) in &tokens {
                if Some(*i) == cut { parts.push("Zz9".to_owned()); }
                parts.push(t.clone());
            }
            if cut == Some(moves.len()) { parts.push("Zz9".to_owned()); }
            let mut r = full().await;
            r.start_fen(START_FEN).await;
            let info = r.think(&format!("5 +1 1 1 {}", parts.join(" "))).await;
            let want = cut.unwrap_or(moves.len());
            assert_eq!(info.pv, moves[..want].to_vec(), "{parts:?}");
            assert_eq!(info.pv_truncated, cut.is_some(), "{parts:?}");
        });
    }
}

#[tokio::test(start_paused = true)]
async fn session_survives_a_burst_of_junk_and_exits_cleanly_on_quit() {
    let mut r = analyzing(START_FEN).await;
    for i in 0..2000 {
        r.peer
            .say(&format!("junk {i} \u{1b}[0m ??? {}", "x".repeat(i % 97)))
            .await;
    }
    r.engine.send(EngineCommand::Quit).unwrap();
    let exit = next_exit(&mut r.events).await;
    assert_eq!(exit.reason, ExitReason::Quit);
}
