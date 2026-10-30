//! Helpers shared by the integration tests (local fake server only).
#![allow(dead_code, unreachable_pub)]

use std::time::Duration;

use mychess_online::{
    Client, ClientConfig, GateConfig, StreamHandle, StreamMessage, Token, testing::FakeLichess,
};

pub const TOKEN: &str = "lip_TESTTOKEN123";

pub const GAME_ID: &str = "abcd1234";

pub fn full_json() -> &'static str {
    include_str!("../fixtures/game_full.json").trim()
}

pub fn state_json() -> &'static str {
    include_str!("../fixtures/game_state.json").trim()
}

pub async fn fake() -> FakeLichess {
    FakeLichess::start().await.expect("fake server")
}

/// Client pointing at the fake server, with short timings.
pub fn client_with(fake: &FakeLichess, tweak: impl FnOnce(&mut ClientConfig)) -> Client {
    let mut cfg = ClientConfig {
        base_url: fake.base_url(),
        token: Token::new(TOKEN).ok(),
        action_retry_delay: Duration::from_millis(10),
        gate: GateConfig {
            base_block: Duration::from_millis(150),
            max_block: Duration::from_millis(300),
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

/// Next message, failing the test after 5 s.
pub async fn recv<T>(h: &mut StreamHandle<T>) -> StreamMessage<T> {
    tokio::time::timeout(Duration::from_secs(5), h.next())
        .await
        .expect("timed out waiting for a stream message")
        .expect("stream channel closed")
}
