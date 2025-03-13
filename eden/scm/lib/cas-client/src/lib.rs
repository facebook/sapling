/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use configmodel::Config;
use futures::stream::BoxStream;
pub use types::CasDigest;
pub use types::CasDigestType;
pub use types::CasFetchedStats;

pub struct CasSuccessTrackerConfig {
    pub max_failures: usize,
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
}

impl CasSuccessTracker {
    pub fn new(config: CasSuccessTrackerConfig) -> Self {
        let downtime_on_failure_ms = config.downtime_on_failure.as_millis() as u64;
        Self {
            config,
            failures_since_last_success: AtomicUsize::new(0),
            last_failure_ms: AtomicU64::new(0),
            downtime_on_failure_ms,
        }
    }

    pub fn record_success(&self) -> anyhow::Result<()> {
        self.failures_since_last_success.store(0, Ordering::Relaxed);
        Ok(())
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
            // if it has been too long since the last request, allow the request
            if time_now - last_failure > self.downtime_on_failure_ms {
                return Ok(true);
            }
            // otherwise, don't allow the request
            tracing::warn!(target: "cas", "CAS is unhealthy, should not be used at this time");
            return Ok(false);
        }
        // CAS is considered healthy if it has not failed too many times
        Ok(true)
    }
}

pub fn new(config: Arc<dyn Config>) -> anyhow::Result<Option<Arc<dyn CasClient>>> {
    match factory::call_constructor::<_, Arc<dyn CasClient>>(&config as &dyn Config) {
        Ok(client) => {
            tracing::debug!(target: "cas", "created client");
            Ok(Some(client))
        }
        Err(err) => {
            if factory::is_error_from_constructor(&err) {
                tracing::debug!(target: "cas", ?err, "error creating client");
                Err(err)
            } else {
                tracing::debug!(target: "cas", "no constructors produced a client");
                Ok(None)
            }
        }
    }
}

#[async_trait::async_trait]
#[auto_impl::auto_impl(&, Box, Arc)]
pub trait CasClient: Sync + Send {
    /// Fetch blobs from CAS.
    async fn fetch<'a>(
        &'a self,
        digests: &'a [CasDigest],
        log_name: CasDigestType,
    ) -> BoxStream<
        'a,
        anyhow::Result<(
            CasFetchedStats,
            Vec<(CasDigest, anyhow::Result<Option<Vec<u8>>>)>,
        )>,
    >;

    /// Prefetch blobs into the CAS cache
    /// Returns a stream of (stats, digests_prefetched, digests_not_found) tuples.
    async fn prefetch<'a>(
        &'a self,
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
        std::thread::sleep(Duration::from_secs(2));
        assert!(tracker.allow_request().unwrap());

        tracker.record_failure().unwrap();
        assert!(!tracker.allow_request().unwrap());

        // Test that the tracker allows requests after the downtime has passed
        std::thread::sleep(Duration::from_secs(2));
        assert!(tracker.allow_request().unwrap());

        tracker.record_success().unwrap();
        assert!(tracker.allow_request().unwrap());
    }
}
