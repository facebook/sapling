/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod show;

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use git_source_of_truth::GitSourceOfTruthConfig;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use repo_identity::RepoIdentity;
use show::show;
use show::ShowArgs;

/// Query and manage git source of truth config for a repo
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: GitSourceOfTruthSubcommand,
}

#[derive(Subcommand)]
pub enum GitSourceOfTruthSubcommand {
    /// Show git source of truth config for this repo.
    Show(ShowArgs),
}

#[facet::container]
pub struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    git_source_of_truth_config: dyn GitSourceOfTruthConfig,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();
    let repo: Repo = app.open_repo(&args.repo).await?;

    match args.subcommand {
        GitSourceOfTruthSubcommand::Show(args) => show(&ctx, &repo, args).await,
    }
}
