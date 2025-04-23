/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod connector;
mod streaming_connector;

use std::path::PathBuf;
use std::time::Duration;

pub(crate) use connector::*;
use edenfs_error::ConnectError;
use fbinit::FacebookInit;
pub(crate) use streaming_connector::*;

// TODO: select better defaults (e.g. 1s connection timeout, 1m recv timeout)
const DEFAULT_CONN_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_RECV_TIMEOUT: Duration = Duration::from_secs(300);

/// A trait that defines the common interface for EdenFS connectors.
///
/// Connectors are responsible for creating and managing connections to the EdenFS service.
/// They abstract away the details of how to connect to EdenFS, allowing the client to focus
/// on making requests.
///
/// There are two main implementations of this trait:
/// - `EdenFsConnector`: Creates standard Thrift clients for regular operations
/// - `StreamingEdenFsConnector`: Creates streaming Thrift clients for operations that
///   return large amounts of data or need to stream results incrementally
///
/// # Type Parameters
///
/// * `Client` - The type of Thrift client this connector creates
/// * `ClientFuture` - The future type returned by the connect method
pub trait Connector: Send + Sync {
    /// The type of client this connector creates.
    ///
    /// This is typically an Arc-wrapped Thrift client trait object.
    type Client: Send + Sync + 'static;

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
    /// * `fb` - Facebook initialization context
    /// * `socket_file` - Path to the EdenFS socket file
    ///
    /// # Returns
    ///
    /// Returns a new connector instance.
    fn new(fb: FacebookInit, socket_file: PathBuf) -> Self;

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
