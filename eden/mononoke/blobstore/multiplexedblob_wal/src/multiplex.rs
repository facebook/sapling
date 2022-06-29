/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Context as _;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::BlobstorePutOps;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use blobstore_sync_queue::BlobstoreWal;
use blobstore_sync_queue::BlobstoreWalEntry;
use blobstore_sync_queue::OperationKey;
use cloned::cloned;
use context::CoreContext;
use futures::stream::FuturesUnordered;
use futures::Future;
use futures::StreamExt;
use metaconfig_types::BlobstoreId;
use metaconfig_types::MultiplexId;
use mononoke_types::BlobstoreBytes;
use mononoke_types::Timestamp;
use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;
use thiserror::Error;

type BlobstoresReturnedError = HashMap<BlobstoreId, Error>;

#[derive(Error, Debug, Clone)]
pub enum ErrorKind {
    #[error("All blobstores failed: {0:?}")]
    AllFailed(Arc<BlobstoresReturnedError>),
    #[error("Failures on put in underlying single blobstores: {0:?}")]
    UnderlyingPutFailures(Arc<BlobstoresReturnedError>),
}

#[derive(Clone, Debug)]
pub struct MultiplexQuorum {
    #[allow(dead_code)]
    read: NonZeroUsize,
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

// TODO(aida):
// - Add scuba logging for the multiplexed operations
// - Add perf counters
// - Timeout on background futures
#[derive(Clone)]
pub struct WalMultiplexedBlobstore {
    /// Multiplexed blobstore configuration.
    multiplex_id: MultiplexId,
    /// Write-ahead log used to keep data consistent across blobstores.
    wal_queue: Arc<dyn BlobstoreWal>,
    /// These are the "normal" blobstores, which are read from on `get`, and written to on `put`
    /// as part of normal operation.
    blobstores: Arc<[(BlobstoreId, Arc<dyn BlobstorePutOps>)]>,
    /// Write-mostly blobstores are not normally read from on `get`, but take part in writes
    /// like a normal blobstore.
    write_mostly_blobstores: Arc<[(BlobstoreId, Arc<dyn BlobstorePutOps>)]>,
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

    async fn put_impl<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: Option<PutBehaviour>,
    ) -> Result<OverwriteStatus> {
        // Unique id associated with the put operation for this multiplexed blobstore.
        let operation_key = OperationKey::gen();
        let blob_size = value.len() as u64;

        // Log the blobstore key and wait till it succeeds
        let ts = Timestamp::now();
        let log_entry = BlobstoreWalEntry::new(
            key.clone(),
            self.multiplex_id,
            ts,
            operation_key,
            Some(blob_size),
        );
        self.wal_queue.log(ctx, log_entry).await.with_context(|| {
            format!(
                "WAL Multiplexed Blobstore: Failed writing to the WAL: key {}",
                &key
            )
        })?;

        // Prepare underlying main blobstores puts
        let mut put_futs = inner_multi_put(
            ctx,
            self.blobstores.clone(),
            key.clone(),
            value.clone(),
            put_behaviour,
        );

        // Wait for the quorum successful writes
        let mut quorum: usize = self.quorum.write.get();
        let mut put_errors = HashMap::new();
        while let Some(result) = put_futs.next().await {
            match result {
                Ok(_overwrite_status) => {
                    quorum = quorum.saturating_sub(1);
                    if quorum == 0 {
                        // Quorum blobstore writes succeeded, we can spawn the rest
                        // of the writes and not wait for them.
                        spawn_stream_completion(put_futs);

                        // Spawn the write-mostly blobstore writes, we don't want to wait for them
                        let write_mostly_puts = inner_multi_put(
                            ctx,
                            self.write_mostly_blobstores.clone(),
                            key,
                            value,
                            put_behaviour,
                        );
                        spawn_stream_completion(write_mostly_puts);

                        return Ok(OverwriteStatus::NotChecked);
                    }
                }
                Err((bs_id, err)) => {
                    put_errors.insert(bs_id, err);
                }
            }
        }

        // At this point the multiplexed put failed: we didn't get the quorum of successes.
        let errors = Arc::new(put_errors);
        let result_err = if errors.len() == self.blobstores.len() {
            // all main writes failed
            ErrorKind::AllFailed(errors)
        } else {
            // some main writes failed
            ErrorKind::UnderlyingPutFailures(errors)
        };

        Err(result_err.into())
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
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        self.put_impl(ctx, key, value, Some(put_behaviour)).await
    }

    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        self.put_impl(ctx, key, value, None).await
    }
}

fn spawn_stream_completion(s: impl StreamExt + Send + 'static) {
    tokio::spawn(s.for_each(|_| async {}));
}

fn inner_multi_put(
    ctx: &CoreContext,
    blobstores: Arc<[(BlobstoreId, Arc<dyn BlobstorePutOps>)]>,
    key: String,
    value: BlobstoreBytes,
    put_behaviour: Option<PutBehaviour>,
) -> FuturesUnordered<impl Future<Output = Result<OverwriteStatus, (BlobstoreId, Error)>>> {
    let put_futs: FuturesUnordered<_> = blobstores
        .iter()
        .map(|(bs_id, bs)| {
            cloned!(bs_id, bs, ctx, key, value, put_behaviour);
            async move {
                inner_put(&ctx, bs.as_ref(), key, value, put_behaviour)
                    .await
                    .map_err(|er| (bs_id, er))
            }
        })
        .collect();
    put_futs
}

async fn inner_put(
    ctx: &CoreContext,
    blobstore: &dyn BlobstorePutOps,
    key: String,
    value: BlobstoreBytes,
    put_behaviour: Option<PutBehaviour>,
) -> Result<OverwriteStatus> {
    if let Some(put_behaviour) = put_behaviour {
        blobstore.put_explicit(ctx, key, value, put_behaviour).await
    } else {
        blobstore.put_with_status(ctx, key, value).await
    }
}
