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

use anyhow::Result;
use async_trait::async_trait;
use blobstore::{
    Blobstore, BlobstoreGetData, BlobstoreMetadata, BlobstorePutOps, OverwriteStatus, PutBehaviour,
};
use blobstore_sync_queue::BlobstoreSyncQueue;
use chrono::Duration as ChronoDuration;
use context::CoreContext;
use futures::stream::{FuturesUnordered, TryStreamExt};
use metaconfig_types::{BlobstoreId, MultiplexId};
use mononoke_types::{BlobstoreBytes, Timestamp};
use once_cell::sync::Lazy;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::{info, warn};
use std::collections::HashMap;
use std::fmt;
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::{atomic::AtomicUsize, Arc};
use std::time::Duration;
use strum_macros::{EnumString, EnumVariantNames, IntoStaticStr};

static HEAL_MAX_BACKLOG: Lazy<Duration> =
    Lazy::new(|| Duration::from_secs(ChronoDuration::days(7).num_seconds() as u64));

/// What to do when the ScrubBlobstore finds a problem
#[derive(
    Debug,
    Clone,
    Copy,
    Eq,
    PartialEq,
    Hash,
    EnumString,
    EnumVariantNames,
    IntoStaticStr
)]
pub enum ScrubAction {
    /// Log items needing repair
    ReportOnly,
    /// Do repairs
    Repair,
}

#[derive(Clone, Debug)]
pub struct ScrubOptions {
    pub scrub_action: ScrubAction,
    pub scrub_handler: Arc<dyn ScrubHandler>,
    pub scrub_grace: Option<Duration>,
}

impl Default for ScrubOptions {
    fn default() -> Self {
        Self {
            scrub_action: ScrubAction::ReportOnly,
            scrub_handler: Arc::new(LoggingScrubHandler::new(false)) as Arc<dyn ScrubHandler>,
            scrub_grace: None,
        }
    }
}

pub trait ScrubHandler: Send + Sync + fmt::Debug {
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

#[derive(Debug)]
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
    scrub_options: ScrubOptions,
    scuba: MononokeScubaSampleBuilder,
    scrub_stores: Arc<HashMap<BlobstoreId, Arc<dyn BlobstorePutOps>>>,
    queue: Arc<dyn BlobstoreSyncQueue>,
}

impl ScrubBlobstore {
    pub fn new(
        multiplex_id: MultiplexId,
        blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
        write_mostly_blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
        minimum_successful_writes: NonZeroUsize,
        queue: Arc<dyn BlobstoreSyncQueue>,
        scuba: MononokeScubaSampleBuilder,
        scuba_sample_rate: NonZeroU64,
        scrub_options: ScrubOptions,
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
            scrub_options,
            scuba,
            scrub_stores: Arc::new(
                blobstores
                    .into_iter()
                    .chain(write_mostly_blobstores.into_iter())
                    .collect::<HashMap<BlobstoreId, Arc<dyn BlobstorePutOps>>>(),
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
    scuba: &MononokeScubaSampleBuilder,
    order: &AtomicUsize,
    id: BlobstoreId,
    store: &dyn BlobstorePutOps,
    key: &str,
    value: &BlobstoreGetData,
    scrub_handler: &dyn ScrubHandler,
) -> Result<()> {
    let (_, res) = inner_put(
        ctx,
        scuba.clone(),
        order,
        id,
        store,
        key.to_owned(),
        value.as_bytes().clone(),
        // We are repairing, overwrite is right thing to do as
        // bad keys may be is_present, but not retrievable.
        Some(PutBehaviour::Overwrite),
    )
    .await;
    scrub_handler.on_repair(&ctx, id, key, res.is_ok(), value.as_meta());
    res.map(|_status| ())
}

// Workaround for Blobstore returning a static lifetime future
async fn blobstore_get(
    inner_blobstore: &MultiplexedBlobstoreBase,
    ctx: &CoreContext,
    key: &str,
    queue: &dyn BlobstoreSyncQueue,
    scrub_stores: &HashMap<BlobstoreId, Arc<dyn BlobstorePutOps>>,
    scrub_options: &ScrubOptions,
    scuba: &MononokeScubaSampleBuilder,
) -> Result<Option<BlobstoreGetData>> {
    match inner_blobstore.scrub_get(ctx, key).await {
        Ok(value) => return Ok(value),
        Err(error) => match error {
            ErrorKind::SomeFailedOthersNone(_) => {
                // MultiplexedBlobstore returns Ok(None) here if queue is empty for the key
                // and Error otherwise. Scrub does likewise.
                let entries = queue.get(ctx, key).await?;
                if entries.is_empty() {
                    // No pending write for the key, it really is None
                    Ok(None)
                } else {
                    // Pending write, we don't know what the value is.
                    Err(error.into())
                }
            }
            ErrorKind::SomeMissingItem(missing_reads, Some(value)) => {
                let ctime_age = value.as_meta().ctime().and_then(|ctime| {
                    let age_secs = Timestamp::from_timestamp_secs(ctime).since_seconds();
                    if age_secs > 0 {
                        Some(Duration::from_secs(age_secs as u64))
                    } else {
                        None
                    }
                });

                match (ctime_age, scrub_options.scrub_grace) {
                    // value written recently, within the grace period, so don't attempt repair
                    (Some(ctime_age), Some(scrub_grace)) if ctime_age < scrub_grace => {
                        return Ok(Some(value));
                    }
                    _ => {}
                }

                let entries = match ctime_age.as_ref() {
                    // Avoid false alarms for recently written items still on the healer queue
                    Some(ctime_age) if ctime_age < &*HEAL_MAX_BACKLOG => {
                        queue.get(ctx, key).await?
                    }
                    _ => vec![],
                };

                let mut needs_repair: HashMap<BlobstoreId, &dyn BlobstorePutOps> = HashMap::new();

                for k in missing_reads.iter() {
                    match scrub_stores.get(k) {
                        Some(s) => {
                            // Key is missing in the store so needs repair
                            if entries.is_empty() {
                                needs_repair.insert(*k, s.as_ref());
                            }
                        }
                        None => {}
                    }
                }

                if scrub_options.scrub_action == ScrubAction::ReportOnly {
                    for id in needs_repair.keys() {
                        scrub_options.scrub_handler.on_repair(
                            &ctx,
                            *id,
                            key,
                            false,
                            value.as_meta(),
                        );
                    }
                } else {
                    // inner_put to the stores that need it.
                    let order = AtomicUsize::new(0);
                    let repair_puts: FuturesUnordered<_> = needs_repair
                        .into_iter()
                        .map(|(id, store)| {
                            put_and_mark_repaired(
                                ctx,
                                scuba,
                                &order,
                                id,
                                store,
                                key,
                                &value,
                                scrub_options.scrub_handler.as_ref(),
                            )
                        })
                        .collect();

                    repair_puts.try_for_each(|_| async { Ok(()) }).await?;
                }
                Ok(Some(value))
            }
            _ => Err(error.into()),
        },
    }
}

#[async_trait]
impl Blobstore for ScrubBlobstore {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        blobstore_get(
            self.inner.blobstore.as_ref(),
            ctx,
            key,
            self.queue.as_ref(),
            self.scrub_stores.as_ref(),
            &self.scrub_options,
            &self.scuba,
        )
        .await
    }

    async fn is_present<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<bool> {
        self.inner.is_present(ctx, key).await
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
impl BlobstorePutOps for ScrubBlobstore {
    async fn put_explicit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        self.inner
            .put_explicit(ctx, key, value, put_behaviour)
            .await
    }

    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        self.inner.put_with_status(ctx, key, value).await
    }
}
