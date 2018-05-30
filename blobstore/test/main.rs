// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Tests run against all blobstore implementations.

#![deny(warnings)]
#![feature(never_type)]

extern crate bytes;
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate tempdir;

extern crate blobstore;
extern crate fileblob;
extern crate memblob;
extern crate mononoke_types;
extern crate rocksblob;

use bytes::Bytes;
use futures::Future;
use tempdir::TempDir;

use blobstore::Blobstore;
use fileblob::Fileblob;
use memblob::EagerMemblob;
use mononoke_types::BlobstoreBytes;
use rocksblob::Rocksblob;

fn simple<B>(blobstore: B)
where
    B: Blobstore,
{
    let foo = "foo".to_string();
    let res = blobstore
        .put(foo.clone(), BlobstoreBytes::from_bytes(&b"bar"[..]))
        .and_then(|_| blobstore.get(foo));
    let out = res.wait().expect("pub/get failed").expect("missing");

    assert_eq!(out.into_bytes(), Bytes::from_static(b"bar"));
}

fn missing<B>(blobstore: B)
where
    B: Blobstore,
{
    let res = blobstore.get("missing".to_string());
    let out = res.wait().expect("get failed");

    assert!(out.is_none());
}

fn boxable<B>(blobstore: B)
where
    B: Blobstore,
{
    let blobstore = Box::new(blobstore);

    let foo = "foo".to_string();
    let res = blobstore
        .put(foo.clone(), BlobstoreBytes::from_bytes(&b"bar"[..]))
        .and_then(|_| blobstore.get(foo));
    let out: BlobstoreBytes = res.wait().expect("pub/get failed").expect("missing");

    assert_eq!(out.into_bytes(), Bytes::from_static(b"bar"));
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
        new: |_| EagerMemblob::new(),
        persistent: false,
    }
}

blobstore_test_impl! {
    fileblob_test => {
        state: TempDir::new("fileblob_test").unwrap(),
        new: |dir| Fileblob::open(dir).unwrap(),
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
