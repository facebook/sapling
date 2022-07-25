/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use slog::info;
use slog::Logger;
use std::fmt;
use std::time::Duration;
use tokio::time;

const MAX_ALLOWED_REPLICATION_LAG_SECS: u64 = 5;
const REPLICATION_LAG_POLL_INTERVAL_SECS: u64 = 2;

// Laggable refers to an item that can lag.
// ReplicaLagMonitor can refer to a collection of Laggables or it can refer to services that
// will monitor replication lag.

// ---- ReplicaLagMonitor ----

#[async_trait]
pub trait ReplicaLagMonitor: Send + Sync {
    /// Returns the lag of all replicas configured.
    /// The Primary replica may or may not be returned, that is an implementation detail.
    async fn get_replica_lag(&self) -> Result<Vec<ReplicaLag>>;

    /// Returns the maximum lag of all replicas in cases that replicas are configured and available.
    /// In cases that no replica is configured, the result will be `ReplicaLag::no_delay()`.
    async fn get_max_replica_lag(&self) -> Result<ReplicaLag> {
        let max = self
            .get_replica_lag()
            .await?
            .into_iter()
            .max_by_key(|lag| lag.delay)
            .unwrap_or_else(ReplicaLag::no_delay);
        Ok(max)
    }

    /// Will poll periodically until the all replicas are below the given threshold of delay from
    /// the primary instance.
    async fn wait_for_replication(&self, config: &WaitForReplicationConfig<'_>) -> Result<()> {
        loop {
            let max_lag = self.get_max_replica_lag().await?;
            if let Some(logger) = config.logger {
                info!(logger, "{}", max_lag);
            }
            if max_lag.delay < config.max_replication_lag_allowed {
                return Ok(());
            }
            // Wait before polling again.
            time::sleep(config.poll_interval).await;
        }
    }
}

pub struct NoReplicaLagMonitor();

#[async_trait]
impl ReplicaLagMonitor for NoReplicaLagMonitor {
    async fn get_replica_lag(&self) -> Result<Vec<ReplicaLag>> {
        Ok(vec![])
    }
}

pub struct ReplicaLag {
    pub delay: Duration,
    pub details: Option<String>,
}

impl ReplicaLag {
    pub fn new(delay: Duration, details: Option<String>) -> Self {
        ReplicaLag { delay, details }
    }

    pub fn no_delay() -> Self {
        ReplicaLag {
            delay: Duration::new(0, 0),
            details: None,
        }
    }
}

impl fmt::Display for ReplicaLag {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Replication lag is {}.{:03}s.",
            self.delay.as_secs(),
            self.delay.subsec_millis(),
        )?;
        for details in self.details.iter() {
            write!(f, " {}", details)?;
        }
        Ok(())
    }
}

impl fmt::Debug for ReplicaLag {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

pub struct WaitForReplicationConfig<'a> {
    logger: Option<&'a Logger>,
    max_replication_lag_allowed: Duration,
    poll_interval: Duration,
}

impl<'a> Default for WaitForReplicationConfig<'a> {
    fn default() -> Self {
        WaitForReplicationConfig {
            logger: None,
            max_replication_lag_allowed: Duration::from_secs(MAX_ALLOWED_REPLICATION_LAG_SECS),
            poll_interval: Duration::from_secs(REPLICATION_LAG_POLL_INTERVAL_SECS),
        }
    }
}

impl<'a> WaitForReplicationConfig<'a> {
    pub fn with_logger(mut self, logger: &'a Logger) -> Self {
        self.logger = Some(logger);
        self
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use assert_matches::assert_matches;

    struct TestMonitor(u64);

    #[async_trait]
    impl ReplicaLagMonitor for TestMonitor {
        async fn get_replica_lag(&self) -> Result<Vec<ReplicaLag>> {
            Ok((1..self.0)
                .map(|lag| ReplicaLag::new(Duration::from_secs(lag), Some(format!("{}", lag))))
                .collect())
        }
    }

    #[tokio::test]
    async fn test_no_replica_lag_monitor() {
        let monitor = NoReplicaLagMonitor();
        let lag = monitor.get_max_replica_lag().await;
        // Linter gets confused here, says that expected is not used.
        let _expected = ReplicaLag::new(Duration::from_secs(0), None);
        assert_matches!(lag, Ok(_expected));
    }

    #[tokio::test]
    async fn test_max_lag() {
        let monitor = TestMonitor(5);
        let lag = monitor.get_max_replica_lag().await;
        // Linter gets confused here, says that expected is not used.
        let _expected = ReplicaLag::new(Duration::from_secs(5), Some("5".to_string()));
        assert_matches!(lag, Ok(_expected));
    }
}
