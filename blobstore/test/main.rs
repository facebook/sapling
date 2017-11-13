// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Tests run against all blobstore implementations.

#![deny(warnings)]
#![feature(never_type)]

#[macro_use]
extern crate error_chain;
extern crate futures;
extern crate futures_ext;
extern crate tempdir;
extern crate tokio_core;

extern crate blobstore;
extern crate fileblob;
extern crate memblob;
extern crate rocksblob;

use futures::Future;
use tempdir::TempDir;

use blobstore::Blobstore;
use fileblob::Fileblob;
use memblob::Memblob;
use rocksblob::Rocksblob;

mod errors {
    error_chain! {
        links {
            Fileblob(::fileblob::Error, ::fileblob::ErrorKind);
            Rocksblob(::rocksblob::Error, ::rocksblob::ErrorKind);
        }
    }

    impl From<!> for Error {
        fn from(_t: !) -> Error {
            unreachable!("! can never be instantiated")
        }
    }
}
pub use errors::*;

fn simple<B>(blobstore: B)
where
    B: Blobstore<Key = String>,
    B::ValueIn: From<&'static [u8]>,
{
    let foo = "foo".to_string();
    let res = blobstore
        .put(foo.clone(), b"bar"[..].into())
        .and_then(|_| blobstore.get(&foo));
    let out = res.wait().expect("pub/get failed").expect("missing");

    assert_eq!(out.as_ref(), b"bar".as_ref());
}

fn missing<B>(blobstore: B)
where
    B: Blobstore<Key = String>,
{
    let res = blobstore.get(&"missing".to_string());
    let out = res.wait().expect("get failed");

    assert!(out.is_none());
}

fn boxable<B>(blobstore: B)
where
    B: Blobstore<Key = String>,
    B::ValueIn: From<&'static [u8]>,
    Vec<u8>: From<B::ValueOut>,
    Error: From<B::Error>,
{
    let blobstore = blobstore.boxed::<_, _, Error>();

    let foo = "foo".to_string();
    let res = blobstore
        .put(foo.clone(), b"bar".as_ref())
        .and_then(|_| blobstore.get(&foo));
    let out: Vec<u8> = res.wait().expect("pub/get failed").expect("missing");

    assert_eq!(&*out, b"bar".as_ref());
}

macro_rules! blobstore_test_impl {
    ($mod_name: ident => {
        state: $state: expr,
        new: $new_cb: expr,
        persistent: $persistent: expr,
    }) => {
        mod $mod_name {
            use super::*;

            #[test]
            fn test_simple() {
                let state = $state;
                simple($new_cb(&state));
            }

            #[test]
            fn test_missing() {
                let state = $state;
                missing($new_cb(&state));
            }

            #[test]
            fn test_boxable() {
                let state = $state;
                boxable($new_cb(&state));
            }
        }
    }
}

blobstore_test_impl! {
    memblob_test => {
        state: (),
        new: |_| Memblob::new(),
        persistent: false,
    }
}

blobstore_test_impl! {
    fileblob_test => {
        state: TempDir::new("fileblob_test").unwrap(),
        new: |dir| Fileblob::<_, Vec<u8>>::open(dir).unwrap(),
        persistent: true,
    }
}

blobstore_test_impl! {
    rocksblob_test => {
        state: TempDir::new("rocksblob_test").unwrap(),
        // create/open may need to be unified once persistence tests are added
        new: |dir| Rocksblob::create(dir).unwrap(),
        persistent: true,
    }
}
