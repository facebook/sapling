/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use context::CoreContext;

use crate::bubble::Bubble;

/// EphemeralHandle is a blobstore that wraps both a bubble blobstore and a
/// backing "persistent" blobstore. First, it queries the bubble blobstore
/// and if a blob is not present, it queries the persistent one.
#[derive(Debug)]
pub struct EphemeralHandle<B: Blobstore> {
    bubble: Bubble,
    main_blobstore: B,
}

impl<B: Blobstore> EphemeralHandle<B> {
    pub(crate) fn new(bubble: Bubble, main_blobstore: B) -> Self {
        Self {
            bubble,
            main_blobstore,
        }
    }
}

impl<B: Blobstore> fmt::Display for EphemeralHandle<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "EphemeralHandle<{}, {}>",
            self.bubble, self.main_blobstore
        )
    }
}

#[async_trait]
impl<B: Blobstore> Blobstore for EphemeralHandle<B> {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        Ok(match self.bubble.get(ctx, key).await? {
            Some(content) => Some(content),
            None => self.main_blobstore.get(ctx, key).await?,
        })
    }

    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        self.bubble.put(ctx, key, value).await
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        Ok(match self.bubble.is_present(ctx, key).await? {
            BlobstoreIsPresent::Absent | BlobstoreIsPresent::ProbablyNotPresent(_) => {
                self.main_blobstore.is_present(ctx, key).await?
            }
            BlobstoreIsPresent::Present => BlobstoreIsPresent::Present,
        })
    }
}
