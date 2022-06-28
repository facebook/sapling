/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateLogRef;
use bookmarks::Freshness;
use clap::Args;
use context::CoreContext;
use futures::stream::TryStreamExt;
use mononoke_types::DateTime;

use super::Repo;
use crate::bookmark_log_entry::BookmarkLogEntry;
use crate::commit_id::IdentityScheme;

#[derive(Args)]
pub struct BookmarksLogArgs {
    /// Name of the bookmark to show the log for
    name: BookmarkName,

    /// Commit identity schemes to display
    #[clap(long, short='S', arg_enum, default_values = &["bonsai"], use_value_delimiter = true)]
    schemes: Vec<IdentityScheme>,

    /// Limit the number of entries returned
    #[clap(long, short = 'l', default_value_t = 25)]
    limit: u32,

    /// Filter log records to those starting at this time
    /// (either absolute time, or e.g. "2 hours ago").
    #[clap(long, short = 's')]
    start_time: Option<DateTime>,

    /// Filter log records to those ending at this time
    /// (either absolute time, or e.g. "2 hours ago").
    #[clap(long, short = 'e', requires = "start-time")]
    end_time: Option<DateTime>,
}

pub async fn log(ctx: &CoreContext, repo: &Repo, log_args: BookmarksLogArgs) -> Result<()> {
    let entries = if log_args.start_time.is_some() || log_args.end_time.is_some() {
        let start = log_args
            .start_time
            .ok_or_else(|| anyhow!("end-time requires start-time"))?;
        let end = log_args.end_time.unwrap_or_else(DateTime::now);
        repo.bookmark_update_log()
            .list_bookmark_log_entries_ts_in_range(
                ctx.clone(),
                log_args.name.clone(),
                log_args.limit,
                start.into(),
                end.into(),
            )
    } else {
        repo.bookmark_update_log().list_bookmark_log_entries(
            ctx.clone(),
            log_args.name.clone(),
            log_args.limit,
            None,
            Freshness::MostRecent,
        )
    };

    entries
        .map_ok(|(entry_id, cs_id, reason, timestamp)| {
            BookmarkLogEntry::new(
                ctx,
                repo,
                timestamp,
                log_args.name.clone(),
                reason,
                cs_id,
                Some(entry_id),
                &log_args.schemes,
            )
        })
        .try_buffered(100)
        .try_for_each(|entry| async move {
            println!("{}", entry);
            Ok(())
        })
        .await?;
    Ok(())
}
