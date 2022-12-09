/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod info;
mod lock;

use anyhow::Result;
use bookmarks::Bookmarks;
use clap::Parser;
use clap::Subcommand;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use repo_identity::RepoIdentity;
use repo_lock::RepoLock;

/// Operations over a whole repo
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: RepoSubcommand,
}

#[derive(Clone)]
#[facet::container]
pub struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    lock: dyn RepoLock,
}

#[derive(Subcommand)]
pub enum RepoSubcommand {
    /// Show information about a repository
    Info(info::RepoInfoArgs),
    /// Lock a repository
    Lock(lock::RepoLockArgs),
    /// Unlock a repository
    Unlock(lock::RepoUnlockArgs),
    /// Show current lock status of a repository
    ShowLock(lock::RepoShowLockArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();
    let repo: Repo = app.open_repo(&args.repo).await?;

    use RepoSubcommand::*;
    match args.subcommand {
        Info(args) => info::repo_info(&ctx, &repo, args).await?,
        Lock(args) => lock::repo_lock(&app, &repo, args).await?,
        Unlock(args) => lock::repo_unlock(&app, &repo, args).await?,
        ShowLock(args) => lock::repo_show_lock(&app, &repo, args).await?,
    }
    Ok(())
}
