/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Context, Error};
use async_trait::async_trait;
use futures::compat::Future01CompatExt;
use futures::future;
use slog::{info, Logger};
use sql::Connection;
use sql_common::ext::ConnectionExt;
use std::time::Duration;
use tokio::time;

const MAX_ALLOWED_REPLICATION_LAG_SECS: u64 = 5;
const REPLICATION_LAG_POLL_INTERVAL_SECS: u64 = 2;

#[async_trait]
pub trait Laggable {
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

pub async fn wait_for_replication<C: Laggable>(
    logger: &Logger,
    conns: &[(String, C)],
) -> Result<(), Error> {
    loop {
        match get_max_replication_lag(conns).await? {
            Some((region, lag)) => {
                info!(logger, "Max replication lag is {}: {}s", region, lag);
                if lag < MAX_ALLOWED_REPLICATION_LAG_SECS {
                    return Ok(());
                }

                // Wait for a bit before polling again.
                time::delay_for(Duration::from_secs(REPLICATION_LAG_POLL_INTERVAL_SECS)).await;
            }
            None => {
                return Ok(());
            }
        }
    }
}

async fn get_max_replication_lag<'a, C: Laggable>(
    conns: &'a [(String, C)],
) -> Result<Option<(&'a str, u64)>, Error> {
    let futs = conns.iter().map(|(region, conn)| async move {
        let lag = conn
            .get_lag_secs()
            .await
            .with_context(|| format!("While fetching replication lag for {}", region))?
            .ok_or_else(|| {
                format_err!(
                    "Could not fetch db replication lag for {}. Failing to avoid overloading db",
                    region
                )
            })?;

        Result::<_, Error>::Ok((region, lag))
    });

    let lags = future::try_join_all(futs).await?;
    let max = lags
        .into_iter()
        .max_by_key(|(_, lag)| *lag)
        .map(|(r, l)| (r.as_ref(), l));
    Ok(max)
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
            let lag = get_max_replication_lag(conns.as_ref()).await;
            assert_matches!(lag, Err(_));
        })
    }

    #[test]
    fn test_max_lag() {
        async_unit::tokio_unit_test(async move {
            let conns = vec![
                ("c1".to_string(), TestConn { lag: 1 }),
                ("c2".to_string(), TestConn { lag: 2 }),
            ];
            let lag = get_max_replication_lag(conns.as_ref()).await;
            assert_matches!(lag, Ok(Some(("c2", 2))));
        })
    }
}
