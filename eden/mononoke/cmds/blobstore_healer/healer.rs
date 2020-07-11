/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobstore::Blobstore;
use blobstore_sync_queue::{BlobstoreSyncQueue, BlobstoreSyncQueueEntry, OperationKey};
use cloned::cloned;
use context::CoreContext;
use failure_ext::FutureFailureErrorExt;
use futures::future::TryFutureExt;
use futures_ext::FutureExt;
use futures_old::{self, future::join_all, prelude::*};
use itertools::{Either, Itertools};
use metaconfig_types::{BlobstoreId, MultiplexId};
use mononoke_types::{BlobstoreBytes, DateTime};
use scuba_ext::ScubaSampleBuilderExt;
use slog::{info, warn};
use std::collections::{HashMap, HashSet};
use std::iter::Sum;
use std::ops::Add;
use std::sync::Arc;

#[cfg(test)]
mod tests;

pub struct Healer {
    blobstore_sync_queue_limit: usize,
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
            heal_concurrency,
            sync_queue,
            blobstores,
            multiplex_id,
            blobstore_key_like,
            drain_only,
        }
    }

    /// Heal one batch of entries. It selects a set of entries which are not too young (bounded
    /// by healing_deadline) up to `blobstore_sync_queue_limit` at once.
    /// Returns a tuple:
    /// - first item indicates whether a full batch was fetcehd
    /// - second item shows how many rows were deleted from the DB
    pub fn heal(
        &self,
        ctx: CoreContext,
        healing_deadline: DateTime,
    ) -> impl Future<Item = (bool, u64), Error = Error> {
        cloned!(
            self.blobstore_sync_queue_limit,
            self.sync_queue,
            self.blobstores,
        );

        let max_batch_size = self.blobstore_sync_queue_limit;
        let heal_concurrency = self.heal_concurrency;
        let drain_only = self.drain_only;
        let multiplex_id = self.multiplex_id;

        sync_queue
            .iter(
                ctx.clone(),
                self.blobstore_key_like.clone(),
                multiplex_id,
                healing_deadline.clone(),
                blobstore_sync_queue_limit,
            )
            .compat()
            .and_then(move |queue_entries: Vec<BlobstoreSyncQueueEntry>| {
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
                        cloned!(ctx, sync_queue, blobstores, healing_deadline);
                        let entries: Vec<_> = entries.collect();
                        if drain_only {
                            Some(
                                futures_old::future::ok((
                                    HealStats {
                                        queue_del: entries.len(),
                                        queue_add: 0,
                                        put_success: 0,
                                        put_failure: 0,
                                    },
                                    entries,
                                ))
                                .left_future(),
                            )
                        } else {
                            let heal_opt = heal_blob(
                                ctx,
                                sync_queue,
                                blobstores,
                                healing_deadline,
                                key,
                                multiplex_id,
                                &entries,
                            );
                            heal_opt.map(|fut| {
                                fut.map(|heal_stats| (heal_stats, entries)).right_future()
                            })
                        }
                    })
                    .collect();

                let last_batch_size = healing_futures.len();

                if last_batch_size == 0 {
                    info!(ctx.logger(), "All caught up, nothing to do");
                    return futures_old::future::ok((false, 0)).left_future();
                }

                info!(
                    ctx.logger(),
                    "Found {} blobs to be healed... Doing it", last_batch_size
                );

                futures_old::stream::iter_ok(healing_futures)
                    .buffered(heal_concurrency)
                    .collect()
                    .and_then(
                        move |heal_res: Vec<(HealStats, Vec<BlobstoreSyncQueueEntry>)>| {
                            let (chunk_stats, processed_entries): (Vec<_>, Vec<_>) =
                                heal_res.into_iter().unzip();
                            let summary_stats: HealStats = chunk_stats.into_iter().sum();
                            info!(
                                ctx.logger(),
                                "For {} blobs did {:?}",
                                processed_entries.len(),
                                summary_stats
                            );
                            let entries_to_remove =
                                processed_entries.into_iter().flatten().collect();
                            cleanup_after_healing(ctx, sync_queue, entries_to_remove).and_then(
                                move |deleted_rows| {
                                    return futures_old::future::ok((
                                        unique_operation_keys == max_batch_size,
                                        deleted_rows,
                                    ));
                                },
                            )
                        },
                    )
                    .right_future()
            })
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
fn heal_blob(
    ctx: CoreContext,
    sync_queue: Arc<dyn BlobstoreSyncQueue>,
    blobstores: Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
    healing_deadline: DateTime,
    key: String,
    multiplex_id: MultiplexId,
    entries: &[BlobstoreSyncQueueEntry],
) -> Option<impl Future<Item = HealStats, Error = Error>> {
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
            if unknown_seen_blobstores.is_empty() {
                futures_old::future::ok(()).left_future()
            } else {
                requeue_partial_heal(
                    ctx,
                    sync_queue,
                    key,
                    unknown_seen_blobstores,
                    multiplex_id,
                    operation_key,
                )
                .right_future()
            }
            .map(move |()| HealStats {
                queue_del: num_entries,
                queue_add: num_unknown_entries,
                put_success: 0,
                put_failure: 0,
            })
            .left_future(),
        );
    }

    let heal_future = fetch_blob(
        ctx.clone(),
        blobstores.clone(),
        key.clone(),
        seen_blobstores.clone(),
    )
    .and_then(move |fetch_data| {
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

        let heal_puts: Vec<_> = stores_to_heal
            .into_iter()
            .map(|bid| {
                let blobstore = blobstores
                    .get(&bid)
                    .expect("stores_to_heal contains unknown blobstore?");
                blobstore
                    .put(ctx.clone(), key.clone(), fetch_data.blob.clone())
                    .compat()
                    .then(move |result| Ok((bid, result.is_ok())))
            })
            .collect();

        // If any puts fail make sure we put a good source blobstore_id for that blob
        // back on the queue
        join_all(heal_puts).and_then(move |heal_results| {
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
                    key,
                    healed_stores,
                    multiplex_id,
                    operation_key,
                )
                .map(|()| heal_stats)
                .left_future()
            } else {
                let heal_stats = HealStats {
                    queue_del: num_entries,
                    queue_add: num_unknown_entries,
                    put_success: healed_stores.len(),
                    put_failure: unhealed_stores.len(),
                };
                futures_old::future::ok(heal_stats).right_future()
            }
        })
    });

    Some(heal_future.right_future())
}

struct FetchData {
    blob: BlobstoreBytes,
    good_sources: Vec<BlobstoreId>,
    missing_sources: Vec<BlobstoreId>,
}

/// Fetch a blob by `key` from one of the `seen_blobstores`. This tries them one at at time
/// sequentially, returning the known good store plus those found missing, or an error
fn fetch_blob(
    ctx: CoreContext,
    blobstores: Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
    key: String,
    seen_blobstores: HashSet<BlobstoreId>,
) -> impl Future<Item = FetchData, Error = Error> {
    let err_context = format!(
        "While fetching blob '{}', seen in blobstores: {:?}",
        key, seen_blobstores
    );

    let get_futs: Vec<_> = seen_blobstores
        .iter()
        .cloned()
        .map(|bid| {
            let blobstore = blobstores
                .get(&bid)
                .expect("blobstores_to_fetch contains only existing blobstores");
            blobstore
                .get(ctx.clone(), key.clone())
                .compat()
                .then(move |result| Ok((bid, result)))
        })
        .collect();

    join_all(get_futs)
        .and_then(move |get_res| {
            let mut blob = None;
            let mut good_sources = vec![];
            let mut missing_sources = vec![];
            for (bid, r) in get_res {
                match r {
                    Ok(Some(blob_data)) => {
                        blob = Some(blob_data);
                        good_sources.push(bid);
                    }
                    Ok(None) => {
                        missing_sources.push(bid);
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
                None => futures_old::future::err(Error::msg(
                    "None of the blobstores to fetch responded",
                )),
                Some(blob_data) => futures_old::future::ok(FetchData {
                    blob: blob_data.into(),
                    good_sources,
                    missing_sources,
                }),
            }
        })
        .context(err_context)
        .from_err()
}

/// Removed healed entries from the queue.
fn cleanup_after_healing(
    ctx: CoreContext,
    sync_queue: Arc<dyn BlobstoreSyncQueue>,
    entries: Vec<BlobstoreSyncQueueEntry>,
) -> impl Future<Item = u64, Error = Error> {
    let n = entries.len() as u64;
    info!(ctx.logger(), "Deleting {} actioned queue entries", n);
    sync_queue.del(ctx, entries).compat().map(move |_| n)
}

/// Write new queue items with a populated source blobstore for unhealed entries
/// Uses a current timestamp so we'll get around to trying them again for the destination
/// blobstores eventually without getting stuck on them permanently.
/// Uses a new queue entry id so the delete of original entry is safe.
fn requeue_partial_heal(
    ctx: CoreContext,
    sync_queue: Arc<dyn BlobstoreSyncQueue>,
    blobstore_key: String,
    source_blobstores: impl IntoIterator<Item = BlobstoreId>,
    multiplex_id: MultiplexId,
    operation_key: OperationKey,
) -> impl Future<Item = (), Error = Error> {
    let timestamp = DateTime::now();
    let new_entries: Vec<_> = source_blobstores
        .into_iter()
        .map(|blobstore_id| {
            cloned!(blobstore_key, timestamp);
            BlobstoreSyncQueueEntry {
                blobstore_key,
                blobstore_id,
                multiplex_id,
                timestamp,
                operation_key: operation_key.clone(),
                id: None,
            }
        })
        .collect();
    sync_queue
        .add_many(ctx, Box::new(new_entries.into_iter()))
        .compat()
}
