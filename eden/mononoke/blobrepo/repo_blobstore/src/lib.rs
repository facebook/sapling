/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobstore::{Blobstore, BlobstoreGetData};
use context::CoreContext;
use context_concurrency_blobstore::ContextConcurrencyBlobstore;
use futures::future::BoxFuture;
use mononoke_types::{BlobstoreBytes, RepositoryId};
use prefixblob::PrefixBlobstore;
use redactedblobstore::{RedactedBlobstore, RedactedBlobstoreConfig};
use scuba_ext::ScubaSampleBuilder;
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

/// RedactedBlobstore should be part of every blobstore since it is a layer
/// which adds security by preventing users to access sensitive content.

/// Making PrefixBlobstore part of every blobstore does two things:
/// 1. It ensures that the prefix applies first, which is important for shared caches like
///    memcache.
/// 2. It ensures that all possible blobrepos use a prefix.
type RepoBlobstoreStack<T> = RedactedBlobstore<PrefixBlobstore<ContextConcurrencyBlobstore<T>>>;

// NOTE: We parametize AbstractRepoBlobstore over T instead of explicitly using Arc<dyn Blobstore>
// so that even if we were to add a blobstore to the RepoBlobstoreStack that actually is a Arc<dyn
// Blobstore>, then we cannot accidentally forget to unwrap it below (since we wouldn't get a T
// back).
#[derive(Clone, Debug)]
pub struct AbstractRepoBlobstore<T>(RepoBlobstoreStack<T>);

impl<T: Blobstore + Clone> AbstractRepoBlobstore<T> {
    pub fn as_parts(&self) -> (T, RedactedBlobstoreConfig) {
        let (blobstore, redacted_blobstore_config) = self.0.as_parts();

        (
            blobstore.into_inner().into_inner(),
            redacted_blobstore_config,
        )
    }
}

impl<T> Deref for AbstractRepoBlobstore<T> {
    type Target = RepoBlobstoreStack<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<B> Blobstore for AbstractRepoBlobstore<B>
where
    B: Blobstore + Clone,
{
    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        self.0.get(ctx, key)
    }
    fn put(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        self.0.put(ctx, key, value)
    }
    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<'static, Result<bool, Error>> {
        self.0.is_present(ctx, key)
    }
    fn assert_present(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<(), Error>> {
        self.0.assert_present(ctx, key)
    }
}

pub type RepoBlobstore = AbstractRepoBlobstore<Arc<dyn Blobstore>>;

pub struct RepoBlobstoreArgs {
    blobstore: RepoBlobstore,
    repoid: RepositoryId,
}

impl RepoBlobstoreArgs {
    pub fn new<T: Blobstore + Clone>(
        blobstore: T,
        redacted_blobs: Option<HashMap<String, String>>,
        repoid: RepositoryId,
        scuba_builder: ScubaSampleBuilder,
    ) -> Self {
        let redacted_blobstore_config = RedactedBlobstoreConfig::new(redacted_blobs, scuba_builder);
        Self::build(blobstore, repoid, redacted_blobstore_config)
    }

    pub fn new_with_wrapped_inner_blobstore<T, F>(
        blobstore: RepoBlobstore,
        repoid: RepositoryId,
        wrapper: F,
    ) -> Self
    where
        T: Blobstore + Clone,
        F: FnOnce(Arc<dyn Blobstore>) -> T,
    {
        let (blobstore, redacted_blobstore_config) = blobstore.as_parts();
        let new_inner_blobstore = wrapper(blobstore);
        Self::build(new_inner_blobstore, repoid, redacted_blobstore_config)
    }

    pub fn into_blobrepo_parts(self) -> (RepoBlobstore, RepositoryId) {
        let Self { blobstore, repoid } = self;
        (blobstore, repoid)
    }

    fn build<T: Blobstore + Clone>(
        blobstore: T,
        repoid: RepositoryId,
        redacted_blobstore_config: RedactedBlobstoreConfig,
    ) -> Self {
        let blobstore: Arc<dyn Blobstore> = Arc::new(blobstore);
        let blobstore = ContextConcurrencyBlobstore::new(blobstore);
        let blobstore = PrefixBlobstore::new(blobstore, repoid.prefix());
        let blobstore = RedactedBlobstore::new(blobstore, redacted_blobstore_config);
        let blobstore = AbstractRepoBlobstore(blobstore);

        Self { blobstore, repoid }
    }
}
