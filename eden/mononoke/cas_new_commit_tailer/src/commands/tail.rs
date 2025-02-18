/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Parser;
use futures::TryStreamExt;
use mononoke_app::MononokeApp;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;

#[derive(Parser)]
pub struct CommandArgs {}

pub async fn run(_app: MononokeApp, _args: CommandArgs) -> Result<()> {
    let stdin = BufReader::new(tokio::io::stdin());
    let lines = tokio_stream::wrappers::LinesStream::new(stdin.lines());

    lines
        .try_for_each(|line| async move {
            println!("{line}");
            Ok(())
        })
        .await?;
    Ok(())
}
