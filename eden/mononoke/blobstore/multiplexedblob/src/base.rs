/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error};
use blobstore::{Blobstore, BlobstoreGetData};
use blobstore_stats::{record_get_stats, record_put_stats, OperationType};
use blobstore_sync_queue::OperationKey;
use cloned::cloned;
use context::{CoreContext, PerfCounterType, SessionClass};
use futures::{
    future::{join_all, select, BoxFuture, Either as FutureEither, FutureExt},
    stream::{FuturesUnordered, StreamExt, TryStreamExt},
};
use futures_stats::TimedFutureExt;
use itertools::{Either, Itertools};
use metaconfig_types::{BlobstoreId, MultiplexId};
use mononoke_types::BlobstoreBytes;
use scuba::ScubaSampleBuilder;
use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet},
    fmt,
    future::Future,
    iter::Iterator,
    num::{NonZeroU64, NonZeroUsize},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use thiserror::Error;
use time_ext::DurationExt;
use tokio::time::timeout;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

type BlobstoresWithEntry = HashSet<BlobstoreId>;
type BlobstoresReturnedNone = HashSet<BlobstoreId>;
type BlobstoresReturnedError = HashMap<BlobstoreId, Error>;

#[derive(Error, Debug, Clone)]
pub enum ErrorKind {
    #[error("Some blobstores failed, and other returned None: {0:?}")]
    SomeFailedOthersNone(Arc<BlobstoresReturnedError>),
    #[error("All blobstores failed: {0:?}")]
    AllFailed(Arc<BlobstoresReturnedError>),
    // Errors below this point are from ScrubBlobstore only. If they include an
    // Option<BlobstoreBytes>, this implies that this error is recoverable
    #[error(
        "Different blobstores have different values for this item: {0:?} differ, {1:?} do not have"
    )]
    ValueMismatch(Arc<BlobstoresWithEntry>, Arc<BlobstoresReturnedNone>),
    #[error("Some blobstores missing this item: {0:?}")]
    SomeMissingItem(Arc<BlobstoresReturnedNone>, Option<BlobstoreGetData>),
    #[error("Multiple failures on put: {0:?}")]
    MultiplePutFailures(Arc<BlobstoresReturnedError>),
}

/// This handler is called on each successful put to underlying blobstore,
/// for put to be considered successful this handler must return success.
/// It will be used to keep self-healing table up to date.
pub trait MultiplexedBlobstorePutHandler: Send + Sync {
    fn on_put<'out>(
        &'out self,
        ctx: &'out CoreContext,
        blobstore_id: BlobstoreId,
        multiplex_id: MultiplexId,
        operation_key: &'out OperationKey,
        key: &'out str,
    ) -> BoxFuture<'out, Result<(), Error>>;
}

pub struct MultiplexedBlobstoreBase {
    multiplex_id: MultiplexId,
    /// These are the "normal" blobstores, which are read from on `get`, and written to on `put`
    /// as part of normal operation. No special treatment is applied.
    blobstores: Arc<[(BlobstoreId, Arc<dyn Blobstore>)]>,
    /// Write-mostly blobstores are not normally read from on `get`, but take part in writes
    /// like a normal blobstore.
    ///
    /// There are two circumstances in which a write-mostly blobstore will be read from on `get`:
    /// 1. The normal blobstores (above) all return Ok(None) or Err for a blob.
    ///    In this case, we read as it's our only chance of returning data that we previously accepted
    ///    during a `put` operation.
    /// 2. When we're recording blobstore stats to Scuba on a `get` - in this case, the read executes
    ///    solely to gather statistics, and the result is discarded
    write_mostly_blobstores: Arc<[(BlobstoreId, Arc<dyn Blobstore>)]>,
    /// At least this many `put` and `on_put` pairs have to succeed before we consider a `put` successful
    /// This is meant to ensure that `put` fails if the data could end up lost (e.g. if a buggy experimental
    /// blobstore wins the `put` race).
    /// Note that if this is bigger than the number of blobstores, we will always fail writes
    minimum_successful_writes: NonZeroUsize,
    handler: Arc<dyn MultiplexedBlobstorePutHandler>,
    scuba: ScubaSampleBuilder,
    scuba_sample_rate: NonZeroU64,
}

impl MultiplexedBlobstoreBase {
    pub fn new(
        multiplex_id: MultiplexId,
        blobstores: Vec<(BlobstoreId, Arc<dyn Blobstore>)>,
        write_mostly_blobstores: Vec<(BlobstoreId, Arc<dyn Blobstore>)>,
        minimum_successful_writes: NonZeroUsize,
        handler: Arc<dyn MultiplexedBlobstorePutHandler>,
        mut scuba: ScubaSampleBuilder,
        scuba_sample_rate: NonZeroU64,
    ) -> Self {
        scuba.add_common_server_data();

        Self {
            multiplex_id,
            blobstores: blobstores.into(),
            write_mostly_blobstores: write_mostly_blobstores.into(),
            minimum_successful_writes,
            handler,
            scuba,
            scuba_sample_rate,
        }
    }

    pub async fn scrub_get(
        &self,
        ctx: &CoreContext,
        key: &String,
    ) -> Result<Option<BlobstoreGetData>, ErrorKind> {
        let mut scuba = self.scuba.clone();
        scuba.sampled(self.scuba_sample_rate);

        let results = join_all(
            multiplexed_get(
                ctx,
                self.blobstores.as_ref(),
                key,
                OperationType::ScrubGet,
                scuba.clone(),
            )
            .chain(multiplexed_get(
                ctx,
                self.write_mostly_blobstores.as_ref(),
                key,
                OperationType::ScrubGet,
                scuba,
            )),
        )
        .await;

        let (successes, errors): (HashMap<_, _>, HashMap<_, _>) =
            results.into_iter().partition_map(|(id, r)| match r {
                Ok(v) => Either::Left((id, v)),
                Err(v) => Either::Right((id, v)),
            });

        if successes.is_empty() {
            return Err(ErrorKind::AllFailed(errors.into()));
        }

        let mut best_value = None;
        let mut missing = HashSet::new();
        let mut answered = HashSet::new();
        let mut all_same = true;

        for (blobstore_id, value) in successes.into_iter() {
            if value.is_none() {
                missing.insert(blobstore_id);
            } else {
                answered.insert(blobstore_id);
                if best_value.is_none() {
                    best_value = value;
                } else if value.as_ref().map(BlobstoreGetData::as_bytes)
                    != best_value.as_ref().map(BlobstoreGetData::as_bytes)
                {
                    all_same = false;
                } else if value.as_ref().and_then(|v| v.as_meta().ctime())
                    > best_value.as_ref().and_then(|v| v.as_meta().ctime())
                {
                    best_value = value;
                }
            }
        }

        match (all_same, best_value.is_some(), missing.is_empty()) {
            (false, _, _) => Err(ErrorKind::ValueMismatch(
                Arc::new(answered),
                Arc::new(missing),
            )),
            (true, false, _) => {
                if errors.is_empty() {
                    Ok(None)
                } else {
                    Err(ErrorKind::SomeFailedOthersNone(errors.into()))
                }
            }
            (true, true, false) => Err(ErrorKind::SomeMissingItem(Arc::new(missing), best_value)),
            (true, true, true) => Ok(best_value),
        }
    }
}

fn remap_timeout_result<O>(
    timeout_or_result: Result<Result<O, Error>, tokio::time::Elapsed>,
) -> Result<O, Error> {
    timeout_or_result.unwrap_or_else(|_| Err(Error::msg("blobstore operation timeout")))
}

pub async fn inner_put(
    ctx: &CoreContext,
    mut scuba: ScubaSampleBuilder,
    write_order: &AtomicUsize,
    blobstore_id: BlobstoreId,
    blobstore: &dyn Blobstore,
    key: String,
    value: BlobstoreBytes,
) -> (BlobstoreId, Result<(), Error>) {
    let size = value.len();
    let (stats, timeout_or_res) = timeout(
        REQUEST_TIMEOUT,
        blobstore.put(ctx.clone(), key.clone(), value),
    )
    .timed()
    .await;
    let result = remap_timeout_result(timeout_or_res);
    record_put_stats(
        &mut scuba,
        stats,
        result.as_ref(),
        key,
        ctx.session_id().to_string(),
        OperationType::Put,
        size,
        Some(blobstore_id),
        Some(write_order.fetch_add(1, Ordering::Relaxed) + 1),
    );
    (blobstore_id, result)
}

// Workaround for Blobstore returning a static lifetime future
async fn blobstore_get(
    ctx: CoreContext,
    blobstores: Arc<[(BlobstoreId, Arc<dyn Blobstore>)]>,
    write_mostly_blobstores: Arc<[(BlobstoreId, Arc<dyn Blobstore>)]>,
    key: String,
    scuba: ScubaSampleBuilder,
) -> Result<Option<BlobstoreGetData>, Error> {
    let is_logged = scuba.sampling().is_logged();
    let blobstores_count = blobstores.len() + write_mostly_blobstores.len();

    let (stats, result) = {
        let ctx = &ctx;
        async move {
            let mut errors = HashMap::new();
            ctx.perf_counters()
                .increment_counter(PerfCounterType::BlobGets);

            let main_requests: FuturesUnordered<_> = multiplexed_get(
                ctx.clone(),
                blobstores.as_ref(),
                &key,
                OperationType::Get,
                scuba.clone(),
            )
            .collect();
            let write_mostly_requests: FuturesUnordered<_> = multiplexed_get(
                ctx.clone(),
                write_mostly_blobstores.as_ref(),
                &key,
                OperationType::Get,
                scuba,
            )
            .collect();

            // `chain` here guarantees that `main_requests` is empty before it starts
            // polling anything in `write_mostly_requests`
            let mut requests = main_requests.chain(write_mostly_requests);
            while let Some(result) = requests.next().await {
                match result {
                    (_, Ok(Some(mut value))) => {
                        if is_logged {
                            // Allow the other requests to complete so that we can record some
                            // metrics for the blobstore. This will also log metrics for write-mostly
                            // blobstores, which helps us decide whether they're good
                            tokio::spawn(requests.for_each(|_| async {}));
                        }
                        // Return the blob that won the race
                        value.remove_ctime();
                        return Ok(Some(value));
                    }
                    (blobstore_id, Err(error)) => {
                        errors.insert(blobstore_id, error);
                    }
                    (_, Ok(None)) => (),
                }
            }

            if errors.is_empty() {
                // All blobstores must have returned None, as Some would have triggered a return,
                Ok(None)
            } else {
                if errors.len() == blobstores_count {
                    Err(ErrorKind::AllFailed(Arc::new(errors)))
                } else {
                    Err(ErrorKind::SomeFailedOthersNone(Arc::new(errors)))
                }
            }
        }
        .timed()
        .await
    };

    ctx.perf_counters().set_max_counter(
        PerfCounterType::BlobGetsMaxLatency,
        stats.completion_time.as_millis_unchecked() as i64,
    );
    Ok(result?)
}

fn spawn_stream_completion(s: impl StreamExt + Send + 'static) {
    tokio::spawn(s.for_each(|_| async {}));
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

impl Blobstore for MultiplexedBlobstoreBase {
    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        let mut scuba = self.scuba.clone();
        let blobstores = self.blobstores.clone();
        let write_mostly_blobstores = self.write_mostly_blobstores.clone();
        scuba.sampled(self.scuba_sample_rate);

        async move { blobstore_get(ctx, blobstores, write_mostly_blobstores, key, scuba).await }
            .boxed()
    }

    fn put(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        let write_order = Arc::new(AtomicUsize::new(0));
        let operation_key = OperationKey::gen();
        let mut needed_handlers: usize = self.minimum_successful_writes.into();
        let run_handlers_on_success = match ctx.session().session_class() {
            SessionClass::UserWaiting => true,
            SessionClass::Background => false,
        };

        let mut puts: FuturesUnordered<_> = self
            .blobstores
            .iter()
            .chain(self.write_mostly_blobstores.iter())
            .cloned()
            .map({
                |(blobstore_id, blobstore)| {
                    cloned!(
                        self.handler,
                        self.multiplex_id,
                        self.scuba,
                        ctx,
                        write_order,
                        key,
                        value,
                        operation_key
                    );
                    async move {
                        let (blobstore_id, res) = inner_put(
                            &ctx,
                            scuba,
                            write_order.as_ref(),
                            blobstore_id,
                            blobstore.as_ref(),
                            key.clone(),
                            value,
                        )
                        .await;
                        res.map_err(|err| (blobstore_id, err))?;
                        // Return the on_put handler
                        Ok(async move {
                            let res = handler
                                .on_put(&ctx, blobstore_id, multiplex_id, &operation_key, &key)
                                .await;
                            res.map_err(|err| (blobstore_id, err))
                        })
                    }
                }
            })
            .collect();

        async move {
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

                    let mut put_errors = HashMap::new();
                    let mut handlers = FuturesUnordered::new();

                    while let Some(result) = select_next(
                        &mut puts,
                        &mut handlers,
                        run_handlers_on_success || !put_errors.is_empty(),
                    )
                    .await
                    {
                        use Either::*;
                        match result {
                            Left(Ok(handler)) => {
                                handlers.push(handler);
                                // All puts have succeeded, no errors - we're done
                                if puts.is_empty() && put_errors.is_empty() {
                                    if run_handlers_on_success {
                                        // Spawn off the handlers to ensure that all writes are logged.
                                        spawn_stream_completion(handlers);
                                    }
                                    return Ok(());
                                }
                            }
                            Left(Err((blobstore_id, e))) => {
                                put_errors.insert(blobstore_id, e);
                            }
                            Right(Ok(())) => {
                                needed_handlers = needed_handlers.saturating_sub(1);
                                // Can only get here if at least one handler has been run, therefore need to ensure all handlers
                                // run.
                                if needed_handlers == 0 {
                                    // Handlers were successful. Spawn off remaining puts and handler
                                    // writes, then done
                                    spawn_stream_completion(puts.and_then(|handler| handler));
                                    spawn_stream_completion(handlers);
                                    return Ok(());
                                }
                            }
                            Right(Err((blobstore_id, e))) => {
                                put_errors.insert(blobstore_id, e);
                            }
                        }
                    }
                    if put_errors.len() == 1 {
                        let (_, put_error) = put_errors.drain().next().unwrap();
                        Err(put_error)
                    } else {
                        Err(ErrorKind::MultiplePutFailures(Arc::new(put_errors)).into())
                    }
                }
                .timed()
                .await
            };

            ctx.perf_counters().set_max_counter(
                PerfCounterType::BlobPutsMaxLatency,
                stats.completion_time.as_millis_unchecked() as i64,
            );
            result
        }
        .boxed()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<'static, Result<bool, Error>> {
        let blobstores_count = self.blobstores.len() + self.write_mostly_blobstores.len();

        let main_requests: FuturesUnordered<_> = self
            .blobstores
            .iter()
            .cloned()
            .map(|(blobstore_id, blobstore)| {
                let ctx = ctx.clone();
                let key = key.clone();
                async move { (blobstore_id, blobstore.is_present(ctx, key).await) }
            })
            .collect();
        let write_mostly_requests: FuturesUnordered<_> = self
            .write_mostly_blobstores
            .iter()
            .cloned()
            .map(|(blobstore_id, blobstore)| {
                let ctx = ctx.clone();
                let key = key.clone();
                async move { (blobstore_id, blobstore.is_present(ctx, key).await) }
            })
            .collect();

        // `chain` here guarantees that `main_requests` is empty before it starts
        // polling anything in `write_mostly_requests`
        let mut requests = main_requests.chain(write_mostly_requests);
        async move {
            let (stats, result) = {
                let ctx = &ctx;
                async move {
                    let mut errors = HashMap::new();
                    ctx.perf_counters()
                        .increment_counter(PerfCounterType::BlobPresenceChecks);
                    while let Some(result) = requests.next().await {
                        match result {
                            (_, Ok(true)) => {
                                return Ok(true);
                            }
                            (blobstore_id, Err(error)) => {
                                errors.insert(blobstore_id, error);
                            }
                            (_, Ok(false)) => (),
                        }
                    }
                    if errors.is_empty() {
                        Ok(false)
                    } else {
                        if errors.len() == blobstores_count {
                            Err(ErrorKind::AllFailed(Arc::new(errors)))
                        } else {
                            Err(ErrorKind::SomeFailedOthersNone(Arc::new(errors)))
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
        .boxed()
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

async fn multiplexed_get_one(
    ctx: impl Borrow<CoreContext>,
    blobstore: Arc<dyn Blobstore>,
    blobstore_id: BlobstoreId,
    key: String,
    operation: OperationType,
    mut scuba: ScubaSampleBuilder,
) -> (BlobstoreId, Result<Option<BlobstoreGetData>, Error>) {
    let (stats, timeout_or_res) = timeout(
        REQUEST_TIMEOUT,
        blobstore.get(ctx.borrow().clone(), key.clone()),
    )
    .timed()
    .await;
    let result = remap_timeout_result(timeout_or_res);
    record_get_stats(
        &mut scuba,
        stats,
        result.as_ref(),
        key,
        ctx.borrow().session_id().to_string(),
        operation,
        Some(blobstore_id),
    );
    (blobstore_id, result)
}

fn multiplexed_get<'fut: 'iter, 'iter>(
    ctx: impl Borrow<CoreContext> + Clone + 'fut,
    blobstores: &'iter [(BlobstoreId, Arc<dyn Blobstore>)],
    key: &'iter String,
    operation: OperationType,
    scuba: ScubaSampleBuilder,
) -> impl Iterator<
    Item = impl Future<Output = (BlobstoreId, Result<Option<BlobstoreGetData>, Error>)> + 'fut,
> + 'iter {
    blobstores.iter().map(move |(blobstore_id, blobstore)| {
        multiplexed_get_one(
            ctx.clone(),
            blobstore.clone(),
            *blobstore_id,
            key.clone(),
            operation,
            scuba.clone(),
        )
    })
}
