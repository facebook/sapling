/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This dummy crate contains dummy implementation of traits that are being used only in the
//! --dry-run mode to test the healer

use anyhow::Error;
use async_trait::async_trait;
use blobstore::{Blobstore, BlobstoreGetData};
use blobstore_sync_queue::{BlobstoreSyncQueue, BlobstoreSyncQueueEntry};
use context::CoreContext;
use futures::future::{self, BoxFuture, FutureExt};
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

#[async_trait]
impl<Q: BlobstoreSyncQueue> BlobstoreSyncQueue for DummyBlobstoreSyncQueue<Q> {
    async fn add_many<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        entries: Vec<BlobstoreSyncQueueEntry>,
    ) -> Result<(), Error> {
        let entries: Vec<_> = entries.into_iter().map(|e| format!("{:?}", e)).collect();
        info!(self.logger, "I would have written {}", entries.join(",\n"));
        Ok(())
    }

    async fn iter<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key_like: Option<&'a String>,
        multiplex_id: MultiplexId,
        older_than: DateTime,
        limit: usize,
    ) -> Result<Vec<BlobstoreSyncQueueEntry>, Error> {
        self.inner
            .iter(ctx, key_like, multiplex_id, older_than, limit)
            .await
    }

    async fn del<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        entries: &'a [BlobstoreSyncQueueEntry],
    ) -> Result<(), Error> {
        let entries: Vec<_> = entries.iter().map(|e| format!("{:?}", e)).collect();
        info!(self.logger, "I would have deleted {}", entries.join(",\n"));
        Ok(())
    }

    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a String,
    ) -> Result<Vec<BlobstoreSyncQueueEntry>, Error> {
        self.inner.get(ctx, key).await
    }
}
