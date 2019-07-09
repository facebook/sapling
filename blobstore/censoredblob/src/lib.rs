// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use blobstore::{Blobstore, BlobstoreBytes};
use context::CoreContext;
use failure_ext::Error;
use futures::future::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::Timestamp;
use scuba_ext::ScubaSampleBuilder;
use slog::debug;
use std::collections::HashMap;
mod errors;
use cloned::cloned;
use scuba_ext::ScubaSampleBuilderExt;

use crate::errors::ErrorKind;
use std::sync::{
    atomic::{AtomicI64, Ordering},
    Arc,
};

mod store;
pub use crate::store::SqlCensoredContentStore;

pub mod config {
    pub const GET_OPERATION: &str = "GET";
    pub const PUT_OPERATION: &str = "PUT";
    pub const MIN_REPORT_TIME_DIFFERENCE_NS: i64 = 1_000_000_000;
}

// A wrapper for any blobstore, which provides a verification layer for the blacklisted blobs.
// The goal is to deny access to fetch sensitive data from the repository.
#[derive(Debug, Clone)]
pub struct CensoredBlob<T: Blobstore + Clone> {
    blobstore: T,
    censored: Option<HashMap<String, String>>,
    scuba_builder: ScubaSampleBuilder,
    timestamp: Arc<AtomicI64>,
}

impl<T: Blobstore + Clone> CensoredBlob<T> {
    pub fn new(
        blobstore: T,
        censored: Option<HashMap<String, String>>,
        scuba_censored_table: Option<String>,
    ) -> Self {
        let scuba_builder = ScubaSampleBuilder::with_opt_table(scuba_censored_table);
        let timestamp = Arc::new(AtomicI64::new(Timestamp::now().timestamp_nanos()));

        Self {
            blobstore,
            censored,
            scuba_builder,
            timestamp,
        }
    }

    pub fn err_if_censored(&self, key: &String) -> Result<(), Error> {
        match &self.censored {
            Some(censored) => censored.get(key).map_or(Ok(()), |task| {
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

    pub fn to_scuba_censored_blobstore_accessed(
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
}

impl<T: Blobstore + Clone> Blobstore for CensoredBlob<T> {
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        self.err_if_censored(&key)
            .map_err({
                cloned!(ctx, key);
                move |err| {
                    debug!(
                        ctx.logger(),
                        "Accessing censored blobstore with key {:?}", key
                    );
                    self.to_scuba_censored_blobstore_accessed(&ctx, &key, config::GET_OPERATION);
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
        self.err_if_censored(&key)
            .map_err({
                cloned!(ctx, key);
                move |err| {
                    debug!(
                        ctx.logger(),
                        "Updating censored blobstore with key {:?}", key
                    );

                    self.to_scuba_censored_blobstore_accessed(&ctx, &key, config::PUT_OPERATION);
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
    fn test_censored_key() {
        let mut rt = Runtime::new().unwrap();

        let uncensored_key = "foo".to_string();
        let censored_key = "bar".to_string();
        let censored_task = "bar task".to_string();

        let ctx = CoreContext::test_mock();

        let inner = EagerMemblob::new();
        let censored_pairs = hashmap! {
            censored_key.clone() => censored_task.clone(),
        };

        let blob = CensoredBlob::new(
            PrefixBlobstore::new(inner, "prefix"),
            Some(censored_pairs),
            None,
        );

        //Test put with blacklisted key
        let res = rt.block_on(blob.put(
            ctx.clone(),
            censored_key.clone(),
            BlobstoreBytes::from_bytes("test bar"),
        ));

        assert_matches!(
            res.expect_err("the key should be blacklisted").downcast::<ErrorKind>(),
            Ok(ErrorKind::Censored(_, ref task)) if task == &censored_task
        );

        //Test key added to the blob
        let res = rt.block_on(blob.put(
            ctx.clone(),
            uncensored_key.clone(),
            BlobstoreBytes::from_bytes("test foo"),
        ));
        assert!(res.is_ok(), "the key should be added successfully");

        // Test accessing a key which is censored
        let res = rt.block_on(blob.get(ctx.clone(), censored_key.clone()));

        assert_matches!(
            res.expect_err("the key should be censored").downcast::<ErrorKind>(),
            Ok(ErrorKind::Censored(_, ref task)) if task == &censored_task
        );

        // Test accessing a key which exists and is accesible
        let res = rt.block_on(blob.get(ctx.clone(), uncensored_key.clone()));
        assert!(res.is_ok(), "the key should be found and available");
    }
}
