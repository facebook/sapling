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
use crate::Subcommand;

mod changes_since;
mod enter_state;
mod get_position;

#[derive(Parser, Debug)]
#[clap(about = "Provides a list of filesystem changes since the specified position")]
pub struct NotifyCmd {
    #[clap(subcommand)]
    subcommand: NotifySubcommand,
}

#[derive(Parser, Debug)]
pub enum NotifySubcommand {
    GetPosition(get_position::GetPositionCmd),
    ChangesSince(changes_since::ChangesSinceCmd),
    EnterState(enter_state::EnterStateCmd),
}

#[async_trait]
impl Subcommand for NotifyCmd {
    async fn run(&self) -> Result<ExitCode> {
        use NotifySubcommand::*;
        let sc: &(dyn Subcommand + Send + Sync) = match &self.subcommand {
            GetPosition(cmd) => cmd,
            ChangesSince(cmd) => cmd,
            EnterState(cmd) => cmd,
        };
        sc.run().await
    }
}
