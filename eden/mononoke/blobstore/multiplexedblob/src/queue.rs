/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::num::NonZeroU64;
use std::num::NonZeroUsize;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::BlobstorePutOps;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use blobstore_sync_queue::BlobstoreSyncQueue;
use blobstore_sync_queue::BlobstoreSyncQueueEntry;
use blobstore_sync_queue::OperationKey;
use context::CoreContext;
use futures_stats::TimedFutureExt;
use metaconfig_types::BlobstoreId;
use metaconfig_types::MultiplexId;
use mononoke_types::BlobstoreBytes;
use mononoke_types::DateTime;
use scuba_ext::MononokeScubaSampleBuilder;
use tunables::tunables;

use crate::base::ErrorKind;
use crate::base::MultiplexedBlobstoreBase;
use crate::base::MultiplexedBlobstorePutHandler;
use crate::scuba;

/// Special error for cases where some blobstores failed during get/is_present
/// call and some returned None/Absent.
const SOME_FAILED_OTHERS_NONE: &str = "some_failed_others_none";
/// Number of entries we've fetched from the queue trying to resolve
/// SOME_FAILED_OTHERS_NONE case.
const QUEUE_ENTRIES: &str = "queue_entries_count";

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
        write_only_blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
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
                write_only_blobstores,
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

        scuba::record_queue_stats(
            ctx,
            &mut scuba,
            key,
            stats,
            Some(blobstore_id),
            blobstore_type,
            result.as_ref(),
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
                    if let Some(ErrorKind::SomeFailedOthersNone { main_errors, .. }) =
                        error.downcast_ref()
                    {
                        scuba.unsampled();
                        scuba.add(SOME_FAILED_OTHERS_NONE, format!("{:?}", main_errors));

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
        scuba::record_get(ctx, &mut scuba, multiplex_id, key, stats, &result);

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
        scuba::record_put(ctx, &mut scuba, multiplex_id, &key, size, stats, &result);

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
        scuba::record_is_present(ctx, &mut scuba, multiplex_id, key, stats, &result);

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
