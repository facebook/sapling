/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::Args;
use clap::Parser;
use clap::command;
use cmdlib_logging::ScribeLoggingArgs;
use executor_lib::args::ShardedExecutorArgs;
use mononoke_app::args::OptSourceAndTargetRepoArgs;

pub(crate) const SCUBA_TABLE: &str = "mononoke_xrepo_backsync";

#[derive(Clone, Debug, Args)]
#[clap(about = "Syncs all commits from the file")]
pub struct CommitsCommandArgs {
    /// File containing the list of commits to sync
    #[clap(long)]
    pub input_file: String,

    /// How many commits to backsync at once
    #[clap(long)]
    pub batch_size: Option<usize>,
}

#[derive(Debug, clap::Subcommand)]
pub enum BacksyncerCommand {
    /// Backsyncs all new bookmark moves once
    #[command(name = "backsync-all")]
    Once,
    /// Backsyncs all new bookmark moves
    #[command(name = "backsync-forever")]
    Forever,
    /// Syncs all commits from the file
    #[command(name = "backsync-commits")]
    Commits(CommitsCommandArgs),
}

#[derive(Debug, Parser)]
#[clap(about = "CLI to sync commits from large repositories (i.e. mega repos) to small ones")]
pub struct BacksyncerArgs {
    /// Identifiers or names for the source and target repos
    #[clap(flatten, next_help_heading = "CROSS REPO OPTIONS")]
    pub repo_args: OptSourceAndTargetRepoArgs,

    #[clap(flatten)]
    pub scribe_logging_args: ScribeLoggingArgs,

    #[clap(flatten)]
    pub sharded_executor_args: ShardedExecutorArgs,

    #[command(subcommand)]
    pub command: BacksyncerCommand,
}
