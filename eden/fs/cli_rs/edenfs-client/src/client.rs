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
use std::time::Duration;

use connector::EdenFsConnector;
use edenfs_error::ConnectAndRequestError;
use edenfs_error::ErrorHandlingStrategy;
use edenfs_error::HasErrorHandlingStrategy;
use edenfs_error::Result;
use fbinit::FacebookInit;
use parking_lot::Mutex;

use crate::client::connector::EdenFsThriftClient;
use crate::client::connector::EdenFsThriftClientFuture;
use crate::client::connector::StreamingEdenFsThriftClient;
use crate::client::connector::StreamingEdenFsThriftClientFuture;

const MAX_RETRY_ATTEMPTS: usize = 3;

/// An EdenFs client and an epoch to keep track of reconnections.
#[derive(Clone, Debug)]
struct EdenFsConnection<T> {
    /// This starts at zero and increments every time we reconnect. We use this to keep track of
    /// whether another client already recycled the connection when we need to reconnect.
    epoch: usize,
    client: T,
}

pub trait EdenFsClientStatsHandler {
    fn on_success(&self, attempts: usize, retries: usize);
}

struct NoopEdenFsClientStatsHandler {}

impl EdenFsClientStatsHandler for NoopEdenFsClientStatsHandler {
    fn on_success(&self, _attempts: usize, _retries: usize) {}
}

pub struct EdenFsClient {
    connector: EdenFsConnector,
    connection: Mutex<EdenFsConnection<EdenFsThriftClientFuture>>,
    streaming_connection: Mutex<EdenFsConnection<StreamingEdenFsThriftClientFuture>>,
    stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>,
    streaming_stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>,
}

impl EdenFsClient {
    pub(crate) fn new(fb: FacebookInit, socket_file: PathBuf) -> Self {
        let connector = EdenFsConnector::new(fb, socket_file);
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
            stats_handler: Box::new(NoopEdenFsClientStatsHandler {}),
            streaming_stats_handler: Box::new(NoopEdenFsClientStatsHandler {}),
        }
    }

    pub fn set_stats_handler(
        &mut self,
        stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>,
    ) {
        self.stats_handler = stats_handler;
    }

    pub fn set_streaming_stats_handler(
        &mut self,
        stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>,
    ) {
        self.streaming_stats_handler = stats_handler;
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
        let mut connection = (*self.connection.lock()).clone();
        let mut attempts = 0;
        let mut retries = 0;

        loop {
            attempts += 1;

            let result = async {
                let client = connection
                    .client
                    .clone()
                    .await
                    .map_err(|e| ConnectAndRequestError::ConnectionError(e))?;

                f(&client)
                    .await
                    .map_err(|e| ConnectAndRequestError::RequestError(e))
            }
            .await;

            let error = match result {
                Ok(result) => {
                    self.stats_handler.on_success(attempts, retries);
                    break Ok(result);
                }
                Err(e) => e,
            };

            match error.get_error_handling_strategy() {
                ErrorHandlingStrategy::Reconnect => {
                    // Our connection to EdenFS broke.
                    // This typically means Eden restarted. Just reconnect.
                    tracing::info!(
                        "Reconnecting ({}/{} attempts) to EdenFS after: {:#}",
                        attempts,
                        MAX_RETRY_ATTEMPTS,
                        error
                    );
                    let mut guard = self.connection.lock();
                    if guard.epoch == connection.epoch {
                        guard.client = self.connector.connect(conn_timeout, recv_timeout);
                        guard.epoch += 1;
                    }
                    connection = (*guard).clone();
                }
                ErrorHandlingStrategy::Retry => {
                    // Our request failed but needs retrying.
                    retries += 1;
                    tracing::info!(
                        "Retrying ({}/{} attempts) EdenFS request after: {:#}",
                        attempts,
                        MAX_RETRY_ATTEMPTS,
                        error
                    );
                }
                ErrorHandlingStrategy::Abort => {
                    break Err(error);
                }
            };

            if attempts > MAX_RETRY_ATTEMPTS {
                break Err(error);
            }
        }
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
        let mut connection = (*self.streaming_connection.lock()).clone();
        let mut attempts = 0;
        let mut retries = 0;

        loop {
            attempts += 1;

            let result = async {
                let client = connection
                    .client
                    .clone()
                    .await
                    .map_err(|e| ConnectAndRequestError::ConnectionError(e))?;

                f(&client)
                    .await
                    .map_err(|e| ConnectAndRequestError::RequestError(e))
            }
            .await;

            let error = match result {
                Ok(result) => {
                    self.streaming_stats_handler.on_success(attempts, retries);
                    break Ok(result);
                }
                Err(e) => e,
            };

            match error.get_error_handling_strategy() {
                ErrorHandlingStrategy::Reconnect => {
                    // Our connection to EdenFS broke.
                    // This typically means Eden restarted. Just reconnect.
                    tracing::info!(
                        "Reconnecting ({}/{} attempts) to EdenFS after: {:#}",
                        attempts,
                        MAX_RETRY_ATTEMPTS,
                        error
                    );
                    let mut guard = self.streaming_connection.lock();
                    if guard.epoch == connection.epoch {
                        guard.client = self.connector.connect_streaming(conn_timeout, recv_timeout);
                        guard.epoch += 1;
                    }
                    connection = (*guard).clone();
                }
                ErrorHandlingStrategy::Retry => {
                    // Our request failed but needs retrying.
                    retries += 1;
                    tracing::info!(
                        "Retrying ({}/{} attempts) EdenFS request after: {:#}",
                        attempts,
                        MAX_RETRY_ATTEMPTS,
                        error
                    );
                }
                ErrorHandlingStrategy::Abort => {
                    break Err(error);
                }
            };

            if attempts > MAX_RETRY_ATTEMPTS {
                break Err(error);
            }
        }
    }
}
