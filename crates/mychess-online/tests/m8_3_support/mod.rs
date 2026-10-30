//! Helpers of the M8-3a contract suite (local fake server only, no real network).
#![allow(dead_code, unreachable_pub)]

use std::time::Duration;

use mychess_online::{
    Client, ClientConfig, GateConfig, StreamHandle, StreamMessage, Token, testing::FakeLichess,
};

/// A distinctive token so leaks are easy to grep for.
pub const SECRET: &str = "lip_SuperSecretTokenValue0123456789";

/// A valid 8-character game id.
pub const GAME: &str = "abcd1234";

pub async fn fake() -> FakeLichess {
    FakeLichess::start().await.expect("fake server")
}

/// Client for the fake server with short timings; `tweak` may change anything.
pub fn client_with(fake: &FakeLichess, tweak: impl FnOnce(&mut ClientConfig)) -> Client {
    let mut cfg = ClientConfig {
        base_url: fake.base_url(),
        token: Token::new(SECRET).ok(),
        action_retry_delay: Duration::from_millis(10),
        gate: GateConfig {
            base_block: Duration::from_millis(200),
            max_block: Duration::from_millis(800),
        },
        ..ClientConfig::default()
    };
    cfg.streams.backoff_unit = Duration::from_millis(5);
    tweak(&mut cfg);
    Client::new(cfg).expect("client")
}

pub fn client(fake: &FakeLichess) -> Client {
    client_with(fake, |_| {})
}

/// Next stream message, failing the test after 5 s.
pub async fn recv<T>(h: &mut StreamHandle<T>) -> StreamMessage<T> {
    tokio::time::timeout(Duration::from_secs(5), h.next())
        .await
        .expect("timed out waiting for a stream message")
        .expect("stream channel closed")
}

/// Unwraps an item or fails the test.
pub fn item<T: std::fmt::Debug>(m: StreamMessage<T>) -> T {
    match m {
        StreamMessage::Item(t) => t,
        other => panic!("expected an item, got {other:?}"),
    }
}

pub const GAME_FULL: &str = r#"{"id":"abcd1234","variant":{"key":"standard","name":"Standard","short":"Std"},"speed":"rapid","perf":{"name":"Rapid"},"rated":false,"createdAt":1700000000000,"white":{"id":"alice","name":"alice","title":"NM","rating":1800,"provisional":false},"black":{"id":"bob","name":"Bob","rating":1500,"provisional":true},"initialFen":"startpos","clock":{"initial":600000,"increment":5000},"type":"gameFull","state":{"type":"gameState","moves":"e2e4 e7e5","wtime":600000,"btime":600000,"winc":5000,"binc":5000,"status":"started"}}"#;

pub const STATE_STARTED: &str = r#"{"type":"gameState","moves":"e2e4 e7e5 g1f3","wtime":590000,"btime":600000,"winc":5000,"binc":5000,"status":"started"}"#;

pub const STATE_RESIGN: &str = r#"{"type":"gameState","moves":"e2e4 e7e5 g1f3","wtime":580000,"btime":570000,"winc":5000,"binc":5000,"status":"resign","winner":"white"}"#;
