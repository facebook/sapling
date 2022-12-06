/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::BlobstorePutOps;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use blobstore_stats::OperationType;
use blobstore_sync_queue::BlobstoreWal;
use context::CoreContext;
use futures::stream::StreamExt;
use metaconfig_types::BlobstoreId;
use metaconfig_types::MultiplexId;
use multiplexedblob::base::ErrorKind;
use multiplexedblob::ScrubHandler;
use multiplexedblob::ScrubOptions;
use multiplexedblob::SrubWriteOnly;

use crate::multiplex;
use crate::MultiplexTimeout;
use crate::Scuba;
use crate::WalMultiplexedBlobstore;

impl WalMultiplexedBlobstore {
    async fn scrub_get(
        &self,
        ctx: &CoreContext,
        key: &str,
        write_only: SrubWriteOnly,
    ) -> Result<Option<BlobstoreGetData>, ErrorKind> {
        let mut scuba = self.scuba.clone();
        scuba.sampled();

        let results = multiplexedblob::base::scrub_get_results(
            || {
                multiplex::inner_multi_get(
                    ctx,
                    self.blobstores.clone(),
                    key,
                    OperationType::ScrubGet,
                    &scuba,
                )
                .collect::<Vec<_>>()
            },
            || {
                multiplex::inner_multi_get(
                    ctx,
                    self.write_only_blobstores.clone(),
                    key,
                    OperationType::ScrubGet,
                    &scuba,
                )
                .collect::<Vec<_>>()
            },
            self.write_only_blobstores.iter().map(|b| *b.id()),
            write_only,
        )
        .await;

        multiplexedblob::base::scrub_parse_results(results, self.blobstores.iter().map(|b| *b.id()))
    }
}

#[derive(Clone, Debug)]
pub struct WalScrubBlobstore {
    inner: WalMultiplexedBlobstore,
    all_blobstores: Arc<HashMap<BlobstoreId, Arc<dyn BlobstorePutOps>>>,
    scrub_options: ScrubOptions,
    scrub_handler: Arc<dyn ScrubHandler>,
}

impl std::fmt::Display for WalScrubBlobstore {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "WalScrubBlobstore[{}]", self.inner)
    }
}

impl WalScrubBlobstore {
    pub fn new(
        multiplex_id: MultiplexId,
        wal_queue: Arc<dyn BlobstoreWal>,
        blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
        write_only_blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
        write_quorum: usize,
        timeout: Option<MultiplexTimeout>,
        scuba: Scuba,
        scrub_options: ScrubOptions,
        scrub_handler: Arc<dyn ScrubHandler>,
    ) -> Result<Self> {
        let all_blobstores = Arc::new(
            blobstores
                .iter()
                .cloned()
                .chain(write_only_blobstores.iter().cloned())
                .collect(),
        );
        let inner = WalMultiplexedBlobstore::new(
            multiplex_id,
            wal_queue,
            blobstores,
            write_only_blobstores,
            write_quorum,
            timeout,
            scuba,
        )?;
        Ok(Self {
            inner,
            all_blobstores,
            scrub_options,
            scrub_handler,
        })
    }
}

#[async_trait]
impl Blobstore for WalScrubBlobstore {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let write_only = self.scrub_options.scrub_action_on_missing_write_only;
        match self.inner.scrub_get(ctx, key, write_only).await {
            Ok(value) => Ok(value),
            Err(ErrorKind::SomeFailedOthersNone {
                main_errors,
                write_only_errors,
            }) => {
                if self.inner.blobstores.len() - main_errors.len() < self.inner.quorum.read.get() {
                    Err(ErrorKind::SomeFailedOthersNone {
                        main_errors,
                        write_only_errors,
                    })
                    .context("Can't tell if blob exists or not due to failing blobstores")
                } else {
                    // We have a quorum of blobstores saying the key doesn't exist. That's enough.
                    Ok(None)
                }
            }
            Err(ErrorKind::SomeMissingItem {
                missing_main,
                missing_write_only,
                value,
            }) => {
                multiplexedblob::scrub::maybe_repair(
                    ctx,
                    key,
                    value,
                    missing_main,
                    missing_write_only,
                    self.all_blobstores.as_ref(),
                    self.scrub_handler.as_ref(),
                    &self.scrub_options,
                    &self.inner.scuba.inner_blobstores_scuba,
                    // On WAL we never look into queue except on healer
                    || futures::future::ok(true),
                )
                .await
                .with_context(|| anyhow!("While repairing blobstore key {}", key))
            }
            Err(err) => Err(err.into()),
        }
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
        self.inner.put(ctx, key, value).await
    }
}

#[async_trait]
impl BlobstorePutOps for WalScrubBlobstore {
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
