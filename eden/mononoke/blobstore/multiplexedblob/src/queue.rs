/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::base::{ErrorKind, MultiplexedBlobstoreBase, MultiplexedBlobstorePutHandler};
use anyhow::Result;
use async_trait::async_trait;
use blobstore::{Blobstore, BlobstoreGetData, BlobstorePutOps, OverwriteStatus, PutBehaviour};
use blobstore_sync_queue::{BlobstoreSyncQueue, BlobstoreSyncQueueEntry, OperationKey};
use context::CoreContext;
use metaconfig_types::{BlobstoreId, MultiplexId};
use mononoke_types::{BlobstoreBytes, DateTime};
use scuba::ScubaSampleBuilder;
use std::fmt;
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::Arc;

#[derive(Clone)]
pub struct MultiplexedBlobstore {
    pub(crate) blobstore: Arc<MultiplexedBlobstoreBase>,
    queue: Arc<dyn BlobstoreSyncQueue>,
}

impl MultiplexedBlobstore {
    pub fn new(
        multiplex_id: MultiplexId,
        blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
        write_mostly_blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
        minimum_successful_writes: NonZeroUsize,
        queue: Arc<dyn BlobstoreSyncQueue>,
        scuba: ScubaSampleBuilder,
        scuba_sample_rate: NonZeroU64,
    ) -> Self {
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
        }
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
        blobstore_id: BlobstoreId,
        multiplex_id: MultiplexId,
        operation_key: &'out OperationKey,
        key: &'out str,
    ) -> Result<()> {
        self.queue
            .add(
                ctx,
                BlobstoreSyncQueueEntry::new(
                    key.to_string(),
                    blobstore_id,
                    multiplex_id,
                    DateTime::now(),
                    operation_key.clone(),
                ),
            )
            .await
    }
}

#[async_trait]
impl Blobstore for MultiplexedBlobstore {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let result = self.blobstore.get(ctx, key).await;
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                if let Some(ErrorKind::AllFailed(_)) = error.downcast_ref() {
                    return Err(error);
                }
                // This means that some underlying blobstore returned error, and
                // other return None. To distinguish incomplete sync from true-none we
                // check synchronization queue. If it does not contain entries with this key
                // it means it is true-none otherwise, only replica containing key has
                // failed and we need to return error.
                let entries = self.queue.get(ctx, key).await?;
                if entries.is_empty() {
                    Ok(None)
                } else {
                    Err(error)
                }
            }
        }
    }

    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        self.blobstore.put(ctx, key, value).await
    }

    async fn is_present<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<bool> {
        let result = self.blobstore.is_present(ctx, key).await;
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                if let Some(ErrorKind::AllFailed(_)) = error.downcast_ref() {
                    return Err(error);
                }
                let entries = self.queue.get(&ctx, &key).await?;
                if entries.is_empty() {
                    Ok(false)
                } else {
                    Err(error)
                }
            }
        }
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
