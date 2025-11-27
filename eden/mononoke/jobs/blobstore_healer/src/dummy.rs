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
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use blobstore_sync_queue::BlobstoreWal;
use blobstore_sync_queue::BlobstoreWalEntry;
use context::CoreContext;
use metaconfig_types::MultiplexId;
use mononoke_types::BlobstoreBytes;
use mononoke_types::Timestamp;
use tracing::info;

#[derive(Debug)]
pub struct DummyBlobstore<B> {
    inner: B,
}

impl<B: std::fmt::Display> std::fmt::Display for DummyBlobstore<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "DummyBlobstore<{}>", &self.inner)
    }
}

impl<B: Blobstore> DummyBlobstore<B> {
    pub fn new(inner: B) -> Self {
        Self { inner }
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

    async fn put_explicit<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        _put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        info!("I would have written blob {} of size {}", key, value.len());
        Ok(OverwriteStatus::NotChecked)
    }

    async fn put_with_status<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        info!("I would have written blob {} of size {}", key, value.len());
        Ok(OverwriteStatus::NotChecked)
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        self.inner.is_present(ctx, key).await
    }

    async fn unlink<'a>(&'a self, _ctx: &'a CoreContext, key: &'a str) -> Result<()> {
        info!("I would have unlinked blob {}", key);
        Ok(())
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
        _ctx: &'a CoreContext,
        entries: Vec<BlobstoreWalEntry>,
    ) -> Result<Vec<BlobstoreWalEntry>> {
        let entries_str: Vec<_> = entries.iter().map(|e| format!("{:?}", e)).collect();
        info!("I would have written {}", entries_str.join(",\n"));
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
        _ctx: &'a CoreContext,
        entries: &'a [BlobstoreWalEntry],
    ) -> Result<()> {
        let entries: Vec<_> = entries.iter().map(|e| format!("{:?}", e)).collect();
        info!("I would have deleted {}", entries.join(",\n"));
        Ok(())
    }

    async fn delete_by_key(&self, _ctx: &CoreContext, entries: &[BlobstoreWalEntry]) -> Result<()> {
        let entries: Vec<_> = entries.iter().map(|e| format!("{:?}", e)).collect();
        info!("I would have deleted by key {}", entries.join(",\n"));
        Ok(())
    }
}
