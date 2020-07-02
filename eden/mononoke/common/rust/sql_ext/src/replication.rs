/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Context, Error, Result};
use async_trait::async_trait;
use futures::compat::Future01CompatExt;
use futures::future;
use slog::{info, Logger};
use sql::Connection;
use sql_common::ext::ConnectionExt;
use std::{fmt, time::Duration};
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
            time::delay_for(config.poll_interval).await;
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

// ---- Laggable ----

#[async_trait]
pub trait Laggable: Send + Sync {
    async fn get_lag_secs(&self) -> Result<Option<u64>, Error>;
}

#[async_trait]
impl Laggable for Connection {
    async fn get_lag_secs(&self) -> Result<Option<u64>, Error> {
        match self {
            Connection::Sqlite(_) => Ok(Some(0)),
            conn => match conn.show_replica_lag_secs().compat().await {
                Ok(s) => Ok(s),
                Err(e) => match e.downcast_ref::<sql::error::ServerError>() {
                    Some(server_error) => {
                        // 1918 is discovery failed (i.e. there is no server matching the
                        // constraints). This is fine, that means we don't need to monitor it.
                        if server_error.code == 1918 {
                            Ok(Some(0))
                        } else {
                            Err(e)
                        }
                    }
                    None => Err(e),
                },
            },
        }
    }
}

// Note. It is enough to have borrows to Laggable. Using owned Connection for convenience.
pub struct LaggableCollectionMonitor<L: Laggable> {
    laggables: Vec<(String, L)>,
}

impl<L: Laggable> LaggableCollectionMonitor<L> {
    pub fn new(laggables: Vec<(String, L)>) -> Self {
        // Note. An empty collection will result in queries returning no replication lag.
        Self { laggables }
    }
}

#[async_trait]
impl<L: Laggable> ReplicaLagMonitor for LaggableCollectionMonitor<L> {
    async fn get_replica_lag(&self) -> Result<Vec<ReplicaLag>> {
        let futs = self.laggables.iter().map(|(region, conn)| async move {
            let delay = conn
                .get_lag_secs()
                .await
                .with_context(|| format!("While fetching replication lag for {}", region))?
                .ok_or_else(|| {
                    format_err!(
                        "Could not fetch db replication lag for {}. Failing to avoid overloading db",
                        region
                    )
                })?;


            Result::<_, Error>::Ok(ReplicaLag::new(Duration::from_secs(delay), Some(region.to_string())))
        });
        Ok(future::try_join_all(futs).await?.into_iter().collect())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use assert_matches::assert_matches;

    struct TestConn {
        lag: u64,
    }

    #[async_trait]
    impl Laggable for TestConn {
        async fn get_lag_secs(&self) -> Result<Option<u64>, Error> {
            Ok(Some(self.lag))
        }
    }

    struct BrokenTestCon;

    #[async_trait]
    impl Laggable for BrokenTestCon {
        async fn get_lag_secs(&self) -> Result<Option<u64>, Error> {
            Ok(None)
        }
    }

    #[test]
    fn test_max_requires_lag() {
        async_unit::tokio_unit_test(async move {
            let conns = vec![("conn".to_string(), BrokenTestCon)];
            let monitor = LaggableCollectionMonitor::new(conns);
            let lag = monitor.get_max_replica_lag().await;
            assert!(lag.is_err());
        })
    }

    #[test]
    fn test_max_lag() {
        async_unit::tokio_unit_test(async move {
            let conns = vec![
                ("c1".to_string(), TestConn { lag: 1 }),
                ("c2".to_string(), TestConn { lag: 2 }),
            ];
            let monitor = LaggableCollectionMonitor::new(conns);
            let lag = monitor.get_max_replica_lag().await;
            // Linter gets confused here, says that expected is not used.
            let _expected = ReplicaLag::new(Duration::from_secs(2), Some("c2".to_string()));
            assert_matches!(lag, Ok(_expected));
        })
    }
}
