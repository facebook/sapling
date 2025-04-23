/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod connector;

use std::fmt::Debug;
use std::fmt::Display;
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use edenfs_error::ConnectAndRequestError;
use edenfs_error::ErrorHandlingStrategy;
use edenfs_error::HasErrorHandlingStrategy;
use edenfs_error::Result;
use fbinit::FacebookInit;
use parking_lot::Mutex;
use tokio::sync::Semaphore;

use crate::client::connector::Connector;
use crate::client::connector::EdenFsConnector;
use crate::client::connector::EdenFsThriftClientFuture;
use crate::client::connector::StreamingEdenFsConnector;
use crate::client::connector::StreamingEdenFsThriftClientFuture;

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

/// A trait for handling statistics about EdenFS client requests.
///
/// Implementations of this trait can be used to collect metrics about EdenFS client
/// requests, such as the number of attempts and retries required for successful requests.
pub trait EdenFsClientStatsHandler {
    /// Called when a request completes successfully.
    ///
    /// # Parameters
    ///
    /// * `attempts` - The total number of attempts made for this request
    /// * `retries` - The number of retries (not including the initial attempt)
    fn on_success(&self, attempts: usize, retries: usize);
}

struct NoopEdenFsClientStatsHandler {}

impl EdenFsClientStatsHandler for NoopEdenFsClientStatsHandler {
    fn on_success(&self, _attempts: usize, _retries: usize) {}
}

/// A client for interacting with the EdenFS daemon.
///
/// `EdenFsClient` provides methods for communicating with the EdenFS daemon, allowing you to
/// perform operations such as querying mount points, checking daemon status, and managing
/// checkouts.
///
/// This is the core client implementation that handles connections, retries, and error handling.
///
/// The client automatically handles:
/// - Connection management and reconnection if EdenFS restarts
/// - Request retries based on error types
/// - Concurrency limiting to prevent overloading the EdenFS server
pub struct EdenFsClient {
    connector: EdenFsConnector,
    connection: Mutex<EdenFsConnection<EdenFsThriftClientFuture>>,
    streaming_connector: StreamingEdenFsConnector,
    streaming_connection: Mutex<EdenFsConnection<StreamingEdenFsThriftClientFuture>>,
    stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>,
    /// Eden has limits on concurrency and will return server overloaded (or timeout) errors if we
    /// send too many. Experimentally, even for large builds (see details in D36136516), we don't
    /// get much performance improvement beyond 2K concurrent requests, regardless of whether Eden
    /// has a fast or slow connection to source control, a warm cache or not, and a lot of CPU
    /// available to run or not.
    semaphore: Semaphore,
}

impl EdenFsClient {
    /// Creates a new EdenFS client.
    ///
    /// This constructor creates a new client that connects to the EdenFS daemon using the
    /// specified socket file path. It's typically not called directly; instead, use
    /// [`EdenFsInstance::get_client`](crate::instance::EdenFsInstance::get_client) if
    /// using the globally initialized `EdenFsInstance`.
    ///
    /// # Parameters
    ///
    /// * `fb` - Facebook initialization context
    /// * `socket_file` - Path to the EdenFS socket file
    /// * `semaphore` - Optional semaphore to limit concurrent requests
    ///
    /// # Returns
    ///
    /// Returns a new `EdenFsClient` instance.
    pub fn new(fb: FacebookInit, socket_file: PathBuf, semaphore: Option<Semaphore>) -> Self {
        let connector = EdenFsConnector::new(fb, socket_file.clone());
        let connection = Mutex::new(EdenFsConnection {
            epoch: 0,
            client: connector.connect(None, None),
        });

        let streaming_connector = StreamingEdenFsConnector::new(fb, socket_file);
        let streaming_connection = Mutex::new(EdenFsConnection {
            epoch: 0,
            client: streaming_connector.connect(None, None),
        });

        Self {
            connector,
            connection,
            streaming_connector,
            streaming_connection,
            stats_handler: Box::new(NoopEdenFsClientStatsHandler {}),
            semaphore: semaphore.unwrap_or(Semaphore::new(DEFAULT_MAX_OUTSTANDING_REQUESTS)),
        }
    }

    /// Sets a custom stats handler for the client.
    ///
    /// The stats handler receives notifications about successful requests,
    /// including the number of attempts and retries.
    ///
    /// # Parameters
    ///
    /// * `stats_handler` - The stats handler to use
    pub fn set_stats_handler(
        &mut self,
        stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>,
    ) {
        self.stats_handler = stats_handler;
    }
}

#[async_trait]
pub trait Client: Send + Sync {
    /// Executes a Thrift request with automatic connection management and retries.
    ///
    /// This method handles connecting to the EdenFS service, executing the request,
    /// and automatically retrying or reconnecting if necessary based on the error type.
    ///
    /// # Parameters
    ///
    /// * `f` - A function that takes a Thrift client and returns a future that resolves
    ///   to a result
    ///
    /// # Returns
    ///
    /// Returns a result containing the response if successful, or an error if the request
    /// failed after all retry attempts.
    async fn with_thrift<F, Fut, T, E>(
        &self,
        f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&<EdenFsConnector as Connector>::Client) -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, E>> + Send,
        T: Send,
        E: HasErrorHandlingStrategy + Debug + Display,
    {
        self.with_thrift_with_timeouts(None, None, f).await
    }

    /// Executes a Thrift request with custom timeouts.
    ///
    /// This method is similar to [`with_thrift`](Self::with_thrift), but allows
    /// specifying custom connection and receive timeouts.
    ///
    /// # Parameters
    ///
    /// * `conn_timeout` - Optional connection timeout
    /// * `recv_timeout` - Optional receive timeout
    /// * `f` - A function that takes a Thrift client and returns a future that resolves
    ///   to a result
    ///
    /// # Returns
    ///
    /// Returns a result containing the response if successful, or an error if the request
    /// failed after all retry attempts.
    async fn with_thrift_with_timeouts<F, Fut, T, E>(
        &self,
        conn_timeout: Option<Duration>,
        recv_timeout: Option<Duration>,
        f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&<EdenFsConnector as Connector>::Client) -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, E>> + Send,
        T: Send,
        E: HasErrorHandlingStrategy + Debug + Display;

    /// Executes a streaming Thrift request with automatic connection management and retries.
    ///
    /// This method handles connecting to the EdenFS service, executing the request,
    /// and automatically retrying or reconnecting if necessary based on the error type.
    ///
    /// # Parameters
    ///
    /// * `f` - A function that takes a streaming Thrift client and returns a future that resolves
    ///   to a result
    ///
    /// # Returns
    ///
    /// Returns a result containing the response if successful, or an error if the request
    /// failed after all retry attempts.
    async fn with_streaming_thrift<F, Fut, T, E>(
        &self,
        f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&<StreamingEdenFsConnector as Connector>::Client) -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, E>> + Send,
        E: HasErrorHandlingStrategy + Debug + Display,
    {
        self.with_streaming_thrift_with_timeouts(None, None, f)
            .await
    }

    /// Executes a streaming Thrift request with custom timeouts.
    ///
    /// This method is similar to [`with_streaming_thrift`](Self::with_streaming_thrift), but allows
    /// specifying custom connection and receive timeouts.
    ///
    /// # Parameters
    ///
    /// * `conn_timeout` - Optional connection timeout
    /// * `recv_timeout` - Optional receive timeout
    /// * `f` - A function that takes a streanubg Thrift client and returns a future that resolves
    ///   to a result
    ///
    /// # Returns
    ///
    /// Returns a result containing the response if successful, or an error if the request
    /// failed after all retry attempts.
    async fn with_streaming_thrift_with_timeouts<F, Fut, T, E>(
        &self,
        conn_timeout: Option<Duration>,
        recv_timeout: Option<Duration>,
        f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&<StreamingEdenFsConnector as Connector>::Client) -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, E>> + Send,
        E: HasErrorHandlingStrategy + Debug + Display;
}

#[async_trait]
impl Client for EdenFsClient {
    async fn with_thrift_with_timeouts<F, Fut, T, E>(
        &self,
        conn_timeout: Option<Duration>,
        recv_timeout: Option<Duration>,
        f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&<EdenFsConnector as Connector>::Client) -> Fut + Send + Sync,
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

    async fn with_streaming_thrift_with_timeouts<F, Fut, T, E>(
        &self,
        conn_timeout: Option<Duration>,
        recv_timeout: Option<Duration>,
        f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&<StreamingEdenFsConnector as Connector>::Client) -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, E>> + Send,
        E: HasErrorHandlingStrategy + Debug + Display,
    {
        // Acquire a permit from the semaphore. This will block if we have too many outstanding requests.
        let _permit = self
            .semaphore
            .acquire()
            .await
            .expect("Eden I/O semaphore is never closed");

        let mut streaming_connection = (*self.streaming_connection.lock()).clone();
        let mut attempts = 0;
        let mut retries = 0;

        loop {
            attempts += 1;

            let result = async {
                let client = streaming_connection
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
                    // Our streaming_connection to EdenFS broke.
                    // This typically means Eden restarted. Just reconnect.
                    tracing::info!(
                        "Reconnecting ({}/{} attempts) to EdenFS after: {:#}",
                        attempts,
                        MAX_RETRY_ATTEMPTS,
                        error
                    );
                    let mut guard = self.streaming_connection.lock();
                    if guard.epoch == streaming_connection.epoch {
                        guard.client = self.streaming_connector.connect(conn_timeout, recv_timeout);
                        guard.epoch += 1;
                    }
                    streaming_connection = (*guard).clone();
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
