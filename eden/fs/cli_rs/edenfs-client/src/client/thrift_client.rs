/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Debug;
use std::fmt::Display;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use edenfs_error::ConnectAndRequestError;
use edenfs_error::ErrorHandlingStrategy;
use edenfs_error::HasErrorHandlingStrategy;
use edenfs_error::Result;
use fbinit::FacebookInit;
use parking_lot::Mutex;
use tokio::sync::Semaphore;

use crate::client::Client;
use crate::client::EdenFsClientStatsHandler;
use crate::client::EdenFsConnection;
use crate::client::NoopEdenFsClientStatsHandler;
use crate::client::connector::Connector;
use crate::client::connector::StreamingEdenFsConnector;
use crate::client::connector::StreamingEdenFsThriftClientFuture;
use crate::use_case::UseCase;

// Number of attempts to make for a given Thrift request before giving up.
const MAX_RETRY_ATTEMPTS: usize = 3;

/// A client for interacting with the EdenFS Thrift service.
///
/// `ThriftClient` provides methods for communicating with the EdenFS Thrift service, allowing you to
/// perform operations such as querying mount points, checking daemon status, and managing
/// checkouts.
///
/// This is the core client implementation that handles connections, retries, and error handling.
///
/// The client automatically handles:
/// - Connection management and reconnection if EdenFS restarts
/// - Request retries based on error types
/// - Concurrency limiting to prevent overloading the EdenFS server
#[allow(dead_code)]
pub struct ThriftClient {
    use_case: Arc<UseCase>,
    connector: StreamingEdenFsConnector,
    connection: Mutex<EdenFsConnection<StreamingEdenFsThriftClientFuture>>,
    stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>,
    /// Eden has limits on concurrency and will return server overloaded (or timeout) errors if we
    /// send too many. Experimentally, even for large builds (see details in D36136516), we don't
    /// get much performance improvement beyond 2K concurrent requests, regardless of whether Eden
    /// has a fast or slow connection to source control, a warm cache or not, and a lot of CPU
    /// available to run or not.
    semaphore: Semaphore,
}

#[async_trait]
impl Client for ThriftClient {
    fn new(fb: FacebookInit, use_case: Arc<UseCase>, socket_file: PathBuf) -> Self {
        let connector = StreamingEdenFsConnector::new(fb, socket_file.clone());
        let connection = Mutex::new(EdenFsConnection {
            epoch: 0,
            client: connector.connect(None, None),
        });

        Self {
            use_case: use_case.clone(),
            connector,
            connection,
            stats_handler: Box::new(NoopEdenFsClientStatsHandler {}),
            semaphore: Semaphore::new(use_case.max_outstanding_requests()),
        }
    }

    fn set_stats_handler(
        &mut self,
        stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>,
    ) {
        self.stats_handler = stats_handler;
    }

    async fn with_thrift_with_timeouts<F, Fut, T, E>(
        &self,
        conn_timeout: Option<Duration>,
        recv_timeout: Option<Duration>,
        f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&<StreamingEdenFsConnector as Connector>::Client) -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, E>> + Send,
        T: Send,
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
