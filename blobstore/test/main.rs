/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Tests run against all blobstore implementations.

#![deny(warnings)]
#![feature(never_type)]

use std::sync::Arc;

use bytes::Bytes;
use failure_ext::Error;
use fbinit::FacebookInit;
use futures::Future;
use tempdir::TempDir;
use tokio::{prelude::*, runtime::Runtime};

use blobstore::Blobstore;
use context::CoreContext;
use fileblob::Fileblob;
use memblob::EagerMemblob;
use mononoke_types::BlobstoreBytes;
use rocksblob::Rocksblob;

fn simple<B>(fb: FacebookInit, blobstore: B)
where
    B: IntoFuture,
    B::Item: Blobstore,
    B::Future: Send + 'static,
    Error: From<B::Error>,
{
    let ctx = CoreContext::test_mock(fb);
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

fn missing<B>(fb: FacebookInit, blobstore: B)
where
    B: IntoFuture,
    B::Item: Blobstore,
    B::Future: Send + 'static,
    Error: From<B::Error>,
{
    let ctx = CoreContext::test_mock(fb);
    let blobstore = blobstore.into_future().map_err(|err| err.into());

    let fut = future::lazy(move || {
        blobstore.and_then(|blobstore| blobstore.get(ctx, "missing".to_string()))
    });

    let mut runtime = Runtime::new().expect("runtime creation failed");
    let out = runtime.block_on(fut).expect("get failed");

    assert!(out.is_none());
}

fn boxable<B>(fb: FacebookInit, blobstore: B)
where
    B: IntoFuture,
    B::Item: Blobstore,
    B::Future: Send + 'static,
    Error: From<B::Error>,
{
    let ctx = CoreContext::test_mock(fb);
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

            #[fbinit::test]
            fn test_simple(fb: FacebookInit) {
                let state = $state;
                simple(fb, $new_cb(state.clone()));
            }

            #[fbinit::test]
            fn test_missing(fb: FacebookInit) {
                let state = $state;
                missing(fb, $new_cb(state.clone()));
            }

            #[fbinit::test]
            fn test_boxable(fb: FacebookInit) {
                let state = $state;
                boxable(fb, $new_cb(state.clone()));
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
