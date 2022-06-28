/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use cloned::cloned;
use fbinit::FacebookInit;
use fbthrift::compact_protocol;
use futures::future::select;
use futures::future::BoxFuture;
use futures::future::Either;
use memcache::KeyGen;
use memcache::MemcacheClient;
use memcache_lock_thrift::LockState;
use slog::warn;

use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::CountedBlobstore;
use context::CoreContext;
use context::PerfCounterType;
use hostname::get_hostname;
use stats::prelude::*;
use tunables::tunables;

use crate::dummy::DummyLease;
use crate::CacheBlobstore;
use crate::CacheOps;
use crate::LeaseOps;

define_stats! {
    prefix = "mononoke.blobstore.memcache";
    blob_put: timeseries("blob_put"; Rate, Sum),
    blob_put_err: timeseries("blob_put_err"; Rate, Sum),
    presence_put: timeseries("presence_put"; Rate, Sum),
    presence_put_err: timeseries("presence_put_err"; Rate, Sum),
    blob_presence: timeseries("blob_presence"; Rate, Sum),
    blob_presence_hit: timeseries("blob_presence_hit"; Rate, Sum),
    blob_presence_miss: timeseries("blob_presence_miss"; Rate, Sum),
    blob_presence_err: timeseries("blob_presence_err"; Rate, Sum),
    presence_get: timeseries("presence_get"; Rate, Sum),
    presence_check_hit: timeseries("presence_check_hit"; Rate, Sum),
    presence_check_miss: timeseries("presence_check_miss"; Rate, Sum),
    // This can come from leases as well as presence checking.
    presence_err: timeseries("presence_err"; Rate, Sum),
}

#[allow(non_snake_case)]
mod LEASE_STATS {
    use stats::define_stats;
    define_stats! {
        prefix = "mononoke.blobstore.memcache.lease";
        claim: dynamic_timeseries("{}.claim", (lease_type: &'static str); Rate, Sum),
        claim_err: dynamic_timeseries("{}.claim_err", (lease_type: &'static str); Rate, Sum),
        conflict: dynamic_timeseries("{}.conflict", (lease_type: &'static str); Rate, Sum),
        wait_ms: dynamic_timeseries("{}.wait_ms", (lease_type: &'static str); Rate, Sum),
        release: dynamic_timeseries("{}.release", (lease_type: &'static str); Rate, Sum),
        release_good: dynamic_timeseries("{}.release_good", (lease_type: &'static str); Rate, Sum),
        release_held_by_other: dynamic_timeseries("{}.release_held_by_other", (lease_type: &'static str); Rate, Sum),
        release_bad_key: dynamic_timeseries("{}.release_bad_key", (lease_type: &'static str); Rate, Sum),
        release_key_set: dynamic_timeseries("{}.release_key_set", (lease_type: &'static str); Rate, Sum),
        release_no_lease: dynamic_timeseries("{}.release_no_lease", (lease_type: &'static str); Rate, Sum),
    }
    pub use self::STATS::*;
}

/// A caching layer over an existing blobstore, backed by memcache
#[derive(Clone, Debug)]
pub struct MemcacheOps {
    lease_type: &'static str,
    memcache: MemcacheClient,
    keygen: KeyGen,
    presence_keygen: KeyGen,
    hostname: String,
}

impl std::fmt::Display for MemcacheOps {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "MemcacheOps")
    }
}

const MEMCACHE_MAX_SIZE: usize = 1024000;
const MC_CODEVER: u32 = 0;
const MC_SITEVER: u32 = 1;

async fn mc_raw_put(
    memcache: MemcacheClient,
    orig_key: String,
    key: String,
    value: BlobstoreGetData,
    presence_key: String,
) {
    let uploaded = compact_protocol::serialize(&LockState::uploaded_key(orig_key));

    STATS::presence_put.add_value(1);
    // This cache key is read by leases, and if it's set then lease can't be reacquired.
    // To be on the safe side let's add a ttl on this memcache key.
    let lock_ttl = Duration::from_secs(50);
    let res = memcache
        .set_with_ttl(presence_key, uploaded, lock_ttl)
        .await;
    if res.is_err() {
        STATS::presence_put_err.add_value(1);
    }

    if value.as_bytes().len() < MEMCACHE_MAX_SIZE {
        STATS::blob_put.add_value(1);
        let res = memcache.set(key, value.into_raw_bytes()).await;
        if res.is_err() {
            STATS::blob_put_err.add_value(1);
        }
    }
}

impl MemcacheOps {
    pub fn new(
        fb: FacebookInit,
        lease_type: &'static str,
        backing_store_params: impl ToString,
    ) -> Result<Self> {
        let hostname = get_hostname()?;

        let blob_key = format!(
            "scm.mononoke.blobstore.{}.{}",
            lease_type,
            backing_store_params.to_string()
        );

        let presence_key = format!(
            "scm.mononoke.blobstore.presence.{}.{}",
            lease_type,
            backing_store_params.to_string()
        );

        let sitever = if tunables().get_blobstore_memcache_sitever() > 0 {
            tunables().get_blobstore_memcache_sitever() as u32
        } else {
            MC_SITEVER
        };

        Ok(Self {
            lease_type,
            memcache: MemcacheClient::new(fb)?,
            keygen: KeyGen::new(blob_key, MC_CODEVER, sitever),
            presence_keygen: KeyGen::new(presence_key, MC_CODEVER, sitever),
            hostname,
        })
    }

    async fn get_lock_state(&self, key: String) -> Option<LockState> {
        let mc_key = self.presence_keygen.key(key.clone());
        STATS::presence_get.add_value(1);
        match self.memcache.get(mc_key.clone()).await {
            Ok(opt_blob) => {
                let blob = opt_blob?;
                let state = compact_protocol::deserialize(Vec::from(blob)).ok()?;
                if let LockState::uploaded_key(ref up_key) = state {
                    if key != *up_key {
                        // The lock state is invalid - fix it up by dropping the lock
                        let _ = self.memcache.del(mc_key).await;
                        return None;
                    }
                }
                Some(state)
            }
            Err(_) => {
                STATS::presence_err.add_value(1);
                None
            }
        }
    }
}

pub fn new_memcache_blobstore<T>(
    fb: FacebookInit,
    blobstore: T,
    backing_store_name: &'static str,
    backing_store_params: impl ToString,
) -> Result<CountedBlobstore<CacheBlobstore<MemcacheOps, DummyLease, T>>>
where
    T: Blobstore + Clone,
{
    let cache_ops = MemcacheOps::new(fb, backing_store_name, backing_store_params)?;
    Ok(CountedBlobstore::new(
        "memcache".to_string(),
        CacheBlobstore::new(cache_ops, DummyLease {}, blobstore, true),
    ))
}

pub fn new_memcache_blobstore_no_lease<T>(
    fb: FacebookInit,
    blobstore: T,
    backing_store_name: &'static str,
    backing_store_params: impl ToString,
) -> Result<CountedBlobstore<CacheBlobstore<MemcacheOps, DummyLease, T>>>
where
    T: Blobstore + Clone,
{
    let cache_ops = MemcacheOps::new(fb, backing_store_name, backing_store_params)?;
    Ok(CountedBlobstore::new(
        "memcache".to_string(),
        CacheBlobstore::new(cache_ops, DummyLease {}, blobstore, true),
    ))
}

#[async_trait]
impl CacheOps for MemcacheOps {
    const HIT_COUNTER: Option<PerfCounterType> = Some(PerfCounterType::MemcacheHits);
    const MISS_COUNTER: Option<PerfCounterType> = Some(PerfCounterType::MemcacheMisses);
    const CACHE_NAME: &'static str = "memcache";

    // Turns errors to Ok(None)
    async fn get(&self, key: &str) -> Option<BlobstoreGetData> {
        let mc_key = self.keygen.key(key);
        let buf = self.memcache.get(mc_key).await.ok()??;
        Some(BlobstoreGetData::from_bytes(buf))
    }

    async fn put(&self, key: &str, value: BlobstoreGetData) {
        let mc_key = self.keygen.key(key);
        let presence_key = self.presence_keygen.key(key);
        let orig_key = key.to_string();

        mc_raw_put(self.memcache.clone(), orig_key, mc_key, value, presence_key).await
    }

    async fn check_present(&self, key: &str) -> bool {
        let key = key.to_string();
        match self.get_lock_state(key.clone()).await {
            // get_lock_state will delete the lock and return None if there's a bad
            // uploaded_key
            Some(LockState::uploaded_key(_)) => {
                STATS::presence_check_hit.add_value(1);
                true
            }
            _ => {
                STATS::presence_check_miss.add_value(1);
                let mc_key = self.keygen.key(key);
                STATS::blob_presence.add_value(1);
                let blob_presence = self.memcache.get(mc_key).await;
                match blob_presence {
                    Ok(Some(_)) => STATS::blob_presence_hit.add_value(1),
                    Ok(None) => STATS::blob_presence_miss.add_value(1),
                    Err(_) => STATS::blob_presence_err.add_value(1),
                }
                blob_presence.unwrap_or(None).is_some()
            }
        }
    }
}

#[async_trait]
impl LeaseOps for MemcacheOps {
    async fn try_add_put_lease(&self, key: &str) -> Result<bool> {
        let mc_key = self.presence_keygen.key(key);
        let lockstate = compact_protocol::serialize(&LockState::locked_by(self.hostname.clone()));
        let lock_ttl = Duration::from_secs(10);
        let lease_type = self.lease_type;
        let res = self
            .memcache
            .add_with_ttl(mc_key, lockstate, lock_ttl)
            .await;
        match res {
            Ok(true) => LEASE_STATS::claim.add_value(1, (lease_type,)),
            Ok(false) => LEASE_STATS::conflict.add_value(1, (lease_type,)),
            Err(_) => LEASE_STATS::claim_err.add_value(1, (lease_type,)),
        }
        res
    }

    fn renew_lease_until(&self, ctx: CoreContext, key: &str, mut done: BoxFuture<'static, ()>) {
        let lockstate = compact_protocol::serialize(&LockState::locked_by(self.hostname.clone()));
        let lock_ttl = Duration::from_secs(10);
        let mc_key = self.presence_keygen.key(key);
        let key = key.to_string();
        cloned!(self.memcache);

        let this = self.clone();
        tokio::spawn(async move {
            loop {
                let res = memcache
                    .set_with_ttl(mc_key.clone(), lockstate.clone(), lock_ttl)
                    .await;
                if res.is_err() {
                    warn!(ctx.logger(), "failed to renew lease for {}", mc_key);
                }

                let sleep = tokio::time::sleep(Duration::from_secs(1));
                futures::pin_mut!(sleep);
                let res = select(sleep, done).await;
                match res {
                    Either::Left((_, new_done)) => {
                        done = new_done;
                    }
                    Either::Right(..) => {
                        break;
                    }
                }
            }

            this.release_lease(&key).await;
        });
    }

    async fn wait_for_other_leases(&self, _key: &str) {
        let retry_millis = 200;
        let retry_delay = Duration::from_millis(retry_millis);
        LEASE_STATS::wait_ms.add_value(retry_millis as i64, (self.lease_type,));
        tokio::time::sleep(retry_delay).await;
    }

    async fn release_lease(&self, key: &str) {
        let mc_key = self.presence_keygen.key(key);
        LEASE_STATS::release.add_value(1, (self.lease_type,));
        cloned!(self.memcache, self.hostname, self.lease_type);

        // This future checks the state of the lease, and releases it only
        // if it's locked by us right now.
        let f = async move {
            let bytes = match memcache.get(mc_key.clone()).await {
                Ok(Some(bytes)) => bytes,
                Ok(None) => {
                    LEASE_STATS::release_no_lease.add_value(1, (lease_type,));
                    return;
                }
                Err(_) => return,
            };

            let state: LockState = match compact_protocol::deserialize(Vec::from(bytes)) {
                Ok(state) => state,
                Err(_) => {
                    LEASE_STATS::release_bad_key.add_value(1, (lease_type,));
                    // Fix up invalid value
                    let _ = memcache.del(mc_key).await;
                    return;
                }
            };

            match state {
                LockState::locked_by(locked_by) => {
                    if locked_by == hostname {
                        LEASE_STATS::release_good.add_value(1, (lease_type,));
                        // The lease is held by us - just remove it
                        let _ = memcache.del(mc_key).await;
                    } else {
                        LEASE_STATS::release_held_by_other.add_value(1, (lease_type,));
                        // Someone else grabbed a lease, leave it alone
                    }
                }
                LockState::uploaded_key(up_key) => {
                    if up_key != mc_key {
                        LEASE_STATS::release_bad_key.add_value(1, (lease_type,));
                        // Invalid key - fix it up. Normally that shouldn't
                        // ever occur
                        let _ = memcache.del(mc_key).await;
                    } else {
                        LEASE_STATS::release_key_set.add_value(1, (lease_type,));
                        // Key is valid, and is most likely set by
                        // cache.put(...). Lease is already release,
                        // no need to do anything here
                    }
                }
                LockState::UnknownField(_) => {
                    LEASE_STATS::release_bad_key.add_value(1, (lease_type,));
                    // Possibly a newer version of the server enabled it?
                    // Don't touch it just in case
                }
            }
        };
        // We don't have to wait for the releasing to finish, it can be done in background
        // because leases have a timeout. So even if they haven't been released explicitly they
        // will be released after a timeout.
        tokio::spawn(f);
    }
}
