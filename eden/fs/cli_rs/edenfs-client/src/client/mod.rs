/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod connector;
pub mod mock_client;
pub mod mock_service;
pub mod thrift_client;

use std::fmt::Debug;
use std::fmt::Display;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use edenfs_error::ConnectAndRequestError;
use edenfs_error::HasErrorHandlingStrategy;
use edenfs_error::Result;
use fbinit::FacebookInit;

use crate::client::connector::Connector;
use crate::client::connector::StreamingEdenFsConnector;
#[cfg(not(test))]
use crate::client::thrift_client::ThriftClient;
use crate::methods::EdenThriftMethod;
use crate::use_case::UseCase;
#[cfg(test)]
pub type ThriftClient = crate::client::mock_client::MockThriftClient;

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

#[async_trait]
pub trait Client: Send + Sync {
    /// Creates a new Client instance.
    ///
    /// # Parameters
    ///
    /// * `fb` - Facebook initialization context
    /// * `use_case` - Use case configuration settings
    /// * `socket_file` - Path to the EdenFS socket file
    ///
    /// # Returns
    ///
    /// Returns a new `Client` instance.
    fn new(fb: FacebookInit, use_case: Arc<UseCase>, socket_file: PathBuf) -> Self;

    /// Sets a custom stats handler for the client.
    ///
    /// The stats handler receives notifications about successful requests,
    /// including the number of attempts and retries.
    ///
    /// # Parameters
    ///
    /// * `stats_handler` - The stats handler to use
    fn set_stats_handler(&mut self, stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>);

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
        F: Fn(&<StreamingEdenFsConnector as Connector>::Client) -> (Fut, EdenThriftMethod)
            + Send
            + Sync,
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
        F: Fn(&<StreamingEdenFsConnector as Connector>::Client) -> (Fut, EdenThriftMethod)
            + Send
            + Sync,
        Fut: Future<Output = Result<T, E>> + Send,
        T: Send,
        E: HasErrorHandlingStrategy + Debug + Display;
}

/// A client for interacting with the EdenFS daemon.
///
/// `EdenFsClient` provides methods for communicating with the EdenFS daemon, allowing you to
/// perform operations such as querying mount points, checking daemon status, and managing
/// checkouts.
pub struct EdenFsClient(ThriftClient);

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
    ///
    /// * `use_case_id` - A unique identifier for a use case - used to access configuration settings and attribute usage to a given use case.
    /// * `socket_file` - Path to the EdenFS socket file
    /// # Returns
    ///
    /// Returns a new `EdenFsClient` instance.
    pub(crate) fn new(fb: FacebookInit, use_case: Arc<UseCase>, socket_file: PathBuf) -> Self {
        Self(ThriftClient::new(fb, use_case, socket_file))
    }
}

// Forward all methods to the inner Client
impl std::ops::Deref for EdenFsClient {
    type Target = ThriftClient;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for EdenFsClient {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
