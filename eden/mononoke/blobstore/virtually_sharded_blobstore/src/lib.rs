/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error};
use blobstore::{Blobstore, BlobstoreGetData, BlobstoreMetadata};
use bytes::Bytes;
use cachelib::VolatileLruCachePool;
use cloned::cloned;
use context::{CoreContext, PerfCounterType};
use futures::future::{BoxFuture, FutureExt};
use futures_stats::TimedFutureExt;
use mononoke_types::BlobstoreBytes;
use std::collections::hash_map::DefaultHasher;
use std::convert::AsRef;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::Arc;
use time_ext::DurationExt;
use tokio::sync::{Semaphore, SemaphorePermit};

const MAX_CACHELIB_VALUE_SIZE: u64 = 4 * 1024 * 1024;

struct CacheKey(String);

impl CacheKey {
    fn from_key(key: &str) -> Self {
        Self(format!("vsb.{}", key))
    }
}

impl AsRef<[u8]> for CacheKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

/// A layer over an existing blobstore that serializes access to virtual slices of the blobstore,
/// indexed by key. It also deduplicates writes for data that is already present.
#[derive(Clone)]
pub struct VirtuallyShardedBlobstore<T> {
    inner: Arc<Inner<T>>,
}

impl<T> VirtuallyShardedBlobstore<T> {
    pub fn new(
        blobstore: T,
        blob_pool: VolatileLruCachePool,
        presence_pool: VolatileLruCachePool,
        shards: NonZeroUsize,
    ) -> Self {
        let inner = Inner {
            blobstore,
            write_shards: Shards::new(shards, PerfCounterType::BlobPutsShardAccessWait),
            read_shards: Shards::new(shards, PerfCounterType::BlobGetsShardAccessWait),
            blob_pool,
            presence_pool,
        };

        Self {
            inner: Arc::new(inner),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for VirtuallyShardedBlobstore<T> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("VirtuallyShardedBlobstore")
            .field("blobstore", &self.inner.blobstore)
            .field("write_shards", &self.inner.write_shards.len())
            .field("read_shards", &self.inner.read_shards.len())
            .finish()
    }
}

struct Inner<T> {
    blobstore: T,
    write_shards: Shards,
    read_shards: Shards,
    presence_pool: VolatileLruCachePool,
    blob_pool: VolatileLruCachePool,
}

pub struct Shards {
    semaphores: Vec<Semaphore>,
    perf_counter_type: PerfCounterType,
}

impl Shards {
    fn new(shard_count: NonZeroUsize, perf_counter_type: PerfCounterType) -> Self {
        let semaphores = (0..shard_count.get())
            .into_iter()
            .map(|_| Semaphore::new(1))
            .collect();

        Self {
            semaphores,
            perf_counter_type,
        }
    }

    fn len(&self) -> usize {
        self.semaphores.len()
    }

    async fn acquire<'a>(&'a self, ctx: &CoreContext, key: &str) -> SemaphorePermit<'a> {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);

        let (stats, permit) = self.semaphores
            [(hasher.finish() % self.semaphores.len() as u64) as usize]
            .acquire()
            .timed()
            .await;

        ctx.perf_counters().add_to_counter(
            self.perf_counter_type,
            stats.completion_time.as_millis_unchecked() as i64,
        );

        permit
    }
}

impl<T> Inner<T> {
    fn get_from_cache(&self, key: &CacheKey) -> Result<BlobstoreGetData, Error> {
        let val = self
            .blob_pool
            .get(key)?
            .ok_or_else(|| anyhow!("Key is missing"))?;
        let val = BlobstoreGetData::decode(val).map_err(|()| anyhow!("Could not decode"))?;
        Ok(val)
    }

    fn set_is_present(&self, key: &CacheKey) -> Result<(), Error> {
        self.presence_pool.set(key, Bytes::from(b"P".as_ref()))?;
        Ok(())
    }

    fn set_in_cache(&self, key: &CacheKey, value: BlobstoreGetData) -> Result<(), Error> {
        self.set_is_present(key)?;

        let bytes = value
            .encode(MAX_CACHELIB_VALUE_SIZE)
            .map_err(|()| anyhow!("Could not encode"))?;
        self.blob_pool.set(key, bytes)?;

        Ok(())
    }

    /// Ask the cache if it knows whether the backing store has a value for this key. Returns
    /// `true` if there is definitely a value (i.e. cache entry in Present or Known state), `false`
    /// otherwise (Empty or Leased states).
    fn known_to_be_present_in_blobstore(&self, key: &CacheKey) -> Result<bool, Error> {
        Ok(self.presence_pool.get(key)?.is_some())
    }
}

impl<T: Blobstore> Blobstore for VirtuallyShardedBlobstore<T> {
    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        // TODO (later in this stack): cache that something exists but is too big to cache (I can
        // use the presence cache for this).
        cloned!(self.inner);

        async move {
            let cache_key = CacheKey::from_key(&key);

            if let Ok(v) = inner.get_from_cache(&cache_key) {
                return Ok(Some(v));
            }

            let permit = inner.read_shards.acquire(&ctx, &key).await;
            scopeguard::defer! { drop(permit); };

            if let Ok(v) = inner.get_from_cache(&cache_key) {
                return Ok(Some(v));
            }

            let res = inner.blobstore.get(ctx, key.clone()).await?;

            if let Some(ref data) = res {
                let _ = inner.set_in_cache(&cache_key, data.clone());
            }

            Ok(res)
        }
        .boxed()
    }

    fn put(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        cloned!(self.inner);

        async move {
            let cache_key = CacheKey::from_key(&key);

            if let Ok(true) = inner.known_to_be_present_in_blobstore(&cache_key) {
                return Ok(());
            }

            let permit = inner.write_shards.acquire(&ctx, &key).await;
            scopeguard::defer! { drop(permit); };

            if let Ok(true) = inner.known_to_be_present_in_blobstore(&cache_key) {
                return Ok(());
            }

            let res = inner.blobstore.put(ctx, key.clone(), value.clone()).await?;

            let value = BlobstoreGetData::new(BlobstoreMetadata::new(None), value);
            let _ = inner.set_in_cache(&cache_key, value);

            Ok(res)
        }
        .boxed()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<'static, Result<bool, Error>> {
        cloned!(self.inner);

        async move {
            let cache_key = CacheKey::from_key(&key);

            if let Ok(true) = inner.known_to_be_present_in_blobstore(&cache_key) {
                return Ok(true);
            }

            let exists = inner.blobstore.is_present(ctx, key.clone()).await?;

            if exists {
                let _ = inner.set_is_present(&cache_key);
            }

            Ok(exists)
        }
        .boxed()
    }
}

#[cfg(all(test, fbcode_build))]
mod test {
    use super::*;
    use fbinit::FacebookInit;
    use nonzero_ext::nonzero;
    use once_cell::sync::OnceCell;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default, Debug)]
    struct Blob {
        puts: u64,
        gets: u64,
        bytes: Option<BlobstoreBytes>,
    }

    #[derive(Debug, Clone)]
    struct TestBlobstore {
        data: Arc<Mutex<HashMap<String, Blob>>>,
    }

    impl TestBlobstore {
        fn new() -> Self {
            Self {
                data: Arc::new(Mutex::new(HashMap::new())),
            }
        }
    }

    impl Blobstore for TestBlobstore {
        fn put(
            &self,
            _ctx: CoreContext,
            key: String,
            value: BlobstoreBytes,
        ) -> BoxFuture<'static, Result<(), Error>> {
            cloned!(self.data);

            async move {
                let mut data = data.lock().unwrap();
                let mut blob = data.entry(key).or_default();
                blob.puts += 1;
                blob.bytes = Some(value);
                Ok(())
            }
            .boxed()
        }

        fn get(
            &self,
            _ctx: CoreContext,
            key: String,
        ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
            cloned!(self.data);

            async move {
                let mut data = data.lock().unwrap();
                let mut blob = data.entry(key).or_default();
                blob.gets += 1;
                let ret = blob
                    .bytes
                    .as_ref()
                    .map(|b| BlobstoreGetData::new(BlobstoreMetadata::new(None), b.clone()));
                Ok(ret)
            }
            .boxed()
        }
    }

    fn make_blobstore(fb: FacebookInit) -> Result<VirtuallyShardedBlobstore<TestBlobstore>, Error> {
        static INSTANCE: OnceCell<()> = OnceCell::new();
        INSTANCE.get_or_init(|| {
            let config = cachelib::LruCacheConfig::new(64 * 1024 * 1024);
            cachelib::init_cache_once(fb, config).unwrap();
        });

        let blob_pool = cachelib::get_or_create_volatile_pool("blobs", 8 * 1024 * 1024)?;
        let presence_pool = cachelib::get_or_create_volatile_pool("presence", 8 * 1024 * 1024)?;

        Ok(VirtuallyShardedBlobstore::new(
            TestBlobstore::new(),
            blob_pool,
            presence_pool,
            nonzero!(2usize),
        ))
    }

    #[fbinit::compat_test]
    async fn test_dedupe_reads(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let blobstore = make_blobstore(fb)?;

        let key = "foo".to_string();

        futures::future::try_join_all(
            (0..10usize).map(|_| blobstore.get(ctx.clone(), key.clone())),
        )
        .await?;

        {
            let mut data = blobstore.inner.blobstore.data.lock().unwrap();
            let mut blob = data.entry(key.clone()).or_default();
            assert_eq!(blob.gets, 10);
            blob.bytes = Some(BlobstoreBytes::from_bytes("foo"));
        }

        futures::future::try_join_all(
            (0..10usize).map(|_| blobstore.get(ctx.clone(), key.clone())),
        )
        .await?;

        {
            let mut data = blobstore.inner.blobstore.data.lock().unwrap();
            let blob = data.entry(key.clone()).or_default();
            assert_eq!(blob.gets, 11);
        }

        futures::future::try_join_all(
            (0..10usize).map(|_| blobstore.is_present(ctx.clone(), key.clone())),
        )
        .await?;

        {
            let mut data = blobstore.inner.blobstore.data.lock().unwrap();
            let blob = data.entry(key.clone()).or_default();
            assert_eq!(blob.gets, 11);
        }

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_dedupe_writes(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let blobstore = make_blobstore(fb)?;

        let key = "foo".to_string();
        let val = BlobstoreBytes::from_bytes("foo");

        futures::future::try_join_all(
            (0..10usize).map(|_| blobstore.put(ctx.clone(), key.clone(), val.clone())),
        )
        .await?;

        {
            let mut data = blobstore.inner.blobstore.data.lock().unwrap();
            let blob = data.entry(key.clone()).or_default();
            assert_eq!(blob.puts, 1);
            assert_eq!(blob.bytes, Some(val));
        }

        futures::future::try_join_all(
            (0..10usize).map(|_| blobstore.get(ctx.clone(), key.clone())),
        )
        .await?;

        {
            let mut data = blobstore.inner.blobstore.data.lock().unwrap();
            let blob = data.entry(key.clone()).or_default();
            assert_eq!(blob.gets, 0);
        }

        Ok(())
    }
}
