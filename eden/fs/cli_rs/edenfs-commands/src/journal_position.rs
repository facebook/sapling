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
#[clap(about = "Returns the current EdenFS journal position")]
pub struct JournalPositionCmd {}

#[async_trait]
impl crate::Subcommand for JournalPositionCmd {
    async fn run(&self) -> Result<ExitCode> {
        println!("Not Implemented");
        Ok(0)
    }
}
