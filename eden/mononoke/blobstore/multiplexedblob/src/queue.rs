/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::base::{ErrorKind, MultiplexedBlobstoreBase, MultiplexedBlobstorePutHandler};
use anyhow::Error;
use blobstore::Blobstore;
use blobstore_sync_queue::{BlobstoreSyncQueue, BlobstoreSyncQueueEntry};
use cloned::cloned;
use context::CoreContext;
use futures::future::{self, Future};
use futures_ext::{BoxFuture, FutureExt};
use metaconfig_types::BlobstoreId;
use mononoke_types::{BlobstoreBytes, DateTime};
use scuba::ScubaSampleBuilder;
use std::fmt;
use std::sync::Arc;

#[derive(Clone)]
pub struct MultiplexedBlobstore {
    pub(crate) blobstore: Arc<MultiplexedBlobstoreBase>,
    queue: Arc<dyn BlobstoreSyncQueue>,
}

impl MultiplexedBlobstore {
    pub fn new(
        blobstores: Vec<(BlobstoreId, Arc<dyn Blobstore>)>,
        queue: Arc<dyn BlobstoreSyncQueue>,
        scuba: ScubaSampleBuilder,
    ) -> Self {
        let put_handler = Arc::new(QueueBlobstorePutHandler {
            queue: queue.clone(),
        });
        Self {
            blobstore: Arc::new(MultiplexedBlobstoreBase::new(
                blobstores,
                put_handler,
                scuba,
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
    fn on_put(
        &self,
        ctx: CoreContext,
        blobstore_id: BlobstoreId,
        key: String,
    ) -> BoxFuture<(), Error> {
        self.queue
            .add(
                ctx,
                BlobstoreSyncQueueEntry::new(key, blobstore_id, DateTime::now()),
            )
            .map(|_| ())
            .boxify()
    }
}

impl Blobstore for MultiplexedBlobstore {
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        self.blobstore
            .get(ctx.clone(), key.clone())
            .then({
                cloned!(self.queue);
                move |result| match result {
                    Ok(value) => future::ok(value).left_future(),
                    Err(error) => {
                        if let Some(ErrorKind::AllFailed(_)) = error.downcast_ref() {
                            return future::err(error).left_future();
                        }
                        // This means that some underlying blobstore returned error, and
                        // other return None. To distinguish incomplete sync from true-none we
                        // check synchronization queue. If it does not contain entries with this key
                        // it means it is true-none otherwise, only replica containing key has
                        // failed and we need to return error.
                        queue
                            .get(ctx, key)
                            .and_then(|entries| {
                                if entries.is_empty() {
                                    Ok(None)
                                } else {
                                    Err(error)
                                }
                            })
                            .right_future()
                    }
                }
            })
            .boxify()
    }

    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        self.blobstore.put(ctx, key, value)
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        self.blobstore
            .is_present(ctx.clone(), key.clone())
            .then({
                cloned!(self.queue);
                move |result| match result {
                    Ok(value) => future::ok(value).left_future(),
                    Err(error) => {
                        if let Some(ErrorKind::AllFailed(_)) = error.downcast_ref() {
                            return future::err(error).left_future();
                        }
                        queue
                            .get(ctx, key)
                            .and_then(|entries| {
                                if entries.is_empty() {
                                    Ok(false)
                                } else {
                                    Err(error)
                                }
                            })
                            .right_future()
                    }
                }
            })
            .boxify()
    }
}
