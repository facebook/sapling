/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use edenfs_error::ConnectError;
use fbinit::FacebookInit;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::Shared;
use thrift_streaming_clients::StreamingEdenService;
use thrift_streaming_thriftclients::make_StreamingEdenServiceExt_thriftclient;

pub type StreamingEdenFsThriftClient = Arc<dyn StreamingEdenService + Send + Sync + 'static>;
pub type StreamingEdenFsThriftClientFuture =
    Shared<BoxFuture<'static, std::result::Result<StreamingEdenFsThriftClient, ConnectError>>>;

// TODO: select better defaults (e.g. 1s connection timeout, 1m recv timeout)
const DEFAULT_CONN_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_RECV_TIMEOUT: Duration = Duration::from_secs(300);

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

pub struct StreamingEdenFsConnector {
    fb: FacebookInit,
    socket_file: PathBuf,
}

impl Connector for StreamingEdenFsConnector {
    type Client = StreamingEdenFsThriftClient;
    type ClientFuture = StreamingEdenFsThriftClientFuture;

    fn new(fb: FacebookInit, socket_file: PathBuf) -> Self {
        Self { fb, socket_file }
    }

    fn connect(
        &self,
        conn_timeout: Option<Duration>,
        recv_timeout: Option<Duration>,
    ) -> Self::ClientFuture {
        let socket_file = self.socket_file.clone();
        let fb = self.fb;

        tokio::task::spawn(async move {
            tracing::info!(
                "Creating a new EdenFS streaming connection via `{}`",
                socket_file.display()
            );

            // get future for the connection
            let client: StreamingEdenFsThriftClient = make_StreamingEdenServiceExt_thriftclient!(
                fb,
                protocol = CompactProtocol,
                from_path = &socket_file,
                with_conn_timeout =
                    conn_timeout.map_or(DEFAULT_CONN_TIMEOUT, |t| t).as_millis() as u32,
                with_recv_timeout =
                    recv_timeout.map_or(DEFAULT_RECV_TIMEOUT, |t| t).as_millis() as u32,
                with_secure = false,
            )
            .with_context(|| "Unable to create an EdenFS streaming thrift client")
            .map_err(|e| ConnectError::ConnectionError(e.to_string()))?;

            Ok(client)
        })
        .map(|r| match r {
            Ok(r) => r,
            Err(e) => Err(ConnectError::ConnectionError(e.to_string())),
        })
        .boxed()
        .shared()
    }
}
