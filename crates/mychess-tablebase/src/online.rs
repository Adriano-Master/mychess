//! The Lichess online tablebase (spec m8-8, D2564, D2582).
//!
//! `GET https://tablebase.lichess.ovh/standard?fen=...` through the HTTP
//! client of `mychess-online` (https only, no redirects, bounded body, one
//! request at a time, block after a 429), without any token. The answer comes
//! from a stranger on the network: it is read with a bound on size and on the
//! number of moves, every move is checked against the legal moves of the
//! position, and unknown categories become [`Category::Unknown`].

use std::time::Duration;

use mychess_core::Position;
use mychess_online::{Client, ClientConfig};
use serde::Deserialize;

use crate::category::{Category, Evaluation, MoveEntry, PositionReport, Source};
use crate::convert::is_zeroing;
use crate::error::{Result, TablebaseError};
use crate::probe::{BoxFuture, ONLINE_MAX_PIECES, OnlineProbe, check_applicable};

/// The server (the base URL only changes in tests).
pub const LICHESS_TABLEBASE_URL: &str = "https://tablebase.lichess.ovh";
/// Longest answer read.
pub const MAX_BODY_BYTES: usize = 128 * 1024;
/// Most moves read from an answer (a position has at most 218).
pub const MAX_REPLY_MOVES: usize = 256;
/// Timeout of the connection and of the request.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// The Lichess tablebase. Cheap to clone (the client is shared).
#[derive(Debug, Clone)]
pub struct LichessTablebase {
    client: Client,
}

impl LichessTablebase {
    /// The real server.
    ///
    /// # Errors
    /// [`TablebaseError::Network`] if the TLS stack cannot start.
    pub fn new() -> Result<LichessTablebase> {
        LichessTablebase::with_base_url(LICHESS_TABLEBASE_URL)
    }

    /// Another server (tests use a fake one on loopback).
    ///
    /// # Errors
    /// [`TablebaseError::Network`] for a URL that is not `https` (or `http`
    /// on loopback).
    pub fn with_base_url(base_url: &str) -> Result<LichessTablebase> {
        let client = Client::new(ClientConfig {
            base_url: base_url.to_owned(),
            user_agent: format!("MyChess/{}", env!("CARGO_PKG_VERSION")),
            token: None,
            connect_timeout: REQUEST_TIMEOUT,
            request_timeout: REQUEST_TIMEOUT,
            max_body: MAX_BODY_BYTES,
            ..ClientConfig::default()
        })?;
        Ok(LichessTablebase { client })
    }

    /// Uses a client built by the caller (for instance with another gate).
    #[must_use]
    pub fn with_client(client: Client) -> LichessTablebase {
        LichessTablebase { client }
    }
}

impl OnlineProbe for LichessTablebase {
    fn probe<'a>(&'a self, position: &'a Position) -> BoxFuture<'a, Result<PositionReport>> {
        Box::pin(async move {
            check_applicable(position, ONLINE_MAX_PIECES)?;
            let fen = position.fen();
            let body = self
                .client
                .get_public(&["standard"], &[("fen", fen.as_str())])
                .await?;
            parse_reply(position, &body)
        })
    }
}

#[derive(Debug, Deserialize)]
struct Reply {
    #[serde(default)]
    checkmate: bool,
    #[serde(default)]
    stalemate: bool,
    #[serde(default)]
    insufficient_material: bool,
    dtz: Option<i64>,
    precise_dtz: Option<i64>,
    dtm: Option<i64>,
    category: Option<String>,
    #[serde(default)]
    moves: Vec<ReplyMove>,
}

#[derive(Debug, Deserialize)]
struct ReplyMove {
    uci: String,
    #[serde(default)]
    checkmate: bool,
    #[serde(default)]
    stalemate: bool,
    #[serde(default)]
    insufficient_material: bool,
    dtz: Option<i64>,
    precise_dtz: Option<i64>,
    dtm: Option<i64>,
    category: Option<String>,
}

fn small(value: Option<i64>) -> Option<i32> {
    value.and_then(|v| i32::try_from(v).ok())
}

/// The numbers of an answer: `precise_dtz` when the server has it, else the
/// (rounded) `dtz`.
fn numbers(
    category: Option<&str>,
    dtz: Option<i64>,
    precise: Option<i64>,
    dtm: Option<i64>,
) -> Evaluation {
    let precise = small(precise);
    let rounded = small(dtz);
    Evaluation {
        category: category.map_or(Category::Unknown, Category::from_lichess),
        dtz: precise.or(rounded),
        dtz_rounded: precise.is_none() && rounded.is_some(),
        dtm: small(dtm),
    }
}

/// Reads the JSON answer of the server for `position` (D2564).
///
/// The result of each move in the answer is from the point of view of the side
/// that moves **next** (the opponent); here it is turned around so that it is
/// from the point of view of the side that plays the move (D2553).
///
/// # Errors
/// [`TablebaseError::Protocol`] for JSON that is not an answer, too many
/// moves, or a move that is not legal in `position`.
pub fn parse_reply(position: &Position, body: &[u8]) -> Result<PositionReport> {
    let reply: Reply = serde_json::from_slice(body)
        .map_err(|_| TablebaseError::Protocol("the answer is not valid JSON".to_owned()))?;
    if reply.moves.len() > MAX_REPLY_MOVES {
        return Err(TablebaseError::Protocol("too many moves".to_owned()));
    }
    let pieces = u8::try_from(position.pieces().count()).unwrap_or(u8::MAX);
    let root = if reply.checkmate {
        Evaluation {
            dtz: Some(0),
            ..Evaluation::of(Category::Loss)
        }
    } else if reply.stalemate || reply.insufficient_material {
        Evaluation {
            dtz: Some(0),
            ..Evaluation::of(Category::Draw)
        }
    } else {
        numbers(
            reply.category.as_deref(),
            reply.dtz,
            reply.precise_dtz,
            reply.dtm,
        )
    };
    let mut entries = Vec::with_capacity(reply.moves.len());
    for item in reply.moves {
        let mv = position
            .parse_uci(&item.uci)
            .map_err(|_| TablebaseError::Protocol("an illegal move in the answer".to_owned()))?;
        let evaluation = if item.checkmate {
            Evaluation {
                category: Category::Win,
                dtz: Some(1),
                dtz_rounded: false,
                dtm: Some(1),
            }
        } else if item.stalemate || item.insufficient_material {
            Evaluation {
                dtz: Some(0),
                ..Evaluation::of(Category::Draw)
            }
        } else {
            numbers(
                item.category.as_deref(),
                item.dtz,
                item.precise_dtz,
                item.dtm,
            )
            .negate()
        };
        if entries.iter().any(|e: &MoveEntry| e.mv == mv) {
            return Err(TablebaseError::Protocol(
                "a move twice in the answer".to_owned(),
            ));
        }
        let uci = position.uci(mv);
        // The SAN comes from our own rules, never from the server.
        let san = position.san(mv).unwrap_or_else(|_| uci.clone());
        entries.push(MoveEntry {
            mv,
            san,
            uci,
            evaluation,
            zeroing: is_zeroing(mv),
            checkmate: item.checkmate,
            stalemate: item.stalemate,
            insufficient_material: item.insufficient_material,
        });
    }
    // A server that leaves moves out: they are listed as unknown and nothing
    // is recommended (the best move may be one of them).
    let mut complete = true;
    for mv in position.legal_moves() {
        if entries.iter().all(|e| e.mv != mv) {
            complete = false;
            let uci = position.uci(mv);
            let san = position.san(mv).unwrap_or_else(|_| uci.clone());
            entries.push(MoveEntry {
                mv,
                san,
                uci,
                evaluation: Evaluation::UNKNOWN,
                zeroing: is_zeroing(mv),
                checkmate: false,
                stalemate: false,
                insufficient_material: false,
            });
        }
    }
    let report = PositionReport::new(Source::Online, pieces, root, entries);
    Ok(if complete {
        report
    } else {
        report.incomplete()
    })
}
