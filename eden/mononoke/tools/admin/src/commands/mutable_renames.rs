/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod check_commit;
mod get;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;

use crate::repo::AdminRepo;

use check_commit::CheckCommitArgs;
use get::GetArgs;

/// Fetch and update mutable renames information
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: MutableRenamesSubcommand,
}

#[derive(Subcommand)]
pub enum MutableRenamesSubcommand {
    /// Determine if a commit has mutable rename information attached
    CheckCommit(CheckCommitArgs),
    /// Get mutable rename information for a given commit, path pair
    Get(GetArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();

    let repo: AdminRepo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;

    match args.subcommand {
        MutableRenamesSubcommand::CheckCommit(args) => {
            check_commit::check_commit(&ctx, &repo, args).await?
        }
        MutableRenamesSubcommand::Get(args) => get::get(&ctx, &repo, args).await?,
    }

    Ok(())
}
