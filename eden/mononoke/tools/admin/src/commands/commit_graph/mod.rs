/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod backfill;
mod checkpoints;

use anyhow::Result;
use backfill::BackfillArgs;
use changesets::Changesets;
use clap::Parser;
use clap::Subcommand;
use metaconfig_types::RepoConfig;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use repo_identity::RepoIdentity;

#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: CommitGraphSubcommand,
}

#[derive(Subcommand)]
pub enum CommitGraphSubcommand {
    /// Backfill commit graph entries
    Backfill(BackfillArgs),
}

#[facet::container]
pub struct Repo {
    #[facet]
    changesets: dyn Changesets,
    #[facet]
    config: RepoConfig,
    #[facet]
    id: RepoIdentity,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();
    let repo: Repo = app.open_repo(&args.repo).await?;

    match args.subcommand {
        CommitGraphSubcommand::Backfill(args) => backfill::backfill(&ctx, &app, &repo, args).await,
    }
}
