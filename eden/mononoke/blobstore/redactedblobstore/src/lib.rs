/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Error;
use blobstore::{Blobstore, BlobstoreGetData};
use context::CoreContext;
use futures::future::{BoxFuture, FutureExt};
use mononoke_types::{BlobstoreBytes, Timestamp};
use scuba_ext::ScubaSampleBuilder;
use slog::debug;
use std::collections::HashMap;
mod errors;
pub use crate::errors::ErrorKind;
use std::{
    ops::Deref,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    },
};
use tunables::tunables;
mod store;
pub use crate::store::{RedactedMetadata, SqlRedactedContentStore};

pub mod config {
    pub const GET_OPERATION: &str = "GET";
    pub const PUT_OPERATION: &str = "PUT";
}

#[derive(Debug, Clone)]
pub struct RedactedBlobstoreConfigInner {
    redacted: Option<HashMap<String, RedactedMetadata>>,
    scuba_builder: ScubaSampleBuilder,
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
        redacted: Option<HashMap<String, RedactedMetadata>>,
        scuba_builder: ScubaSampleBuilder,
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
    timestamp: Arc<AtomicI64>,
}

// A wrapper for any blobstore, which provides a verification layer for the redacted blobs.
// The goal is to deny access to fetch sensitive data from the repository.
#[derive(Debug, Clone)]
pub struct RedactedBlobstore<T> {
    inner: Arc<RedactedBlobstoreInner<T>>,
}

impl<T> Deref for RedactedBlobstore<T> {
    type Target = RedactedBlobstoreInner<T>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: Blobstore + Clone> RedactedBlobstore<T> {
    pub fn new(blobstore: T, config: RedactedBlobstoreConfig) -> Self {
        Self {
            inner: Arc::new(RedactedBlobstoreInner::new(blobstore, config)),
        }
    }

    pub fn boxed(&self) -> Arc<dyn Blobstore> {
        self.inner.clone()
    }

    pub fn as_parts(&self) -> (T, RedactedBlobstoreConfig) {
        (self.blobstore.clone(), self.config.clone())
    }
}

impl<T: Blobstore + Clone> RedactedBlobstoreInner<T> {
    pub fn new(blobstore: T, config: RedactedBlobstoreConfig) -> Self {
        let timestamp = Arc::new(AtomicI64::new(Timestamp::now().timestamp_nanos()));
        Self {
            blobstore,
            config,
            timestamp,
        }
    }

    // Checks for access to this key, then yields the blobstore if access is allowed.
    pub fn access_blobstore(
        &self,
        ctx: &CoreContext,
        key: &str,
        operation: &'static str,
    ) -> Result<&T, Error> {
        match &self.config.redacted {
            Some(redacted) => redacted.get(key).map_or(Ok(&self.blobstore), |metadata| {
                debug!(
                    ctx.logger(),
                    "{} operation with redacted blobstore with key {:?}", operation, key
                );
                self.to_scuba_redacted_blob_accessed(&ctx, &key, operation);

                if metadata.log_only {
                    Ok(&self.blobstore)
                } else {
                    Err(ErrorKind::Censored(key.to_string(), metadata.task.to_string()).into())
                }
            }),
            None => Ok(&self.blobstore),
        }
    }

    pub fn to_scuba_redacted_blob_accessed(&self, ctx: &CoreContext, key: &str, operation: &str) {
        let curr_timestamp = Timestamp::now().timestamp_nanos();
        let last_timestamp = self.timestamp.load(Ordering::Acquire);

        let sampling_rate =
            core::num::NonZeroU64::new(tunables().get_redacted_logging_sampling_rate() as u64);

        let res = &self.timestamp.compare_exchange(
            last_timestamp,
            curr_timestamp,
            Ordering::Acquire,
            Ordering::Relaxed,
        );

        if res.is_ok() {
            let mut scuba_builder = self.config.scuba_builder.clone();

            if let Some(sampling_rate) = sampling_rate {
                scuba_builder.sampled(sampling_rate);
            } else {
                scuba_builder.unsampled();
            }

            let session = &ctx.metadata().session_id();
            scuba_builder
                .add("time", curr_timestamp)
                .add("operation", operation)
                .add("key", key.to_string())
                .add("session_uuid", session.to_string());

            if let Some(unix_username) = ctx.metadata().unix_name() {
                scuba_builder.add("unix_username", unix_username);
            }

            scuba_builder.log();
        }
    }
}

impl<T: Blobstore + Clone> Blobstore for RedactedBlobstoreInner<T> {
    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        let get = self
            .access_blobstore(&ctx, &key, config::GET_OPERATION)
            .map(move |blobstore| blobstore.get(ctx, key));
        async move { get?.await }.boxed()
    }

    fn put(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        let put = self
            .access_blobstore(&ctx, &key, config::PUT_OPERATION)
            .map(move |blobstore| blobstore.put(ctx, key, value));
        async move { put?.await }.boxed()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<'static, Result<bool, Error>> {
        self.blobstore.is_present(ctx, key)
    }
}

impl<B> Blobstore for RedactedBlobstore<B>
where
    B: Blobstore + Clone,
{
    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        self.inner.get(ctx, key)
    }
    fn put(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        self.inner.put(ctx, key, value)
    }
    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<'static, Result<bool, Error>> {
        self.inner.is_present(ctx, key)
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
    use super::*;
    use assert_matches::assert_matches;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use memblob::EagerMemblob;
    use prefixblob::PrefixBlobstore;

    #[fbinit::compat_test]
    async fn test_redacted_key(fb: FacebookInit) {
        let unredacted_key = "foo".to_string();
        let redacted_key = "bar".to_string();
        let redacted_task = "bar task".to_string();

        let ctx = CoreContext::test_mock(fb);

        let inner = EagerMemblob::default();
        let redacted_pairs = hashmap! {
            redacted_key.clone() => RedactedMetadata {
                task: redacted_task.clone(),
                log_only: false,
            },
        };

        let blob = RedactedBlobstore::new(
            PrefixBlobstore::new(inner, "prefix"),
            RedactedBlobstoreConfig::new(Some(redacted_pairs), ScubaSampleBuilder::with_discard()),
        );

        //Test put with redacted key
        let res = blob
            .put(
                ctx.clone(),
                redacted_key.clone(),
                BlobstoreBytes::from_bytes("test bar"),
            )
            .await;

        assert_matches!(
            res.expect_err("the key should be redacted").downcast::<ErrorKind>(),
            Ok(ErrorKind::Censored(_, ref task)) if task == &redacted_task
        );

        //Test key added to the blob
        let res = blob
            .put(
                ctx.clone(),
                unredacted_key.clone(),
                BlobstoreBytes::from_bytes("test foo"),
            )
            .await;
        assert!(res.is_ok(), "the key should be added successfully");

        // Test accessing a key which is redacted
        let res = blob.get(ctx.clone(), redacted_key.clone()).await;

        assert_matches!(
            res.expect_err("the key should be redacted").downcast::<ErrorKind>(),
            Ok(ErrorKind::Censored(_, ref task)) if task == &redacted_task
        );

        // Test accessing a key which exists and is accesible
        let res = blob.get(ctx.clone(), unredacted_key.clone()).await;
        assert!(res.is_ok(), "the key should be found and available");
    }

    #[fbinit::compat_test]
    async fn test_log_only_redacted_key(fb: FacebookInit) -> Result<(), Error> {
        let redacted_log_only_key = "bar".to_string();
        let redacted_task = "bar task".to_string();

        let ctx = CoreContext::test_mock(fb);

        let inner = EagerMemblob::default();
        let redacted_pairs = hashmap! {
            redacted_log_only_key.clone() => RedactedMetadata {
                task: redacted_task.clone(),
                log_only: true,
            },
        };

        let blob = RedactedBlobstore::new(
            PrefixBlobstore::new(inner, "prefix"),
            RedactedBlobstoreConfig::new(Some(redacted_pairs), ScubaSampleBuilder::with_discard()),
        );

        // Since this is a log-only mode it should succeed
        let val = BlobstoreBytes::from_bytes("test bar");
        blob.put(ctx.clone(), redacted_log_only_key.clone(), val.clone())
            .await?;

        let actual = blob.get(ctx.clone(), redacted_log_only_key.clone()).await?;
        assert_eq!(Some(val), actual.map(|val| val.into_bytes()));

        Ok(())
    }
}
