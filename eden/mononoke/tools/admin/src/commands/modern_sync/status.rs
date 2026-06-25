/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateLogRef;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use clap::Args;
use commit_id::IdentityScheme;
use commit_id::print_commit_id;
use context::CoreContext;
use futures::stream::TryStreamExt;
use metaconfig_types::RepoConfigRef;
use repo_identity::RepoIdentityRef;

use super::Repo;
use crate::bookmark_log_entry::BookmarkLogEntry;

// Everything is rendered as bonsai -- the native changeset id, and what the AWS
// side prints too, so the two sides line up for an eyeball comparison.
const BONSAI: &[IdentityScheme] = &[IdentityScheme::Bonsai];

#[derive(Args)]
pub struct StatusArgs {
    /// The bookmark modern sync mirrors (it only syncs this one bookmark)
    #[clap(long, default_value = "master")]
    bookmark: BookmarkKey,
}

pub async fn status(ctx: &CoreContext, repo: &Repo, args: StatusArgs) -> Result<()> {
    let repo_name = repo.repo_identity().name();
    println!("Modern sync status for repo '{repo_name}'");
    println!();

    // --- Enablement gate ---
    // This is exactly what the sync job gates on (`sync.rs` / `sync_sharded.rs`
    // bail with "No modern sync config found"). If there is no config, the repo
    // is not mirrored to AWS, so there is nothing to compare.
    if repo.repo_config().modern_sync_config.is_none() {
        println!("Modern sync is NOT configured for this repo; nothing to compare.");
        return Ok(());
    }

    // --- bookmark (internal) ---
    println!("== {} ==", args.bookmark);
    let internal_master = repo
        .bookmarks()
        .get(ctx.clone(), &args.bookmark, Freshness::MostRecent)
        .await
        .with_context(|| format!("Failed to resolve bookmark '{}'", args.bookmark))?;
    print!("  internal: ");
    match internal_master {
        Some(cs_id) => print_commit_id(ctx, repo, BONSAI, cs_id).await?,
        None => println!("(not set)"),
    }
    println!();

    // --- latest movement (internal) ---
    println!("== latest movement of '{}' ==", args.bookmark);
    print!("  internal: ");
    print_internal_latest_movement(ctx, repo, &args.bookmark).await?;

    Ok(())
}

/// Print the most recent `bookmark_update_log` entry for the bookmark on the
/// internal (prod) side.
async fn print_internal_latest_movement(
    ctx: &CoreContext,
    repo: &Repo,
    bookmark: &BookmarkKey,
) -> Result<()> {
    let latest = repo
        .bookmark_update_log()
        .list_bookmark_log_entries(
            ctx.clone(),
            bookmark.clone(),
            1,
            None,
            Freshness::MostRecent,
        )
        .try_next()
        .await
        .context("Failed to read latest bookmark log entry")?;
    match latest {
        None => println!("(no log entries)"),
        Some((entry_id, cs_id, reason, timestamp)) => {
            let rendered = BookmarkLogEntry::new(
                ctx,
                repo,
                timestamp,
                bookmark.clone(),
                reason,
                cs_id,
                Some(entry_id),
                BONSAI,
            )
            .await?;
            println!("{rendered}");
        }
    }
    Ok(())
}
