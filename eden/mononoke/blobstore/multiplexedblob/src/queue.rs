/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::base::{ErrorKind, MultiplexedBlobstoreBase, MultiplexedBlobstorePutHandler};
use anyhow::{Error, Result};
use async_trait::async_trait;
use blobstore::{
    Blobstore, BlobstoreGetData, BlobstoreIsPresent, BlobstorePutOps, OverwriteStatus, PutBehaviour,
};
use blobstore_stats::{add_completion_time, record_queue_stats, OperationType};
use blobstore_sync_queue::{BlobstoreSyncQueue, BlobstoreSyncQueueEntry, OperationKey};
use context::CoreContext;
use futures_stats::{FutureStats, TimedFutureExt};
use metaconfig_types::{BlobstoreId, MultiplexId};
use mononoke_types::{BlobstoreBytes, DateTime};
use scuba::value::{NullScubaValue, ScubaValue};
use scuba_ext::MononokeScubaSampleBuilder;
use std::fmt;
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::Arc;
use tunables::tunables;

const SYNC_QUEUE: &str = "mysql_sync_queue";
/// Special error for cases where some blobstores failed during get/is_present
/// call and some returned None/Absent.
const SOME_FAILED_OTHERS_NONE: &str = "some_failed_others_none";
/// Number of entries we've fetched from the queue trying to resolve
/// SOME_FAILED_OTHERS_NONE case.
const QUEUE_ENTRIES: &str = "queue_entries_count";
const KEY: &str = "key";
const OPERATION: &str = "operation";
const SUCCESS: &str = "success";
const ERROR: &str = "error";
/// Was the blob found during the get/is_present operations?
const BLOB_PRESENT: &str = "blob_present";

#[derive(Clone)]
pub struct MultiplexedBlobstore {
    pub(crate) blobstore: Arc<MultiplexedBlobstoreBase>,
    queue: Arc<dyn BlobstoreSyncQueue>,
    multiplex_scuba: MononokeScubaSampleBuilder,
}

impl MultiplexedBlobstore {
    pub fn new(
        multiplex_id: MultiplexId,
        blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
        write_mostly_blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
        minimum_successful_writes: NonZeroUsize,
        queue: Arc<dyn BlobstoreSyncQueue>,
        scuba: MononokeScubaSampleBuilder,
        mut multiplex_scuba: MononokeScubaSampleBuilder,
        scuba_sample_rate: NonZeroU64,
    ) -> Self {
        multiplex_scuba.add_common_server_data();
        let put_handler = Arc::new(QueueBlobstorePutHandler {
            queue: queue.clone(),
        });
        Self {
            blobstore: Arc::new(MultiplexedBlobstoreBase::new(
                multiplex_id,
                blobstores,
                write_mostly_blobstores,
                minimum_successful_writes,
                put_handler,
                scuba,
                scuba_sample_rate,
            )),
            queue,
            multiplex_scuba,
        }
    }
}

impl fmt::Display for MultiplexedBlobstore {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MultiplexedBlobstore[{}]", self.blobstore.as_ref())
    }
}

impl fmt::Debug for MultiplexedBlobstore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MultiplexedBlobstore")
            .field("base", &self.blobstore)
            .finish()
    }
}

struct QueueBlobstorePutHandler {
    queue: Arc<dyn BlobstoreSyncQueue>,
}

#[async_trait]
impl MultiplexedBlobstorePutHandler for QueueBlobstorePutHandler {
    async fn on_put<'out>(
        &'out self,
        ctx: &'out CoreContext,
        mut scuba: MononokeScubaSampleBuilder,
        blobstore_id: BlobstoreId,
        blobstore_type: String,
        multiplex_id: MultiplexId,
        operation_key: &'out OperationKey,
        key: &'out str,
        blob_size: Option<u64>,
    ) -> Result<()> {
        let (stats, result) = self
            .queue
            .add(
                ctx,
                BlobstoreSyncQueueEntry::new(
                    key.to_string(),
                    blobstore_id,
                    multiplex_id,
                    DateTime::now(),
                    operation_key.clone(),
                    blob_size,
                ),
            )
            .timed()
            .await;

        let mut ctx = ctx.clone();
        let pc = ctx.fork_perf_counters();
        record_queue_stats(
            &mut scuba,
            &pc,
            stats,
            result.as_ref(),
            &key,
            ctx.metadata().session_id().as_str(),
            OperationType::Put,
            Some(blobstore_id),
            blobstore_type,
            SYNC_QUEUE,
        );
        result
    }
}

fn log_scuba_common(
    mut ctx: CoreContext,
    scuba: &mut MononokeScubaSampleBuilder,
    key: &str,
    operation: OperationType,
) {
    let pc = ctx.fork_perf_counters();
    pc.insert_nonzero_perf_counters(scuba);

    scuba.add(KEY, key);
    scuba.add(OPERATION, operation);
}

fn log_scuba_outcome(
    ctx: &CoreContext,
    scuba: &mut MononokeScubaSampleBuilder,
    stats: FutureStats,
    blob_present: Result<Option<bool>, &Error>,
) {
    add_completion_time(scuba, ctx.metadata().session_id().as_str(), stats);
    match blob_present {
        Err(error) => {
            scuba.unsampled();
            scuba.add(ERROR, format!("{:#}", error)).add(SUCCESS, false);
        }
        Ok(outcome) => {
            let outcome = outcome.map_or(ScubaValue::Null(NullScubaValue::Normal), |v| {
                ScubaValue::from(v)
            });
            scuba.add(BLOB_PRESENT, outcome).add(SUCCESS, true);
        }
    }
    scuba.log();
}

#[async_trait]
impl Blobstore for MultiplexedBlobstore {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let mut scuba = self.multiplex_scuba.clone();
        log_scuba_common(ctx.clone(), &mut scuba, key, OperationType::Get);

        let (stats, result) = async {
            let result = self.blobstore.get(ctx, key).await;

            match result {
                Ok(value) => Ok(value),
                Err(error) => {
                    if let Some(ErrorKind::SomeFailedOthersNone(er)) = error.downcast_ref() {
                        scuba.add(SOME_FAILED_OTHERS_NONE, format!("{:?}", er));

                        if !tunables().get_multiplex_blobstore_get_do_queue_lookup() {
                            // trust the first lookup, don't check the sync-queue and return None
                            return Ok(None);
                        }

                        // This means that some underlying blobstore returned error, and
                        // other return None. To distinguish incomplete sync from true-none we
                        // check synchronization queue. If it does not contain entries with this key
                        // it means it is true-none otherwise, only replica containing key has
                        // failed and we need to return error.
                        let entries = self.queue.get(ctx, key).await?;
                        scuba.add(QUEUE_ENTRIES, entries.len());

                        if entries.is_empty() {
                            Ok(None)
                        } else {
                            // Oh boy. If we found this on the queue but we didn't find it in the
                            // blobstores, it's possible that the content got written to the blobstore in
                            // the meantime. To account for this ... we have to check again.
                            self.blobstore.get(ctx, key).await
                        }
                    } else {
                        return Err(error);
                    }
                }
            }
        }
        .timed()
        .await;

        let outcome = result
            .as_ref()
            .map(|mb_blob| Some(mb_blob.as_ref().map_or(false, |_v| true)));
        log_scuba_outcome(ctx, &mut scuba, stats, outcome);

        result
    }

    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        let mut scuba = self.multiplex_scuba.clone();
        log_scuba_common(ctx.clone(), &mut scuba, &key, OperationType::Put);

        let (stats, result) = self.blobstore.put(ctx, key, value).timed().await;
        log_scuba_outcome(ctx, &mut scuba, stats, Ok(None));

        result
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        let mut scuba = self.multiplex_scuba.clone();
        log_scuba_common(ctx.clone(), &mut scuba, key, OperationType::IsPresent);

        let (stats, result) = async {
            let result = self.blobstore.is_present(ctx, key).await?;
            if !tunables().get_multiplex_blobstore_is_present_do_queue_lookup() {
                // trust the first lookup, don't check the sync-queue
                return Ok(result);
            }

            match &result {
                BlobstoreIsPresent::Present | BlobstoreIsPresent::Absent => Ok(result),
                BlobstoreIsPresent::ProbablyNotPresent(er) => {
                    scuba.add(SOME_FAILED_OTHERS_NONE, format!("{:#}", er));
                    // If a subset of blobstores failed, then we go to the queue. This is a way to
                    // "break the tie" if we had at least one blobstore that said the content didn't
                    // exist but the others failed to give a response: if any of those failing
                    // blobstores has the content, then it *must* be on the queue (it cannot have been
                    // pruned yet because if it was, then it would be in the blobstore that succeeded).
                    let entries = self.queue.get(ctx, key).await?;
                    scuba.add(QUEUE_ENTRIES, entries.len());

                    if entries.is_empty() {
                        Ok(BlobstoreIsPresent::Absent)
                    } else {
                        // Oh boy. If we found this on the queue but we didn't find it in the
                        // blobstores, it's possible that the content got written to the blobstore in
                        // the meantime. To account for this ... we have to check again.
                        self.blobstore.is_present(ctx, key).await
                    }
                }
            }
        }
        .timed()
        .await;


        let outcome = result.as_ref().map(|is_present| match is_present {
            BlobstoreIsPresent::Present => Some(true),
            BlobstoreIsPresent::Absent => Some(false),
            BlobstoreIsPresent::ProbablyNotPresent(_) => None,
        });
        log_scuba_outcome(ctx, &mut scuba, stats, outcome);

        result
    }
}

#[async_trait]
impl BlobstorePutOps for MultiplexedBlobstore {
    async fn put_explicit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        self.blobstore
            .put_explicit(ctx, key, value, put_behaviour)
            .await
    }

    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        self.blobstore.put_with_status(ctx, key, value).await
    }
}
