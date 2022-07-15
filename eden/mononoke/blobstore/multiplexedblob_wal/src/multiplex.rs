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
use blobstore_stats::OperationType;
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
use scuba_ext::MononokeScubaSampleBuilder;
use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroU64;
use std::num::NonZeroUsize;
use std::sync::Arc;
use thiserror::Error;

use crate::timed::with_timed_stores;
use crate::timed::MultiplexTimeout;
use crate::timed::TimedStore;

type BlobstoresReturnedError = HashMap<BlobstoreId, Error>;

#[derive(Error, Debug, Clone)]
pub enum ErrorKind {
    #[error("All blobstores failed: {0:?}")]
    AllFailed(Arc<BlobstoresReturnedError>),
    #[error("Failures on put in underlying single blobstores: {0:?}")]
    SomePutsFailed(Arc<BlobstoresReturnedError>),
    #[error("Failures on get in underlying single blobstores: {0:?}")]
    SomeGetsFailed(Arc<BlobstoresReturnedError>),
    #[error("Failures on is_present in underlying single blobstores: {0:?}")]
    SomeIsPresentsFailed(Arc<BlobstoresReturnedError>),
}

#[derive(Clone, Debug)]
pub struct MultiplexQuorum {
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

#[derive(Clone)]
pub struct Scuba {
    scuba: MononokeScubaSampleBuilder,
    sample_rate: NonZeroU64,
}

impl Scuba {
    pub fn new(mut scuba: MononokeScubaSampleBuilder, sample_rate: u64) -> Result<Self> {
        scuba.add_common_server_data();

        let sample_rate =
            NonZeroU64::new(sample_rate).ok_or_else(|| anyhow!("Scuba sample rate cannot be 0"))?;
        Ok(Self { scuba, sample_rate })
    }

    pub fn sampled(&mut self) {
        self.scuba.sampled(self.sample_rate);
    }
}

// TODO(aida):
// - Add perf counters
#[derive(Clone)]
pub struct WalMultiplexedBlobstore {
    /// Multiplexed blobstore configuration.
    multiplex_id: MultiplexId,
    /// Write-ahead log used to keep data consistent across blobstores.
    wal_queue: Arc<dyn BlobstoreWal>,

    quorum: MultiplexQuorum,
    /// These are the "normal" blobstores, which are read from on `get`, and written to on `put`
    /// as part of normal operation.
    blobstores: Arc<[TimedStore]>,
    /// Write-mostly blobstores are not normally read from on `get`, but take part in writes
    /// like a normal blobstore.
    write_mostly_blobstores: Arc<[TimedStore]>,

    /// Scuba table to log status of the underlying single blobstore queries.
    scuba: Scuba,
}

impl std::fmt::Display for WalMultiplexedBlobstore {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "WAL MultiplexedBlobstore[normal {:?}, write mostly {:?}]",
            self.blobstores, self.write_mostly_blobstores
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
            .entries(self.blobstores.iter().map(|v| (v.id(), v)))
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
        timeout: Option<MultiplexTimeout>,
        scuba: Scuba,
    ) -> Result<Self> {
        let quorum = MultiplexQuorum::new(blobstores.len(), write_quorum)?;

        let to = timeout.unwrap_or_default();
        let blobstores = with_timed_stores(blobstores, to.clone()).into();
        let write_mostly_blobstores = with_timed_stores(write_mostly_blobstores, to).into();

        Ok(Self {
            multiplex_id,
            wal_queue,
            blobstores,
            write_mostly_blobstores,
            quorum,
            scuba,
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
            &self.scuba,
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
                            &self.scuba,
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
            ErrorKind::SomePutsFailed(errors)
        };

        Err(result_err.into())
    }

    async fn get_impl<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let mut scuba = self.scuba.clone();
        // the read requests are sampled unless they fail
        scuba.sampled();

        let mut get_futs =
            inner_multi_get(ctx, self.blobstores.clone(), key, OperationType::Get, scuba);

        // Wait for the quorum successful "Not Found" reads before
        // returning Ok(None).
        let mut quorum: usize = self.quorum.read.get();
        let mut get_errors = HashMap::with_capacity(get_futs.len());
        while let Some(result) = get_futs.next().await {
            match result {
                Ok(Some(get_data)) => {
                    return Ok(Some(get_data));
                }
                Ok(None) => {
                    quorum = quorum.saturating_sub(1);
                    if quorum == 0 {
                        // quorum blobstores couldn't find the given key in the blobstores
                        // let's trust them
                        return Ok(None);
                    }
                }
                Err((bs_id, err)) => {
                    get_errors.insert(bs_id, err);
                }
            }
        }

        // TODO(aida): read from write-mostly blobstores once in a while, but don't use
        // those in the quorum.

        // At this point the multiplexed get failed:
        // - we couldn't find the blob
        // - and there was no quorum on "not found" result
        let errors = Arc::new(get_errors);
        let result_err = if errors.len() == self.blobstores.len() {
            // all main reads failed
            ErrorKind::AllFailed(errors)
        } else {
            // some main reads failed
            ErrorKind::SomeGetsFailed(errors)
        };

        Err(result_err.into())
    }

    // TODO(aida): comprehensive lookup (D30839608)
    async fn is_present_impl<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        let mut scuba = self.scuba.clone();
        // the read requests are sampled unless they fail
        scuba.sampled();

        let mut futs = inner_multi_is_present(ctx, self.blobstores.clone(), key, scuba);

        // Wait for the quorum successful "Not Found" reads before
        // returning Ok(None).
        let mut quorum: usize = self.quorum.read.get();
        let mut errors = HashMap::with_capacity(futs.len());
        while let Some(result) = futs.next().await {
            match result {
                (_, Ok(BlobstoreIsPresent::Present)) => {
                    return Ok(BlobstoreIsPresent::Present);
                }
                (_, Ok(BlobstoreIsPresent::Absent)) => {
                    quorum = quorum.saturating_sub(1);
                    if quorum == 0 {
                        // quorum blobstores couldn't find the given key in the blobstores
                        // let's trust them
                        return Ok(BlobstoreIsPresent::Absent);
                    }
                }
                (bs_id, Ok(BlobstoreIsPresent::ProbablyNotPresent(err))) => {
                    // Treat this like an error from the underlying blobstore.
                    // In reality, this won't happen as multiplexed operates over sinle
                    // standard blobstores, which always can answer if the blob is present.
                    errors.insert(bs_id, err);
                }
                (bs_id, Err(err)) => {
                    errors.insert(bs_id, err);
                }
            }
        }

        // TODO(aida): read from write-mostly blobstores once in a while, but don't use
        // those in the quorum.

        // At this point the multiplexed is_present either failed or cannot say for sure
        // if the blob is present:
        // - no blob was found, but some of the blobstore `is_present` calls failed
        // - there was no read quorum on "not found" result
        let errors = Arc::new(errors);
        if errors.len() == self.blobstores.len() {
            // all main reads failed -> is_present failed
            return Err(ErrorKind::AllFailed(errors).into());
        }

        Ok(BlobstoreIsPresent::ProbablyNotPresent(
            ErrorKind::SomeIsPresentsFailed(errors).into(),
        ))
    }
}

#[async_trait]
impl Blobstore for WalMultiplexedBlobstore {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.get_impl(ctx, key).await
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        self.is_present_impl(ctx, key).await
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
    blobstores: Arc<[TimedStore]>,
    key: String,
    value: BlobstoreBytes,
    put_behaviour: Option<PutBehaviour>,
    scuba: &Scuba,
) -> FuturesUnordered<impl Future<Output = Result<OverwriteStatus, (BlobstoreId, Error)>>> {
    let put_futs: FuturesUnordered<_> = blobstores
        .iter()
        .map(|bs| {
            cloned!(bs, ctx, key, value, put_behaviour, scuba.scuba);
            async move { bs.put(&ctx, key, value, put_behaviour, scuba).await }
        })
        .collect();
    put_futs
}

fn inner_multi_get<'a>(
    ctx: &'a CoreContext,
    blobstores: Arc<[TimedStore]>,
    key: &'a str,
    operation: OperationType,
    scuba: Scuba,
) -> FuturesUnordered<
    impl Future<Output = Result<Option<BlobstoreGetData>, (BlobstoreId, Error)>> + 'a,
> {
    let get_futs: FuturesUnordered<_> = blobstores
        .iter()
        .map(|bs| {
            cloned!(bs, scuba.scuba);
            async move { bs.get(ctx, key, operation, scuba).await }
        })
        .collect();
    get_futs
}

fn inner_multi_is_present<'a>(
    ctx: &'a CoreContext,
    blobstores: Arc<[TimedStore]>,
    key: &'a str,
    scuba: Scuba,
) -> FuturesUnordered<impl Future<Output = (BlobstoreId, Result<BlobstoreIsPresent, Error>)> + 'a> {
    let futs: FuturesUnordered<_> = blobstores
        .iter()
        .map(|bs| {
            cloned!(bs, scuba.scuba);
            async move { bs.is_present(ctx, key, scuba).await }
        })
        .collect();
    futs
}
