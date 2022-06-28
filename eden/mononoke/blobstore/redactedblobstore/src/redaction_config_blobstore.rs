/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;

use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use context::CoreContext;
use mononoke_types::BlobstoreBytes;
use prefixblob::PrefixBlobstore;

use std::sync::Arc;

/// Blobstore where redaction sets are stored
#[facet::facet]
#[derive(Debug)]
pub struct RedactionConfigBlobstore(PrefixBlobstore<Arc<dyn Blobstore>>);

impl RedactionConfigBlobstore {
    pub fn new(blobstore: Arc<dyn Blobstore>) -> Self {
        Self(PrefixBlobstore::new(blobstore, "redactionconfig"))
    }
}

impl std::fmt::Display for RedactionConfigBlobstore {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "RedactionConfigBlobstore<{}>", self.0)
    }
}

#[async_trait]
impl Blobstore for RedactionConfigBlobstore {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.0.get(ctx, key).await
    }
    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        self.0.put(ctx, key, value).await
    }
    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        self.0.is_present(ctx, key).await
    }
}
