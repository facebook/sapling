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
use futures::future::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::{BlobstoreBytes, Timestamp};
use scuba_ext::ScubaSampleBuilder;
use slog::debug;
use std::collections::HashMap;
mod errors;
pub use crate::errors::ErrorKind;
use cloned::cloned;
use std::{
    ops::Deref,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    },
};
mod store;
pub use crate::store::SqlRedactedContentStore;

pub mod config {
    pub const GET_OPERATION: &str = "GET";
    pub const PUT_OPERATION: &str = "PUT";
    pub const MIN_REPORT_TIME_DIFFERENCE_NS: i64 = 1_000_000_000;
}

#[derive(Debug, Clone)]
pub struct RedactedBlobstoreConfigInner {
    redacted: Option<HashMap<String, String>>,
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
        redacted: Option<HashMap<String, String>>,
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
    pub fn access_blobstore(&self, key: &str) -> Result<&T, Error> {
        match &self.config.redacted {
            Some(redacted) => redacted.get(key).map_or(Ok(&self.blobstore), |task| {
                Err(ErrorKind::Censored(key.to_string(), task.to_string()).into())
            }),
            None => Ok(&self.blobstore),
        }
    }

    pub fn to_scuba_redacted_blob_accessed(&self, ctx: &CoreContext, key: &str, operation: &str) {
        let curr_timestamp = Timestamp::now().timestamp_nanos();
        let last_timestamp = self.timestamp.load(Ordering::Acquire);
        if config::MIN_REPORT_TIME_DIFFERENCE_NS < curr_timestamp - last_timestamp {
            let res = &self.timestamp.compare_exchange(
                last_timestamp,
                curr_timestamp,
                Ordering::Acquire,
                Ordering::Relaxed,
            );

            if res.is_ok() {
                let mut scuba_builder = self.config.scuba_builder.clone();
                let session = &ctx.session_id();
                scuba_builder
                    .add("time", curr_timestamp)
                    .add("operation", operation)
                    .add("key", key.to_string())
                    .add("session_uuid", session.to_string());

                if let Some(unix_username) = ctx.user_unix_name().clone() {
                    scuba_builder.add("unix_username", unix_username);
                }

                scuba_builder.log();
            }
        }
    }
}

impl<T: Blobstore + Clone> Blobstore for RedactedBlobstoreInner<T> {
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreGetData>, Error> {
        self.access_blobstore(&key)
            .map_err({
                cloned!(ctx, key);
                move |err| {
                    debug!(
                        ctx.logger(),
                        "Accessing redacted blobstore with key {:?}", key
                    );
                    self.to_scuba_redacted_blob_accessed(&ctx, &key, config::GET_OPERATION);
                    err
                }
            })
            .map({ move |blobstore| blobstore.get(ctx, key) })
            .into_future()
            .flatten()
            .boxify()
    }

    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        self.access_blobstore(&key)
            .map_err({
                cloned!(ctx, key);
                move |err| {
                    debug!(
                        ctx.logger(),
                        "Updating redacted blobstore with key {:?}", key
                    );

                    self.to_scuba_redacted_blob_accessed(&ctx, &key, config::PUT_OPERATION);
                    err
                }
            })
            .map({ move |blobstore| blobstore.put(ctx, key, value) })
            .into_future()
            .flatten()
            .boxify()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        self.blobstore.is_present(ctx, key)
    }

    fn assert_present(&self, ctx: CoreContext, key: String) -> BoxFuture<(), Error> {
        self.blobstore.assert_present(ctx, key)
    }
}

impl<B> Blobstore for RedactedBlobstore<B>
where
    B: Blobstore + Clone,
{
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreGetData>, Error> {
        self.inner.get(ctx, key)
    }
    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        self.inner.put(ctx, key, value)
    }
    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        self.inner.is_present(ctx, key)
    }
    fn assert_present(&self, ctx: CoreContext, key: String) -> BoxFuture<(), Error> {
        self.inner.assert_present(ctx, key)
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
    use tokio_compat::runtime::Runtime;

    #[fbinit::test]
    fn test_redacted_key(fb: FacebookInit) {
        let mut rt = Runtime::new().unwrap();

        let unredacted_key = "foo".to_string();
        let redacted_key = "bar".to_string();
        let redacted_task = "bar task".to_string();

        let ctx = CoreContext::test_mock(fb);

        let inner = EagerMemblob::new();
        let redacted_pairs = hashmap! {
            redacted_key.clone() => redacted_task.clone(),
        };

        let blob = RedactedBlobstore::new(
            PrefixBlobstore::new(inner, "prefix"),
            RedactedBlobstoreConfig::new(Some(redacted_pairs), ScubaSampleBuilder::with_discard()),
        );

        //Test put with redacted key
        let res = rt.block_on(blob.put(
            ctx.clone(),
            redacted_key.clone(),
            BlobstoreBytes::from_bytes("test bar"),
        ));

        assert_matches!(
            res.expect_err("the key should be redacted").downcast::<ErrorKind>(),
            Ok(ErrorKind::Censored(_, ref task)) if task == &redacted_task
        );

        //Test key added to the blob
        let res = rt.block_on(blob.put(
            ctx.clone(),
            unredacted_key.clone(),
            BlobstoreBytes::from_bytes("test foo"),
        ));
        assert!(res.is_ok(), "the key should be added successfully");

        // Test accessing a key which is redacted
        let res = rt.block_on(blob.get(ctx.clone(), redacted_key.clone()));

        assert_matches!(
            res.expect_err("the key should be redacted").downcast::<ErrorKind>(),
            Ok(ErrorKind::Censored(_, ref task)) if task == &redacted_task
        );

        // Test accessing a key which exists and is accesible
        let res = rt.block_on(blob.get(ctx.clone(), unredacted_key.clone()));
        assert!(res.is_ok(), "the key should be found and available");
    }
}
