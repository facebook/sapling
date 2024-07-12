/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod summary;

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use metaconfig_types::RepoConfig;
use metaconfig_types::RepoConfigRef;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use repo_derivation_queues::RepoDerivationQueues;
use summary::SummaryArgs;

/// Query and manage the derivation queue
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    /// Name of the config to query the derivation queue of.
    #[clap(short, long)]
    config_name: Option<String>,

    #[clap(subcommand)]
    subcommand: DerivationQueueSubcommand,
}

#[derive(Subcommand)]
pub enum DerivationQueueSubcommand {
    /// Display a summary of the items in the derivation queue.
    Summary(SummaryArgs),
}

#[facet::container]
pub struct Repo {
    #[facet]
    repo_derivation_queues: RepoDerivationQueues,
    #[facet]
    repo_config: RepoConfig,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();
    let repo: Repo = app.open_repo(&args.repo).await?;

    let config_name = args
        .config_name
        .as_ref()
        .unwrap_or_else(|| &repo.repo_config().derived_data_config.enabled_config_name);

    match args.subcommand {
        DerivationQueueSubcommand::Summary(args) => {
            summary::summary(&ctx, &repo, config_name, args).await
        }
    }
}
