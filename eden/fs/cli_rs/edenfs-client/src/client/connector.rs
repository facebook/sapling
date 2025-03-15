/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use edenfs_error::ConnectError;
use fbinit::FacebookInit;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::Shared;
use thrift_streaming_clients::StreamingEdenServiceExt;
use thrift_streaming_thriftclients::make_StreamingEdenServiceExt_thriftclient;
use thrift_thriftclients::make_EdenServiceExt_thriftclient;
use thrift_thriftclients::EdenService;
use thrift_types::edenfs_clients::EdenServiceExt;
use thriftclient::ThriftChannel;

pub type EdenFsThriftClient = Arc<dyn EdenServiceExt<ThriftChannel> + Send + Sync + 'static>;
type EdenFsThriftClientFuture =
    Shared<BoxFuture<'static, std::result::Result<EdenFsThriftClient, ConnectError>>>;

pub type StreamingEdenFsThriftClient =
    Arc<dyn StreamingEdenServiceExt<ThriftChannel> + Send + Sync + 'static>;
type StreamingEdenFsThriftClientFuture =
    Shared<BoxFuture<'static, std::result::Result<StreamingEdenFsThriftClient, ConnectError>>>;

pub(crate) struct EdenFsConnector {
    fb: FacebookInit,
    socket_file: PathBuf,
}

impl EdenFsConnector {
    pub(crate) fn new(fb: FacebookInit, socket_file: PathBuf) -> Self {
        Self { fb, socket_file }
    }

    pub(crate) fn connect(&self, timeout: Option<Duration>) -> EdenFsThriftClientFuture {
        let socket_file = self.socket_file.clone();
        let fb = self.fb;

        tokio::task::spawn(async move {
            tracing::info!(
                "Creating a new EdenFs connection via `{}`",
                socket_file.display()
            );

            // get future for the connection
            let client_future = EdenFsConnector::connect_impl(fb, &socket_file);

            // wait for the connection - with or without timeout
            let client = if let Some(timeout) = timeout {
                tokio::time::timeout(timeout, client_future)
                    .await
                    .with_context(|| "Unable to connect to EdenFS daemon")
                    .map_err(|e| ConnectError::ConnectionError(e.to_string()))??
            } else {
                client_future.await?
            };

            // wait until the daemon is ready
            EdenFsConnector::wait_until_deamon_is_ready(client.clone()).await?;

            Ok(client)
        })
        .map(|r| match r {
            Ok(r) => r,
            Err(e) => Err(ConnectError::ConnectionError(e.to_string())),
        })
        .boxed()
        .shared()
    }

    async fn connect_impl(
        fb: FacebookInit,
        socket_file: &Path,
    ) -> std::result::Result<EdenFsThriftClient, ConnectError> {
        make_EdenServiceExt_thriftclient!(
            fb,
            protocol = CompactProtocol,
            from_path = socket_file,
            with_conn_timeout = 120_000, // 2 minutes
            with_recv_timeout = 300_000, // 5 minutes
            with_secure = false,
        )
        .with_context(|| "Unable to create an EdenFS thrift client")
        .map_err(|e| ConnectError::ConnectionError(e.to_string()))
    }

    pub(crate) async fn connect_streaming(
        &self,
        timeout: Option<Duration>,
    ) -> StreamingEdenFsThriftClientFuture {
        let socket_file = self.socket_file.clone();
        let fb = self.fb;

        tokio::task::spawn(async move {
            tracing::info!(
                "Creating a new EdenFs streaming connection via `{}`",
                socket_file.display()
            );

            // get future for the connection
            let client_future = EdenFsConnector::connect_streaming_impl(fb, &socket_file);

            // wait for the connection - with or without timeout
            let client = if let Some(timeout) = timeout {
                tokio::time::timeout(timeout, client_future)
                    .await
                    .with_context(|| "Unable to connect to EdenFS daemon")
                    .map_err(|e| ConnectError::ConnectionError(e.to_string()))??
            } else {
                client_future.await?
            };

            // wait until the mount is ready
            EdenFsConnector::wait_until_deamon_is_ready(client.clone()).await?;

            Ok(client)
        })
        .map(|r| match r {
            Ok(r) => r,
            Err(e) => Err(ConnectError::ConnectionError(e.to_string())),
        })
        .boxed()
        .shared()
    }

    pub async fn connect_streaming_impl(
        fb: FacebookInit,
        socket_file: &Path,
    ) -> std::result::Result<StreamingEdenFsThriftClient, ConnectError> {
        make_StreamingEdenServiceExt_thriftclient!(
            fb,
            protocol = CompactProtocol,
            from_path = socket_file,
            with_conn_timeout = 120_000, // 2 minutes
            with_recv_timeout = 300_000, // 5 minutes
            with_secure = false,
        )
        .with_context(|| "Unable to create an EdenFS streaming thrift client")
        .map_err(|e| ConnectError::ConnectionError(e.to_string()))
    }

    async fn wait_until_deamon_is_ready(
        _client: Arc<dyn EdenService + Send + Sync>,
    ) -> std::result::Result<(), ConnectError> {
        // TODO: implement logic to wait until the daemon is ready
        Ok(())
    }
}
