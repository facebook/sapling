/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use bookmarks::BookmarkUpdateLogRef;
use bookmarks::Freshness;
use clap::Args;
use context::CoreContext;
use futures::stream::TryStreamExt;
use mutable_counters::MutableCountersRef;

use super::Repo;
use super::LATEST_REPLAYED_REQUEST_KEY;
use crate::bookmark_log_entry::BookmarkLogEntry;
use crate::commit_id::IdentityScheme;

#[derive(Args)]
pub struct HgSyncShowArgs {
    /// How many bundles to show
    #[clap(long, short = 'l', default_value_t = 10)]
    limit: u64,
}

pub async fn show(ctx: &CoreContext, repo: &Repo, show_args: HgSyncShowArgs) -> Result<()> {
    // This is not quite correct: the counter not existing and having the
    // value 0 are actually different, but this is an edge case we will ignore.
    let counter = repo
        .mutable_counters()
        .get_counter(ctx, LATEST_REPLAYED_REQUEST_KEY)
        .await?
        .unwrap_or(0)
        .try_into()?;

    repo.bookmark_update_log()
        .read_next_bookmark_log_entries(
            ctx.clone(),
            counter,
            show_args.limit,
            Freshness::MostRecent,
        )
        .map_ok(|entry| async move {
            BookmarkLogEntry::new(
                ctx,
                repo,
                entry.timestamp,
                entry.bookmark_name,
                entry.reason,
                entry.to_changeset_id,
                Some(entry.id.try_into()?),
                &[IdentityScheme::Hg],
            )
            .await
        })
        .try_buffered(100)
        .try_for_each(|entry| async move {
            println!("{}", entry);
            Ok(())
        })
        .await?;

    Ok(())
}
