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
use std::collections::HashSet;
use std::sync::Arc;

mod errors;
use crate::errors::ErrorKind;

// A wrapper for any blobstore, which provides a verification layer for the blacklisted blobs.
// The goal is to deny access to fetch sensitive data from the repository.
#[derive(Debug, Clone)]
pub struct CensoredBlob {
    blobstore: Arc<dyn Blobstore>,
    censored: Arc<HashSet<String>>,
}

impl CensoredBlob {
    pub fn new(blobstore: Arc<dyn Blobstore>, censored: Arc<HashSet<String>>) -> Self {
        Self {
            blobstore,
            censored,
        }
    }
}

impl Blobstore for CensoredBlob {
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        if !self.censored.contains(&key) {
            self.blobstore.get(ctx, key)
        } else {
            future::err(ErrorKind::Censored(key).into()).boxify()
        }
    }

    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        self.blobstore.put(ctx, key, value)
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
    use context::CoreContext;
    use maplit::hashset;
    use memblob::EagerMemblob;
    use tokio::runtime::Runtime;

    #[test]
    fn test_censored_key() {
        let mut rt = Runtime::new().unwrap();

        let foo_key = "foo".to_string();
        let bar_key = "bar".to_string();
        let ctx = CoreContext::test_mock();

        let inner = EagerMemblob::new();

        let censored_keys = Arc::new(hashset! {bar_key.clone()});

        let blob = CensoredBlob::new(Arc::new(inner), censored_keys);

        //Test key added to the blob
        let res = rt.block_on(blob.put(
            ctx.clone(),
            foo_key.clone(),
            BlobstoreBytes::from_bytes("test foo"),
        ));
        assert!(res.is_ok(), "the key should be added successfully");

        // Test accessing a key which is censored
        let res = rt.block_on(blob.get(ctx.clone(), bar_key.clone()));
        assert!(!res.is_ok(), "the key should be censored");

        // Test accessing a key which exists and is accesible
        let res = rt.block_on(blob.get(ctx.clone(), foo_key.clone()));
        assert!(res.is_ok(), "the key should be found and available");
    }
}
