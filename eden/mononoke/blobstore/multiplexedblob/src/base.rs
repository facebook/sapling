/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Borrow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::future::Future;
use std::hash::Hasher;
use std::num::NonZeroU64;
use std::num::NonZeroUsize;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::BlobstorePutOps;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use blobstore_stats::record_get_stats;
use blobstore_stats::record_is_present_stats;
use blobstore_stats::record_put_stats;
use blobstore_stats::OperationType;
use blobstore_sync_queue::OperationKey;
use cloned::cloned;
use context::CoreContext;
use context::PerfCounterType;
use context::SessionClass;
use futures::future;
use futures::future::join_all;
use futures::future::select;
use futures::future::Either as FutureEither;
use futures::pin_mut;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_stats::TimedFutureExt;
use itertools::Either;
use metaconfig_types::BlobstoreId;
use metaconfig_types::MultiplexId;
use mononoke_types::BlobstoreBytes;
use scuba_ext::MononokeScubaSampleBuilder;
use thiserror::Error;
use time_ext::DurationExt;
use tokio::time::timeout;
use tunables::tunables;
use twox_hash::XxHash;

use crate::scrub::SrubWriteOnly;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
const DEFAULT_IS_PRESENT_TIMEOUT_MS: i64 = 10000;

type BlobstoresWithEntry = Vec<HashSet<BlobstoreId>>;
type BlobstoresReturnedNone = HashSet<BlobstoreId>;
type BlobstoresReturnedError = HashMap<BlobstoreId, Error>;

#[derive(Error, Debug, Clone)]
pub enum ErrorKind {
    #[error("Some blobstores failed, and other returned None: {main_errors:?}")]
    SomeFailedOthersNone {
        main_errors: Arc<BlobstoresReturnedError>,
        write_only_errors: Arc<BlobstoresReturnedError>,
    },
    #[error("All blobstores failed: {main_errors:?}")]
    AllFailed {
        main_errors: Arc<BlobstoresReturnedError>,
        write_only_errors: Arc<BlobstoresReturnedError>,
    },
    // Errors below this point are from ScrubBlobstore only. If they include an
    // Option<BlobstoreBytes>, this implies that this error is recoverable
    #[error(
        "Different blobstores have different values for this item: {0:?} are grouped by content, {1:?} do not have"
    )]
    ValueMismatch(Arc<BlobstoresWithEntry>, Arc<BlobstoresReturnedNone>),
    #[error("Some blobstores missing this item: {missing_main:?}")]
    SomeMissingItem {
        missing_main: Arc<BlobstoresReturnedNone>,
        missing_write_only: Arc<BlobstoresReturnedNone>,
        value: BlobstoreGetData,
    },
    #[error("Multiple failures on put: {0:?}")]
    MultiplePutFailures(Arc<BlobstoresReturnedError>),
}

/// This handler is called on each successful put to underlying blobstore,
/// for put to be considered successful this handler must return success.
/// It will be used to keep self-healing table up to date.
#[async_trait]
pub trait MultiplexedBlobstorePutHandler: Send + Sync {
    async fn on_put<'out>(
        &'out self,
        ctx: &'out CoreContext,
        mut scuba: MononokeScubaSampleBuilder,
        blobstore_id: BlobstoreId,
        blobstore_type: String,
        multiplex_id: MultiplexId,
        operation_key: &'out OperationKey,
        key: &'out str,
        blob_size: Option<u64>,
    ) -> Result<()>;
}

pub struct MultiplexedBlobstoreBase {
    multiplex_id: MultiplexId,
    /// These are the "normal" blobstores, which are read from on `get`, and written to on `put`
    /// as part of normal operation. No special treatment is applied.
    blobstores: Arc<[(BlobstoreId, Arc<dyn BlobstorePutOps>)]>,
    /// Write-mostly blobstores are not normally read from on `get`, but take part in writes
    /// like a normal blobstore.
    ///
    /// There are two circumstances in which a write-only blobstore will be read from on `get`:
    /// 1. The normal blobstores (above) all return Ok(None) or Err for a blob.
    ///    In this case, we read as it's our only chance of returning data that we previously accepted
    ///    during a `put` operation.
    /// 2. When we're recording blobstore stats to Scuba on a `get` - in this case, the read executes
    ///    solely to gather statistics, and the result is discarded
    write_only_blobstores: Arc<[(BlobstoreId, Arc<dyn BlobstorePutOps>)]>,
    /// `put` is considered successful if either this many `put` and `on_put` pairs succeeded or all puts were
    /// successful (regardless of whether `on_put`s were successful).
    /// This is meant to ensure that `put` fails if the data could end up lost (e.g. if a buggy experimental
    /// blobstore wins the `put` race).
    /// Note that if this is bigger than the number of blobstores, we will always fail writes
    minimum_successful_writes: NonZeroUsize,
    /// During a `get/is_present`, if this many blobstores successfully return that the blob is not present, then the
    /// blob is considered not present in this blobstore.
    not_present_read_quorum: NonZeroUsize,
    handler: Arc<dyn MultiplexedBlobstorePutHandler>,
    scuba: MononokeScubaSampleBuilder,
    scuba_sample_rate: NonZeroU64,
}

impl std::fmt::Display for MultiplexedBlobstoreBase {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let blobstores: Vec<_> = self
            .blobstores
            .iter()
            .map(|(id, store)| (*id, store.to_string()))
            .collect();
        let write_only_blobstores: Vec<_> = self
            .write_only_blobstores
            .iter()
            .map(|(id, store)| (*id, store.to_string()))
            .collect();
        write!(
            f,
            "Normal {:?}, write only {:?}",
            blobstores, write_only_blobstores
        )
    }
}

fn blobstores_failed_error(
    main_blobstore_ids: impl Iterator<Item = BlobstoreId>,
    main_errors: HashMap<BlobstoreId, Error>,
    write_only_errors: HashMap<BlobstoreId, Error>,
) -> ErrorKind {
    let main_errored_ids: HashSet<BlobstoreId> = main_errors.keys().copied().collect();
    let all_main_ids: HashSet<BlobstoreId> = main_blobstore_ids.collect();
    if main_errored_ids == all_main_ids {
        // The write only stores that returned None might not have been fully populated
        ErrorKind::AllFailed {
            main_errors: Arc::new(main_errors),
            write_only_errors: Arc::new(write_only_errors),
        }
    } else {
        ErrorKind::SomeFailedOthersNone {
            main_errors: Arc::new(main_errors),
            write_only_errors: Arc::new(write_only_errors),
        }
    }
}

type GetResult = (BlobstoreId, Result<Option<BlobstoreGetData>, Error>);

/// Get normal and write only results based on SrubWriteOnly
/// mode of getting results, which might optimise for less access
/// to main blobstores or less access to write only blobstores
pub async fn scrub_get_results<MF, WF>(
    get_main_results: impl FnOnce() -> MF,
    mut get_write_only_results: impl FnMut() -> WF,
    write_only_blobstores: impl Iterator<Item = BlobstoreId>,
    write_only: SrubWriteOnly,
) -> impl Iterator<Item = (bool, GetResult)>
where
    MF: Future<Output = Vec<GetResult>>,
    WF: Future<Output = Vec<GetResult>>,
{
    // Exit early if all mostly-write are ok, and don't check main blobstores
    if write_only == SrubWriteOnly::ScrubIfAbsent {
        let mut results = get_write_only_results().await.into_iter();
        if let Some((bs_id, Ok(Some(data)))) = results.next() {
            if results.all(|(_, r)| match r {
                Ok(Some(other_data)) => other_data == data,
                _ => false,
            }) {
                return Either::Left(std::iter::once((true, (bs_id, Ok(Some(data))))));
            }
        }
    }

    let write_only_results = async {
        match write_only {
            SrubWriteOnly::Scrub | SrubWriteOnly::SkipMissing => get_write_only_results().await,
            SrubWriteOnly::PopulateIfAbsent | SrubWriteOnly::ScrubIfAbsent => {
                write_only_blobstores.map(|id| (id, Ok(None))).collect()
            }
        }
    };
    let (normal_results, write_only_results) =
        future::join(get_main_results(), write_only_results).await;

    Either::Right(
        normal_results
            .into_iter()
            .map(|r| (false, r))
            .chain(write_only_results.into_iter().map(|r| (true, r))),
    )
}

pub fn scrub_parse_results(
    results: impl Iterator<Item = (bool, GetResult)>,
    all_main: impl Iterator<Item = BlobstoreId>,
) -> Result<Option<BlobstoreGetData>, ErrorKind> {
    let mut missing_main = HashSet::new();
    let mut missing_write_only = HashSet::new();
    let mut get_data = None;
    let mut main_errors = HashMap::new();
    let mut write_only_errors = HashMap::new();

    for (is_write_only, (blobstore_id, result)) in results {
        match result {
            Ok(None) => {
                if is_write_only {
                    missing_write_only.insert(blobstore_id);
                } else {
                    missing_main.insert(blobstore_id);
                }
            }
            Ok(Some(value)) => {
                let mut content_hash = XxHash::with_seed(0);
                content_hash.write(value.as_raw_bytes());
                let content_hash = content_hash.finish();
                let (all_values, _) = get_data.get_or_insert_with(|| (HashMap::new(), value));
                all_values
                    .entry(content_hash)
                    .or_insert_with(HashSet::new)
                    .insert(blobstore_id);
            }
            Err(err) => {
                if is_write_only {
                    write_only_errors.insert(blobstore_id, err);
                } else {
                    main_errors.insert(blobstore_id, err);
                }
            }
        }
    }
    match get_data {
        None => {
            if main_errors.is_empty() && write_only_errors.is_empty() {
                Ok(None)
            } else {
                Err(blobstores_failed_error(
                    all_main,
                    main_errors,
                    write_only_errors,
                ))
            }
        }
        Some((all_values, value)) if all_values.len() == 1 => {
            if missing_main.is_empty() && missing_write_only.is_empty() {
                Ok(Some(value))
            } else {
                // This silently ignores failed blobstores if at least one has a value
                Err(ErrorKind::SomeMissingItem {
                    missing_main: Arc::new(missing_main),
                    missing_write_only: Arc::new(missing_write_only),
                    value,
                })
            }
        }
        Some((all_values, _)) => {
            let answered = all_values.into_iter().map(|(_, stores)| stores).collect();
            let mut all_missing = HashSet::new();
            all_missing.extend(missing_main.into_iter());
            all_missing.extend(missing_write_only.into_iter());
            Err(ErrorKind::ValueMismatch(
                Arc::new(answered),
                Arc::new(all_missing),
            ))
        }
    }
}

impl MultiplexedBlobstoreBase {
    pub fn new(
        multiplex_id: MultiplexId,
        blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
        write_only_blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
        minimum_successful_writes: NonZeroUsize,
        not_present_read_quorum: NonZeroUsize,
        handler: Arc<dyn MultiplexedBlobstorePutHandler>,
        mut scuba: MononokeScubaSampleBuilder,
        scuba_sample_rate: NonZeroU64,
    ) -> Self {
        scuba.add_common_server_data();

        Self {
            multiplex_id,
            blobstores: blobstores.into(),
            write_only_blobstores: write_only_blobstores.into(),
            minimum_successful_writes,
            not_present_read_quorum,
            handler,
            scuba,
            scuba_sample_rate,
        }
    }

    pub fn multiplex_id(&self) -> &MultiplexId {
        &self.multiplex_id
    }

    pub async fn scrub_get(
        &self,
        ctx: &CoreContext,
        key: &str,
        write_only: SrubWriteOnly,
    ) -> Result<Option<BlobstoreGetData>, ErrorKind> {
        let mut scuba = self.scuba.clone();
        scuba.sampled(self.scuba_sample_rate);

        let results = scrub_get_results(
            || {
                join_all(multiplexed_get(
                    ctx,
                    self.blobstores.as_ref(),
                    key,
                    OperationType::ScrubGet,
                    scuba.clone(),
                ))
            },
            || {
                join_all(multiplexed_get(
                    ctx,
                    self.write_only_blobstores.as_ref(),
                    key,
                    OperationType::ScrubGet,
                    scuba.clone(),
                ))
            },
            self.write_only_blobstores.iter().map(|(id, _)| *id),
            write_only,
        )
        .await;

        scrub_parse_results(results, self.blobstores.iter().map(|(id, _)| *id))
    }
}

fn remap_timeout_result<O>(
    timeout_or_result: Result<Result<O, Error>, tokio::time::error::Elapsed>,
) -> Result<O, Error> {
    timeout_or_result.unwrap_or_else(|_| Err(Error::msg("blobstore operation timeout")))
}

pub async fn inner_put(
    ctx: &CoreContext,
    mut scuba: MononokeScubaSampleBuilder,
    write_order: &AtomicUsize,
    blobstore_id: BlobstoreId,
    blobstore: &dyn BlobstorePutOps,
    key: String,
    value: BlobstoreBytes,
    put_behaviour: Option<PutBehaviour>,
) -> (BlobstoreId, Result<OverwriteStatus, Error>) {
    let size = value.len();
    let (pc, (stats, timeout_or_res)) = {
        let mut ctx = ctx.clone();
        let pc = ctx.fork_perf_counters();
        let ret = timeout(
            REQUEST_TIMEOUT,
            if let Some(put_behaviour) = put_behaviour {
                blobstore.put_explicit(&ctx, key.clone(), value, put_behaviour)
            } else {
                blobstore.put_with_status(&ctx, key.clone(), value)
            },
        )
        .timed()
        .await;
        (pc, ret)
    };
    let result = remap_timeout_result(timeout_or_res);
    record_put_stats(
        &mut scuba,
        &pc,
        stats,
        result.as_ref(),
        &key,
        ctx.metadata().session_id().as_str(),
        size,
        Some(blobstore_id),
        blobstore,
        Some(write_order.fetch_add(1, Ordering::Relaxed) + 1),
    );
    (blobstore_id, result)
}

async fn blobstore_get<'a>(
    ctx: &'a CoreContext,
    blobstores: Arc<[(BlobstoreId, Arc<dyn BlobstorePutOps>)]>,
    write_only_blobstores: Arc<[(BlobstoreId, Arc<dyn BlobstorePutOps>)]>,
    not_present_read_quorum: NonZeroUsize,
    key: &'a str,
    scuba: MononokeScubaSampleBuilder,
) -> Result<Option<BlobstoreGetData>, Error> {
    let is_logged = scuba.sampling().is_logged();
    let blobstores_count = blobstores.len() + write_only_blobstores.len();
    let mut needed_not_present: usize = not_present_read_quorum.get();

    if needed_not_present > blobstores_count {
        anyhow::bail!(
            "Not enough blobstores for configured get needs. Have {}, need {}",
            blobstores_count,
            needed_not_present
        )
    }

    let (stats, result) = {
        async move {
            let mut main_errors = HashMap::new();
            let mut write_only_errors = HashMap::new();
            ctx.perf_counters()
                .increment_counter(PerfCounterType::BlobGets);

            let main_requests: FuturesUnordered<_> = multiplexed_get(
                ctx.clone(),
                blobstores.as_ref(),
                key.to_owned(),
                OperationType::Get,
                scuba.clone(),
            )
            .collect();
            let write_only_requests: FuturesUnordered<_> = multiplexed_get(
                ctx.clone(),
                write_only_blobstores.as_ref(),
                key.to_owned(),
                OperationType::Get,
                scuba,
            )
            .collect();

            // `chain` here guarantees that `main_requests` is empty before it starts
            // polling anything in `write_only_requests`
            let mut requests = main_requests
                .map(|r| (false, r))
                .chain(write_only_requests.map(|r| (true, r)));
            while let Some((is_write_only, result)) = requests.next().await {
                match result {
                    (_, Ok(Some(mut value))) => {
                        if is_logged {
                            // Allow the other requests to complete so that we can record some
                            // metrics for the blobstore. This will also log metrics for write-only
                            // blobstores, which helps us decide whether they're good
                            spawn_stream_completion(requests);
                        }
                        // Return the blob that won the race
                        value.remove_ctime();
                        return Ok(Some(value));
                    }
                    (blobstore_id, Err(error)) => {
                        if is_write_only {
                            write_only_errors.insert(blobstore_id, error);
                        } else {
                            main_errors.insert(blobstore_id, error);
                        }
                    }
                    (_, Ok(None)) => {
                        needed_not_present = needed_not_present.saturating_sub(1);
                        if needed_not_present == 0 {
                            if is_logged {
                                spawn_stream_completion(requests);
                            }
                            return Ok(None);
                        }
                    }
                }
            }

            let error_count = main_errors.len() + write_only_errors.len();
            if error_count == 0 {
                // All blobstores must have returned None, as Some would have triggered a return,
                Ok(None)
            } else if error_count == blobstores_count {
                Err(ErrorKind::AllFailed {
                    main_errors: Arc::new(main_errors),
                    write_only_errors: Arc::new(write_only_errors),
                })
            } else {
                Err(blobstores_failed_error(
                    blobstores.iter().map(|(id, _)| *id),
                    main_errors,
                    write_only_errors,
                ))
            }
        }
        .timed()
        .await
    };

    ctx.perf_counters().set_max_counter(
        PerfCounterType::BlobGetsMaxLatency,
        stats.completion_time.as_millis_unchecked() as i64,
    );

    match result {
        Ok(Some(ref data)) => {
            ctx.perf_counters().add_to_counter(
                PerfCounterType::BlobGetsTotalSize,
                data.len().try_into().unwrap_or(0),
            );
        }
        Ok(None) => {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::BlobGetsNotFound);
            ctx.perf_counters().set_max_counter(
                PerfCounterType::BlobGetsNotFoundMaxLatency,
                stats.completion_time.as_millis_unchecked() as i64,
            );
        }
        _ => {}
    }

    Ok(result?)
}

fn spawn_stream_completion(s: impl StreamExt + Send + 'static) {
    tokio::spawn(s.for_each(|_| async {}));
}

struct Timeout;

// Waits for select_next and timer if it's set, and returns
// whichever finishes first
async fn select_next_with_timeout<F1: Future, F2: Future>(
    left: &mut FuturesUnordered<F1>,
    right: &mut FuturesUnordered<F2>,
    consider_right: bool,
    maybe_timer: &mut Option<std::pin::Pin<Box<tokio::time::Sleep>>>,
) -> Option<Result<Either<F1::Output, F2::Output>, Timeout>> {
    let select_next_fut = select_next(left, right, consider_right);
    match maybe_timer {
        Some(timer) => {
            pin_mut!(select_next_fut);
            match future::select(select_next_fut, timer).await {
                FutureEither::Left((value, _)) => value.map(Ok),
                FutureEither::Right(((), _)) => Some(Err(Timeout)),
            }
        }
        None => select_next_fut.await.map(Ok),
    }
}

/// Select the next item from one of two FuturesUnordered stream.
/// With `consider_right` set to false, this is the same as `left.next().await.map(Either::Left)`.
/// With `consider_right` set to true, this picks the first item to complete from either stream.
/// The idea is that `left` contains your core work, and you always want to poll futures in that
/// stream, while `right` contains failure recovery, and you only want to poll futures in that
/// stream if you need to do failure recovery.
async fn select_next<F1: Future, F2: Future>(
    left: &mut FuturesUnordered<F1>,
    right: &mut FuturesUnordered<F2>,
    consider_right: bool,
) -> Option<Either<F1::Output, F2::Output>> {
    use Either::*;
    let right_empty = !consider_right || right.is_empty();
    // Can't use a match block because that infers the wrong Send + Sync bounds for this future
    if left.is_empty() && right_empty {
        None
    } else if right_empty {
        left.next().await.map(Left)
    } else if left.is_empty() {
        right.next().await.map(Right)
    } else {
        use Either::*;
        // Although we drop the second element in the pair returned by select (which represents
        // the unfinished future), this does not cause data loss, because until that future is
        // awaited, it won't pull data out of the stream.
        match select(left.next(), right.next()).await {
            FutureEither::Left((None, other)) => other.await.map(Right),
            FutureEither::Right((None, other)) => other.await.map(Left),
            FutureEither::Left((Some(res), _)) => Some(Left(res)),
            FutureEither::Right((Some(res), _)) => Some(Right(res)),
        }
    }
}

#[async_trait]
impl Blobstore for MultiplexedBlobstoreBase {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let mut scuba = self.scuba.clone();
        let blobstores = self.blobstores.clone();
        let write_only_blobstores = self.write_only_blobstores.clone();
        let not_present_read_quorum = self.not_present_read_quorum;
        scuba.sampled(self.scuba_sample_rate);

        blobstore_get(
            ctx,
            blobstores,
            write_only_blobstores,
            not_present_read_quorum,
            key,
            scuba,
        )
        .await
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        let mut scuba = self.scuba.clone();
        scuba.sampled(self.scuba_sample_rate);
        let is_logged = scuba.sampling().is_logged();
        let blobstores_count = self.blobstores.len() + self.write_only_blobstores.len();
        let mut needed_not_present: usize = self.not_present_read_quorum.get();
        let comprehensive_lookup = matches!(
            ctx.session().session_class(),
            SessionClass::ComprehensiveLookup
        );

        let main_requests: FuturesUnordered<_> = multiplexed_is_present(
            ctx.clone(),
            &self.blobstores.clone(),
            key.to_owned(),
            scuba.clone(),
        )
        .collect();

        let write_only_requests: FuturesUnordered<_> = multiplexed_is_present(
            ctx.clone(),
            &self.write_only_blobstores.clone(),
            key.to_owned(),
            scuba,
        )
        .collect();

        // Lookup algorithm supports two strategies:
        // "comprehensive" and "regular"
        //
        // Comprehensive lookup requires presence in all the blobstores.
        // Regular lookup requires presence in at least one main or write only blobstore.

        // `chain` here guarantees that `main_requests` is empty before it starts
        // polling anything in `write_only_requests`
        let mut requests = main_requests
            .map(|r| (false, r))
            .chain(write_only_requests.map(|r| (true, r)));
        let (stats, result) = {
            let blobstores = &self.blobstores;
            async move {
                let mut main_errors = HashMap::new();
                let mut write_only_errors = HashMap::new();
                let mut present_counter = 0;
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::BlobPresenceChecks);
                while let Some((is_write_only, result)) = requests.next().await {
                    match result {
                        (_, Ok(BlobstoreIsPresent::Present)) => {
                            if !comprehensive_lookup {
                                if is_logged {
                                    // Allow the other requests to complete so that we can record some
                                    // metrics for the blobstore. This will also log metrics for write-only
                                    // blobstores, which helps us decide whether they're good
                                    spawn_stream_completion(requests);
                                }
                                return Ok(BlobstoreIsPresent::Present);
                            }
                            present_counter += 1;
                        }
                        (_, Ok(BlobstoreIsPresent::Absent)) => {
                            needed_not_present = needed_not_present.saturating_sub(1);
                            if comprehensive_lookup || needed_not_present == 0 {
                                if is_logged {
                                    spawn_stream_completion(requests);
                                }
                                return Ok(BlobstoreIsPresent::Absent);
                            }
                        }
                        // is_present failed for the underlying blobstore
                        (blobstore_id, Err(error)) => {
                            if is_write_only {
                                write_only_errors.insert(blobstore_id, error);
                            } else {
                                main_errors.insert(blobstore_id, error);
                            }
                        }
                        (blobstore_id, Ok(BlobstoreIsPresent::ProbablyNotPresent(err))) => {
                            let err = err.context(
                                "Received 'ProbablyNotPresent' from the underlying blobstore"
                                    .to_string(),
                            );
                            if is_write_only {
                                write_only_errors.insert(blobstore_id, err);
                            } else {
                                main_errors.insert(blobstore_id, err);
                            }
                        }
                    }
                }

                if comprehensive_lookup {
                    // all blobstores reported the blob is present
                    if main_errors.is_empty() && write_only_errors.is_empty() {
                        Ok(BlobstoreIsPresent::Present)
                    }
                    // some blobstores reported the blob is present, others failed
                    else if present_counter > 0 {
                        let err = Error::from(ErrorKind::SomeFailedOthersNone {
                            main_errors: Arc::new(main_errors),
                            write_only_errors: Arc::new(write_only_errors),
                        });
                        Ok(BlobstoreIsPresent::ProbablyNotPresent(err))
                    }
                    // all blobstores failed
                    else {
                        Err(ErrorKind::AllFailed {
                            main_errors: Arc::new(main_errors),
                            write_only_errors: Arc::new(write_only_errors),
                        })
                    }
                } else {
                    // all blobstores reported the blob is missing
                    if main_errors.is_empty() && write_only_errors.is_empty() {
                        Ok(BlobstoreIsPresent::Absent)
                    }
                    // all blobstores failed
                    else if main_errors.len() + write_only_errors.len() == blobstores_count {
                        Err(ErrorKind::AllFailed {
                            main_errors: Arc::new(main_errors),
                            write_only_errors: Arc::new(write_only_errors),
                        })
                    }
                    // some blobstores reported the blob is missing, others failed
                    else {
                        let write_only_err = blobstores_failed_error(
                            blobstores.iter().map(|(id, _)| *id),
                            main_errors,
                            write_only_errors,
                        );
                        if matches!(write_only_err, ErrorKind::SomeFailedOthersNone { .. }) {
                            let err = Error::from(write_only_err);
                            Ok(BlobstoreIsPresent::ProbablyNotPresent(err))
                        } else {
                            Err(write_only_err)
                        }
                    }
                }
            }
            .timed()
            .await
        };

        ctx.perf_counters().set_max_counter(
            PerfCounterType::BlobPresenceChecksMaxLatency,
            stats.completion_time.as_millis_unchecked() as i64,
        );

        Ok(result?)
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

impl MultiplexedBlobstoreBase {
    // If put_behaviour is None, we we call inner BlobstorePutOps::put_with_status()
    // If put_behaviour is Some, we we call inner BlobstorePutOps::put_explicit()
    async fn put_impl<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: Option<PutBehaviour>,
    ) -> Result<OverwriteStatus> {
        let write_order = Arc::new(AtomicUsize::new(0));
        let operation_key = OperationKey::gen();
        let mut needed_handlers: usize = self.minimum_successful_writes.into();
        let run_handlers_on_success = !matches!(
            ctx.session().session_class(),
            SessionClass::Background | SessionClass::BackgroundUnlessTooSlow
        );

        let mut puts: FuturesUnordered<_> = self
            .blobstores
            .iter()
            .chain(self.write_only_blobstores.iter())
            .cloned()
            .map({
                |(blobstore_id, blobstore)| {
                    cloned!(
                        self.handler,
                        self.multiplex_id,
                        mut self.scuba,
                        mut ctx,
                        write_order,
                        key,
                        value,
                        operation_key
                    );
                    async move {
                        let blob_size = value.len() as u64;
                        let (blobstore_id, res) = inner_put(
                            &ctx,
                            scuba.clone(),
                            write_order.as_ref(),
                            blobstore_id,
                            blobstore.as_ref(),
                            key.clone(),
                            value,
                            put_behaviour,
                        )
                        .await;
                        res.map_err(|err| (blobstore_id, err))?;
                        // Return the on_put handler
                        Ok(async move {
                            let res = handler
                                .on_put(
                                    &ctx,
                                    scuba,
                                    blobstore_id,
                                    blobstore.to_string(),
                                    multiplex_id,
                                    &operation_key,
                                    &key,
                                    Some(blob_size),
                                )
                                .await;

                            res.map_err(|err| (blobstore_id, err))
                        })
                    }
                }
            })
            .collect();

        if needed_handlers > puts.len() {
            return Err(anyhow!(
                "Not enough blobstores for configured put needs. Have {}, need {}",
                puts.len(),
                needed_handlers
            ));
        }
        let (stats, result) = {
            let ctx = &ctx;
            async move {
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::BlobPuts);

                let mut too_slow_signal = maybe_create_too_slow_signal(ctx);
                let mut too_slow = false;
                let mut put_errors = HashMap::new();
                let mut handler_errors = HashMap::new();
                let mut handlers = FuturesUnordered::new();

                while let Some(result) = select_next_with_timeout(
                    &mut puts,
                    &mut handlers,
                    run_handlers_on_success || !put_errors.is_empty() || too_slow,
                    &mut too_slow_signal,
                )
                .await
                {
                    use Either::*;
                    match result {
                        Ok(Left(Ok(handler))) => {
                            handlers.push(handler);
                            // All puts have succeeded, no errors - we're done
                            if puts.is_empty() && put_errors.is_empty() {
                                if run_handlers_on_success {
                                    // Spawn off the handlers to ensure that all writes are logged.
                                    spawn_stream_completion(handlers);
                                }
                                // Inner statuses can differ, don't attempt to return them
                                return Ok(OverwriteStatus::NotChecked);
                            }
                        }
                        Ok(Left(Err((blobstore_id, e)))) => {
                            put_errors.insert(blobstore_id, e);
                        }
                        Err(Timeout) => {
                            // We ran into a timeout, so one (or a few) blobstores
                            // puts is taking too long. We don't want to wait for the
                            // slowest blobstore, so if we haven't been running handlers
                            // before we should start doing so now.
                            too_slow = true;
                            too_slow_signal.take();
                        }
                        Ok(Right(Ok(()))) => {
                            needed_handlers = needed_handlers.saturating_sub(1);
                            // Can only get here if at least one handler has been run, therefore need to ensure all handlers
                            // run.
                            if needed_handlers == 0 {
                                // Handlers were successful. Spawn off remaining puts and handler
                                // writes, then done
                                spawn_stream_completion(puts.and_then(|handler| handler));
                                spawn_stream_completion(handlers);
                                // Inner statuses can differ, don't attempt to return them
                                return Ok(OverwriteStatus::NotChecked);
                            }
                        }
                        Ok(Right(Err((blobstore_id, e)))) => {
                            handler_errors.insert(blobstore_id, e);
                        }
                    }
                }
                let mut errors = put_errors;
                errors.extend(handler_errors.into_iter());
                if errors.len() == 1 {
                    let (_, error) = errors.into_iter().next().unwrap();
                    Err(error)
                } else {
                    Err(ErrorKind::MultiplePutFailures(Arc::new(errors)).into())
                }
            }
            .timed()
            .await
        };

        if result.is_ok() {
            ctx.perf_counters().add_to_counter(
                PerfCounterType::BlobPutsTotalSize,
                value.len().try_into().unwrap_or(0),
            );
        }

        ctx.perf_counters().set_max_counter(
            PerfCounterType::BlobPutsMaxLatency,
            stats.completion_time.as_millis_unchecked() as i64,
        );
        result
    }
}

#[async_trait]
impl BlobstorePutOps for MultiplexedBlobstoreBase {
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

impl fmt::Debug for MultiplexedBlobstoreBase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MultiplexedBlobstoreBase: multiplex_id: {}",
            &self.multiplex_id
        )?;
        f.debug_map()
            .entries(self.blobstores.iter().map(|(ref k, ref v)| (k, v)))
            .finish()
    }
}

// If SessionClass::BackgroundUnlessTooSlow is set we generally want to wait
// until data is written to all blobstores, but there's one exception: we don't
// want to wait if some of the blobstores are too slow. In that case
// we'd rather use blobstore sync queue.
fn maybe_create_too_slow_signal(
    ctx: &CoreContext,
) -> Option<std::pin::Pin<Box<tokio::time::Sleep>>> {
    if matches!(
        ctx.session().session_class(),
        SessionClass::BackgroundUnlessTooSlow
    ) {
        let mut timeout =
            tunables::tunables().get_multiplex_blobstore_background_session_timeout_ms();

        if timeout <= 0 {
            timeout = 5000;
        }

        let timeout = timeout.try_into().unwrap();
        // tokio::time::Sleep is !Unpin, however later we use it in future::select
        // which requires a future to be Unpin. So to make it Unpin we put it in Pin<Box<...>>
        Some(Box::pin(tokio::time::sleep(Duration::from_millis(timeout))))
    } else {
        None
    }
}

async fn multiplexed_get_one<'a>(
    mut ctx: CoreContext,
    blobstore: &'a dyn BlobstorePutOps,
    blobstore_id: BlobstoreId,
    key: &'a str,
    operation: OperationType,
    mut scuba: MononokeScubaSampleBuilder,
) -> (BlobstoreId, Result<Option<BlobstoreGetData>, Error>) {
    let (pc, (stats, timeout_or_res)) = {
        let pc = ctx.fork_perf_counters();
        let ret = timeout(REQUEST_TIMEOUT, blobstore.get(&ctx, key))
            .timed()
            .await;
        (pc, ret)
    };
    let result = remap_timeout_result(timeout_or_res);
    record_get_stats(
        &mut scuba,
        &pc,
        stats,
        result.as_ref(),
        key,
        ctx.metadata().session_id().as_str(),
        operation,
        Some(blobstore_id),
        blobstore,
    );
    (blobstore_id, result)
}

fn multiplexed_get<'fut: 'iter, 'iter>(
    ctx: impl Borrow<CoreContext> + Clone + 'fut,
    blobstores: &'iter [(BlobstoreId, Arc<dyn BlobstorePutOps>)],
    key: impl Borrow<str> + Clone + 'fut,
    operation: OperationType,
    scuba: MononokeScubaSampleBuilder,
) -> impl Iterator<
    Item = impl Future<Output = (BlobstoreId, Result<Option<BlobstoreGetData>, Error>)> + 'fut,
> + 'iter {
    blobstores.iter().map(move |(blobstore_id, blobstore)| {
        let ctx = ctx.borrow().clone();
        cloned!(blobstore, blobstore_id, key, scuba);
        async move {
            multiplexed_get_one(
                ctx,
                blobstore.as_ref(),
                blobstore_id,
                key.borrow(),
                operation,
                scuba,
            )
            .await
        }
    })
}

fn multiplexed_is_present<'fut: 'iter, 'iter>(
    ctx: impl Borrow<CoreContext> + Clone + 'fut,
    blobstores: &'iter [(BlobstoreId, Arc<dyn BlobstorePutOps>)],
    key: impl Borrow<str> + Clone + 'fut,
    scuba: MononokeScubaSampleBuilder,
) -> impl Iterator<Item = impl Future<Output = (BlobstoreId, Result<BlobstoreIsPresent>)> + 'fut> + 'iter
{
    blobstores.iter().map(move |(blobstore_id, blobstore)| {
        let ctx = ctx.borrow().clone();
        cloned!(blobstore, blobstore_id, key, scuba);
        async move {
            multiplexed_is_present_one(ctx, blobstore.as_ref(), blobstore_id, key.borrow(), scuba)
                .await
        }
    })
}

async fn multiplexed_is_present_one<'a>(
    mut ctx: CoreContext,
    blobstore: &'a dyn BlobstorePutOps,
    blobstore_id: BlobstoreId,
    key: &'a str,
    mut scuba: MononokeScubaSampleBuilder,
) -> (BlobstoreId, Result<BlobstoreIsPresent>) {
    let is_present_timeout = {
        let duration = tunables().get_is_present_timeout_ms();
        Duration::from_millis(if duration > 0 {
            duration
        } else {
            DEFAULT_IS_PRESENT_TIMEOUT_MS
        } as u64)
    };
    let (pc, (stats, timeout_or_res)) = {
        let pc = ctx.fork_perf_counters();
        let ret = timeout(is_present_timeout, blobstore.is_present(&ctx, key))
            .timed()
            .await;
        (pc, ret)
    };
    let result = remap_timeout_result(timeout_or_res);
    record_is_present_stats(
        &mut scuba,
        &pc,
        stats,
        result.as_ref(),
        key,
        ctx.metadata().session_id().as_str(),
        Some(blobstore_id),
        blobstore,
    );
    (blobstore_id, result)
}
