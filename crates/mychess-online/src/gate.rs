//! Request gate (spec m8-3, D1308): the Lichess usage policy as code.
//!
//! * one non-stream request at a time (a semaphore of one);
//! * after an HTTP 429, new normal requests are refused without touching the
//!   network for 60 s, doubling on consecutive 429s up to 5 minutes;
//! * urgent requests (moves and other game actions) skip the queue and ignore
//!   the block, because a late move loses the game.
//!
//! Time uses `tokio::time::Instant`, so tests run with a paused clock.

use std::{
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    time::Instant,
};

use crate::error::{Error, Result};

/// Queue class of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// Waits its turn and respects the 429 block.
    Normal,
    /// Game action: skips the queue and ignores the block.
    Urgent,
}

/// Tuning of the block after a 429.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateConfig {
    /// First block (60 s).
    pub base_block: Duration,
    /// Largest block from doubling (5 min).
    pub max_block: Duration,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            base_block: Duration::from_secs(60),
            max_block: Duration::from_secs(300),
        }
    }
}

#[derive(Debug)]
struct GateState {
    blocked_until: Option<Instant>,
    next_block: Duration,
}

/// Serializes requests and applies the 429 policy. Cheap to clone.
#[derive(Debug, Clone)]
pub struct RequestGate {
    cfg: GateConfig,
    sem: Arc<Semaphore>,
    state: Arc<Mutex<GateState>>,
}

/// Holds the "one request at a time" slot until dropped.
#[derive(Debug)]
pub struct GatePermit {
    _slot: Option<OwnedSemaphorePermit>,
}

impl RequestGate {
    /// New gate with the given tuning.
    #[must_use]
    pub fn new(cfg: GateConfig) -> Self {
        Self {
            cfg,
            sem: Arc::new(Semaphore::new(1)),
            state: Arc::new(Mutex::new(GateState {
                blocked_until: None,
                next_block: cfg.base_block,
            })),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, GateState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Time left in the 429 block, if blocked.
    #[must_use]
    pub fn blocked_for(&self) -> Option<Duration> {
        let until = self.state().blocked_until?;
        let left = until.saturating_duration_since(Instant::now());
        (!left.is_zero()).then_some(left)
    }

    /// Takes a slot. Normal requests fail fast with [`Error::RateLimited`]
    /// while blocked and otherwise wait for the slot; urgent ones never wait.
    ///
    /// # Errors
    /// [`Error::RateLimited`] for a normal request during the block.
    pub async fn acquire(&self, priority: Priority) -> Result<GatePermit> {
        if priority == Priority::Urgent {
            return Ok(GatePermit { _slot: None });
        }
        self.check_not_blocked()?;
        let permit = self
            .sem
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Error::Closed)?;
        // A 429 may have arrived while this request waited in the queue.
        self.check_not_blocked()?;
        Ok(GatePermit {
            _slot: Some(permit),
        })
    }

    /// Fails with [`Error::RateLimited`] while the block lasts (used by streams
    /// before (re)connecting).
    ///
    /// # Errors
    /// [`Error::RateLimited`] during the block.
    pub fn check_not_blocked(&self) -> Result<()> {
        match self.blocked_for() {
            Some(retry_after) => Err(Error::RateLimited { retry_after }),
            None => Ok(()),
        }
    }

    /// Records an HTTP status. On 429 starts (or extends) the block and returns
    /// its length; on any other status ends the doubling once the block is over.
    pub fn record_status(&self, status: u16, retry_after: Option<Duration>) -> Option<Duration> {
        let now = Instant::now();
        let mut st = self.state();
        if status == 429 {
            let block = st
                .next_block
                .max(retry_after.unwrap_or_default())
                .min(Duration::from_secs(3600));
            let until = now + block;
            st.blocked_until = Some(st.blocked_until.map_or(until, |old| old.max(until)));
            st.next_block = (st.next_block * 2).min(self.cfg.max_block);
            Some(block)
        } else {
            if st.blocked_until.is_none_or(|u| u <= now) {
                st.blocked_until = None;
                st.next_block = self.cfg.base_block;
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate() -> RequestGate {
        RequestGate::new(GateConfig::default())
    }

    #[tokio::test(start_paused = true)]
    async fn normal_requests_are_serialized() {
        let g = gate();
        let first = g.acquire(Priority::Normal).await.expect("first");
        let g2 = g.clone();
        let second = tokio::spawn(async move { g2.acquire(Priority::Normal).await.map(|_| ()) });
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(!second.is_finished(), "must wait for the first to finish");
        drop(first);
        assert!(second.await.expect("join").is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn urgent_skips_the_queue_and_the_block() {
        let g = gate();
        let _held = g.acquire(Priority::Normal).await.expect("held");
        assert!(g.acquire(Priority::Urgent).await.is_ok());
        g.record_status(429, None);
        assert!(g.acquire(Priority::Urgent).await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn a_429_blocks_for_a_minute_without_waiting() {
        let g = gate();
        assert_eq!(g.record_status(429, None), Some(Duration::from_secs(60)));
        let Err(Error::RateLimited { retry_after }) = g.acquire(Priority::Normal).await else {
            panic!("expected RateLimited");
        };
        assert_eq!(retry_after, Duration::from_secs(60));
        tokio::time::advance(Duration::from_secs(59)).await;
        assert!(g.acquire(Priority::Normal).await.is_err());
        tokio::time::advance(Duration::from_secs(2)).await;
        assert!(g.acquire(Priority::Normal).await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn consecutive_429s_double_up_to_the_cap() {
        let g = gate();
        let mut blocks = Vec::new();
        for _ in 0..5 {
            blocks.push(g.record_status(429, None).map(|d| d.as_secs()));
            tokio::time::advance(Duration::from_secs(301)).await;
        }
        assert_eq!(
            blocks,
            [Some(60), Some(120), Some(240), Some(300), Some(300)]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn success_after_the_block_resets_the_doubling() {
        let g = gate();
        g.record_status(429, None);
        tokio::time::advance(Duration::from_secs(61)).await;
        g.record_status(200, None);
        assert_eq!(g.record_status(429, None), Some(Duration::from_secs(60)));
    }

    #[tokio::test(start_paused = true)]
    async fn retry_after_longer_than_the_default_wins() {
        let g = gate();
        let block = g.record_status(429, Some(Duration::from_secs(90)));
        assert_eq!(block, Some(Duration::from_secs(90)));
        assert_eq!(g.blocked_for(), Some(Duration::from_secs(90)));
    }

    #[tokio::test(start_paused = true)]
    async fn queued_request_fails_if_a_429_arrives_while_waiting() {
        let g = gate();
        let first = g.acquire(Priority::Normal).await.expect("first");
        let g2 = g.clone();
        let queued = tokio::spawn(async move { g2.acquire(Priority::Normal).await.map(|_| ()) });
        tokio::time::sleep(Duration::from_millis(10)).await;
        g.record_status(429, None);
        drop(first);
        assert!(matches!(
            queued.await.expect("join"),
            Err(Error::RateLimited { .. })
        ));
    }
}
