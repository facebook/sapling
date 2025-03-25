/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use fbinit::FacebookInit;
use tokio::sync::Semaphore;

use crate::client::Client;
use crate::client::Connector;
use crate::client::EdenFsConnector;

/// A client for interacting with the EdenFS daemon.
///
/// `EdenFsClient` provides methods for communicating with the EdenFS daemon, allowing you to
/// perform operations such as querying mount points, checking daemon status, and managing
/// checkouts.
///
/// This client uses the standard Thrift protocol for communication. For streaming operations,
/// use [`StreamingEdenFsClient`](crate::client::StreamingEdenFsClient) instead.
pub struct EdenFsClient(Client<EdenFsConnector>);

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
    pub(crate) fn new(
        fb: FacebookInit,
        socket_file: PathBuf,
        semaphore: Option<Semaphore>,
    ) -> Self {
        let connector = EdenFsConnector::new(fb, socket_file);
        Self(Client::new(connector, semaphore))
    }
}

// Forward all methods to the inner GenericEdenFsClient
impl std::ops::Deref for EdenFsClient {
    type Target = Client<EdenFsConnector>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for EdenFsClient {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
