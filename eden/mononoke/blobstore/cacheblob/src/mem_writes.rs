/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobstore::{Blobstore, BlobstoreGetData};
use context::CoreContext;
use futures::future::{self, BoxFuture, FutureExt, TryFutureExt};
use futures_old::{stream, Future, Stream};
use lock_ext::LockExt;
use mononoke_types::BlobstoreBytes;
use std::{
    collections::HashMap,
    mem,
    sync::{Arc, Mutex},
};

/// A blobstore wrapper that reads from the underlying blobstore but writes to memory.
#[derive(Clone, Debug)]
pub struct MemWritesBlobstore<T: Blobstore + Clone> {
    inner: T,
    cache: Arc<Mutex<HashMap<String, BlobstoreBytes>>>,
}

impl<T: Blobstore + Clone> MemWritesBlobstore<T> {
    pub fn new(blobstore: T) -> Self {
        Self {
            inner: blobstore,
            cache: Default::default(),
        }
    }

    /// Writre all in-memory entries to unerlying blobstore.
    ///
    /// NOTE: In case of error all pending changes will be lost.
    pub fn persist(&self, ctx: CoreContext) -> impl Future<Item = (), Error = Error> {
        let items = self.cache.with(|cache| mem::replace(cache, HashMap::new()));
        stream::iter_ok(items)
            .map({
                let inner = self.inner.clone();
                move |(key, value)| inner.put(ctx.clone(), key, value).compat()
            })
            .buffered(4096)
            .for_each(|_| Ok(()))
    }

    pub fn get_inner(&self) -> T {
        self.inner.clone()
    }

    pub fn get_cache(&self) -> &Arc<Mutex<HashMap<String, BlobstoreBytes>>> {
        &self.cache
    }
}

impl<T: Blobstore + Clone> Blobstore for MemWritesBlobstore<T> {
    fn put(
        &self,
        _ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        self.cache.with(|cache| cache.insert(key, value));
        future::ok(()).boxed()
    }

    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        match self.cache.with(|cache| cache.get(&key).cloned()) {
            Some(value) => future::ok(Some(value.into())).boxed(),
            None => self
                .inner
                .get(ctx, key)
                .map_ok(|opt_blob| opt_blob.map(Into::into))
                .boxed(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use bytes::Bytes;
    use fbinit::FacebookInit;
    use futures::compat::Future01CompatExt;
    use memblob::EagerMemblob;

    #[fbinit::compat_test]
    async fn basic_read(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let inner = EagerMemblob::new();
        let foo_key = "foo".to_string();
        inner
            .put(
                ctx.clone(),
                foo_key.clone(),
                BlobstoreBytes::from_bytes("foobar"),
            )
            .await
            .expect("initial put should work");
        let outer = MemWritesBlobstore::new(inner.clone());

        assert!(outer
            .is_present(ctx.clone(), foo_key.clone())
            .await
            .expect("is_present to inner should work"));

        assert_eq!(
            outer
                .get(ctx, foo_key.clone())
                .await
                .expect("get to inner should work")
                .expect("value should be present")
                .into_raw_bytes(),
            Bytes::from("foobar"),
        );
    }

    #[fbinit::compat_test]
    async fn redirect_writes(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let inner = EagerMemblob::new();
        let foo_key = "foo".to_string();

        let outer = MemWritesBlobstore::new(inner.clone());
        outer
            .put(
                ctx.clone(),
                foo_key.clone(),
                BlobstoreBytes::from_bytes("foobar"),
            )
            .await
            .expect("put should work");

        assert!(
            !inner
                .is_present(ctx.clone(), foo_key.clone())
                .await
                .expect("is_present on inner should work"),
            "foo should not be present in inner",
        );

        assert!(
            outer
                .is_present(ctx.clone(), foo_key.clone())
                .await
                .expect("is_present on outer should work"),
            "foo should be present in outer",
        );

        assert_eq!(
            outer
                .get(ctx, foo_key.clone())
                .await
                .expect("get to outer should work")
                .expect("value should be present")
                .into_raw_bytes(),
            Bytes::from("foobar"),
        );
    }

    #[fbinit::compat_test]
    async fn test_persist(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        let inner = EagerMemblob::new();
        let outer = MemWritesBlobstore::new(inner.clone());

        let key = "key".to_string();
        let value = BlobstoreBytes::from_bytes("value");

        outer.put(ctx.clone(), key.clone(), value.clone()).await?;

        assert!(inner.get(ctx.clone(), key.clone()).await?.is_none());

        outer.persist(ctx.clone()).compat().await?;

        assert_eq!(inner.get(ctx, key).await?, Some(value.into()));

        Ok(())
    }
}
