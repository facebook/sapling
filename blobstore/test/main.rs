// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Tests run against all blobstore implementations.

#![deny(warnings)]
#![feature(never_type)]

use std::sync::Arc;

use bytes::Bytes;
use failure_ext::Error;
use futures::Future;
use rand::prelude::*;
use tempdir::TempDir;
use tokio::{prelude::*, runtime::Runtime};

use blobstore::Blobstore;
use context::CoreContext;
use fileblob::Fileblob;
use glusterblob::Glusterblob;
use memblob::EagerMemblob;
use mononoke_types::BlobstoreBytes;
use rocksblob::Rocksblob;

fn simple<B>(blobstore: B)
where
    B: IntoFuture,
    B::Item: Blobstore,
    B::Future: Send + 'static,
    Error: From<B::Error>,
{
    let ctx = CoreContext::test_mock();
    let blobstore = blobstore.into_future().map_err(|err| err.into());

    let foo = "foo".to_string();

    let fut = future::lazy(|| {
        blobstore.and_then(|blobstore| {
            blobstore
                .put(
                    ctx.clone(),
                    foo.clone(),
                    BlobstoreBytes::from_bytes(&b"bar"[..]),
                )
                .and_then(move |_| blobstore.get(ctx, foo))
        })
    });

    let mut runtime = Runtime::new().expect("runtime creation failed");
    let out = runtime
        .block_on(fut)
        .expect("pub/get failed")
        .expect("missing");

    assert_eq!(out.into_bytes(), Bytes::from_static(b"bar"));
}

fn missing<B>(blobstore: B)
where
    B: IntoFuture,
    B::Item: Blobstore,
    B::Future: Send + 'static,
    Error: From<B::Error>,
{
    let ctx = CoreContext::test_mock();
    let blobstore = blobstore.into_future().map_err(|err| err.into());

    let fut = future::lazy(move || {
        blobstore.and_then(|blobstore| blobstore.get(ctx, "missing".to_string()))
    });

    let mut runtime = Runtime::new().expect("runtime creation failed");
    let out = runtime.block_on(fut).expect("get failed");

    assert!(out.is_none());
}

fn boxable<B>(blobstore: B)
where
    B: IntoFuture,
    B::Item: Blobstore,
    B::Future: Send + 'static,
    Error: From<B::Error>,
{
    let ctx = CoreContext::test_mock();
    let blobstore = Box::new(blobstore.into_future().map_err(|err| err.into()));

    let foo = "foo".to_string();

    let fut = future::lazy(|| {
        blobstore.and_then(|blobstore| {
            blobstore
                .put(
                    ctx.clone(),
                    foo.clone(),
                    BlobstoreBytes::from_bytes(&b"bar"[..]),
                )
                .and_then(move |_| blobstore.get(ctx, foo))
        })
    });
    let mut runtime = Runtime::new().expect("runtime creation failed");

    let out: BlobstoreBytes = runtime
        .block_on(fut)
        .expect("pub/get failed")
        .expect("missing");

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
                simple($new_cb(state.clone()));
            }

            #[test]
            fn test_missing() {
                let state = $state;
                missing($new_cb(state.clone()));
            }

            #[test]
            fn test_boxable() {
                let state = $state;
                boxable($new_cb(state.clone()));
            }
        }
    };
}

blobstore_test_impl! {
    memblob_test => {
        state: (),
        new: move |_| Ok::<_,!>(EagerMemblob::new()),
        persistent: false,
    }
}

blobstore_test_impl! {
    fileblob_test => {
        state: Arc::new(TempDir::new("fileblob_test").unwrap()),
        new: move |dir: Arc<TempDir>| Fileblob::open(&*dir),
        persistent: true,
    }
}

blobstore_test_impl! {
    rocksblob_test => {
        state: Arc::new(TempDir::new("rocksblob_test").unwrap()),
        // create/open may need to be unified once persistence tests are added
        new: move |dir: Arc<TempDir>| Rocksblob::create(&*dir),
        persistent: true,
    }
}

const GLUSTER_TIER: &str = "gluster.prod.flash.prn.cell002";
const GLUSTER_EXPORT: &str = "groot";
const GLUSTER_BASEPATH: &str = "mononoke/glusterblob-test";

blobstore_test_impl! {
    glusterblob_test => {
        state: {
            let name = format!("{}-{}", GLUSTER_BASEPATH, random::<u32>());
            println!("glusterblob name {}", name);
            name
        },
        new: |dir| Glusterblob::with_smc(GLUSTER_TIER, GLUSTER_EXPORT, dir),
        persistent: true,
    }
}
