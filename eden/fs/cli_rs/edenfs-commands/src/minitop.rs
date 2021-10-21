/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl minitop

use async_trait::async_trait;
use structopt::StructOpt;

use anyhow::Error;
use edenfs_client::EdenFsInstance;
use edenfs_error::{EdenFsError, Result};

use crate::ExitCode;

#[derive(StructOpt, Debug)]
#[structopt(about = "Simple monitoring of EdenFS accesses by process.")]
pub struct MinitopCmd {}

#[async_trait]
impl crate::Subcommand for MinitopCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        Err(EdenFsError::Other(Error::msg("Not implemented yet.")))
    }
}
