/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use bookmarks::BookmarkUpdateLog;
use bookmarks::BookmarkUpdateLogEntry;
use bookmarks::BookmarkUpdateLogId;
use bookmarks::Freshness;
use context::CoreContext;
use futures::stream;
use futures::stream::StreamExt;

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
