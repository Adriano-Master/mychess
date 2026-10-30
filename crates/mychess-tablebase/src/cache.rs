//! The cache of online answers (spec m8-8, D2565) and the online source that
//! uses it.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, PoisonError};

use mychess_core::Position;

use crate::category::PositionReport;
use crate::error::Result;
use crate::probe::{BoxFuture, OnlineProbe, position_key};

/// How many positions are kept (the same limit as the Scid).
pub const DEFAULT_CAPACITY: usize = 500;

/// A bounded cache from the key of a position to its report; the oldest
/// entry goes first.
#[derive(Debug)]
pub struct ProbeCache {
    entries: HashMap<String, PositionReport>,
    order: VecDeque<String>,
    capacity: usize,
}

impl ProbeCache {
    /// An empty cache that keeps at most `capacity` positions (at least 1).
    #[must_use]
    pub fn new(capacity: usize) -> ProbeCache {
        let capacity = capacity.max(1);
        ProbeCache {
            entries: HashMap::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    /// The report stored under `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&PositionReport> {
        self.entries.get(key)
    }

    /// Stores a report, dropping the oldest one when the cache is full.
    pub fn insert(&mut self, key: String, report: PositionReport) {
        if self.entries.insert(key.clone(), report).is_none() {
            self.order.push_back(key);
        }
        while self.entries.len() > self.capacity {
            match self.order.pop_front() {
                Some(oldest) => {
                    self.entries.remove(&oldest);
                }
                None => break,
            }
        }
    }

    /// Positions stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// An online source with a [`ProbeCache`] in front of it: a position asked
/// twice goes to the network once. Only successful answers are kept.
#[derive(Debug)]
pub struct CachedOnline<P> {
    inner: P,
    cache: Mutex<ProbeCache>,
}

impl<P: OnlineProbe> CachedOnline<P> {
    /// Wraps `inner` with a cache of [`DEFAULT_CAPACITY`] positions.
    #[must_use]
    pub fn new(inner: P) -> CachedOnline<P> {
        CachedOnline::with_capacity(inner, DEFAULT_CAPACITY)
    }

    /// Wraps `inner` with a cache of `capacity` positions.
    #[must_use]
    pub fn with_capacity(inner: P, capacity: usize) -> CachedOnline<P> {
        CachedOnline {
            inner,
            cache: Mutex::new(ProbeCache::new(capacity)),
        }
    }

    /// Positions in the cache.
    #[must_use]
    pub fn cached(&self) -> usize {
        self.cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

impl<P: OnlineProbe> OnlineProbe for CachedOnline<P> {
    fn probe<'a>(&'a self, position: &'a Position) -> BoxFuture<'a, Result<PositionReport>> {
        Box::pin(async move {
            let key = position_key(position);
            let hit = self
                .cache
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&key)
                .cloned();
            if let Some(report) = hit {
                return Ok(report);
            }
            let report = self.inner.probe(position).await?;
            self.cache
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(key, report.clone());
            Ok(report)
        })
    }
}
