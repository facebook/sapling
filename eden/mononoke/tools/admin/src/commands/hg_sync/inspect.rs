/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use bookmarks::BookmarkUpdateLogRef;
use bookmarks::Freshness;
use clap::Args;
use context::CoreContext;
use futures::stream::StreamExt;

use super::Repo;

#[derive(Args)]
pub struct HgSyncInspectArgs {
    /// Sync log entry to inspect
    id: i64,
}

pub async fn inspect(
    ctx: &CoreContext,
    repo: &Repo,
    inspect_args: HgSyncInspectArgs,
) -> Result<()> {
    let log_entry = repo
        .bookmark_update_log()
        .read_next_bookmark_log_entries(
            ctx.clone(),
            (inspect_args.id - 1).try_into().context("Invalid log id")?,
            1,
            Freshness::MostRecent,
        )
        .next()
        .await
        .ok_or_else(|| anyhow!("No log entries found"))??;

    if log_entry.id != inspect_args.id {
        bail!("No entry with id {} found", inspect_args.id);
    }

    println!("Bookmark: {}", log_entry.bookmark_name);
    match (&log_entry.from_changeset_id, &log_entry.to_changeset_id) {
        (None, Some(csid)) => println!("Created at: {}", csid),
        (Some(old), Some(new)) => println!("Moved from: {}\nMoved to: {}", old, new),
        (Some(csid), None) => println!("Deleted from: {}", csid),
        _ => bail!("Invalid log entry"),
    }

    Ok(())
}
