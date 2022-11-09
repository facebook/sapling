/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This dummy crate contains dummy implementation of traits that are being used only in the
//! --dry-run mode to test the healer

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore_sync_queue::BlobstoreSyncQueue;
use blobstore_sync_queue::BlobstoreSyncQueueEntry;
use blobstore_sync_queue::BlobstoreWal;
use blobstore_sync_queue::BlobstoreWalEntry;
use context::CoreContext;
use metaconfig_types::MultiplexId;
use mononoke_types::BlobstoreBytes;
use mononoke_types::DateTime;
use mononoke_types::Timestamp;
use slog::info;
use slog::Logger;

#[derive(Debug)]
pub struct DummyBlobstore<B> {
    inner: B,
    logger: Logger,
}

impl<B: std::fmt::Display> std::fmt::Display for DummyBlobstore<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "DummyBlobstore<{}>", &self.inner)
    }
}

impl<B: Blobstore> DummyBlobstore<B> {
    pub fn new(inner: B, logger: Logger) -> Self {
        Self { inner, logger }
    }
}

#[async_trait]
impl<B: Blobstore> Blobstore for DummyBlobstore<B> {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.inner.get(ctx, key).await
    }

    async fn put<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        info!(
            self.logger,
            "I would have written blob {} of size {}",
            key,
            value.len()
        );
        Ok(())
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        self.inner.is_present(ctx, key).await
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
    ) -> Result<()> {
        let entries: Vec<_> = entries.into_iter().map(|e| format!("{:?}", e)).collect();
        info!(self.logger, "I would have written {}", entries.join(",\n"));
        Ok(())
    }

    async fn iter<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key_like: Option<&'a str>,
        multiplex_id: MultiplexId,
        older_than: DateTime,
        limit: usize,
    ) -> Result<Vec<BlobstoreSyncQueueEntry>> {
        self.inner
            .iter(ctx, key_like, multiplex_id, older_than, limit)
            .await
    }

    async fn del<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        entries: &'a [BlobstoreSyncQueueEntry],
    ) -> Result<()> {
        let entries: Vec<_> = entries.iter().map(|e| format!("{:?}", e)).collect();
        info!(self.logger, "I would have deleted {}", entries.join(",\n"));
        Ok(())
    }

    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Vec<BlobstoreSyncQueueEntry>> {
        self.inner.get(ctx, key).await
    }
}

pub struct DummyBlobstoreWal<Q> {
    inner: Q,
}

impl<Q: BlobstoreWal> DummyBlobstoreWal<Q> {
    pub fn new(inner: Q) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<Q: BlobstoreWal> BlobstoreWal for DummyBlobstoreWal<Q> {
    async fn log_many<'a>(
        &'a self,
        ctx: &'a CoreContext,
        entries: Vec<BlobstoreWalEntry>,
    ) -> Result<Vec<BlobstoreWalEntry>> {
        let entries_str: Vec<_> = entries.iter().map(|e| format!("{:?}", e)).collect();
        info!(
            ctx.logger(),
            "I would have written {}",
            entries_str.join(",\n")
        );
        Ok(entries)
    }

    async fn read<'a>(
        &'a self,
        ctx: &'a CoreContext,
        multiplex_id: &MultiplexId,
        older_than: &Timestamp,
        limit: usize,
    ) -> Result<Vec<BlobstoreWalEntry>> {
        self.inner.read(ctx, multiplex_id, older_than, limit).await
    }

    async fn delete<'a>(
        &'a self,
        ctx: &'a CoreContext,
        entries: &'a [BlobstoreWalEntry],
    ) -> Result<()> {
        let entries: Vec<_> = entries.iter().map(|e| format!("{:?}", e)).collect();
        info!(ctx.logger(), "I would have deleted {}", entries.join(",\n"));
        Ok(())
    }

    async fn delete_by_key(&self, ctx: &CoreContext, entries: &[BlobstoreWalEntry]) -> Result<()> {
        let entries: Vec<_> = entries.iter().map(|e| format!("{:?}", e)).collect();
        info!(
            ctx.logger(),
            "I would have deleted by key {}",
            entries.join(",\n")
        );
        Ok(())
    }
}
