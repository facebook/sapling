/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use bookmarks_movement::check_bookmark_sync_config;
use bookmarks_movement::BookmarkKind;
use clap::Args;
use context::CoreContext;

use super::Repo;
use crate::commit_id::parse_commit_id;

#[derive(Args)]
pub struct BookmarksDeleteArgs {
    /// Name of the bookmark to delete
    name: BookmarkName,

    /// Force deleting of bookmark in repos with pushredirection enabled
    /// (WARNING: this may break megarepo sync)
    #[clap(long)]
    force_megarepo: bool,

    /// Delete a scratch bookmark
    ///
    /// Normally whether a bookmark is scratch or not is determined by
    /// a regex pattern in repository config.  This command does not use
    /// that configuration, and you must specify whether or not the
    /// bookmark is scratch using this flag.
    #[clap(long)]
    scratch: bool,

    /// Specify the expected current value for the bookmark.
    ///
    /// This can be any commit id type.  Specify 'scheme=id' to disambiguate
    /// commit identity scheme (e.g. 'hg=HASH', 'globalrev=REV').
    #[clap(long)]
    old_commit_id: Option<String>,
}

pub async fn delete(
    ctx: &CoreContext,
    repo: &Repo,
    delete_args: BookmarksDeleteArgs,
) -> Result<()> {
    let kind = if delete_args.scratch {
        BookmarkKind::Scratch
    } else {
        BookmarkKind::Publishing
    };
    let old_value = if let Some(old_commit_id) = &delete_args.old_commit_id {
        parse_commit_id(ctx, repo, old_commit_id).await?
    } else {
        repo.bookmarks()
            .get(ctx.clone(), &delete_args.name)
            .await
            .with_context(|| format!("Failed to resolve bookmark '{}'", delete_args.name))?
            .ok_or_else(|| {
                anyhow!(
                    "Cannot delete non-existent {} bookmark {}",
                    kind.to_string(),
                    delete_args.name
                )
            })?
    };

    println!(
        "Deleting {} bookmark {} at {}",
        kind, delete_args.name, old_value,
    );

    if let Err(e) = check_bookmark_sync_config(repo, &delete_args.name, kind) {
        if delete_args.force_megarepo {
            println!("Deleting bookmark in megarepo-synced repository (--force-megarepo)");
            println!("Waiting 3 seconds. Ctrl-C now if you did not intend this - risk of SEV!");
            tokio::time::sleep(Duration::from_secs(3)).await;
        } else {
            return Err(e).context("Refusing to delete bookmark in megarepo-synced repository");
        }
    };

    // Wait 1s to allow for Ctrl-C
    tokio::time::sleep(Duration::from_secs(1)).await;

    let mut transaction = repo.bookmarks().create_transaction(ctx.clone());

    match kind {
        BookmarkKind::Publishing | BookmarkKind::PullDefaultPublishing => {
            transaction.delete(
                &delete_args.name,
                old_value,
                BookmarkUpdateReason::ManualMove,
            )?;
        }
        BookmarkKind::Scratch => {
            transaction.delete_scratch(&delete_args.name, old_value)?;
        }
    }
    transaction.commit().await?;
    Ok(())
}
