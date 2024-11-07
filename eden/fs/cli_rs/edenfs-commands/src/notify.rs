/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl notify

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;

use crate::ExitCode;

#[derive(Parser, Debug)]
#[clap(about = "Provides a list of filesystem changes since the specified position")]
pub struct NotifyCmd {
    #[clap(
        short,
        long,
        help = "Return filesystem changes starting from this position to the current one. Returns the current position at time of call."
    )]
    position: String,
}

#[async_trait]
impl crate::Subcommand for NotifyCmd {
    async fn run(&self) -> Result<ExitCode> {
        println!("Not Implemented. Args: position: {:?}", self.position);
        Ok(0)
    }
}
