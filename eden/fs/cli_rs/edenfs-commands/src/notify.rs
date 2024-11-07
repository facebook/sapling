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
#[clap(about = "Provides a list of files changed since the specified journal position")]
pub struct NotifyCmd {}

#[async_trait]
impl crate::Subcommand for NotifyCmd {
    async fn run(&self) -> Result<ExitCode> {
        Ok(0)
    }
}
