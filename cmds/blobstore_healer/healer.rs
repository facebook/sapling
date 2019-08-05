// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobstore::Blobstore;
use blobstore_sync_queue::{BlobstoreSyncQueue, BlobstoreSyncQueueEntry};
use chrono::Duration as ChronoDuration;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{err_msg, prelude::*};
use futures::{
    self,
    future::{join_all, loop_fn, Loop},
    prelude::*,
};
use futures_ext::FutureExt;
use itertools::{Either, Itertools};
use lazy_static::lazy_static;
use metaconfig_types::BlobstoreId;
use mononoke_types::{BlobstoreBytes, DateTime};
use slog::{info, warn, Logger};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

lazy_static! {
    /// Minimal age of entry to consider if it has to be healed
    static ref ENTRY_HEALING_MIN_AGE: ChronoDuration = ChronoDuration::minutes(2);
}

pub struct Healer {
    logger: Logger,
    blobstore_sync_queue_limit: usize,
    sync_queue: Arc<dyn BlobstoreSyncQueue>,
    blobstores: Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
    blobstore_key_like: Option<String>,
}

impl Healer {
    pub fn new(
        logger: Logger,
        blobstore_sync_queue_limit: usize,
        sync_queue: Arc<dyn BlobstoreSyncQueue>,
        blobstores: Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
        blobstore_key_like: Option<String>,
    ) -> Self {
        Self {
            logger,
            blobstore_sync_queue_limit,
            sync_queue,
            blobstores,
            blobstore_key_like,
        }
    }

    /// Heal one batch of entries. It selects a set of entries which are not too young (bounded
    /// by ENTRY_HEALING_MIN_AGE) up to `blobstore_sync_queue_limit` at once.
    pub fn heal(&self, ctx: CoreContext) -> impl Future<Item = (), Error = Error> {
        cloned!(
            self.logger,
            self.blobstore_sync_queue_limit,
            self.sync_queue,
            self.blobstores,
        );

        let now = DateTime::now().into_chrono();
        let healing_deadline = DateTime::new(now - *ENTRY_HEALING_MIN_AGE);

        sync_queue
            .iter(
                ctx.clone(),
                self.blobstore_key_like.clone(),
                healing_deadline.clone(),
                blobstore_sync_queue_limit,
            )
            .and_then(move |queue_entries: Vec<BlobstoreSyncQueueEntry>| {
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

    let mut missing_blobstores: HashSet<BlobstoreId> = blobstores
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
        seen_blobstores.clone(),
    )
    .and_then(
        move |(blob, known_good_blobstore, missing_source_blobstores)| {
            if !missing_source_blobstores.is_empty() {
                warn!(
                    ctx.logger(),
                    "Source Blobstores {:?} of {:?} returned None even though they\
                     should contain data",
                    missing_source_blobstores,
                    seen_blobstores
                );
                for bid in missing_source_blobstores {
                    missing_blobstores.insert(bid);
                }
            }

            let heal_puts: Vec<_> = missing_blobstores
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

            // If any puts fail make sure we put a good source blobstore_id for that blob
            // back on the queue
            join_all(heal_puts).and_then(move |heal_results| {
                let (mut healed_stores, unhealed_stores): (HashSet<_>, HashSet<_>) =
                    heal_results.into_iter().partition_map(|(id, put_ok)| {
                        if put_ok {
                            Either::Left(id)
                        } else {
                            Either::Right(id)
                        }
                    });
                if !unhealed_stores.is_empty() {
                    // Add known_good_blobstore to the healed_stores as we should write all
                    // known good blobstores so that the missing_blobstores logic run on read
                    // has the full data for the blobstore_key
                    //
                    // This also ensures we requeue at least one good source store in the case
                    // where all heal puts fail
                    healed_stores.insert(known_good_blobstore);
                    warn!(
                        ctx.logger(),
                        "Adding source blobstores {:?} to the queue so that failed \
                         destination blob stores {:?} will be retried later",
                        healed_stores,
                        unhealed_stores
                    );
                    requeue_partial_heal(ctx, sync_queue, key, healed_stores)
                        .map(|()| entries)
                        .left_future()
                } else {
                    futures::future::ok(entries).right_future()
                }
            })
        },
    );

    Some(heal_future.right_future())
}

/// Fetch a blob by `key` from one of the `seen_blobstores`. This tries them one at at time
/// sequentially, returning the known good store plus those found missing, or an error
fn fetch_blob(
    ctx: CoreContext,
    blobstores: Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
    key: String,
    seen_blobstores: HashSet<BlobstoreId>,
) -> impl Future<Item = (BlobstoreBytes, BlobstoreId, Vec<BlobstoreId>), Error = Error> {
    let blobstores_to_fetch: Vec<_> = seen_blobstores.iter().cloned().collect();
    let err_context = format!(
        "While fetching blob '{}', seen in blobstores: {:?}",
        key, seen_blobstores
    );
    let missing_blobstores: Vec<BlobstoreId> = vec![];

    loop_fn(
        (blobstores_to_fetch, missing_blobstores),
        move |(mut blobstores_to_fetch, mut missing_blobstores)| {
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
                    Err(_) => return Ok(Loop::Continue((blobstores_to_fetch, missing_blobstores))),
                    Ok(None) => {
                        missing_blobstores.push(bid);
                        return Ok(Loop::Continue((blobstores_to_fetch, missing_blobstores)));
                    }
                    Ok(Some(blob)) => Ok(Loop::Break((blob, bid, missing_blobstores))),
                })
                .right_future()
        },
    )
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

/// Write new queue items with a populated source blobstore for unhealed entries
/// Uses a current timestamp so we'll get around to trying them again for the destination
/// blobstores eventually without getting stuck on them permanently.
/// Uses a new queue entry id so the delete of original entry is safe.
fn requeue_partial_heal(
    ctx: CoreContext,
    sync_queue: Arc<dyn BlobstoreSyncQueue>,
    blobstore_key: String,
    source_blobstores: impl IntoIterator<Item = BlobstoreId>,
) -> impl Future<Item = (), Error = Error> {
    let timestamp = DateTime::now();

    join_all(source_blobstores.into_iter().map({
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
    .map(|_: Vec<()>| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use memblob::EagerMemblob;
    use std::iter::FromIterator;

    fn make_empty_stores(n: usize) -> (Vec<BlobstoreId>, HashMap<BlobstoreId, Arc<dyn Blobstore>>) {
        let mut test_bids = Vec::new();
        let mut test_stores = HashMap::new();
        for i in 0..n {
            test_bids.push(BlobstoreId::new(i as u64));
            let s: Arc<dyn Blobstore> = Arc::new(EagerMemblob::new());
            test_stores.insert(test_bids[i], s);
        }
        (test_bids, test_stores)
    }

    fn make_value(value: &str) -> BlobstoreBytes {
        BlobstoreBytes::from_bytes(value.as_bytes())
    }

    fn put_value(ctx: &CoreContext, store: Option<&Arc<dyn Blobstore>>, key: &str, value: &str) {
        store.map(|s| s.put(ctx.clone(), key.to_string(), make_value(value)));
    }

    #[test]
    fn fetch_blob_missing_all() {
        let ctx = CoreContext::test_mock();
        let (bids, stores) = make_empty_stores(3);
        put_value(&ctx, stores.get(&bids[0]), "dummyk", "dummyv");
        put_value(&ctx, stores.get(&bids[1]), "dummyk", "dummyv");
        put_value(&ctx, stores.get(&bids[2]), "dummyk", "dummyv");
        let fut = fetch_blob(
            ctx,
            Arc::new(stores),
            "specialk".to_string(),
            HashSet::from_iter(bids.into_iter()),
        );
        let r = fut.wait();
        let msg = r
            .err()
            .and_then(|e| e.as_fail().cause().map(|f| (format!("{}", f))));
        assert_eq!(
            Some("None of the blobstores to fetch responded".to_string()),
            msg
        );
    }

    #[test]
    fn fetch_blob_missing_none() {
        let ctx = CoreContext::test_mock();
        let (bids, stores) = make_empty_stores(3);
        put_value(&ctx, stores.get(&bids[0]), "specialk", "specialv");
        put_value(&ctx, stores.get(&bids[1]), "specialk", "specialv");
        put_value(&ctx, stores.get(&bids[2]), "specialk", "specialv");
        let fut = fetch_blob(
            ctx,
            Arc::new(stores),
            "specialk".to_string(),
            HashSet::from_iter(bids.into_iter()),
        );
        let r = fut.wait();
        let foundv = r.ok().unwrap().0;
        assert_eq!(make_value("specialv"), foundv);
    }

    // TODO enable as test once fetch_blob gives repeatable results in the some case
    // fn fetch_blob_missing_some() {
    //     let ctx = CoreContext::test_mock();
    //     let (bids, stores) = make_empty_stores(3);
    //     put_value(&ctx, stores.get(&bids[0]), "specialk", "specialv");
    //     put_value(&ctx, stores.get(&bids[1]), "dummyk", "dummyv");
    //     put_value(&ctx, stores.get(&bids[2]), "dummyk", "dummyv");
    //     let fut = fetch_blob(
    //         ctx,
    //         Arc::new(stores),
    //         "specialk".to_string(),
    //         HashSet::from_iter(bids.clone().into_iter()),
    //     );
    //     let r = fut.wait();
    //     let (foundblob, mut missing_bids) = r.ok().unwrap();
    //     assert_eq!(make_value("specialv"), foundblob);
    //     missing_bids.sort();
    //     assert_eq!(missing_bids, &bids[1..3]);
    // }
}
