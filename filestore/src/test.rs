// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use super::*;
use mononoke_types::typed_hash;

fn storekey(data: impl AsRef<[u8]>) -> StoreKey {
    let data = data.as_ref();

    let mut ctx = typed_hash::ContentIdContext::new();
    ctx.update(data);
    StoreKey {
        total_size: data.len() as u64,
        canonical: ctx.finish(),
        sha1: None,
        git_sha1: None,
        sha256: None,
    }
}

#[test]
fn filestore_put_alias() {
    let mut rt = tokio::runtime::Runtime::new().unwrap();

    let data = &b"hello, world"[..];
    let key = storekey(data);

    let res = rt.block_on(future::lazy({
        cloned!(key);
        move || {
            let blob = memblob::LazyMemblob::new();
            let filestore = Filestore::new(Arc::new(blob));

            let ctxt = CoreContext::test_mock();

            filestore
                .store(ctxt.clone(), &key, stream::once(Ok(Bytes::from(data))))
                .and_then({
                    cloned!(filestore, ctxt, key);
                    move |()| filestore.get_aliases(ctxt, &FetchKey::Canonical(key.canonical))
                })
        }
    }));

    println!("res = {:#?}", res);

    assert_eq!(
        res.unwrap(),
        Some(ContentMetadata {
            total_size: 12,
            content_id: key.canonical,
            sha1: Some(
                hash::Sha1::from_bytes([
                    0xb7, 0xe2, 0x3e, 0xc2, 0x9a, 0xf2, 0x2b, 0x0b, 0x4e, 0x41, 0xda, 0x31, 0xe8,
                    0x68, 0xd5, 0x72, 0x26, 0x12, 0x1c, 0x84
                ])
                .unwrap()
            ),
            git_sha1: Some(
                hash::GitSha1::from_bytes(
                    [
                        0x8c, 0x01, 0xd8, 0x9a, 0xe0, 0x63, 0x11, 0x83, 0x4e, 0xe4, 0xb1, 0xfa,
                        0xb2, 0xf0, 0x41, 0x4d, 0x35, 0xf0, 0x11, 0x02
                    ],
                    "blob",
                    12
                )
                .unwrap()
            ),
            sha256: Some(
                hash::Sha256::from_bytes([
                    0x09, 0xca, 0x7e, 0x4e, 0xaa, 0x6e, 0x8a, 0xe9, 0xc7, 0xd2, 0x61, 0x16, 0x71,
                    0x29, 0x18, 0x48, 0x83, 0x64, 0x4d, 0x07, 0xdf, 0xba, 0x7c, 0xbf, 0xbc, 0x4c,
                    0x8a, 0x2e, 0x08, 0x36, 0x0d, 0x5b,
                ],)
                .unwrap()
            )
        })
    );
}

#[test]
fn filestore_put_get_canon() {
    let mut rt = tokio::runtime::Runtime::new().unwrap();

    let data = &b"hello, world"[..];
    let key = storekey(data);

    let res = rt.block_on(future::lazy({
        cloned!(key);
        move || {
            let blob = memblob::LazyMemblob::new();
            let filestore = Filestore::new(Arc::new(blob));

            let ctxt = CoreContext::test_mock();

            filestore
                .store(ctxt.clone(), &key, stream::once(Ok(Bytes::from(data))))
                .and_then({
                    cloned!(filestore, ctxt, key);
                    move |()| filestore.fetch(ctxt, &FetchKey::Canonical(key.canonical))
                })
                .map(|maybe_str| maybe_str.map(|s| s.concat2()))
                .flatten()
        }
    }));

    println!("res = {:#?}", res);

    assert_eq!(res.unwrap(), Some(Bytes::from(data)));
}

#[test]
fn filestore_put_get_sha1() {
    let mut rt = tokio::runtime::Runtime::new().unwrap();

    let data = &b"hello, world"[..];
    let key = storekey(data);

    let res = rt.block_on(future::lazy({
        cloned!(key);
        move || {
            let blob = memblob::LazyMemblob::new();
            let filestore = Filestore::new(Arc::new(blob));

            let ctxt = CoreContext::test_mock();

            filestore
                .store(ctxt.clone(), &key, stream::once(Ok(Bytes::from(data))))
                .and_then({
                    cloned!(filestore, ctxt);
                    move |()| {
                        filestore.fetch(
                            ctxt,
                            &FetchKey::Sha1(
                                hash::Sha1::from_bytes([
                                    0xb7, 0xe2, 0x3e, 0xc2, 0x9a, 0xf2, 0x2b, 0x0b, 0x4e, 0x41,
                                    0xda, 0x31, 0xe8, 0x68, 0xd5, 0x72, 0x26, 0x12, 0x1c, 0x84,
                                ])
                                .unwrap(),
                            ),
                        )
                    }
                })
                .map(|maybe_str| maybe_str.map(|s| s.concat2()))
                .flatten()
        }
    }));

    println!("res = {:#?}", res);

    assert_eq!(res.unwrap(), Some(Bytes::from(data)));
}

#[test]
fn filestore_put_get_git_sha1() {
    let mut rt = tokio::runtime::Runtime::new().unwrap();

    let data = &b"hello, world"[..];
    let key = storekey(data);

    let res = rt.block_on(future::lazy({
        cloned!(key);
        move || {
            let blob = memblob::LazyMemblob::new();
            let filestore = Filestore::new(Arc::new(blob));

            let ctxt = CoreContext::test_mock();

            filestore
                .store(ctxt.clone(), &key, stream::once(Ok(Bytes::from(data))))
                .and_then({
                    cloned!(filestore, ctxt);
                    move |()| {
                        filestore.fetch(
                            ctxt,
                            &FetchKey::GitSha1(
                                hash::GitSha1::from_bytes(
                                    [
                                        0x8c, 0x01, 0xd8, 0x9a, 0xe0, 0x63, 0x11, 0x83, 0x4e, 0xe4,
                                        0xb1, 0xfa, 0xb2, 0xf0, 0x41, 0x4d, 0x35, 0xf0, 0x11, 0x02,
                                    ],
                                    "blob",
                                    12,
                                )
                                .unwrap(),
                            ),
                        )
                    }
                })
                .map(|maybe_str| maybe_str.map(|s| s.concat2()))
                .flatten()
        }
    }));

    println!("res = {:#?}", res);

    assert_eq!(res.unwrap(), Some(Bytes::from(data)));
}

#[test]
fn filestore_put_get_sha256() {
    let mut rt = tokio::runtime::Runtime::new().unwrap();

    let data = &b"hello, world"[..];
    let key = storekey(data);

    let res = rt.block_on(future::lazy({
        cloned!(key);
        move || {
            let blob = memblob::LazyMemblob::new();
            let filestore = Filestore::new(Arc::new(blob));

            let ctxt = CoreContext::test_mock();

            filestore
                .store(ctxt.clone(), &key, stream::once(Ok(Bytes::from(data))))
                .and_then({
                    cloned!(filestore, ctxt);
                    move |()| {
                        filestore.fetch(
                            ctxt,
                            &FetchKey::Sha256(
                                hash::Sha256::from_bytes([
                                    0x09, 0xca, 0x7e, 0x4e, 0xaa, 0x6e, 0x8a, 0xe9, 0xc7, 0xd2,
                                    0x61, 0x16, 0x71, 0x29, 0x18, 0x48, 0x83, 0x64, 0x4d, 0x07,
                                    0xdf, 0xba, 0x7c, 0xbf, 0xbc, 0x4c, 0x8a, 0x2e, 0x08, 0x36,
                                    0x0d, 0x5b,
                                ])
                                .unwrap(),
                            ),
                        )
                    }
                })
                .map(|maybe_str| maybe_str.map(|s| s.concat2()))
                .flatten()
        }
    }));

    println!("res = {:#?}", res);

    assert_eq!(res.unwrap(), Some(Bytes::from(data)));
}
