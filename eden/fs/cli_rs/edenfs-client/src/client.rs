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
use std::sync::Mutex;
use std::time::Duration;

use connector::EdenFsConnector;
use edenfs_error::ConnectAndRequestError;
use edenfs_error::HasErrorHandlingStrategy;
use edenfs_error::Result;
use fbinit::expect_init;

use crate::client::connector::EdenFsThriftClient;
use crate::client::connector::EdenFsThriftClientFuture;
use crate::client::connector::StreamingEdenFsThriftClient;
use crate::client::connector::StreamingEdenFsThriftClientFuture;

/// An EdenFs client and an epoch to keep track of reconnections.
#[derive(Clone, Debug)]
struct EdenFsConnection<T> {
    /// This starts at zero and increments every time we reconnect. We use this to keep track of
    /// whether another client already recycled the connection when we need to reconnect.
    #[allow(dead_code)]
    epoch: usize,
    #[allow(dead_code)]
    client: T,
}

pub struct EdenFsClient {
    connector: EdenFsConnector,
    #[allow(dead_code)]
    connection: Mutex<EdenFsConnection<EdenFsThriftClientFuture>>,
    #[allow(dead_code)]
    streaming_connection: Mutex<EdenFsConnection<StreamingEdenFsThriftClientFuture>>,
}

impl EdenFsClient {
    // TODO: pass in FacebookInit
    pub(crate) fn new(socket_file: PathBuf) -> Self {
        let connector = EdenFsConnector::new(expect_init(), socket_file);
        let connection = Mutex::new(EdenFsConnection {
            epoch: 0,
            client: connector.connect(None, None),
        });
        let streaming_connection = Mutex::new(EdenFsConnection {
            epoch: 0,
            client: connector.connect_streaming(None, None),
        });

        Self {
            connector,
            connection,
            streaming_connection,
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
        self.with_thrift_with_timeouts(None, None, f).await
    }

    pub async fn with_thrift_with_timeouts<F, Fut, T, E>(
        &self,
        conn_timeout: Option<Duration>,
        recv_timeout: Option<Duration>,
        f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&EdenFsThriftClient) -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: HasErrorHandlingStrategy + Debug + Display,
    {
        let client_future = self.connector.connect(conn_timeout, recv_timeout);
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
        self.with_streaming_thrift_with_timeouts(None, None, f)
            .await
    }

    pub async fn with_streaming_thrift_with_timeouts<F, Fut, T, E>(
        &self,
        conn_timeout: Option<Duration>,
        recv_timeout: Option<Duration>,
        f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&StreamingEdenFsThriftClient) -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: HasErrorHandlingStrategy + Debug + Display,
    {
        let client_future = self.connector.connect_streaming(conn_timeout, recv_timeout);
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
}
