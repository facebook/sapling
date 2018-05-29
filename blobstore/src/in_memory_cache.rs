// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use failure::Error;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};

use asyncmemo::{Asyncmemo, Filler};
use mononoke_types::BlobstoreBytes;

use Blobstore;

/// A caching layer over an existing blobstore, backed by an in-memory cache layer
pub struct CachingBlobstore {
    cache: Asyncmemo<BlobstoreCacheFiller>,
    blobstore: Arc<Blobstore>,
}

impl CachingBlobstore {
    pub fn new(blobstore: Arc<Blobstore>, entries_limit: usize, bytes_limit: usize) -> Self {
        let filler = BlobstoreCacheFiller::new(blobstore.clone());
        let cache = Asyncmemo::with_limits(filler, entries_limit, bytes_limit);
        CachingBlobstore { cache, blobstore }
    }
}

impl Blobstore for CachingBlobstore {
    fn get(&self, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        self.cache
            .get(key)
            .then(|val| match val {
                Ok(val) => Ok(Some(val)),
                Err(Some(err)) => Err(err),
                Err(None) => Ok(None),
            })
            .boxify()
    }

    fn put(&self, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        self.blobstore.put(key, value)
    }
}

struct BlobstoreCacheFiller {
    blobstore: Arc<Blobstore>,
}

impl BlobstoreCacheFiller {
    fn new(blobstore: Arc<Blobstore>) -> Self {
        Self { blobstore }
    }
}

impl Filler for BlobstoreCacheFiller {
    type Key = String;
    type Value = BoxFuture<BlobstoreBytes, Option<Error>>;

    fn fill(&self, _cache: &Asyncmemo<Self>, key: &Self::Key) -> Self::Value {
        // Asyncmemo Fillers use the error return for two purposes:
        // 1. Some(Error) means that a real error happened that should be returned to the user.
        // 2. None means that no error happened, but that the underlying store returned None.
        // This allows Asyncmemo to cache the value returned if there is one, but to not cache
        // a None return from the store, so that we will keep requerying the underlying store
        // until we get a result back.
        // So, we return one of Ok(val), Err(None), or Err(Some(err)) to Asyncmemo.
        // The caller of `get` above will receive Ok(Some(val)), Ok(None), or Err(err) respectively
        self.blobstore
            .get(key.clone())
            .map_err(|err| Some(err))
            .and_then(|res| match res {
                Some(val) => Ok(val),
                None => Err(None),
            })
            .boxify()
    }
}
