/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl status

use std::time::Duration;

use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::DaemonHealthy;
use edenfs_client::EdenFsInstance;
use futures::stream::StreamExt;
use tokio::time;
use tracing::event;
use tracing::Level;

use crate::ExitCode;

#[derive(Parser, Debug)]
#[clap(about = "Check the health of the EdenFS service")]
pub struct StatusCmd {
    /// Wait up to TIMEOUT seconds for the daemon to respond
    #[clap(long, default_value = "3")]
    timeout: u64,

    #[clap(
        long,
        help = "Print progress of a starting EdenFS process and wait for it to \
successfully start."
    )]
    wait: bool,
}

enum EdenFsRunningStatus {
    Starting,
    Running(i32), // Holds the pid.
}

impl StatusCmd {
    async fn get_status_simple(
        &self,
        instance: &EdenFsInstance,
    ) -> edenfs_error::Result<thrift_types::edenfs::DaemonInfo> {
        let timeout = Duration::from_secs(self.timeout);
        let health = instance.get_health(Some(timeout));

        time::timeout(timeout, health)
            .await
            .map_err(edenfs_error::EdenFsError::RequestTimeout)?
    }

    #[cfg(fbcode_build)]
    async fn get_status_blocking_on_startup(
        &self,
        instance: &EdenFsInstance,
    ) -> edenfs_error::Result<thrift_types::edenfs::DaemonInfo> {
        let timeout = Duration::from_secs(self.timeout);
        let initial_result_and_stream = instance.get_health_with_startup_updates_included(timeout);
        let waited_health = time::timeout(timeout, initial_result_and_stream)
            .await
            .map_err(edenfs_error::EdenFsError::RequestTimeout)?;
        match waited_health {
            Ok((initial_result, mut startup_stream))
                if initial_result.status
                    == Some(thrift_types::fb303_core::fb303_status::STARTING) =>
            {
                println!("EdenFS is starting ...");
                while let Some(value) = startup_stream.next().await {
                    match value {
                        Ok(message) => {
                            println!("{}", String::from_utf8_lossy(&message));
                        }
                        Err(e) => {
                            println!("Error received from EdenFS while starting: {}", e);
                            break;
                        }
                    }
                }
                Ok(initial_result)
            }
            Ok((initial_result, _)) => Ok(initial_result),
            Err(edenfs_error::EdenFsError::Other(e)) => Err(edenfs_error::EdenFsError::Other(e)),
            Err(e) => Err(e),
        }
    }

    async fn get_status(
        &self,
        instance: &EdenFsInstance,
    ) -> edenfs_error::Result<thrift_types::edenfs::DaemonInfo> {
        #[cfg(fbcode_build)]
        if self.wait {
            let waited_status = self.get_status_blocking_on_startup(instance).await;
            if let Err(edenfs_error::EdenFsError::UnknownMethod(_)) = waited_status {
                println!(
                    "The version of EdenFS you are running does not yet support streaming status. Falling back to regular status ..."
                );
                return self.get_status_simple(instance).await;
            } else {
                return waited_status;
            }
        }
        self.get_status_simple(instance).await
    }

    /// @returns the pid of the running EdenFS daemon, and if the daemon is
    /// starting
    fn interpret_status(
        &self,
        instance: &EdenFsInstance,
        health: edenfs_error::Result<thrift_types::edenfs::DaemonInfo>,
    ) -> Result<EdenFsRunningStatus, anyhow::Error> {
        match health {
            Ok(health) if health.is_healthy() => {
                return Ok(EdenFsRunningStatus::Running(health.pid));
            }
            Ok(health) => {
                event!(
                    Level::DEBUG,
                    ?health,
                    "Connected to EdenFS daemon but daemon reported unhealthy status"
                );
                if health.status == Some(thrift_types::fb303_core::fb303_status::STARTING) {
                    return Ok(EdenFsRunningStatus::Starting);
                }
                if let Some(status) = health.status {
                    return Err(anyhow!("EdenFS is {}", status));
                }
            }
            Err(e) => {
                event!(
                    Level::DEBUG,
                    ?e,
                    "Error while collecting status information from EdenFS"
                );
            }
        }

        instance
            .status_from_lock()
            .map(EdenFsRunningStatus::Running)
    }

    fn display_simple(&self, status: Result<EdenFsRunningStatus, anyhow::Error>) -> ExitCode {
        match status {
            Ok(EdenFsRunningStatus::Running(pid)) => {
                println!("EdenFS is running normally (pid {})", pid);
                0
            }
            Ok(EdenFsRunningStatus::Starting) => {
                println!(
                    "EdenFS is still starting (hint: run `eden status --wait` to watch its progress)",
                );
                1
            }
            Err(cause) => {
                println!("EdenFS is not healthy: {}", cause);
                1
            }
        }
    }
}

#[async_trait]
impl crate::Subcommand for StatusCmd {
    async fn run(&self) -> Result<ExitCode> {
        let instance = EdenFsInstance::global();
        let status = self.get_status(instance).await;
        event!(Level::TRACE, ?status, "get_health");
        let display_result = self.interpret_status(instance, status);
        match display_result {
            #[cfg(fbcode_build)]
            Ok(EdenFsRunningStatus::Starting) if self.wait => {
                // get_status will have already printed out all the start up
                // status at this point. EdenFS might have crashed or finished
                // successfully. Check the status and display a clearer message
                // if EdenFS is not running.
                let final_status = self.get_status_simple(instance).await;
                let final_display_result = self.interpret_status(instance, final_status);
                if let Ok(EdenFsRunningStatus::Running(_)) = final_display_result {
                    return Ok(0);
                }
                Ok(self.display_simple(final_display_result))
            }
            _ => Ok(self.display_simple(display_result)),
        }
    }
}
