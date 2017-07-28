// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;
use tempdir::TempDir;

use super::*;
use rocksdb::Buffer;

#[test]
fn simple() {
    let dir = TempDir::new("rocksdb").expect("tempdir failed");

    let blobstore = Rocksblob::create(dir).expect("rocksblob new failed");

    let res = blobstore
        .put("foo", Bytes::from(b"bar".as_ref()))
        .and_then(|_| blobstore.get(&"foo"));
    let out = res.wait().expect("pub/get failed").expect("missing");

    assert_eq!(&*out, b"bar".as_ref());
}

#[test]
fn boxable() {
    let dir = TempDir::new("rocksdb").expect("tempdir failed");

    let blobstore = Rocksblob::create(dir).expect("rocksblob new failed");

    let blobstore = blobstore.boxed::<_, _, Error>();

    let res = blobstore
        .put("foo", Bytes::from(b"bar".as_ref()))
        .and_then(|_| blobstore.get(&"foo"));
    let out: Buffer = res.wait().expect("pub/get failed").expect("missing");
    let out = Vec::from(out);

    assert_eq!(&*out, b"bar".as_ref());
}
