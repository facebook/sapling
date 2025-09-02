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
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use phases::Phases;
use phases::PhasesRef;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;
use sql_commit_graph_storage::CommitGraphBulkFetcher;
use sql_commit_graph_storage::CommitGraphBulkFetcherRef;

/// Show information about a repository
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,
    /// Show total, public and draft commit counts (this can be expensive for large repos)
    #[clap(long)]
    show_commit_count: bool,
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

    #[facet]
    phases: dyn Phases,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();

    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;
    let id = repo.repo_identity().id();
    println!("Repo: {}", repo.repo_identity().name());
    println!("Repo-Id: {}", repo.repo_identity().id());
    let main_bookmark = BookmarkKey::new("master")?;
    let main_bookmark_value = repo
        .bookmarks()
        .get(
            ctx.clone(),
            &main_bookmark,
            bookmarks::Freshness::MostRecent,
        )
        .await
        .with_context(|| format!("Failed to resolve main bookmark ({})", main_bookmark))?
        .as_ref()
        .map(ToString::to_string);
    println!(
        "Main-Bookmark: {} {}",
        main_bookmark,
        main_bookmark_value.as_deref().unwrap_or("(not set)")
    );
    if args.show_commit_count {
        let commits = repo
            .commit_graph_bulk_fetcher()
            .fetch_commit_count(&ctx, id)
            .await?;

        let public = repo.phases().count_all_public(&ctx, id).await?;

        println!(
            "Commits: {} (Public: {}, Draft: {})",
            commits,
            public,
            commits - public
        );
    }

    Ok(())
}
