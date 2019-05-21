// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use failure_ext::Error;
use futures_ext::BoxFuture;
use std::sync::Arc;

use blobstore::{Blobstore, BlobstoreBytes};
use context::CoreContext;

// A wrapper for any blobstore, which provides a verification layer for the blacklisted blobs.
// The goal is to deny access to fetch sensitive data from the repository.
#[derive(Debug)]
pub struct Censoredblob {
    blobstore: Arc<dyn Blobstore>,
}

impl Censoredblob {
    pub fn new(blobstore: Arc<dyn Blobstore>) -> Self {
        Self { blobstore }
    }
}

impl Blobstore for Censoredblob {
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        self.blobstore.get(ctx, key)
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
