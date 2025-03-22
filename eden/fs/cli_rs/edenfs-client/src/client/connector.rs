/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use edenfs_error::ConnectError;
use edenfs_error::ErrorHandlingStrategy;
use edenfs_error::HasErrorHandlingStrategy;
use fbinit::FacebookInit;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::Shared;
use thrift_streaming_clients::StreamingEdenServiceExt;
use thrift_streaming_thriftclients::make_StreamingEdenServiceExt_thriftclient;
use thrift_thriftclients::make_EdenServiceExt_thriftclient;
use thrift_types::edenfs_clients::errors::GetDaemonInfoError;
use thrift_types::edenfs_clients::EdenServiceExt;
use thrift_types::fb303_core::fb303_status;
use thriftclient::ThriftChannel;

// TODO: select better defaults (e.g. 1s connection timeout, 1m recv timeout)
const DEFAULT_CONN_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_RECV_TIMEOUT: Duration = Duration::from_secs(300);

pub type EdenFsThriftClient = Arc<dyn EdenServiceExt<ThriftChannel> + Send + Sync + 'static>;
pub type EdenFsThriftClientFuture =
    Shared<BoxFuture<'static, std::result::Result<EdenFsThriftClient, ConnectError>>>;

pub type StreamingEdenFsThriftClient =
    Arc<dyn StreamingEdenServiceExt<ThriftChannel> + Send + Sync + 'static>;
pub type StreamingEdenFsThriftClientFuture =
    Shared<BoxFuture<'static, std::result::Result<StreamingEdenFsThriftClient, ConnectError>>>;

pub(crate) struct EdenFsConnector {
    fb: FacebookInit,
    socket_file: PathBuf,
}

impl EdenFsConnector {
    pub(crate) fn new(fb: FacebookInit, socket_file: PathBuf) -> Self {
        Self { fb, socket_file }
    }

    pub(crate) fn connect(
        &self,
        conn_timeout: Option<Duration>,
        recv_timeout: Option<Duration>,
    ) -> EdenFsThriftClientFuture {
        let socket_file = self.socket_file.clone();
        let fb = self.fb;

        tokio::task::spawn(async move {
            tracing::info!(
                "Creating a new EdenFs connection via `{}`",
                socket_file.display()
            );

            // get the connection
            let client = EdenFsConnector::connect_impl(
                fb,
                &socket_file,
                conn_timeout.map_or(DEFAULT_CONN_TIMEOUT, |t| t).as_millis() as u32,
                recv_timeout.map_or(DEFAULT_RECV_TIMEOUT, |t| t).as_millis() as u32,
            )?;

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

    fn connect_impl(
        fb: FacebookInit,
        socket_file: &Path,
        conn_timeout: u32,
        recv_timeout: u32,
    ) -> std::result::Result<EdenFsThriftClient, ConnectError> {
        make_EdenServiceExt_thriftclient!(
            fb,
            protocol = CompactProtocol,
            from_path = socket_file,
            with_conn_timeout = conn_timeout,
            with_recv_timeout = recv_timeout,
            with_secure = false,
        )
        .with_context(|| "Unable to create an EdenFS thrift client")
        .map_err(|e| ConnectError::ConnectionError(e.to_string()))
    }

    pub(crate) fn connect_streaming(
        &self,
        conn_timeout: Option<Duration>,
        recv_timeout: Option<Duration>,
    ) -> StreamingEdenFsThriftClientFuture {
        let socket_file = self.socket_file.clone();
        let fb = self.fb;

        tokio::task::spawn(async move {
            tracing::info!(
                "Creating a new EdenFs streaming connection via `{}`",
                socket_file.display()
            );

            // get future for the connection
            let client = EdenFsConnector::connect_streaming_impl(
                fb,
                &socket_file,
                conn_timeout.map_or(DEFAULT_CONN_TIMEOUT, |t| t).as_millis() as u32,
                recv_timeout.map_or(DEFAULT_RECV_TIMEOUT, |t| t).as_millis() as u32,
            )?;

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

    pub fn connect_streaming_impl(
        fb: FacebookInit,
        socket_file: &Path,
        conn_timeout: u32,
        recv_timeout: u32,
    ) -> std::result::Result<StreamingEdenFsThriftClient, ConnectError> {
        make_StreamingEdenServiceExt_thriftclient!(
            fb,
            protocol = CompactProtocol,
            from_path = socket_file,
            with_conn_timeout = conn_timeout,
            with_recv_timeout = recv_timeout,
            with_secure = false,
        )
        .with_context(|| "Unable to create an EdenFS streaming thrift client")
        .map_err(|e| ConnectError::ConnectionError(e.to_string()))
    }

    async fn wait_until_deamon_is_ready(
        client: Arc<dyn EdenServiceExt<ThriftChannel> + Send + Sync>,
    ) -> std::result::Result<(), ConnectError> {
        let period = Duration::from_secs(1);
        let intervals = 10;
        let mut interval = tokio::time::interval(period);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        for _ in 0..intervals {
            match EdenFsConnector::is_daemon_ready(client.clone()).await {
                Ok(true) => return Ok(()),
                Ok(false) => {
                    // Fallthrough to keep going
                }
                Err(e) if e.get_error_handling_strategy() == ErrorHandlingStrategy::Retry => {
                    // Fallthrough to keep going
                    tracing::info!("The daemon is not ready: {e:?}. Retrying...");
                }
                Err(e) => {
                    tracing::info!("The daemon is not ready: {e:?}. Timing out...");
                    return Err(ConnectError::DaemonNotReadyError(e.to_string()));
                }
            }

            interval.tick().await;
        }

        let message = format!(
            "Timed out waiting for the daemon to be ready after {} seconds",
            intervals * period.as_secs()
        );
        tracing::info!(message);
        Err(ConnectError::DaemonNotReadyError(message))
    }

    async fn is_daemon_ready(
        client: Arc<dyn EdenServiceExt<ThriftChannel> + Send + Sync>,
    ) -> std::result::Result<bool, GetDaemonInfoError> {
        // Some tests set the EDENFS_SKIP_DAEMON_READY_CHECK environment variable
        // because they don't want to wait for the daemon to be ready - typically
        // due to fault injection stalling the daemon.
        //
        // In those cases, we just return success immediately.
        match env::var_os("EDENFS_SKIP_DAEMON_READY_CHECK") {
            Some(_) => Ok(true),
            None => {
                let daemon_info = client.getDaemonInfo().await?;
                Ok(daemon_info.status == Some(fb303_status::ALIVE))
            }
        }
    }
}
