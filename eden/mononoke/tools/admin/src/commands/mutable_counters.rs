/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod get;
mod list;
mod set;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use get::GetArgs;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use mutable_counters::MutableCounters;
use repo_identity::RepoIdentity;
use set::SetArgs;

/// Get, set or list mutable counters
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: MutableCountersSubcommand,
}

#[facet::container]
pub struct Repo {
    #[facet]
    mutable_counters: dyn MutableCounters,
    #[facet]
    repo_identity: RepoIdentity,
}

#[derive(Subcommand)]
pub enum MutableCountersSubcommand {
    /// Get the current value of a mutable counter in a repo.
    Get(GetArgs),
    /// Set the value of a mutable counter in a repo. If the counter doesn't exist, create it.
    Set(SetArgs),
    /// List all the mutable counters in repos along with their values.
    List,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();
    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;

    match args.subcommand {
        MutableCountersSubcommand::Get(args) => get::get(&ctx, &repo, args).await?,
        MutableCountersSubcommand::Set(args) => set::set(&ctx, &repo, args).await?,
        MutableCountersSubcommand::List => list::list(&ctx, &repo).await?,
    }
    Ok(())
}
