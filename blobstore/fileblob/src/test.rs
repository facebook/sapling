// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::Future;
use tempdir::TempDir;

use super::*;

#[test]
fn simple() {
    let dir = TempDir::new("files").expect("tempdir failed");

    let blobstore = Fileblob::create(&dir).expect("fileblob new failed");

    let res = blobstore
        .put("foo", b"bar")
        .and_then(|_| blobstore.get(&"foo"));
    let out = res.wait().expect("pub/get failed").expect("missing");

    assert!(dir.path().join("blob-foo").is_file());

    assert_eq!(&*out, b"bar".as_ref());
}

#[test]
fn missing() {
    let dir = TempDir::new("files").expect("tempdir failed");

    let blobstore = Fileblob::<_, Vec<u8>>::create(&dir).expect("fileblob new failed");

    let res = blobstore.get(&"missing");
    let out = res.wait().expect("get failed");

    assert!(out.is_none());
}


#[test]
fn boxable() {
    let dir = TempDir::new("files").expect("tempdir failed");

    let blobstore = Fileblob::<_, Vec<u8>>::create(&dir).expect("fileblob new failed");

    let blobstore = blobstore.boxed::<_, _, Error>();

    let res = blobstore
        .put("foo", b"bar".as_ref())
        .and_then(|_| blobstore.get(&"foo"));
    let out: Vec<u8> = res.wait().expect("pub/get failed").expect("missing");
    let out = Vec::from(out);

    assert!(dir.path().join("blob-foo").is_file());

    assert_eq!(&*out, b"bar".as_ref());
}
