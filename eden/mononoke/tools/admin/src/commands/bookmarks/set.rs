/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

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
pub struct BookmarksSetArgs {
    /// Name of the bookmark to set
    name: BookmarkName,

    /// Commit ID to set the bookmark to
    ///
    /// This can be any commit id type.  Specify 'scheme=id' to disambiguate
    /// commit identity scheme (e.g. 'hg=HASH', 'globalrev=REV').
    commit_id: String,

    /// Force setting of bookmark in repos with pushredirection enabled
    /// (WARNING: this may break megarepo sync)
    #[clap(long)]
    force_megarepo: bool,

    /// Mark the bookmark being created or modified as scratch
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

    /// Only set this bookmark if it does not already exist.
    #[clap(long, conflicts_with = "old-commit-id")]
    create_only: bool,
}

pub async fn set(ctx: &CoreContext, repo: &Repo, set_args: BookmarksSetArgs) -> Result<()> {
    let kind = if set_args.scratch {
        BookmarkKind::Scratch
    } else {
        BookmarkKind::Publishing
    };
    let target = parse_commit_id(ctx, repo, &set_args.commit_id).await?;
    let old_value = if set_args.create_only {
        None
    } else if let Some(old_commit_id) = &set_args.old_commit_id {
        Some(parse_commit_id(ctx, repo, old_commit_id).await?)
    } else {
        repo.bookmarks()
            .get(ctx.clone(), &set_args.name)
            .await
            .with_context(|| format!("Failed to resolve bookmark '{}'", set_args.name))?
    };

    match old_value {
        Some(old_value) => {
            println!(
                "Updating {} bookmark {} from {} to {}",
                kind, set_args.name, old_value, target
            );
        }
        None => {
            println!("Creating {} bookmark {} at {}", kind, set_args.name, target);
        }
    }

    if let Err(e) = check_bookmark_sync_config(repo, &set_args.name, kind) {
        if set_args.force_megarepo {
            println!("Moving bookmark in megarepo-synced repository (--force-megarepo)");
            println!("Waiting 3 seconds. Ctrl-C now if you did not intend this - risk of SEV!");
            tokio::time::sleep(Duration::from_secs(3)).await;
        } else {
            return Err(e).context("Refusing to move bookmark in megarepo-synced repository");
        }
    };

    // Wait 1s to allow for Ctrl-C
    tokio::time::sleep(Duration::from_secs(1)).await;

    let mut transaction = repo.bookmarks().create_transaction(ctx.clone());

    match (old_value, kind) {
        (Some(old_value), BookmarkKind::Publishing | BookmarkKind::PullDefaultPublishing) => {
            transaction.update(
                &set_args.name,
                target,
                old_value,
                BookmarkUpdateReason::ManualMove,
            )?;
        }
        (None, BookmarkKind::Publishing | BookmarkKind::PullDefaultPublishing) => {
            transaction.create(&set_args.name, target, BookmarkUpdateReason::ManualMove)?;
        }
        (Some(old_value), BookmarkKind::Scratch) => {
            transaction.update_scratch(&set_args.name, target, old_value)?;
        }
        (None, BookmarkKind::Scratch) => {
            transaction.create_scratch(&set_args.name, target)?;
        }
    }
    transaction.commit().await?;
    Ok(())
}
