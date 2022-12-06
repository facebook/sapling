/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Error;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateLog;
use bookmarks::BookmarkUpdateLogEntry;
use bookmarks::Freshness;
use cloned::cloned;
use context::CoreContext;
use futures::future;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::TryStreamExt;
use mononoke_types::RepositoryId;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::debug;

use crate::reporting::log_noop_iteration_to_scuba;

const SLEEP_SECS: u64 = 10;
const SINGLE_DB_QUERY_ENTRIES_LIMIT: u64 = 10;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct QueueSize(pub usize);

/// Adds remaining number of changesets to sync to each entry
/// The idea is to indicate the number of changesets, left to sync
/// *after* a given changeset has been synced, therefore `n-i-1`
/// For example, `[c1, c2, c3]` will turn into `[(c1, 2), (c2, 1), (c3, 0)]`
fn add_queue_sizes<T>(
    items: Vec<T>,
    initial_queue_size: usize,
) -> impl Iterator<Item = (T, QueueSize)> {
    items
        .into_iter()
        .enumerate()
        .map(move |(i, item)| (item, QueueSize(initial_queue_size - i - 1)))
}

/// Run a queue size query, consume the produced `Result` and turn it
/// into an `Option`, suitable for `unfold`
async fn query_queue_size(
    ctx: CoreContext,
    bookmark_update_log: Arc<dyn BookmarkUpdateLog>,
    current_id: u64,
) -> Result<u64, Error> {
    bookmark_update_log
        .count_further_bookmark_log_entries(ctx.clone(), current_id, None)
        .await
        .map(|queue_size| {
            debug!(ctx.logger(), "queue size query returned: {}", queue_size);
            queue_size
        })
}

/// Produce an infinite stream of `Result` from a fallible item factory
/// Two differences with the normal `unfold`:
/// - this one does not expect the item factory (`f`) to return an `Option`,
///   so there's no way to terminate a stream from within `f`
/// - this one expects `f` to return a `Result`, which is threaded downstream
///   and allows the consumer of the stream to terminate it on `Err`
/// The main motivation for this is to be able to use `?` in the item factory
fn unfold_forever<T, F, Fut, Item>(
    init: T,
    mut f: F,
) -> impl stream::Stream<Item = Result<Item, Error>>
where
    T: Copy,
    F: FnMut(T) -> Fut,
    Fut: future::Future<Output = Result<(Item, T), Error>>,
{
    stream::unfold(init, move |iteration_value| {
        f(iteration_value).then(move |result| match result {
            Ok((item, next_it_val)) => future::ready(Some((Ok(item), next_it_val))),
            Err(e) => future::ready(Some((Err(e), iteration_value))),
        })
    })
}

pub(crate) fn tail_entries(
    ctx: CoreContext,
    start_id: u64,
    skip_bookmarks: HashSet<BookmarkName>,
    repo_id: RepositoryId,
    bookmark_update_log: Arc<dyn BookmarkUpdateLog>,
    scuba_sample: MononokeScubaSampleBuilder,
) -> impl stream::Stream<Item = Result<(BookmarkUpdateLogEntry, QueueSize), Error>> {
    unfold_forever((0, start_id), move |(iteration, current_id)| {
        cloned!(ctx, bookmark_update_log, skip_bookmarks, scuba_sample);
        async move {
            let entries: Vec<_> = bookmark_update_log
                .read_next_bookmark_log_entries(
                    ctx.clone(),
                    current_id,
                    SINGLE_DB_QUERY_ENTRIES_LIMIT,
                    Freshness::MaybeStale,
                )
                .try_collect()
                .await
                .context("While querying bookmarks_update_log")?;

            let queue_size =
                query_queue_size(ctx.clone(), bookmark_update_log.clone(), current_id).await?;

            match entries.last().map(|last_item_ref| last_item_ref.id) {
                Some(last_entry_id) => {
                    let entries: Vec<_> = entries
                        .into_iter()
                        .filter(|entry| !skip_bookmarks.contains(&entry.bookmark_name))
                        .collect();
                    debug!(
                        ctx.logger(),
                        "tail_entries generating {} new entries, queue size {}, iteration {}",
                        entries.len(),
                        (queue_size as usize) - entries.len(),
                        iteration
                    );

                    let entries_with_queue_size: std::iter::Map<_, _> =
                        add_queue_sizes(entries, queue_size as usize).map(Ok);

                    Ok((
                        stream::iter(entries_with_queue_size).boxed(),
                        (iteration + 1, last_entry_id as u64),
                    ))
                }
                None => {
                    debug!(
                        ctx.logger(),
                        "tail_entries: no more entries during iteration {}. Sleeping.", iteration
                    );
                    log_noop_iteration_to_scuba(scuba_sample, repo_id);
                    tokio::time::sleep(Duration::new(SLEEP_SECS, 0)).await;
                    Ok((stream::empty().boxed(), (iteration + 1, current_id)))
                }
            }
        }
    })
    .try_flatten()
}
