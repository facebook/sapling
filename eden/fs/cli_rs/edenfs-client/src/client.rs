/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use fbinit::expect_init;
use thrift_streaming_thriftclients::build_StreamingEdenService_client;
use thrift_thriftclients::make_EdenServiceExt_thriftclient;
use thriftclient::ThriftChannelBuilder;

use crate::instance::EdenFsInstance;
use crate::EdenFsThriftClient;
use crate::StreamingEdenFsThriftClient;

pub struct EdenFsClient {
    pub(crate) client: EdenFsThriftClient,
}

impl EdenFsClient {
    pub(crate) async fn new(instance: &EdenFsInstance, timeout: Option<Duration>) -> Result<Self> {
        let client = EdenFsClient::connect(instance, timeout).await?;
        Ok(Self { client })
    }

    // TEMPORARY: This is a temporary workaround while we are refactoring EdenFsInstance into smaller modules
    pub fn get_thrift_client(&self) -> &EdenFsThriftClient {
        &self.client
    }

    async fn connect(
        instance: &EdenFsInstance,
        timeout: Option<Duration>,
    ) -> Result<EdenFsThriftClient> {
        let socket_path = instance.socketfile();

        let connect = EdenFsClient::_connect(&socket_path);
        if let Some(timeout) = timeout {
            tokio::time::timeout(timeout, connect)
                .await
                .with_context(|| "Unable to connect to EdenFS daemon")
                .map_err(|_| EdenFsError::ThriftConnectionTimeout(socket_path))?
        } else {
            connect.await.map_err(|err| EdenFsError::Other(err.into()))
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
}

pub struct StreamingEdenFsClient {
    pub(crate) streaming_client: StreamingEdenFsThriftClient,
}

impl StreamingEdenFsClient {
    pub(crate) async fn new(instance: &EdenFsInstance, timeout: Option<Duration>) -> Result<Self> {
        let streaming_client = StreamingEdenFsClient::connect_streaming(instance, timeout).await?;
        Ok(Self { streaming_client })
    }

    // TEMPORARY: This is a temporary workaround while we are refactoring EdenFsInstance into smaller modules
    pub fn get_thrift_client(&self) -> &StreamingEdenFsThriftClient {
        &self.streaming_client
    }

    #[cfg(fbcode_build)]
    pub async fn connect_streaming(
        instance: &EdenFsInstance,
        timeout: Option<Duration>,
    ) -> Result<StreamingEdenFsThriftClient> {
        let socket_path = instance.socketfile();

        let client = StreamingEdenFsClient::_connect_streaming(&socket_path);

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
