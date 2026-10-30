//! Contract tests of the models, the time-control rules and the backoff
//! (spec m8-3 sections 1.4, 3 D1304, D1314, D1333, D1348; acceptance criteria
//! "Modelos" and "`time_control`"). The JSON samples follow the format of the
//! Lichess API documentation.

#![allow(clippy::assert_is_empty, clippy::type_complexity)]

use std::time::Duration;

use mychess_online::{
    Error,
    backoff::Backoff,
    models::{
        Account, Challenge, ChallengeParams, ChallengeUpdate, Clock, Color, ColorChoice, Direction,
        Event, GameEvent, GameFull, GameState, SeekParams, Speed, Status,
    },
    time_control::{TimeControlError, classify, validate_challenge, validate_seek},
};
use proptest::prelude::*;

fn full(json: &str) -> GameFull {
    match GameEvent::parse_line(json) {
        GameEvent::Full(f) => *f,
        other => panic!("expected gameFull, got {other:?}"),
    }
}

fn state(json: &str) -> GameState {
    match GameEvent::parse_line(json) {
        GameEvent::State(s) => s,
        other => panic!("expected gameState, got {other:?}"),
    }
}

// ----------------------------------------------------------------- gameFull

#[test]
fn game_full_with_all_documented_fields() {
    let f = full(
        r#"{"type":"gameFull","id":"q7ZvsdUF","rated":true,"variant":{"key":"standard","name":"Standard","short":"Std"},
        "clock":{"initial":300000,"increment":3000},"speed":"blitz","perf":{"name":"Blitz"},"createdAt":1523825103841,
        "white":{"id":"georges","name":"georges","title":"GM","rating":2800,"provisional":false},
        "black":{"id":"bobby","name":"Bobby","rating":1700,"provisional":true},
        "initialFen":"startpos","tournamentId":"abcd",
        "state":{"type":"gameState","moves":"e2e4 c7c5","wtime":300000,"btime":299000,"winc":3000,"binc":3000,"status":"started",
                 "wdraw":true,"btakeback":true,"expiration":{"idleMillis":1000,"millisToMove":20000}},
        "someFutureField":{"nested":[1,2,3]}}"#,
    );
    assert_eq!(f.id, "q7ZvsdUF");
    assert!(f.rated);
    assert_eq!(f.speed, Some(Speed::Blitz));
    assert_eq!(f.perf.as_ref().map(|p| p.name.as_str()), Some("Blitz"));
    assert_eq!(
        f.clock,
        Some(Clock {
            initial: 300_000,
            increment: 3000
        })
    );
    assert_eq!(f.created_at, Some(1_523_825_103_841));
    assert_eq!(f.white.title.as_deref(), Some("GM"));
    assert_eq!(f.white.rating, Some(2800));
    assert!(f.black.provisional);
    assert_eq!(f.tournament_id.as_deref(), Some("abcd"));
    assert_eq!(f.initial_fen, "startpos");
    assert!(f.state.wdraw && !f.state.bdraw && f.state.btakeback && !f.state.wtakeback);
    let e = f.state.expiration.expect("expiration");
    assert_eq!((e.idle_millis, e.millis_to_move), (1000, 20_000));
    assert_eq!(f.state.move_list(), ["e2e4", "c7c5"]);
}

#[test]
fn game_full_defaults_for_missing_optional_fields() {
    let f = full(r#"{"type":"gameFull","id":"abcd1234","state":{"type":"gameState"}}"#);
    assert!(f.variant.is_standard(), "missing variant means standard");
    assert_eq!(f.initial_fen, "startpos");
    assert_eq!(f.clock, None, "correspondence has no clock");
    assert!(!f.rated);
    assert_eq!(f.white.rating, None);
    assert!(f.state.moves.is_empty() && f.state.move_list().is_empty());
    assert!(!f.state.is_final());
}

#[test]
fn game_full_correspondence_and_ai_opponent() {
    let f = full(
        r#"{"type":"gameFull","id":"abcd1234","speed":"correspondence","daysPerTurn":3,
        "white":{"id":"bob","name":"Bob","rating":1500},"black":{"aiLevel":4},"state":{"type":"gameState","moves":""}}"#,
    );
    assert_eq!(f.speed, Some(Speed::Correspondence));
    assert_eq!(f.days_per_turn, Some(3));
    assert_eq!(f.black.ai_level, Some(4));
    assert_eq!(f.black.name, None);
}

#[test]
fn game_full_player_username_alias_and_custom_start() {
    let f = full(
        r#"{"type":"gameFull","id":"abcd1234","variant":{"key":"chess960","name":"Chess960"},
        "initialFen":"bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w HFhf - 0 1",
        "white":{"username":"alice"},"state":{"type":"gameState","moves":"g1h1"}}"#,
    );
    assert_eq!(f.variant.key, "chess960");
    assert!(!f.variant.is_standard());
    assert!(f.initial_fen.starts_with("bbqnnrkr"));
    assert_eq!(f.white.name.as_deref(), Some("alice"));
}

#[test]
fn game_full_with_a_final_state_is_final() {
    let f = full(
        r#"{"type":"gameFull","id":"abcd1234","state":{"type":"gameState","moves":"f2f3 e7e5 g2g4 d8h4","status":"mate","winner":"black"}}"#,
    );
    assert!(f.state.is_final());
    assert_eq!(f.state.winner, Some(Color::Black));
}

// ---------------------------------------------------------------- gameState

#[test]
fn every_documented_status_decodes_and_only_created_and_started_are_not_final() {
    let cases: Vec<(&str, bool)> = vec![
        ("created", false),
        ("started", false),
        ("aborted", true),
        ("mate", true),
        ("resign", true),
        ("stalemate", true),
        ("timeout", true),
        ("draw", true),
        ("outoftime", true),
        ("cheat", true),
        ("noStart", true),
        ("unknownFinish", true),
        ("insufficientMaterialClaim", true),
        ("variantEnd", true),
    ];
    for (name, final_) in cases {
        let s = state(&format!(
            r#"{{"type":"gameState","moves":"e2e4","status":"{name}"}}"#
        ));
        let status = s.status.expect("status decoded");
        assert_ne!(status, Status::Unknown, "{name} is documented");
        assert_eq!(status.is_final(), final_, "{name}");
        assert_eq!(s.is_final(), final_, "{name}");
    }
}

#[test]
fn an_unknown_status_string_is_final_and_does_not_break_the_message() {
    let s = state(
        r#"{"type":"gameState","moves":"e2e4 e7e5","wtime":1000,"btime":2000,"status":"someFutureEnding"}"#,
    );
    assert_eq!(s.status, Some(Status::Unknown));
    assert!(s.is_final(), "D1304: an unknown status counts as final");
    assert_eq!(s.move_list(), ["e2e4", "e7e5"]);
    assert_eq!((s.wtime, s.btime), (1000, 2000));
}

#[test]
fn a_status_of_the_wrong_type_keeps_the_rest_of_the_state() {
    // gameFinish carries {"id":31,"name":"resign"}; a gameState with such an
    // object must still give the moves and clocks.
    let s = state(
        r#"{"type":"gameState","moves":"e2e4","wtime":5,"btime":6,"status":{"id":31,"name":"resign"}}"#,
    );
    assert_eq!(s.move_list(), ["e2e4"]);
    assert_eq!((s.wtime, s.btime), (5, 6));
    let s = state(r#"{"type":"gameState","moves":"e2e4","status":42}"#);
    assert_eq!(s.move_list(), ["e2e4"]);
}

#[test]
fn winner_is_absent_for_draws_and_tolerated_when_bogus() {
    let s = state(r#"{"type":"gameState","moves":"","status":"draw"}"#);
    assert_eq!(s.winner, None);
    let s = state(r#"{"type":"gameState","moves":"","status":"mate","winner":"purple"}"#);
    assert_eq!(s.winner, None);
    assert!(s.is_final());
}

#[test]
fn offers_and_takeback_flags() {
    let s = state(r#"{"type":"gameState","moves":"","bdraw":true,"wtakeback":true}"#);
    assert!(s.bdraw && !s.wdraw && s.wtakeback && !s.btakeback);
}

#[test]
fn move_list_splits_on_any_whitespace_and_ignores_empties() {
    let s = state(r#"{"type":"gameState","moves":"  e2e4   e7e5\t g1f3 "}"#);
    assert_eq!(s.move_list(), ["e2e4", "e7e5", "g1f3"]);
}

#[test]
fn castling_in_both_notations_is_passed_through_untouched() {
    // D1318: the decoder of the app handles both; the model must not rewrite.
    let s = state(r#"{"type":"gameState","moves":"e2e4 e7e5 g1f3 b8c6 f1c4 g8f6 e1h1 f6e4 e1g1"}"#);
    let list = s.move_list();
    assert!(list.contains(&"e1h1") && list.contains(&"e1g1"));
}

#[test]
fn malformed_move_text_is_data_not_a_decode_error() {
    // Validity of each move is the app's job; the model keeps the text.
    let s = state(r#"{"type":"gameState","moves":"zz99 e2e4 !!!"}"#);
    assert_eq!(s.move_list(), ["zz99", "e2e4", "!!!"]);
}

#[test]
fn moves_that_are_not_text_become_unparsed() {
    let cases: Vec<&str> = vec![
        (r#"{"type":"gameState","moves":123}"#),
        (r#"{"type":"gameState","moves":["e2e4"]}"#),
        (r#"{"type":"gameState","moves":{"a":1}}"#),
        (r#"{"type":"gameState","moves":true}"#),
    ];
    for line in cases {
        assert!(
            matches!(GameEvent::parse_line(line), GameEvent::Unparsed { .. }),
            "{line}"
        );
    }
}

#[test]
fn out_of_range_numbers_are_tolerated_and_do_not_cost_the_moves() {
    // D1348: negative, fractional, huge or wrongly typed numbers become 0.
    let bad = [
        "-5",
        "1.5",
        "1e30",
        "18446744073709551616",
        "\"900\"",
        "null",
        "true",
        "[1]",
        "{\"a\":1}",
    ];
    for v in bad {
        let line = format!(
            r#"{{"type":"gameState","moves":"e2e4 e7e5","wtime":{v},"btime":{v},"winc":{v},"binc":{v},"status":"started"}}"#
        );
        let s = state(&line);
        assert_eq!(
            (s.wtime, s.btime, s.winc, s.binc),
            (0, 0, 0, 0),
            "value {v}"
        );
        assert_eq!(s.move_list(), ["e2e4", "e7e5"], "value {v}");
        assert_eq!(s.status, Some(Status::Started), "value {v}");
    }
    // The largest u64 is in range and kept.
    let s = state(r#"{"type":"gameState","moves":"","wtime":18446744073709551615}"#);
    assert_eq!(s.wtime, u64::MAX);
}

#[test]
fn out_of_range_numbers_in_game_full_do_not_drop_the_game() {
    let f = full(
        r#"{"type":"gameFull","id":"abcd1234","createdAt":-1,"daysPerTurn":99999999999,
        "clock":{"initial":-1,"increment":"x"},
        "white":{"id":"a","rating":-100,"aiLevel":1.5},"black":{"id":"b","rating":4294967296},
        "state":{"type":"gameState","moves":"e2e4","wtime":-1}}"#,
    );
    assert_eq!(f.created_at, None);
    assert_eq!(f.days_per_turn, None);
    assert_eq!(
        f.clock,
        Some(Clock {
            initial: 0,
            increment: 0
        })
    );
    assert_eq!(f.white.rating, None);
    assert_eq!(f.white.ai_level, None);
    assert_eq!(f.black.rating, None);
    assert_eq!(f.state.move_list(), ["e2e4"]);
}

#[test]
fn opponent_gone_variants() {
    assert_eq!(
        GameEvent::parse_line(r#"{"type":"opponentGone","gone":true,"claimWinInSeconds":15}"#),
        GameEvent::OpponentGone {
            gone: true,
            claim_win_in_seconds: Some(15)
        }
    );
    assert_eq!(
        GameEvent::parse_line(r#"{"type":"opponentGone","gone":true,"claimWinInSeconds":-3}"#),
        GameEvent::OpponentGone {
            gone: true,
            claim_win_in_seconds: None
        }
    );
    assert_eq!(
        GameEvent::parse_line(r#"{"type":"opponentGone","gone":false}"#),
        GameEvent::OpponentGone {
            gone: false,
            claim_win_in_seconds: None
        }
    );
}

#[test]
fn chat_lines_and_unknown_game_events_do_not_carry_text() {
    let chat = GameEvent::parse_line(
        r#"{"type":"chatLine","username":"alice","text":"play Qh5!","room":"player"}"#,
    );
    assert_eq!(chat, GameEvent::ChatLine);
    assert!(
        !format!("{chat:?}").contains("Qh5"),
        "D1326: chat text is dropped"
    );
    assert_eq!(
        GameEvent::parse_line(r#"{"type":"brandNewThing","x":1}"#),
        GameEvent::Unknown
    );
}

#[test]
fn garbage_lines_become_unparsed_without_panicking() {
    let cases: Vec<&str> = vec![
        (""),
        ("   "),
        ("not json"),
        ("[]"),
        ("null"),
        ("42"),
        ("\"gameFull\""),
        ("{"),
        ("{\"type\":"),
        ("{\"moves\":\"e2e4\"}"),
        ("{\"type\":42}"),
    ];
    for line in cases {
        assert!(matches!(
            GameEvent::parse_line(line),
            GameEvent::Unparsed { .. } | GameEvent::Unknown
        ));
        assert!(matches!(
            Event::parse_line(line),
            Event::Unparsed { .. } | Event::Unknown
        ));
        assert!(matches!(
            ChallengeUpdate::parse_line(line),
            ChallengeUpdate::Unparsed { .. }
        ));
    }
}

#[test]
fn unparsed_lines_are_clipped_so_logs_stay_small() {
    let long = format!(
        "{{\"type\":\"gameState\",\"moves\":123,\"pad\":\"{}\"}}",
        "x".repeat(5000)
    );
    let GameEvent::Unparsed { line } = GameEvent::parse_line(&long) else {
        panic!("expected Unparsed");
    };
    assert!(line.chars().count() <= 200, "{}", line.len());
}

proptest! {
    #[test]
    fn decoders_never_panic_on_arbitrary_text(s in ".{0,300}") {
        drop(GameEvent::parse_line(&s));
        drop(Event::parse_line(&s));
        drop(ChallengeUpdate::parse_line(&s));
    }

    #[test]
    fn game_state_survives_any_json_number_in_clock_fields(n in any::<i64>(), f in any::<f64>()) {
        let line = format!(
            r#"{{"type":"gameState","moves":"e2e4","wtime":{n},"btime":{},"status":"started"}}"#,
            serde_json::Number::from_f64(f).map_or("0".to_owned(), |x| x.to_string())
        );
        let GameEvent::State(s) = GameEvent::parse_line(&line) else {
            return Err(TestCaseError::fail("must stay a gameState"));
        };
        prop_assert_eq!(s.move_list(), vec!["e2e4"]);
        if n >= 0 {
            prop_assert_eq!(i128::from(s.wtime), i128::from(n));
        } else {
            prop_assert_eq!(s.wtime, 0);
        }
    }
}

// ------------------------------------------------------------------- events

#[test]
fn game_start_and_finish_decode_with_extra_fields() {
    let start = Event::parse_line(
        r#"{"type":"gameStart","game":{"fullId":"abcd1234wxyz","gameId":"abcd1234","fen":"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "color":"black","lastMove":"","source":"friend","status":{"id":20,"name":"started"},
        "variant":{"key":"standard","name":"Standard"},"speed":"rapid","perf":"rapid","rated":true,"hasMoved":false,
        "opponent":{"id":"alice","username":"alice","rating":1800},"isMyTurn":false,"secondsLeft":600,"compat":{"bot":false}}}"#,
    );
    let Event::GameStart(g) = start else {
        panic!("expected gameStart");
    };
    assert_eq!(g.id, "abcd1234");
    assert_eq!(g.full_id.as_deref(), Some("abcd1234wxyz"));
    assert_eq!(g.color, Some(Color::Black));
    assert!(g.rated && !g.is_my_turn);
    assert_eq!(g.speed, Some(Speed::Rapid));
    assert_eq!(
        g.opponent.as_ref().and_then(|o| o.name.as_deref()),
        Some("alice")
    );

    let finish = Event::parse_line(
        r#"{"type":"gameFinish","game":{"gameId":"abcd1234","color":"white","winner":"white","status":{"id":31,"name":"resign"},"rated":false}}"#,
    );
    let Event::GameFinish(g) = finish else {
        panic!("expected gameFinish");
    };
    assert_eq!(g.id, "abcd1234");
    assert_eq!(g.winner, Some(Color::White));
}

#[test]
fn game_start_with_the_plain_id_field_also_decodes() {
    // Older/other payloads use "id" instead of "gameId".
    let Event::GameStart(g) = Event::parse_line(r#"{"type":"gameStart","game":{"id":"zzzz9999"}}"#)
    else {
        panic!("expected gameStart");
    };
    assert_eq!(g.id, "zzzz9999");
}

fn challenge(json: &str) -> Challenge {
    match Event::parse_line(json) {
        Event::Challenge(c) | Event::ChallengeCanceled(c) | Event::ChallengeDeclined(c) => *c,
        other => panic!("expected a challenge event, got {other:?}"),
    }
}

const CHALLENGE_IN: &str = r#"{"type":"challenge","challenge":{"id":"chal0001","url":"https://lichess.org/chal0001","status":"created",
"challenger":{"id":"alice","name":"alice","rating":1800,"title":null,"online":true,"lag":3},
"destUser":{"id":"bob","name":"Bob","rating":1500},"variant":{"key":"standard","name":"Standard","short":"Std"},
"rated":false,"speed":"rapid","timeControl":{"type":"clock","limit":600,"increment":5,"show":"10+5"},
"color":"random","finalColor":"white","perf":{"icon":"x","name":"Rapid"},"direction":"in"}}"#;

#[test]
fn incoming_challenge_decodes_and_is_playable() {
    let c = challenge(CHALLENGE_IN);
    assert_eq!(c.id, "chal0001");
    assert_eq!(c.status, "created");
    assert_eq!(c.direction, Some(Direction::In));
    assert_eq!(c.final_color, Some(Color::White));
    assert_eq!(c.challenger.as_ref().and_then(|p| p.rating), Some(1800));
    let tc = c.time_control.as_ref().expect("time control");
    assert_eq!(
        (tc.kind.as_str(), tc.limit, tc.increment),
        ("clock", Some(600), Some(5))
    );
    assert_eq!(tc.show.as_deref(), Some("10+5"));
    assert!(c.is_playable());
}

#[test]
fn challenge_canceled_and_declined_keep_the_reason() {
    let line = CHALLENGE_IN
        .replace("challenge\",\"challenge", "challengeDeclined\",\"challenge")
        .replace(
            "\"direction\":\"in\"",
            "\"direction\":\"out\",\"declineReason\":\"I'm not accepting challenges at the moment.\",\"declineReasonKey\":\"later\"",
        );
    let Event::ChallengeDeclined(c) = Event::parse_line(&line) else {
        panic!("expected challengeDeclined");
    };
    assert_eq!(c.decline_reason_key.as_deref(), Some("later"));
    assert_eq!(c.direction, Some(Direction::Out));

    let line = CHALLENGE_IN.replace("challenge\",\"challenge", "challengeCanceled\",\"challenge");
    assert!(matches!(
        Event::parse_line(&line),
        Event::ChallengeCanceled(_)
    ));
}

#[test]
fn only_standard_blitz_or_slower_clock_challenges_are_playable() {
    let cases: Vec<(&str, &str, bool)> = vec![
        (
            r#"{"type":"clock","limit":600,"increment":5}"#,
            "standard",
            true,
        ),
        (
            r#"{"type":"clock","limit":180,"increment":0}"#,
            "standard",
            true,
        ),
        (
            r#"{"type":"clock","limit":60,"increment":0}"#,
            "standard",
            false,
        ), // bullet
        (
            r#"{"type":"clock","limit":15,"increment":0}"#,
            "standard",
            false,
        ), // ultrabullet
        (
            r#"{"type":"clock","limit":600,"increment":5}"#,
            "crazyhouse",
            false,
        ),
        (
            r#"{"type":"clock","limit":600,"increment":5}"#,
            "chess960",
            true, // M9-6: Chess960 challenges are playable now
        ),
        (
            r#"{"type":"clock","limit":600,"increment":5}"#,
            "fromPosition",
            false,
        ),
        (
            r#"{"type":"correspondence","daysPerTurn":3}"#,
            "standard",
            false,
        ),
        (r#"{"type":"unlimited"}"#, "standard", false),
    ];
    for (tc, variant, playable) in cases {
        let line = format!(
            r#"{{"type":"challenge","challenge":{{"id":"chal0001","variant":{{"key":"{variant}"}},"timeControl":{tc},"direction":"in"}}}}"#
        );
        assert_eq!(challenge(&line).is_playable(), playable, "{tc} {variant}");
    }
}

#[test]
fn a_challenge_without_time_control_is_not_playable() {
    let c = challenge(r#"{"type":"challenge","challenge":{"id":"chal0001"}}"#);
    assert!(!c.is_playable());
}

#[test]
fn unknown_event_types_and_future_fields_do_not_break() {
    assert_eq!(
        Event::parse_line(r#"{"type":"somethingNew","payload":{"a":[1,2]}}"#),
        Event::Unknown
    );
    let Event::Challenge(c) = Event::parse_line(
        r#"{"type":"challenge","challenge":{"id":"chal0001","speed":"hyperBulletFromTheFuture","direction":"sideways","finalColor":"green"}}"#,
    ) else {
        panic!("expected a challenge");
    };
    assert_eq!(c.speed, Some(Speed::Unknown));
    assert_eq!(c.direction, None);
    assert_eq!(c.final_color, None);
}

#[test]
fn challenge_update_stream_lines() {
    assert_eq!(
        ChallengeUpdate::parse_line(r#"{"done":"declined"}"#),
        ChallengeUpdate::Done("declined".to_owned())
    );
    assert!(matches!(
        ChallengeUpdate::parse_line(r#"{"id":"chal0001","status":"created","url":"x"}"#),
        ChallengeUpdate::Challenge(c) if c.id == "chal0001"
    ));
    assert!(matches!(
        ChallengeUpdate::parse_line("{}"),
        ChallengeUpdate::Unparsed { .. }
    ));
    assert!(matches!(
        ChallengeUpdate::parse_line(r#"{"done":7}"#),
        ChallengeUpdate::Unparsed { .. }
    ));
}

// ------------------------------------------------------------------ account

#[test]
fn account_decodes_perfs_and_ignores_future_fields() {
    let a2: Account = serde_json::from_str(
        r#"{"id":"bob","username":"Bob","title":"NM","perfs":{"blitz":{"games":10,"rating":1500,"rd":80,"prog":3,"prov":true},
        "rapid":{"games":-2,"rating":1612.5}},"createdAt":1,"unknownFuture":{"x":1}}"#,
    )
    .expect("tolerant account");
    assert_eq!(a2.username, "Bob");
    assert_eq!(a2.perfs["blitz"].rating, 1500);
    assert!(a2.perfs["blitz"].prov);
    assert_eq!(a2.perfs["rapid"].rating, 0, "out-of-range number tolerated");
    assert_eq!(a2.perfs["rapid"].games, 0);
    assert!(!a2.is_bot());
}

#[test]
fn only_the_bot_title_marks_a_bot_account() {
    let cases: Vec<(Option<&str>, bool)> = vec![
        (Some("BOT"), true),
        (Some("GM"), false),
        (Some("bot"), false),
        (None, false),
    ];
    for (title, bot) in cases {
        let a = Account {
            title: title.map(str::to_owned),
            ..Account::default()
        };
        assert_eq!(a.is_bot(), bot);
    }
}

// ------------------------------------------------------------ request forms

fn field<'a>(fields: &'a [(&'static str, String)], key: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.as_str())
}

#[test]
fn seek_form_carries_the_documented_fields() {
    let mut p = SeekParams::new(10, 5);
    p.rated = true;
    p.color = ColorChoice::Black;
    p.rating_range = Some((1400, 1700));
    let f = p.form_fields().expect("valid");
    assert_eq!(field(&f, "time"), Some("10"));
    assert_eq!(field(&f, "increment"), Some("5"));
    assert_eq!(field(&f, "rated"), Some("true"));
    assert_eq!(field(&f, "variant"), Some("standard"));
    assert_eq!(field(&f, "color"), Some("black"));
    assert_eq!(field(&f, "ratingRange"), Some("1400-1700"));
}

#[test]
fn seek_form_defaults_are_casual_random_color_no_range() {
    let f = SeekParams::new(15, 10).form_fields().expect("valid");
    assert_eq!(
        field(&f, "rated"),
        Some("false"),
        "D1315: informal by default"
    );
    assert_eq!(field(&f, "color"), None, "random color is left empty");
    assert_eq!(field(&f, "ratingRange"), None);
}

#[test]
fn seek_form_validates_before_any_network() {
    assert!(matches!(
        SeekParams::new(3, 0).form_fields(),
        Err(Error::TimeControl(TimeControlError::TooFast(Speed::Blitz)))
    ));
    assert!(matches!(
        SeekParams::new(181, 0).form_fields(),
        Err(Error::TimeControl(TimeControlError::OutOfRange))
    ));
    let mut p = SeekParams::new(10, 0);
    p.rating_range = Some((1700, 1400));
    assert!(matches!(
        p.form_fields(),
        Err(Error::InvalidArgument { .. })
    ));
    p.rating_range = Some((1500, 1500));
    assert!(p.form_fields().is_ok(), "lo == hi is a valid range");
}

#[test]
fn challenge_form_keeps_the_stream_alive_and_validates() {
    let mut p = ChallengeParams::new(600, 5);
    p.rated = true;
    p.color = ColorChoice::White;
    let f = p.form_fields().expect("valid");
    assert_eq!(field(&f, "clock.limit"), Some("600"));
    assert_eq!(field(&f, "clock.increment"), Some("5"));
    assert_eq!(field(&f, "rated"), Some("true"));
    assert_eq!(field(&f, "color"), Some("white"));
    assert_eq!(field(&f, "variant"), Some("standard"));
    assert_eq!(field(&f, "keepAliveStream"), Some("true"), "D1316");
    assert_eq!(
        field(
            &ChallengeParams::new(600, 0).form_fields().expect("ok"),
            "color"
        ),
        Some("random")
    );

    assert!(matches!(
        ChallengeParams::new(60, 0).form_fields(),
        Err(Error::TimeControl(TimeControlError::TooFast(Speed::Bullet)))
    ));
    assert!(matches!(
        ChallengeParams::new(75, 0).form_fields(),
        Err(Error::TimeControl(TimeControlError::BadLimit))
    ));
}

#[test]
fn color_opposite_is_an_involution() {
    assert_eq!(Color::White.opposite(), Color::Black);
    assert_eq!(Color::Black.opposite().opposite(), Color::Black);
}

// ------------------------------------------------------------- time control

#[test]
fn classification_follows_limit_plus_forty_times_increment() {
    let cases: Vec<(u32, u32, Speed)> = vec![
        (0, 0, Speed::UltraBullet),
        (15, 0, Speed::UltraBullet),
        (29, 0, Speed::UltraBullet),
        (30, 0, Speed::Bullet),
        (60, 0, Speed::Bullet),
        (179, 0, Speed::Bullet),
        (180, 0, Speed::Blitz),
        (300, 0, Speed::Blitz),
        (479, 0, Speed::Blitz),
        (480, 0, Speed::Rapid),
        (600, 0, Speed::Rapid),
        (1499, 0, Speed::Rapid),
        (1500, 0, Speed::Classical),
        (3600, 0, Speed::Classical),
    ];
    for (limit, inc, want) in cases {
        assert_eq!(classify(limit, inc), want, "{limit}+{inc}");
    }
}

#[test]
fn classification_counts_forty_seconds_per_increment_second() {
    let cases: Vec<(u32, u32, Speed)> = vec![
        (0, 1, Speed::Bullet),        // 40
        (60, 1, Speed::Bullet),       // 100
        (120, 2, Speed::Blitz),       // 200
        (180, 2, Speed::Blitz),       // 260
        (300, 3, Speed::Blitz),       // 420: 5+3
        (60, 10, Speed::Blitz),       // 460
        (80, 10, Speed::Rapid),       // 480
        (600, 5, Speed::Rapid),       // 800: 10+5
        (900, 10, Speed::Rapid),      // 1300: 15+10
        (1800, 20, Speed::Classical), // 30+20
        (1100, 10, Speed::Classical), // 1500
    ];
    for (limit, inc, want) in cases {
        assert_eq!(classify(limit, inc), want, "{limit}+{inc}");
    }
}

#[test]
fn classification_does_not_overflow_on_extreme_input() {
    assert_eq!(classify(u32::MAX, u32::MAX), Speed::Classical);
    assert_eq!(classify(0, u32::MAX), Speed::Classical);
}

#[test]
fn seek_accepts_rapid_and_classical_only() {
    let cases: Vec<(u32, u32, Result<Speed, TimeControlError>)> = vec![
        (10, 0, Ok(Speed::Rapid)),
        (10, 5, Ok(Speed::Rapid)),
        (15, 10, Ok(Speed::Rapid)),
        (30, 0, Ok(Speed::Classical)),
        (30, 20, Ok(Speed::Classical)),
        (8, 0, Ok(Speed::Rapid)),
        (180, 180, Ok(Speed::Classical)),
        (7, 0, Err(TimeControlError::TooFast(Speed::Blitz))),
        (3, 0, Err(TimeControlError::TooFast(Speed::Blitz))),
        (5, 3, Err(TimeControlError::TooFast(Speed::Blitz))),
        (1, 0, Err(TimeControlError::TooFast(Speed::Bullet))),
        (0, 0, Err(TimeControlError::TooFast(Speed::UltraBullet))),
        (181, 0, Err(TimeControlError::OutOfRange)),
        (10, 181, Err(TimeControlError::OutOfRange)),
        (u32::MAX, 0, Err(TimeControlError::OutOfRange)),
        (10, u32::MAX, Err(TimeControlError::OutOfRange)),
    ];
    for (minutes, inc, want) in cases {
        assert_eq!(validate_seek(minutes, inc), want, "{minutes}+{inc}");
    }
}

#[test]
fn challenge_accepts_blitz_or_slower_with_the_documented_limits() {
    let cases: Vec<(u32, u32, Result<Speed, TimeControlError>)> = vec![
        (180, 0, Ok(Speed::Blitz)),  // 3+0
        (300, 3, Ok(Speed::Blitz)),  // 5+3
        (600, 0, Ok(Speed::Rapid)),  // 10+0
        (900, 10, Ok(Speed::Rapid)), // 15+10
        (1800, 0, Ok(Speed::Classical)),
        (10_800, 60, Ok(Speed::Classical)),
        (0, 5, Ok(Speed::Blitz)),  // 0+5 = 200
        (45, 4, Ok(Speed::Blitz)), // 205
        (60, 0, Err(TimeControlError::TooFast(Speed::Bullet))),
        (120, 1, Err(TimeControlError::TooFast(Speed::Bullet))), // 160
        (15, 0, Err(TimeControlError::TooFast(Speed::UltraBullet))),
        (0, 0, Err(TimeControlError::TooFast(Speed::UltraBullet))),
        (75, 0, Err(TimeControlError::BadLimit)),
        (61, 0, Err(TimeControlError::BadLimit)),
        (10_860, 0, Err(TimeControlError::OutOfRange)),
        (600, 61, Err(TimeControlError::OutOfRange)),
        (u32::MAX, u32::MAX, Err(TimeControlError::OutOfRange)),
    ];
    for (limit, inc, want) in cases {
        assert_eq!(validate_challenge(limit, inc), want, "{limit}+{inc}");
    }
}

#[test]
fn challenge_limits_0_15_30_45_60_90_and_multiples_of_sixty_are_the_only_ones() {
    for limit in 0..=300u32 {
        let allowed = matches!(limit, 0 | 15 | 30 | 45 | 90) || limit % 60 == 0;
        let r = validate_challenge(limit, 5);
        assert_eq!(
            !matches!(r, Err(TimeControlError::BadLimit)),
            allowed,
            "limit {limit}"
        );
    }
}

#[test]
fn speed_names_decode_and_unknown_is_tolerated() {
    for (name, want) in [
        ("ultraBullet", Speed::UltraBullet),
        ("bullet", Speed::Bullet),
        ("blitz", Speed::Blitz),
        ("rapid", Speed::Rapid),
        ("classical", Speed::Classical),
        ("correspondence", Speed::Correspondence),
        ("tomorrowSpeed", Speed::Unknown),
    ] {
        let got: Speed = serde_json::from_str(&format!("\"{name}\"")).expect(name);
        assert_eq!(got, want, "{name}");
    }
}

// ------------------------------------------------------------------ backoff

#[test]
fn backoff_nominal_sequence_is_1_2_4_8_16_30_30() {
    let b = Backoff::with_unit(Duration::from_secs(1), 99);
    let got: Vec<u64> = (0..9).map(|i| b.nominal(i).as_secs()).collect();
    assert_eq!(got, [1, 2, 4, 8, 16, 30, 30, 30, 30]);
    for huge in [31, 32, 33, 63, 64, 1000, u32::MAX] {
        assert_eq!(b.nominal(huge).as_secs(), 30, "attempt {huge}");
    }
}

#[test]
fn backoff_delays_follow_the_sequence_with_twenty_percent_jitter() {
    let nominal = [1.0, 2.0, 4.0, 8.0, 16.0, 30.0, 30.0, 30.0];
    for seed in [0u64, 1, 2, 12345, u64::MAX, 0xDEAD_BEEF] {
        let mut b = Backoff::with_unit(Duration::from_secs(1), seed);
        for (i, n) in nominal.iter().enumerate() {
            let d = b.next_delay().as_secs_f64();
            assert!(
                d >= n * 0.8 - 1e-6 && d <= n * 1.2 + 1e-6,
                "seed {seed} attempt {i}: {d} vs {n}"
            );
        }
        assert_eq!(b.attempts(), 8);
    }
}

#[test]
fn backoff_jitter_actually_varies_and_goes_both_ways() {
    let mut b = Backoff::with_unit(Duration::from_secs(10), 2024);
    let delays: Vec<Duration> = (0..40)
        .map(|_| {
            b.reset();
            b.next_delay()
        })
        .collect();
    let nominal = Duration::from_secs(10);
    assert!(delays.iter().any(|d| *d < nominal), "some below nominal");
    assert!(delays.iter().any(|d| *d > nominal), "some above nominal");
    let first = delays[0];
    assert!(delays.iter().any(|d| *d != first), "not a constant");
}

#[test]
fn backoff_reset_restarts_at_the_first_step() {
    let mut b = Backoff::with_unit(Duration::from_secs(1), 5);
    for _ in 0..6 {
        let _ = b.next_delay();
    }
    assert_eq!(b.attempts(), 6);
    b.reset();
    assert_eq!(b.attempts(), 0);
    assert!(b.next_delay() <= Duration::from_millis(1200));
}

#[test]
fn backoff_scales_with_the_unit_and_never_panics_when_it_is_huge() {
    let mut b = Backoff::with_unit(Duration::from_millis(5), 3);
    let d = b.next_delay();
    assert!(
        d >= Duration::from_millis(4) && d <= Duration::from_millis(6),
        "{d:?}"
    );

    let mut huge = Backoff::with_unit(Duration::MAX, 3);
    for _ in 0..10 {
        let _ = huge.next_delay();
    }
    let mut zero = Backoff::with_unit(Duration::ZERO, 3);
    assert_eq!(zero.next_delay(), Duration::ZERO);
}

#[test]
fn backoff_attempt_counter_saturates() {
    let mut b = Backoff::new();
    for _ in 0..200 {
        let d = b.next_delay();
        assert!(d <= Duration::from_secs(36), "{d:?}");
    }
    assert_eq!(b.attempts(), 200);
}

proptest! {
    #[test]
    fn backoff_jitter_bound_holds_for_any_seed_and_unit(seed in any::<u64>(), unit_ms in 1u64..5000, steps in 1usize..20) {
        let mut b = Backoff::with_unit(Duration::from_millis(unit_ms), seed);
        for i in 0..steps {
            let nominal = b.nominal(u32::try_from(i).unwrap_or(u32::MAX)).as_secs_f64();
            let d = b.next_delay().as_secs_f64();
            prop_assert!(d >= nominal * 0.8 - 1e-6 && d <= nominal * 1.2 + 1e-6, "{d} vs {nominal}");
        }
    }
}

#[test]
fn m9_6_chess960_seek_and_challenge_fields() {
    let mut seek = SeekParams::new(10, 5);
    seek.chess960 = true;
    let f = seek.form_fields().expect("valid");
    assert_eq!(field(&f, "variant"), Some("chess960"));

    let mut c = ChallengeParams::new(600, 5);
    c.chess960 = true;
    let f = c.form_fields().expect("valid");
    assert_eq!(field(&f, "variant"), Some("chess960"));
    assert_eq!(
        field(&f, "fen"),
        None,
        "no fixed position: the server draws"
    );

    c.initial_fen = Some("bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq - 0 1".to_owned());
    let f = c.form_fields().expect("valid");
    assert_eq!(
        field(&f, "fen"),
        Some("bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq - 0 1")
    );
    assert_eq!(field(&f, "keepAliveStream"), Some("true"));
}

#[test]
fn m9_6_a_fixed_position_needs_a_casual_chess960_challenge() {
    let fen = Some("bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq - 0 1".to_owned());
    let mut c = ChallengeParams::new(600, 5);
    c.initial_fen = fen.clone();
    assert!(c.form_fields().is_err(), "standard variant refuses a fen");
    c.chess960 = true;
    c.rated = true;
    assert!(c.form_fields().is_err(), "a fixed position cannot be rated");
    c.rated = false;
    c.initial_fen = Some("  ".to_owned());
    assert!(c.form_fields().is_err(), "blank fen");
    c.initial_fen = Some("8/8\nk7".to_owned());
    assert!(c.form_fields().is_err(), "multi-line fen");
    c.initial_fen = fen;
    assert!(c.form_fields().is_ok());
}
