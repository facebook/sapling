// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use blobstore::Blobstore;
use context::CoreContext;
use failure_ext::Error;
use futures::future::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::{BlobstoreBytes, Timestamp};
use scuba_ext::ScubaSampleBuilder;
use slog::debug;
use std::collections::HashMap;
mod errors;
use cloned::cloned;

pub use crate::errors::ErrorKind;
use std::sync::{
    atomic::{AtomicI64, Ordering},
    Arc,
};

mod store;
pub use crate::store::SqlRedactedContentStore;

pub mod config {
    pub const GET_OPERATION: &str = "GET";
    pub const PUT_OPERATION: &str = "PUT";
    pub const MIN_REPORT_TIME_DIFFERENCE_NS: i64 = 1_000_000_000;
}

// A wrapper for any blobstore, which provides a verification layer for the redacted blobs.
// The goal is to deny access to fetch sensitive data from the repository.
#[derive(Debug, Clone)]
pub struct RedactedBlobstore<T: Blobstore + Clone> {
    blobstore: T,
    redacted: Option<HashMap<String, String>>,
    scuba_builder: ScubaSampleBuilder,
    timestamp: Arc<AtomicI64>,
}

impl<T: Blobstore + Clone> RedactedBlobstore<T> {
    pub fn new(
        blobstore: T,
        redacted: Option<HashMap<String, String>>,
        scuba_builder: ScubaSampleBuilder,
    ) -> Self {
        let timestamp = Arc::new(AtomicI64::new(Timestamp::now().timestamp_nanos()));

        Self {
            blobstore,
            redacted,
            scuba_builder,
            timestamp,
        }
    }

    pub fn err_if_redacted(&self, key: &String) -> Result<(), Error> {
        match &self.redacted {
            Some(redacted) => redacted.get(key).map_or(Ok(()), |task| {
                Err(ErrorKind::Censored(key.to_string(), task.to_string()).into())
            }),
            None => Ok(()),
        }
    }

    #[inline]
    pub fn into_inner(self) -> T {
        self.blobstore
    }

    #[inline]
    pub fn as_inner(&self) -> &T {
        &self.blobstore
    }

    pub fn to_scuba_redacted_blob_accessed(
        &self,
        ctx: &CoreContext,
        key: &String,
        operation: &str,
    ) {
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
                let mut scuba_builder = self.scuba_builder.clone();
                let session = &ctx.session();
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

    #[inline]
    pub fn into_parts(self) -> (T, Option<HashMap<String, String>>, ScubaSampleBuilder) {
        (self.blobstore, self.redacted, self.scuba_builder)
    }
}

impl<T: Blobstore + Clone> Blobstore for RedactedBlobstore<T> {
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        self.err_if_redacted(&key)
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
            .into_future()
            .and_then({
                cloned!(self.blobstore);
                move |()| blobstore.get(ctx, key)
            })
            .boxify()
    }

    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        self.err_if_redacted(&key)
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
            .into_future()
            .and_then({
                cloned!(self.blobstore);
                move |()| blobstore.put(ctx, key, value)
            })
            .boxify()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        self.blobstore.is_present(ctx, key)
    }

    fn assert_present(&self, ctx: CoreContext, key: String) -> BoxFuture<(), Error> {
        self.blobstore.assert_present(ctx, key)
    }
}

#[cfg(test)]
mod test {

    use super::*;
    use assert_matches::assert_matches;
    use context::CoreContext;
    use maplit::hashmap;
    use memblob::EagerMemblob;
    use prefixblob::PrefixBlobstore;
    use tokio::runtime::Runtime;

    #[test]
    fn test_redacted_key() {
        let mut rt = Runtime::new().unwrap();

        let unredacted_key = "foo".to_string();
        let redacted_key = "bar".to_string();
        let redacted_task = "bar task".to_string();

        let ctx = CoreContext::test_mock();

        let inner = EagerMemblob::new();
        let redacted_pairs = hashmap! {
            redacted_key.clone() => redacted_task.clone(),
        };

        let blob = RedactedBlobstore::new(
            PrefixBlobstore::new(inner, "prefix"),
            Some(redacted_pairs),
            ScubaSampleBuilder::with_discard(),
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
