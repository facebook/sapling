// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure::Error;
use futures::{future, Future, IntoFuture, future::Either};
use futures_ext::{BoxFuture, FutureExt};
use memcache::{KeyGen, MemcacheClient};
use tokio;

use mononoke_types::BlobstoreBytes;

use Blobstore;
use CountedBlobstore;

/// A caching layer over an existing blobstore, backed by memcache
#[derive(Clone)]
pub struct MemcacheBlobstore<T: Blobstore + Clone> {
    blobstore: T,
    memcache: MemcacheClient,
    keygen: KeyGen,
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
    memcache: &MemcacheClient,
    key: String,
    value: BlobstoreBytes,
) -> impl Future<Item = (), Error = Error> {
    if value.len() < MEMCACHE_MAX_SIZE {
        Either::A(
            memcache
                .set(key, value.into_bytes())
                .or_else(|_| Ok(()).into_future()),
        )
    } else {
        Either::B(Ok(()).into_future())
    }
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
        let backing_store_name = backing_store_name.as_ref();
        let blob_key = "scm.mononoke.blobstore.".to_string() + backing_store_name.as_ref() + "."
            + backing_store_params.as_ref();

        Ok(CountedBlobstore::new(
            "memcache",
            MemcacheBlobstore {
                blobstore: blobstore,
                memcache: MemcacheClient::new(),
                keygen: KeyGen::new(blob_key, MC_CODEVER, MC_SITEVER),
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
        let mc_key = self.keygen.key(&key);
        mc_raw_put(&self.memcache, mc_key, value)
    }

    fn mc_put_closure(
        &self,
        key: &String,
    ) -> impl FnOnce(Option<BlobstoreBytes>) -> Option<BlobstoreBytes> {
        let mc_key = self.keygen.key(&key);
        let memcache = self.memcache.clone();
        move |value| {
            if let Some(ref value) = value {
                tokio::spawn(mc_raw_put(&memcache, mc_key, value.clone()).map_err(|_| ()));
            }
            value
        }
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
        let mc_put = self.mc_put(&key, value.clone());
        let bs_put = self.blobstore.put(key, value);

        bs_put.and_then(move |_| mc_put).boxify()
    }

    fn is_present(&self, key: String) -> BoxFuture<bool, Error> {
        let mc_check = self.mc_get(&key).map(|blob| blob.is_some());
        let bs_check = future::lazy({
            let blobstore = self.blobstore.clone();
            move || blobstore.is_present(key)
        });

        mc_check
            .and_then(|blob| {
                if blob {
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
