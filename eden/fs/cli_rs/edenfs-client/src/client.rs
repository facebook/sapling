/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Debug;
use std::fmt::Display;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use edenfs_error::ConnectAndRequestError;
use edenfs_error::EdenFsError;
use edenfs_error::HasErrorHandlingStrategy;
use edenfs_error::Result;
use fbinit::expect_init;
use thrift_streaming_clients::StreamingEdenService;
use thrift_streaming_thriftclients::build_StreamingEdenService_client;
use thrift_thriftclients::make_EdenServiceExt_thriftclient;
use thrift_types::edenfs_clients::EdenServiceExt;
use thriftclient::ThriftChannel;
use thriftclient::ThriftChannelBuilder;
use thriftclient::TransportType;

#[cfg(fbcode_build)]
pub type EdenFsThriftClient = Arc<dyn EdenServiceExt<ThriftChannel> + Send + Sync + 'static>;
#[cfg(fbcode_build)]
pub type StreamingEdenFsThriftClient = Arc<dyn StreamingEdenService + Send + Sync + 'static>;

pub struct EdenFsClient {
    socket_file: PathBuf,
}

impl EdenFsClient {
    pub(crate) fn new(socket_file: PathBuf) -> Self {
        Self { socket_file }
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
        let client = self
            .connect(None)
            .await
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
        let streaming_client = self
            .connect_streaming(None)
            .await
            .map_err(|e| ConnectAndRequestError::ConnectionError(e))?;

        // TODO: switch to buck2 lazy connection
        // TODO: switch to buck2 retry logic
        // TOOD: switch to buck2 error handling
        f(&streaming_client)
            .await
            .map_err(|e| ConnectAndRequestError::RequestError(e))
    }

    async fn connect(&self, timeout: Option<Duration>) -> Result<EdenFsThriftClient> {
        let client_future = self.connect_impl();
        if let Some(timeout) = timeout {
            tokio::time::timeout(timeout, client_future)
                .await
                .with_context(|| "Unable to connect to EdenFS daemon")
                .map_err(|_| EdenFsError::ThriftConnectionTimeout(self.socket_file.clone()))?
        } else {
            client_future.await
        }
    }

    async fn connect_impl(&self) -> Result<EdenFsThriftClient> {
        const THRIFT_TIMEOUT_MS: u32 = 120_000;
        let client = make_EdenServiceExt_thriftclient!(
            expect_init(),
            protocol = CompactProtocol,
            from_path = &self.socket_file,
            with_conn_timeout = THRIFT_TIMEOUT_MS,
            with_recv_timeout = THRIFT_TIMEOUT_MS,
            with_secure = false,
        )?;
        Ok(client)
    }

    #[cfg(fbcode_build)]
    pub async fn connect_streaming(
        &self,
        timeout: Option<Duration>,
    ) -> Result<StreamingEdenFsThriftClient> {
        let client_future = self.connect_streaming_impl();

        if let Some(timeout) = timeout {
            tokio::time::timeout(timeout, client_future)
                .await
                .map_err(|_| EdenFsError::ThriftConnectionTimeout(self.socket_file.clone()))?
        } else {
            client_future.await
        }
    }

    #[cfg(fbcode_build)]
    pub async fn connect_streaming_impl(&self) -> Result<StreamingEdenFsThriftClient> {
        let client = build_StreamingEdenService_client(
            ThriftChannelBuilder::from_path(expect_init(), &self.socket_file)?
                .with_transport_type(TransportType::Rocket)
                .with_secure(false),
        )?;
        Ok(client)
    }
}
