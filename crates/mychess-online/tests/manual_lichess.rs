//! Manual check against the real Lichess. Ignored: no automatic test touches
//! `lichess.org` (spec m8-3, D1339). Run with
//! `LICHESS_TOKEN=... cargo test -p mychess-online --test manual_lichess -- --ignored`.
//! It only does `GET /api/account`.

use mychess_online::{Client, ClientConfig, Token};

#[tokio::test]
#[ignore = "talks to lichess.org; needs LICHESS_TOKEN"]
async fn account_with_a_real_token() {
    let text = std::env::var("LICHESS_TOKEN").expect("set LICHESS_TOKEN");
    let client = Client::new(ClientConfig {
        token: Some(Token::new(&text).expect("token shape")),
        ..ClientConfig::default()
    })
    .expect("client");
    let info = client.validate_login().await.expect("account");
    assert_ne!(info.account.username, "");
}
