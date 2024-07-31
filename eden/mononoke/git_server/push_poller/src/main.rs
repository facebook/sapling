/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::anyhow;
use anyhow::Result;
use clap::Parser;
use fbinit::FacebookInit;
use poller::poll;
use poller::Args as PollerArgs;

#[derive(Debug, Parser)]
#[clap(about = "Manage MetaGit as follower of Mononoke Git repositories.")]
struct Args {
    #[clap(subcommand)]
    cmd: Command,
}

#[derive(Debug, Parser)]
enum Command {
    /// Polls repositories with Mononoke Git server as backend and signals
    /// MetaGit followers to start replication.
    Poll(PollerArgs),
}

fn setup_logging() -> Result<()> {
    let glog_drain = slog_glog_fmt::default_drain();
    let drain = slog_envlogger::new(glog_drain);
    let logger = slog::Logger::root(drain, slog::o!());
    logging::set_from_main(&logger).map_err(|_| anyhow!("Logger already initialized"))
}

#[fbinit::main]
async fn main(fb: FacebookInit) -> Result<()> {
    setup_logging()?;
    let args = Args::parse();
    match args.cmd {
        Command::Poll(args) => poll(fb, args).await,
    }
}
