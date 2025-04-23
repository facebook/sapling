/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg(test)]

use std::fmt::Debug;
use std::fmt::Display;
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use edenfs_error::ConnectAndRequestError;
use edenfs_error::HasErrorHandlingStrategy;
use fbinit::FacebookInit;
use tokio::sync::Semaphore;

use crate::client::Client;
use crate::client::EdenFsClientStatsHandler;
use crate::client::NoopEdenFsClientStatsHandler;
use crate::client::connector::Connector;
use crate::client::connector::EdenFsConnector;
use crate::client::connector::StreamingEdenFsConnector;

pub struct MockThriftClient {
    stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>,
}

#[async_trait]
impl Client for MockThriftClient {
    fn new(_fb: FacebookInit, _socket_file: PathBuf, _semaphore: Option<Semaphore>) -> Self {
        Self {
            stats_handler: Box::new(NoopEdenFsClientStatsHandler {}),
        }
    }

    fn set_stats_handler(
        &mut self,
        stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>,
    ) {
        self.stats_handler = stats_handler;
    }

    async fn with_thrift_with_timeouts<F, Fut, T, E>(
        &self,
        _conn_timeout: Option<Duration>,
        _recv_timeout: Option<Duration>,
        _f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&<EdenFsConnector as Connector>::Client) -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, E>> + Send,
        T: Send,
        E: HasErrorHandlingStrategy + Debug + Display,
    {
        unimplemented!()
    }

    async fn with_streaming_thrift_with_timeouts<F, Fut, T, E>(
        &self,
        _conn_timeout: Option<Duration>,
        _recv_timeout: Option<Duration>,
        _f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&<StreamingEdenFsConnector as Connector>::Client) -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, E>> + Send,
        T: Send,
        E: HasErrorHandlingStrategy + Debug + Display,
    {
        unimplemented!()
    }
}
