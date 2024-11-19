/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use bookmarks::BookmarkUpdateLog;
use bookmarks::BookmarkUpdateLogEntry;
use bookmarks::BookmarkUpdateLogId;
use bookmarks::Freshness;
use cloned::cloned;
use context::CoreContext;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;

use crate::sync::ExecutionType;

const SINGLE_DB_QUERY_ENTRIES_LIMIT: u64 = 10;

pub(crate) fn read_bookmark_update_log(
    ctx: &CoreContext,
    start_id: BookmarkUpdateLogId,
    exec_type: ExecutionType,
    bookmark_update_log: Arc<dyn BookmarkUpdateLog>,
) -> impl stream::Stream<Item = Result<Vec<BookmarkUpdateLogEntry>, Error>> + '_ {
    stream::try_unfold(Some(start_id), move |maybe_id| {
        cloned!(ctx, bookmark_update_log, exec_type);
        async move {
            match maybe_id {
                Some(id) => {
                    let entries: Vec<_> = bookmark_update_log
                        .read_next_bookmark_log_entries_same_bookmark_and_reason(
                            ctx.clone(),
                            id,
                            SINGLE_DB_QUERY_ENTRIES_LIMIT,
                        )
                        .try_collect()
                        .await
                        .context("While querying bookmarks_update_log")?;

                    match entries.iter().last().cloned() {
                        Some(last_entry) => Ok(Some((entries, Some(last_entry.id)))),
                        None => match exec_type {
                            ExecutionType::SyncOnce => Ok(Some((vec![], None))),
                            ExecutionType::Tail => Ok(Some((vec![], Some(id)))),
                        },
                    }
                }
                None => Ok(None),
            }
        }
    })
}

#[allow(dead_code)] // Keeping for future use
pub async fn get_one_entry(
    ctx: &CoreContext,
    bookmark_update_log: Arc<dyn BookmarkUpdateLog>,
    entry_id: BookmarkUpdateLogId,
) -> impl stream::Stream<Item = Result<BookmarkUpdateLogEntry, Error>> {
    let entries: Vec<Result<BookmarkUpdateLogEntry, Error>> = bookmark_update_log
        .read_next_bookmark_log_entries(ctx.clone(), entry_id, 1, Freshness::MaybeStale)
        .collect()
        .await;

    stream::iter(entries)
}
