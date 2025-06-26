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
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use edenfs_error::ConnectAndRequestError;
use edenfs_error::HasErrorHandlingStrategy;
use fbinit::FacebookInit;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
pub use thrift_thriftclients::EdenService;

use crate::client::Client;
use crate::client::Connector;
use crate::client::EdenFsClientStatsHandler;
use crate::client::NoopEdenFsClientStatsHandler;
use crate::client::connector::StreamingEdenFsConnector;
use crate::client::connector::StreamingEdenFsThriftClient;
use crate::methods::EdenThriftMethod;
use crate::use_case::UseCase;

pub struct MockThriftClient {
    thrift_service: Option<StreamingEdenFsThriftClient>,
    stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>,
}

impl MockThriftClient {
    pub fn set_thrift_service(&mut self, client: StreamingEdenFsThriftClient) {
        self.thrift_service = Some(client);
    }
}

#[async_trait]
impl Client for MockThriftClient {
    fn new(_fb: FacebookInit, _use_case: Arc<UseCase>, _socket_file: PathBuf) -> Self {
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
        F: Fn(&<StreamingEdenFsConnector as Connector>::Client) -> (Fut, EdenThriftMethod)
            + Send
            + Sync,
        Fut: Future<Output = Result<T, E>> + Send,
        T: Send,
        E: HasErrorHandlingStrategy + Debug + Display,
    {
        let service = self.thrift_service.clone().unwrap();
        let (fut, _method) = f(&service);
        fut.await.map_err(|e| e.into())
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
            Err(_) => panic!("Error joining tokio task."),
        })
        .boxed()
}

pub fn make_boxed_stream_result<T, StreamError, ApiError>(
    results: Result<Vec<Result<T, StreamError>>, ApiError>,
) -> BoxFuture<'static, Result<BoxStream<'static, Result<T, StreamError>>, ApiError>>
where
    T: Send + 'static,
    StreamError: Send + 'static,
    ApiError: Send + 'static,
{
    tokio::task::spawn(async move {
        match results {
            Ok(results) => Ok(stream::iter(results).boxed()),
            Err(e) => Err(e),
        }
    })
    .map(|r| match r {
        Ok(r) => r,
        Err(_) => panic!("Error joining tokio task."),
    })
    .boxed()
}
