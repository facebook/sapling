/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use blobstore::BlobCopier;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::BlobstorePutOps;
use blobstore::BlobstoreUnlinkOps;
use blobstore::GenericBlobstoreCopier;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use context::CoreContext;
use mononoke_types::BlobstoreBytes;
use mononoke_types::RepositoryId;
use prefixblob::PrefixBlobstore;
use redactedblobstore::RedactedBlobs;
use redactedblobstore::RedactedBlobstore;
use redactedblobstore::RedactedBlobstoreConfig;
use scuba_ext::MononokeScubaSampleBuilder;

/// RedactedBlobstore should be part of every blobstore since it is a layer
/// which adds security by preventing users to access sensitive content.
///
/// Making PrefixBlobstore part of every blobstore does two things:
/// 1. It ensures that the prefix applies first, which is important for shared caches like
///    memcache.
/// 2. It ensures that all possible blobrepos use a prefix.
type RepoBlobstoreStack<T> = RedactedBlobstore<PrefixBlobstore<T>>;

// NOTE: We parametize AbstractRepoBlobstore over T instead of explicitly using Arc<dyn Blobstore>
// so that even if we were to add a blobstore to the RepoBlobstoreStack that actually is a Arc<dyn
// Blobstore>, then we cannot accidentally forget to unwrap it below (since we wouldn't get a T
// back).
#[derive(Clone, Debug)]
pub struct AbstractRepoBlobstore<T>(RepoBlobstoreStack<T>);

impl<T: Blobstore + Clone> AbstractRepoBlobstore<T> {
    pub fn as_parts(&self) -> (T, RedactedBlobstoreConfig, String) {
        let (blobstore, redacted_blobstore_config) = self.0.as_parts();
        let prefix = blobstore.prefix();

        (blobstore.into_inner(), redacted_blobstore_config, prefix)
    }
}

#[facet::facet]
#[derive(Clone, Debug)]
pub struct RepoBlobstore(AbstractRepoBlobstore<Arc<dyn Blobstore>>);

impl RepoBlobstore {
    pub fn boxed(&self) -> Arc<dyn Blobstore> {
        self.0.0.boxed()
    }

    pub fn new(
        blobstore: Arc<dyn Blobstore>,
        redacted_blobs: Option<Arc<RedactedBlobs>>,
        repoid: RepositoryId,
        scuba_builder: MononokeScubaSampleBuilder,
    ) -> Self {
        let redacted_blobstore_config = RedactedBlobstoreConfig::new(redacted_blobs, scuba_builder);
        Self::build(blobstore, repoid.prefix(), redacted_blobstore_config)
    }

    pub fn new_with_wrapped_inner_blobstore<F>(blobstore: RepoBlobstore, wrapper: F) -> Self
    where
        F: FnOnce(Arc<dyn Blobstore>) -> Arc<dyn Blobstore>,
    {
        let (blobstore, redacted_blobstore_config, prefix) = blobstore.0.as_parts();
        let new_inner_blobstore = wrapper(blobstore);
        Self::build(new_inner_blobstore, prefix, redacted_blobstore_config)
    }

    #[allow(clippy::let_and_return)]
    fn build(
        blobstore: Arc<dyn Blobstore>,
        prefix: String,
        redacted_blobstore_config: RedactedBlobstoreConfig,
    ) -> Self {
        let blobstore = PrefixBlobstore::new(blobstore, prefix);
        let blobstore = RedactedBlobstore::new(blobstore, redacted_blobstore_config);
        let blobstore = RepoBlobstore(AbstractRepoBlobstore(blobstore));

        blobstore
    }

    pub fn copier_to<'a>(&'a self, other: &'a RepoBlobstore) -> RepoBlobstoreCopier<'a> {
        RepoBlobstoreCopier::new(self, other)
    }
}

impl std::fmt::Display for RepoBlobstore {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "RepoBlobstore<{}>", self.0.0)
    }
}

#[async_trait]
impl Blobstore for RepoBlobstore {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.0.0.get(ctx, key).await
    }
    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        self.0.0.put(ctx, key, value).await
    }
    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        self.0.0.is_present(ctx, key).await
    }
    async fn copy<'a>(
        &'a self,
        ctx: &'a CoreContext,
        old_key: &'a str,
        new_key: String,
    ) -> Result<()> {
        self.0.0.copy(ctx, old_key, new_key).await
    }
}

#[facet::facet]
#[derive(Clone, Debug)]
pub struct RepoBlobstoreUnlinkOps(AbstractRepoBlobstore<Arc<dyn BlobstoreUnlinkOps>>);

impl RepoBlobstoreUnlinkOps {
    pub fn new(
        blobstore: Arc<dyn BlobstoreUnlinkOps>,
        redacted_blobs: Option<Arc<RedactedBlobs>>,
        repoid: RepositoryId,
        scuba_builder: MononokeScubaSampleBuilder,
    ) -> Self {
        let redacted_blobstore_config = RedactedBlobstoreConfig::new(redacted_blobs, scuba_builder);
        Self::build(blobstore, repoid.prefix(), redacted_blobstore_config)
    }

    #[allow(clippy::let_and_return)]
    fn build(
        blobstore: Arc<dyn BlobstoreUnlinkOps>,
        prefix: String,
        redacted_blobstore_config: RedactedBlobstoreConfig,
    ) -> Self {
        let blobstore = PrefixBlobstore::new(blobstore, prefix);
        let blobstore = RedactedBlobstore::new(blobstore, redacted_blobstore_config);
        let blobstore = RepoBlobstoreUnlinkOps(AbstractRepoBlobstore(blobstore));

        blobstore
    }
}

impl std::fmt::Display for RepoBlobstoreUnlinkOps {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "RepoBlobstoreUnlinkOps<{}>", self.0.0)
    }
}

#[async_trait]
impl Blobstore for RepoBlobstoreUnlinkOps {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.0.0.get(ctx, key).await
    }
    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        self.0.0.put(ctx, key, value).await
    }
    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        self.0.0.is_present(ctx, key).await
    }
    async fn copy<'a>(
        &'a self,
        ctx: &'a CoreContext,
        old_key: &'a str,
        new_key: String,
    ) -> Result<()> {
        self.0.0.copy(ctx, old_key, new_key).await
    }
}

#[async_trait]
impl BlobstorePutOps for RepoBlobstoreUnlinkOps {
    async fn put_explicit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        self.0.0.put_explicit(ctx, key, value, put_behaviour).await
    }
    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        self.0.0.put_with_status(ctx, key, value).await
    }
}

#[async_trait]
impl BlobstoreUnlinkOps for RepoBlobstoreUnlinkOps {
    async fn unlink<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<()> {
        self.0.0.unlink(ctx, key).await
    }
}

pub enum RepoBlobstoreCopier<'a> {
    Unoptimized(GenericBlobstoreCopier<'a, RepoBlobstore, RepoBlobstore>),
    /// We checked both repo blobstores have the same inner storage, but differ
    /// in prefixes
    Optimized {
        source: &'a PrefixBlobstore<Arc<dyn Blobstore>>,
        target: &'a PrefixBlobstore<Arc<dyn Blobstore>>,
    },
}

impl<'a> RepoBlobstoreCopier<'a> {
    fn new(source: &'a RepoBlobstore, target: &'a RepoBlobstore) -> Self {
        let inner_source = source.0.0.as_inner_unredacted();
        let inner_target = target.0.0.as_inner_unredacted();
        #[allow(ambiguous_wide_pointer_comparisons)]
        if Arc::ptr_eq(inner_source.as_inner(), inner_target.as_inner()) {
            Self::Optimized {
                source: inner_source,
                target: inner_target,
            }
        } else {
            Self::Unoptimized(GenericBlobstoreCopier { source, target })
        }
    }

    pub fn is_optimized(&self) -> bool {
        matches!(self, RepoBlobstoreCopier::Optimized { .. })
    }
}

#[async_trait]
impl<'a> BlobCopier for RepoBlobstoreCopier<'a> {
    async fn copy(&self, ctx: &CoreContext, key: String) -> Result<()> {
        match self {
            Self::Unoptimized(generic) => generic.copy(ctx, key).await,
            Self::Optimized { source, target } => {
                // same as target.as_inner()
                let inner: &Arc<dyn Blobstore> = source.as_inner();
                let source_key = source.prepend(&key);
                let target_key = target.prepend(&key);
                inner.copy(ctx, &source_key, target_key).await
            }
        }
    }
}
