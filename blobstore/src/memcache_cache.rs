// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::time::Duration;

use failure::{err_msg, Error};
use futures::{future, Future, IntoFuture, future::Either};
use futures_ext::{BoxFuture, FutureExt};
use memcache::{KeyGen, MemcacheClient};
use rust_thrift::compact_protocol;
use tokio;
use tokio_timer::Timer;

use fbwhoami::FbWhoAmI;
use mononoke_types::BlobstoreBytes;

use Blobstore;
use CountedBlobstore;
use memcache_lock_thrift::LockState;

/// A caching layer over an existing blobstore, backed by memcache
#[derive(Clone)]
pub struct MemcacheBlobstore<T: Blobstore + Clone> {
    blobstore: T,
    memcache: MemcacheClient,
    timer: Timer,
    keygen: KeyGen,
    presence_keygen: KeyGen,
    hostname: String,
}

/// Extra operations that can be performed on memcache. Other wrappers can implement this trait for
/// e.g. all `WrapperBlobstore<MemcacheBlobstore<T>>`.
///
/// This is primarily used by the admin command to manually check memcache.
pub trait MemcacheBlobstoreExt: Blobstore + Clone {
    fn get_no_cache_fill(&self, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error>;
    fn get_memcache_only(&self, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error>;
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
) -> impl Future<Item = (), Error = Error> {
    let uploaded = compact_protocol::serialize(&LockState::uploaded_key(orig_key));

    memcache.set(presence_key, uploaded).then(move |_| {
        if value.len() < MEMCACHE_MAX_SIZE {
            Either::A(
                memcache
                    .set(key, value.into_bytes())
                    .or_else(|_| Ok(()).into_future()),
            )
        } else {
            Either::B(Ok(()).into_future())
        }
    })
}

impl<T: Blobstore + Clone> MemcacheBlobstore<T> {
    pub fn new<S>(
        blobstore: T,
        backing_store_name: S,
        backing_store_params: S,
    ) -> Result<CountedBlobstore<Self>, Error>
    where
        S: AsRef<str>,
    {
        let hostname = FbWhoAmI::new()?
            .get_name()
            .ok_or(err_msg("No hostname in fbwhoami"))?
            .to_string();

        let backing_store_name = backing_store_name.as_ref();
        let blob_key = "scm.mononoke.blobstore.".to_string() + backing_store_name.as_ref() + "."
            + backing_store_params.as_ref();
        let presence_key = "scm.mononoke.blobstore.presence.".to_string()
            + backing_store_name.as_ref() + "."
            + backing_store_params.as_ref();

        Ok(CountedBlobstore::new(
            "memcache",
            MemcacheBlobstore {
                blobstore: blobstore,
                memcache: MemcacheClient::new(),
                timer: Timer::default(),
                keygen: KeyGen::new(blob_key, MC_CODEVER, MC_SITEVER),
                presence_keygen: KeyGen::new(presence_key, MC_CODEVER, MC_SITEVER),
                hostname,
            },
        ))
    }

    // Turns errors to Ok(None)
    fn mc_get(&self, key: &String) -> impl Future<Item = Option<BlobstoreBytes>, Error = Error> {
        let mc_key = self.keygen.key(key);
        self.memcache
            .get(mc_key)
            .map(|buf| buf.map(|buf| BlobstoreBytes::from_bytes(buf)))
            .or_else(|_| Ok(None).into_future())
    }

    fn mc_put(&self, key: &String, value: BlobstoreBytes) -> impl Future<Item = (), Error = Error> {
        let mc_key = self.keygen.key(key);
        let presence_key = self.presence_keygen.key(key);
        let orig_key = key.clone();
        let mc = self.memcache.clone();

        future::lazy(move || mc_raw_put(mc, orig_key, mc_key, value, presence_key))
    }

    fn mc_put_closure(
        &self,
        key: &String,
    ) -> impl FnOnce(Option<BlobstoreBytes>) -> Option<BlobstoreBytes> {
        let mc_key = self.keygen.key(key);
        let presence_key = self.presence_keygen.key(key);
        let orig_key = key.clone();

        let memcache = self.memcache.clone();
        move |value| {
            if let Some(ref value) = value {
                tokio::spawn(
                    mc_raw_put(memcache, orig_key, mc_key, value.clone(), presence_key)
                        .map_err(|_| ()),
                );
            }
            value
        }
    }

    fn mc_get_lock_state(
        &self,
        key: String,
    ) -> impl Future<Item = Option<LockState>, Error = Error> + Send {
        let mc_key = self.presence_keygen.key(key.clone());
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
            .or_else(|_| Ok(None).into_future())
    }

    fn mc_is_present(&self, key: &String) -> impl Future<Item = bool, Error = Error> + Send {
        let lock_presence = self.mc_get_lock_state(key.clone())
            .map(|lockstate| match lockstate {
                // mc_get_lock_state will delete the lock and return None if there's a bad
                // uploaded_key
                Some(LockState::uploaded_key(_)) => true,
                _ => false,
            });

        let blob_presence = self.mc_get(key).map(|blob| blob.is_some());

        lock_presence.and_then(move |present| {
            if present {
                Either::A(Ok(true).into_future())
            } else {
                Either::B(blob_presence)
            }
        })
    }

    fn mc_can_put_to_bs(&self, key: String) -> impl Future<Item = bool, Error = Error> + Send {
        // We can't put if the key is present.
        // Otherwise, we sleep and retry
        self.mc_is_present(&key).and_then({
            let mc = self.memcache.clone();
            let mc_key = self.presence_keygen.key(key.clone());
            let hostname = self.hostname.clone();
            let timer = self.timer.clone();
            let this = self.clone();

            move |present| {
                if present {
                    // It's in the blobstore already
                    Either::A(Ok(false).into_future())
                } else {
                    let lockstate = compact_protocol::serialize(&LockState::locked_by(hostname));
                    let lock_ttl = Duration::from_secs(10);
                    let retry_delay = Duration::from_millis(200);

                    Either::B(
                        mc.add_with_ttl(mc_key, lockstate, lock_ttl)
                            .then(move |locked| {
                                if Ok(true) == locked {
                                    // We own the lock
                                    Either::A(Ok(true).into_future())
                                } else {
                                    // Someone else owns the lock, or memcache failed
                                    Either::B(future::lazy(move || {
                                        timer
                                            .sleep(retry_delay)
                                            .then(move |_| this.mc_can_put_to_bs(key))
                                            .boxify()
                                    }))
                                }
                            }),
                    )
                }
            }
        })
    }

    // The following are used by the admin command to manually check on memcache
    pub fn get_no_cache_fill(&self, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        let mc_get = self.mc_get(&key);
        let bs_get = self.blobstore.get(key);

        mc_get
            .and_then(move |blob| {
                if blob.is_some() {
                    Ok(blob).into_future().boxify()
                } else {
                    bs_get.boxify()
                }
            })
            .boxify()
    }

    pub fn get_memcache_only(&self, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        self.mc_get(&key).boxify()
    }
}

impl<T: Blobstore + Clone> Blobstore for MemcacheBlobstore<T> {
    fn get(&self, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        let mc_get = self.mc_get(&key);
        let mc_put = self.mc_put_closure(&key);
        let bs_get = future::lazy({
            let blobstore = self.blobstore.clone();
            move || blobstore.get(key)
        });

        mc_get
            .and_then({
                move |blob| {
                    if blob.is_some() {
                        future::Either::A(Ok(blob).into_future())
                    } else {
                        future::Either::B(bs_get.map(mc_put))
                    }
                }
            })
            .boxify()
    }

    fn put(&self, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        let presence_key = self.presence_keygen.key(key.clone());

        let can_put = self.mc_can_put_to_bs(key.clone());
        let mc_put = self.mc_put(&key, value.clone());
        let bs_put = future::lazy({
            let mc = self.memcache.clone();
            let blobstore = self.blobstore.clone();
            move || {
                blobstore
                    .put(key, value)
                    .or_else(move |r| mc.del(presence_key).then(|_| Err(r)))
            }
        });

        can_put
            .and_then(|can_put| {
                if can_put {
                    Either::A(bs_put.and_then(move |_| mc_put))
                } else {
                    Either::B(Ok(()).into_future())
                }
            })
            .boxify()
    }

    fn is_present(&self, key: String) -> BoxFuture<bool, Error> {
        let mc_check = self.mc_is_present(&key);
        let bs_check = future::lazy({
            let blobstore = self.blobstore.clone();
            move || blobstore.is_present(key)
        });

        mc_check
            .and_then(|present| {
                if present {
                    Either::A(Ok(true).into_future())
                } else {
                    Either::B(bs_check)
                }
            })
            .boxify()
    }
}

impl<T: Blobstore + Clone> MemcacheBlobstoreExt for MemcacheBlobstore<T> {
    fn get_no_cache_fill(&self, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        let mc_get = self.mc_get(&key);
        let bs_get = self.blobstore.get(key);

        mc_get
            .and_then(move |blob| {
                if blob.is_some() {
                    Ok(blob).into_future().boxify()
                } else {
                    bs_get.boxify()
                }
            })
            .boxify()
    }

    fn get_memcache_only(&self, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        self.mc_get(&key).boxify()
    }
}
