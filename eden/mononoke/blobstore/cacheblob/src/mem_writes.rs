/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use context::CoreContext;
use futures::future;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use lock_ext::LockExt;
use mononoke_types::BlobstoreBytes;
use std::collections::HashMap;
use std::mem;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::Mutex as AsyncMutex;

#[derive(Default, Debug)]
pub struct Cache {
    live: HashMap<String, BlobstoreBytes>,
    flushing: Option<Arc<HashMap<String, BlobstoreBytes>>>,
}

impl Cache {
    pub fn len(&self) -> usize {
        self.live.len() + self.flushing.as_ref().map_or(0, |flushing| flushing.len())
    }
}

/// A blobstore wrapper that reads from the underlying blobstore but writes to memory.
#[derive(Clone, Debug)]
pub struct MemWritesBlobstore<T> {
    inner: T,
    cache: Arc<Mutex<Cache>>,
    // Mutex to ensure only one task is flushing the cache at a time.
    // Note: this doesn't wrap the cache as read access is permitted while
    // the mutex is held.
    flush_mutex: Arc<AsyncMutex<()>>,
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
            flush_mutex: Default::default(),
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

        // Obtain the flush mutex.  This should ensure that only one persist
        // happens at a time.
        let _flush_guard = self.flush_mutex.lock().await;

        let items = self.cache.with(|cache| {
            if cache.flushing.is_some() {
                // This should be prevented by the flush guard.
                return Err(anyhow!(
                    "unexpected persist while another persist is ongoing"
                ));
            }
            let flushing = Arc::new(mem::take(&mut cache.live));
            cache.flushing = Some(flushing.clone());
            Ok(flushing)
        })?;

        let flush = async_stream::stream! {
            for (key, value) in items.iter() {
                 yield self.inner.put(ctx, key.clone(), value.clone());
            }
        };

        let result = flush
            .buffered(4096)
            .try_for_each(|_| future::ready(Ok(())))
            .await;

        // Discard flushing items, whether or not we were successful at
        // flushing the cache.
        self.cache.with(|cache| {
            cache.flushing = None;
        });

        result
    }

    pub fn get_inner(&self) -> T {
        self.inner.clone()
    }

    pub fn get_cache(&self) -> &Arc<Mutex<Cache>> {
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
        self.cache.with(|cache| cache.live.insert(key, value));
        Ok(())
    }

    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let value = self.cache.with(|cache| {
            if let Some(value) = cache.live.get(key) {
                Some(value.clone())
            } else if let Some(flushing) = cache.flushing.as_ref() {
                flushing.get(key).cloned()
            } else {
                None
            }
        });

        match value {
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
    use blobstore::PutBehaviour;
    use borrowed::borrowed;
    use bytes::Bytes;
    use cloned::cloned;
    use fbinit::FacebookInit;
    use memblob::Memblob;
    use std::time::Duration;
    use tokio::sync::watch;

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

    /// A blobstore wrapper that prevents writes until a flag is set.
    #[derive(Clone, Debug)]
    pub struct GatedBlobstore<T> {
        inner: T,
        allow: watch::Receiver<bool>,
    }

    impl<T: Blobstore + Clone> GatedBlobstore<T> {
        pub fn new(blobstore: T, allow: watch::Receiver<bool>) -> Self {
            Self {
                inner: blobstore,
                allow,
            }
        }
    }
    impl<T: std::fmt::Display> std::fmt::Display for GatedBlobstore<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "GatedBlobstore<{}>", self.inner)
        }
    }

    #[async_trait]
    impl<T: Blobstore + Clone> Blobstore for GatedBlobstore<T> {
        async fn put<'a>(
            &'a self,
            ctx: &'a CoreContext,
            key: String,
            value: BlobstoreBytes,
        ) -> Result<()> {
            let mut allow = self.allow.clone();
            while !*allow.borrow() {
                // Wait until we are allowed to continue.
                allow.changed().await?;
            }
            self.inner.put(ctx, key, value).await
        }

        async fn get<'a>(
            &'a self,
            ctx: &'a CoreContext,
            key: &'a str,
        ) -> Result<Option<BlobstoreGetData>> {
            self.inner.get(ctx, key).await
        }
    }

    #[fbinit::test]
    async fn test_persist_concurrency(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);

        let inner = Memblob::new(PutBehaviour::Overwrite);
        let (allow_tx, allow_rx) = watch::channel(false);
        let delay = Arc::new(GatedBlobstore::new(inner.clone(), allow_rx));
        let outer = MemWritesBlobstore::new(delay);

        let key = "key";
        let value = BlobstoreBytes::from_bytes("value");

        outer.put(ctx, key.to_owned(), value.clone()).await?;
        assert!(inner.get(ctx, key).await?.is_none());

        let persist = tokio::spawn({
            cloned!(ctx, outer);
            async move { outer.persist(&ctx).await }
        });

        // Wait until a write is in flight (there will be more than 1
        // receiver, as each write clones the receiver)
        while allow_tx.receiver_count() <= 1 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert_eq!(inner.get(ctx, key).await?, None);
        assert_eq!(outer.get(ctx, key).await?, Some(value.clone().into()));

        let key2 = "key2";
        let value2 = BlobstoreBytes::from_bytes("value2");

        outer.put(ctx, key.to_owned(), value2.clone()).await?;
        outer.put(ctx, key2.to_owned(), value2.clone()).await?;

        assert_eq!(inner.get(ctx, key).await?, None);
        assert_eq!(inner.get(ctx, key2).await?, None);
        assert_eq!(outer.get(ctx, key).await?, Some(value2.clone().into()));
        assert_eq!(outer.get(ctx, key2).await?, Some(value2.clone().into()));

        allow_tx.send(true)?;
        persist.await??;

        assert_eq!(inner.get(ctx, key).await?, Some(value.into()));
        assert_eq!(inner.get(ctx, key2).await?, None);
        assert_eq!(outer.get(ctx, key).await?, Some(value2.clone().into()));
        assert_eq!(outer.get(ctx, key2).await?, Some(value2.clone().into()));

        outer.persist(ctx).await?;
        assert_eq!(inner.get(ctx, key).await?, Some(value2.clone().into()));
        assert_eq!(inner.get(ctx, key2).await?, Some(value2.into()));

        Ok(())
    }
}
