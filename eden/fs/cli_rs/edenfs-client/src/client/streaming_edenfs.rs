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
use crate::client::StreamingEdenFsConnector;

/// A client for interacting with the EdenFS service using streaming Thrift.
///
/// `StreamingEdenFsClient` provides methods for communicating with the EdenFS daemon using
/// streaming Thrift, which is particularly useful for operations that return large amounts
/// of data or need to stream results incrementally.
///
/// This client is specialized for streaming operations. For standard operations,
/// use [`EdenFsClient`](crate::client::EdenFsClient) instead.
pub struct StreamingEdenFsClient(Client<StreamingEdenFsConnector>);

impl StreamingEdenFsClient {
    /// Creates a new streaming EdenFS client.
    ///
    /// This constructor creates a new streaming client that connects to the EdenFS daemon using the
    /// specified socket file path. It's typically not called directly; instead, use
    /// [`EdenFsInstance::get_streaming_client`](crate::instance::EdenFsInstance::get_streaming_client).
    ///
    /// # Parameters
    ///
    /// * `fb` - Facebook initialization context
    /// * `socket_file` - Path to the EdenFS socket file
    /// * `semaphore` - Optional semaphore to limit concurrent requests
    ///
    /// # Returns
    ///
    /// Returns a new `StreamingEdenFsClient` instance.
    pub(crate) fn new(
        fb: FacebookInit,
        socket_file: PathBuf,
        semaphore: Option<Semaphore>,
    ) -> Self {
        let connector = StreamingEdenFsConnector::new(fb, socket_file);
        Self(Client::new(connector, semaphore))
    }
}

// Forward all methods to the inner GenericEdenFsClient
impl std::ops::Deref for StreamingEdenFsClient {
    type Target = Client<StreamingEdenFsConnector>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for StreamingEdenFsClient {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
