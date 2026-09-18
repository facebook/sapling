/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use async_runtime::block_on;
use configmodel::Config;
use configmodel::ConfigExt;
use configmodel::convert::ByteCount;
use futures::StreamExt;
use futures::stream::BoxStream;
use metrics::Counter;
use parking_lot::Mutex;
use slex::Background;

use crate::CasBatch;
use crate::CasClient;
use crate::CasDigest;
use crate::CasDigestType;

const DEFAULT_MAX_BLOB_SIZE_BYTES: u64 = 1024 * 1024;
const DEFAULT_FAILURE_THRESHOLD: u32 = 3;
const DEFAULT_OPEN_DURATION: Duration = Duration::from_secs(30);

const CAS_WARMUP_HASH: &str = "83276a160cfb6ca8983969c499ee6500dda6ed3a67bb49973fb34186f1e8c05d";
const CAS_WARMUP_SIZE: u64 = 12;

static BREAKER_OPENED: Counter = Counter::new_counter("scmstore.cas.breaker.opened");
static BREAKER_CLOSED: Counter = Counter::new_counter("scmstore.cas.breaker.closed");
static BREAKER_SKIPPED: Counter = Counter::new_counter("scmstore.cas.breaker.skipped");
static FETCH_REJECTED_OVERSIZED: Counter =
    Counter::new_counter("scmstore.cas.fetch.rejected_oversized");

/// A shared CAS client with a circuit breaker protecting remote fetches.
pub struct CasFetchManager {
    client: Arc<dyn CasClient>,
    max_blob_size_bytes: u64,
    breaker: Mutex<CircuitBreaker>,
    warmup_task: OnceLock<Background<()>>,
}

/// Configures a [`CasFetchManager`].
pub struct CasFetchManagerBuilder {
    client: Arc<dyn CasClient>,
    max_blob_size_bytes: u64,
    failure_threshold: u32,
    open_duration: Duration,
}

impl CasFetchManager {
    /// Creates a builder with the default fetch policy.
    pub fn builder(client: Arc<dyn CasClient>) -> CasFetchManagerBuilder {
        CasFetchManagerBuilder {
            client,
            max_blob_size_bytes: DEFAULT_MAX_BLOB_SIZE_BYTES,
            failure_threshold: DEFAULT_FAILURE_THRESHOLD,
            open_duration: DEFAULT_OPEN_DURATION,
        }
    }

    pub(crate) fn from_config(client: Arc<dyn CasClient>, config: &dyn Config) -> Result<Self> {
        let max_blob_size_bytes = config
            .get_or("scmstore", "cas-max-blob-size", || {
                ByteCount::from(DEFAULT_MAX_BLOB_SIZE_BYTES)
            })?
            .value();
        let failure_threshold = config.get_or("scmstore", "cas-failure-threshold", || {
            DEFAULT_FAILURE_THRESHOLD
        })?;
        let open_duration = config.get_or("scmstore", "cas-reenable-probe-interval", || {
            DEFAULT_OPEN_DURATION
        })?;
        Ok(Self::builder(client)
            .max_blob_size_bytes(max_blob_size_bytes)
            .failure_threshold(failure_threshold)
            .open_duration(open_duration)
            .build())
    }

    /// Initializes the underlying client and starts a background warmup.
    pub fn init(&self) -> Result<()> {
        self.client.init()?;
        self.warmup_task.get_or_init(|| {
            let client = Arc::clone(&self.client);
            slex::background("CAS client warmup", move || {
                let start = Instant::now();
                match block_on(warm_cas_client(client.as_ref())) {
                    Ok(()) => tracing::info!(
                        target: "cas_client",
                        duration = ?start.elapsed(),
                        "warmed CAS client"
                    ),
                    Err(error) => tracing::warn!(
                        target: "cas_client",
                        ?error,
                        duration = ?start.elapsed(),
                        "failed to warm CAS client; normal requests will continue"
                    ),
                }
            })
        });
        Ok(())
    }

    /// Returns whether a blob is eligible for CAS fetching.
    pub fn can_fetch_blob(&self, size_bytes: u64) -> bool {
        size_bytes <= self.max_blob_size_bytes
    }

    /// Starts a fetch when every digest is eligible and the circuit breaker permits it.
    ///
    /// Returns `None` if the input is empty, any digest is oversized, or the
    /// circuit breaker is open.
    pub fn fetch<'a>(
        &'a self,
        digests: &'a [CasDigest],
        digest_type: CasDigestType,
    ) -> Option<(CasFetchGuard<'a>, BoxStream<'a, Result<CasBatch>>)> {
        if digests.is_empty() {
            return None;
        }

        if digests
            .iter()
            .any(|digest| !self.can_fetch_blob(digest.size))
        {
            FETCH_REJECTED_OVERSIZED.increment();
            return None;
        }

        let Some(generation) = self.breaker.lock().start(Instant::now()) else {
            BREAKER_SKIPPED.increment();
            return None;
        };
        let guard = CasFetchGuard {
            manager: self,
            generation: Some(generation),
        };
        let batches = self.client.fetch(digests, digest_type);
        Some((guard, batches))
    }
}

impl CasFetchManagerBuilder {
    /// Sets the maximum blob size eligible for CAS fetching.
    pub fn max_blob_size_bytes(mut self, max_blob_size_bytes: u64) -> Self {
        self.max_blob_size_bytes = max_blob_size_bytes;
        self
    }

    /// Sets the number of consecutive failures that opens the circuit breaker.
    pub fn failure_threshold(mut self, failure_threshold: u32) -> Self {
        self.failure_threshold = failure_threshold;
        self
    }

    /// Sets how long the circuit breaker remains open before allowing a probe.
    pub fn open_duration(mut self, open_duration: Duration) -> Self {
        self.open_duration = open_duration;
        self
    }

    /// Builds the manager.
    pub fn build(self) -> CasFetchManager {
        CasFetchManager {
            client: self.client,
            max_blob_size_bytes: self.max_blob_size_bytes,
            breaker: Mutex::new(CircuitBreaker::new(
                self.failure_threshold,
                self.open_duration,
            )),
            warmup_task: OnceLock::new(),
        }
    }
}

async fn warm_cas_client(client: &dyn CasClient) -> Result<()> {
    let digest = CasDigest {
        hash: CAS_WARMUP_HASH
            .parse()
            .context("parsing CAS warmup digest")?,
        size: CAS_WARMUP_SIZE,
    };
    let digests = [digest];
    let mut batches = client.fetch(&digests, CasDigestType::File);

    while let Some(batch) = batches.next().await {
        for (_, result) in batch.context("fetching CAS warmup blob")?.results {
            let _ = result.context("fetching CAS warmup digest")?;
        }
    }

    Ok(())
}

/// Tracks a CAS fetch until its outcome is reported.
#[must_use = "CAS fetch outcomes must be reported with CasFetchGuard::finish"]
pub struct CasFetchGuard<'a> {
    manager: &'a CasFetchManager,
    generation: Option<u64>,
}

impl CasFetchGuard<'_> {
    /// Records the fetch outcome and completes the guard.
    pub fn finish(mut self, outcome: CasFetchOutcome) {
        self.record(outcome);
    }

    fn record(&mut self, outcome: CasFetchOutcome) {
        let Some(generation) = self.generation.take() else {
            return;
        };
        self.manager
            .breaker
            .lock()
            .finish(generation, outcome, Instant::now());
    }
}

impl Drop for CasFetchGuard<'_> {
    fn drop(&mut self) {
        self.record(CasFetchOutcome::Failed);
    }
}

/// The health outcome of a CAS fetch.
#[derive(Clone, Copy)]
pub enum CasFetchOutcome {
    /// CAS completed the request without an operational or validation error.
    Healthy,
    /// CAS failed the request or returned invalid data.
    Failed,
}

struct CircuitBreaker {
    state: CircuitBreakerState,
    failure_threshold: u32,
    open_duration: Duration,
    generation: u64,
}

enum CircuitBreakerState {
    Closed { consecutive_failures: u32 },
    Open { next_probe_at: Instant },
}

impl CircuitBreaker {
    fn new(failure_threshold: u32, open_duration: Duration) -> Self {
        Self {
            state: CircuitBreakerState::Closed {
                consecutive_failures: 0,
            },
            failure_threshold,
            open_duration,
            generation: 0,
        }
    }

    fn start(&mut self, now: Instant) -> Option<u64> {
        let generation = match &mut self.state {
            CircuitBreakerState::Closed { .. } => self.generation,
            CircuitBreakerState::Open { next_probe_at } if now < *next_probe_at => return None,
            CircuitBreakerState::Open { next_probe_at } => {
                self.generation = self.generation.wrapping_add(1);
                *next_probe_at = now + self.open_duration;
                self.generation
            }
        };
        Some(generation)
    }

    fn finish(&mut self, generation: u64, outcome: CasFetchOutcome, now: Instant) {
        if generation != self.generation {
            return;
        }

        match (&mut self.state, outcome) {
            (
                CircuitBreakerState::Closed {
                    consecutive_failures,
                },
                CasFetchOutcome::Healthy,
            ) => *consecutive_failures = 0,
            (
                CircuitBreakerState::Closed {
                    consecutive_failures,
                },
                CasFetchOutcome::Failed,
            ) => {
                *consecutive_failures += 1;
                if *consecutive_failures >= self.failure_threshold {
                    self.open(now);
                }
            }
            (CircuitBreakerState::Open { .. }, CasFetchOutcome::Healthy) => self.close(),
            (CircuitBreakerState::Open { .. }, CasFetchOutcome::Failed) => self.open(now),
        }
    }

    fn open(&mut self, now: Instant) {
        self.generation = self.generation.wrapping_add(1);
        self.state = CircuitBreakerState::Open {
            next_probe_at: now + self.open_duration,
        };
        BREAKER_OPENED.increment();
        tracing::warn!(
            target: "cas_client",
            cooldown = ?self.open_duration,
            "disabling CAS due to errors"
        );
    }

    fn close(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.state = CircuitBreakerState::Closed {
            consecutive_failures: 0,
        };
        BREAKER_CLOSED.increment();
        tracing::info!(target: "cas_client", "re-enabling CAS after a successful probe");
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use types::Blake3;

    use super::*;
    use crate::CasBatch;
    use crate::CasDigest;
    use crate::CasDigestType;

    #[test]
    fn fetch_policy_is_configurable() -> Result<()> {
        let default_config = BTreeMap::<&str, &str>::new();
        let manager =
            CasFetchManager::from_config(Arc::new(CountingClient::default()), &default_config)?;
        assert!(manager.can_fetch_blob(1024 * 1024));
        assert!(!manager.can_fetch_blob(1024 * 1024 + 1));
        assert_eq!(manager.breaker.lock().failure_threshold, 3);
        assert_eq!(
            manager.breaker.lock().open_duration,
            Duration::from_secs(30)
        );

        let configured = BTreeMap::from([
            ("scmstore.cas-max-blob-size", "2M"),
            ("scmstore.cas-failure-threshold", "5"),
            ("scmstore.cas-reenable-probe-interval", "9s"),
        ]);
        let manager =
            CasFetchManager::from_config(Arc::new(CountingClient::default()), &configured)?;
        assert!(manager.can_fetch_blob(2 * 1024 * 1024));
        assert!(!manager.can_fetch_blob(2 * 1024 * 1024 + 1));
        assert_eq!(manager.breaker.lock().failure_threshold, 5);
        assert_eq!(manager.breaker.lock().open_duration, Duration::from_secs(9));

        Ok(())
    }

    #[derive(Default)]
    struct CountingClient {
        fetches: AtomicUsize,
    }

    impl CasClient for CountingClient {
        fn fetch<'a>(
            &'a self,
            _digests: &'a [CasDigest],
            _digest_type: CasDigestType,
        ) -> BoxStream<'a, Result<CasBatch>> {
            self.fetches.fetch_add(1, Ordering::Relaxed);
            Box::pin(futures::stream::empty())
        }
    }

    fn digest(size: u64) -> CasDigest {
        CasDigest {
            hash: Blake3::from([0; 32]),
            size,
        }
    }

    #[test]
    fn builder_uses_defaults_and_overrides() {
        let manager = CasFetchManager::builder(Arc::new(CountingClient::default())).build();
        assert!(manager.can_fetch_blob(1024 * 1024));
        assert!(!manager.can_fetch_blob(1024 * 1024 + 1));
        assert_eq!(manager.breaker.lock().failure_threshold, 3);
        assert_eq!(
            manager.breaker.lock().open_duration,
            Duration::from_secs(30)
        );

        let manager = CasFetchManager::builder(Arc::new(CountingClient::default()))
            .max_blob_size_bytes(7)
            .failure_threshold(5)
            .open_duration(Duration::from_secs(9))
            .build();
        assert!(manager.can_fetch_blob(7));
        assert!(!manager.can_fetch_blob(8));
        assert_eq!(manager.breaker.lock().failure_threshold, 5);
        assert_eq!(manager.breaker.lock().open_duration, Duration::from_secs(9));
    }

    #[test]
    fn init_starts_warmup_once() -> Result<()> {
        let client = Arc::new(CountingClient::default());
        let manager = CasFetchManager::builder(client.clone()).build();

        manager.init()?;
        manager.init()?;
        manager
            .warmup_task
            .get()
            .expect("init should create the warmup task")
            .get();

        assert_eq!(
            client.fetches.load(Ordering::Relaxed),
            1,
            "the shared manager should warm the CAS client only once"
        );
        Ok(())
    }

    #[test]
    fn open_breaker_does_not_start_client_fetch() {
        let client = Arc::new(CountingClient::default());
        let manager = CasFetchManager::builder(client.clone()).build();
        let digests = [digest(1)];

        for _ in 0..DEFAULT_FAILURE_THRESHOLD {
            let (guard, _batches) = manager
                .fetch(&digests, CasDigestType::File)
                .expect("closed breaker should start the fetch");
            guard.finish(CasFetchOutcome::Failed);
        }

        assert!(
            manager.fetch(&digests, CasDigestType::File).is_none(),
            "open breaker should reject the fetch"
        );
        assert_eq!(
            client.fetches.load(Ordering::Relaxed),
            DEFAULT_FAILURE_THRESHOLD as usize,
            "rejected fetch should not reach the client"
        );
    }

    #[test]
    fn abandoned_fetch_is_recorded_as_failed() {
        let manager = CasFetchManager::builder(Arc::new(CountingClient::default()))
            .failure_threshold(1)
            .build();
        let digests = [digest(1)];

        let (guard, _batches) = manager
            .fetch(&digests, CasDigestType::File)
            .expect("closed breaker should start the fetch");
        drop(guard);

        assert!(
            manager.fetch(&digests, CasDigestType::File).is_none(),
            "abandoning a fetch should open the breaker"
        );
    }

    #[test]
    fn oversized_blob_does_not_start_client_fetch() {
        let client = Arc::new(CountingClient::default());
        let manager = CasFetchManager::builder(client.clone())
            .max_blob_size_bytes(7)
            .build();

        assert!(
            manager.fetch(&[digest(8)], CasDigestType::File).is_none(),
            "oversized blob should be rejected"
        );
        assert_eq!(
            client.fetches.load(Ordering::Relaxed),
            0,
            "oversized blob should not reach the client"
        );
    }

    #[test]
    fn empty_fetch_does_not_start_client_fetch() {
        let client = Arc::new(CountingClient::default());
        let manager = CasFetchManager::builder(client.clone()).build();

        assert!(
            manager.fetch(&[], CasDigestType::File).is_none(),
            "empty fetch should be rejected"
        );
        assert_eq!(
            client.fetches.load(Ordering::Relaxed),
            0,
            "empty fetch should not reach the client"
        );
    }

    #[test]
    fn opens_after_consecutive_failures_and_recovers() {
        let start = Instant::now();
        let mut breaker = CircuitBreaker::new(2, Duration::from_secs(10));

        let first = breaker
            .start(start)
            .expect("closed breaker should allow requests");
        breaker.finish(first, CasFetchOutcome::Failed, start);
        assert!(
            breaker.start(start).is_some(),
            "one failure should not open the breaker"
        );

        let second = breaker
            .start(start)
            .expect("closed breaker should allow requests");
        breaker.finish(second, CasFetchOutcome::Failed, start);
        assert!(
            breaker.start(start).is_none(),
            "failure threshold should open the breaker"
        );

        let first_probe_time = start + Duration::from_secs(10);
        let first_probe = breaker
            .start(first_probe_time)
            .expect("expired cooldown should allow one probe");
        assert!(
            breaker.start(first_probe_time).is_none(),
            "open breaker should allow only one probe per cooldown"
        );

        let second_probe_time = first_probe_time + Duration::from_secs(10);
        let second_probe = breaker
            .start(second_probe_time)
            .expect("a lost probe should not block later probes");
        breaker.finish(first_probe, CasFetchOutcome::Healthy, second_probe_time);
        assert!(
            breaker.start(second_probe_time).is_none(),
            "a stale probe must not close the breaker"
        );

        breaker.finish(second_probe, CasFetchOutcome::Healthy, second_probe_time);
        assert!(
            breaker.start(second_probe_time).is_some(),
            "successful probe should close the breaker"
        );
    }

    #[test]
    fn healthy_request_resets_consecutive_failures() {
        let now = Instant::now();
        let mut breaker = CircuitBreaker::new(2, Duration::from_secs(10));

        let failed = breaker
            .start(now)
            .expect("closed breaker should allow requests");
        breaker.finish(failed, CasFetchOutcome::Failed, now);
        let healthy = breaker
            .start(now)
            .expect("closed breaker should allow requests");
        breaker.finish(healthy, CasFetchOutcome::Healthy, now);
        let failed = breaker
            .start(now)
            .expect("closed breaker should allow requests");
        breaker.finish(failed, CasFetchOutcome::Failed, now);

        assert!(
            breaker.start(now).is_some(),
            "a healthy response should reset consecutive failures"
        );
    }
}
