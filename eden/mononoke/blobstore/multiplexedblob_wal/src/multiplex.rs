/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::BlobstorePutOps;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use blobstore_sync_queue::BlobstoreWal;
use context::CoreContext;
use metaconfig_types::BlobstoreId;
use metaconfig_types::MultiplexId;
use mononoke_types::BlobstoreBytes;
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct MultiplexQuorum {
    #[allow(dead_code)]
    read: NonZeroUsize,
    #[allow(dead_code)]
    write: NonZeroUsize,
}

impl MultiplexQuorum {
    fn new(num_stores: usize, write: usize) -> Result<Self> {
        if write > num_stores {
            return Err(anyhow!(
                "Not enough blobstores for configured put or get needs. Have {}, need {} puts",
                num_stores,
                write,
            ));
        }

        Ok(Self {
            write: NonZeroUsize::new(write).ok_or_else(|| anyhow!("Write quorum cannot be 0"))?,
            read: NonZeroUsize::new(num_stores - write + 1).unwrap(),
        })
    }
}

#[derive(Clone)]
pub struct WalMultiplexedBlobstore {
    /// Multiplexed blobstore configuration.
    multiplex_id: MultiplexId,
    /// Write-ahead log used to keep data consistent across blobstores.
    #[allow(dead_code)]
    wal_queue: Arc<dyn BlobstoreWal>,
    /// These are the "normal" blobstores, which are read from on `get`, and written to on `put`
    /// as part of normal operation.
    blobstores: Arc<[(BlobstoreId, Arc<dyn BlobstorePutOps>)]>,
    /// Write-mostly blobstores are not normally read from on `get`, but take part in writes
    /// like a normal blobstore.
    write_mostly_blobstores: Arc<[(BlobstoreId, Arc<dyn BlobstorePutOps>)]>,
    #[allow(dead_code)]
    quorum: MultiplexQuorum,
}

impl std::fmt::Display for WalMultiplexedBlobstore {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let blobstores: Vec<_> = self
            .blobstores
            .iter()
            .map(|(id, store)| (*id, store.to_string()))
            .collect();
        let write_mostly_blobstores: Vec<_> = self
            .write_mostly_blobstores
            .iter()
            .map(|(id, store)| (*id, store.to_string()))
            .collect();
        write!(
            f,
            "WAL MultiplexedBlobstore[normal {:?}, write mostly {:?}]",
            blobstores, write_mostly_blobstores
        )
    }
}

impl fmt::Debug for WalMultiplexedBlobstore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WalMultiplexedBlobstore: multiplex_id: {}",
            &self.multiplex_id
        )?;
        f.debug_map()
            .entries(self.blobstores.iter().map(|(ref k, ref v)| (k, v)))
            .finish()
    }
}

impl WalMultiplexedBlobstore {
    pub fn new(
        multiplex_id: MultiplexId,
        wal_queue: Arc<dyn BlobstoreWal>,
        blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
        write_mostly_blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
        write_quorum: usize,
    ) -> Result<Self> {
        let quorum = MultiplexQuorum::new(blobstores.len(), write_quorum)?;
        Ok(Self {
            multiplex_id,
            wal_queue,
            blobstores: blobstores.into(),
            write_mostly_blobstores: write_mostly_blobstores.into(),
            quorum,
        })
    }
}

#[async_trait]
impl Blobstore for WalMultiplexedBlobstore {
    async fn get<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        unimplemented!();
    }

    async fn is_present<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        unimplemented!();
    }

    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        BlobstorePutOps::put_with_status(self, ctx, key, value).await?;
        Ok(())
    }
}

#[async_trait]
impl BlobstorePutOps for WalMultiplexedBlobstore {
    async fn put_explicit<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _key: String,
        _value: BlobstoreBytes,
        _put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        unimplemented!();
    }

    async fn put_with_status<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _key: String,
        _value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        unimplemented!();
    }
}
