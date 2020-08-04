/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::base::{ErrorKind, MultiplexedBlobstoreBase, MultiplexedBlobstorePutHandler};
use anyhow::Error;
use blobstore::{Blobstore, BlobstoreGetData};
use blobstore_sync_queue::{BlobstoreSyncQueue, BlobstoreSyncQueueEntry, OperationKey};
use cloned::cloned;
use context::CoreContext;
use futures::future::{BoxFuture, FutureExt};
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
        blobstores: Vec<(BlobstoreId, Arc<dyn Blobstore>)>,
        write_mostly_blobstores: Vec<(BlobstoreId, Arc<dyn Blobstore>)>,
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

impl MultiplexedBlobstorePutHandler for QueueBlobstorePutHandler {
    fn on_put<'out>(
        &'out self,
        ctx: &'out CoreContext,
        blobstore_id: BlobstoreId,
        multiplex_id: MultiplexId,
        operation_key: &'out OperationKey,
        key: &'out str,
    ) -> BoxFuture<'out, Result<(), Error>> {
        self.queue.add(
            ctx.clone(),
            BlobstoreSyncQueueEntry::new(
                key.to_string(),
                blobstore_id,
                multiplex_id,
                DateTime::now(),
                operation_key.clone(),
            ),
        )
    }
}

impl Blobstore for MultiplexedBlobstore {
    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        let get = self.blobstore.get(ctx.clone(), key.clone());
        cloned!(self.queue);

        async move {
            let result = get.await;
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
                    let entries = queue.get(ctx, key).await?;
                    if entries.is_empty() {
                        Ok(None)
                    } else {
                        Err(error)
                    }
                }
            }
        }
        .boxed()
    }

    fn put(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        self.blobstore.put(ctx, key, value)
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<'static, Result<bool, Error>> {
        cloned!(self.queue);
        let is_present = self.blobstore.is_present(ctx.clone(), key.clone());

        async move {
            let result = is_present.await;
            match result {
                Ok(value) => Ok(value),
                Err(error) => {
                    if let Some(ErrorKind::AllFailed(_)) = error.downcast_ref() {
                        return Err(error);
                    }
                    let entries = queue.get(ctx, key).await?;
                    if entries.is_empty() {
                        Ok(false)
                    } else {
                        Err(error)
                    }
                }
            }
        }
        .boxed()
    }
}
