/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroU64;
use std::num::NonZeroUsize;
use std::sync::Arc;

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
use cloned::cloned;
use context::CoreContext;
use context::PerfCounterType;
use context::SessionClass;
use fbinit::FacebookInit;
use futures::future;
use futures::stream::FuturesUnordered;
use futures::Future;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use futures_stats::TimedFutureExt;
use metaconfig_types::BlobstoreId;
use metaconfig_types::MultiplexId;
use mononoke_types::BlobstoreBytes;
use mononoke_types::Timestamp;
use multiplexedblob::scuba;
use scuba_ext::MononokeScubaSampleBuilder;
use thiserror::Error;
use time_ext::DurationExt;
use tokio::task::JoinHandle;

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
    pub(crate) read: NonZeroUsize,
    pub(crate) write: NonZeroUsize,
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
    pub(crate) inner_blobstores_scuba: MononokeScubaSampleBuilder,
    multiplex_scuba: MononokeScubaSampleBuilder,
    sample_rate: NonZeroU64,
}

impl Scuba {
    pub fn new_from_raw(
        fb: FacebookInit,
        inner_blobstores_scuba_table: Option<String>,
        multiplex_scuba_table: Option<String>,
        sample_rate: NonZeroU64,
    ) -> Result<Self> {
        let inner = inner_blobstores_scuba_table.map_or_else(
            || Ok(MononokeScubaSampleBuilder::with_discard()),
            |table| MononokeScubaSampleBuilder::new(fb, &table),
        )?;
        let multiplex = multiplex_scuba_table.map_or_else(
            || Ok(MononokeScubaSampleBuilder::with_discard()),
            |table| MononokeScubaSampleBuilder::new(fb, &table),
        )?;

        Self::new(inner, multiplex, sample_rate)
    }

    pub fn new(
        mut inner_blobstores_scuba: MononokeScubaSampleBuilder,
        mut multiplex_scuba: MononokeScubaSampleBuilder,
        sample_rate: NonZeroU64,
    ) -> Result<Self> {
        inner_blobstores_scuba.add_common_server_data();
        multiplex_scuba.add_common_server_data();
        Ok(Self {
            inner_blobstores_scuba,
            multiplex_scuba,
            sample_rate,
        })
    }

    pub fn sampled(&mut self) {
        self.inner_blobstores_scuba.sampled(self.sample_rate);
        self.multiplex_scuba.sampled(self.sample_rate);
    }
}

#[derive(Clone)]
pub struct WalMultiplexedBlobstore {
    /// Multiplexed blobstore configuration.
    pub(crate) multiplex_id: MultiplexId,
    /// Write-ahead log used to keep data consistent across blobstores.
    pub(crate) wal_queue: Arc<dyn BlobstoreWal>,

    pub(crate) quorum: MultiplexQuorum,
    /// These are the "normal" blobstores, which are read from on `get`, and written to on `put`
    /// as part of normal operation.
    pub(crate) blobstores: Arc<[TimedStore]>,
    /// Write-mostly blobstores are not normally read from on `get`, but take part in writes
    /// like a normal blobstore.
    pub(crate) write_only_blobstores: Arc<[TimedStore]>,

    /// Scuba table to log status of the underlying single blobstore queries.
    pub(crate) scuba: Scuba,
}

impl std::fmt::Display for WalMultiplexedBlobstore {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "WAL MultiplexedBlobstore[normal {:?}, write only {:?}]",
            self.blobstores, self.write_only_blobstores
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
        write_only_blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
        write_quorum: usize,
        timeout: Option<MultiplexTimeout>,
        scuba: Scuba,
    ) -> Result<Self> {
        let quorum = MultiplexQuorum::new(blobstores.len(), write_quorum)?;

        let to = timeout.unwrap_or_default();
        let blobstores = with_timed_stores(blobstores, to.clone()).into();
        let write_only_blobstores = with_timed_stores(write_only_blobstores, to).into();

        Ok(Self {
            multiplex_id,
            wal_queue,
            blobstores,
            write_only_blobstores,
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
        scuba: &Scuba,
    ) -> Result<OverwriteStatus> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::BlobPuts);

        let blob_size = value.len() as u64;

        // Log the blobstore key and wait till it succeeds
        let ts = Timestamp::now();
        let log_entry = BlobstoreWalEntry::new(key.clone(), self.multiplex_id, ts, blob_size);
        let (stats, result) = self.wal_queue.log(ctx, log_entry).timed().await;

        scuba::record_queue_stats(
            ctx,
            &mut scuba.multiplex_scuba.clone(),
            &key,
            stats,
            None,
            self.to_string(),
            result.as_ref().map(|_| &()),
        );

        let entry = result.with_context(|| {
            format!(
                "WAL Multiplexed Blobstore: Failed writing to the WAL: key {}",
                key
            )
        })?;

        // Prepare underlying main blobstores puts
        let mut put_futs = inner_multi_put(
            ctx,
            self.blobstores.clone(),
            &key,
            &value,
            put_behaviour,
            scuba,
        );

        // Wait for the quorum successful writes
        let mut quorum: usize = self.quorum.write.get();
        let mut put_errors = HashMap::new();
        let (stats, result) = async move {
            while let Some(result) = put_futs.next().await {
                match result {
                    Ok(_overwrite_status) => {
                        quorum = quorum.saturating_sub(1);
                        if quorum == 0 {
                            // Quorum blobstore writes succeeded, we can spawn the rest
                            // of the writes and not wait for them.
                            let main_puts =
                                spawn_stream_completion(put_futs.map_err(|(_id, err)| err));

                            // Spawn the write-only blobstore writes, we don't want to wait for them
                            let write_only_puts = inner_multi_put(
                                ctx,
                                self.write_only_blobstores.clone(),
                                &key,
                                &value,
                                put_behaviour,
                                scuba,
                            );
                            let write_only_puts =
                                spawn_stream_completion(write_only_puts.map_err(|(_id, err)| err));

                            cloned!(ctx, self.wal_queue);
                            if put_errors.is_empty() {
                                // Optimisation: It put fully succeeded on all blobstores, we can remove
                                // it from queue and healer doesn't need to deal with it.
                                tokio::spawn(async move {
                                    let (r1, r2) = futures::join!(main_puts, write_only_puts);
                                    r1??;
                                    r2??;
                                    // TODO(yancouto): Batch deletes together.
                                    wal_queue.delete_by_key(&ctx, &[entry]).await?;
                                    anyhow::Ok(())
                                });
                            }

                            return Ok(OverwriteStatus::NotChecked);
                        }
                    }
                    Err((bs_id, err)) => {
                        put_errors.insert(bs_id, err);
                    }
                }
            }
            Err(put_errors)
        }
        .timed()
        .await;

        ctx.perf_counters().set_max_counter(
            PerfCounterType::BlobPutsMaxLatency,
            stats.completion_time.as_millis_unchecked() as i64,
        );
        ctx.perf_counters()
            .add_to_counter(PerfCounterType::BlobPutsTotalSize, blob_size as i64);

        result.map_err(|put_errors| {
            let errors = Arc::new(put_errors);
            let result_err = if errors.len() == self.blobstores.len() {
                // all main writes failed
                ErrorKind::AllFailed(errors)
            } else {
                // some main writes failed
                ErrorKind::SomePutsFailed(errors)
            };
            result_err.into()
        })
    }

    async fn get_impl<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
        scuba: &Scuba,
    ) -> Result<Option<BlobstoreGetData>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::BlobGets);

        let mut get_futs =
            inner_multi_get(ctx, self.blobstores.clone(), key, OperationType::Get, scuba);

        // Wait for the quorum successful "Not Found" reads before
        // returning Ok(None).
        let mut quorum: usize = self.quorum.read.get();
        let mut get_errors = HashMap::with_capacity(get_futs.len());
        let (stats, result) = async move {
            while let Some((bs_id, result)) = get_futs.next().await {
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
                    Err(err) => {
                        get_errors.insert(bs_id, err);
                    }
                }
            }
            Err(get_errors)
        }
        .timed()
        .await;

        ctx.perf_counters().set_max_counter(
            PerfCounterType::BlobGetsMaxLatency,
            stats.completion_time.as_millis_unchecked() as i64,
        );

        let result = result.map_err(|get_errors| {
            let errors = Arc::new(get_errors);
            let result_err = if errors.len() == self.blobstores.len() {
                // all main reads failed
                ErrorKind::AllFailed(errors)
            } else {
                // some main reads failed
                ErrorKind::SomeGetsFailed(errors)
            };
            result_err.into()
        });

        match result {
            Ok(Some(ref data)) => {
                ctx.perf_counters()
                    .add_to_counter(PerfCounterType::BlobGetsTotalSize, data.len() as i64);
            }
            Ok(None) => {
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::BlobGetsNotFound);
            }
            _ => {}
        }
        result
    }

    // TODO(aida): comprehensive lookup (D30839608)
    async fn is_present_impl<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
        scuba: &Scuba,
    ) -> Result<BlobstoreIsPresent> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::BlobPresenceChecks);

        // Comprehensive lookup requires blob presence in all of the blobstores.
        let comprehensive_lookup = is_comprehensive_lookup(ctx);

        let mut futs = inner_multi_is_present(ctx, self.blobstores.clone(), key, scuba);

        // Wait for the quorum successful "Not Found" reads before
        // returning Ok(None).
        let mut quorum: usize = self.quorum.read.get();
        let mut errors = HashMap::with_capacity(futs.len());
        let (stats, result) = async move {
            while let Some(result) = futs.next().await {
                match result {
                    (_, Ok(BlobstoreIsPresent::Present)) => {
                        // we only return on the first presence for the regular lookup
                        if !comprehensive_lookup {
                            return Ok(BlobstoreIsPresent::Present);
                        }
                    }
                    (_, Ok(BlobstoreIsPresent::Absent)) => {
                        quorum = quorum.saturating_sub(1);
                        // we return if there is either quorum on missing
                        // or it's a comprehensive lookup and we don't tolerate misses
                        if comprehensive_lookup || quorum == 0 {
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
            Err(errors)
        }
        .timed()
        .await;

        ctx.perf_counters().set_max_counter(
            PerfCounterType::BlobPresenceChecksMaxLatency,
            stats.completion_time.as_millis_unchecked() as i64,
        );

        let errors = match result {
            Ok(is_present) => {
                return Ok(is_present);
            }
            Err(errs) => errs,
        };

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
        let mut scuba = self.scuba.clone();
        scuba.sampled();
        let (stats, result) = self.get_impl(ctx, key, &scuba).timed().await;
        scuba::record_get(
            ctx,
            &mut scuba.multiplex_scuba,
            &self.multiplex_id,
            key,
            stats,
            &result,
        );
        result
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        let mut scuba = self.scuba.clone();
        scuba.sampled();
        let (stats, result) = self.is_present_impl(ctx, key, &scuba).timed().await;
        scuba::record_is_present(
            ctx,
            &mut scuba.multiplex_scuba,
            &self.multiplex_id,
            key,
            stats,
            &result,
        );
        result
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
        let size = value.len();
        let (stats, result) = self
            .put_impl(ctx, key.clone(), value, Some(put_behaviour), &self.scuba)
            .timed()
            .await;
        scuba::record_put(
            ctx,
            &mut self.scuba.multiplex_scuba.clone(),
            &self.multiplex_id,
            &key,
            size,
            stats,
            &result,
        );
        result
    }

    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        let size = value.len();
        let (stats, result) = self
            .put_impl(ctx, key.clone(), value, None, &self.scuba)
            .timed()
            .await;
        scuba::record_put(
            ctx,
            &mut self.scuba.multiplex_scuba.clone(),
            &self.multiplex_id,
            &key,
            size,
            stats,
            &result,
        );
        result
    }
}

fn spawn_stream_completion<T>(
    s: impl Stream<Item = Result<T>> + Send + 'static,
) -> JoinHandle<Result<()>> {
    tokio::spawn(s.try_for_each(|_| future::ok(())))
}

fn inner_multi_put(
    ctx: &CoreContext,
    blobstores: Arc<[TimedStore]>,
    key: &String,
    value: &BlobstoreBytes,
    put_behaviour: Option<PutBehaviour>,
    scuba: &Scuba,
) -> FuturesUnordered<impl Future<Output = Result<OverwriteStatus, (BlobstoreId, Error)>>> {
    let put_futs: FuturesUnordered<_> = blobstores
        .iter()
        .map(|bs| {
            cloned!(
                bs,
                ctx,
                key,
                value,
                put_behaviour,
                scuba.inner_blobstores_scuba
            );
            async move {
                bs.put(&ctx, key, value, put_behaviour, inner_blobstores_scuba)
                    .await
            }
        })
        .collect();
    put_futs
}

pub(crate) type GetResult = (BlobstoreId, Result<Option<BlobstoreGetData>, Error>);

pub(crate) fn inner_multi_get<'a>(
    ctx: &'a CoreContext,
    blobstores: Arc<[TimedStore]>,
    key: &'a str,
    operation: OperationType,
    scuba: &Scuba,
) -> FuturesUnordered<impl Future<Output = GetResult> + 'a> {
    let get_futs: FuturesUnordered<_> = blobstores
        .iter()
        .map(|bs| {
            cloned!(bs, scuba.inner_blobstores_scuba);
            async move {
                (
                    *bs.id(),
                    bs.get(ctx, key, operation, inner_blobstores_scuba).await,
                )
            }
        })
        .collect();
    get_futs
}

fn inner_multi_is_present<'a>(
    ctx: &'a CoreContext,
    blobstores: Arc<[TimedStore]>,
    key: &'a str,
    scuba: &Scuba,
) -> FuturesUnordered<impl Future<Output = (BlobstoreId, Result<BlobstoreIsPresent, Error>)> + 'a> {
    let futs: FuturesUnordered<_> = blobstores
        .iter()
        .map(|bs| {
            cloned!(bs, scuba.inner_blobstores_scuba);
            async move { bs.is_present(ctx, key, inner_blobstores_scuba).await }
        })
        .collect();
    futs
}

fn is_comprehensive_lookup(ctx: &CoreContext) -> bool {
    matches!(
        ctx.session().session_class(),
        SessionClass::ComprehensiveLookup
    )
}
