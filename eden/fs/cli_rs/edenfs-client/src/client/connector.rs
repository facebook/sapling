/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use edenfs_error::ConnectError;
use futures::future::BoxFuture;
use futures::future::Shared;
use thrift_streaming_clients::StreamingEdenService;

pub type StreamingEdenFsThriftClient = Arc<dyn StreamingEdenService + Send + Sync + 'static>;
pub type StreamingEdenFsThriftClientFuture =
    Shared<BoxFuture<'static, std::result::Result<StreamingEdenFsThriftClient, ConnectError>>>;

// TODO: select better defaults (e.g. 1s connection timeout, 1m recv timeout)
const DEFAULT_CONN_TIMEOUT: Duration = Duration::from_mins(2);
const DEFAULT_RECV_TIMEOUT: Duration = Duration::from_mins(5);

/// A trait that defines the common interface for EdenFS connectors.
///
/// Connectors are responsible for creating and managing connections to the EdenFS service.
/// They abstract away the details of how to connect to EdenFS, allowing the client to focus
/// on making requests.
///
/// # Type Parameters
///
/// * `Client` - The type of Thrift client this connector creates
/// * `ClientFuture` - The future type returned by the connect method
pub trait Connector: Send + Sync {
    /// The type of client this connector creates.
    ///
    /// This is typically an Arc-wrapped Thrift client trait object.
    type Client: Clone + Send + Sync + 'static;

    /// The future type returned by the connect method.
    ///
    /// This future resolves to either a client instance or a connection error.
    type ClientFuture: Clone
        + std::future::Future<Output = std::result::Result<Self::Client, ConnectError>>
        + Send
        + Sync;

    /// Creates a new connector instance.
    ///
    /// # Parameters
    ///
    /// * `socket_file` - Path to the EdenFS socket file
    ///
    /// # Returns
    ///
    /// Returns a new connector instance.
    fn new(socket_file: PathBuf) -> Self;

    /// Connects to the EdenFS service.
    ///
    /// This method initiates a connection to the EdenFS service and returns a future
    /// that resolves to a Thrift client when the connection is established.
    ///
    /// # Parameters
    ///
    /// * `conn_timeout` - Optional connection timeout
    /// * `recv_timeout` - Optional receive timeout
    ///
    /// # Returns
    ///
    /// Returns a future that resolves to either a client instance or a connection error.
    fn connect(
        &self,
        conn_timeout: Option<Duration>,
        recv_timeout: Option<Duration>,
    ) -> Self::ClientFuture;
}

pub struct StreamingEdenFsConnector {
    socket_file: PathBuf,
}

impl Connector for StreamingEdenFsConnector {
    type Client = StreamingEdenFsThriftClient;
    type ClientFuture = StreamingEdenFsThriftClientFuture;

    fn new(socket_file: PathBuf) -> Self {
        Self { socket_file }
    }

    fn connect(
        &self,
        conn_timeout: Option<Duration>,
        recv_timeout: Option<Duration>,
    ) -> Self::ClientFuture {
        crate::facebook::connect(
            self.socket_file.clone(),
            conn_timeout.unwrap_or(DEFAULT_CONN_TIMEOUT),
            recv_timeout.unwrap_or(DEFAULT_RECV_TIMEOUT),
        )
    }
}
