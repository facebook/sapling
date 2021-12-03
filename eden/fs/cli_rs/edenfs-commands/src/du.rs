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

use anyhow::Error;
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

#[async_trait]
impl crate::Subcommand for DiskUsageCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        Err(EdenFsError::Other(Error::msg("Not implemented yet.")))
    }
}
