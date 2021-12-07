/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl list

use async_trait::async_trait;
use structopt::StructOpt;

use anyhow::Error;
use edenfs_client::EdenFsInstance;
use edenfs_error::{EdenFsError, Result};

use crate::ExitCode;

#[derive(StructOpt, Debug)]
#[structopt(about = "List available checkouts")]
pub struct ListCmd {
    #[structopt(long, help = "Print the output in JSON format")]
    json: bool,
}

#[async_trait]
impl crate::Subcommand for ListCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        Err(EdenFsError::Other(Error::msg("Not implemented yet.")))
    }
}
