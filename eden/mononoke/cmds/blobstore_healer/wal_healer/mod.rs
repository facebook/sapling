/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore_sync_queue::BlobstoreWal;
use blobstore_sync_queue::BlobstoreWalEntry;
use chrono::Duration as ChronoDuration;
use cloned::cloned;
use context::CoreContext;
use futures::future::join_all;
use futures::stream;
use futures::FutureExt;
use futures::StreamExt;
use futures_03_ext::BufferedParams;
use futures_03_ext::FbStreamExt;
use itertools::Itertools;
use metaconfig_types::BlobstoreId;
use metaconfig_types::MultiplexId;
use mononoke_types::BlobstoreBytes;
use mononoke_types::DateTime;
use mononoke_types::Timestamp;
use rand::thread_rng;
use rand::Rng;
use slog::info;
use slog::warn;

use crate::healer::HealResult;
use crate::healer::Healer;
use crate::healer::DEFAULT_BLOB_SIZE_BYTES;
use crate::healer::MAX_FETCH_FAILURE_DELAY;
use crate::healer::MIN_FETCH_FAILURE_DELAY;

#[cfg(test)]
mod tests;

/// How many times to put a blob back in the queue
/// if it couldn't be found.
const MAX_WAL_RETRIES: u32 = 20;

pub struct WalHealer {
    /// The amount of entries healer processes in one go.
    batch_size: usize,
    /// Dynamic batch size, that helps to recover in case of failures.
    current_fetch_size: AtomicUsize,
    /// The buffered params are specified in order to balance our concurrent and parallel
    /// blobs' healing according to their sizes.
    buffered_params: BufferedParams,
    /// Write-ahead log that is used by multiplexed storage to synchronize the data.
    wal: Arc<dyn BlobstoreWal>,
    /// The blob storages healer operates on.
    blobstores: Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
    /// Multiplex configuration id.
    multiplex_id: MultiplexId,
    #[allow(dead_code)]
    /// Optional pattern for fetching specific keys in SQL LIKE format.
    blobstore_key_like: Option<String>,
    #[allow(dead_code)]
    /// Drain the queue without healing. Use with caution.
    // TODO: support this mode
    drain_only: bool,
}

impl WalHealer {
    pub fn new(
        batch_size: usize,
        buffered_params: BufferedParams,
        wal: Arc<dyn BlobstoreWal>,
        blobstores: Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
        multiplex_id: MultiplexId,
        blobstore_key_like: Option<String>,
        drain_only: bool,
    ) -> Self {
        Self {
            batch_size,
            current_fetch_size: AtomicUsize::new(batch_size),
            buffered_params,
            wal,
            blobstores,
            multiplex_id,
            blobstore_key_like,
            drain_only,
        }
    }

    async fn fetch_entries(
        &self,
        ctx: &CoreContext,
        older_than: Timestamp,
    ) -> Result<(usize, Vec<BlobstoreWalEntry>)> {
        let mut fetch_size = self.current_fetch_size.load(Ordering::Relaxed);
        loop {
            match self
                .wal
                .read(ctx, &self.multiplex_id, &older_than, fetch_size)
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
                            self.batch_size.min(fetch_size + fetch_size / 10 + 1)
                        } else {
                            fetch_size
                        };

                    self.current_fetch_size
                        .store(new_fetch_size, Ordering::Relaxed);

                    return Ok((fetch_size, queue_entries));
                }
                Err(e) => {
                    // Error, so fall in size fast
                    let new_fetch_size = fetch_size / 2;
                    warn!(
                        ctx.logger(),
                        "Failed to read full batch from th WAL, failing in batch size: old {}, new {}",
                        fetch_size,
                        new_fetch_size
                    );

                    if new_fetch_size < 1 {
                        return Err(e);
                    }

                    fetch_size = new_fetch_size;
                    let delay =
                        thread_rng().gen_range(MIN_FETCH_FAILURE_DELAY..MAX_FETCH_FAILURE_DELAY);
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    pub async fn heal_impl(
        &self,
        ctx: &CoreContext,
        minimum_age: ChronoDuration,
    ) -> Result<HealResult> {
        let now = DateTime::now().into_chrono();
        let older_than = DateTime::new(now - minimum_age);
        let (batch_size, queue_entries) = self.fetch_entries(ctx, older_than.into()).await?;

        // all entries in the queue correspond to the different put operations
        let unique_puts = queue_entries.len();
        info!(
            ctx.logger(),
            "Fetched {} distinct put operations", unique_puts
        );

        if unique_puts == 0 {
            info!(ctx.logger(), "All caught up, nothing to do");
            return Ok(HealResult {
                processed_full_batch: false,
                processed_rows: 0,
            });
        }

        let unique_blobstore_keys = queue_entries
            .iter()
            .unique_by(|entry| entry.blobstore_key.clone())
            .into_iter()
            .count();

        let healing_futures: Vec<(_, u64)> = queue_entries
            .into_iter()
            .sorted_by_key(|entry| entry.blobstore_key.clone())
            .group_by(|entry| entry.blobstore_key.clone())
            .into_iter()
            .map(|(key, entries)| {
                let entries: Vec<_> = entries.into_iter().collect();
                let healing_weight = entries
                    .iter()
                    .find_map(|entry| entry.blob_size)
                    .unwrap_or(DEFAULT_BLOB_SIZE_BYTES);

                let fut =
                    heal_blob(ctx, self.blobstores.clone(), key).map(|outcome| (outcome, entries));

                (fut.boxed(), healing_weight)
            })
            .collect();

        info!(
            ctx.logger(),
            "Found {} blobs to be healed... Doing it with weight limit {}, max concurrency: {}",
            healing_futures.len(),
            self.buffered_params.weight_limit,
            self.buffered_params.buffer_size,
        );

        let heal_res: Vec<_> = stream::iter(healing_futures)
            .buffered_weight_limited(self.buffered_params)
            .collect()
            .await;

        let mut healthy_blobs = 0;
        let mut to_enqueue = vec![];
        let mut missing_blobs = vec![];
        let processed_entries: Vec<_> = heal_res
            .into_iter()
            .flat_map(|(outcome, entries)| {
                match outcome {
                    HealBlobOutcome::Healthy => {
                        // blob was healthy and did not require to be healed
                        healthy_blobs += 1;
                    }
                    HealBlobOutcome::Healed => {} // all done!
                    HealBlobOutcome::MissingBlob(key) => {
                        // the blob is missing, we'll requeue the entries in case the blob
                        // was not propagated yet to the blobstores
                        //
                        // TODO: log missing blobs to scuba
                        let retries = entries.first().map_or(0, |e| e.retry_count);
                        warn!(
                            ctx.logger(),
                            "Missing blob detected: key {} ({} retries so far)", key, retries
                        );
                        to_enqueue.push(
                            entries
                                .iter()
                                .filter(|e| e.retry_count < MAX_WAL_RETRIES)
                                .cloned()
                                .update(BlobstoreWalEntry::increment_retry)
                                .collect(),
                        );
                        missing_blobs.push(key);
                    }
                    HealBlobOutcome::MissingBlobstores(key, blobstores) => {
                        info!(
                            ctx.logger(),
                            "Couldn't heal blob {} in these blobstores: {:?}", key, blobstores
                        );
                        to_enqueue.push(entries.clone());
                    }
                }
                entries
            })
            .collect();

        let blobs_not_healed = to_enqueue.len();
        let to_enqueue: Vec<_> = to_enqueue.into_iter().flatten().collect();

        info!(
            ctx.logger(),
            "For {} processed entries and {} blobstore keys: healthy blobs {}, healed blobs {}, failed to heal {}, missing blobs {}",
            processed_entries.len(),
            unique_blobstore_keys,
            healthy_blobs,
            unique_blobstore_keys - blobs_not_healed - healthy_blobs,
            blobs_not_healed,
            missing_blobs.len()
        );

        enqueue_entries(ctx, self.wal.as_ref(), to_enqueue).await?;
        let deleted_entries =
            cleanup_after_healing(ctx, self.wal.as_ref(), processed_entries).await?;

        Ok(HealResult {
            processed_full_batch: unique_puts == batch_size,
            processed_rows: deleted_entries,
        })
    }
}

#[async_trait]
impl Healer for WalHealer {
    async fn heal(&self, ctx: &CoreContext, minimum_age: ChronoDuration) -> Result<HealResult> {
        self.heal_impl(ctx, minimum_age).await
    }
}

struct HealingBlob {
    blob_get_data: Option<BlobstoreGetData>,
    missing_blobstores: HashMap<BlobstoreId, Arc<dyn Blobstore>>,
    failing_blobstores: Vec<BlobstoreId>,
}

enum HealBlobOutcome {
    // The blob was found in all of the blobstores and did not require healing
    Healthy,
    // The blobstores missing the blob were successfully healed
    Healed,
    // The blob was not found in any of the blobstores
    MissingBlob(String),
    // The blobstores that don't have the blob and could not be healed. These
    // are mainly the blobstores that failed on healing write.
    MissingBlobstores(String, HashSet<BlobstoreId>),
}

async fn heal_blob(
    ctx: &CoreContext,
    blobstores: Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
    key: String,
) -> HealBlobOutcome {
    let HealingBlob {
        blob_get_data,
        missing_blobstores,
        failing_blobstores,
    } = fetch_blob(ctx, blobstores, &key).await;

    let blob = if let Some(get_data) = blob_get_data {
        // we successfully found the blob in at least one of the blobstores
        if missing_blobstores.is_empty() && failing_blobstores.is_empty() {
            // the blob is well and healthy across all the blobstores
            return HealBlobOutcome::Healthy;
        }

        get_data.into_bytes()
    } else {
        // We couldn't find the blob in the stores: it's either missing,
        // or the blobstores with the blob are failing
        if failing_blobstores.is_empty() {
            // no blobstore failed, the blob is missing
            return HealBlobOutcome::MissingBlob(key);
        }
        return HealBlobOutcome::MissingBlobstores(
            key,
            missing_blobstores.into_iter().map(|(bid, _)| bid).collect(),
        );
    };

    let missing = try_heal(ctx, key.clone(), blob, missing_blobstores).await;
    if missing.is_empty() {
        return HealBlobOutcome::Healed;
    }
    HealBlobOutcome::MissingBlobstores(key, missing)
}

async fn fetch_blob(
    ctx: &CoreContext,
    blobstores: Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
    key: &str,
) -> HealingBlob {
    let gets = join_all(blobstores.iter().map(|(bid, blobstore)| async move {
        let result = blobstore.get(ctx, key).await;
        (bid, result)
    }))
    .await;

    let mut blob = None;
    let mut failing_blobstores = vec![];
    let missing_storages_ids: HashSet<_> = gets
        .into_iter()
        .filter_map(|(bid, result)| match result {
            Ok(None) => Some(bid),
            Ok(Some(get_data)) => {
                if blob.is_none() {
                    blob = Some(get_data);
                }
                None
            }
            Err(_) => {
                failing_blobstores.push(bid.clone());
                Some(bid)
            }
        })
        .collect();
    let missing_blobstores = blobstores
        .iter()
        .filter_map(|(bid, blobstore)| {
            if missing_storages_ids.contains(bid) {
                Some((bid.clone(), blobstore.clone()))
            } else {
                None
            }
        })
        .collect();

    HealingBlob {
        blob_get_data: blob,
        missing_blobstores,
        failing_blobstores,
    }
}

async fn try_heal(
    ctx: &CoreContext,
    key: String,
    blob: BlobstoreBytes,
    missing_blobstores: HashMap<BlobstoreId, Arc<dyn Blobstore>>,
) -> HashSet<BlobstoreId> {
    let puts = missing_blobstores.into_iter().map(|(bid, blobstore)| {
        cloned!(key, blob);
        async move {
            let put_result = blobstore.put(ctx, key, blob).await;
            (bid, put_result)
        }
    });
    let results = join_all(puts).await;

    results
        .into_iter()
        .filter_map(
            |(bid, put_result)| {
                if put_result.is_ok() { None } else { Some(bid) }
            },
        )
        .collect()
}

async fn enqueue_entries(
    ctx: &CoreContext,
    wal: &dyn BlobstoreWal,
    entries: Vec<BlobstoreWalEntry>,
) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    info!(
        ctx.logger(),
        "Requeueing {} queue entries for another healing attempt",
        entries.len()
    );
    let new_entries = entries
        .into_iter()
        .map(|entry| {
            let BlobstoreWalEntry {
                blobstore_key,
                multiplex_id,
                operation_key,
                blob_size,
                ..
            } = entry;

            BlobstoreWalEntry::new(
                blobstore_key,
                multiplex_id,
                Timestamp::now(),
                operation_key,
                blob_size,
            )
        })
        .collect();

    wal.log_many(ctx, new_entries).await
}

/// Removed healed entries from the queue.
async fn cleanup_after_healing(
    ctx: &CoreContext,
    wal: &dyn BlobstoreWal,
    entries: Vec<BlobstoreWalEntry>,
) -> Result<u64> {
    let n = entries.len() as u64;
    info!(ctx.logger(), "Deleting {} actioned queue entries", n);
    wal.delete(ctx, &entries).await?;
    Ok(n)
}
