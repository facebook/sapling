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

use connector::EdenFsConnector;
use edenfs_error::ConnectAndRequestError;
use edenfs_error::HasErrorHandlingStrategy;
use edenfs_error::Result;
use fbinit::expect_init;

use crate::client::connector::EdenFsThriftClient;
use crate::client::connector::StreamingEdenFsThriftClient;

pub struct EdenFsClient {
    connector: EdenFsConnector,
}

impl EdenFsClient {
    pub(crate) fn new(socket_file: PathBuf) -> Self {
        Self {
            connector: EdenFsConnector::new(expect_init(), socket_file),
        }
    }

    pub async fn with_thrift<F, Fut, T, E>(
        &self,
        f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&EdenFsThriftClient) -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: HasErrorHandlingStrategy + Debug + Display,
    {
        let client_future = self.connector.connect(None);
        let client = client_future
            .await
            .clone()
            .map_err(|e| ConnectAndRequestError::ConnectionError(e))?;

        // TODO: switch to buck2 lazy connection
        // TODO: switch to buck2 retry logic
        // TOOD: switch to buck2 error handling
        f(&client)
            .await
            .map_err(|e| ConnectAndRequestError::RequestError(e))
    }

    pub async fn with_streaming_thrift<F, Fut, T, E>(
        &self,
        f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&StreamingEdenFsThriftClient) -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: HasErrorHandlingStrategy + Debug + Display,
    {
        let client_future = self.connector.connect_streaming(None);
        let client = client_future
            .await
            .clone()
            .await
            .map_err(|e| ConnectAndRequestError::ConnectionError(e))?;

        // TODO: switch to buck2 lazy connection
        // TODO: switch to buck2 retry logic
        // TOOD: switch to buck2 error handling
        f(&client)
            .await
            .map_err(|e| ConnectAndRequestError::RequestError(e))
    }
}
