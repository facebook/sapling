/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{
    base::{inner_put, ErrorKind, MultiplexedBlobstoreBase},
    queue::MultiplexedBlobstore,
};

use anyhow::Error;
use blobstore::{Blobstore, BlobstoreGetData, BlobstoreMetadata};
use blobstore_sync_queue::BlobstoreSyncQueue;
use chrono::Duration as ChronoDuration;
use cloned::cloned;
use context::CoreContext;
use futures::{
    future::{BoxFuture, FutureExt},
    stream::{FuturesUnordered, StreamExt},
};
use metaconfig_types::{BlobstoreId, MultiplexId, ScrubAction};
use mononoke_types::{BlobstoreBytes, DateTime};
use once_cell::sync::OnceCell;
use scuba::ScubaSampleBuilder;
use slog::{info, warn};
use std::collections::HashMap;
use std::fmt;
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::{atomic::AtomicUsize, Arc};

static DEFAULT_HEAL_MAX_BACKLOG_DAYS: i64 = 7;
static HEAL_MAX_BACKLOG: OnceCell<ChronoDuration> = OnceCell::new();

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
        write_mostly_blobstores: Vec<(BlobstoreId, Arc<dyn Blobstore>)>,
        minimum_successful_writes: NonZeroUsize,
        queue: Arc<dyn BlobstoreSyncQueue>,
        scuba: ScubaSampleBuilder,
        scuba_sample_rate: NonZeroU64,
        scrub_handler: Arc<dyn ScrubHandler>,
        scrub_action: ScrubAction,
    ) -> Self {
        let inner = MultiplexedBlobstore::new(
            multiplex_id,
            blobstores.clone(),
            write_mostly_blobstores.clone(),
            minimum_successful_writes,
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
                    .chain(write_mostly_blobstores.into_iter())
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

// Would be a closure, but async closures are unstable
async fn put_and_mark_repaired(
    ctx: &CoreContext,
    scuba: &ScubaSampleBuilder,
    order: &AtomicUsize,
    id: BlobstoreId,
    store: &dyn Blobstore,
    key: &String,
    value: &BlobstoreGetData,
    scrub_handler: &dyn ScrubHandler,
) {
    let (_, res) = inner_put(
        ctx,
        scuba.clone(),
        order,
        id,
        store,
        key.clone(),
        value.as_bytes().clone(),
    )
    .await;
    scrub_handler.on_repair(&ctx, id, &key, res.is_ok(), value.as_meta());
}

// Workaround for Blobstore returning a static lifetime future
async fn blobstore_get(
    inner_blobstore: &MultiplexedBlobstoreBase,
    ctx: &CoreContext,
    key: String,
    queue: &dyn BlobstoreSyncQueue,
    scrub_stores: &HashMap<BlobstoreId, Arc<dyn Blobstore>>,
    scrub_handler: &dyn ScrubHandler,
    scrub_action: ScrubAction,
    scuba: ScubaSampleBuilder,
) -> Result<Option<BlobstoreGetData>, Error> {
    match inner_blobstore.scrub_get(ctx, &key).await {
        Ok(value) => return Ok(value),
        Err(error) => match error {
            ErrorKind::SomeFailedOthersNone(_) => {
                // MultiplexedBlobstore returns Ok(None) here if queue is empty for the key
                // and Error otherwise. Scrub does likewise.
                let entries = queue.get(ctx, &key).await?;
                if entries.is_empty() {
                    // No pending write for the key, it really is None
                    Ok(None)
                } else {
                    // Pending write, we don't know what the value is.
                    Err(error.into())
                }
            }
            ErrorKind::SomeMissingItem(missing_reads, Some(value)) => {
                // Avoid false alarms for recently written items still on the healer queue
                let mut do_peek_queue = true;
                if let Some(ctime) = value.as_meta().ctime() {
                    let now = DateTime::now().into_chrono();
                    let peek_max = now
                        - *HEAL_MAX_BACKLOG
                            .get_or_init(|| ChronoDuration::days(DEFAULT_HEAL_MAX_BACKLOG_DAYS));
                    if ctime < peek_max.timestamp() {
                        do_peek_queue = false;
                    }
                }

                let entries = if do_peek_queue {
                    queue.get(ctx, &key).await?
                } else {
                    vec![]
                };
                let mut needs_repair: HashMap<BlobstoreId, &dyn Blobstore> = HashMap::new();

                for k in missing_reads.iter() {
                    match scrub_stores.get(k) {
                        Some(s) => {
                            // Key is missing in the store so needs repair
                            if entries.is_empty() {
                                needs_repair.insert(*k, s.as_ref());
                            }
                        }
                        None => (),
                    }
                }
                if scrub_action == ScrubAction::ReportOnly {
                    for id in needs_repair.keys() {
                        scrub_handler.on_repair(&ctx, *id, &key, false, value.as_meta());
                    }
                } else {
                    // inner_put to the stores that need it.
                    let order = AtomicUsize::new(0);
                    let repair_puts: FuturesUnordered<_> = needs_repair
                        .into_iter()
                        .map(|(id, store)| {
                            put_and_mark_repaired(
                                ctx,
                                &scuba,
                                &order,
                                id,
                                store,
                                &key,
                                &value,
                                scrub_handler,
                            )
                        })
                        .collect();

                    repair_puts.for_each(|_| async {}).await;
                }
                Ok(Some(value))
            }
            _ => Err(error.into()),
        },
    }
}

impl Blobstore for ScrubBlobstore {
    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        cloned!(
            ctx,
            self.scrub_stores,
            self.scrub_handler,
            self.scuba,
            self.scrub_action,
            self.queue,
        );
        let inner_blobstore = self.inner.blobstore.clone();

        async move {
            blobstore_get(
                inner_blobstore.as_ref(),
                &ctx,
                key,
                queue.as_ref(),
                scrub_stores.as_ref(),
                scrub_handler.as_ref(),
                scrub_action,
                scuba,
            )
            .await
        }
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
