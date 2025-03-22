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

/// A client for interacting with the EdenFS service.
pub struct EdenFsClient(Client<EdenFsConnector>);

impl EdenFsClient {
    /// Creates a new EdenFS client.
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
