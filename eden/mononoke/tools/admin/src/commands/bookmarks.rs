/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod delete;
mod get;
mod list;
mod log;
mod set;

use anyhow::Context;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bookmarks::BookmarkUpdateLog;
use bookmarks::Bookmarks;
use clap::Parser;
use clap::Subcommand;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use repo_cross_repo::RepoCrossRepo;
use repo_identity::RepoIdentity;

use delete::BookmarksDeleteArgs;
use get::BookmarksGetArgs;
use list::BookmarksListArgs;
use log::BookmarksLogArgs;
use set::BookmarksSetArgs;

/// Manage repository bookmarks
///
/// This is a low-level command providing direct access to the bookmarks
/// data store.  It allows modifications that would not ordinarily be
/// possible through normal bookmark movement requests.  You should prefer
/// using normal bookmark movements (via 'hg push' or 'scsc') unless the
/// modification you are making needs to be a low-level one.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: BookmarksSubcommand,
}

#[facet::container]
pub struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    bookmark_update_log: dyn BookmarkUpdateLog,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    bonsai_svnrev_mapping: dyn BonsaiSvnrevMapping,

    #[facet]
    repo_cross_repo: RepoCrossRepo,
}

#[derive(Subcommand)]
pub enum BookmarksSubcommand {
    /// Get the changeset of a bookmark
    Get(BookmarksGetArgs),
    /// List bookmarks
    List(BookmarksListArgs),
    /// Show the log of changesets for a bookmark
    Log(BookmarksLogArgs),
    /// Set a bookmark to a specific changeset
    ///
    /// This is a low-level command that writes directly to the bookmark
    /// store.  Prefer using ordinary methods to modify bookmarks where
    /// possible.
    Set(BookmarksSetArgs),
    /// Delete a bookmark
    ///
    /// This is a low-level command that writes directly to the bookmark
    /// store.  Prefer using ordinary methods to modify bookmarks where
    /// possible.
    Delete(BookmarksDeleteArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();

    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;

    match args.subcommand {
        BookmarksSubcommand::Get(get_args) => get::get(&ctx, &repo, get_args).await?,
        BookmarksSubcommand::Log(log_args) => log::log(&ctx, &repo, log_args).await?,
        BookmarksSubcommand::List(list_args) => list::list(&ctx, &repo, list_args).await?,
        BookmarksSubcommand::Set(set_args) => set::set(&ctx, &repo, set_args).await?,
        BookmarksSubcommand::Delete(delete_args) => {
            delete::delete(&ctx, &repo, delete_args).await?
        }
    }

    Ok(())
}
