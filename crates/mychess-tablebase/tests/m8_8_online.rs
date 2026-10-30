//! Contract tests of the online source against the fake server (spec m8-8,
//! D2564, D2565, D2566, D2582, D2590; acceptance criterion 5). No real network.

#![allow(clippy::assert_is_empty, clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use mychess_core::Position;
use mychess_online::testing::{FakeLichess, Response};
use mychess_online::{Client, ClientConfig};
use mychess_tablebase::online::{MAX_BODY_BYTES, MAX_REPLY_MOVES, parse_reply};
use mychess_tablebase::{
    CachedOnline, Category, LichessTablebase, OnlineProbe, Source, TablebaseError,
};
use pretty_assertions::assert_eq;

const FEN: &str = "8/8/8/8/8/2k5/8/KQ6 w - - 0 1";

fn kq() -> Position {
    Position::from_fen(FEN).unwrap()
}

/// What the server says about `FEN`; the categories of the moves are from the
/// point of view of the side that moves next (the opponent).
const REPLY: &str = r#"{"checkmate":false,"stalemate":false,"variant_win":false,
"variant_loss":false,"insufficient_material":false,"dtz":11,"precise_dtz":11,
"dtm":11,"dtw":null,"dtc":null,"category":"win","unknown_field":[1,2],
"moves":[
{"uci":"b1e4","san":"WRONG","zeroing":false,"checkmate":false,"stalemate":false,
 "insufficient_material":false,"dtz":-10,"precise_dtz":-10,"dtm":-10,"category":"loss"},
{"uci":"b1d1","san":"Qd1","zeroing":false,"checkmate":false,"stalemate":false,
 "insufficient_material":false,"dtz":-12,"precise_dtz":null,"dtm":null,"category":"loss"},
{"uci":"a1a2","san":"Ka2","zeroing":false,"checkmate":false,"stalemate":false,
 "insufficient_material":false,"dtz":0,"precise_dtz":0,"dtm":null,"category":"surprise"}
]}"#;

async fn server(response: Response) -> (FakeLichess, LichessTablebase) {
    let fake = FakeLichess::start().await.unwrap();
    fake.fallback("GET", "/standard", response);
    let tb = LichessTablebase::with_base_url(&fake.base_url()).unwrap();
    (fake, tb)
}

#[tokio::test]
async fn a_normal_answer_is_read_from_the_point_of_view_of_the_mover() {
    let (fake, tb) = server(Response::json(200, REPLY)).await;
    let r = tb.probe(&kq()).await.unwrap();
    assert_eq!(r.source, Source::Online);
    assert_eq!(r.pieces, 3);
    assert_eq!(r.evaluation.category, Category::Win);
    assert_eq!(r.evaluation.dtz, Some(11));
    assert_eq!(r.evaluation.dtm, Some(11));
    assert!(!r.evaluation.dtz_rounded);
    assert!(r.has_dtm());
    // "loss" for the opponent after Qe4 is a win for the mover; the SAN is ours.
    let e4 = r.moves.iter().find(|m| m.uci == "b1e4").unwrap();
    assert_eq!(e4.san, "Qe4");
    assert_eq!(e4.evaluation.category, Category::Win);
    assert_eq!(e4.evaluation.dtz, Some(10));
    assert_eq!(e4.evaluation.dtm, Some(10));
    // A rounded DTZ is flagged.
    let d1 = r.moves.iter().find(|m| m.uci == "b1d1").unwrap();
    assert_eq!(d1.evaluation.dtz, Some(12));
    assert!(d1.evaluation.dtz_rounded);
    // A category we do not know is unknown, not an error.
    let a2 = r.moves.iter().find(|m| m.uci == "a1a2").unwrap();
    assert_eq!(a2.evaluation.category, Category::Unknown);
    // Sorted: shortest win first.
    assert_eq!(r.moves[0].uci, "b1e4");
    assert_eq!(r.moves[1].uci, "b1d1");
    drop(fake);
}

#[tokio::test]
async fn the_request_is_a_plain_get_with_the_fen_and_no_credentials() {
    let (fake, tb) = server(Response::json(200, REPLY)).await;
    tb.probe(&kq()).await.unwrap();
    let requests = fake.requests_for("GET", "/standard");
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(
        request.query_pairs(),
        vec![("fen".to_owned(), FEN.to_owned())]
    );
    assert!(request.header("authorization").is_none());
    assert!(
        request
            .header("user-agent")
            .is_some_and(|ua| ua.starts_with("MyChess/"))
    );
}

#[tokio::test]
async fn http_errors_become_typed_errors() {
    for (status, expected) in [
        (404, TablebaseError::NotFound),
        (500, TablebaseError::Server(500)),
        (503, TablebaseError::Server(503)),
    ] {
        let (_fake, tb) = server(Response::status(status)).await;
        assert_eq!(tb.probe(&kq()).await.unwrap_err(), expected, "{status}");
    }
    let (_fake, tb) = server(Response::json(400, r#"{"error":"bad fen"}"#)).await;
    assert!(matches!(
        tb.probe(&kq()).await.unwrap_err(),
        TablebaseError::Protocol(_)
    ));
}

#[tokio::test]
async fn a_429_blocks_the_next_query_without_touching_the_network() {
    let (fake, tb) = server(Response::status(429)).await;
    assert!(matches!(
        tb.probe(&kq()).await.unwrap_err(),
        TablebaseError::RateLimited { .. }
    ));
    let other = Position::from_fen("8/8/8/8/8/2k5/8/KR6 w - - 0 1").unwrap();
    assert!(matches!(
        tb.probe(&other).await.unwrap_err(),
        TablebaseError::RateLimited { retry_after } if retry_after > Duration::from_secs(1)
    ));
    assert_eq!(fake.requests_for("GET", "/standard").len(), 1);
}

#[tokio::test]
async fn a_body_over_the_limit_is_a_protocol_error() {
    let big = format!(
        r#"{{"category":"win","pad":"{}"}}"#,
        "x".repeat(MAX_BODY_BYTES)
    );
    let (_fake, tb) = server(Response::json(200, &big)).await;
    assert!(matches!(
        tb.probe(&kq()).await.unwrap_err(),
        TablebaseError::Protocol(_)
    ));
}

#[tokio::test]
async fn a_slow_server_times_out() {
    let fake = FakeLichess::start().await.unwrap();
    fake.fallback(
        "GET",
        "/standard",
        Response::json(200, REPLY).head_delay(Duration::from_secs(5)),
    );
    let client = Client::new(ClientConfig {
        base_url: fake.base_url(),
        request_timeout: Duration::from_millis(150),
        ..ClientConfig::default()
    })
    .unwrap();
    let tb = LichessTablebase::with_client(client);
    assert_eq!(tb.probe(&kq()).await.unwrap_err(), TablebaseError::Timeout);
}

#[test]
fn hostile_json_is_a_protocol_error_and_never_a_panic() {
    let pos = kq();
    let many_moves = format!(
        r#"{{"category":"win","moves":[{}]}}"#,
        vec![r#"{"uci":"b1e4","category":"loss"}"#; MAX_REPLY_MOVES + 1].join(",")
    );
    let deep = format!("{}{}", "[".repeat(10_000), "]".repeat(10_000));
    for body in [
        "",
        "not json",
        "[]",
        "null",
        "{",
        r#"{"moves":"x"}"#,
        r#"{"moves":[1,2]}"#,
        r#"{"moves":[{"uci":42}]}"#,
        r#"{"moves":[{"uci":"zzzz"}]}"#,
        r#"{"moves":[{"uci":"a1h8"}]}"#, // illegal in this position
        r#"{"checkmate":"yes"}"#,
        r#"{"dtz":"far"}"#,
        many_moves.as_str(),
        deep.as_str(),
    ] {
        let result = parse_reply(&pos, body.as_bytes());
        assert!(
            matches!(result, Err(TablebaseError::Protocol(_))),
            "{:?}",
            &body[..body.len().min(40)]
        );
    }
}

#[test]
fn absurd_numbers_and_missing_fields_are_tolerated() {
    let pos = kq();
    let r = parse_reply(
        &pos,
        br#"{"category":"win","dtz":99999999999999,"moves":[{"uci":"b1e4","dtz":-9223372036854775808}]}"#,
    )
    .unwrap();
    assert_eq!(r.evaluation.dtz, None);
    let e4 = r.moves.iter().find(|m| m.uci == "b1e4").unwrap();
    assert_eq!(e4.evaluation.dtz, None);
    assert_eq!(e4.evaluation.category, Category::Unknown);
    let empty = parse_reply(&pos, b"{}").unwrap();
    assert_eq!(empty.evaluation.category, Category::Unknown);
    assert!(!empty.complete);
    assert!(
        empty
            .moves
            .iter()
            .all(|m| m.evaluation.category == Category::Unknown)
    );
}

#[test]
fn mate_stalemate_and_insufficient_material_flags_are_honoured() {
    let pos = Position::from_fen("k7/8/1K6/8/8/8/8/1Q6 w - - 0 1").unwrap();
    let r = parse_reply(
        &pos,
        br#"{"category":"win","moves":[
          {"uci":"b1b2","checkmate":true,"category":"loss"},
          {"uci":"b1c1","stalemate":true,"category":"draw"},
          {"uci":"b1a1","insufficient_material":true,"category":"draw"}]}"#,
    )
    .unwrap();
    let mate = r.moves.iter().find(|m| m.uci == "b1b2").unwrap();
    assert!(mate.checkmate);
    assert_eq!(mate.evaluation.category, Category::Win);
    assert_eq!(mate.evaluation.dtm, Some(1));
    let stale = r.moves.iter().find(|m| m.uci == "b1c1").unwrap();
    assert!(stale.stalemate);
    assert_eq!(stale.evaluation.category, Category::Draw);
    assert_eq!(r.moves[0].uci, "b1b2");
}

#[tokio::test]
async fn positions_outside_the_server_range_never_reach_the_network() {
    let (fake, tb) = server(Response::json(200, REPLY)).await;
    let castling = Position::new();
    let eight = Position::from_fen("4k3/pppp4/8/8/8/8/PPPP4/4K3 w - - 0 1").unwrap();
    let two = Position::from_fen("8/8/8/8/8/2k5/8/K7 w - - 0 1").unwrap();
    assert_eq!(
        tb.probe(&castling).await.unwrap_err(),
        TablebaseError::Castling
    );
    assert!(matches!(
        tb.probe(&eight).await.unwrap_err(),
        TablebaseError::TooManyPieces { max: 7, .. }
    ));
    assert!(matches!(
        tb.probe(&two).await.unwrap_err(),
        TablebaseError::TooFewPieces { pieces: 2 }
    ));
    assert!(fake.requests().is_empty());
}

#[tokio::test]
async fn the_cache_asks_the_server_once_per_position() {
    let (fake, tb) = server(Response::json(200, REPLY)).await;
    let cached = CachedOnline::new(tb);
    let pos = kq();
    let first = cached.probe(&pos).await.unwrap();
    let second = cached.probe(&pos).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(fake.requests_for("GET", "/standard").len(), 1);
    assert_eq!(cached.cached(), 1);
    // Same board, other move number: the same key (D2565).
    let later = Position::from_fen("8/8/8/8/8/2k5/8/KQ6 w - - 0 30").unwrap();
    cached.probe(&later).await.unwrap();
    assert_eq!(fake.requests_for("GET", "/standard").len(), 1);
    // Another 50-move clock is another position.
    let clock = Position::from_fen("8/8/8/8/8/2k5/8/KQ6 w - - 9 30").unwrap();
    cached.probe(&clock).await.unwrap();
    assert_eq!(fake.requests_for("GET", "/standard").len(), 2);
}

#[tokio::test]
async fn failures_are_not_cached() {
    let fake = FakeLichess::start().await.unwrap();
    fake.enqueue("GET", "/standard", Response::status(500));
    fake.enqueue("GET", "/standard", Response::json(200, REPLY));
    let cached = CachedOnline::new(LichessTablebase::with_base_url(&fake.base_url()).unwrap());
    assert_eq!(
        cached.probe(&kq()).await.unwrap_err(),
        TablebaseError::Server(500)
    );
    assert_eq!(cached.cached(), 0);
    assert!(cached.probe(&kq()).await.is_ok());
    assert_eq!(cached.cached(), 1);
}

#[test]
fn the_cache_keeps_at_most_its_capacity_dropping_the_oldest() {
    use mychess_tablebase::Evaluation;
    use mychess_tablebase::ProbeCache;
    use mychess_tablebase::testing::report;
    let pos = kq();
    let mut cache = ProbeCache::new(3);
    for i in 0..5 {
        cache.insert(
            format!("k{i}"),
            report(&pos, Source::Online, Evaluation::of(Category::Win), &[]),
        );
    }
    assert_eq!(cache.len(), 3);
    assert!(cache.get("k0").is_none() && cache.get("k1").is_none());
    assert!(cache.get("k2").is_some() && cache.get("k4").is_some());
    assert!(!cache.is_empty());
}

#[test]
fn an_insecure_server_url_is_refused() {
    assert!(LichessTablebase::with_base_url("http://tablebase.lichess.ovh").is_err());
    assert!(LichessTablebase::with_base_url("https://tablebase.lichess.ovh").is_ok());
    assert!(LichessTablebase::new().is_ok());
}

#[test]
fn an_answer_that_leaves_moves_out_is_incomplete_and_recommends_nothing() {
    let pos = kq();
    let legal = pos.legal_moves().len();
    let r = parse_reply(
        &pos,
        br#"{"category":"win","moves":[{"uci":"b1e4","category":"loss","dtz":-2}]}"#,
    )
    .unwrap();
    assert!(!r.complete);
    // Every legal move is listed; the ones the server left out are unknown.
    assert_eq!(r.moves.len(), legal);
    assert!(r.best().is_none() && r.recommended(3).is_none());
}

#[test]
fn a_complete_answer_recommends_its_best_move() {
    let pos = kq();
    let moves: Vec<String> = pos
        .legal_moves()
        .into_iter()
        .map(|mv| {
            let uci = pos.uci(mv);
            let dtz = if uci == "b1e4" { -2 } else { -8 };
            format!(r#"{{"uci":"{uci}","category":"loss","dtz":{dtz},"precise_dtz":{dtz}}}"#)
        })
        .collect();
    let body = format!(r#"{{"category":"win","moves":[{}]}}"#, moves.join(","));
    let r = parse_reply(&pos, body.as_bytes()).unwrap();
    assert!(r.complete);
    assert_eq!(r.best().map(|m| m.uci.as_str()), Some("b1e4"));
}

#[test]
fn a_move_listed_twice_is_a_protocol_error() {
    let pos = kq();
    let twice = br#"{"moves":[{"uci":"b1e4","category":"loss"},{"uci":"b1e4","category":"win"}]}"#;
    assert!(matches!(
        parse_reply(&pos, twice),
        Err(TablebaseError::Protocol(_))
    ));
}
