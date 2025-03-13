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

use crate::instance::EdenFsInstance;

#[cfg(fbcode_build)]
pub type EdenFsThriftClient = Arc<dyn EdenServiceExt<ThriftChannel> + Send + Sync + 'static>;
#[cfg(fbcode_build)]
pub type StreamingEdenFsThriftClient = Arc<dyn StreamingEdenService + Send + Sync + 'static>;

pub struct EdenFsClient<'a> {
    instance: &'a EdenFsInstance,
}

impl<'a> EdenFsClient<'a> {
    pub(crate) fn new(instance: &'a EdenFsInstance) -> Self {
        Self { instance }
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
        let socket_path = self.instance.socketfile();

        let connect = EdenFsClient::_connect(&socket_path);
        if let Some(timeout) = timeout {
            tokio::time::timeout(timeout, connect)
                .await
                .with_context(|| "Unable to connect to EdenFS daemon")
                .map_err(|_| EdenFsError::ThriftConnectionTimeout(socket_path))?
        } else {
            connect.await
        }
    }

    async fn _connect(socket_path: &PathBuf) -> Result<EdenFsThriftClient> {
        const THRIFT_TIMEOUT_MS: u32 = 120_000;
        let client = make_EdenServiceExt_thriftclient!(
            expect_init(),
            protocol = CompactProtocol,
            from_path = socket_path,
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
        let socket_path = self.instance.socketfile();

        let client = EdenFsClient::_connect_streaming(&socket_path);

        if let Some(timeout) = timeout {
            tokio::time::timeout(timeout, client)
                .await
                .map_err(|_| EdenFsError::ThriftConnectionTimeout(socket_path))?
        } else {
            client.await
        }
    }

    #[cfg(fbcode_build)]
    pub async fn _connect_streaming(socket_path: &PathBuf) -> Result<StreamingEdenFsThriftClient> {
        use thriftclient::TransportType;

        let client = build_StreamingEdenService_client(
            ThriftChannelBuilder::from_path(expect_init(), socket_path)?
                .with_transport_type(TransportType::Rocket)
                .with_secure(false),
        )?;
        Ok(client)
    }
}
