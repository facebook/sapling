/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use blobstore::Blobstore;
use context::CoreContext;
use futures_ext::BoxFuture;
use mononoke_types::BlobstoreBytes;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub struct TracingBlobstore<T> {
    inner: T,
    gets: Arc<Mutex<Vec<String>>>,
}

impl<T> TracingBlobstore<T> {
    pub fn new(inner: T) -> Self {
        let gets = Arc::new(Mutex::new(vec![]));
        Self { inner, gets }
    }
}

impl<T> TracingBlobstore<T> {
    pub fn tracing_gets(&self) -> Vec<String> {
        let mut gets = self.gets.lock().expect("poisoned lock");
        std::mem::replace(&mut *gets, vec![])
    }
}

impl<T> Blobstore for TracingBlobstore<T>
where
    T: Blobstore,
{
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        let mut gets = self.gets.lock().expect("poisoned lock");
        gets.push(key.clone());

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
