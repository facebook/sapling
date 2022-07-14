/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use context::CoreContext;
use mononoke_types::BlobstoreBytes;
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Debug)]
pub struct TracingBlobstore<T> {
    inner: T,
    gets: Arc<Mutex<Vec<String>>>,
}

impl<T: std::fmt::Display> std::fmt::Display for TracingBlobstore<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "TracingBlobstore<{}>", &self.inner)
    }
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
        std::mem::take(&mut *gets)
    }
}

#[async_trait]
impl<T: Blobstore> Blobstore for TracingBlobstore<T> {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.gets
            .lock()
            .expect("poisoned lock")
            .push(key.to_owned());
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
