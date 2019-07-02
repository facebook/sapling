// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use blobstore::{Blobstore, BlobstoreBytes};
use context::CoreContext;
use failure_ext::Error;
use futures::future;
use futures_ext::{BoxFuture, FutureExt};
use std::collections::HashMap;

mod errors;
use crate::errors::ErrorKind;

mod store;
pub use crate::store::SqlCensoredContentStore;

// A wrapper for any blobstore, which provides a verification layer for the blacklisted blobs.
// The goal is to deny access to fetch sensitive data from the repository.
#[derive(Debug, Clone)]
pub struct CensoredBlob<T: Blobstore + Clone> {
    blobstore: T,
    censored: HashMap<String, String>,
}

impl<T: Blobstore + Clone> CensoredBlob<T> {
    pub fn new(blobstore: T, censored: HashMap<String, String>) -> Self {
        Self {
            blobstore,
            censored,
        }
    }

    pub fn is_censored(&self, key: String) -> Result<(), Error> {
        match self.censored.get(&key) {
            Some(task) => Err(ErrorKind::Censored(key, task.clone()).into()),
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
}

impl<T: Blobstore + Clone> Blobstore for CensoredBlob<T> {
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        match self.censored.get(&key) {
            Some(task) => future::err(ErrorKind::Censored(key, task.clone()).into()).boxify(),
            None => self.blobstore.get(ctx, key),
        }
    }

    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        match self.censored.get(&key) {
            Some(task) => future::err(ErrorKind::Censored(key, task.clone()).into()).boxify(),
            None => self.blobstore.put(ctx, key, value),
        }
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

        let blob = CensoredBlob::new(PrefixBlobstore::new(inner, "prefix"), censored_pairs);

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
