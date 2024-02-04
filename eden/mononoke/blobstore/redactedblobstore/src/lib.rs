/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod errors;
mod redaction_config_blobstore;
pub mod store;

use std::ops::Deref;
use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::BlobstorePutOps;
use blobstore::BlobstoreUnlinkOps;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use context::CoreContext;
use mononoke_types::BlobstoreBytes;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::debug;

pub use crate::errors::ErrorKind;
pub use crate::redaction_config_blobstore::ArcRedactionConfigBlobstore;
pub use crate::redaction_config_blobstore::RedactionConfigBlobstore;
pub use crate::store::RedactedBlobs;
pub use crate::store::RedactedMetadata;

pub mod config {
    pub const GET_OPERATION: &str = "GET";
    pub const PUT_OPERATION: &str = "PUT";
}

#[derive(Debug, Clone)]
pub struct RedactedBlobstoreConfigInner {
    redacted: Option<Arc<RedactedBlobs>>,
    scuba_builder: MononokeScubaSampleBuilder,
}

#[derive(Debug, Clone)]
pub struct RedactedBlobstoreConfig {
    inner: Arc<RedactedBlobstoreConfigInner>,
}

impl Deref for RedactedBlobstoreConfig {
    type Target = RedactedBlobstoreConfigInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl RedactedBlobstoreConfig {
    pub fn new(
        redacted: Option<Arc<RedactedBlobs>>,
        scuba_builder: MononokeScubaSampleBuilder,
    ) -> Self {
        Self {
            inner: Arc::new(RedactedBlobstoreConfigInner {
                redacted,
                scuba_builder,
            }),
        }
    }
}

#[derive(Debug)]
pub struct RedactedBlobstoreInner<T> {
    blobstore: T,
    config: RedactedBlobstoreConfig,
}

impl<T: std::fmt::Display> std::fmt::Display for RedactedBlobstoreInner<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RedactedBlobstoreInner<{}>", &self.blobstore)
    }
}

// A wrapper for any blobstore, which provides a verification layer for the redacted blobs.
// The goal is to deny access to fetch sensitive data from the repository.
#[derive(Debug, Clone)]
pub struct RedactedBlobstore<T> {
    inner: Arc<RedactedBlobstoreInner<T>>,
}

impl<T: std::fmt::Display> std::fmt::Display for RedactedBlobstore<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RedactedBlobstore<{}>", &self.inner.blobstore)
    }
}

impl<T> Deref for RedactedBlobstore<T> {
    type Target = RedactedBlobstoreInner<T>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: Blobstore> RedactedBlobstore<T> {
    pub fn new(blobstore: T, config: RedactedBlobstoreConfig) -> Self {
        Self {
            inner: Arc::new(RedactedBlobstoreInner::new(blobstore, config)),
        }
    }

    pub fn boxed(&self) -> Arc<dyn Blobstore>
    where
        T: 'static,
    {
        self.inner.clone()
    }

    pub fn as_parts(&self) -> (T, RedactedBlobstoreConfig)
    where
        T: Clone,
    {
        (self.blobstore.clone(), self.config.clone())
    }

    pub fn as_inner_unredacted(&self) -> &T {
        &self.blobstore
    }
}

impl<T: Blobstore> RedactedBlobstoreInner<T> {
    pub fn new(blobstore: T, config: RedactedBlobstoreConfig) -> Self {
        Self { blobstore, config }
    }

    // Checks for access to this key, then yields the blobstore if access is allowed.
    pub fn access_blobstore<'s: 'a, 'a>(
        &'s self,
        ctx: &'a CoreContext,
        key: &'a str,
        operation: &'static str,
    ) -> Result<&'s T> {
        match &self.config.redacted {
            Some(redacted) => {
                redacted
                    .redacted()
                    .get(key)
                    .map_or(Ok(&self.blobstore), |metadata| {
                        debug!(
                            ctx.logger(),
                            "{} operation with redacted blobstore with key {:?}", operation, key
                        );
                        self.log_redacted_blob_access_to_scuba(ctx, key, operation, metadata);

                        if metadata.log_only {
                            Ok(&self.blobstore)
                        } else {
                            Err(
                                ErrorKind::Censored(key.to_string(), metadata.task.to_string())
                                    .into(),
                            )
                        }
                    })
            }
            None => Ok(&self.blobstore),
        }
    }

    fn log_redacted_blob_access_to_scuba(
        &self,
        ctx: &CoreContext,
        key: &str,
        operation: &str,
        metadata: &RedactedMetadata,
    ) {
        let mut scuba_builder = self.config.scuba_builder.clone();
        scuba_builder.unsampled();

        scuba_builder
            .add("operation", operation)
            .add("key", key.to_string())
            .add("reason", metadata.task.to_string())
            .add("enforced", (!metadata.log_only) as u32)
            .add("session_uuid", ctx.metadata().session_id().to_string());

        if let Some(unix_username) = ctx.metadata().unix_name() {
            scuba_builder.add("unix_username", unix_username);
        }

        scuba_builder.log();
    }
}

#[async_trait]
impl<B: Blobstore> Blobstore for RedactedBlobstoreInner<B> {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let blobstore = self.access_blobstore(ctx, key, config::GET_OPERATION)?;
        blobstore.get(ctx, key).await
    }

    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        let blobstore = self.access_blobstore(ctx, &key, config::PUT_OPERATION)?;
        blobstore.put(ctx, key, value).await
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        self.blobstore.is_present(ctx, key).await
    }

    async fn copy<'a>(
        &'a self,
        ctx: &'a CoreContext,
        old_key: &'a str,
        new_key: String,
    ) -> Result<()> {
        let blobstore = self.access_blobstore(ctx, old_key, config::GET_OPERATION)?;
        self.access_blobstore(ctx, &new_key, config::PUT_OPERATION)?;
        blobstore.copy(ctx, old_key, new_key).await
    }
}

#[async_trait]
impl<B: BlobstorePutOps> BlobstorePutOps for RedactedBlobstoreInner<B> {
    async fn put_explicit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        let blobstore = self.access_blobstore(ctx, &key, config::PUT_OPERATION)?;
        blobstore.put_explicit(ctx, key, value, put_behaviour).await
    }
    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        let blobstore = self.access_blobstore(ctx, &key, config::PUT_OPERATION)?;
        blobstore.put_with_status(ctx, key, value).await
    }
}

#[async_trait]
impl<B: BlobstoreUnlinkOps> BlobstoreUnlinkOps for RedactedBlobstoreInner<B> {
    async fn unlink<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<()> {
        let blobstore = self.access_blobstore(ctx, key, config::PUT_OPERATION)?;
        blobstore.unlink(ctx, key).await
    }
}

#[async_trait]
impl<B: Blobstore> Blobstore for RedactedBlobstore<B> {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.inner.get(ctx, key).await
    }
    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        self.inner.put(ctx, key, value).await
    }
    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        self.inner.is_present(ctx, key).await
    }
}

#[async_trait]
impl<B: BlobstorePutOps> BlobstorePutOps for RedactedBlobstore<B> {
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

#[async_trait]
impl<B: BlobstoreUnlinkOps> BlobstoreUnlinkOps for RedactedBlobstore<B> {
    async fn unlink<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<()> {
        self.inner.unlink(ctx, key).await
    }
}

pub fn has_redaction_root_cause(e: &Error) -> bool {
    match e.root_cause().downcast_ref::<ErrorKind>() {
        Some(ErrorKind::Censored(_, _)) => true,
        None => false,
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use assert_matches::assert_matches;
    use borrowed::borrowed;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use memblob::Memblob;
    use prefixblob::PrefixBlobstore;

    use super::*;

    #[fbinit::test]
    async fn test_redacted_key(fb: FacebookInit) {
        let unredacted_key = "foo";
        let redacted_key = "bar";
        let redacted_task = "bar task";

        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);

        let inner = Memblob::default();
        let redacted_pairs = RedactedBlobs::FromHashMapForTests(Arc::new(hashmap! {
            redacted_key.to_owned() => RedactedMetadata {
                task: redacted_task.to_owned(),
                log_only: false,
            },
        }));

        let blob = RedactedBlobstore::new(
            PrefixBlobstore::new(inner, "prefix"),
            RedactedBlobstoreConfig::new(
                Some(Arc::new(redacted_pairs)),
                MononokeScubaSampleBuilder::with_discard(),
            ),
        );

        //Test put with redacted key
        let res = blob
            .put(
                ctx,
                redacted_key.to_owned(),
                BlobstoreBytes::from_bytes("test bar"),
            )
            .await;

        assert_matches!(
            res.expect_err("the key should be redacted").downcast::<ErrorKind>(),
            Ok(ErrorKind::Censored(_, ref task)) if task == redacted_task
        );

        //Test key added to the blob
        let res = blob
            .put(
                ctx,
                unredacted_key.to_owned(),
                BlobstoreBytes::from_bytes("test foo"),
            )
            .await;
        assert!(res.is_ok(), "the key should be added successfully");

        // Test accessing a key which is redacted
        let res = blob.get(ctx, redacted_key).await;

        assert_matches!(
            res.expect_err("the key should be redacted").downcast::<ErrorKind>(),
            Ok(ErrorKind::Censored(_, ref task)) if *task == redacted_task
        );

        // Test accessing a key which exists and is accesible
        let res = blob.get(ctx, unredacted_key).await;
        assert!(res.is_ok(), "the key should be found and available");
    }

    #[fbinit::test]
    async fn test_log_only_redacted_key(fb: FacebookInit) -> Result<()> {
        let redacted_log_only_key = "bar";
        let redacted_task = "bar task";

        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);

        let inner = Memblob::default();
        let redacted_pairs = RedactedBlobs::FromHashMapForTests(Arc::new(hashmap! {
            redacted_log_only_key.to_owned() => RedactedMetadata {
                task: redacted_task.to_owned(),
                log_only: true,
            },
        }));

        let blob = RedactedBlobstore::new(
            PrefixBlobstore::new(inner, "prefix"),
            RedactedBlobstoreConfig::new(
                Some(Arc::new(redacted_pairs)),
                MononokeScubaSampleBuilder::with_discard(),
            ),
        );

        // Since this is a log-only mode it should succeed
        let val = BlobstoreBytes::from_bytes("test bar");
        blob.put(ctx, redacted_log_only_key.to_owned(), val.clone())
            .await?;

        let actual = blob.get(ctx, redacted_log_only_key).await?;
        assert_eq!(Some(val), actual.map(|val| val.into_bytes()));

        Ok(())
    }
}
