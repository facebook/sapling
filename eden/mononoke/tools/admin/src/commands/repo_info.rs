/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Result};
use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use clap::Parser;
use mononoke_app::MononokeApp;
use mononoke_args::repo::RepoArgs;
use repo_identity::RepoIdentityRef;

/// List configured repositories.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();

    let repo: BlobRepo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;

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
