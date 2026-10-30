//! Login by OAuth 2 Authorization Code + PKCE through a loopback callback
//! (spec m8-3, D1306 and D3005-D3015).
//!
//! The flow, for a public client without secret:
//!
//! 1. [`OAuthFlow::start`] binds `127.0.0.1:0` (an ephemeral port), draws a
//!    `code_verifier` and a `state` from the operating system random source
//!    and builds the authorization URL (`S256` challenge only);
//! 2. the caller opens [`OAuthFlow::authorize_url`] in the browser;
//! 3. [`OAuthFlow::finish`] waits for the browser to come back to
//!    `http://127.0.0.1:<port>/callback?code=...&state=...`, checks the
//!    `state`, answers with a small page, closes the listener and exchanges
//!    the code for a token ([`crate::Client::exchange_code`]).
//!
//! Safety rules: only `GET /callback` with a `Host` of the loopback listener
//! counts (anything else gets an error page and the wait goes on, so a stray
//! request cannot cancel the login); a callback with a wrong `state` is
//! ignored the same way; the verifier, the code and the token are never
//! logged, printed (`Debug` is redacted) or placed in errors.

use std::{fmt, net::SocketAddr, time::Duration};

use reqwest::Url;
use ring::{
    digest::{SHA256, digest},
    rand::{SecureRandom, SystemRandom},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinSet,
};
use zeroize::Zeroize;

use crate::{
    client::{Client, ClientConfig, validate_base},
    error::{Error, Result},
    token::Token,
};

/// The `client_id` of MyChess (a public client needs no registration at
/// Lichess; the value only names the application on the consent page).
pub const CLIENT_ID: &str = "mychess";

/// The scopes asked for (the same as the personal token of D1305).
pub const SCOPES: [&str; 3] = ["board:play", "challenge:read", "challenge:write"];

/// How long the browser has to complete the login.
pub const DEFAULT_WAIT: Duration = Duration::from_secs(300);

/// Largest request head read from the browser.
const MAX_HEAD: usize = 8 * 1024;
/// Longest time spent on a single connection to the callback port.
const PER_CONNECTION: Duration = Duration::from_secs(10);
/// How many connections are served at once (D3113).
const MAX_CONNECTIONS: usize = 8;
/// Longest accepted authorization code.
const MAX_CODE: usize = 512;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Base64url without padding (RFC 4648 section 5), as PKCE requires.
#[must_use]
pub fn base64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let sextets = [(n >> 18) & 63, (n >> 12) & 63, (n >> 6) & 63, n & 63];
        // 1 byte -> 2 characters, 2 bytes -> 3, 3 bytes -> 4.
        for &s in sextets.iter().take(chunk.len() + 1) {
            if let Some(&c) = usize::try_from(s).ok().and_then(|i| ALPHABET.get(i)) {
                out.push(char::from(c));
            }
        }
    }
    out
}

fn random_text(len_bytes: usize) -> Result<String> {
    let mut buf = vec![0_u8; len_bytes];
    SystemRandom::new()
        .fill(&mut buf)
        .map_err(|_| Error::OAuth("random source failed"))?;
    let text = base64url(&buf);
    buf.zeroize();
    Ok(text)
}

/// The PKCE pair: a secret `code_verifier` and its `S256` challenge.
pub struct Pkce {
    verifier: String,
    challenge: String,
}

impl Pkce {
    /// A fresh pair: 32 random bytes give a 43-character verifier, the
    /// minimum RFC 7636 allows with the most entropy per character.
    ///
    /// # Errors
    /// [`Error::OAuth`] if the system random source fails.
    pub fn generate() -> Result<Self> {
        Ok(Self::from_verifier(random_text(32)?))
    }

    /// The pair of a given verifier (RFC 7636 test vectors).
    #[must_use]
    pub fn from_verifier(verifier: String) -> Self {
        let challenge = base64url(digest(&SHA256, verifier.as_bytes()).as_ref());
        Self {
            verifier,
            challenge,
        }
    }

    /// The secret verifier, for the token request only.
    #[must_use]
    pub fn verifier(&self) -> &str {
        &self.verifier
    }

    /// The `code_challenge` (public).
    #[must_use]
    pub fn challenge(&self) -> &str {
        &self.challenge
    }
}

impl fmt::Debug for Pkce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Pkce(<redacted>)")
    }
}

impl Drop for Pkce {
    fn drop(&mut self) {
        self.verifier.zeroize();
    }
}

/// The authorization URL: `{base}/oauth?response_type=code&client_id=...`.
///
/// # Errors
/// [`Error::InsecureUrl`] when `base_url` is not `https` (or loopback `http`).
pub fn authorize_url(
    base_url: &str,
    redirect_uri: &str,
    code_challenge: &str,
    state: &str,
) -> Result<String> {
    let mut url = validate_base(base_url)?;
    url.path_segments_mut()
        .map_err(|()| Error::InsecureUrl)?
        .clear()
        .push("oauth");
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", &SCOPES.join(" "))
        .append_pair("code_challenge_method", "S256")
        .append_pair("code_challenge", code_challenge)
        .append_pair("state", state);
    Ok(url.into())
}

/// What the callback listener decided about one connection.
enum Verdict {
    /// Not the callback we wait for: keep waiting.
    Ignore,
    /// The authorization code.
    Code(String),
    /// The user (or the server) refused.
    Denied,
}

/// A one-shot HTTP listener on `127.0.0.1` for the redirect of the browser.
#[derive(Debug)]
pub struct CallbackServer {
    listener: TcpListener,
    addr: SocketAddr,
}

impl CallbackServer {
    /// Binds an ephemeral loopback port.
    ///
    /// # Errors
    /// [`Error::Network`] if no port can be bound.
    pub async fn bind() -> Result<Self> {
        Self::bind_port(0).await
    }

    /// Binds `port` on the loopback interface (0 = any free one). A busy
    /// port is an error, never a panic or a silent switch.
    async fn bind_port(port: u16) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .await
            .map_err(|_| Error::Network("cannot bind the login callback port".to_owned()))?;
        let addr = listener
            .local_addr()
            .map_err(|_| Error::Network("cannot read the login callback port".to_owned()))?;
        Ok(Self { listener, addr })
    }

    /// The `redirect_uri` to register in the authorization request.
    #[must_use]
    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}/callback", self.addr.port())
    }

    /// The port in use.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Waits up to `wait` for the callback carrying `state` and returns its
    /// `code`.
    ///
    /// # Errors
    /// [`Error::Timeout`] when `wait` runs out; [`Error::OAuth`] when the
    /// authorization was denied.
    pub async fn wait_for_code(&self, state: &str, wait: Duration) -> Result<String> {
        tokio::time::timeout(wait, self.accept_loop(state))
            .await
            .map_err(|_| Error::Timeout)?
    }

    /// Serves every connection in a task of its own, at most
    /// [`MAX_CONNECTIONS`] at a time, so that a silent client (a browser
    /// that opened a speculative connection, a local program) holds only
    /// its own slot for [`PER_CONNECTION`] and never delays the real
    /// callback. The tasks end with the login: the set aborts them on drop.
    async fn accept_loop(&self, state: &str) -> Result<String> {
        let port = self.addr.port();
        let mut serving: JoinSet<Verdict> = JoinSet::new();
        loop {
            let full = serving.len() >= MAX_CONNECTIONS;
            tokio::select! {
                done = serving.join_next(), if !serving.is_empty() => match done {
                    Some(Ok(Verdict::Code(code))) => return Ok(code),
                    Some(Ok(Verdict::Denied)) => return Err(Error::OAuth("authorization denied")),
                    _ => {}
                },
                accepted = self.listener.accept(), if !full => match accepted {
                    Ok((mut socket, _)) => {
                        let state = state.to_owned();
                        serving.spawn(async move {
                            tokio::time::timeout(
                                PER_CONNECTION,
                                serve_connection(&mut socket, &state, port),
                            )
                            .await
                            .unwrap_or(Verdict::Ignore)
                        });
                    }
                    // A failed accept (aborted connection, descriptor
                    // pressure) must not end the login; do not spin either.
                    Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
                },
            }
        }
    }
}

const PAGE_OK: &str = "<!doctype html><meta charset=\"utf-8\"><title>MyChess</title>\
<p>MyChess: login complete. You can close this tab.</p>\
<p>MyChess: login conclu&iacute;do. Voc&ecirc; pode fechar esta aba.</p>";
const PAGE_DENIED: &str = "<!doctype html><meta charset=\"utf-8\"><title>MyChess</title>\
<p>MyChess: the login was not authorized.</p>\
<p>MyChess: o login n&atilde;o foi autorizado.</p>";
const PAGE_BAD: &str = "<!doctype html><meta charset=\"utf-8\"><title>MyChess</title>\
<p>Invalid request.</p>";

async fn respond(socket: &mut TcpStream, status: &str, body: &str) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
Cache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        body.len()
    );
    // The browser may have gone away; nothing to do about it.
    socket.write_all(head.as_bytes()).await.ok();
    socket.write_all(body.as_bytes()).await.ok();
    socket.shutdown().await.ok();
}

async fn read_head(socket: &mut TcpStream) -> Option<String> {
    let mut data = Vec::with_capacity(1024);
    let mut buf = [0_u8; 1024];
    loop {
        let n = socket.read(&mut buf).await.ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(buf.get(..n)?);
        if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            // Only the head is text; whatever follows (a body) is never read
            // as such, so its bytes cannot make the head invalid (D3112).
            return String::from_utf8(data.get(..end)?.to_vec()).ok();
        }
        if data.len() > MAX_HEAD {
            return None;
        }
    }
}

async fn serve_connection(socket: &mut TcpStream, state: &str, port: u16) -> Verdict {
    let Some(head) = read_head(socket).await else {
        respond(socket, "400 Bad Request", PAGE_BAD).await;
        return Verdict::Ignore;
    };
    let verdict = judge(&head, state, port);
    match &verdict {
        Verdict::Ignore => respond(socket, "400 Bad Request", PAGE_BAD).await,
        Verdict::Code(_) => respond(socket, "200 OK", PAGE_OK).await,
        Verdict::Denied => respond(socket, "200 OK", PAGE_DENIED).await,
    }
    verdict
}

/// Equality that does not stop at the first difference.
fn same_bytes(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0_u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Decides what a request head means; pure, so the edge cases are testable.
fn judge(head: &str, state: &str, port: u16) -> Verdict {
    let mut lines = head.split("\r\n");
    let Some(request_line) = lines.next() else {
        return Verdict::Ignore;
    };
    let mut parts = request_line.split(' ');
    let (Some("GET"), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Verdict::Ignore;
    };
    if !version.starts_with("HTTP/1.") {
        return Verdict::Ignore;
    }
    // DNS rebinding: a page of another origin cannot make the browser send a
    // loopback `Host`.
    let host_ok = lines.any(|l| {
        l.split_once(':').is_some_and(|(name, value)| {
            let value = value.trim();
            name.eq_ignore_ascii_case("host")
                && (value == format!("127.0.0.1:{port}") || value == format!("localhost:{port}"))
        })
    });
    if !host_ok {
        return Verdict::Ignore;
    }
    // Origin-form only: a target such as `@evil/x` or `:80/x` would change
    // the authority of the URL built below (D3111).
    if !target.starts_with('/') {
        return Verdict::Ignore;
    }
    let Ok(url) = Url::parse(&format!("http://127.0.0.1{target}")) else {
        return Verdict::Ignore;
    };
    if url.path() != "/callback" {
        return Verdict::Ignore;
    }
    let (mut code, mut got_state, mut error) = (None, None, false);
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => got_state = Some(value.into_owned()),
            "error" => error = true,
            _ => {}
        }
    }
    let state_ok = got_state
        .as_deref()
        .is_some_and(|s| same_bytes(s.as_bytes(), state.as_bytes()));
    if !state_ok {
        return Verdict::Ignore;
    }
    if error {
        return Verdict::Denied;
    }
    match code {
        Some(c)
            if !c.is_empty() && c.len() <= MAX_CODE && c.bytes().all(|b| b.is_ascii_graphic()) =>
        {
            Verdict::Code(c)
        }
        _ => Verdict::Ignore,
    }
}

/// A login in progress: the listener, the PKCE pair and the `state`.
#[derive(Debug)]
pub struct OAuthFlow {
    server: CallbackServer,
    pkce: Pkce,
    state: String,
    base_url: String,
}

impl OAuthFlow {
    /// Binds the callback port and draws the secrets.
    ///
    /// # Errors
    /// [`Error::InsecureUrl`] for a bad `base_url`; [`Error::Network`] if the
    /// port cannot be bound; [`Error::OAuth`] if the random source fails.
    pub async fn start(base_url: &str) -> Result<Self> {
        validate_base(base_url)?;
        let server = CallbackServer::bind().await?;
        Ok(Self {
            server,
            pkce: Pkce::generate()?,
            state: random_text(16)?,
            base_url: base_url.to_owned(),
        })
    }

    /// The page the user must open in the browser.
    ///
    /// # Errors
    /// See [`authorize_url`].
    pub fn authorize_url(&self) -> Result<String> {
        authorize_url(
            &self.base_url,
            &self.server.redirect_uri(),
            self.pkce.challenge(),
            &self.state,
        )
    }

    /// The `state` of this attempt (tests play the browser with it).
    #[must_use]
    pub fn state(&self) -> &str {
        &self.state
    }

    /// The `redirect_uri` of this attempt.
    #[must_use]
    pub fn redirect_uri(&self) -> String {
        self.server.redirect_uri()
    }

    /// Waits for the browser and exchanges the code for a token. The
    /// listener is closed before the exchange.
    ///
    /// # Errors
    /// [`Error::Timeout`] if the browser never comes back within `wait`;
    /// [`Error::OAuth`] if the user denied; the errors of
    /// [`Client::exchange_code`].
    pub async fn finish(self, wait: Duration) -> Result<Token> {
        let code = self.server.wait_for_code(&self.state, wait).await?;
        let redirect = self.server.redirect_uri();
        drop(self.server);
        let client = Client::new(ClientConfig {
            base_url: self.base_url.clone(),
            token: None,
            ..ClientConfig::default()
        })?;
        let mut code = code;
        let result = client
            .exchange_code(&code, self.pkce.verifier(), &redirect, CLIENT_ID)
            .await;
        code.zeroize();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_matches_rfc_4648_vectors() {
        for (input, want) in [
            ("", ""),
            ("f", "Zg"),
            ("fo", "Zm8"),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg"),
            ("fooba", "Zm9vYmE"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64url(input.as_bytes()), want);
        }
        assert_eq!(base64url(&[0xfb, 0xff, 0xfe]), "-__-");
    }

    #[test]
    fn challenge_matches_the_rfc_7636_vector() {
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_owned());
        assert_eq!(
            pkce.challenge(),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn generated_pairs_are_distinct_urlsafe_and_long_enough() {
        let a = Pkce::generate().expect("pkce");
        let b = Pkce::generate().expect("pkce");
        assert_ne!(a.verifier(), b.verifier());
        assert_eq!(a.verifier().len(), 43);
        assert!(
            a.verifier()
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        );
        assert_eq!(a.challenge().len(), 43);
        assert!(!format!("{a:?}").contains(a.verifier()));
    }

    #[test]
    fn authorize_url_has_every_required_parameter() {
        let url = authorize_url(
            "https://lichess.org",
            "http://127.0.0.1:4242/callback",
            "CHALLENGE",
            "STATE",
        )
        .expect("url");
        let parsed = Url::parse(&url).expect("parse");
        assert_eq!(parsed.path(), "/oauth");
        let q: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["client_id"], CLIENT_ID);
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:4242/callback");
        assert_eq!(q["scope"], "board:play challenge:read challenge:write");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["code_challenge"], "CHALLENGE");
        assert_eq!(q["state"], "STATE");
        assert!(!q.contains_key("code_verifier"));
        assert_eq!(
            authorize_url("http://example.com", "x", "c", "s").err(),
            Some(Error::InsecureUrl)
        );
    }

    fn head(target: &str, host: &str) -> String {
        format!("GET {target} HTTP/1.1\r\nHost: {host}\r\nAccept: */*\r\n\r\n")
    }

    #[test]
    fn judge_accepts_only_the_exact_callback() {
        let ok = judge(
            &head("/callback?code=abc123&state=S", "127.0.0.1:9"),
            "S",
            9,
        );
        assert!(matches!(ok, Verdict::Code(c) if c == "abc123"));
        let localhost = judge(&head("/callback?state=S&code=a%2Bb", "localhost:9"), "S", 9);
        assert!(matches!(localhost, Verdict::Code(c) if c == "a+b"));
        let denied = judge(
            &head("/callback?error=access_denied&state=S", "127.0.0.1:9"),
            "S",
            9,
        );
        assert!(matches!(denied, Verdict::Denied));
        for (target, host, state) in [
            ("/callback?code=abc&state=OTHER", "127.0.0.1:9", "S"),
            ("/callback?code=abc", "127.0.0.1:9", "S"),
            ("/callback?code=abc&state=S", "evil.example:9", "S"),
            ("/callback?code=abc&state=S", "127.0.0.1:10", "S"),
            ("/other?code=abc&state=S", "127.0.0.1:9", "S"),
            ("/favicon.ico", "127.0.0.1:9", "S"),
            ("/callback?code=&state=S", "127.0.0.1:9", "S"),
            ("/callback?state=S", "127.0.0.1:9", "S"),
            ("/callback?error=x&state=WRONG", "127.0.0.1:9", "S"),
        ] {
            let v = judge(&head(target, host), state, 9);
            assert!(matches!(v, Verdict::Ignore), "{target} {host}");
        }
        assert!(matches!(
            judge(
                "POST /callback?code=a&state=S HTTP/1.1\r\nHost: 127.0.0.1:9\r\n\r\n",
                "S",
                9
            ),
            Verdict::Ignore
        ));
        assert!(matches!(judge("garbage", "S", 9), Verdict::Ignore));
        assert!(matches!(judge("", "S", 9), Verdict::Ignore));
        let long = format!("/callback?code={}&state=S", "a".repeat(MAX_CODE + 1));
        assert!(matches!(
            judge(&head(&long, "127.0.0.1:9"), "S", 9),
            Verdict::Ignore
        ));
    }

    #[test]
    fn judge_rejects_a_target_that_is_not_in_origin_form() {
        for target in [
            "@evil.example/callback?code=a&state=S",
            ":80/callback?code=a&state=S",
            "callback?code=a&state=S",
            "http://127.0.0.1/callback?code=a&state=S",
            "",
        ] {
            let v = judge(&head(target, "127.0.0.1:9"), "S", 9);
            assert!(matches!(v, Verdict::Ignore), "{target:?}");
        }
    }

    #[tokio::test]
    async fn the_head_is_read_as_text_only_up_to_its_end() {
        let server = CallbackServer::bind().await.expect("bind");
        let port = server.port();
        let waiter =
            tokio::spawn(
                async move { server.wait_for_code("GOOD", Duration::from_secs(10)).await },
            );
        let mut s = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect");
        let mut req =
            format!("GET /callback?code=ok&state=GOOD HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n")
                .into_bytes();
        // Bytes after the head that are not UTF-8 do not spoil the request.
        req.extend_from_slice(&[0xff, 0xfe, 0x80]);
        s.write_all(&req).await.expect("write");
        let mut out = Vec::new();
        s.read_to_end(&mut out).await.expect("read");
        assert!(out.starts_with(b"HTTP/1.1 200"));
        assert_eq!(waiter.await.expect("join"), Ok("ok".to_owned()));
    }

    #[tokio::test]
    async fn silent_connections_do_not_delay_the_real_callback() {
        let server = CallbackServer::bind().await.expect("bind");
        let port = server.port();
        let waiter =
            tokio::spawn(
                async move { server.wait_for_code("GOOD", Duration::from_secs(20)).await },
            );
        // More silent clients than the limit: each holds a slot of its own
        // for PER_CONNECTION, none holds the line.
        let mut silent = Vec::new();
        for _ in 0..(MAX_CONNECTIONS - 1) {
            silent.push(
                TcpStream::connect(("127.0.0.1", port))
                    .await
                    .expect("connect"),
            );
        }
        let page = tokio::time::timeout(
            Duration::from_secs(3),
            browser(port, "/callback?code=quick&state=GOOD"),
        )
        .await
        .expect("served without waiting for the silent ones");
        assert!(page.starts_with("HTTP/1.1 200"));
        assert_eq!(waiter.await.expect("join"), Ok("quick".to_owned()));
        drop(silent);
    }

    async fn browser(port: u16, target: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect");
        let req =
            format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
        s.write_all(req.as_bytes()).await.expect("write");
        let mut out = String::new();
        s.read_to_string(&mut out).await.expect("read");
        out
    }

    #[tokio::test]
    async fn server_skips_strays_and_returns_the_code_for_the_right_state() {
        let server = CallbackServer::bind().await.expect("bind");
        let port = server.port();
        assert_eq!(
            server.redirect_uri(),
            format!("http://127.0.0.1:{port}/callback")
        );
        let waiter =
            tokio::spawn(
                async move { server.wait_for_code("GOOD", Duration::from_secs(10)).await },
            );
        let stray = browser(port, "/favicon.ico").await;
        assert!(stray.starts_with("HTTP/1.1 400"), "{stray}");
        let wrong = browser(port, "/callback?code=evil&state=BAD").await;
        assert!(wrong.starts_with("HTTP/1.1 400"));
        let page = browser(port, "/callback?code=the_code&state=GOOD").await;
        assert!(page.starts_with("HTTP/1.1 200"));
        assert!(page.contains("Referrer-Policy: no-referrer"));
        assert!(!page.contains("the_code"));
        assert_eq!(waiter.await.expect("join"), Ok("the_code".to_owned()));
    }

    #[tokio::test]
    async fn a_busy_port_is_a_network_error_not_a_panic() {
        let first = CallbackServer::bind().await.expect("bind");
        let second = CallbackServer::bind_port(first.port()).await;
        assert!(matches!(second, Err(Error::Network(_))), "{second:?}");
    }

    #[tokio::test]
    async fn server_reports_denial_and_times_out() {
        let server = CallbackServer::bind().await.expect("bind");
        let port = server.port();
        let waiter =
            tokio::spawn(async move { server.wait_for_code("S", Duration::from_secs(10)).await });
        browser(port, "/callback?error=access_denied&state=S").await;
        assert_eq!(
            waiter.await.expect("join"),
            Err(Error::OAuth("authorization denied"))
        );

        let idle = CallbackServer::bind().await.expect("bind");
        assert_eq!(
            idle.wait_for_code("S", Duration::from_millis(80)).await,
            Err(Error::Timeout)
        );
    }
}
