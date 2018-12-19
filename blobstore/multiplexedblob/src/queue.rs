// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt;
use std::sync::Arc;

use cloned::cloned;
use failure::Error;
use futures::future::{self, Future};
use futures_ext::{BoxFuture, FutureExt};

use blobstore::Blobstore;
use blobstore_sync_queue::{BlobstoreSyncQueue, BlobstoreSyncQueueEntry};
use context::CoreContext;
use metaconfig::BlobstoreId;
use mononoke_types::{BlobstoreBytes, DateTime, RepositoryId};

use crate::base::{ErrorKind, MultiplexedBlobstoreBase, MultiplexedBlobstorePutHandler};

#[derive(Clone)]
pub struct MultiplexedBlobstore {
    repo_id: RepositoryId,
    blobstore: Arc<MultiplexedBlobstoreBase>,
    queue: Arc<BlobstoreSyncQueue>,
}

impl MultiplexedBlobstore {
    pub fn new(
        repo_id: RepositoryId,
        blobstores: Vec<(BlobstoreId, Arc<Blobstore>)>,
        queue: Arc<BlobstoreSyncQueue>,
    ) -> Self {
        let put_handler = Arc::new(QueueBlobstorePutHandler {
            repo_id,
            queue: queue.clone(),
        });
        Self {
            repo_id,
            blobstore: Arc::new(MultiplexedBlobstoreBase::new(blobstores, put_handler)),
            queue,
        }
    }
}

impl fmt::Debug for MultiplexedBlobstore {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MultiplexedBlobstore")
            .field("base", &self.blobstore)
            .finish()
    }
}

struct QueueBlobstorePutHandler {
    repo_id: RepositoryId,
    queue: Arc<BlobstoreSyncQueue>,
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
                BlobstoreSyncQueueEntry::new(self.repo_id, key, blobstore_id, DateTime::now()),
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
                cloned!(self.repo_id, self.queue);
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
                            .get(ctx, repo_id, key)
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
                cloned!(self.repo_id, self.queue);
                move |result| match result {
                    Ok(value) => future::ok(value).left_future(),
                    Err(error) => {
                        if let Some(ErrorKind::AllFailed(_)) = error.downcast_ref() {
                            return future::err(error).left_future();
                        }
                        queue
                            .get(ctx, repo_id, key)
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
