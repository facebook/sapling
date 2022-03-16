/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod check_commit;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;

use crate::repo::AdminRepo;

use check_commit::CheckCommitArgs;

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
    }

    Ok(())
}
