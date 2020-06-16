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
use futures::compat::Future01CompatExt;
use tempdir::TempDir;

use blobstore::Blobstore;
use context::CoreContext;
use fileblob::Fileblob;
use memblob::{EagerMemblob, LazyMemblob};
use mononoke_types::BlobstoreBytes;

async fn round_trip<B: Blobstore>(
    fb: FacebookInit,
    blobstore: B,
    has_ctime: bool,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);

    let key = "foo".to_string();
    let value = BlobstoreBytes::from_bytes(&b"bar"[..]);

    blobstore
        .put(ctx.clone(), key.clone(), value)
        .compat()
        .await?;

    let out = blobstore.get(ctx, key).compat().await?.unwrap();

    assert_eq!(out.clone().into_raw_bytes(), Bytes::from_static(b"bar"));
    assert_eq!(out.as_meta().as_ctime().is_some(), has_ctime);
    Ok(())
}

async fn missing<B: Blobstore>(fb: FacebookInit, blobstore: B) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);

    let key = "missing".to_string();
    let out = blobstore.get(ctx, key).compat().await?;

    assert!(out.is_none());
    Ok(())
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

            #[fbinit::compat_test]
            async fn test_round_trip(fb: FacebookInit) -> Result<(), Error> {
                let state = $state;
                let has_ctime = $has_ctime;
                let factory = $new_cb;
                round_trip(fb, factory(state.clone())?, has_ctime).await
            }

            #[fbinit::compat_test]
            async fn test_missing(fb: FacebookInit) -> Result<(), Error> {
                let state = $state;
                let factory = $new_cb;
                missing(fb, factory(state)?).await
            }

            #[fbinit::compat_test]
            async fn test_boxable(_fb: FacebookInit) -> Result<(), Error> {
                let state = $state;
                let factory = $new_cb;
                // This is really just checking that the constructed type is Sized
                Box::new(factory(state)?);
                Ok(())
            }
        }
    };
}

blobstore_test_impl! {
    eager_memblob_test => {
        state: (),
        new: move |_| Ok::<_,Error>(EagerMemblob::new()),
        persistent: false,
        has_ctime: false,
    }
}

blobstore_test_impl! {
    box_blobstore_test => {
        state: (),
        new: move |_| Ok::<_,Error>(Box::new(EagerMemblob::new())),
        persistent: false,
        has_ctime: false,
    }
}

blobstore_test_impl! {
    lazy_memblob_test => {
        state: (),
        new: move |_| Ok::<_,Error>(LazyMemblob::new()),
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
