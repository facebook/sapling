/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod fetch;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use repo_blobstore::RepoBlobstore;
use repo_identity::RepoIdentity;

use self::fetch::FetchArgs;

/// Perform git objects related operations.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: GitObjectsSubcommand,
}

#[facet::container]
pub struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_blobstore: RepoBlobstore,
}

#[derive(Subcommand)]
pub enum GitObjectsSubcommand {
    /// Fetch Git objects
    Fetch(FetchArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();
    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;
    match args.subcommand {
        GitObjectsSubcommand::Fetch(fetch_args) => fetch::fetch(&repo, &ctx, fetch_args).await?,
    }
    Ok(())
}
