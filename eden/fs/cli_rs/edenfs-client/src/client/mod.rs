/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod connector;
mod edenfs;
mod streaming_edenfs;

use std::fmt::Debug;
use std::fmt::Display;
use std::future::Future;
use std::time::Duration;

use connector::*;
pub use edenfs::*;
use edenfs_error::ConnectAndRequestError;
use edenfs_error::ErrorHandlingStrategy;
use edenfs_error::HasErrorHandlingStrategy;
use edenfs_error::Result;
use parking_lot::Mutex;
pub use streaming_edenfs::*;
use tokio::sync::Semaphore;

// This value was selected semi-randomly and should be revisited in the future. Anecdotally, we
// have seen EdenFS struggle with <<< 2048 outstanding requests, but the exact number depends
// on the size/complexity/cost of the outstanding requests.
const DEFAULT_MAX_OUTSTANDING_REQUESTS: usize = 2048;

// Number of attempts to make for a given Thrift request before giving up.
const MAX_RETRY_ATTEMPTS: usize = 3;

/// An EdenFs client and an epoch to keep track of reconnections.
#[derive(Clone, Debug)]
struct EdenFsConnection<T> {
    /// This starts at zero and increments every time we reconnect. We use this to keep track of
    /// whether another client already recycled the connection when we need to reconnect.
    epoch: usize,
    client: T,
}

pub trait EdenFsClientStatsHandler {
    fn on_success(&self, attempts: usize, retries: usize);
}

struct NoopEdenFsClientStatsHandler {}

impl EdenFsClientStatsHandler for NoopEdenFsClientStatsHandler {
    fn on_success(&self, _attempts: usize, _retries: usize) {}
}

/// A generic EdenFS client that can work with any connector type.
pub struct Client<C: Connector> {
    connector: C,
    connection: Mutex<EdenFsConnection<C::ClientFuture>>,
    stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>,
    /// Eden has limits on concurrency and will return server overloaded (or timeout) errors if we
    /// send too many. Experimentally, even for large builds (see details in D36136516), we don't
    /// get much performance improvement beyond 2K concurrent requests, regardless of whether Eden
    /// has a fast or slow connection to source control, a warm cache or not, and a lot of CPU
    /// available to run or not.
    semaphore: Semaphore,
}

impl<C: Connector> Client<C> {
    pub(crate) fn new(connector: C, semaphore: Option<Semaphore>) -> Self {
        let connection = Mutex::new(EdenFsConnection {
            epoch: 0,
            client: connector.connect(None, None),
        });

        Self {
            connector,
            connection,
            stats_handler: Box::new(NoopEdenFsClientStatsHandler {}),
            semaphore: semaphore.unwrap_or(Semaphore::new(DEFAULT_MAX_OUTSTANDING_REQUESTS)),
        }
    }

    pub fn set_stats_handler(
        &mut self,
        stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>,
    ) {
        self.stats_handler = stats_handler;
    }

    pub async fn with_thrift<F, Fut, T, E>(
        &self,
        f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&C::Client) -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: HasErrorHandlingStrategy + Debug + Display,
    {
        self.with_thrift_with_timeouts(None, None, f).await
    }

    pub async fn with_thrift_with_timeouts<F, Fut, T, E>(
        &self,
        conn_timeout: Option<Duration>,
        recv_timeout: Option<Duration>,
        f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&C::Client) -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: HasErrorHandlingStrategy + Debug + Display,
    {
        // Acquire a permit from the semaphore. This will block if we have too many outstanding requests.
        let _permit = self
            .semaphore
            .acquire()
            .await
            .expect("Eden I/O semaphore is never closed");

        let mut connection = (*self.connection.lock()).clone();
        let mut attempts = 0;
        let mut retries = 0;

        loop {
            attempts += 1;

            let result = async {
                let client = connection
                    .client
                    .clone()
                    .await
                    .map_err(|e| ConnectAndRequestError::ConnectionError(e))?;

                f(&client)
                    .await
                    .map_err(|e| ConnectAndRequestError::RequestError(e))
            }
            .await;

            let error = match result {
                Ok(result) => {
                    self.stats_handler.on_success(attempts, retries);
                    break Ok(result);
                }
                Err(e) => e,
            };

            match error.get_error_handling_strategy() {
                ErrorHandlingStrategy::Reconnect => {
                    // Our connection to EdenFS broke.
                    // This typically means Eden restarted. Just reconnect.
                    tracing::info!(
                        "Reconnecting ({}/{} attempts) to EdenFS after: {:#}",
                        attempts,
                        MAX_RETRY_ATTEMPTS,
                        error
                    );
                    let mut guard = self.connection.lock();
                    if guard.epoch == connection.epoch {
                        guard.client = self.connector.connect(conn_timeout, recv_timeout);
                        guard.epoch += 1;
                    }
                    connection = (*guard).clone();
                }
                ErrorHandlingStrategy::Retry => {
                    // Our request failed but needs retrying.
                    retries += 1;
                    tracing::info!(
                        "Retrying ({}/{} attempts) EdenFS request after: {:#}",
                        attempts,
                        MAX_RETRY_ATTEMPTS,
                        error
                    );
                }
                ErrorHandlingStrategy::Abort => {
                    break Err(error);
                }
            };

            if attempts > MAX_RETRY_ATTEMPTS {
                break Err(error);
            }
        }
    }
}
