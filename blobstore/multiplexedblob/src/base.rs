/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use blobstore::Blobstore;
use cloned::cloned;
use context::{CoreContext, PerfCounterType};
use failure::err_msg;
use failure_ext::{Error, Fail};
use futures::future::{self, Future, Loop};
use futures_ext::{BoxFuture, FutureExt};
use futures_stats::Timed;
use itertools::{Either, Itertools};
use metaconfig_types::BlobstoreId;
use mononoke_types::BlobstoreBytes;
use rand::{thread_rng, Rng};
use scuba::ScubaSampleBuilder;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use time_ext::DurationExt;
use tokio::executor::spawn;
use tokio::prelude::FutureExt as TokioFutureExt;
use tokio::timer::timeout::Error as TimeoutError;

const SLOW_REQUEST_THRESHOLD: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
const SAMPLING_THRESHOLD: f32 = 1.0 - (1.0 / 100.0);

type BlobstoresWithEntry = HashSet<BlobstoreId>;
type BlobstoresReturnedNone = HashSet<BlobstoreId>;
type BlobstoresReturnedError = HashMap<BlobstoreId, Error>;

#[derive(Fail, Debug, Clone)]
pub enum ErrorKind {
    #[fail(display = "Some blobstores failed, and other returned None: {:?}", _0)]
    SomeFailedOthersNone(Arc<BlobstoresReturnedError>),
    #[fail(display = "All blobstores failed: {:?}", _0)]
    AllFailed(Arc<BlobstoresReturnedError>),
    // Errors below this point are from ScrubBlobstore only. If they include an
    // Option<BlobstoreBytes>, this implies that this error is recoverable
    #[fail(
        display = "Different blobstores have different values for this item: {:?} differ, {:?} do not have",
        _0, _1
    )]
    ValueMismatch(Arc<BlobstoresWithEntry>, Arc<BlobstoresReturnedNone>),
    #[fail(display = "Some blobstores missing this item: {:?}", _0)]
    SomeMissingItem(Arc<BlobstoresReturnedNone>, Option<BlobstoreBytes>),
}

/// This handler is called on each successful put to underlying blobstore,
/// for put to be considered successful this handler must return success.
/// It will be used to keep self-healing table up to date.
pub trait MultiplexedBlobstorePutHandler: Send + Sync {
    fn on_put(
        &self,
        ctx: CoreContext,
        blobstore_id: BlobstoreId,
        key: String,
    ) -> BoxFuture<(), Error>;
}

pub struct MultiplexedBlobstoreBase {
    blobstores: Arc<[(BlobstoreId, Arc<dyn Blobstore>)]>,
    handler: Arc<dyn MultiplexedBlobstorePutHandler>,
    scuba: ScubaSampleBuilder,
}

impl MultiplexedBlobstoreBase {
    pub fn new(
        blobstores: Vec<(BlobstoreId, Arc<dyn Blobstore>)>,
        handler: Arc<dyn MultiplexedBlobstorePutHandler>,
        mut scuba: ScubaSampleBuilder,
    ) -> Self {
        scuba.add_common_server_data();

        Self {
            blobstores: blobstores.into(),
            handler,
            scuba,
        }
    }

    fn get_from_all(
        &self,
        ctx: &CoreContext,
        key: &String,
        operation: &'static str,
        should_log: bool,
    ) -> Vec<BoxFuture<(BlobstoreId, Option<BlobstoreBytes>), (BlobstoreId, Error)>> {
        self.blobstores
            .iter()
            .map(|&(blobstore_id, ref blobstore)| {
                blobstore
                    .get(ctx.clone(), key.clone())
                    .map({
                        cloned!(blobstore_id);
                        move |val| (blobstore_id, val)
                    })
                    .timeout(REQUEST_TIMEOUT)
                    .map_err({
                        cloned!(blobstore_id);
                        move |error| (blobstore_id, remap_timeout_error(error))
                    })
                    .timed({
                        cloned!(key);
                        {
                            let session = ctx.session().clone();
                            cloned!(mut self.scuba);
                            move |stats, result| {
                                if !should_log {
                                    return future::ok(());
                                }

                                scuba
                                    .add("key", key.clone())
                                    .add("operation", operation)
                                    .add("blobstore_id", blobstore_id)
                                    .add(
                                        "completion_time",
                                        stats.completion_time.as_micros_unchecked(),
                                    );

                                // log session id only for slow requests
                                if stats.completion_time >= SLOW_REQUEST_THRESHOLD {
                                    scuba.add("session", session.to_string());
                                }

                                match result {
                                    Ok((_, Some(data))) => {
                                        scuba.add("size", data.len());
                                    }
                                    Err((_, error)) => {
                                        scuba.add("error", error.to_string());
                                    }
                                    Ok((_, None)) => {}
                                }
                                scuba.log();

                                future::ok(())
                            }
                        }
                    })
            })
            .collect()
    }

    pub fn scrub_get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        let should_log = thread_rng().gen::<f32>() > SAMPLING_THRESHOLD;

        let requests = self
            .get_from_all(&ctx, &key, "scrub_get", should_log)
            .into_iter()
            .map(|f| f.then(|r| Ok(r)));
        future::join_all(requests)
            .and_then(|results| {
                let (successes, errors): (HashMap<_, _>, HashMap<_, _>) =
                    results.into_iter().partition_map(|r| match r {
                        Ok(v) => Either::Left(v),
                        Err(v) => Either::Right(v),
                    });

                if successes.is_empty() {
                    future::err(ErrorKind::AllFailed(errors.into()).into())
                } else {
                    let mut best_value = None;
                    let mut missing = HashSet::new();
                    let mut answered = HashSet::new();
                    let mut all_same = true;

                    for (blobstore_id, value) in successes.into_iter() {
                        if value.is_none() {
                            missing.insert(blobstore_id);
                        } else {
                            answered.insert(blobstore_id);
                        }
                        if best_value.is_none() {
                            best_value = value;
                        } else if value != best_value {
                            all_same = false;
                        }
                    }

                    match (all_same, missing.is_empty() && best_value.is_some()) {
                        (false, _) => future::err(
                            ErrorKind::ValueMismatch(Arc::new(answered), Arc::new(missing)).into(),
                        ),
                        (true, false) => future::err(
                            ErrorKind::SomeMissingItem(Arc::new(missing), best_value).into(),
                        ),
                        (true, true) => future::ok(best_value),
                    }
                }
            })
            .boxify()
    }
}

fn remap_timeout_error(err: TimeoutError<Error>) -> Error {
    match err.into_inner() {
        Some(err) => err,
        None => err_msg("blobstore operation timeout"),
    }
}

impl Blobstore for MultiplexedBlobstoreBase {
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::BlobGets);

        let should_log = thread_rng().gen::<f32>() > SAMPLING_THRESHOLD;
        let requests = self.get_from_all(&ctx, &key, "get", should_log);
        let state = (
            requests,                             // pending requests
            HashMap::<BlobstoreId, Error>::new(), // previous errors
        );
        let blobstores_count = self.blobstores.len();
        future::loop_fn(state, move |(requests, mut errors)| {
            future::select_all(requests).then({
                move |result| {
                    let requests = match result {
                        Ok(((_, value @ Some(_)), _, requests)) => {
                            if should_log {
                                // Allow the other requests to complete so that we can record some
                                // metrics for the blobstore.
                                let requests_fut = future::join_all(
                                    requests.into_iter().map(|request| request.then(|_| Ok(()))),
                                )
                                .map(|_| ());
                                spawn(requests_fut);
                            }
                            return future::ok(Loop::Break(value));
                        }
                        Ok(((_, None), _, requests)) => requests,
                        Err(((blobstore_id, error), _, requests)) => {
                            errors.insert(blobstore_id, error);
                            requests
                        }
                    };
                    if requests.is_empty() {
                        if errors.is_empty() {
                            future::ok(Loop::Break(None))
                        } else {
                            let error = if errors.len() == blobstores_count {
                                ErrorKind::AllFailed(errors.into())
                            } else {
                                ErrorKind::SomeFailedOthersNone(errors.into())
                            };
                            future::err(error.into())
                        }
                    } else {
                        future::ok(Loop::Continue((requests, errors)))
                    }
                }
            })
        })
        .timed(move |stats, _| {
            ctx.perf_counters().set_max_counter(
                PerfCounterType::BlobGetsMaxLatency,
                stats.completion_time.as_millis_unchecked() as i64,
            );
            Ok(())
        })
        .boxify()
    }

    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::BlobPuts);
        let size = value.len();
        let write_order = Arc::new(AtomicUsize::new(0));
        let should_log = thread_rng().gen::<f32>() > SAMPLING_THRESHOLD;
        let session = ctx.session();

        let puts = self
            .blobstores
            .iter()
            .map(|(blobstore_id, blobstore)| {
                blobstore
                    .put(ctx.clone(), key.clone(), value.clone())
                    .timeout(REQUEST_TIMEOUT)
                    .map({
                        cloned!(blobstore_id);
                        move |_| blobstore_id
                    })
                    .map_err(remap_timeout_error)
                    .timed({
                        cloned!(blobstore_id, key, write_order, size, session, mut self.scuba);
                        move |stats, result| {
                            if should_log {
                                scuba
                                    .add("key", key.clone())
                                    .add("operation", "put")
                                    .add("blobstore_id", blobstore_id)
                                    .add("size", size)
                                    .add(
                                        "completion_time",
                                        stats.completion_time.as_micros_unchecked(),
                                    );

                                match result {
                                    Ok(_) => scuba.add(
                                        "write_order",
                                        write_order.fetch_add(1, Ordering::SeqCst),
                                    ),
                                    Err(error) => scuba.add("error", error.to_string()),
                                };

                                // log session uuid only for slow requests
                                if stats.completion_time >= SLOW_REQUEST_THRESHOLD {
                                    scuba.add("session", session.to_string());
                                }
                                scuba.log();
                            }

                            Ok(())
                        }
                    })
            })
            .collect();

        multiplexed_put(ctx.clone(), self.handler.clone(), key, puts)
            .timed(move |stats, _| {
                ctx.perf_counters().set_max_counter(
                    PerfCounterType::BlobPutsMaxLatency,
                    stats.completion_time.as_millis_unchecked() as i64,
                );
                Ok(())
            })
            .boxify()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::BlobPresenceChecks);
        let requests = self
            .blobstores
            .iter()
            .map(|&(blobstore_id, ref blobstore)| {
                blobstore
                    .is_present(ctx.clone(), key.clone())
                    .map_err(move |error| (blobstore_id, error))
            })
            .collect();
        let state = (
            requests,                             // pending requests
            HashMap::<BlobstoreId, Error>::new(), // previous errors
        );
        let blobstores_count = self.blobstores.len();
        future::loop_fn(state, move |(requests, mut errors)| {
            future::select_all(requests).then({
                move |result| {
                    let requests = match result {
                        Ok((true, ..)) => return future::ok(Loop::Break(true)),
                        Ok((false, _, requests)) => requests,
                        Err(((blobstore_id, error), _, requests)) => {
                            errors.insert(blobstore_id, error);
                            requests
                        }
                    };
                    if requests.is_empty() {
                        if errors.is_empty() {
                            future::ok(Loop::Break(false))
                        } else {
                            let error = if errors.len() == blobstores_count {
                                ErrorKind::AllFailed(errors.into())
                            } else {
                                ErrorKind::SomeFailedOthersNone(errors.into())
                            };
                            future::err(error.into())
                        }
                    } else {
                        future::ok(Loop::Continue((requests, errors)))
                    }
                }
            })
        })
        .timed(move |stats, _| {
            ctx.perf_counters().set_max_counter(
                PerfCounterType::BlobPresenceChecksMaxLatency,
                stats.completion_time.as_millis_unchecked() as i64,
            );
            Ok(())
        })
        .boxify()
    }
}

impl fmt::Debug for MultiplexedBlobstoreBase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MultiplexedBlobstoreBase")?;
        f.debug_map()
            .entries(self.blobstores.iter().map(|(ref k, ref v)| (k, v)))
            .finish()
    }
}

fn multiplexed_put<F: Future<Item = BlobstoreId, Error = Error> + Send + 'static>(
    ctx: CoreContext,
    handler: Arc<dyn MultiplexedBlobstorePutHandler>,
    key: String,
    puts: Vec<F>,
) -> impl Future<Item = (), Error = Error> {
    future::select_ok(puts).and_then(move |(blobstore_id, other_puts)| {
        finish_put(ctx, handler, key, blobstore_id, other_puts)
    })
}

fn finish_put<F: Future<Item = BlobstoreId, Error = Error> + Send + 'static>(
    ctx: CoreContext,
    handler: Arc<dyn MultiplexedBlobstorePutHandler>,
    key: String,
    blobstore_id: BlobstoreId,
    other_puts: Vec<F>,
) -> BoxFuture<(), Error> {
    // Ocne we finished a put in one blobstore, we want to return once this blob is in a position
    // to be replicated properly to the multiplexed stores. This can happen in two cases:
    // - We wrote it to the SQL queue that will replicate it to other blobstores.
    // - We wrote it to all the blobstores.
    // As soon as either of those things happen, we can report the put as successful.
    use futures::future::Either;

    let queue_write = handler.on_put(ctx.clone(), blobstore_id, key.clone());

    let rest_put = if other_puts.len() > 0 {
        multiplexed_put(ctx, handler, key, other_puts).left_future()
    } else {
        // We have no remaining puts to perform, which means we've successfully written to all
        // blobstores.
        future::ok(()).right_future()
    };

    queue_write
        .select2(rest_put)
        .then(|res| match res {
            Ok(Either::A((_, rest_put))) => {
                // Blobstore queue write succeeded. Spawn the rest of the puts to give them a
                // chance to complete, but we're done.
                spawn(rest_put.discard());
                future::ok(()).boxify()
            }
            Ok(Either::B((_, queue_write))) => {
                // Remaininig puts succeeded (note that this might mean one of them and its
                // corresponding SQL write succeeded). Spawn the queue write, but we're done.
                spawn(queue_write.discard());
                future::ok(()).boxify()
            }
            Err(Either::A((_, rest_put))) => {
                // Blobstore queue write failed. We might still succeed if the other puts succeed.
                rest_put.boxify()
            }
            Err(Either::B((_, queue_write))) => {
                // Remaining puts failed. We might sitll succeed if the queue write succeeds.
                queue_write
            }
        })
        .boxify()
}
