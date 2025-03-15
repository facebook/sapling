/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use fbinit::FacebookInit;
use futures::future::BoxFuture;
use futures::future::Shared;
use thrift_streaming_clients::StreamingEdenServiceExt;
use thrift_streaming_thriftclients::make_StreamingEdenServiceExt_thriftclient;
use thrift_thriftclients::make_EdenServiceExt_thriftclient;
use thrift_types::edenfs_clients::EdenServiceExt;
use thriftclient::ThriftChannel;

pub type EdenFsThriftClient = Arc<dyn EdenServiceExt<ThriftChannel> + Send + Sync + 'static>;
#[allow(dead_code)]
type EdenFsThriftClientFuture = Shared<BoxFuture<'static, Result<EdenFsThriftClient>>>;

pub type StreamingEdenFsThriftClient =
    Arc<dyn StreamingEdenServiceExt<ThriftChannel> + Send + Sync + 'static>;
#[allow(dead_code)]
type StreamingEdenFsThriftClientFuture =
    Shared<BoxFuture<'static, Result<StreamingEdenFsThriftClient>>>;

pub(crate) struct EdenFsConnector {
    fb: FacebookInit,
    socket_file: PathBuf,
}

impl EdenFsConnector {
    pub(crate) fn new(fb: FacebookInit, socket_file: PathBuf) -> Self {
        Self { fb, socket_file }
    }

    pub(crate) async fn connect(&self, timeout: Option<Duration>) -> Result<EdenFsThriftClient> {
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
        let client = make_EdenServiceExt_thriftclient!(
            self.fb,
            protocol = CompactProtocol,
            from_path = &self.socket_file,
            with_conn_timeout = 120_000, // 2 minutes
            with_recv_timeout = 300_000, // 5 minutes
            with_secure = false,
        )?;
        Ok(client)
    }

    pub(crate) async fn connect_streaming(
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

    pub async fn connect_streaming_impl(&self) -> Result<StreamingEdenFsThriftClient> {
        let client = make_StreamingEdenServiceExt_thriftclient!(
            self.fb,
            protocol = CompactProtocol,
            from_path = &self.socket_file,
            with_conn_timeout = 120_000, // 2 minutes
            with_recv_timeout = 300_000, // 5 minutes
            with_secure = false,
        )?;
        Ok(client)
    }
}
