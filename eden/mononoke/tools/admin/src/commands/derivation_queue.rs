/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod summary;
mod unsafe_evict;

use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bookmarks::Bookmarks;
use clap::Parser;
use clap::Subcommand;
use metaconfig_types::RepoConfig;
use metaconfig_types::RepoConfigRef;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use repo_derivation_queues::RepoDerivationQueues;
use repo_identity::RepoIdentity;
use summary::SummaryArgs;
use unsafe_evict::UnsafeEvictArgs;

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
    /// Evict an item (referenced by root cs_id and derived data type) from the derivation queue. WARNING: can leave dependent items in the queue stuck
    UnsafeEvict(UnsafeEvictArgs),
}

#[facet::container]
pub struct Repo {
    #[facet]
    repo_derivation_queues: RepoDerivationQueues,

    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_config: RepoConfig,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    bonsai_svnrev_mapping: dyn BonsaiSvnrevMapping,

    #[facet]
    bookmarks: dyn Bookmarks,
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
        DerivationQueueSubcommand::UnsafeEvict(args) => {
            unsafe_evict::unsafe_evict(&ctx, &repo, config_name, args).await
        }
    }
}
