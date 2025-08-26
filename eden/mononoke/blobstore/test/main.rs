/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tests run against all blobstore implementations.

#![cfg_attr(not(fbcode_build), allow(unused_crate_dependencies))]
#![feature(never_type)]

use std::sync::Arc;

use anyhow::Error;
use blobstore::Blobstore;
use blobstore::BlobstorePutOps;
use blobstore::BlobstoreUnlinkOps;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use borrowed::borrowed;
use bytes::Bytes;
use context::CoreContext;
use fbinit::FacebookInit;
use fileblob::Fileblob;
use memblob::Memblob;
use mononoke_macros::mononoke;
use mononoke_types::BlobstoreBytes;
use sqlblob::Sqlblob;
use sqlblob::get_test_config_store;
use strum::IntoEnumIterator;
use tempfile::TempDir;

async fn overwrite<B: Blobstore + BlobstorePutOps>(
    fb: FacebookInit,
    blobstore: B,
    has_ctime: bool,
    put_behaviour: PutBehaviour,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx);

    let key = "some_key".to_string() + &put_behaviour.to_string();
    let value = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(b"appleveldatav1"));

    let put_status1 = blobstore
        .put_with_status(ctx, key.clone(), value.clone())
        .await?;
    let expected_status1 = match put_behaviour {
        PutBehaviour::Overwrite => OverwriteStatus::NotChecked,
        PutBehaviour::OverwriteAndLog => OverwriteStatus::New,
        PutBehaviour::IfAbsent => OverwriteStatus::New,
    };
    assert_eq!(
        expected_status1, put_status1,
        "checking new {:?}",
        put_behaviour
    );

    let roundtrip1 = blobstore.get(ctx, &key).await?.unwrap();

    let ctime1 = roundtrip1.as_meta().ctime();

    let value2 = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(b"appleveldatav2"));

    let put_status2 = blobstore
        .put_with_status(ctx, key.clone(), value2.clone())
        .await?;

    let roundtrip2 = blobstore.get(ctx, &key).await?.unwrap();
    let ctime2 = roundtrip2.as_meta().ctime();
    if put_behaviour.should_overwrite() {
        assert_eq!(ctime2.is_some(), has_ctime);
        assert_eq!(
            value2,
            roundtrip2.into_bytes(),
            "checking overwrite value {:?}",
            put_behaviour
        );
    } else {
        assert_eq!(ctime1, ctime2);
        assert_eq!(
            value,
            roundtrip2.into_bytes(),
            "checking overwrite value {:?}",
            put_behaviour
        );
    }

    let expected_status2 = match put_behaviour {
        PutBehaviour::Overwrite => OverwriteStatus::NotChecked,
        PutBehaviour::OverwriteAndLog => OverwriteStatus::Overwrote,
        PutBehaviour::IfAbsent => OverwriteStatus::Prevented,
    };
    assert_eq!(
        expected_status2, put_status2,
        "checking overwrite {:?}",
        put_behaviour
    );

    Ok(())
}

async fn roundtrip_and_link<B: BlobstoreUnlinkOps>(
    fb: FacebookInit,
    blobstore: B,
    has_ctime: bool,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx);

    let key = "randomkey";
    let value = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(b"appleveldata"));

    // Roundtrip
    blobstore.put(ctx, key.to_owned(), value.clone()).await?;

    let roundtrip = blobstore.get(ctx, key).await?.unwrap();

    let orig_ctime = roundtrip.as_meta().ctime();

    assert_eq!(orig_ctime.is_some(), has_ctime);
    assert_eq!(value, roundtrip.into_bytes());

    let newkey = "newkey";

    // And now the link
    blobstore.copy(ctx, key, newkey.to_owned()).await?;

    let newvalue = blobstore.get(ctx, newkey).await?.unwrap();

    let new_ctime = newvalue.as_meta().ctime();
    assert_eq!(new_ctime.is_some(), has_ctime);
    assert_eq!(orig_ctime, new_ctime);
    assert_eq!(value, newvalue.into_bytes());

    let newkey_is_present = blobstore
        .is_present(ctx, newkey)
        .await?
        .assume_not_found_if_unsure();

    assert!(newkey_is_present);

    // Try unlink
    blobstore.unlink(ctx, newkey).await?;
    let newkey_is_present2 = blobstore
        .is_present(ctx, newkey)
        .await?
        .assume_not_found_if_unsure();
    assert!(!newkey_is_present2);

    // Check we get error when unlinking an unknown key
    let unknown_key = "expected_missing_key";
    assert!(blobstore.unlink(ctx, unknown_key).await.is_err());

    Ok(())
}

async fn missing<B: Blobstore>(fb: FacebookInit, blobstore: B) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx);

    let key = "missing";
    let out = blobstore.get(ctx, key).await?;

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

            #[mononoke::fbinit_test]
            async fn test_overwrite(fb: FacebookInit) -> Result<(), Error> {
                let state = $state;
                let has_ctime = $has_ctime;
                let factory = $new_cb;
                for b in PutBehaviour::iter() {
                    overwrite(fb, factory(state.clone(), b)?, has_ctime, b).await?
                }
                Ok(())
            }

            #[mononoke::fbinit_test]
            async fn test_roundtrip_and_link(fb: FacebookInit) -> Result<(), Error> {
                let state = $state;
                let has_ctime = $has_ctime;
                let factory = $new_cb;
                roundtrip_and_link(
                    fb,
                    factory(state.clone(), PutBehaviour::Overwrite)?,
                    has_ctime,
                )
                .await
            }

            #[mononoke::fbinit_test]
            async fn test_missing(fb: FacebookInit) -> Result<(), Error> {
                let state = $state;
                let factory = $new_cb;
                missing(fb, factory(state, PutBehaviour::Overwrite)?).await
            }

            #[mononoke::fbinit_test]
            async fn test_boxable(_fb: FacebookInit) -> Result<(), Error> {
                let state = $state;
                let factory = $new_cb;
                // This is really just checking that the constructed type is Sized
                let _ = Box::new(factory(state, PutBehaviour::Overwrite)?);
                Ok(())
            }
        }
    };
}

blobstore_test_impl! {
    memblob_test => {
        state: (),
        new: move |_, put_behaviour| Ok::<_,Error>(Memblob::new(put_behaviour)),
        persistent: false,
        has_ctime: false,
    }
}

blobstore_test_impl! {
    box_blobstore_test => {
        state: (),
        new: move |_, put_behaviour| Ok::<_,Error>(Box::new(Memblob::new(put_behaviour))),
        persistent: false,
        has_ctime: false,
    }
}

blobstore_test_impl! {
    fileblob_test => {
        state: Arc::new(TempDir::with_prefix("fileblob_test.").unwrap()),
        new: move |dir: Arc<TempDir>, put_behaviour,| Fileblob::open(&*dir, put_behaviour),
        persistent: true,
        has_ctime: true,
    }
}

blobstore_test_impl! {
    sqlblob_test_no_inline => {
        state: (),
        new: move |_, put_behaviour,| Sqlblob::with_sqlite_in_memory(put_behaviour, &(get_test_config_store().1), false, 0),
        persistent: true,
        has_ctime: true,
    }
}

blobstore_test_impl! {
    sqlblob_test_allow_inline => {
        state: (),
        new: move |_, put_behaviour,| Sqlblob::with_sqlite_in_memory(put_behaviour, &(get_test_config_store().1), true, 0),
        persistent: true,
        has_ctime: true,
    }
}

#[cfg(fbcode_build)]
fn create_cache(fb: FacebookInit) -> Result<(), Error> {
    let config = cachelib::LruCacheConfig::new(128 * 1024 * 1024);
    cachelib::init_cache(fb, config)?;
    Ok(())
}

#[cfg(fbcode_build)]
#[mononoke::fbinit_test]
async fn test_cache_blob_maybe_zstd(fb: FacebookInit) -> Result<(), Error> {
    cache_blob_tests(fb, true).await
}

#[cfg(fbcode_build)]
#[mononoke::fbinit_test]
async fn test_cache_blob_no_zstd(fb: FacebookInit) -> Result<(), Error> {
    cache_blob_tests(fb, false).await
}

#[cfg(fbcode_build)]
async fn cache_blob_tests(fb: FacebookInit, expect_zstd: bool) -> Result<(), Error> {
    let options = cacheblob::CachelibBlobstoreOptions::new_eager(Some(expect_zstd));
    let suffix = if expect_zstd { "_maybe_zstd" } else { "_raw" };

    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx);
    create_cache(fb)?;
    let blob_pool = Arc::new(cachelib::get_or_create_pool(
        &["blob_pool", suffix].concat(),
        20 * 1024 * 1024,
    )?);
    let presence_pool = Arc::new(cachelib::get_or_create_pool(
        &["presence_pool", suffix].concat(),
        20 * 1024 * 1024,
    )?);

    let inner = Arc::new(Memblob::new(PutBehaviour::Overwrite));
    let cache_blob =
        cacheblob::new_cachelib_blobstore(inner.clone(), blob_pool.clone(), presence_pool, options);

    let small_key = "small_key";
    let value = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(b"smalldata"));
    cache_blob
        .put(ctx, small_key.to_owned(), value.clone())
        .await?;

    // Peek into cachelib to check its as expected
    let cachelib_len = blob_pool
        .get(small_key)
        .map(|bytes| bytes.map(|b| b.len()))?;
    assert!(cachelib_len.is_some());
    assert!(
        cachelib_len.unwrap() > value.as_bytes().len(),
        "Expected cachelib value to be larger due to framing"
    );

    assert_eq!(
        cache_blob
            .get(ctx, small_key)
            .await?
            .map(|bytes| bytes.into_bytes()),
        Some(value)
    );

    let large_key = "large_key";
    let size = 5 * 1024 * 1024;
    let mut large_value = Vec::with_capacity(size);
    large_value.resize(size, b'a');
    let large_value = BlobstoreBytes::from_bytes(large_value);

    cache_blob
        .put(ctx, large_key.to_owned(), large_value.clone())
        .await?;

    // Peek into cachelib to check its as expected
    let cachelib_len = blob_pool
        .get(large_key)
        .map(|bytes| bytes.map(|b| b.len()))?;

    if expect_zstd {
        assert!(cachelib_len.is_some());
        assert!(
            cachelib_len < Some(large_value.len()),
            "Expected cachelib value to be smaller due to compression"
        );
    } else {
        assert!(
            cachelib_len.is_none(),
            "Cachelib value is too large, so should not be in cachelib"
        );
    }

    // Check that inner blob is same size after put
    let inner_blob = inner.get(ctx, large_key).await?;
    assert_eq!(
        inner_blob.map(|b| b.as_bytes().len()),
        Some(large_value.len())
    );

    // Check that blob is the same when read through cache_blob's cachelib
    // layer
    assert_eq!(
        cache_blob
            .get(ctx, large_key)
            .await?
            .map(|bytes| bytes.into_bytes()),
        Some(large_value)
    );

    Ok(())
}
