/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use blob::Blob;
use configmodel::Config;
use futures::stream::BoxStream;
pub use types::CasDigest;
pub use types::CasDigestType;
pub use types::CasFetchedStats;
pub use types::FetchContext;

pub struct CasSuccessTrackerConfig {
    // number of failures before the CAS is considered unhealthy
    pub max_failures: usize,
    // how long to wait before allowing requests again after a failure
    // this is used as initial downtime, and then it is exponentially increased if the request fails again
    pub downtime_on_failure: Duration,
}

pub struct CasSuccessTracker {
    pub config: CasSuccessTrackerConfig,
    // number of failures since last success
    pub failures_since_last_success: AtomicUsize,
    // timestamp of the last failure
    // number of ms since the Unix epoch
    pub last_failure_ms: AtomicU64,
    pub downtime_on_failure_ms: u64,
    // number of times the downtime has been lifted on sequential failures
    // used to calculate exponential backoff
    // the counter is reset on success
    pub number_of_downtimes: AtomicUsize,
}

impl CasSuccessTracker {
    pub fn new(config: CasSuccessTrackerConfig) -> Self {
        let downtime_on_failure_ms = config.downtime_on_failure.as_millis() as u64;
        Self {
            config,
            failures_since_last_success: AtomicUsize::new(0),
            last_failure_ms: AtomicU64::new(0),
            downtime_on_failure_ms,
            number_of_downtimes: AtomicUsize::new(0),
        }
    }

    pub fn record_success(&self) {
        self.failures_since_last_success.store(0, Ordering::Relaxed);
        self.number_of_downtimes.store(0, Ordering::Relaxed);
    }

    pub fn record_failure(&self) -> anyhow::Result<()> {
        self.failures_since_last_success
            .fetch_add(1, Ordering::Relaxed);
        Ok(self.last_failure_ms.store(
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64,
            Ordering::Relaxed,
        ))
    }

    pub fn allow_request(&self) -> anyhow::Result<bool> {
        let failures = self.failures_since_last_success.load(Ordering::Relaxed);
        if failures >= self.config.max_failures {
            let last_failure = self.last_failure_ms.load(Ordering::Relaxed);
            let time_now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
            let number_of_downtimes = self.number_of_downtimes.load(Ordering::Relaxed);
            // exponential backoff coefficient
            let expn_backoff_coefficient = std::cmp::min(1 << number_of_downtimes, 16);
            // the request is allowed if the downtime has expired with exponential backoff (capped)
            // the downtime would be:
            // 1 * downtime_on_failure_ms, 2 * downtime_on_failure_ms, 4 * downtime_on_failure_ms
            // 8 * downtime_on_failure_ms, 16 * downtime_on_failure_ms (this will be the max)
            //
            // if it has been too long since the last request was allowed, allow the request now!
            if time_now - last_failure >= self.downtime_on_failure_ms * expn_backoff_coefficient {
                self.number_of_downtimes.fetch_add(1, Ordering::Relaxed);
                // reset the counter, because we would like to allow at least max_failures before
                // we start to apply the downtime again
                self.failures_since_last_success.store(0, Ordering::Relaxed);
                return Ok(true);
            }
            // otherwise, don't allow the request
            tracing::warn!(target: "cas_client", "CAS is unhealthy, should not be used at this time");
            return Ok(false);
        }
        // CAS is considered healthy if it has not failed too many times
        Ok(true)
    }
}

pub fn new(config: Arc<dyn Config>) -> anyhow::Result<Option<Arc<dyn CasClient>>> {
    match factory::call_constructor::<_, Arc<dyn CasClient>>(&config as &dyn Config) {
        Ok(client) => {
            tracing::debug!(target: "cas_client", "created client");
            Ok(Some(client))
        }
        Err(err) => {
            if factory::is_error_from_constructor(&err) {
                tracing::debug!(target: "cas_client", ?err, "error creating client");
                Err(err)
            } else {
                tracing::debug!(target: "cas_client", "no constructors produced a client");
                Ok(None)
            }
        }
    }
}

#[async_trait::async_trait]
#[auto_impl::auto_impl(&, Box, Arc)]
pub trait CasClient: Sync + Send {
    /// Fetch a single blob from local CAS caches.
    fn fetch_single_locally_cached(
        &self,
        digest: &CasDigest,
    ) -> anyhow::Result<(CasFetchedStats, Option<Blob>)>;

    /// Fetch blobs from CAS.
    async fn fetch<'a>(
        &'a self,
        _fctx: FetchContext,
        digests: &'a [CasDigest],
        log_name: CasDigestType,
    ) -> BoxStream<
        'a,
        anyhow::Result<(
            CasFetchedStats,
            Vec<(CasDigest, anyhow::Result<Option<Blob>>)>,
        )>,
    >;

    /// Upload blobs to CAS.
    async fn upload(&self, blobs: Vec<Blob>) -> anyhow::Result<Vec<CasDigest>>;

    /// Prefetch blobs into the CAS local caches.
    /// Returns a stream of (stats, digests_prefetched, digests_not_found) tuples.
    async fn prefetch<'a>(
        &'a self,
        _fctx: FetchContext,
        digests: &'a [CasDigest],
        log_name: CasDigestType,
    ) -> BoxStream<'a, anyhow::Result<(CasFetchedStats, Vec<CasDigest>, Vec<CasDigest>)>>;
}

#[cfg(test)]
mod tests {
    use crate::*;
    #[test]
    fn test_success_tracker() {
        let config = CasSuccessTrackerConfig {
            max_failures: 3,
            downtime_on_failure: Duration::from_secs(1),
        };
        let tracker = CasSuccessTracker::new(config);

        // Test that the tracker allows requests when it's healthy
        assert!(tracker.allow_request().unwrap());

        // Test that the tracker doesn't allow requests when it's not healthy
        for _ in 0..3 {
            tracker.record_failure().unwrap();
        }
        assert!(!tracker.allow_request().unwrap());

        // Test that the tracker allows requests after the downtime has passed
        std::thread::sleep(Duration::from_secs(1));
        assert!(tracker.allow_request().unwrap());

        for _ in 0..3 {
            tracker.record_failure().unwrap();
        }
        assert!(!tracker.allow_request().unwrap());

        // Test that the tracker does not allow requests after the downtime has passed again (from the last failure)
        std::thread::sleep(Duration::from_secs(1));
        assert!(!tracker.allow_request().unwrap());

        // Test that the tracker does allow requests after 2 times the downtime has passed (1+1 seconds)
        std::thread::sleep(Duration::from_secs(1));
        assert!(tracker.allow_request().unwrap());

        tracker.record_success();
        assert!(tracker.allow_request().unwrap());

        for _ in 0..3 {
            tracker.record_failure().unwrap();
        }
        assert!(!tracker.allow_request().unwrap());

        // Test that the tracker allows requests after there was a success after a failure
        tracker.record_success();
        assert!(tracker.allow_request().unwrap());
    }

    #[test]
    fn test_success_tracker_exponential_backoff() {
        let config = CasSuccessTrackerConfig {
            max_failures: 1,
            downtime_on_failure: Duration::from_secs(1),
        };
        let tracker = CasSuccessTracker::new(config);
        tracker.record_failure().unwrap();
        for i in [1, 2, 4, 8] {
            std::thread::sleep(Duration::from_secs(i - 1));
            assert!(!tracker.allow_request().unwrap()); // exponential backoff is not yet lifted
            std::thread::sleep(Duration::from_secs(1));
            assert!(tracker.allow_request().unwrap()); // exponential backoff is lifted
            tracker.record_failure().unwrap();
        }
    }
}
