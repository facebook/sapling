/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::time::Duration;

use anyhow::Error;
use cloned::cloned;
use fbinit::FacebookInit;
use fbthrift::compact_protocol;
use futures::{
    future::{self, Either},
    Future, IntoFuture,
};
use futures_ext::{BoxFuture, FutureExt};
use memcache::{KeyGen, MemcacheClient};

use blobstore::{Blobstore, CountedBlobstore};
use context::PerfCounterType;
use fbwhoami::FbWhoAmI;
use mononoke_types::BlobstoreBytes;
use stats::prelude::*;

use crate::dummy::DummyLease;
use crate::CacheBlobstore;
use crate::CacheOps;
use crate::LeaseOps;
use memcache_lock_thrift::LockState;

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

const MEMCACHE_MAX_SIZE: usize = 1024000;
const MC_CODEVER: u32 = 0;
const MC_SITEVER: u32 = 0;

fn mc_raw_put(
    memcache: MemcacheClient,
    orig_key: String,
    key: String,
    value: BlobstoreBytes,
    presence_key: String,
) -> impl Future<Item = (), Error = ()> {
    let uploaded = compact_protocol::serialize(&LockState::uploaded_key(orig_key));

    STATS::presence_put.add_value(1);
    // This cache key is read by leases, and if it's set then lease can't be reacquired.
    // To be on the safe side let's add a ttl on this memcache key.
    let lock_ttl = Duration::from_secs(50);
    memcache
        .set_with_ttl(presence_key, uploaded, lock_ttl)
        .then(move |res| {
            if let Err(_) = res {
                STATS::presence_put_err.add_value(1);
            }
            if value.len() < MEMCACHE_MAX_SIZE {
                STATS::blob_put.add_value(1);
                Either::A(memcache.set(key, value.into_bytes()).or_else(|_| {
                    STATS::blob_put_err.add_value(1);
                    Ok(()).into_future()
                }))
            } else {
                Either::B(Ok(()).into_future())
            }
        })
}

impl MemcacheOps {
    pub fn new(
        fb: FacebookInit,
        lease_type: &'static str,
        backing_store_params: impl ToString,
    ) -> Result<Self, Error> {
        let hostname = FbWhoAmI::new()?
            .get_name()
            .ok_or(Error::msg("No hostname in fbwhoami"))?
            .to_string();

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

        Ok(Self {
            lease_type,
            memcache: MemcacheClient::new(fb),
            keygen: KeyGen::new(blob_key, MC_CODEVER, MC_SITEVER),
            presence_keygen: KeyGen::new(presence_key, MC_CODEVER, MC_SITEVER),
            hostname,
        })
    }

    fn get_lock_state(
        &self,
        key: String,
    ) -> impl Future<Item = Option<LockState>, Error = ()> + Send {
        let mc_key = self.presence_keygen.key(key.clone());
        STATS::presence_get.add_value(1);
        self.memcache
            .get(mc_key.clone())
            .and_then({
                let mc = self.memcache.clone();
                move |opt_blob| {
                    let opt_res = opt_blob
                        .and_then(|blob| compact_protocol::deserialize(Vec::from(blob)).ok());

                    if let Some(LockState::uploaded_key(up_key)) = &opt_res {
                        if key != *up_key {
                            // The lock state is invalid - fix it up by dropping the lock
                            return Either::A(mc.del(mc_key).map(|_| None));
                        }
                    }

                    Either::B(Ok(opt_res).into_future())
                }
            })
            .or_else(move |_| {
                STATS::presence_err.add_value(1);
                Ok(None).into_future()
            })
    }
}

pub fn new_memcache_blobstore<T>(
    fb: FacebookInit,
    blobstore: T,
    backing_store_name: &'static str,
    backing_store_params: impl ToString,
) -> Result<CountedBlobstore<CacheBlobstore<MemcacheOps, MemcacheOps, T>>, Error>
where
    T: Blobstore + Clone,
{
    let cache_ops = MemcacheOps::new(fb, backing_store_name, backing_store_params)?;
    Ok(CountedBlobstore::new(
        "memcache".to_string(),
        CacheBlobstore::new(cache_ops.clone(), cache_ops, blobstore),
    ))
}

pub fn new_memcache_blobstore_no_lease<T>(
    fb: FacebookInit,
    blobstore: T,
    backing_store_name: &'static str,
    backing_store_params: impl ToString,
) -> Result<CountedBlobstore<CacheBlobstore<MemcacheOps, DummyLease, T>>, Error>
where
    T: Blobstore + Clone,
{
    let cache_ops = MemcacheOps::new(fb, backing_store_name, backing_store_params)?;
    Ok(CountedBlobstore::new(
        "memcache".to_string(),
        CacheBlobstore::new(cache_ops, DummyLease {}, blobstore),
    ))
}

impl CacheOps for MemcacheOps {
    const HIT_COUNTER: Option<PerfCounterType> = Some(PerfCounterType::MemcacheHits);
    const MISS_COUNTER: Option<PerfCounterType> = Some(PerfCounterType::MemcacheMisses);

    // Turns errors to Ok(None)
    fn get(&self, key: &str) -> BoxFuture<Option<BlobstoreBytes>, ()> {
        let mc_key = self.keygen.key(key);
        self.memcache
            .get(mc_key)
            .map(|buf| buf.map(|buf| BlobstoreBytes::from_bytes(buf)))
            .boxify()
    }

    fn put(&self, key: &str, value: BlobstoreBytes) -> BoxFuture<(), ()> {
        let mc_key = self.keygen.key(key);
        let presence_key = self.presence_keygen.key(key);
        let orig_key = key.to_string();

        mc_raw_put(self.memcache.clone(), orig_key, mc_key, value, presence_key).boxify()
    }

    fn check_present(&self, key: &str) -> BoxFuture<bool, ()> {
        let lock_presence = self.get_lock_state(key.to_string()).map({
            move |lockstate| match lockstate {
                // get_lock_state will delete the lock and return None if there's a bad
                // uploaded_key
                Some(LockState::uploaded_key(_)) => {
                    STATS::presence_check_hit.add_value(1);
                    true
                }
                _ => {
                    STATS::presence_check_miss.add_value(1);
                    false
                }
            }
        });

        let mc_key = self.keygen.key(key);
        STATS::blob_presence.add_value(1);
        let blob_presence = self
            .memcache
            .get(mc_key)
            .map(|blob| blob.is_some())
            .then(move |res| {
                match res {
                    Ok(true) => STATS::blob_presence_hit.add_value(1),
                    Ok(false) => STATS::blob_presence_miss.add_value(1),
                    Err(_) => STATS::blob_presence_err.add_value(1),
                };
                res
            });

        lock_presence
            .and_then(move |present| {
                if present {
                    Either::A(Ok(true).into_future())
                } else {
                    Either::B(blob_presence)
                }
            })
            .boxify()
    }
}

impl LeaseOps for MemcacheOps {
    fn try_add_put_lease(&self, key: &str) -> BoxFuture<bool, ()> {
        let lockstate = compact_protocol::serialize(&LockState::locked_by(self.hostname.clone()));
        let lock_ttl = Duration::from_secs(10);
        let mc_key = self.presence_keygen.key(key);
        let lease_type = self.lease_type;
        self.memcache
            .add_with_ttl(mc_key, lockstate, lock_ttl)
            .then(move |res| {
                match res {
                    Ok(true) => LEASE_STATS::claim.add_value(1, (lease_type,)),
                    Ok(false) => LEASE_STATS::conflict.add_value(1, (lease_type,)),
                    Err(_) => LEASE_STATS::claim_err.add_value(1, (lease_type,)),
                }
                res
            })
            .boxify()
    }

    fn wait_for_other_leases(&self, _key: &str) -> BoxFuture<(), ()> {
        let retry_millis = 200;
        let retry_delay = Duration::from_millis(retry_millis);
        LEASE_STATS::wait_ms.add_value(retry_millis as i64, (self.lease_type,));
        tokio_timer::sleep(retry_delay).map_err(|_| ()).boxify()
    }

    fn release_lease(&self, key: &str) -> BoxFuture<(), ()> {
        let mc_key = self.presence_keygen.key(key);
        LEASE_STATS::release.add_value(1, (self.lease_type,));
        cloned!(self.memcache, self.hostname, self.lease_type);

        // This future checks the state of the lease, and releases it only
        // if it's locked by us right now.
        let f = future::lazy(move || {
            memcache
                .get(mc_key.clone())
                .and_then(move |maybe_data| match maybe_data {
                    Some(bytes) => {
                        let state: Result<LockState, Error> =
                            compact_protocol::deserialize(Vec::from(bytes));
                        match state {
                            Ok(state) => match state {
                                LockState::locked_by(locked_by) => {
                                    if locked_by == hostname {
                                        LEASE_STATS::release_good.add_value(1, (lease_type,));
                                        // The lease is held by us - just remove it
                                        memcache.del(mc_key).left_future()
                                    } else {
                                        LEASE_STATS::release_held_by_other
                                            .add_value(1, (lease_type,));
                                        // Someone else grabbed a lease, leave it alone
                                        future::ok(()).right_future()
                                    }
                                }
                                LockState::uploaded_key(up_key) => {
                                    if up_key != mc_key {
                                        LEASE_STATS::release_bad_key.add_value(1, (lease_type,));
                                        // Invalid key - fix it up. Normally that shouldn't
                                        // ever occur
                                        memcache.del(mc_key).left_future()
                                    } else {
                                        LEASE_STATS::release_key_set.add_value(1, (lease_type,));
                                        // Key is valid, and is most likely set by
                                        // cache.put(...). Lease is already release,
                                        // no need to do anything here
                                        future::ok(()).right_future()
                                    }
                                }
                                LockState::UnknownField(_) => {
                                    LEASE_STATS::release_bad_key.add_value(1, (lease_type,));
                                    // Possibly a newer version of the server enabled it?
                                    // Don't touch it just in case
                                    future::ok(()).right_future()
                                }
                            },
                            Err(_) => {
                                LEASE_STATS::release_bad_key.add_value(1, (lease_type,));
                                // Fix up invalid value
                                memcache.del(mc_key).left_future()
                            }
                        }
                    }
                    None => {
                        LEASE_STATS::release_no_lease.add_value(1, (lease_type,));
                        future::ok(()).right_future()
                    }
                })
        });
        // We don't have to wait for the releasing to finish, it can be done in background
        // because leases have a timeout. So even if they haven't been released explicitly they
        // will be released after a timeout.
        tokio::spawn(f);
        future::ok(()).boxify()
    }
}
