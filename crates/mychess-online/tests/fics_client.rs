//! The FICS connection against the fake server (spec m8-9, D2620). Real
//! short times: socket tests do not mix with a paused clock.

use std::time::Duration;

use mychess_online::fics::testing::{FakeFics, FakeOptions};
use mychess_online::fics::{
    Command, Disconnect, FicsConfig, FicsEvent, FicsEvents, FicsMessage, FicsSender, Login,
    LoginFailure, Password, connect,
};

const LIMIT: Duration = Duration::from_secs(5);

async fn next(events: &mut FicsEvents) -> FicsMessage {
    tokio::time::timeout(LIMIT, events.next())
        .await
        .expect("a message in time")
        .expect("the stream is open")
}

/// Reads until `pick` accepts a message.
async fn until<T>(events: &mut FicsEvents, mut pick: impl FnMut(&FicsMessage) -> Option<T>) -> T {
    loop {
        let m = next(events).await;
        if let Some(found) = pick(&m) {
            return found;
        }
    }
}

fn guest(server: &FakeFics) -> (FicsSender, FicsEvents) {
    connect(FicsConfig::new(&server.host(), server.port(), Login::Guest)).expect("starts")
}

#[tokio::test]
async fn guest_login_needs_no_password_and_sends_the_init_commands() {
    let server = FakeFics::start(FakeOptions::default()).await;
    let (_tx, mut ev) = guest(&server);
    assert_eq!(next(&mut ev).await, FicsMessage::Connected);
    let name = until(&mut ev, |m| match m {
        FicsMessage::LoggedIn { name } => Some(name.clone()),
        _ => None,
    })
    .await;
    assert_eq!(name, "GuestTEST");
    let got = server.wait_line(|l| l == "set availinfo off", LIMIT).await;
    assert!(got.is_some());
    let lines = server.received();
    assert!(lines.contains(&"style 12".to_owned()));
    assert!(lines.contains(&"iset seekinfo 1".to_owned()));
}

#[tokio::test]
async fn registered_login_sends_the_password_only_to_the_socket() {
    let server = FakeFics::start(FakeOptions::default()).await;
    let login = Login::Registered {
        name: "alice".into(),
        password: Password::new("s3cret").expect("valid"),
    };
    let config = FicsConfig::new(&server.host(), server.port(), login);
    assert!(!format!("{config:?}").contains("s3cret"));
    let (_tx, mut ev) = connect(config).expect("starts");
    let mut seen = Vec::new();
    let name = until(&mut ev, |m| {
        seen.push(format!("{m:?}"));
        match m {
            FicsMessage::LoggedIn { name } => Some(name.clone()),
            _ => None,
        }
    })
    .await;
    assert_eq!(name, "alice");
    // The password is not in anything the client reported.
    assert!(seen.iter().all(|s| !s.contains("s3cret")));
}

#[tokio::test]
async fn a_wrong_password_ends_the_connection_with_a_reason() {
    let server = FakeFics::start(FakeOptions::default()).await;
    let login = Login::Registered {
        name: "alice".into(),
        password: Password::new("wrong").expect("valid"),
    };
    let (_tx, mut ev) =
        connect(FicsConfig::new(&server.host(), server.port(), login)).expect("starts");
    let end = until(&mut ev, |m| match m {
        FicsMessage::Disconnected(d) => Some(*d),
        _ => None,
    })
    .await;
    assert_eq!(end, Disconnect::LoginFailed(LoginFailure::InvalidPassword));
    assert!(ev.next().await.is_none());
}

#[tokio::test]
async fn an_unknown_account_is_reported() {
    let server = FakeFics::start(FakeOptions {
        account: None,
        ..FakeOptions::default()
    })
    .await;
    let login = Login::Registered {
        name: "zed".into(),
        password: Password::new("x").expect("valid"),
    };
    let (_tx, mut ev) =
        connect(FicsConfig::new(&server.host(), server.port(), login)).expect("starts");
    let end = until(&mut ev, |m| match m {
        FicsMessage::Disconnected(d) => Some(*d),
        _ => None,
    })
    .await;
    assert_eq!(end, Disconnect::LoginFailed(LoginFailure::NotRegistered));
}

#[tokio::test]
async fn the_login_survives_a_dribbling_server() {
    let server = FakeFics::start(FakeOptions {
        dribble: true,
        ..FakeOptions::default()
    })
    .await;
    let (_tx, mut ev) = guest(&server);
    let name = until(&mut ev, |m| match m {
        FicsMessage::LoggedIn { name } => Some(name.clone()),
        _ => None,
    })
    .await;
    assert_eq!(name, "GuestTEST");
}

#[tokio::test]
async fn commands_go_out_and_events_come_in_parsed() {
    let server = FakeFics::start(FakeOptions::default()).await;
    let (tx, mut ev) = guest(&server);
    until(&mut ev, |m| {
        matches!(m, FicsMessage::LoggedIn { .. }).then_some(())
    })
    .await;
    tx.send(Command::raw("seek 5 0 unrated").expect("ok"))
        .expect("queued");
    assert!(
        server
            .wait_line(|l| l == "seek 5 0 unrated", LIMIT)
            .await
            .is_some()
    );
    server.send_line("<s> 16 w=bob ti=00 rt=1500E t=5 i=0 r=u tp=blitz c=? rr=0-9999 a=t f=f");
    server.send_line("<12> rnbqkbnr pppppppp -------- -------- -------- -------- PPPPPPPP RNBQKBNR W -1 1 1 1 1 0 7 GuestTEST bob 1 5 0 39 39 300 300 1 none (0:00) none 0");
    server.send_line("bob tells you: hello");
    let mut got = (false, false, false);
    while got != (true, true, true) {
        if let FicsMessage::Event(e) = next(&mut ev).await {
            match e {
                FicsEvent::SeekAdd(s) => got.0 = s.index == 16,
                FicsEvent::Style12(s) => got.1 = s.game == 7,
                FicsEvent::Tell { text, .. } => got.2 = text == "hello",
                _ => {}
            }
        }
    }
}

#[tokio::test]
async fn telnet_noise_and_split_lines_are_handled() {
    let server = FakeFics::start(FakeOptions::default()).await;
    let (_tx, mut ev) = guest(&server);
    until(&mut ev, |m| {
        matches!(m, FicsMessage::LoggedIn { .. }).then_some(())
    })
    .await;
    server.send_raw(&[255, 251, 1, b'b', b'o']);
    server.send_raw(b"b tells you: sp");
    server.send_raw(b"lit\r\n");
    let text = until(&mut ev, |m| match m {
        FicsMessage::Event(FicsEvent::Tell { text, .. }) => Some(text.clone()),
        _ => None,
    })
    .await;
    assert_eq!(text, "split");
}

#[tokio::test]
async fn ping_measures_the_latency() {
    let server = FakeFics::start(FakeOptions::default()).await;
    let mut config = FicsConfig::new(&server.host(), server.port(), Login::Guest);
    config.ping_interval = Duration::from_millis(60);
    let (_tx, mut ev) = connect(config).expect("starts");
    let latency = until(&mut ev, |m| match m {
        FicsMessage::Latency(d) => Some(*d),
        _ => None,
    })
    .await;
    assert!(latency < Duration::from_secs(2));
    assert!(server.received().contains(&"ping".to_owned()));
}

#[tokio::test]
async fn a_silent_server_is_declared_dead() {
    let server = FakeFics::start(FakeOptions {
        answer_ping: false,
        ..FakeOptions::default()
    })
    .await;
    let mut config = FicsConfig::new(&server.host(), server.port(), Login::Guest);
    config.ping_interval = Duration::from_millis(40);
    config.silence_timeout = Duration::from_millis(100);
    let (_tx, mut ev) = connect(config).expect("starts");
    let end = until(&mut ev, |m| match m {
        FicsMessage::Disconnected(d) => Some(*d),
        _ => None,
    })
    .await;
    assert_eq!(end, Disconnect::Timeout);
}

#[tokio::test]
async fn the_server_closing_ends_with_server_closed() {
    let server = FakeFics::start(FakeOptions::default()).await;
    let (tx, mut ev) = guest(&server);
    until(&mut ev, |m| {
        matches!(m, FicsMessage::LoggedIn { .. }).then_some(())
    })
    .await;
    tx.send(Command::quit()).expect("queued");
    let end = until(&mut ev, |m| match m {
        FicsMessage::Disconnected(d) => Some(*d),
        _ => None,
    })
    .await;
    assert_eq!(end, Disconnect::ServerClosed);
}

#[tokio::test]
async fn dropping_the_events_closes_the_socket() {
    let server = FakeFics::start(FakeOptions::default()).await;
    let (_tx, mut ev) = guest(&server);
    until(&mut ev, |m| {
        matches!(m, FicsMessage::LoggedIn { .. }).then_some(())
    })
    .await;
    drop(ev);
    assert!(server.wait_closed(1, LIMIT).await);
}

#[tokio::test]
async fn nothing_listening_is_connect_failed_and_the_login_can_time_out() {
    // A port that nobody listens on: bind and drop.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        l.local_addr().expect("addr").port()
    };
    let (_tx, mut ev) = connect(FicsConfig::new("127.0.0.1", port, Login::Guest)).expect("starts");
    assert_eq!(
        next(&mut ev).await,
        FicsMessage::Disconnected(Disconnect::ConnectFailed)
    );

    // A server that accepts and says nothing.
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = silent.local_addr().expect("addr").port();
    let hold = tokio::spawn(async move {
        let _conn = silent.accept().await;
        tokio::time::sleep(Duration::from_secs(5)).await;
    });
    let mut config = FicsConfig::new("127.0.0.1", port, Login::Guest);
    config.login_timeout = Duration::from_millis(100);
    let (_tx, mut ev) = connect(config).expect("starts");
    let end = until(&mut ev, |m| match m {
        FicsMessage::Disconnected(d) => Some(*d),
        _ => None,
    })
    .await;
    assert_eq!(end, Disconnect::Timeout);
    hold.abort();
}

#[tokio::test]
async fn invalid_settings_are_refused_before_any_connection() {
    assert!(connect(FicsConfig::new("", 5000, Login::Guest)).is_err());
    assert!(connect(FicsConfig::new("a b", 5000, Login::Guest)).is_err());
    assert!(connect(FicsConfig::new("127.0.0.1", 0, Login::Guest)).is_err());
    let login = Login::Registered {
        name: "a\nb".into(),
        password: Password::new("x").expect("valid"),
    };
    assert!(connect(FicsConfig::new("127.0.0.1", 5000, login)).is_err());
    assert!(Password::new("").is_err());
    assert!(Password::new("a\nquit").is_err());
    assert_eq!(
        format!("{:?}", Password::new("abc").expect("ok")),
        "Password(<redacted>)"
    );
}

#[test]
fn connecting_outside_a_runtime_is_an_error_not_a_panic() {
    assert!(connect(FicsConfig::new("127.0.0.1", 5000, Login::Guest)).is_err());
}
