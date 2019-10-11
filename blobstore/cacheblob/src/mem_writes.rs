/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use blobstore::Blobstore;
use context::CoreContext;
use failure_ext::Error;
use futures::{future, stream, Future, Stream};
use futures_ext::{BoxFuture, FutureExt};
use lock_ext::LockExt;
use mononoke_types::BlobstoreBytes;
use std::{
    collections::HashMap,
    mem,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug)]
enum Cache {
    Put(BlobstoreBytes),
    Get(Option<BlobstoreBytes>),
}

/// A blobstore wrapper that reads from the underlying blobstore but writes to memory.
#[derive(Clone, Debug)]
pub struct MemWritesBlobstore<T: Blobstore + Clone> {
    inner: T,
    cache: Arc<Mutex<HashMap<String, Cache>>>,
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
            .filter_map(|(key, cache)| match cache {
                Cache::Put(value) => Some((key, value)),
                Cache::Get(_) => None,
            })
            .map({
                let inner = self.inner.clone();
                move |(key, value)| inner.put(ctx.clone(), key, value)
            })
            .buffered(4096)
            .for_each(|_| Ok(()))
    }

    pub fn get_inner(&self) -> T {
        self.inner.clone()
    }
}

impl<T: Blobstore + Clone> Blobstore for MemWritesBlobstore<T> {
    fn put(&self, _ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        self.cache
            .with(|cache| cache.insert(key, Cache::Put(value)));
        future::ok(()).boxify()
    }

    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        match self.cache.with(|cache| cache.get(&key).cloned()) {
            Some(cache) => {
                let result = match cache {
                    Cache::Put(value) => Some(value),
                    Cache::Get(result) => result,
                };
                future::ok(result).boxify()
            }
            None => self
                .inner
                .get(ctx, key.clone())
                .map({
                    let cache = self.cache.clone();
                    move |result| {
                        cache.with(|cache| cache.insert(key, Cache::Get(result.clone())));
                        result
                    }
                })
                .boxify(),
        }
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        self.get(ctx, key).map(|result| result.is_some()).boxify()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use bytes::Bytes;
    use fbinit::FacebookInit;
    use memblob::EagerMemblob;

    #[fbinit::test]
    fn basic_read(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let inner = EagerMemblob::new();
        let foo_key = "foo".to_string();
        inner
            .put(
                ctx.clone(),
                foo_key.clone(),
                BlobstoreBytes::from_bytes("foobar"),
            )
            .wait()
            .expect("initial put should work");
        let outer = MemWritesBlobstore::new(inner.clone());

        assert!(outer
            .is_present(ctx.clone(), foo_key.clone())
            .wait()
            .expect("is_present to inner should work"));

        assert_eq!(
            outer
                .get(ctx, foo_key.clone())
                .wait()
                .expect("get to inner should work")
                .expect("value should be present")
                .into_bytes(),
            Bytes::from("foobar"),
        );
    }

    #[fbinit::test]
    fn redirect_writes(fb: FacebookInit) {
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
            .wait()
            .expect("put should work");

        assert!(
            !inner
                .is_present(ctx.clone(), foo_key.clone())
                .wait()
                .expect("is_present on inner should work"),
            "foo should not be present in inner",
        );

        assert!(
            outer
                .is_present(ctx.clone(), foo_key.clone())
                .wait()
                .expect("is_present on outer should work"),
            "foo should be present in outer",
        );

        assert_eq!(
            outer
                .get(ctx, foo_key.clone())
                .wait()
                .expect("get to outer should work")
                .expect("value should be present")
                .into_bytes(),
            Bytes::from("foobar"),
        );
    }

    #[fbinit::test]
    fn test_persist(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut rt = tokio::runtime::Runtime::new()?;

        let inner = EagerMemblob::new();
        let outer = MemWritesBlobstore::new(inner.clone());

        let key = "key".to_string();
        let value = BlobstoreBytes::from_bytes("value");

        rt.block_on(outer.put(ctx.clone(), key.clone(), value.clone()))?;

        assert!(rt.block_on(inner.get(ctx.clone(), key.clone()))?.is_none());

        rt.block_on(outer.persist(ctx.clone()))?;

        assert_eq!(rt.block_on(inner.get(ctx.clone(), key))?, Some(value));

        Ok(())
    }
}
