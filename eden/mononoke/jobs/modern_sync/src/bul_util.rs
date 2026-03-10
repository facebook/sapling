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
use futures::stream::TryStreamExt;

use crate::sync::ExecutionType;

pub(crate) fn read_bookmark_update_log(
    ctx: &CoreContext,
    start_id: BookmarkUpdateLogId,
    exec_type: ExecutionType,
    bookmark_update_log: Arc<dyn BookmarkUpdateLog>,
    single_db_query_entries_limit: u64,
) -> impl stream::Stream<Item = Result<Vec<BookmarkUpdateLogEntry>, Error>> + '_ {
    stream::try_unfold(Some(start_id), move |maybe_id| {
        cloned!(ctx, bookmark_update_log, exec_type);
        async move {
            match maybe_id {
                Some(id) => {
                    let entries: Vec<_> = bookmark_update_log
                        .read_next_bookmark_log_entries(
                            ctx.clone(),
                            id,
                            single_db_query_entries_limit,
                            Freshness::MaybeStale,
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

pub fn group_entries(entries: Vec<BookmarkUpdateLogEntry>) -> Vec<BookmarkUpdateLogEntry> {
    let mut merged = vec![
        entries
            .first()
            .expect("BUL must have at least one entry")
            .clone(),
    ];
    for entry in &entries[1..] {
        let last_merged = merged.last_mut().unwrap();
        if entry.reason == last_merged.reason
            && entry.bookmark_name == last_merged.bookmark_name
            && (entry.from_changeset_id.is_none()
                || entry.from_changeset_id == last_merged.to_changeset_id)
        {
            last_merged.to_changeset_id = entry.to_changeset_id;
            last_merged.id = entry.id;
        } else {
            merged.push(entry.clone());
        }
    }
    merged
}
