/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::base::ErrorKind;
use crate::base::MultiplexedBlobstoreBase;
use crate::base::MultiplexedBlobstorePutHandler;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::BlobstorePutOps;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use blobstore_stats::add_completion_time;
use blobstore_stats::record_queue_stats;
use blobstore_stats::OperationType;
use blobstore_sync_queue::BlobstoreSyncQueue;
use blobstore_sync_queue::BlobstoreSyncQueueEntry;
use blobstore_sync_queue::OperationKey;
use context::CoreContext;
use futures_stats::FutureStats;
use futures_stats::TimedFutureExt;
use metaconfig_types::BlobstoreId;
use metaconfig_types::MultiplexId;
use mononoke_types::BlobstoreBytes;
use mononoke_types::DateTime;
use scuba_ext::MononokeScubaSampleBuilder;
use std::fmt;
use std::num::NonZeroU64;
use std::num::NonZeroUsize;
use std::sync::Arc;
use tunables::tunables;

const SYNC_QUEUE: &str = "mysql_sync_queue";
/// Special error for cases where some blobstores failed during get/is_present
/// call and some returned None/Absent.
const SOME_FAILED_OTHERS_NONE: &str = "some_failed_others_none";
/// Number of entries we've fetched from the queue trying to resolve
/// SOME_FAILED_OTHERS_NONE case.
const QUEUE_ENTRIES: &str = "queue_entries_count";
const MULTIPLEX_ID: &str = "multiplex_id";
const KEY: &str = "key";
const OPERATION: &str = "operation";
const BLOB_SIZE: &str = "blob_size";
const SUCCESS: &str = "success";
const ERROR: &str = "error";
/// Was the blob found during the get/is_present operations?
const BLOB_PRESENT: &str = "blob_present";

#[derive(Clone)]
pub struct MultiplexedBlobstore {
    pub(crate) blobstore: Arc<MultiplexedBlobstoreBase>,
    queue: Arc<dyn BlobstoreSyncQueue>,
    multiplex_scuba: MononokeScubaSampleBuilder,
    scuba_sample_rate: NonZeroU64,
}

impl MultiplexedBlobstore {
    pub fn new(
        multiplex_id: MultiplexId,
        blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
        write_mostly_blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
        minimum_successful_writes: NonZeroUsize,
        not_present_read_quorum: NonZeroUsize,
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
                not_present_read_quorum,
                put_handler,
                scuba,
                scuba_sample_rate,
            )),
            queue,
            multiplex_scuba,
            scuba_sample_rate,
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
            key,
            ctx.metadata().session_id().as_str(),
            OperationType::Put,
            Some(blobstore_id),
            blobstore_type,
            SYNC_QUEUE,
        );
        result
    }
}

#[async_trait]
impl Blobstore for MultiplexedBlobstore {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let mut scuba = self.multiplex_scuba.clone();
        scuba.sampled(self.scuba_sample_rate);

        let (stats, result) = async {
            let result = self.blobstore.get(ctx, key).await;

            match result {
                Ok(value) => Ok(value),
                Err(error) => {
                    if let Some(ErrorKind::SomeFailedOthersNone(er)) = error.downcast_ref() {
                        scuba.unsampled();
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
                        Err(error)
                    }
                }
            }
        }
        .timed()
        .await;

        let multiplex_id = self.blobstore.multiplex_id();
        record_scuba_get(ctx, &mut scuba, multiplex_id, key, stats, &result);

        result
    }

    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        let size = value.len();
        let (stats, result) = self.blobstore.put(ctx, key.clone(), value).timed().await;

        let mut scuba = self.multiplex_scuba.clone();
        let multiplex_id = self.blobstore.multiplex_id();
        record_scuba_put(ctx, &mut scuba, multiplex_id, &key, size, stats, &result);

        result
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        let mut scuba = self.multiplex_scuba.clone();
        scuba.sampled(self.scuba_sample_rate);

        let (stats, result) = async {
            let result = self.blobstore.is_present(ctx, key).await?;
            if !tunables().get_multiplex_blobstore_is_present_do_queue_lookup() {
                // trust the first lookup, don't check the sync-queue
                return Ok(result);
            }

            match &result {
                BlobstoreIsPresent::Present | BlobstoreIsPresent::Absent => Ok(result),
                BlobstoreIsPresent::ProbablyNotPresent(er) => {
                    scuba.unsampled();
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

        let multiplex_id = self.blobstore.multiplex_id();
        record_scuba_is_present(ctx, &mut scuba, multiplex_id, key, stats, &result);

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

fn record_scuba_common(
    mut ctx: CoreContext,
    scuba: &mut MononokeScubaSampleBuilder,
    multiplex_id: &MultiplexId,
    key: &str,
    stats: FutureStats,
    operation: OperationType,
) {
    let pc = ctx.fork_perf_counters();
    pc.insert_nonzero_perf_counters(scuba);

    add_completion_time(scuba, ctx.metadata().session_id().as_str(), stats);

    scuba.add(KEY, key);
    scuba.add(OPERATION, operation);
    scuba.add(MULTIPLEX_ID, multiplex_id.clone());
}

fn record_scuba_put(
    ctx: &CoreContext,
    scuba: &mut MononokeScubaSampleBuilder,
    multiplex_id: &MultiplexId,
    key: &str,
    blob_size: usize,
    stats: FutureStats,
    result: &Result<()>,
) {
    let op = OperationType::Put;
    record_scuba_common(ctx.clone(), scuba, multiplex_id, key, stats, op);

    scuba.add(BLOB_SIZE, blob_size);

    if let Err(error) = result.as_ref() {
        scuba.unsampled();
        scuba.add(ERROR, format!("{:#}", error)).add(SUCCESS, false);
    } else {
        scuba.add(SUCCESS, true);
    }
    scuba.log();
}

fn record_scuba_get(
    ctx: &CoreContext,
    scuba: &mut MononokeScubaSampleBuilder,
    multiplex_id: &MultiplexId,
    key: &str,
    stats: FutureStats,
    result: &Result<Option<BlobstoreGetData>>,
) {
    let op = OperationType::Get;
    record_scuba_common(ctx.clone(), scuba, multiplex_id, key, stats, op);

    match result.as_ref() {
        Err(error) => {
            scuba.unsampled();
            scuba.add(ERROR, format!("{:#}", error)).add(SUCCESS, false);
        }
        Ok(mb_blob) => {
            let blob_present = mb_blob.is_some();
            scuba.add(BLOB_PRESENT, blob_present).add(SUCCESS, true);

            if let Some(blob) = mb_blob.as_ref() {
                let size = blob.as_bytes().len();
                scuba.add(BLOB_SIZE, size);
            }
        }
    }
    scuba.log();
}

fn record_scuba_is_present(
    ctx: &CoreContext,
    scuba: &mut MononokeScubaSampleBuilder,
    multiplex_id: &MultiplexId,
    key: &str,
    stats: FutureStats,
    result: &Result<BlobstoreIsPresent>,
) {
    let op = OperationType::IsPresent;
    record_scuba_common(ctx.clone(), scuba, multiplex_id, key, stats, op);

    let outcome = result.as_ref().map(|is_present| match is_present {
        BlobstoreIsPresent::Present => Some(true),
        BlobstoreIsPresent::Absent => Some(false),
        BlobstoreIsPresent::ProbablyNotPresent(_) => None,
    });

    match outcome {
        Err(error) => {
            scuba.unsampled();
            scuba.add(ERROR, format!("{:#}", error)).add(SUCCESS, false);
        }
        Ok(is_present) => {
            if let Some(is_present) = is_present {
                scuba.add(BLOB_PRESENT, is_present).add(SUCCESS, true);
            }
        }
    }
    scuba.log();
}
