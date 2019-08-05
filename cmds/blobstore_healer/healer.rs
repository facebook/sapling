// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::rate_limiter::RateLimiter;
use blobstore::Blobstore;
use blobstore_sync_queue::{BlobstoreSyncQueue, BlobstoreSyncQueueEntry};
use chrono::Duration as ChronoDuration;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{err_msg, format_err, prelude::*};
use futures::{
    self,
    future::{join_all, loop_fn, Loop},
    prelude::*,
};
use futures_ext::FutureExt;
use itertools::Itertools;
use lazy_static::lazy_static;
use metaconfig_types::BlobstoreId;
use mononoke_types::{BlobstoreBytes, DateTime};
use slog::{info, Logger};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

lazy_static! {
    /// Minimal age of entry to consider if it has to be healed
    static ref ENTRY_HEALING_MIN_AGE: ChronoDuration = ChronoDuration::minutes(2);
}

pub struct Healer {
    logger: Logger,
    blobstore_sync_queue_limit: usize,
    rate_limiter: RateLimiter,
    sync_queue: Arc<dyn BlobstoreSyncQueue>,
    blobstores: Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
}

impl Healer {
    pub fn new(
        logger: Logger,
        blobstore_sync_queue_limit: usize,
        rate_limiter: RateLimiter,
        sync_queue: Arc<dyn BlobstoreSyncQueue>,
        blobstores: Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
    ) -> Self {
        Self {
            logger,
            blobstore_sync_queue_limit,
            rate_limiter,
            sync_queue,
            blobstores,
        }
    }

    /// Heal one batch of entries. It selects a set of entries which are not too young (bounded
    /// by ENTRY_HEALING_MIN_AGE) up to `blobstore_sync_queue_limit` at once.
    pub fn heal(&self, ctx: CoreContext) -> impl Future<Item = (), Error = Error> {
        cloned!(
            self.logger,
            self.blobstore_sync_queue_limit,
            self.rate_limiter,
            self.sync_queue,
            self.blobstores,
        );

        let now = DateTime::now().into_chrono();
        let healing_deadline = DateTime::new(now - *ENTRY_HEALING_MIN_AGE);

        sync_queue
            .iter(
                ctx.clone(),
                healing_deadline.clone(),
                blobstore_sync_queue_limit,
            )
            .and_then(move |queue_entries: Vec<BlobstoreSyncQueueEntry>| {
                cloned!(rate_limiter);

                let healing_futures: Vec<_> = queue_entries
                    .into_iter()
                    .group_by(|entry| entry.blobstore_key.clone())
                    .into_iter()
                    .filter_map(|(key, entries)| {
                        cloned!(ctx, sync_queue, blobstores, healing_deadline);
                        heal_blob(
                            ctx,
                            sync_queue,
                            blobstores,
                            healing_deadline,
                            key,
                            entries.collect(),
                        )
                    })
                    .map(move |healing_future| rate_limiter.execute(healing_future))
                    .collect();

                info!(
                    logger,
                    "Found {} blobs to be healed... Doing it",
                    healing_futures.len()
                );

                futures::stream::futures_unordered(healing_futures)
                    .collect()
                    .and_then(move |cleaned_entries: Vec<Vec<BlobstoreSyncQueueEntry>>| {
                        let cleaned = cleaned_entries.into_iter().flatten().collect();
                        cleanup_after_healing(ctx, sync_queue, cleaned)
                    })
            })
    }
}

/// Heal an individual blob. The `entries` are the blobstores which have successfully stored
/// this blob; we need to replicate them onto the remaining `blobstores`. If the blob is not
/// yet eligable (too young), then just return None, otherwise we return the healed entries
/// which have now been dealt with.
fn heal_blob(
    ctx: CoreContext,
    sync_queue: Arc<dyn BlobstoreSyncQueue>,
    blobstores: Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
    healing_deadline: DateTime,
    key: String,
    entries: Vec<BlobstoreSyncQueueEntry>,
) -> Option<impl Future<Item = Vec<BlobstoreSyncQueueEntry>, Error = Error>> {
    let seen_blobstores: HashSet<_> = entries
        .iter()
        .filter_map(|entry| {
            let id = entry.blobstore_id;
            if blobstores.contains_key(&id) {
                Some(id)
            } else {
                None
            }
        })
        .collect();

    let missing_blobstores: HashSet<_> = blobstores
        .iter()
        .filter_map(|(key, _)| {
            if seen_blobstores.contains(key) {
                None
            } else {
                Some(key.clone())
            }
        })
        .collect();

    if missing_blobstores.is_empty() {
        // All blobstores have been synchronized
        return Some(futures::future::ok(entries).left_future());
    }

    if !entries
        .iter()
        .any(|entry| entry.timestamp < healing_deadline)
    {
        // The oldes entry is not old enough to be eligible for healing
        return None;
    }

    let heal_future = fetch_blob(
        ctx.clone(),
        blobstores.clone(),
        key.clone(),
        seen_blobstores,
    )
    .and_then(move |blob| {
        let heal_blobstores: Vec<_> = missing_blobstores
            .into_iter()
            .map(|bid| {
                let blobstore = blobstores
                    .get(&bid)
                    .expect("missing_blobstores contains unknown blobstore?");
                blobstore
                    .put(ctx.clone(), key.clone(), blob.clone())
                    .then(move |result| Ok((bid, result.is_ok())))
            })
            .collect();

        // XXX(jsgf) Don't really understand this. I'd expect it to filter the missing_blobstores
        // by put_ok, and then return only those (ie, leave the entries which didn't store
        // correctly in the queue). This logic seems to report success if everything was successful,
        // otherwise it re-puts the successes into the queue (via report_partial_heal), and returns
        // an empty "to be cleaned" vector.
        join_all(heal_blobstores).and_then(move |heal_results| {
            if heal_results.iter().all(|(_, put_ok)| *put_ok) {
                futures::future::ok(entries).left_future()
            } else {
                let healed_blobstores = heal_results
                    .into_iter()
                    .filter_map(|(id, put_ok)| Some(id).filter(|_| put_ok));
                report_partial_heal(ctx, sync_queue, key, healed_blobstores)
                    .map(|_| vec![])
                    .right_future()
            }
        })
    });

    Some(heal_future.right_future())
}

/// Fetch a blob by `key` from one of the `seen_blobstores`. This tries them one at at time
/// sequentially, until either it find the entry or it fails.
/// TODO: if one of the blobstores returns "not found" (None) rather than an error (or success),
/// we should add that blobstore to the missing set. (Currently it just fails, which will not
/// be recoverable.)
fn fetch_blob(
    ctx: CoreContext,
    blobstores: Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
    key: String,
    seen_blobstores: HashSet<BlobstoreId>,
) -> impl Future<Item = BlobstoreBytes, Error = Error> {
    let blobstores_to_fetch: Vec<_> = seen_blobstores.iter().cloned().collect();
    let err_context = format!(
        "While fetching blob '{}', seen in blobstores: {:?}",
        key, seen_blobstores
    );

    loop_fn(blobstores_to_fetch, move |mut blobstores_to_fetch| {
        let bid = match blobstores_to_fetch.pop() {
            None => {
                return Err(err_msg("None of the blobstores to fetch responded"))
                    .into_future()
                    .left_future();
            }
            Some(bid) => bid,
        };

        let blobstore = blobstores
            .get(&bid)
            .expect("blobstores_to_fetch contains only existing blobstores");

        blobstore
            .get(ctx.clone(), key.clone())
            .then(move |result| match result {
                Err(_) => return Ok(Loop::Continue(blobstores_to_fetch)),
                Ok(None) => {
                    return Err(format_err!(
                        "Blobstore {:?} returned None even though it should contain data",
                        bid
                    ));
                }
                Ok(Some(blob)) => Ok(Loop::Break(blob)),
            })
            .right_future()
    })
    .chain_err(err_context)
    .from_err()
}

/// Removed healed entries from the queue.
fn cleanup_after_healing(
    ctx: CoreContext,
    sync_queue: Arc<dyn BlobstoreSyncQueue>,
    entries: Vec<BlobstoreSyncQueueEntry>,
) -> impl Future<Item = (), Error = Error> {
    sync_queue.del(ctx, entries)
}

/// ??? Don't understand this. This is putting the entries we healed back into the queue?
fn report_partial_heal(
    ctx: CoreContext,
    sync_queue: Arc<dyn BlobstoreSyncQueue>,
    blobstore_key: String,
    healed_blobstores: impl IntoIterator<Item = BlobstoreId>,
) -> impl Future<Item = (), Error = Error> {
    let timestamp = DateTime::now();

    join_all(healed_blobstores.into_iter().map({
        move |blobstore_id| {
            cloned!(ctx, blobstore_key, timestamp);
            sync_queue.add(
                ctx,
                BlobstoreSyncQueueEntry {
                    blobstore_key,
                    blobstore_id,
                    timestamp,
                    id: None,
                },
            )
        }
    }))
    .map(|_| ())
}
