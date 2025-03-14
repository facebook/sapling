/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl debug stress
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::attributes::all_attributes;
use edenfs_client::attributes::GetAttributesV2Request;
use edenfs_client::checkout::find_checkout;
use edenfs_client::instance::EdenFsInstance;
use edenfs_client::request_factory::send_requests;
use edenfs_client::utils::expand_path_or_cwd;

use crate::ExitCode;

#[derive(Parser, Debug)]
pub struct CommonOptions {
    #[clap(long, parse(try_from_str = expand_path_or_cwd), default_value = "")]
    /// Path to the mount point
    mount_point: PathBuf,

    #[clap(short, long, default_value = "1000")]
    /// Number of requests to send to the Thrift server
    num_requests: u64,

    #[clap(short, long, default_value = "10")]
    /// Number of tasks to use for sending requests
    num_tasks: u64,
}

#[derive(Parser, Debug)]
#[clap(about = "Stress tests an EdenFS daemon by issuing a large number of thrift requests")]
pub enum StressCmd {
    #[clap(about = "Stress the getAttributesFromFilesV2 endpoint")]
    GetAttributesV2 {
        #[clap(flatten)]
        common: CommonOptions,

        #[clap(
            index = 1,
            required = true,
            help = "Glob pattern to enumerate the list of files for which we'll query attributes"
        )]
        glob_pattern: String,

        #[clap(long, possible_values = all_attributes(), use_value_delimiter = true, default_values = all_attributes())]
        attributes: Vec<String>,
    },
}

#[async_trait]
impl crate::Subcommand for StressCmd {
    async fn run(&self) -> Result<ExitCode> {
        let instance = EdenFsInstance::global();
        let client = instance.get_client();

        match self {
            Self::GetAttributesV2 {
                common,
                glob_pattern,
                attributes,
            } => {
                let checkout = find_checkout(instance, &common.mount_point).with_context(|| {
                    anyhow!(
                        "Failed to find checkout with path {}",
                        common.mount_point.display()
                    )
                })?;
                let glob_result = client
                    .glob_files_foreground(&checkout.path(), vec![glob_pattern.to_string()])
                    .await?;
                let request_factory = Arc::new(GetAttributesV2Request::new(
                    checkout.path(),
                    glob_result.matching_files,
                    attributes,
                ).with_context(|| anyhow!("Failed to create request factory for GetAttributesFromFilesV2 stress test"))?);
                send_requests(request_factory, common.num_requests, common.num_tasks).await?;
            }
        }
        Ok(0)
    }
}
