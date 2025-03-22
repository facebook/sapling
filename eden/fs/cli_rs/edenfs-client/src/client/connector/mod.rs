/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod connector;
mod streaming_connector;

use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub(crate) use connector::*;
use edenfs_error::ConnectError;
use edenfs_error::ErrorHandlingStrategy;
use edenfs_error::HasErrorHandlingStrategy;
use fbinit::FacebookInit;
pub(crate) use streaming_connector::*;
use thrift_types::edenfs_clients::errors::GetDaemonInfoError;
use thrift_types::edenfs_clients::EdenServiceExt;
use thrift_types::fb303_core::fb303_status;
use thriftclient::ThriftChannel;

// TODO: select better defaults (e.g. 1s connection timeout, 1m recv timeout)
const DEFAULT_CONN_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_RECV_TIMEOUT: Duration = Duration::from_secs(300);

/// A trait that defines the common interface for EdenFS connectors.
pub trait Connector {
    /// The type of client this connector creates.
    type Client: Send + Sync + 'static;

    /// The future type returned by the connect method.
    type ClientFuture: Clone
        + std::future::Future<Output = std::result::Result<Self::Client, ConnectError>>
        + Send;

    /// Creates a new connector instance.
    fn new(fb: FacebookInit, socket_file: PathBuf) -> Self;

    /// Connects to the EdenFS service.
    fn connect(
        &self,
        conn_timeout: Option<Duration>,
        recv_timeout: Option<Duration>,
    ) -> Self::ClientFuture;
}

async fn wait_until_deamon_is_ready(
    client: Arc<dyn EdenServiceExt<ThriftChannel> + Send + Sync>,
) -> std::result::Result<(), ConnectError> {
    let period = Duration::from_secs(1);
    let intervals = 10;
    let mut interval = tokio::time::interval(period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    for _ in 0..intervals {
        match is_daemon_ready(client.clone()).await {
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
