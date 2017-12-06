// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type)]

extern crate blobstore;
extern crate bytes;
extern crate failure;
extern crate futures;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use failure::Error;
use futures::future::{FutureResult, IntoFuture};

use blobstore::Blobstore;

/// In-memory "blob store"
///
/// Pure in-memory implementation for testing.
#[derive(Clone)]
pub struct Memblob {
    hash: Arc<Mutex<HashMap<String, Bytes>>>,
}

impl Memblob {
    pub fn new() -> Self {
        Memblob {
            hash: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Blobstore for Memblob {
    type PutBlob = FutureResult<(), Error>;
    type GetBlob = FutureResult<Option<Bytes>, Error>;

    fn put(&self, k: String, v: Bytes) -> Self::PutBlob {
        let mut inner = self.hash.lock().expect("lock poison");

        inner.insert(k, v);
        Ok(()).into_future()
    }

    fn get(&self, k: String) -> Self::GetBlob {
        let inner = self.hash.lock().expect("lock poison");

        Ok(inner.get(&k).map(Clone::clone)).into_future()
    }
}
