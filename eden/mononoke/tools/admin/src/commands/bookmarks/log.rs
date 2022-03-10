/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use anyhow::{anyhow, Error, Result};
use bookmarks::{BookmarkName, BookmarkUpdateLogRef, BookmarkUpdateReason, Freshness};
use clap::Args;
use context::CoreContext;
use futures::stream::{self, StreamExt, TryStreamExt};
use mononoke_types::{ChangesetId, DateTime, Timestamp};

use crate::commit_id::IdentityScheme;
use crate::repo::AdminRepo;

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

struct BookmarkLogEntry {
    timestamp: Timestamp,
    bookmark: BookmarkName,
    reason: BookmarkUpdateReason,
    ids: Vec<(IdentityScheme, String)>,
    bundle_id: Option<u64>,
}

impl BookmarkLogEntry {
    async fn new(
        ctx: &CoreContext,
        repo: &AdminRepo,
        timestamp: Timestamp,
        bookmark: BookmarkName,
        reason: BookmarkUpdateReason,
        changeset_id: Option<ChangesetId>,
        bundle_id: Option<u64>,
        schemes: &[IdentityScheme],
    ) -> Result<Self> {
        let ids = if let Some(changeset_id) = changeset_id {
            stream::iter(schemes.iter().copied())
                .map(|scheme| {
                    Ok::<_, Error>(async move {
                        match scheme.map_commit_id(ctx, repo, changeset_id).await? {
                            Some(commit_id) => Ok(Some((scheme, commit_id))),
                            None => Ok(None),
                        }
                    })
                })
                .try_buffered(10)
                .try_filter_map(|commit_id| async move { Ok(commit_id) })
                .try_collect()
                .await?
        } else {
            Vec::new()
        };
        Ok(BookmarkLogEntry {
            timestamp,
            bookmark,
            reason,
            ids,
            bundle_id,
        })
    }
}

impl fmt::Display for BookmarkLogEntry {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        if let Some(bundle_id) = self.bundle_id {
            write!(fmt, "{} ", bundle_id)?;
        }
        write!(fmt, "({})", self.bookmark)?;
        match self.ids.as_slice() {
            [] => {}
            [(_, id)] => write!(fmt, " {}", id)?,
            ids => {
                for (scheme, id) in ids {
                    write!(fmt, " {}={}", scheme.to_string(), id)?;
                }
            }
        }
        write!(
            fmt,
            " {} {}",
            self.reason,
            DateTime::from(self.timestamp).as_chrono().to_rfc3339()
        )?;
        Ok(())
    }
}

pub async fn log(ctx: &CoreContext, repo: &AdminRepo, log_args: BookmarksLogArgs) -> Result<()> {
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
