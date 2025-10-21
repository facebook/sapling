/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Connection management.

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use std::net::IpAddr;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use std::sync::Arc;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use fbinit::FacebookInit;
use scs_client_raw::SCS_DEFAULT_TIER;
use scs_client_raw::ScsClient;
use scs_client_raw::ScsClientBuilder;
use scs_client_raw::ScsClientHostBuilder;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use tupperware_api_common::TaskHandle;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use tupperware_api_tupperware::ResolveTasksRequest;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use tupperware_api_tupperware::TaskRequest;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use tupperware_api_tupperware_srclients::TupperwareReadOnlyService;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use tupperware_api_tupperware_srclients::make_TupperwareReadOnlyService_srclient;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const SCS_PORT_NAME: &str = "thrift";

#[derive(clap::Args)]
pub(super) struct ConnectionArgs {
    #[clap(long, default_value = "scsc-default-client", global = true)]
    /// Name of the client for quota attribution and logging.
    client_id: String,
    #[clap(long, short, default_value = SCS_DEFAULT_TIER, global = true)]
    /// Connect to SCS through given tier.
    tier: String,
    #[clap(long, short = 'H', conflicts_with = "tier", global = true)]
    /// Connect to SCS through a given host and port pair, format HOST:PORT.
    host: Option<String>,
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[clap(long, conflicts_with_all = ["tier", "host"], global = true)]
    /// Connect to SCS through a Tupperware task handle, format <cluster>/<user>/<name>/<taskID>.
    tw: Option<String>,
    #[clap(long, global = true)]
    processing_timeout: Option<u64>,
    /// Serialized Crypto Auth Token (CAT) to use for authentication.
    #[clap(long, global = true, env = "SCSC_CAT")]
    cat: Option<String>,
}

/// Get the IP address and port for a task handle.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
async fn resolve_task_ip_and_port(
    fb: FacebookInit,
    task_handle: &TaskHandle,
) -> Result<(IpAddr, Option<u16>)> {
    let client = make_TupperwareReadOnlyService_srclient!(fb)?;
    resolve_task_ip_and_port_with_client(task_handle, client).await
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
async fn resolve_task_ip_and_port_with_client(
    task_handle: &TaskHandle,
    client: Arc<dyn TupperwareReadOnlyService + Send + Sync>,
) -> Result<(IpAddr, Option<u16>)> {
    let req = ResolveTasksRequest {
        task: TaskRequest {
            jobHandle: task_handle.jobHandle.clone(),
            taskIDs: vec![task_handle.taskID],
            ..Default::default()
        },
        ..Default::default()
    };

    let resp = client.resolveTasks(&req).await?;
    let task_info = &resp
        .taskData
        .first()
        .context("unable to resolve task")?
        .latestTaskInfo;

    let port = task_info
        .ports
        .iter()
        .find(|p| p.name == SCS_PORT_NAME)
        .map(|p| p.port as u16);

    Ok((task_info.taskIp.parse()?, port))
}

impl ConnectionArgs {
    pub async fn get_connection(
        &self,
        fb: FacebookInit,
        repo: Option<&str>,
    ) -> Result<ScsClient, Error> {
        let disable_sr =
            std::env::var("MONONOKE_INTEGRATION_TEST_DISABLE_SR").is_ok_and(|v| v == "true");

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let host = if let Some(tw_handle) = &self.tw {
            match hostcaps::is_corp() {
                true => {
                    return Err(Error::msg(
                        "Cannot connect to SCS through a Tupperware task handle in corp.",
                    ));
                }
                false => {
                    let task_handle: TaskHandle =
                        tw_handle.parse().context("failed to parse task handle")?;
                    let (ip_addr, port) = resolve_task_ip_and_port(fb, &task_handle).await?;
                    let host_port = if let Some(port) = port {
                        format!("{}:{}", ip_addr, port)
                    } else {
                        ip_addr.to_string()
                    };
                    Some(host_port)
                }
            }
        } else {
            self.host.clone()
        };

        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let host = self.host.clone();

        if let Some(ref host_str) = host {
            if disable_sr {
                return ScsClientHostBuilder::new().build_from_host_port(fb, host_str);
            }
        }

        ScsClientBuilder::new(fb, self.client_id.clone())
            .with_tier(&self.tier)
            .with_repo(repo.map(|r| r.to_string()))
            .with_host_and_port(host)?
            .with_processing_timeout(self.processing_timeout)
            .with_cat(self.cat.clone())
            .build()
    }
}
