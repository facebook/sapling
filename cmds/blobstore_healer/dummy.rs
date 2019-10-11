/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! This dummy crate contains dummy implementation of traits that are being used only in the
//! --dry-run mode to test the healer

use blobstore::Blobstore;
use blobstore_sync_queue::{BlobstoreSyncQueue, BlobstoreSyncQueueEntry};
use context::CoreContext;
use failure_ext::Error;
use futures::prelude::*;
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::{BlobstoreBytes, DateTime};
use slog::{info, Logger};

#[derive(Debug)]
pub struct DummyBlobstore<B> {
    inner: B,
    logger: Logger,
}

impl<B: Blobstore> DummyBlobstore<B> {
    pub fn new(inner: B, logger: Logger) -> Self {
        Self { inner, logger }
    }
}

impl<B: Blobstore> Blobstore for DummyBlobstore<B> {
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        self.inner.get(ctx, key)
    }

    fn put(&self, _ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        info!(
            self.logger,
            "I would have written blob {} of size {}",
            key,
            value.len()
        );
        Ok(()).into_future().boxify()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        self.inner.is_present(ctx, key)
    }

    fn assert_present(&self, ctx: CoreContext, key: String) -> BoxFuture<(), Error> {
        self.inner.assert_present(ctx, key)
    }
}

pub struct DummyBlobstoreSyncQueue<Q> {
    inner: Q,
    logger: Logger,
}

impl<Q: BlobstoreSyncQueue> DummyBlobstoreSyncQueue<Q> {
    pub fn new(inner: Q, logger: Logger) -> Self {
        Self { inner, logger }
    }
}

impl<Q: BlobstoreSyncQueue> BlobstoreSyncQueue for DummyBlobstoreSyncQueue<Q> {
    fn add_many(
        &self,
        _ctx: CoreContext,
        entries: Box<dyn Iterator<Item = BlobstoreSyncQueueEntry> + Send>,
    ) -> BoxFuture<(), Error> {
        let entries: Vec<_> = entries.collect();
        info!(self.logger, "I would have written {:#?}", entries);
        Ok(()).into_future().boxify()
    }

    fn iter(
        &self,
        ctx: CoreContext,
        key_like: Option<String>,
        older_than: DateTime,
        limit: usize,
    ) -> BoxFuture<Vec<BlobstoreSyncQueueEntry>, Error> {
        self.inner.iter(ctx, key_like, older_than, limit)
    }

    fn del(
        &self,
        _ctx: CoreContext,
        entries: Vec<BlobstoreSyncQueueEntry>,
    ) -> BoxFuture<(), Error> {
        info!(self.logger, "I would have deleted {:#?}", entries);
        Ok(()).into_future().boxify()
    }

    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Vec<BlobstoreSyncQueueEntry>, Error> {
        self.inner.get(ctx, key)
    }
}
