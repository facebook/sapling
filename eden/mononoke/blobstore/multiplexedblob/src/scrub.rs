/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::base::{inner_put, ErrorKind};
use crate::queue::MultiplexedBlobstore;

use anyhow::Error;
use blobstore::{Blobstore, BlobstoreGetData, BlobstoreMetadata};
use blobstore_sync_queue::BlobstoreSyncQueue;
use cloned::cloned;
use context::CoreContext;
use futures::{
    compat::Future01CompatExt,
    future::{BoxFuture, FutureExt, TryFutureExt},
};
use futures_ext::FutureExt as _;
use futures_old::future::{self, Future};
use metaconfig_types::{BlobstoreId, MultiplexId, ScrubAction};
use mononoke_types::BlobstoreBytes;
use scuba::ScubaSampleBuilder;
use slog::{info, warn};
use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroU64;
use std::sync::{atomic::AtomicUsize, Arc};

pub trait ScrubHandler: Send + Sync {
    /// Called when one of the inner stores required repair.
    fn on_repair(
        &self,
        ctx: &CoreContext,
        blobstore_id: BlobstoreId,
        key: &str,
        is_repaired: bool,
        meta: &BlobstoreMetadata,
    );
}

pub struct LoggingScrubHandler {
    quiet: bool,
}

impl LoggingScrubHandler {
    pub fn new(quiet: bool) -> Self {
        Self { quiet }
    }
}

impl ScrubHandler for LoggingScrubHandler {
    fn on_repair(
        &self,
        ctx: &CoreContext,
        blobstore_id: BlobstoreId,
        key: &str,
        is_repaired: bool,
        _meta: &BlobstoreMetadata,
    ) {
        if !self.quiet {
            if is_repaired {
                info!(
                    ctx.logger(),
                    "scrub: blobstore_id {:?} repaired for {}", &blobstore_id, &key
                );
            } else {
                warn!(
                    ctx.logger(),
                    "scrub: blobstore_id {:?} not repaired for {}", &blobstore_id, &key
                );
            }
        }
    }
}

#[derive(Clone)]
pub struct ScrubBlobstore {
    inner: MultiplexedBlobstore,
    scrub_handler: Arc<dyn ScrubHandler>,
    scrub_action: ScrubAction,
    scuba: ScubaSampleBuilder,
    scrub_stores: Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
    queue: Arc<dyn BlobstoreSyncQueue>,
}

impl ScrubBlobstore {
    pub fn new(
        multiplex_id: MultiplexId,
        blobstores: Vec<(BlobstoreId, Arc<dyn Blobstore>)>,
        queue: Arc<dyn BlobstoreSyncQueue>,
        scuba: ScubaSampleBuilder,
        scuba_sample_rate: NonZeroU64,
        scrub_handler: Arc<dyn ScrubHandler>,
        scrub_action: ScrubAction,
    ) -> Self {
        let inner = MultiplexedBlobstore::new(
            multiplex_id,
            blobstores.clone(),
            queue.clone(),
            scuba.clone(),
            scuba_sample_rate,
        );
        Self {
            inner,
            scrub_handler,
            scrub_action,
            scuba,
            scrub_stores: Arc::new(
                blobstores
                    .into_iter()
                    .collect::<HashMap<BlobstoreId, Arc<dyn Blobstore>>>(),
            ),
            queue,
        }
    }
}

impl fmt::Debug for ScrubBlobstore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScrubBlobstore")
            .field("inner", &self.inner)
            .finish()
    }
}

impl Blobstore for ScrubBlobstore {
    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        self.inner
            .blobstore
            .scrub_get(ctx.clone(), key.clone())
            .then({
                cloned!(
                    ctx,
                    self.scrub_stores,
                    self.scrub_handler,
                    self.scuba,
                    self.scrub_action,
                    self.queue,
                );
                move |result| {
                    let needs_repair = match result {
                        Ok(value) => return future::ok(value).left_future(),
                        Err(error) => match error.clone() {
                            ErrorKind::SomeFailedOthersNone(_) => {
                                // MultiplexedBlobstore returns Ok(None) here if queue is empty for the key
                                // and Error otherwise. Scrub does likewise.
                                return queue
                                    .get(ctx, key)
                                    .compat()
                                    .and_then(move |entries| {
                                        if entries.is_empty() {
                                            // No pending write for the key, it really is None
                                            Ok(None)
                                        } else {
                                            // Pending write, we don't know what the value is.
                                            Err(error.into())
                                        }
                                    })
                                    .boxify()
                                    .right_future();
                            }
                            ErrorKind::SomeMissingItem(missing_reads, value) => {
                                let value = match value {
                                    // If there is no value no chance of repair
                                    None => {
                                        return future::err(error.into()).boxify().right_future()
                                    }
                                    Some(value) => value,
                                };
                                queue
                                    .get(ctx.clone(), key.clone())
                                    .compat()
                                    .map(move |entries| {
                                        let mut needs_repair: HashMap<
                                            BlobstoreId,
                                            Arc<dyn Blobstore>,
                                        > = HashMap::new();

                                        for k in missing_reads.iter() {
                                            match scrub_stores.get(k) {
                                                Some(s) => {
                                                    // If key has no entries on the queue it needs repair.
                                                    // Don't check individual stores in entries as that is a race vs multiplexed_put().
                                                    //
                                                    // TODO compare timestamp vs original_timestamp to still repair on
                                                    // really old entries, will need schema change.
                                                    if entries.is_empty() {
                                                        needs_repair.insert(*k, s.clone());
                                                    }
                                                }
                                                None => (),
                                            }
                                        }
                                        (needs_repair, value)
                                    })
                            }
                            _ => return future::err(error.into()).boxify().right_future(),
                        },
                    };

                    needs_repair
                        .and_then(move |(needs_repair, value)| {
                            if scrub_action == ScrubAction::ReportOnly {
                                for id in needs_repair.keys() {
                                    scrub_handler.on_repair(
                                        &ctx,
                                        *id,
                                        &key,
                                        false,
                                        value.as_meta(),
                                    );
                                }
                                future::ok(Some(value)).left_future()
                            } else {
                                // inner_put to the stores that need it.
                                let order = Arc::new(AtomicUsize::new(0));
                                let mut repair_puts = vec![];
                                for (id, store) in needs_repair.into_iter() {
                                    cloned!(ctx, scuba, key, value, order);
                                    let repair = inner_put(
                                        ctx.clone(),
                                        scuba,
                                        order,
                                        id,
                                        store,
                                        key.clone(),
                                        value.as_bytes().clone(),
                                    )
                                    .then({
                                        cloned!(ctx, scrub_handler, key);
                                        move |res| {
                                            scrub_handler.on_repair(
                                                &ctx,
                                                id,
                                                &key,
                                                res.is_ok(),
                                                value.as_meta(),
                                            );
                                            res
                                        }
                                    });
                                    repair_puts.push(repair);
                                }

                                future::join_all(repair_puts)
                                    .map(|_| Some(value))
                                    .right_future()
                            }
                        })
                        .boxify()
                        .right_future()
                }
            })
            .compat()
            .boxed()
    }

    fn put(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        self.inner.put(ctx, key, value)
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<'static, Result<bool, Error>> {
        self.inner.is_present(ctx, key)
    }
}
