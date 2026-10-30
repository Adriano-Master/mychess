//! Error type of the crate (spec m8-3, D1328).
//!
//! Variants carry only data that is safe to show or log: never the token, a
//! request URL with credentials or a raw response body. The GUI translates the
//! variant into a Fluent message; `Display` is English text for the logs.

use std::time::Duration;

use crate::time_control::TimeControlError;

/// Result alias of this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Everything that can go wrong talking to Lichess.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// HTTP 401: missing, expired or revoked token.
    #[error("the access token was rejected (HTTP 401)")]
    Unauthorized,
    /// HTTP 403, or a token without a required OAuth scope.
    #[error("the access token lacks a required scope: {scope}")]
    ScopeMissing {
        /// The missing scope (for example `board:play`), or `unknown`.
        scope: String,
    },
    /// The request was not sent (or got HTTP 429): too many requests.
    #[error("rate limited; retry in {retry_after:?}")]
    RateLimited {
        /// How long new requests stay blocked.
        retry_after: Duration,
    },
    /// Connection, DNS, TLS or I/O failure.
    #[error("network error: {0}")]
    Network(String),
    /// A request or a connection attempt timed out.
    #[error("the request timed out")]
    Timeout,
    /// HTTP 404.
    #[error("not found")]
    NotFound,
    /// HTTP 4xx (other than 401, 403, 404, 429): the server refused the request.
    #[error("rejected by the server (HTTP {status}): {reason}")]
    Rejected {
        /// HTTP status code.
        status: u16,
        /// The `error` field of the response (truncated), possibly empty.
        reason: String,
    },
    /// A retried game action got HTTP 400 (for example "Not your turn"). The
    /// earlier attempt may have reached the server and been applied, so the
    /// caller must reconcile with the game stream (`gameState`/`gameFull`)
    /// instead of undoing the local move.
    #[error("a retried game action was rejected (HTTP {status}): {reason}")]
    AmbiguousAfterRetry {
        /// HTTP status code (400).
        status: u16,
        /// The `error` field of the response (truncated), possibly empty.
        reason: String,
    },
    /// HTTP 5xx.
    #[error("server error (HTTP {0})")]
    Server(u16),
    /// The server answered something this client cannot understand.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// A stream line or a response body exceeded the size limit.
    #[error("a line or body exceeded the size limit")]
    LineTooLong,
    /// The server answered with a redirect; authenticated calls never follow them.
    #[error("unexpected redirect (HTTP {0})")]
    Redirect(u16),
    /// The account is a Bot account, which this client refuses (D1301).
    #[error("bot accounts are not supported")]
    BotAccount,
    /// The configured base URL is not `https` (nor `http` on loopback).
    #[error("insecure or invalid base URL")]
    InsecureUrl,
    /// A game id, user name or token has an invalid shape.
    #[error("invalid {what}")]
    InvalidArgument {
        /// What was invalid (`game id`, `user name`, `token`...).
        what: &'static str,
    },
    /// A time control the Board API does not accept (D1314).
    #[error("invalid time control: {0}")]
    TimeControl(#[from] TimeControlError),
    /// The token storage (keychain or file) failed.
    #[error("token storage error: {0}")]
    TokenStore(String),
    /// The browser login (OAuth + PKCE) did not complete: the user denied it,
    /// the callback was malformed or the random source failed (D3005).
    #[error("oauth login failed: {0}")]
    OAuth(&'static str),
    /// The stream or handle was closed by the caller.
    #[error("closed")]
    Closed,
}

impl Error {
    /// Whether retrying the same request later can reasonably succeed.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Error::RateLimited { .. } | Error::Network(_) | Error::Timeout | Error::Server(_)
        )
    }
}
