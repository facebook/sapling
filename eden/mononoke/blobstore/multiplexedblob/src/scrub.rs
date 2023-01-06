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
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreMetadata;
use blobstore::BlobstorePutOps;
use blobstore::PutBehaviour;
use chrono::Duration as ChronoDuration;
use clap::ArgEnum;
use context::CoreContext;
use futures::stream::FuturesUnordered;
use futures::stream::TryStreamExt;
use futures::Future;
use metaconfig_types::BlobstoreId;
use mononoke_types::Timestamp;
use once_cell::sync::Lazy;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::info;
use slog::warn;
use strum_macros::EnumString;
use strum_macros::EnumVariantNames;
use strum_macros::IntoStaticStr;

use crate::base::inner_put;

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
