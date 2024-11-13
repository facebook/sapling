/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use bookmarks::BookmarkKey;
use bookmarks::Bookmarks;
use bookmarks::BookmarksRef;
use clap::Parser;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;
use sql_commit_graph_storage::CommitGraphBulkFetcher;
use sql_commit_graph_storage::CommitGraphBulkFetcherRef;

/// Show information about a repository
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,
}

#[derive(Clone)]
#[facet::container]
pub struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    commit_graph_bulk_fetcher: CommitGraphBulkFetcher,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();

    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;

    println!("Repo: {}", repo.repo_identity().name());
    println!("Repo-Id: {}", repo.repo_identity().id());
    let main_bookmark = BookmarkKey::new("master")?;
    let main_bookmark_value = repo
        .bookmarks()
        .get(ctx.clone(), &main_bookmark)
        .await
        .with_context(|| format!("Failed to resolve main bookmark ({})", main_bookmark))?
        .as_ref()
        .map(ToString::to_string);
    println!(
        "Main-Bookmark: {} {}",
        main_bookmark,
        main_bookmark_value.as_deref().unwrap_or("(not set)")
    );

    let commits = repo
        .commit_graph_bulk_fetcher()
        .fetch_commit_count(&ctx, repo.repo_identity().id())
        .await?;

    println!("Commits: {}", commits);
    Ok(())
}
