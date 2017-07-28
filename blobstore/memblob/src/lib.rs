// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate blobstore;
#[macro_use]
extern crate error_chain;
extern crate futures;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures::future::{FutureResult, IntoFuture};

use blobstore::Blobstore;

mod errors;
pub use errors::*;

/// In-memory "blob store"
///
/// Pure in-memory implementation for testing.
#[derive(Clone)]
pub struct Memblob {
    hash: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl Memblob {
    pub fn new() -> Self {
        Memblob { hash: Arc::new(Mutex::new(HashMap::new())) }
    }
}

impl Blobstore for Memblob {
    type Key = String;
    type ValueIn = Vec<u8>;
    type ValueOut = Self::ValueIn;
    type Error = Error;
    type PutBlob = FutureResult<(), Self::Error>;
    type GetBlob = FutureResult<Option<Self::ValueOut>, Self::Error>;

    fn put(&self, k: Self::Key, v: Self::ValueIn) -> Self::PutBlob {
        let mut inner = self.hash.lock().expect("lock poison");

        inner.insert(k, v);
        Ok(()).into_future()
    }

    fn get(&self, k: &Self::Key) -> Self::GetBlob {
        let inner = self.hash.lock().expect("lock poison");

        Ok(inner.get(k).map(Clone::clone)).into_future()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::Future;

    #[test]
    fn roundtrip() {
        let mb = Memblob::new();

        let res = mb.put("hello".into(), vec![1, 2, 3, 4, 5]);
        assert!(res.wait().is_ok());

        match mb.get(&"hello".into()).wait() {
            Ok(v) => assert_eq!(v, Some(vec![1, 2, 3, 4, 5])),
            Err(err) => panic!("Unexpected error {:?}", err),
        }
    }

    #[test]
    fn missing() {
        let mb = Memblob::new();

        match mb.get(&"hello".into()).wait() {
            Ok(None) => (),
            Ok(v) => panic!("Unexpected success {:?}", v),
            Err(err) => panic!("Unexpected error {:?}", err),
        }
    }
}
