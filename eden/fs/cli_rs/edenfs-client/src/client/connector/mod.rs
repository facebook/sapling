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
pub trait Connector {
    /// The type of client this connector creates.
    type Client: Send + Sync + 'static;

    /// The future type returned by the connect method.
    type ClientFuture: Clone
        + std::future::Future<Output = std::result::Result<Self::Client, ConnectError>>
        + Send;

    /// Creates a new connector instance.
    fn new(fb: FacebookInit, socket_file: PathBuf) -> Self;

    /// Connects to the EdenFS service.
    fn connect(
        &self,
        conn_timeout: Option<Duration>,
        recv_timeout: Option<Duration>,
    ) -> Self::ClientFuture;
}
