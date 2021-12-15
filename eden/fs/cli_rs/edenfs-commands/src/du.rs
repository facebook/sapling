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
use edenfs_client::checkout::find_checkout;
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
    println!("\n{}", title);
    println!("{}", "-".repeat(title.len()));
}

#[async_trait]
impl crate::Subcommand for DiskUsageCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        // Get mount configuration info
        let mounts = if !self.mounts.is_empty() {
            (&self.mounts).to_vec()
        } else {
            let config_paths: Vec<PathBuf> = instance
                .get_configured_mounts_map()?
                .keys()
                .cloned()
                .collect();
            if config_paths.is_empty() {
                return Err(EdenFsError::Other(anyhow!("No EdenFS mount found")));
            }
            config_paths
        };

        let mut backing_repos = Vec::new();

        for mount in &mounts {
            let checkout = find_checkout(&instance, mount)?;
            if let Some(b) = checkout.backing_repo() {
                backing_repos.push(b);
            }
        }
        write_title("Mounts");
        for path in &mounts {
            println!("{}", path.display());
        }

        write_title("Backing repos");
        for backing in backing_repos {
            println!("{}", backing.display());
        }
        println!(
            "\nCAUTION: You can lose work and break things by manually deleting data \
            from the backing repo directory!"
        );
        Ok(0)
    }
}
