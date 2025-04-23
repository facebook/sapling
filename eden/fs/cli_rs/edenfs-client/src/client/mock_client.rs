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
use std::result::Result;
use std::time::Duration;

use async_trait::async_trait;
use edenfs_error::ConnectAndRequestError;
use edenfs_error::HasErrorHandlingStrategy;
use fbinit::FacebookInit;
use futures::future::BoxFuture;
use futures::future::FutureExt;
pub use thrift_thriftclients::EdenService;
use tokio::sync::Semaphore;

use crate::client::Client;
use crate::client::Connector;
use crate::client::EdenFsClientStatsHandler;
use crate::client::NoopEdenFsClientStatsHandler;
use crate::client::connector::EdenFsConnector;
use crate::client::connector::EdenFsThriftClient;
use crate::client::connector::StreamingEdenFsConnector;

pub struct MockThriftClient {
    thrift_service: Option<EdenFsThriftClient>,
    stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>,
}

impl MockThriftClient {
    pub fn set_thrift_service(&mut self, client: EdenFsThriftClient) {
        self.thrift_service = Some(client);
    }
}

#[async_trait]
impl Client for MockThriftClient {
    fn new(_fb: FacebookInit, _socket_file: PathBuf, _semaphore: Option<Semaphore>) -> Self {
        Self {
            thrift_service: None,
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
        f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&<EdenFsConnector as Connector>::Client) -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, E>> + Send,
        T: Send,
        E: HasErrorHandlingStrategy + Debug + Display,
    {
        let service = self.thrift_service.clone().unwrap();
        f(&service).await.map_err(|e| e.into())
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

pub fn make_boxed_future_result<T, E>(result: Result<T, E>) -> BoxFuture<'static, Result<T, E>>
where
    T: Send + 'static,
    E: Send + 'static,
{
    tokio::task::spawn(async move { result })
        .map(|r| match r {
            Ok(r) => r,
            Err(_) => panic!("Error joing tokio task."),
        })
        .boxed()
}
