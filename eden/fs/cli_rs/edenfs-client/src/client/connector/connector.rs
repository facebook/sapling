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
use edenfs_error::ConnectError;
use fbinit::FacebookInit;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::Shared;
use thrift_thriftclients::EdenServiceExt;
use thrift_thriftclients::make_EdenServiceExt_thriftclient;
use thriftclient::ThriftChannel;

use crate::client::connector::Connector;
use crate::client::connector::DEFAULT_CONN_TIMEOUT;
use crate::client::connector::DEFAULT_RECV_TIMEOUT;

pub type EdenFsThriftClient = Arc<dyn EdenServiceExt<ThriftChannel> + Send + Sync + 'static>;
pub type EdenFsThriftClientFuture =
    Shared<BoxFuture<'static, std::result::Result<EdenFsThriftClient, ConnectError>>>;

pub struct EdenFsConnector {
    fb: FacebookInit,
    socket_file: PathBuf,
}

impl Connector for EdenFsConnector {
    type Client = EdenFsThriftClient;
    type ClientFuture = EdenFsThriftClientFuture;

    fn new(fb: FacebookInit, socket_file: PathBuf) -> Self {
        Self { fb, socket_file }
    }

    fn connect(
        &self,
        conn_timeout: Option<Duration>,
        recv_timeout: Option<Duration>,
    ) -> Self::ClientFuture {
        let socket_file = self.socket_file.clone();
        let fb = self.fb;

        tokio::task::spawn(async move {
            tracing::info!(
                "Creating a new EdenFS connection via `{}`",
                socket_file.display()
            );

            // get the connection
            let client = make_EdenServiceExt_thriftclient!(
                fb,
                protocol = CompactProtocol,
                from_path = &socket_file,
                with_conn_timeout =
                    conn_timeout.map_or(DEFAULT_CONN_TIMEOUT, |t| t).as_millis() as u32,
                with_recv_timeout =
                    recv_timeout.map_or(DEFAULT_RECV_TIMEOUT, |t| t).as_millis() as u32,
                with_secure = false,
            )
            .with_context(|| "Unable to create an EdenFS thrift client")
            .map_err(|e| ConnectError::ConnectionError(e.to_string()))?;

            Ok(client)
        })
        .map(|r| match r {
            Ok(r) => r,
            Err(e) => Err(ConnectError::ConnectionError(e.to_string())),
        })
        .boxed()
        .shared()
    }
}
