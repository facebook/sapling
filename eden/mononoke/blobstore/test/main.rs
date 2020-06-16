/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tests run against all blobstore implementations.

#![deny(warnings)]
#![feature(never_type)]

use std::sync::Arc;

use anyhow::Error;
use bytes::Bytes;
use fbinit::FacebookInit;
use futures_old::Future as Future01;
use tempdir::TempDir;
use tokio::{prelude::*, runtime::Runtime};

use blobstore::{Blobstore, BlobstoreGetData};
use context::CoreContext;
use fileblob::Fileblob;
use memblob::EagerMemblob;
use mononoke_types::BlobstoreBytes;

fn simple<B>(fb: FacebookInit, blobstore: B, has_ctime: bool)
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

    assert_eq!(out.clone().into_raw_bytes(), Bytes::from_static(b"bar"));
    assert_eq!(out.as_meta().as_ctime().is_some(), has_ctime);
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

    let out: BlobstoreGetData = runtime
        .block_on(fut)
        .expect("pub/get failed")
        .expect("missing");

    assert_eq!(out.into_raw_bytes(), Bytes::from_static(b"bar"));
}

macro_rules! blobstore_test_impl {
    ($mod_name: ident => {
        state: $state: expr,
        new: $new_cb: expr,
        persistent: $persistent: expr,
        has_ctime: $has_ctime: expr,
    }) => {
        mod $mod_name {
            use super::*;

            #[fbinit::test]
            fn test_simple(fb: FacebookInit) {
                let state = $state;
                let has_ctime = $has_ctime;
                simple(fb, $new_cb(state.clone()), has_ctime);
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
        has_ctime: false,
    }
}

blobstore_test_impl! {
    fileblob_test => {
        state: Arc::new(TempDir::new("fileblob_test").unwrap()),
        new: move |dir: Arc<TempDir>| Fileblob::open(&*dir),
        persistent: true,
        has_ctime: true,
    }
}
