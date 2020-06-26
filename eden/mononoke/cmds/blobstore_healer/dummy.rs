/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This dummy crate contains dummy implementation of traits that are being used only in the
//! --dry-run mode to test the healer

use anyhow::Error;
use blobstore::{Blobstore, BlobstoreGetData};
use blobstore_sync_queue::{BlobstoreSyncQueue, BlobstoreSyncQueueEntry};
use context::CoreContext;
use futures::future::{self, BoxFuture, FutureExt};
use futures_ext::{BoxFuture as BoxFuture01, FutureExt as _};
use futures_old::prelude::*;
use metaconfig_types::MultiplexId;
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
    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        self.inner.get(ctx, key)
    }

    fn put(
        &self,
        _ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        info!(
            self.logger,
            "I would have written blob {} of size {}",
            key,
            value.len()
        );
        future::ok(()).boxed()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<'static, Result<bool, Error>> {
        self.inner.is_present(ctx, key)
    }

    fn assert_present(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<(), Error>> {
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
    ) -> BoxFuture01<(), Error> {
        let entries: Vec<_> = entries.map(|e| format!("{:?}", e)).collect();
        info!(self.logger, "I would have written {}", entries.join(",\n"));
        Ok(()).into_future().boxify()
    }

    fn iter(
        &self,
        ctx: CoreContext,
        key_like: Option<String>,
        multiplex_id: MultiplexId,
        older_than: DateTime,
        limit: usize,
    ) -> BoxFuture01<Vec<BlobstoreSyncQueueEntry>, Error> {
        self.inner
            .iter(ctx, key_like, multiplex_id, older_than, limit)
    }

    fn del(
        &self,
        _ctx: CoreContext,
        entries: Vec<BlobstoreSyncQueueEntry>,
    ) -> BoxFuture01<(), Error> {
        let entries: Vec<_> = entries.iter().map(|e| format!("{:?}", e)).collect();
        info!(self.logger, "I would have deleted {}", entries.join(",\n"));
        Ok(()).into_future().boxify()
    }

    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture01<Vec<BlobstoreSyncQueueEntry>, Error> {
        self.inner.get(ctx, key)
    }
}
