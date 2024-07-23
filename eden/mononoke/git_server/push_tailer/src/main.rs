/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Result;
use clap::Parser;
use fbinit::FacebookInit;

mod tail;

#[derive(Debug, Parser)]
#[clap(about = "Manage MetaGit as follower of Mononoke Git repositories.")]
struct Args {
    #[clap(subcommand)]
    cmd: Command,
}

#[derive(Debug, Parser)]
enum Command {
    /// Tails repositories with Mononoke Git server as backend and signals
    /// MetaGit followers to start replication.
    Tail(tail::Args),
}

#[fbinit::main]
async fn main(fb: FacebookInit) -> Result<()> {
    let args = Args::parse();
    match args.cmd {
        Command::Tail(args) => tail::tail(fb, args).await,
    }
}
