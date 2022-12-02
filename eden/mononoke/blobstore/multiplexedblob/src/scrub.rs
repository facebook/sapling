/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cmp::max;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::num::NonZeroU64;
use std::num::NonZeroUsize;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::BlobstoreMetadata;
use blobstore::BlobstorePutOps;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use blobstore_sync_queue::BlobstoreSyncQueue;
use chrono::Duration as ChronoDuration;
use clap::ArgEnum;
use context::CoreContext;
use futures::stream::FuturesUnordered;
use futures::stream::TryStreamExt;
use futures::Future;
use metaconfig_types::BlobstoreId;
use metaconfig_types::MultiplexId;
use mononoke_types::BlobstoreBytes;
use mononoke_types::Timestamp;
use once_cell::sync::Lazy;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::info;
use slog::warn;
use strum_macros::EnumString;
use strum_macros::EnumVariantNames;
use strum_macros::IntoStaticStr;

use crate::base::inner_put;
use crate::base::ErrorKind;
use crate::base::MultiplexedBlobstoreBase;
use crate::queue::MultiplexedBlobstore;

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
    ArgEnum,
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

// How to treat write only stores during the scrub
#[derive(
    Debug,
    Clone,
    Copy,
    Eq,
    PartialEq,
    Hash,
    ArgEnum,
    EnumString,
    EnumVariantNames,
    IntoStaticStr
)]
pub enum SrubWriteOnly {
    /// don't take action on scrub missing keys from write only stores
    SkipMissing,
    /// take the normal scrub action for write only stores
    Scrub,
    /// Mode for populating empty stores.  Assumes its already missing. Don't attempt to read. Write with IfAbsent so won't overwrite if run incorrectluy.
    /// More efficient than the above if thes store is totally empty.
    PopulateIfAbsent,
    /// Mode for rescrubbing write-only stores before enabling them. Assumes that the data in them is correct,
    /// and won't read from the main stores unless the write-only stores have missing data or read failures
    /// This ensures that load on the main stores is kept to a minimum
    ScrubIfAbsent,
}

#[derive(Clone, Debug)]
pub struct ScrubOptions {
    pub scrub_action: ScrubAction,
    pub scrub_grace: Option<Duration>,
    pub scrub_action_on_missing_write_only: SrubWriteOnly,
    pub queue_peek_bound: Duration,
}

impl Default for ScrubOptions {
    fn default() -> Self {
        Self {
            scrub_action: ScrubAction::ReportOnly,
            scrub_grace: None,
            scrub_action_on_missing_write_only: SrubWriteOnly::Scrub,
            queue_peek_bound: *HEAL_MAX_BACKLOG,
        }
    }
}

pub fn default_scrub_handler() -> Arc<dyn ScrubHandler> {
    Arc::new(LoggingScrubHandler::new(false))
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
    scrub_handler: Arc<dyn ScrubHandler>,
}

impl fmt::Display for ScrubBlobstore {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ScrubBlobstore[{}]", self.inner.blobstore.as_ref())
    }
}

impl ScrubBlobstore {
    pub fn new(
        multiplex_id: MultiplexId,
        blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
        write_only_blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
        minimum_successful_writes: NonZeroUsize,
        not_present_read_quorum: NonZeroUsize,
        queue: Arc<dyn BlobstoreSyncQueue>,
        mut scuba: MononokeScubaSampleBuilder,
        multiplex_scuba: MononokeScubaSampleBuilder,
        scuba_sample_rate: NonZeroU64,
        scrub_options: ScrubOptions,
        scrub_handler: Arc<dyn ScrubHandler>,
    ) -> Self {
        scuba.add_common_server_data();
        let inner = MultiplexedBlobstore::new(
            multiplex_id,
            blobstores.clone(),
            write_only_blobstores.clone(),
            minimum_successful_writes,
            not_present_read_quorum,
            queue.clone(),
            scuba.clone(),
            multiplex_scuba,
            scuba_sample_rate,
        );
        Self {
            inner,
            scrub_options,
            scuba,
            scrub_stores: Arc::new(
                blobstores
                    .into_iter()
                    .chain(write_only_blobstores.into_iter())
                    .collect(),
            ),
            queue,
            scrub_handler,
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
    put_behaviour: PutBehaviour,
) -> Result<()> {
    let (_, res) = inner_put(
        ctx,
        scuba.clone(),
        order,
        id,
        store,
        key.to_owned(),
        value.as_bytes().clone(),
        Some(put_behaviour),
    )
    .await;
    scrub_handler.on_repair(ctx, id, key, res.is_ok(), value.as_meta());
    res.map(|_status| ())
}

pub async fn maybe_repair<F: Future<Output = Result<bool>>>(
    ctx: &CoreContext,
    key: &str,
    value: BlobstoreGetData,
    missing_main: Arc<HashSet<BlobstoreId>>,
    missing_write_only: Arc<HashSet<BlobstoreId>>,
    scrub_stores: &HashMap<BlobstoreId, Arc<dyn BlobstorePutOps>>,
    scrub_handler: &dyn ScrubHandler,
    scrub_options: &ScrubOptions,
    scuba: &MononokeScubaSampleBuilder,
    already_healed: impl FnOnce() -> F,
) -> Result<Option<BlobstoreGetData>> {
    let ctime_age = value.as_meta().ctime().map(|ctime| {
        let age_secs = max(0, Timestamp::from_timestamp_secs(ctime).since_seconds());
        Duration::from_secs(age_secs as u64)
    });

    match (ctime_age, scrub_options.scrub_grace) {
        // value written recently, within the grace period, so don't attempt repair
        (Some(ctime_age), Some(scrub_grace)) if ctime_age < scrub_grace => {
            return Ok(Some(value));
        }
        _ => {}
    }

    let mut needs_repair: HashMap<BlobstoreId, (PutBehaviour, &dyn BlobstorePutOps)> =
        HashMap::new();

    // For write only stores we can chose not to do the scrub action
    // e.g. if store is still being populated, a checking scrub wouldn't want to raise alarm on the store
    if scrub_options.scrub_action_on_missing_write_only != SrubWriteOnly::SkipMissing
        || !missing_main.is_empty()
    {
        // Only peek the queue if needed
        let already_healed = match ctime_age.as_ref() {
            // Avoid false alarms for recently written items still on the healer queue
            Some(ctime_age) if ctime_age < &scrub_options.queue_peek_bound => {
                already_healed().await?
            }
            _ => true,
        };

        // Only attempt the action if we don't know of pending writes from the queue
        if already_healed {
            for k in missing_main.iter() {
                if let Some(s) = scrub_stores.get(k) {
                    // We are repairing, overwrite is right thing to do as
                    // bad keys may be is_present, but not retrievable.
                    needs_repair.insert(*k, (PutBehaviour::Overwrite, s.as_ref()));
                }
            }
            for k in missing_write_only.iter() {
                if let Some(s) = scrub_stores.get(k) {
                    let put_behaviour = match scrub_options.scrub_action_on_missing_write_only {
                        SrubWriteOnly::SkipMissing => None,
                        SrubWriteOnly::Scrub => Some(PutBehaviour::Overwrite),
                        SrubWriteOnly::PopulateIfAbsent | SrubWriteOnly::ScrubIfAbsent => {
                            Some(PutBehaviour::IfAbsent)
                        }
                    };
                    if let Some(put_behaviour) = put_behaviour {
                        needs_repair.insert(*k, (put_behaviour, s.as_ref()));
                    }
                }
            }
        }
    }

    if scrub_options.scrub_action == ScrubAction::ReportOnly {
        for id in needs_repair.keys() {
            scrub_handler.on_repair(ctx, *id, key, false, value.as_meta());
        }
    } else {
        // inner_put to the stores that need it.
        let order = AtomicUsize::new(0);
        let repair_puts: FuturesUnordered<_> = needs_repair
            .into_iter()
            .map(|(id, (put_behaviour, store))| {
                put_and_mark_repaired(
                    ctx,
                    scuba,
                    &order,
                    id,
                    store,
                    key,
                    &value,
                    scrub_handler,
                    put_behaviour,
                )
            })
            .collect();

        repair_puts.try_for_each(|_| async { Ok(()) }).await?;
    }
    Ok(Some(value))
}

// Workaround for Blobstore returning a static lifetime future
async fn blobstore_get(
    inner_blobstore: &MultiplexedBlobstoreBase,
    ctx: &CoreContext,
    key: &str,
    queue: &dyn BlobstoreSyncQueue,
    scrub_stores: &HashMap<BlobstoreId, Arc<dyn BlobstorePutOps>>,
    scrub_options: &ScrubOptions,
    scrub_handler: &dyn ScrubHandler,
    scuba: &MononokeScubaSampleBuilder,
) -> Result<Option<BlobstoreGetData>> {
    match inner_blobstore
        .scrub_get(ctx, key, scrub_options.scrub_action_on_missing_write_only)
        .await
    {
        Ok(value) => Ok(value),
        Err(error) => match error {
            ErrorKind::SomeFailedOthersNone { .. } => {
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
            ErrorKind::SomeMissingItem {
                missing_main,
                missing_write_only,
                value,
            } => {
                maybe_repair(
                    ctx,
                    key,
                    value,
                    missing_main,
                    missing_write_only,
                    scrub_stores,
                    scrub_handler,
                    scrub_options,
                    scuba,
                    || async { Ok(queue.get(ctx, key).await?.is_empty()) },
                )
                .await
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
            self.scrub_handler.as_ref(),
            &self.scuba,
        )
        .await
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
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
