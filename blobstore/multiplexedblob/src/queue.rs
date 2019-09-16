// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::base::{ErrorKind, MultiplexedBlobstoreBase, MultiplexedBlobstorePutHandler};
use blobstore::Blobstore;
use blobstore_sync_queue::{BlobstoreSyncQueue, BlobstoreSyncQueueEntry};
use cloned::cloned;
use context::CoreContext;
use failure_ext::Error;
use futures::future::{self, Future};
use futures_ext::{BoxFuture, FutureExt};
use metaconfig_types::BlobstoreId;
use mononoke_types::{BlobstoreBytes, DateTime};
use scuba::ScubaSampleBuilder;
use std::fmt;
use std::sync::Arc;

#[derive(Clone)]
pub struct MultiplexedBlobstore {
    blobstore: Arc<MultiplexedBlobstoreBase>,
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

#[derive(Clone)]
pub struct ScrubBlobstore {
    inner: MultiplexedBlobstore,
}

impl ScrubBlobstore {
    pub fn new(
        blobstores: Vec<(BlobstoreId, Arc<dyn Blobstore>)>,
        queue: Arc<dyn BlobstoreSyncQueue>,
        scuba: ScubaSampleBuilder,
    ) -> Self {
        let inner = MultiplexedBlobstore::new(blobstores, queue, scuba);
        Self { inner }
    }
}

impl fmt::Debug for ScrubBlobstore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScrubBlobstore")
            .field("inner", &self.inner)
            .finish()
    }
}

impl Blobstore for ScrubBlobstore {
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        self.inner
            .blobstore
            .scrub_get(ctx.clone(), key.clone())
            .then({
                let inner = &self.inner;
                cloned!(inner.queue);
                move |result| {
                    match result {
                        Ok(value) => future::ok(value).left_future(),
                        Err(mut error) => {
                            let (some_none, value) = match error.downcast_mut() {
                                Some(ErrorKind::SomeFailedOthersNone(_)) => (true, None),
                                Some(ErrorKind::SomeMissingItem(has_none, value)) => {
                                    (!has_none.is_empty(), value.take())
                                }
                                _ => return future::err(error).left_future(),
                            };
                            queue
                                .get(ctx, key)
                                .and_then(move |entries| {
                                    match (entries.is_empty(), value.is_some(), some_none) {
                                        // No sync in progress, got None + Error. Assume OK
                                        (true, false, _) => Ok(None),
                                        // Sync in progress, got None as best result. Uh-oh
                                        (false, false, _) => Err(error),
                                        // No sync in progress, got Some + None (+ possibly Error).
                                        // Error for now, but should schedule a sync
                                        // TODO: Should schedule a sync here to fix Some + None. then return Ok
                                        (true, true, true) => Err(error),
                                        // No sync in progress, got Some + Error. Assume OK
                                        (true, true, false) => Ok(value),
                                        // Sync in progress. Mix of Some/None/Error is OK
                                        (false, true, _) => Ok(value),
                                    }
                                })
                                .right_future()
                        }
                    }
                }
            })
            .boxify()
    }

    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        self.inner.put(ctx, key, value)
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        self.inner.is_present(ctx, key)
    }
}
