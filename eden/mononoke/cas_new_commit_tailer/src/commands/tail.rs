/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Parser;
use mononoke_app::MononokeApp;

#[derive(Parser)]
pub struct CommandArgs {}

pub async fn run(_app: MononokeApp, _args: CommandArgs) -> Result<()> {
    eprintln!("Mononoke cas new commit tailer");
    Ok(())
}
