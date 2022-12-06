/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use bookmarks::BookmarkName;
use bookmarks::BookmarksRef;
use clap::Args;
use context::CoreContext;
use repo_identity::RepoIdentityRef;

use super::Repo;

#[derive(Args)]
pub struct RepoInfoArgs {}

pub async fn repo_info(ctx: &CoreContext, repo: &Repo, _args: RepoInfoArgs) -> Result<()> {
    println!("Repo: {}", repo.repo_identity().name());
    println!("Repo-Id: {}", repo.repo_identity().id());
    let main_bookmark = BookmarkName::new("master")?;
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
    Ok(())
}
