/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use blobstore::{Blobstore, BlobstoreGetData};
use context::CoreContext;
use futures::{
    future,
    stream::{self, StreamExt, TryStreamExt},
};
use lock_ext::LockExt;
use mononoke_types::BlobstoreBytes;
use std::{
    collections::HashMap,
    mem,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

/// A blobstore wrapper that reads from the underlying blobstore but writes to memory.
#[derive(Clone, Debug)]
pub struct MemWritesBlobstore<T> {
    inner: T,
    cache: Arc<Mutex<HashMap<String, BlobstoreBytes>>>,
    no_access_to_inner: Arc<AtomicBool>,
}

impl<T: std::fmt::Display> std::fmt::Display for MemWritesBlobstore<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "MemWritesBlobstore<{}>", self.inner)
    }
}

impl<T: Blobstore + Clone> MemWritesBlobstore<T> {
    pub fn new(blobstore: T) -> Self {
        Self {
            inner: blobstore,
            cache: Default::default(),
            no_access_to_inner: Default::default(),
        }
    }

    /// Write all in-memory entries to underlying blobstore.
    ///
    /// NOTE: In case of error all pending changes will be lost.
    pub async fn persist<'a>(&'a self, ctx: &'a CoreContext) -> Result<()> {
        if self.no_access_to_inner.load(Ordering::Relaxed) {
            return Err(anyhow!(
                "unexpected write to memory blobstore when access to inner blobstore was disabled"
            ));
        }

        let items = self.cache.with(|cache| mem::replace(cache, HashMap::new()));
        stream::iter(items)
            .map(|(key, value)| self.inner.put(ctx, key, value))
            .buffered(4096)
            .try_for_each(|_| future::ready(Ok(())))
            .await
    }

    pub fn get_inner(&self) -> T {
        self.inner.clone()
    }

    pub fn get_cache(&self) -> &Arc<Mutex<HashMap<String, BlobstoreBytes>>> {
        &self.cache
    }

    pub fn set_no_access_to_inner(&self, no_access_to_inner: bool) {
        self.no_access_to_inner
            .store(no_access_to_inner, Ordering::Relaxed);
    }
}

#[async_trait]
impl<T: Blobstore + Clone> Blobstore for MemWritesBlobstore<T> {
    async fn put<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        self.cache.with(|cache| cache.insert(key, value));
        Ok(())
    }

    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        match self.cache.with(|cache| cache.get(key).cloned()) {
            Some(value) => Ok(Some(value.into())),
            None => {
                if self.no_access_to_inner.load(Ordering::Relaxed) {
                    return Ok(None);
                }

                Ok(self.inner.get(ctx, key).await?.map(Into::into))
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use borrowed::borrowed;
    use bytes::Bytes;
    use fbinit::FacebookInit;
    use memblob::Memblob;

    #[fbinit::test]
    async fn basic_read(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);
        let inner = Memblob::default();
        let foo_key = "foo";
        inner
            .put(
                ctx,
                foo_key.to_owned(),
                BlobstoreBytes::from_bytes("foobar"),
            )
            .await
            .expect("initial put should work");
        let outer = MemWritesBlobstore::new(inner.clone());

        assert!(
            outer
                .is_present(ctx, foo_key)
                .await
                .expect("is_present to inner should work")
                .assume_not_found_if_unsure()
        );

        assert_eq!(
            outer
                .get(ctx, foo_key)
                .await
                .expect("get to inner should work")
                .expect("value should be present")
                .into_raw_bytes(),
            Bytes::from("foobar"),
        );
    }

    #[fbinit::test]
    async fn redirect_writes(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);
        let inner = Memblob::default();
        let foo_key = "foo";

        let outer = MemWritesBlobstore::new(inner.clone());
        outer
            .put(
                ctx,
                foo_key.to_owned(),
                BlobstoreBytes::from_bytes("foobar"),
            )
            .await
            .expect("put should work");

        assert!(
            !inner
                .is_present(ctx, foo_key)
                .await
                .expect("is_present on inner should work")
                .assume_not_found_if_unsure(),
            "foo should not be present in inner",
        );

        assert!(
            outer
                .is_present(ctx, foo_key)
                .await
                .expect("is_present on outer should work")
                .assume_not_found_if_unsure(),
            "foo should be present in outer",
        );

        assert_eq!(
            outer
                .get(ctx, foo_key)
                .await
                .expect("get to outer should work")
                .expect("value should be present")
                .into_raw_bytes(),
            Bytes::from("foobar"),
        );
    }

    #[fbinit::test]
    async fn test_persist(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);

        let inner = Memblob::default();
        let outer = MemWritesBlobstore::new(inner.clone());

        let key = "key";
        let value = BlobstoreBytes::from_bytes("value");

        outer.put(ctx, key.to_owned(), value.clone()).await?;

        assert!(inner.get(ctx, key).await?.is_none());

        outer.persist(ctx).await?;

        assert_eq!(inner.get(ctx, key).await?, Some(value.into()));

        Ok(())
    }
}
