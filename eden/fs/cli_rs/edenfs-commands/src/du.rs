/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl du

use async_trait::async_trait;
use std::path::PathBuf;
use structopt::StructOpt;

use anyhow::anyhow;
use edenfs_client::EdenFsInstance;
use edenfs_error::{EdenFsError, Result};

use crate::ExitCode;

#[derive(StructOpt, Debug)]
#[structopt(about = "Show disk space usage for a checkout")]
pub struct DiskUsageCmd {
    #[structopt(help = "Names of the mount points")]
    mounts: Vec<PathBuf>,

    #[structopt(long, help = "Performs automated cleanup")]
    clean: bool,

    #[structopt(
        long,
        help = "Performs automated cleanup (--clean) and removes fsck dirs. \
        Unlike --clean this will destroy unrecoverable data. If you have any \
        local changes you hope to recover, recover them before you run this command."
    )]
    deep_clean: bool,

    #[structopt(long, help = "Print the output in JSON format")]
    json: bool,
}

fn write_title(title: &str) {
    println!("{}", title);
    println!("{}", "-".repeat(title.len()));
}

#[async_trait]
impl crate::Subcommand for DiskUsageCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        let mounts = if !self.mounts.is_empty() {
            (&self.mounts).to_vec()
        } else {
            let config_paths: Vec<String> = instance
                .get_configured_mounts_map()?
                .keys()
                .cloned()
                .collect();
            if config_paths.is_empty() {
                return Err(EdenFsError::Other(anyhow!("No EdenFS mount found")));
            }
            config_paths.iter().map(PathBuf::from).collect()
        };

        write_title("Mounts");
        for path in &mounts {
            println!("{}", path.display());
        }

        Ok(0)
    }
}
