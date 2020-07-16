/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod ratelimit;
mod shard;

use anyhow::{anyhow, Context, Error};
use blobstore::{Blobstore, BlobstoreGetData, BlobstoreMetadata};
use bytes::{buf::ext::Chain, BufMut, Bytes, BytesMut};
use cachelib::VolatileLruCachePool;
use cloned::cloned;
use context::{CoreContext, PerfCounterType};
use futures::future::{BoxFuture, FutureExt};
use mononoke_types::BlobstoreBytes;
use scuba_ext::ScubaSampleBuilderExt;
use stats::prelude::*;
use std::convert::AsRef;
use std::convert::TryInto;
use std::fmt;
use std::hash::Hasher;
use std::num::NonZeroU64;
use std::num::NonZeroUsize;
use std::sync::Arc;
use tunables::tunables;
use twox_hash::XxHash;

use crate::ratelimit::{AccessReason, Ticket};
use shard::{SemaphoreAcquisition, Shards};

define_stats! {
    prefix = "mononoke.virtually_sharded_blobstore";
    gets: timeseries(Sum),
    gets_deduped: timeseries(Sum),
    gets_not_storable: timeseries(Sum),
    puts: timeseries(Sum),
    puts_deduped: timeseries(Sum),
}

// 4MiB, minus a little space for the STORED prefix and the key.
const MAX_CACHELIB_VALUE_SIZE: u64 = 4 * 1024 * 1024 - 1024;

const NOT_STORABLE: Bytes = Bytes::from_static(&[0]);
const STORED: Bytes = Bytes::from_static(&[1]);

#[derive(Debug)]
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

enum CacheData {
    /// Represents data that was found in cache.
    Stored(BlobstoreGetData),
    /// Represents data that is known to not be storable in cache (because it's too large,
    /// presumably). For this data, we skip semaphore access.
    NotStorable,
}

impl CacheData {
    fn deserialize(mut val: Bytes) -> Result<Self, Error> {
        let prefix = val.split_to(1);

        if prefix.as_ref() == NOT_STORABLE {
            return Ok(Self::NotStorable);
        }

        if prefix.as_ref() == STORED {
            let val = BlobstoreGetData::decode(val)
                .map_err(|()| anyhow!("Invalid data in blob cache"))?;
            return Ok(Self::Stored(val));
        }

        Err(anyhow!("Invalid prefix: {:?}", prefix))
    }
}

#[derive(PartialEq, Eq, Copy, Clone, Debug)]
enum PresenceData {
    /// We read this at some point. We will not dedupe based on this.
    Get,
    /// We wrote this ourselves, and this is the hash of the data. We will dedupe based on this.
    Put(u64),
}

impl PresenceData {
    const GET: Bytes = Bytes::from_static(&[0]);
    const PUT: Bytes = Bytes::from_static(&[1]);

    fn from_put(v: &BlobstoreBytes) -> Self {
        let mut hasher = XxHash::with_seed(0);
        hasher.write(v.as_bytes().as_ref());
        Self::Put(hasher.finish())
    }

    fn deserialize(mut val: Bytes) -> Result<Self, Error> {
        let prefix = val.split_to(1);

        if prefix.as_ref() == Self::GET {
            return Ok(Self::Get);
        }

        if prefix.as_ref() == Self::PUT {
            let bytes: [u8; 8] = val
                .as_ref()
                .try_into()
                .context("Invalid put data in presence cache")?;

            return Ok(Self::Put(u64::from_ne_bytes(bytes)));
        }

        Err(anyhow!("Invalid prefix: {:?}", prefix))
    }

    fn serialize(&self) -> Bytes {
        match self {
            Self::Get => Self::GET,
            Self::Put(v) => {
                let mut buff = BytesMut::with_capacity(1 + std::mem::size_of::<u64>());
                buff.put(Self::PUT);
                buff.put(v.to_ne_bytes().as_ref());
                buff.freeze()
            }
        }
    }

    fn is_put(&self) -> bool {
        match self {
            Self::Get => false,
            Self::Put(..) => true,
        }
    }
}

/// A type representing that a blob is known to exist. Used in matches below.
struct KnownToExist;

/// We allow filtering cache writes to make testing easier. This function is a default that does
/// not filter.
fn allow_all_filter(_: &Bytes) -> Result<(), Error> {
    Ok(())
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
        let inner = Inner::new(
            blobstore,
            blob_pool,
            presence_pool,
            shards,
            allow_all_filter,
        );

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
    cache_filter: fn(&Bytes) -> Result<(), Error>,
}

impl<T> Inner<T> {
    pub fn new(
        blobstore: T,
        blob_pool: VolatileLruCachePool,
        presence_pool: VolatileLruCachePool,
        shards: NonZeroUsize,
        cache_filter: fn(&Bytes) -> Result<(), Error>,
    ) -> Self {
        Self {
            blobstore,
            write_shards: Shards::new(shards, PerfCounterType::BlobPutsShardAccessWait),
            read_shards: Shards::new(shards, PerfCounterType::BlobGetsShardAccessWait),
            blob_pool,
            presence_pool,
            cache_filter,
        }
    }
}

impl<T> Inner<T> {
    fn get_from_cache(&self, key: &CacheKey) -> Result<Option<CacheData>, Error> {
        let val = match self.blob_pool.get(key)? {
            Some(val) => val,
            None => return Ok(None),
        };

        Ok(Some(CacheData::deserialize(val)?))
    }

    /// Set presence for this cache key.
    fn set_is_present(&self, key: &CacheKey, value: PresenceData) -> Result<(), Error> {
        // If it's a put, then we overwrite existing data in cache, to record the new value.
        if value.is_put() {
            self.presence_pool.set_or_replace(key, value.serialize())?;
            return Ok(());
        }

        // If it's a get, we leave existing data alone. If existing data was a get then there is no
        // use updating it, and if it was a put, we don't *want* to update it (see
        // test_dedupe_writes_different_data).
        self.presence_pool.set(key, value.serialize())?;

        Ok(())
    }

    fn set_in_cache(
        &self,
        key: &CacheKey,
        presence: PresenceData,
        value: BlobstoreGetData,
    ) -> Result<(), Error> {
        self.set_is_present(key, presence)?;

        let stored = value
            .encode(MAX_CACHELIB_VALUE_SIZE)
            .map_err(|()| anyhow!("Could not encode"))
            .and_then(|encoded| {
                (self.cache_filter)(&encoded)?;
                self.blob_pool.set(key, Chain::new(STORED.clone(), encoded))
            })
            .unwrap_or(false);

        // NOTE: If a transient error occured while setting in cache, then we might store
        // NOT_STORABLE, even if the key is in fact storable. That's OK: it just means the next
        // gets will bypass the semaphore, but if the key does turn out to be cacheable, then it'll
        // get cached on the next read.
        if !stored {
            self.blob_pool.set(key, NOT_STORABLE.clone())?;
        }

        Ok(())
    }

    /// Ask the cache if it knows whether the backing store has a given request PresenceData for
    /// this key. Returns Some(KnownToExist) if so. The request argument lets the caller control
    /// what they want to see: PresenceData::Get means we just want to know this key is readable.
    /// PresenceData::Put(v) means we want to know v specifically was last written by us.
    fn check_presence(
        &self,
        key: &CacheKey,
        request: PresenceData,
    ) -> Result<Option<KnownToExist>, Error> {
        let stored = self
            .presence_pool
            .get(key)?
            .map(PresenceData::deserialize)
            .transpose()?;

        let r = match (stored, request) {
            (None, _) => {
                // Nothing in the presence cache at all (see test_read_after_write).
                None
            }
            (Some(_), PresenceData::Get) => {
                // Something in the cache, and we're not really looking for a specific value. This
                // works (see test_dedupe_reads).
                Some(KnownToExist)
            }
            (Some(PresenceData::Get), PresenceData::Put(..)) => {
                // We want a specific value, but we don't have one in cache. This does not work
                // (see test_dedupe_writes_different_data).
                None
            }
            (Some(PresenceData::Put(v1)), PresenceData::Put(v2)) => {
                // We have a put in the cache, and we want to check for a put. If they're the same,
                // this is a match (see test_dedupe_writes and test_dedupe_writes_different_data).
                if v1 == v2 {
                    Some(KnownToExist)
                } else {
                    None
                }
            }
        };

        Ok(r)
    }
}

fn report_deduplicated_put(ctx: &CoreContext, key: &str) {
    STATS::puts_deduped.add_value(1);

    let mut scuba = ctx.scuba().clone();
    if let Ok(Some(v)) = tunables()
        .get_deduplicated_put_sampling_rate()
        .try_into()
        .map(NonZeroU64::new)
    {
        scuba.sampled(v);
    }
    scuba.add("key", key).log_with_msg("Put deduplicated", None);

    ctx.perf_counters()
        .increment_counter(PerfCounterType::BlobPutsDeduplicated);
}

impl<T: Blobstore> Blobstore for VirtuallyShardedBlobstore<T> {
    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        cloned!(self.inner);

        async move {
            STATS::gets.add_value(1);
            let cache_key = CacheKey::from_key(&key);

            // First, check the cache, and acquire a permit for this key if necessary.

            let take_lease = match inner.get_from_cache(&cache_key)? {
                Some(CacheData::Stored(v)) => {
                    ctx.perf_counters()
                        .increment_counter(PerfCounterType::CachelibHits);
                    return Ok(Some(v));
                }
                Some(CacheData::NotStorable) => {
                    // We know for sure this data isn't cacheable. Don't try to acquire a permit
                    // for it, and proceed without the semaphore.
                    false
                }
                None => true,
            };

            let fut = async move {
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::CachelibMisses);

                let ticket = Ticket::new(&ctx, AccessReason::Read);

                let permit = if take_lease {
                    let acq = inner
                        .read_shards
                        .acquire(&ctx, &key, ticket, || inner.get_from_cache(&cache_key))
                        .await?;

                    match acq {
                        SemaphoreAcquisition::Cancelled(CacheData::Stored(v), ticket) => {
                            // The data is cached, that's great. Return it. We're not going to hit
                            // the blobstore, so also return out ticket.
                            STATS::gets_deduped.add_value(1);
                            ctx.perf_counters()
                                .increment_counter(PerfCounterType::BlobGetsDeduplicated);
                            ticket.cancel();
                            return Ok(Some(v));
                        }
                        SemaphoreAcquisition::Cancelled(CacheData::NotStorable, ticket) => {
                            // The data cannot be cached. We'll have to go to the blobstore. Wait
                            // for our ticket first.
                            STATS::gets_not_storable.add_value(1);
                            ticket.finish().await?;
                            None
                        }
                        SemaphoreAcquisition::Acquired(permit) => Some(permit),
                    }
                } else {
                    // We'll go to the blobstore, so wait for our ticket.
                    ticket.finish().await?;
                    None
                };

                // NOTE: This is a no-op, but it's here to ensure permit is still in scope at this
                // point (which it should: if it doesn't, then that means we unconditionally released
                // the semaphore before doing the get, and that's wrong).
                scopeguard::defer! { drop(permit) };

                // Now, actually go the underlying blobstore.
                let res = inner.blobstore.get(ctx.clone(), key.clone()).await?;

                // And finally, attempt to cache what we got back.
                if let Some(ref data) = res {
                    let _ = inner.set_in_cache(&cache_key, PresenceData::Get, data.clone());
                }

                Ok(res)
            };

            tokio::spawn(fut).await?
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
            STATS::puts.add_value(1);
            let cache_key = CacheKey::from_key(&key);
            let presence = PresenceData::from_put(&value);

            if let Ok(Some(KnownToExist)) = inner.check_presence(&cache_key, presence) {
                report_deduplicated_put(&ctx, &key);
                return Ok(());
            }

            let fut = async move {
                let ticket = Ticket::new(&ctx, AccessReason::Write);

                let acq = inner
                    .write_shards
                    .acquire(&ctx, &key, ticket, || {
                        inner.check_presence(&cache_key, presence)
                    })
                    .await?;

                let permit = match acq {
                    SemaphoreAcquisition::Cancelled(KnownToExist, ticket) => {
                        report_deduplicated_put(&ctx, &key);
                        ticket.cancel();
                        return Ok(());
                    }
                    SemaphoreAcquisition::Acquired(permit) => permit,
                };

                scopeguard::defer! { drop(permit) };

                let res = inner
                    .blobstore
                    .put(ctx.clone(), key.clone(), value.clone())
                    .await?;

                let value = BlobstoreGetData::new(BlobstoreMetadata::new(None), value);
                let _ = inner.set_in_cache(&cache_key, presence, value);

                Ok(res)
            };

            tokio::spawn(fut).await?
        }
        .boxed()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<'static, Result<bool, Error>> {
        cloned!(self.inner);

        async move {
            let cache_key = CacheKey::from_key(&key);
            let presence = PresenceData::Get;

            if let Ok(Some(KnownToExist)) = inner.check_presence(&cache_key, presence) {
                return Ok(true);
            }

            Ticket::new(&ctx, AccessReason::Read).finish().await?;

            let exists = inner.blobstore.is_present(ctx, key.clone()).await?;

            if exists {
                let _ = inner.set_is_present(&cache_key, presence);
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
    use futures_stats::TimedTryFutureExt;
    use nonzero_ext::nonzero;
    use once_cell::sync::OnceCell;
    use time_ext::DurationExt;

    fn make_blobstore<B: Blobstore>(
        fb: FacebookInit,
        blob: B,
        blob_pool_name: &str,
        cache_shards: NonZeroUsize,
        cache_filter: fn(&Bytes) -> Result<(), Error>,
    ) -> Result<VirtuallyShardedBlobstore<B>, Error> {
        static INSTANCE: OnceCell<()> = OnceCell::new();
        INSTANCE.get_or_init(|| {
            let config = cachelib::LruCacheConfig::new(64 * 1024 * 1024);
            cachelib::init_cache_once(fb, config).unwrap();
        });

        let blob_pool = cachelib::get_or_create_volatile_pool(blob_pool_name, 8 * 1024 * 1024)?;
        let presence_pool = cachelib::get_or_create_volatile_pool("presence", 8 * 1024 * 1024)?;

        let inner = Inner::new(blob, blob_pool, presence_pool, cache_shards, cache_filter);

        Ok(VirtuallyShardedBlobstore {
            inner: Arc::new(inner),
        })
    }

    fn reject_all_filter(_: &Bytes) -> Result<(), Error> {
        Err(anyhow!("Rejected!"))
    }

    mod sharding {
        use super::*;
        use std::collections::HashMap;
        use std::sync::Mutex;
        use std::time::Duration;
        use tokio::sync::broadcast::{self, Receiver, Sender};

        const TIMEOUT_MS: u64 = 100;

        /// Represents data stored in our TestBlobstore
        #[derive(Debug)]
        enum BlobData {
            Bytes(BlobstoreBytes),
            Channel(Sender<BlobstoreBytes>),
        }

        impl BlobData {
            /// Obtain a handle for a new get
            fn handle(&self) -> BlobDataHandle {
                match self {
                    BlobData::Bytes(ref b) => BlobDataHandle::Bytes(b.clone()),
                    BlobData::Channel(ref s) => BlobDataHandle::Channel(s.subscribe()),
                }
            }
        }

        /// Represents a handle for a single get from our TestBlobstore
        enum BlobDataHandle {
            Bytes(BlobstoreBytes),
            Channel(Receiver<BlobstoreBytes>),
        }

        impl BlobDataHandle {
            /// Obtain the bytes for this get.
            async fn bytes(self) -> Result<BlobstoreBytes, Error> {
                let b = match self {
                    BlobDataHandle::Bytes(b) => b,
                    BlobDataHandle::Channel(mut r) => r.recv().await?,
                };

                Ok(b)
            }
        }

        #[derive(Default, Debug)]
        struct Blob {
            puts: u64,
            gets: u64,
            data: Option<BlobData>,
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
                    blob.data = Some(BlobData::Bytes(value));
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
                    let handle = {
                        let mut data = data.lock().unwrap();
                        let blob = data.entry(key).or_default();
                        blob.gets += 1;
                        blob.data.as_ref().map(BlobData::handle)
                    };

                    let handle = match handle {
                        Some(handle) => handle,
                        None => {
                            return Ok(None);
                        }
                    };

                    let bytes = handle.bytes().await?;

                    Ok(Some(BlobstoreGetData::new(
                        BlobstoreMetadata::new(None),
                        bytes,
                    )))
                }
                .boxed()
            }
        }

        #[fbinit::test]
        async fn test_dedupe_reads(fb: FacebookInit) -> Result<(), Error> {
            let ctx = CoreContext::test_mock(fb);
            let blobstore = make_blobstore(
                fb,
                TestBlobstore::new(),
                "blobs",
                nonzero!(2usize),
                allow_all_filter,
            )?;

            let key = "foo".to_string();

            futures::future::try_join_all(
                (0..10usize).map(|_| blobstore.get(ctx.clone(), key.clone())),
            )
            .await?;

            {
                let mut data = blobstore.inner.blobstore.data.lock().unwrap();
                let mut blob = data.entry(key.clone()).or_default();
                assert_eq!(blob.gets, 10);
                blob.data = Some(BlobData::Bytes(BlobstoreBytes::from_bytes("foo")));
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

        #[fbinit::test]
        async fn test_cache_read(fb: FacebookInit) -> Result<(), Error> {
            let ctx = CoreContext::test_mock(fb);
            let blobstore = make_blobstore(
                fb,
                TestBlobstore::new(),
                "blobs",
                nonzero!(2usize),
                allow_all_filter,
            )?;

            let key = "foo".to_string();
            let val = BlobstoreBytes::from_bytes("foo");

            {
                let mut data = blobstore.inner.blobstore.data.lock().unwrap();
                let mut blob = data.entry(key.clone()).or_default();
                blob.data = Some(BlobData::Bytes(val.clone()));
            }

            let v1 = blobstore.get(ctx.clone(), key.clone()).await?;
            let v2 = blobstore.get(ctx.clone(), key.clone()).await?;

            {
                let mut data = blobstore.inner.blobstore.data.lock().unwrap();
                let blob = data.entry(key.clone()).or_default();
                assert_eq!(blob.gets, 1);
            }

            assert_eq!(v1.unwrap().as_bytes(), &val);
            assert_eq!(v2.unwrap().as_bytes(), &val);

            Ok(())
        }

        #[fbinit::test]
        async fn test_read_after_write(fb: FacebookInit) -> Result<(), Error> {
            let ctx = CoreContext::test_mock(fb);
            let blobstore = make_blobstore(
                fb,
                TestBlobstore::new(),
                "blobs",
                nonzero!(2usize),
                allow_all_filter,
            )?;

            let key = "foo".to_string();
            let val = BlobstoreBytes::from_bytes("foo");

            blobstore.put(ctx.clone(), key.clone(), val.clone()).await?;
            let v1 = blobstore.get(ctx.clone(), key.clone()).await?;

            {
                let mut data = blobstore.inner.blobstore.data.lock().unwrap();
                let blob = data.entry(key.clone()).or_default();
                assert_eq!(blob.gets, 0);
            }

            assert_eq!(v1.unwrap().as_bytes(), &val);

            Ok(())
        }

        #[fbinit::test]
        async fn test_do_not_serialize_not_storable(fb: FacebookInit) -> Result<(), Error> {
            let ctx = CoreContext::test_mock(fb);
            let blobstore = make_blobstore(
                fb,
                TestBlobstore::new(),
                "blobs",
                nonzero!(2usize),
                reject_all_filter,
            )?;

            let key = "foo".to_string();
            let val = BlobstoreBytes::from_bytes("foo");

            let (sender, _) = broadcast::channel(1);
            assert_eq!(sender.receiver_count(), 0); // Nothing is waiting here yet

            {
                let mut data = blobstore.inner.blobstore.data.lock().unwrap();
                let mut blob = data.entry(key.clone()).or_default();
                blob.data = Some(BlobData::Channel(sender.clone()));
            }

            // Spawn a bunch of reads
            let futs = tokio::spawn(futures::future::try_join_all(
                (0..10usize).map(|_| blobstore.get(ctx.clone(), key.clone())),
            ));

            tokio::time::timeout(Duration::from_millis(TIMEOUT_MS), async {
                // Wait for the first request to arrive. It'll be alone, since at this point we don't
                // know this is not cacheable.
                loop {
                    tokio::task::yield_now().await;
                    let count = sender.receiver_count();

                    if count > 1 {
                        return Err(anyhow!("Too many receivers: {}", count));
                    }

                    if count > 0 {
                        sender
                            .send(val.clone())
                            .map_err(|_| anyhow!("First send failed"))?;

                        break;
                    }
                }

                // Wait for the next requests to arrive. At this point, we know this is not cacheable,
                // and they should all arrive concurrently.
                loop {
                    tokio::task::yield_now().await;

                    if sender.receiver_count() >= 9 {
                        sender
                            .send(val.clone())
                            .map_err(|_| anyhow!("Second send failed"))?;
                        break;
                    }
                }

                // Now, spawn a bunch more tasks, and check that they all reach the receiver together.
                // Those tasks are a bit different from the ones we had already spawned, since they'll
                // check the cache *before* acquiring the semaphore, and won't ever try to acquire it
                // (whereas the other ones would have acquired it, and been released by the firs task
                // afterwards).
                let futs = tokio::spawn(futures::future::try_join_all(
                    (0..10usize).map(|_| blobstore.get(ctx.clone(), key.clone())),
                ));

                // Finally, wait for those requests to arrive.
                loop {
                    tokio::task::yield_now().await;

                    if sender.receiver_count() >= 10 {
                        sender
                            .send(val.clone())
                            .map_err(|_| anyhow!("Third send failed"))?;
                        break;
                    }
                }

                // Check our results
                let res = futs.await??;
                assert_eq!(res.len(), 10);
                for v in res {
                    assert_eq!(v.unwrap().as_bytes(), &val);
                }

                Result::<_, Error>::Ok(())
            })
            .await??;

            // Check our results for the earlier calls.
            let res = futs.await??;
            assert_eq!(res.len(), 10);
            for v in res {
                assert_eq!(v.unwrap().as_bytes(), &val);
            }

            Ok(())
        }

        #[fbinit::test]
        async fn test_dedupe_writes(fb: FacebookInit) -> Result<(), Error> {
            let ctx = CoreContext::test_mock(fb);
            let blobstore = make_blobstore(
                fb,
                TestBlobstore::new(),
                "blobs",
                nonzero!(2usize),
                allow_all_filter,
            )?;

            let key = "foo".to_string();
            let val = BlobstoreBytes::from_bytes("foo");

            futures::future::try_join_all(
                (0..10usize).map(|_| blobstore.put(ctx.clone(), key.clone(), val.clone())),
            )
            .await?;

            let handle = {
                let mut data = blobstore.inner.blobstore.data.lock().unwrap();
                let blob = data.entry(key.clone()).or_default();
                assert_eq!(blob.puts, 1);
                blob.data.as_ref().unwrap().handle()
            };
            assert_eq!(handle.bytes().await?, val);

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

        #[fbinit::test]
        async fn test_dedupe_writes_different_data(fb: FacebookInit) -> Result<(), Error> {
            let ctx = CoreContext::test_mock(fb);
            let blobstore = make_blobstore(
                fb,
                TestBlobstore::new(),
                "blobs",
                nonzero!(2usize),
                allow_all_filter,
            )?;

            let key = "foo".to_string();
            let val0 = BlobstoreBytes::from_bytes("foo");
            let val1 = BlobstoreBytes::from_bytes("foo");
            let val2 = BlobstoreBytes::from_bytes("bar");

            // First, populate the presence cache with some get data.

            {
                let mut data = blobstore.inner.blobstore.data.lock().unwrap();
                let mut blob = data.entry(key.clone()).or_default();
                blob.data = Some(BlobData::Bytes(val0.clone()));
            }

            blobstore.get(ctx.clone(), key.clone()).await?;

            // Now, check that a put still goes through.

            blobstore
                .put(ctx.clone(), key.clone(), val1.clone())
                .await?;

            let handle1 = {
                let mut data = blobstore.inner.blobstore.data.lock().unwrap();
                let blob = data.entry(key.clone()).or_default();
                assert_eq!(blob.puts, 1);
                blob.data.as_ref().unwrap().handle()
            };
            assert_eq!(handle1.bytes().await?, val1);

            // Put it again. This time, we don't expect a put to make it through to the blobstore.
            blobstore
                .put(ctx.clone(), key.clone(), val1.clone())
                .await?;

            {
                let mut data = blobstore.inner.blobstore.data.lock().unwrap();
                let blob = data.entry(key.clone()).or_default();
                assert_eq!(blob.puts, 1);
            };

            // Now, check that a put for different data also goes through.

            blobstore
                .put(ctx.clone(), key.clone(), val2.clone())
                .await?;

            let handle2 = {
                let mut data = blobstore.inner.blobstore.data.lock().unwrap();
                let blob = data.entry(key.clone()).or_default();
                assert_eq!(blob.puts, 2);
                blob.data.as_ref().unwrap().handle()
            };
            assert_eq!(handle2.bytes().await?, val2);

            // Finally, "evict" all the cached data by creating a new blobstore with a new blob
            // pool (but with the same presence pool), and check that a get doesn't overwrite the
            // put state after a get.

            let blobstore = make_blobstore(
                fb,
                TestBlobstore::new(),
                "blobs2",
                nonzero!(2usize),
                allow_all_filter,
            )?;

            {
                let mut data = blobstore.inner.blobstore.data.lock().unwrap();
                let mut blob = data.entry(key.clone()).or_default();
                blob.data = Some(BlobData::Bytes(val0.clone()));
            };

            blobstore.get(ctx.clone(), key.clone()).await?;

            blobstore
                .put(ctx.clone(), key.clone(), val2.clone())
                .await?;

            {
                let mut data = blobstore.inner.blobstore.data.lock().unwrap();
                let blob = data.entry(key.clone()).or_default();
                assert_eq!(blob.gets, 1);
                assert_eq!(blob.puts, 0);
            };

            Ok(())
        }
    }

    mod ratelimiting {
        use super::*;
        use async_limiter::AsyncLimiter;
        use context::SessionContainer;
        use fbinit::FacebookInit;
        use nonzero_ext::nonzero;
        use ratelimit_meter::{algorithms::LeakyBucket, DirectRateLimiter};
        use scuba_ext::ScubaSampleBuilder;
        use slog::{o, Drain, Level, Logger};
        use slog_glog_fmt::default_drain;
        use std::time::Duration;

        #[derive(Clone, Debug)]
        struct DummyBlob;

        impl DummyBlob {
            fn new() -> Self {
                Self
            }
        }

        impl Blobstore for DummyBlob {
            fn get(
                &self,
                _ctx: CoreContext,
                _key: String,
            ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
                async move {
                    Ok(Some(BlobstoreGetData::new(
                        BlobstoreMetadata::new(None),
                        BlobstoreBytes::from_bytes("foo"),
                    )))
                }
                .boxed()
            }

            fn put(
                &self,
                _ctx: CoreContext,
                _key: String,
                _value: BlobstoreBytes,
            ) -> BoxFuture<'static, Result<(), Error>> {
                async move { Ok(()) }.boxed()
            }

            fn is_present(
                &self,
                _ctx: CoreContext,
                _key: String,
            ) -> BoxFuture<'static, Result<bool, Error>> {
                async move { Ok(true) }.boxed()
            }
        }

        fn logger() -> Logger {
            let drain = default_drain().filter_level(Level::Debug).ignore_res();
            Logger::root(drain, o![])
        }

        #[fbinit::test]
        async fn test_qps(fb: FacebookInit) -> Result<(), Error> {
            let l1 =
                DirectRateLimiter::<LeakyBucket>::new(nonzero!(1u32), Duration::from_millis(10));
            let l1 = AsyncLimiter::new(l1).await;

            let l2 =
                DirectRateLimiter::<LeakyBucket>::new(nonzero!(1u32), Duration::from_millis(10));
            let l2 = AsyncLimiter::new(l2).await;

            let mut builder = SessionContainer::builder(fb);
            builder.blobstore_read_limiter(l1);
            builder.blobstore_write_limiter(l2);
            let session = builder.build();
            let ctx = session.new_context(logger(), ScubaSampleBuilder::with_discard());

            let blobstore = make_blobstore(
                fb,
                DummyBlob::new(),
                "blobs",
                nonzero!(100usize),
                allow_all_filter,
            )?;

            // get
            let (stats, _) = futures::future::try_join_all(
                (0..10u64).map(|i| blobstore.get(ctx.clone(), format!("get{}", i))),
            )
            .try_timed()
            .await?;
            assert!(stats.completion_time.as_millis_unchecked() > 50);

            // is_present
            let (stats, _) = futures::future::try_join_all(
                (0..10u64).map(|i| blobstore.is_present(ctx.clone(), format!("present{}", i))),
            )
            .try_timed()
            .await?;
            assert!(stats.completion_time.as_millis_unchecked() > 50);

            // put
            let bytes = BlobstoreBytes::from_bytes("test foobar");
            let (stats, _) = futures::future::try_join_all(
                (0..10u64).map(|i| blobstore.put(ctx.clone(), format!("put{}", i), bytes.clone())),
            )
            .try_timed()
            .await?;
            assert!(stats.completion_time.as_millis_unchecked() > 50);

            Ok(())
        }

        #[fbinit::test]
        async fn test_early_cache_hits_do_not_count(fb: FacebookInit) -> Result<(), Error> {
            let l1 =
                DirectRateLimiter::<LeakyBucket>::new(nonzero!(10u32), Duration::from_millis(100));
            let l1 = AsyncLimiter::new(l1).await;

            let l2 =
                DirectRateLimiter::<LeakyBucket>::new(nonzero!(10u32), Duration::from_millis(100));
            let l2 = AsyncLimiter::new(l2).await;

            let mut builder = SessionContainer::builder(fb);
            builder.blobstore_read_limiter(l1);
            builder.blobstore_write_limiter(l2);
            let session = builder.build();
            let ctx = &session.new_context(logger(), ScubaSampleBuilder::with_discard());

            let blobstore = &make_blobstore(
                fb,
                DummyBlob::new(),
                "blobs",
                nonzero!(100usize),
                allow_all_filter,
            )?;

            // get
            let (stats, _) = futures::future::try_join_all(
                (0..10u64)
                    .map(|i| {
                        (0..10u64).map(move |_| blobstore.get(ctx.clone(), format!("get{}", i)))
                    })
                    .flatten(),
            )
            .try_timed()
            .await?;
            assert!(stats.completion_time.as_millis_unchecked() <= 100);

            // put
            let bytes = &BlobstoreBytes::from_bytes("test foobar");
            let (stats, _) = futures::future::try_join_all(
                (0..10u64)
                    .map(|i| {
                        (0..10u64).map(move |_| {
                            blobstore.put(ctx.clone(), format!("put{}", i), bytes.clone())
                        })
                    })
                    .flatten(),
            )
            .try_timed()
            .await?;
            assert!(stats.completion_time.as_millis_unchecked() <= 100);

            Ok(())
        }
    }
}
