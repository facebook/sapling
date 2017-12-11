// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type)]

extern crate blobstore;
extern crate bytes;
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use failure::Error;
use futures::future::IntoFuture;
use futures_ext::{BoxFuture, FutureExt};

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
    fn put(&self, key: String, value: Bytes) -> BoxFuture<(), Error> {
        let mut inner = self.hash.lock().expect("lock poison");

        inner.insert(key, value);
        Ok(()).into_future().boxify()
    }

    fn get(&self, key: String) -> BoxFuture<Option<Bytes>, Error> {
        let inner = self.hash.lock().expect("lock poison");

        Ok(inner.get(&key).map(Clone::clone)).into_future().boxify()
    }
}
