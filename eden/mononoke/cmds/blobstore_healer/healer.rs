/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Error, Result};
use blobstore::Blobstore;
use blobstore_sync_queue::{BlobstoreSyncQueue, BlobstoreSyncQueueEntry, OperationKey};
use context::CoreContext;
use futures::{
    future::{join_all, FutureExt, TryFutureExt},
    stream::{self, StreamExt, TryStreamExt},
};
use itertools::{Either, Itertools};
use metaconfig_types::{BlobstoreId, MultiplexId};
use mononoke_types::{BlobstoreBytes, DateTime};
use rand::{thread_rng, Rng};
use scuba_ext::ScubaSampleBuilderExt;
use slog::{info, warn};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    iter::Sum,
    ops::Add,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

#[cfg(test)]
mod tests;

const MIN_FETCH_FAILURE_DELAY: Duration = Duration::from_millis(1);
const MAX_FETCH_FAILURE_DELAY: Duration = Duration::from_millis(100);

pub struct Healer {
    blobstore_sync_queue_limit: usize,
    current_fetch_size: AtomicUsize,
    heal_concurrency: usize,
    sync_queue: Arc<dyn BlobstoreSyncQueue>,
    blobstores: Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
    multiplex_id: MultiplexId,
    blobstore_key_like: Option<String>,
    drain_only: bool,
}

impl Healer {
    pub fn new(
        blobstore_sync_queue_limit: usize,
        heal_concurrency: usize,
        sync_queue: Arc<dyn BlobstoreSyncQueue>,
        blobstores: Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
        multiplex_id: MultiplexId,
        blobstore_key_like: Option<String>,
        drain_only: bool,
    ) -> Self {
        Self {
            blobstore_sync_queue_limit,
            current_fetch_size: AtomicUsize::new(blobstore_sync_queue_limit),
            heal_concurrency,
            sync_queue,
            blobstores,
            multiplex_id,
            blobstore_key_like,
            drain_only,
        }
    }

    async fn fetch_entries(
        &self,
        ctx: &CoreContext,
        healing_deadline: DateTime,
    ) -> Result<(usize, Vec<BlobstoreSyncQueueEntry>)> {
        let mut fetch_size = self.current_fetch_size.load(Ordering::Relaxed);
        loop {
            match self
                .sync_queue
                .iter(
                    ctx,
                    self.blobstore_key_like.as_ref(),
                    self.multiplex_id,
                    healing_deadline.clone(),
                    fetch_size,
                )
                .await
            {
                Ok(queue_entries) => {
                    // Success. Update fetch size for next loop
                    let new_fetch_size =
                        if fetch_size == self.current_fetch_size.load(Ordering::Relaxed) {
                            // Fetch size didn't change during the loop, which implies that we succeeded
                            // on the first attempt (since all failures decrease it) - increase it for next
                            // time if it's not yet at the limit. Growth is at least 1 each loop
                            // so that if fetch_size / 10 == 0, we still climb back
                            self.blobstore_sync_queue_limit
                                .min(fetch_size + fetch_size / 10 + 1)
                        } else {
                            fetch_size
                        };

                    self.current_fetch_size
                        .store(new_fetch_size, Ordering::Relaxed);

                    return Ok((fetch_size, queue_entries));
                }
                Err(e) => {
                    // Error, so fall in size fast
                    fetch_size = fetch_size / 2;
                    if fetch_size < 1 {
                        return Err(e);
                    }

                    let delay =
                        thread_rng().gen_range(MIN_FETCH_FAILURE_DELAY, MAX_FETCH_FAILURE_DELAY);
                    tokio::time::delay_for(delay).await;
                }
            }
        }
    }
    /// Heal one batch of entries. It selects a set of entries which are not too young (bounded
    /// by healing_deadline) up to `blobstore_sync_queue_limit` at once.
    /// Returns a tuple:
    /// - first item indicates whether a full batch was fetcehd
    /// - second item shows how many rows were deleted from the DB
    pub async fn heal(&self, ctx: &CoreContext, healing_deadline: DateTime) -> Result<(bool, u64)> {
        let heal_concurrency = self.heal_concurrency;
        let drain_only = self.drain_only;
        let multiplex_id = self.multiplex_id;

        let (max_batch_size, queue_entries) =
            self.fetch_entries(ctx, healing_deadline.clone()).await?;

        let entries = queue_entries
            .iter()
            .map(|e| format!("{:?}", e))
            .collect::<Vec<_>>();

        ctx.scuba()
            .clone()
            .add("entries", entries)
            .log_with_msg("Received Entries", None);

        info!(
            ctx.logger(),
            "Fetched {} queue entires (before building healing futures)",
            queue_entries.len()
        );

        let unique_blobstore_keys = queue_entries
            .iter()
            .unique_by(|entry| entry.blobstore_key.clone())
            .into_iter()
            .count();

        let unique_operation_keys = queue_entries
            .iter()
            .unique_by(|entry| entry.operation_key.clone())
            .into_iter()
            .count();

        info!(
            ctx.logger(),
            "Out of them {} distinct blobstore keys, {} distinct operation keys",
            unique_blobstore_keys,
            unique_operation_keys
        );

        let healing_futures: Vec<_> = queue_entries
            .into_iter()
            .sorted_by_key(|entry| entry.blobstore_key.clone())
            .group_by(|entry| entry.blobstore_key.clone())
            .into_iter()
            .filter_map(|(key, entries)| {
                let entries: Vec<_> = entries.collect();
                if drain_only {
                    Some(
                        async move {
                            Ok((
                                HealStats {
                                    queue_del: entries.len(),
                                    queue_add: 0,
                                    put_success: 0,
                                    put_failure: 0,
                                },
                                entries,
                            ))
                        }
                        .left_future(),
                    )
                } else {
                    let heal_opt = heal_blob(
                        ctx,
                        &self.sync_queue,
                        &self.blobstores,
                        healing_deadline,
                        key,
                        multiplex_id,
                        &entries,
                    );
                    heal_opt.map(|heal| {
                        heal.map_ok(|heal_stats| (heal_stats, entries))
                            .right_future()
                    })
                }
            })
            .collect();

        let last_batch_size = healing_futures.len();

        if last_batch_size == 0 {
            info!(ctx.logger(), "All caught up, nothing to do");
            return Ok((false, 0));
        }

        info!(
            ctx.logger(),
            "Found {} blobs to be healed... Doing it", last_batch_size
        );

        let heal_res: Vec<_> = stream::iter(healing_futures)
            .buffered(heal_concurrency)
            .try_collect()
            .await?;
        let (chunk_stats, processed_entries): (Vec<_>, Vec<_>) = heal_res.into_iter().unzip();
        let summary_stats: HealStats = chunk_stats.into_iter().sum();
        info!(
            ctx.logger(),
            "For {} blobs did {:?}",
            processed_entries.len(),
            summary_stats
        );
        let entries_to_remove = processed_entries.into_iter().flatten().collect();
        let deleted_rows =
            cleanup_after_healing(ctx, self.sync_queue.as_ref(), entries_to_remove).await?;
        Ok((unique_operation_keys == max_batch_size, deleted_rows))
    }
}

#[derive(Default, Debug, PartialEq)]
struct HealStats {
    queue_add: usize,
    queue_del: usize,
    put_success: usize,
    put_failure: usize,
}

impl Add for HealStats {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            queue_add: self.queue_add + other.queue_add,
            queue_del: self.queue_del + other.queue_del,
            put_success: self.put_success + other.put_success,
            put_failure: self.put_failure + other.put_failure,
        }
    }
}

impl Sum for HealStats {
    fn sum<I: Iterator<Item = HealStats>>(iter: I) -> HealStats {
        iter.fold(Default::default(), Add::add)
    }
}

/// Heal an individual blob. The `entries` are the blobstores which have successfully stored
/// this blob; we need to replicate them onto the remaining `blobstores`. If the blob is not
/// yet eligable (too young), then just return None, otherwise we return the healed entries
/// which have now been dealt with and can be deleted.
fn heal_blob<'out>(
    ctx: &'out CoreContext,
    sync_queue: &'out dyn BlobstoreSyncQueue,
    blobstores: &'out HashMap<BlobstoreId, Arc<dyn Blobstore>>,
    healing_deadline: DateTime,
    key: String,
    multiplex_id: MultiplexId,
    entries: &[BlobstoreSyncQueueEntry],
) -> Option<impl Future<Output = Result<HealStats>> + 'out> {
    if entries.len() == 0 {
        return None;
    }
    // This is needed as we load by key, and a given key may have entries both before and after
    // the deadline.  We leave the key rather than re-add to avoid entries always being too new.
    if !entries.iter().all(|e| e.timestamp < healing_deadline) {
        return None;
    }

    let num_entries: usize = entries.len();

    let operation_key = entries[0].operation_key.clone();

    let (seen_blobstores, unknown_seen_blobstores): (HashSet<_>, HashSet<_>) =
        entries.iter().partition_map(|entry| {
            let id = entry.blobstore_id;
            if blobstores.contains_key(&id) {
                Either::Left(id)
            } else {
                Either::Right(id)
            }
        });

    let num_unknown_entries: usize = unknown_seen_blobstores.len();

    if !unknown_seen_blobstores.is_empty() {
        warn!(
            ctx.logger(),
            "Ignoring unknown blobstores {:?} for key {}", unknown_seen_blobstores, key
        );
    }

    let mut stores_to_heal: HashSet<BlobstoreId> = blobstores
        .iter()
        .filter_map(|(key, _)| {
            if seen_blobstores.contains(key) {
                None
            } else {
                Some(key.clone())
            }
        })
        .collect();

    if stores_to_heal.is_empty() || seen_blobstores.is_empty() {
        // All blobstores have been synchronized or all are unknown to be requeued
        return Some(
            async move {
                if !unknown_seen_blobstores.is_empty() {
                    requeue_partial_heal(
                        ctx,
                        sync_queue,
                        &key,
                        unknown_seen_blobstores,
                        multiplex_id,
                        operation_key,
                    )
                    .await?;
                }
                Ok(HealStats {
                    queue_del: num_entries,
                    queue_add: num_unknown_entries,
                    put_success: 0,
                    put_failure: 0,
                })
            }
            .left_future(),
        );
    }

    let heal_future = async move {
        let fetch_data = fetch_blob(ctx, &blobstores, &key, &seen_blobstores).await?;
        if !fetch_data.missing_sources.is_empty() {
            warn!(
                ctx.logger(),
                "Source Blobstores {:?} of {:?} returned None even though they \
                 should contain data",
                fetch_data.missing_sources,
                seen_blobstores
            );
            for bid in fetch_data.missing_sources.clone() {
                stores_to_heal.insert(bid);
            }
        }

        // If any puts fail make sure we put a good source blobstore_id for that blob
        // back on the queue
        let heal_results = {
            let blob = &fetch_data.blob;
            let key = &key;
            join_all(stores_to_heal.into_iter().map(|bid| async move {
                let blobstore = blobstores
                    .get(&bid)
                    .expect("stores_to_heal contains unknown blobstore?");
                let result = blobstore.put(ctx.clone(), key.clone(), blob.clone()).await;
                (bid, result.is_ok())
            }))
            .await
        };
        let (mut healed_stores, mut unhealed_stores): (HashSet<_>, Vec<_>) =
            heal_results.into_iter().partition_map(|(id, put_ok)| {
                if put_ok {
                    Either::Left(id)
                } else {
                    Either::Right(id)
                }
            });

        if !unhealed_stores.is_empty() || !unknown_seen_blobstores.is_empty() {
            // Add good_sources to the healed_stores as we should write all
            // known good blobstores so that the stores_to_heal logic run on read
            // has the full data for the blobstore_key
            //
            // This also ensures we requeue at least one good source store in the case
            // where all heal puts fail
            for b in fetch_data.good_sources {
                healed_stores.insert(b);
            }

            let heal_stats = HealStats {
                queue_del: num_entries,
                queue_add: healed_stores.len() + num_unknown_entries,
                put_success: healed_stores.len(),
                put_failure: unhealed_stores.len(),
            };

            // Add unknown stores to queue as well so we try them later
            for b in unknown_seen_blobstores {
                healed_stores.insert(b);
            }
            unhealed_stores.sort();
            warn!(
                ctx.logger(),
                "Adding source blobstores {:?} to the queue so that failed \
                     destination blob stores {:?} will be retried later for {:?}",
                healed_stores.iter().sorted().collect::<Vec<_>>(),
                unhealed_stores,
                key,
            );
            requeue_partial_heal(
                ctx,
                sync_queue,
                &key,
                healed_stores,
                multiplex_id,
                operation_key,
            )
            .await?;
            Ok(heal_stats)
        } else {
            let heal_stats = HealStats {
                queue_del: num_entries,
                queue_add: num_unknown_entries,
                put_success: healed_stores.len(),
                put_failure: unhealed_stores.len(),
            };
            Ok(heal_stats)
        }
    };

    Some(heal_future.right_future())
}

struct FetchData {
    blob: BlobstoreBytes,
    good_sources: Vec<BlobstoreId>,
    missing_sources: Vec<BlobstoreId>,
}

/// Fetch a blob by `key` from one of the `seen_blobstores`. This tries them one at at time
/// sequentially, returning the known good store plus those found missing, or an error
async fn fetch_blob(
    ctx: &CoreContext,
    blobstores: &HashMap<BlobstoreId, Arc<dyn Blobstore>>,
    key: &str,
    seen_blobstores: &HashSet<BlobstoreId>,
) -> Result<FetchData> {
    let err_context = format!(
        "While fetching blob '{}', seen in blobstores: {:?}",
        key, seen_blobstores
    );

    let get_res = join_all(seen_blobstores.iter().map(|bid| async move {
        let blobstore = blobstores
            .get(bid)
            .expect("blobstores_to_fetch contains only existing blobstores");
        let result = blobstore.get(ctx.clone(), key.to_string()).await;
        (bid, result)
    }))
    .await;
    let mut blob = None;
    let mut good_sources = vec![];
    let mut missing_sources = vec![];
    for (bid, r) in get_res {
        match r {
            Ok(Some(blob_data)) => {
                blob = Some(blob_data);
                good_sources.push(*bid);
            }
            Ok(None) => {
                missing_sources.push(*bid);
            }
            Err(e) => {
                warn!(
                    ctx.logger(),
                    "error when loading from store {:?}: {:?}", bid, e
                );
            }
        }
    }
    match blob {
        None => Err(Error::msg("None of the blobstores to fetch responded").context(err_context)),
        Some(blob_data) => Ok(FetchData {
            blob: blob_data.into(),
            good_sources,
            missing_sources,
        }),
    }
}

/// Removed healed entries from the queue.
async fn cleanup_after_healing(
    ctx: &CoreContext,
    sync_queue: &dyn BlobstoreSyncQueue,
    entries: Vec<BlobstoreSyncQueueEntry>,
) -> Result<u64> {
    let n = entries.len() as u64;
    info!(ctx.logger(), "Deleting {} actioned queue entries", n);
    sync_queue.del(ctx, &entries).await?;
    Ok(n)
}

/// Write new queue items with a populated source blobstore for unhealed entries
/// Uses a current timestamp so we'll get around to trying them again for the destination
/// blobstores eventually without getting stuck on them permanently.
/// Uses a new queue entry id so the delete of original entry is safe.
async fn requeue_partial_heal(
    ctx: &CoreContext,
    sync_queue: &dyn BlobstoreSyncQueue,
    blobstore_key: &str,
    source_blobstores: impl IntoIterator<Item = BlobstoreId>,
    multiplex_id: MultiplexId,
    operation_key: OperationKey,
) -> Result<()> {
    let timestamp = DateTime::now();
    let new_entries: Vec<_> = source_blobstores
        .into_iter()
        .map(|blobstore_id| BlobstoreSyncQueueEntry {
            blobstore_key: blobstore_key.to_string(),
            blobstore_id,
            multiplex_id,
            timestamp,
            operation_key: operation_key.clone(),
            id: None,
        })
        .collect();
    sync_queue.add_many(ctx, new_entries).await
}
