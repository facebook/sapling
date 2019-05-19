// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobstore::Blobstore;
use cloned::cloned;
use context::CoreContext;
use failure::err_msg;
use failure_ext::{Error, Fail};
use futures::future::{self, Future, Loop};
use futures_ext::{BoxFuture, FutureExt};
use futures_stats::Timed;
use lazy_static::lazy_static;
use metaconfig_types::BlobstoreId;
use mononoke_types::BlobstoreBytes;
use rand::{thread_rng, Rng};
use scuba::{ScubaClient, ScubaSample};
use std::collections::HashMap;
use std::env;
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

lazy_static! {
    static ref TW_STATS: Vec<(&'static str, String)> = {
        let mut stats = Vec::new();
        if let (Ok(cluster), Ok(user), Ok(name)) = (
            env::var("TW_JOB_CLUSTER"),
            env::var("TW_JOB_USER"),
            env::var("TW_JOB_NAME"),
        ) {
            stats.push(("tw_handle", format!("{}/{}/{}", cluster, user, name)));
        };
        if let Ok(smc_tier) = env::var("SMC_TIERS") {
            stats.push(("server_tier", smc_tier));
        }
        if let Ok(tw_task_id) = env::var("TW_TASK_ID") {
            stats.push(("tw_task_id", tw_task_id));
        }
        stats
    };
}

#[derive(Fail, Debug, Clone)]
pub enum ErrorKind {
    #[fail(display = "Some blobstores failed, and other returned None: {:?}", _0)]
    SomeFailedOthersNone(Arc<HashMap<BlobstoreId, Error>>),
    #[fail(display = "All blobstores failed: {:?}", _0)]
    AllFailed(Arc<HashMap<BlobstoreId, Error>>),
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
    blobstores: Arc<[(BlobstoreId, Arc<Blobstore>)]>,
    handler: Arc<MultiplexedBlobstorePutHandler>,
    scuba_logger: Option<Arc<ScubaClient>>,
}

impl MultiplexedBlobstoreBase {
    pub fn new(
        blobstores: Vec<(BlobstoreId, Arc<Blobstore>)>,
        handler: Arc<MultiplexedBlobstorePutHandler>,
        scuba_logger: Option<Arc<ScubaClient>>,
    ) -> Self {
        Self {
            blobstores: blobstores.into(),
            handler,
            scuba_logger,
        }
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
        let should_log = thread_rng().gen::<f32>() > SAMPLING_THRESHOLD;
        let requests: Vec<_> = self
            .blobstores
            .iter()
            .map(|&(blobstore_id, ref blobstore)| {
                blobstore
                    .get(ctx.clone(), key.clone())
                    .timeout(REQUEST_TIMEOUT)
                    .map_err({
                        cloned!(blobstore_id);
                        move |error| (blobstore_id, remap_timeout_error(error))
                    })
                    .timed({
                        let session = ctx.session().clone();
                        cloned!(self.scuba_logger);
                        move |stats, result| {
                            if !should_log {
                                return future::ok(());
                            }

                            if let (Ok(Some(data)), Some(ref scuba_logger)) = (result, scuba_logger)
                            {
                                let mut sample = ScubaSample::new();
                                sample
                                    .add("operation", "get")
                                    .add("blobstore_id", blobstore_id)
                                    .add("size", data.len())
                                    .add(
                                        "completion_time",
                                        stats.completion_time.as_micros_unchecked(),
                                    );
                                for (key, value) in TW_STATS.iter() {
                                    sample.add(*key, value.clone());
                                }
                                // logging session uuid only for slow requests
                                if stats.completion_time >= SLOW_REQUEST_THRESHOLD {
                                    sample.add("session", session.to_string());
                                }

                                match result {
                                    Ok(Some(data)) => {
                                        sample.add("size", data.len());
                                    }
                                    Err((_, error)) => {
                                        sample.add("error", error.to_string());
                                    }
                                    Ok(None) => {}
                                }
                                scuba_logger.log(&sample);
                            }

                            future::ok(())
                        }
                    })
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
                        Ok((value @ Some(_), _, requests)) => {
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
                        Ok((None, _, requests)) => requests,
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
        .boxify()
    }

    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        let size = value.len();
        let write_order = Arc::new(AtomicUsize::new(0));
        let should_log = thread_rng().gen::<f32>() > SAMPLING_THRESHOLD;

        let requests = self.blobstores.iter().map(|(blobstore_id, blobstore)| {
            blobstore
                .put(ctx.clone(), key.clone(), value.clone())
                .timeout(REQUEST_TIMEOUT)
                .map_err({ move |error| remap_timeout_error(error) })
                .and_then({
                    cloned!(ctx, key, blobstore_id, self.handler);
                    move |_| handler.on_put(ctx, blobstore_id, key)
                })
                .timed({
                    let session = ctx.session().clone();
                    cloned!(blobstore_id, write_order, size, self.scuba_logger);
                    move |stats, result| {
                        if should_log {
                            if let Some(scuba_logger) = scuba_logger {
                                let mut sample = ScubaSample::new();
                                sample
                                    .add("operation", "put")
                                    .add("blobstore_id", blobstore_id)
                                    .add("size", size)
                                    .add(
                                        "completion_time",
                                        stats.completion_time.as_micros_unchecked(),
                                    );
                                match result {
                                    Ok(_) => sample.add(
                                        "write_order",
                                        write_order.fetch_add(1, Ordering::SeqCst),
                                    ),
                                    Err(error) => sample.add("error", error.to_string()),
                                };
                                for (key, value) in TW_STATS.iter() {
                                    sample.add(*key, value.clone());
                                }
                                // logging session uuid only for slow requests
                                if stats.completion_time >= SLOW_REQUEST_THRESHOLD {
                                    sample.add("session", session.to_string());
                                }
                                scuba_logger.log(&sample);
                            }
                        }
                        future::ok(())
                    }
                })
        });

        future::select_ok(requests)
            .map(|(_, requests)| {
                let requests_fut =
                    future::join_all(requests.into_iter().map(|request| request.then(|_| Ok(()))))
                        .map(|_| ());
                spawn(requests_fut);
            })
            .boxify()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
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
