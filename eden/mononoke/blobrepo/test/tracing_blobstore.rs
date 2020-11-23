/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use blobstore::{Blobstore, BlobstoreGetData};
use context::CoreContext;
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

#[async_trait]
impl<T: Blobstore> Blobstore for TracingBlobstore<T> {
    async fn get(&self, ctx: CoreContext, key: String) -> Result<Option<BlobstoreGetData>> {
        self.gets.lock().expect("poisoned lock").push(key.clone());
        self.inner.get(ctx, key).await
    }

    async fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> Result<()> {
        self.inner.put(ctx, key, value).await
    }

    async fn is_present(&self, ctx: CoreContext, key: String) -> Result<bool> {
        self.inner.is_present(ctx, key).await
    }
}
