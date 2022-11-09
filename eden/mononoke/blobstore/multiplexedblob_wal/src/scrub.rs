/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

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
use context::CoreContext;
use futures::stream::StreamExt;
use metaconfig_types::BlobstoreId;
use multiplexedblob::base::ErrorKind;
use multiplexedblob::ScrubHandler;
use multiplexedblob::ScrubOptions;
use multiplexedblob::ScrubWriteMostly;

use crate::multiplex;
use crate::WalMultiplexedBlobstore;

impl WalMultiplexedBlobstore {
    async fn scrub_get(
        &self,
        ctx: &CoreContext,
        key: &str,
        write_mostly: ScrubWriteMostly,
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
                    self.write_mostly_blobstores.clone(),
                    key,
                    OperationType::ScrubGet,
                    &scuba,
                )
                .collect::<Vec<_>>()
            },
            self.write_mostly_blobstores.iter().map(|b| *b.id()),
            write_mostly,
        )
        .await;

        multiplexedblob::base::scrub_parse_results(results, self.blobstores.iter().map(|b| *b.id()))
    }
}

#[derive(Clone, Debug)]
struct WalScrubBlobstore {
    inner: WalMultiplexedBlobstore,
    scrub_options: ScrubOptions,
    all_blobstores: Arc<HashMap<BlobstoreId, Arc<dyn BlobstorePutOps>>>,
    scrub_handler: Arc<dyn ScrubHandler>,
}

impl std::fmt::Display for WalScrubBlobstore {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "WalScrubBlobstore[{}]", self.inner)
    }
}

#[async_trait]
impl Blobstore for WalScrubBlobstore {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let write_mostly = self.scrub_options.scrub_action_on_missing_write_mostly;
        match self.inner.scrub_get(ctx, key, write_mostly).await {
            Ok(value) => Ok(value),
            Err(ErrorKind::SomeMissingItem {
                missing_main,
                missing_write_mostly,
                value: Some(value),
            }) => {
                multiplexedblob::scrub::maybe_repair(
                    ctx,
                    key,
                    value,
                    missing_main,
                    missing_write_mostly,
                    self.all_blobstores.as_ref(),
                    self.scrub_handler.as_ref(),
                    &self.scrub_options,
                    &self.inner.scuba.inner_blobstores_scuba,
                    // On WAL we never look into queue except on healer
                    || futures::future::ok(true),
                )
                .await
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
