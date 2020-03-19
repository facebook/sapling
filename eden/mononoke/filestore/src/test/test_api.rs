/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::{canonical, chunk, request};
use crate as filestore;
use crate::{errors, Alias, FetchKey, FilestoreConfig, StoreRequest};

use super::failing_blobstore::{FailingBlobstore, FailingBlobstoreError};
use anyhow::{Error, Result};
use assert_matches::assert_matches;
use blobstore::Blobstore;
use bytes::{Bytes, BytesMut};
use context::CoreContext;
use fbinit::FacebookInit;
use futures_old::{
    future::Future,
    stream::{self, Stream},
};
use lazy_static::lazy_static;
use mononoke_types::{hash, typed_hash::MononokeId, ContentId, ContentMetadata, ContentMetadataId};
use mononoke_types_mocks::contentid::ONES_CTID;

const HELLO_WORLD: &'static [u8] = b"hello, world";
const HELLO_WORLD_LENGTH: u64 = 12;
const DEFAULT_CONFIG: FilestoreConfig = FilestoreConfig {
    chunk_size: None,
    concurrency: 1,
};

lazy_static! {
    static ref HELLO_WORLD_SHA1: hash::Sha1 = hash::Sha1::from_bytes([
        0xb7, 0xe2, 0x3e, 0xc2, 0x9a, 0xf2, 0x2b, 0x0b, 0x4e, 0x41, 0xda, 0x31, 0xe8, 0x68, 0xd5,
        0x72, 0x26, 0x12, 0x1c, 0x84
    ])
    .unwrap();
    static ref HELLO_WORLD_GIT_SHA1: hash::RichGitSha1 = hash::RichGitSha1::from_bytes(
        [
            0x8c, 0x01, 0xd8, 0x9a, 0xe0, 0x63, 0x11, 0x83, 0x4e, 0xe4, 0xb1, 0xfa, 0xb2, 0xf0,
            0x41, 0x4d, 0x35, 0xf0, 0x11, 0x02
        ],
        "blob",
        HELLO_WORLD_LENGTH
    )
    .unwrap();
    static ref HELLO_WORLD_SHA256: hash::Sha256 = hash::Sha256::from_bytes([
        0x09, 0xca, 0x7e, 0x4e, 0xaa, 0x6e, 0x8a, 0xe9, 0xc7, 0xd2, 0x61, 0x16, 0x71, 0x29, 0x18,
        0x48, 0x83, 0x64, 0x4d, 0x07, 0xdf, 0xba, 0x7c, 0xbf, 0xbc, 0x4c, 0x8a, 0x2e, 0x08, 0x36,
        0x0d, 0x5b,
    ])
    .unwrap();
}

#[fbinit::test]
fn filestore_put_alias(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);

    let blob = memblob::LazyMemblob::new();
    let ctx = CoreContext::test_mock(fb);

    rt.block_on(filestore::store(
        blob.clone(),
        DEFAULT_CONFIG,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ))?;
    let res = rt.block_on(filestore::get_metadata(
        &blob,
        ctx,
        &FetchKey::Canonical(content_id),
    ));

    println!("res = {:#?}", res);

    assert_eq!(
        res?,
        Some(ContentMetadata {
            total_size: HELLO_WORLD_LENGTH,
            content_id,
            sha1: *HELLO_WORLD_SHA1,
            git_sha1: *HELLO_WORLD_GIT_SHA1,
            sha256: *HELLO_WORLD_SHA256
        })
    );

    Ok(())
}

#[fbinit::test]
fn filestore_put_get_canon(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);

    let blob = memblob::LazyMemblob::new();
    let ctx = CoreContext::test_mock(fb);

    rt.block_on(filestore::store(
        blob.clone(),
        DEFAULT_CONFIG,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ))?;

    let res = rt.block_on(filestore::fetch_concat_opt(
        &blob,
        ctx,
        &FetchKey::Canonical(content_id),
    ));

    println!("res = {:#?}", res);

    assert_eq!(res?, Some(Bytes::from(HELLO_WORLD)));

    Ok(())
}

#[fbinit::test]
fn filestore_put_get_sha1(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let req = request(HELLO_WORLD);

    let blob = memblob::LazyMemblob::new();
    let ctx = CoreContext::test_mock(fb);

    rt.block_on(filestore::store(
        blob.clone(),
        DEFAULT_CONFIG,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ))?;

    let res = rt.block_on(filestore::fetch_concat_opt(
        &blob,
        ctx,
        &FetchKey::Aliased(Alias::Sha1(*HELLO_WORLD_SHA1)),
    ));

    println!("res = {:#?}", res);

    assert_eq!(res?, Some(Bytes::from(HELLO_WORLD)));
    Ok(())
}

#[fbinit::test]
fn filestore_put_get_git_sha1(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let req = request(HELLO_WORLD);

    let blob = memblob::LazyMemblob::new();
    let ctx = CoreContext::test_mock(fb);

    rt.block_on(filestore::store(
        blob.clone(),
        DEFAULT_CONFIG,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ))?;

    let res = rt.block_on(filestore::fetch_concat_opt(
        &blob,
        ctx,
        &FetchKey::Aliased(Alias::GitSha1(HELLO_WORLD_GIT_SHA1.sha1())),
    ));

    println!("res = {:#?}", res);

    assert_eq!(res?, Some(Bytes::from(HELLO_WORLD)));
    Ok(())
}

#[fbinit::test]
fn filestore_put_get_sha256(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let req = request(HELLO_WORLD);

    let blob = memblob::LazyMemblob::new();
    let ctx = CoreContext::test_mock(fb);

    rt.block_on(filestore::store(
        blob.clone(),
        DEFAULT_CONFIG,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ))?;

    let res = rt.block_on(filestore::fetch_concat_opt(
        &blob,
        ctx,
        &FetchKey::Aliased(Alias::Sha256(*HELLO_WORLD_SHA256)),
    ));

    println!("res = {:#?}", res);

    assert_eq!(res?, Some(Bytes::from(HELLO_WORLD)));
    Ok(())
}

#[fbinit::test]
fn filestore_chunked_put_get(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);

    let blob = memblob::LazyMemblob::new();
    let config = FilestoreConfig {
        chunk_size: Some(1),
        concurrency: 5,
    };

    let ctx = CoreContext::test_mock(fb);

    rt.block_on(filestore::store(
        blob.clone(),
        config,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ))?;

    let res = rt.block_on(filestore::fetch_concat_opt(
        &blob,
        ctx,
        &FetchKey::Canonical(content_id),
    ));

    println!("res = {:#?}", res);

    assert_eq!(res?, Some(Bytes::from(HELLO_WORLD)));
    Ok(())
}

#[fbinit::test]
fn filestore_chunked_put_get_nested(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let blob = memblob::LazyMemblob::new();

    let small = FilestoreConfig {
        chunk_size: Some(1),
        concurrency: 5,
    };
    let large = FilestoreConfig {
        chunk_size: Some(3),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    let full_data = &b"foobar"[..];
    let full_key = request(full_data);
    let full_id = canonical(full_data);

    let part_data = &b"foo"[..];
    let part_key = request(part_data);

    // Store in 3-byte chunks
    rt.block_on(filestore::store(
        blob.clone(),
        large,
        ctx.clone(),
        &full_key,
        stream::once(Ok(Bytes::from(full_data))),
    ))?;

    // Now, go and split up one chunk into 1-byte parts.
    rt.block_on(filestore::store(
        blob.clone(),
        small,
        ctx.clone(),
        &part_key,
        stream::once(Ok(Bytes::from(part_data))),
    ))?;

    assert_fetches_as(&mut rt, ctx, &blob, full_id, vec!["foo", "bar"])?;
    Ok(())
}

#[fbinit::test]
fn filestore_content_not_found(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let blob = memblob::LazyMemblob::new();
    let ctx = CoreContext::test_mock(fb);

    // Missing content shouldn't throw an error

    let data = &b"foobar"[..];
    let content_id = canonical(data);

    // Verify that we can still read the full thing.
    let res = rt.block_on(filestore::fetch_concat_opt(
        &blob,
        ctx,
        &FetchKey::Canonical(content_id),
    ));

    println!("res = {:#?}", res);
    assert_eq!(res?, None);
    Ok(())
}

#[fbinit::test]
fn filestore_chunk_not_found(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let blob = memblob::LazyMemblob::new();
    let config = FilestoreConfig {
        chunk_size: Some(3),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    let data = &b"foobar"[..];
    let req = request(data);
    let content_id = canonical(data);

    let part = &b"foo"[..];
    let part_id = chunk(part);

    rt.block_on(filestore::store(
        blob.clone(),
        config,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(data))),
    ))?;

    assert!(blob.remove(&part_id.blobstore_key()).is_some());

    // This should fail
    let res = rt.block_on(filestore::fetch_concat_opt(
        &blob,
        ctx,
        &FetchKey::Canonical(content_id),
    ));

    println!("res = {:#?}", res);
    assert!(res.is_err());
    Ok(())
}

#[fbinit::test]
fn filestore_put_invalid_size(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let blob = memblob::LazyMemblob::new();
    let config = FilestoreConfig {
        chunk_size: Some(3),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    let data = &b"foobar"[..];
    let req = StoreRequest::new(123);

    let res = rt.block_on(filestore::store(
        blob,
        config,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(data))),
    ));
    println!("res = {:#?}", res);
    assert_matches!(
        res.unwrap_err().downcast::<errors::ErrorKind>(),
        Ok(errors::ErrorKind::InvalidSize(..))
    );
    Ok(())
}

#[fbinit::test]
fn filestore_put_content_id(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let blob = memblob::LazyMemblob::new();
    let config = FilestoreConfig {
        chunk_size: Some(3),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    // Bad Content Id should fail
    let req = StoreRequest::with_canonical(HELLO_WORLD_LENGTH, ONES_CTID);
    let res = rt.block_on(filestore::store(
        blob.clone(),
        config,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ));
    println!("res = {:#?}", res);
    assert_matches!(
        res.unwrap_err().downcast::<errors::ErrorKind>(),
        Ok(errors::ErrorKind::InvalidContentId(..))
    );

    // Correct content Id should succeed
    let req = StoreRequest::with_canonical(HELLO_WORLD_LENGTH, canonical(HELLO_WORLD));
    let res = rt.block_on(filestore::store(
        blob,
        config,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ));
    println!("res = {:#?}", res);
    assert!(res.is_ok());

    Ok(())
}

#[fbinit::test]
fn filestore_put_sha1(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let blob = memblob::LazyMemblob::new();
    let config = FilestoreConfig {
        chunk_size: Some(3),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    // Bad Content Id should fail
    let req = StoreRequest::with_sha1(HELLO_WORLD_LENGTH, hash::Sha1::from_byte_array([0x00; 20]));
    let res = rt.block_on(filestore::store(
        blob.clone(),
        config,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ));
    println!("res = {:#?}", res);
    assert_matches!(
        res.unwrap_err().downcast::<errors::ErrorKind>(),
        Ok(errors::ErrorKind::InvalidSha1(..))
    );

    // Correct content Id should succeed
    let req = StoreRequest::with_sha1(HELLO_WORLD_LENGTH, *HELLO_WORLD_SHA1);
    let res = rt.block_on(filestore::store(
        blob,
        config,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ));
    println!("res = {:#?}", res);
    assert!(res.is_ok());

    Ok(())
}

#[fbinit::test]
fn filestore_put_git_sha1(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let blob = memblob::LazyMemblob::new();
    let config = FilestoreConfig {
        chunk_size: Some(3),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    // Bad Content Id should fail
    let req = StoreRequest::with_git_sha1(
        HELLO_WORLD_LENGTH,
        hash::RichGitSha1::from_byte_array([0x00; 20], "blob", HELLO_WORLD_LENGTH),
    );
    let res = rt.block_on(filestore::store(
        blob.clone(),
        config,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ));
    println!("res = {:#?}", res);
    assert_matches!(
        res.unwrap_err().downcast::<errors::ErrorKind>(),
        Ok(errors::ErrorKind::InvalidGitSha1(..))
    );

    // Correct content Id should succeed
    let req = StoreRequest::with_git_sha1(HELLO_WORLD_LENGTH, *HELLO_WORLD_GIT_SHA1);
    let res = rt.block_on(filestore::store(
        blob,
        config,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ));
    println!("res = {:#?}", res);
    assert!(res.is_ok());

    Ok(())
}

#[fbinit::test]
fn filestore_put_sha256(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let blob = memblob::LazyMemblob::new();
    let config = FilestoreConfig {
        chunk_size: Some(3),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    // Bad Content Id should fail
    let req = StoreRequest::with_sha256(
        HELLO_WORLD_LENGTH,
        hash::Sha256::from_byte_array([0x00; 32]),
    );
    let res = rt.block_on(filestore::store(
        blob.clone(),
        config,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ));
    println!("res = {:#?}", res);
    assert_matches!(
        res.unwrap_err().downcast::<errors::ErrorKind>(),
        Ok(errors::ErrorKind::InvalidSha256(..))
    );

    // Correct content Id should succeed
    let req = StoreRequest::with_sha256(HELLO_WORLD_LENGTH, *HELLO_WORLD_SHA256);
    let res = rt.block_on(filestore::store(
        blob,
        config,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ));
    println!("res = {:#?}", res);
    assert!(res.is_ok());

    Ok(())
}

#[fbinit::test]
fn filestore_get_range(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);

    let blob = memblob::LazyMemblob::new();
    let ctx = CoreContext::test_mock(fb);

    rt.block_on(filestore::store(
        blob.clone(),
        DEFAULT_CONFIG,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ))?;

    let res = rt.block_on(
        filestore::fetch_range_with_size(&blob, ctx, &FetchKey::Canonical(content_id), 7, 5)
            .map(|maybe_stream| {
                maybe_stream.map(|(stream, _size)| {
                    stream
                        .fold(BytesMut::new(), |mut buff, chunk| {
                            buff.extend_from_slice(&chunk);
                            Result::<_, Error>::Ok(buff)
                        })
                        .map(BytesMut::freeze)
                })
            })
            .flatten(),
    );

    println!("res = {:#?}", res);

    assert_eq!(res?, Some(Bytes::from(&HELLO_WORLD[7..])));

    Ok(())
}

#[fbinit::test]
fn filestore_get_chunked_range(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let small = FilestoreConfig {
        chunk_size: Some(3),
        concurrency: 5,
    };

    let blob = memblob::LazyMemblob::new();
    let ctx = CoreContext::test_mock(fb);

    let full_data = &b"foobarbazquxxyz"[..];
    let full_key = request(full_data);
    let full_id = canonical(full_data);

    // Store in 3-byte chunks
    rt.block_on(filestore::store(
        blob.clone(),
        small,
        ctx.clone(),
        &full_key,
        stream::once(Ok(Bytes::from(full_data))),
    ))?;

    let res = rt.block_on(
        filestore::fetch_range_with_size(&blob, ctx, &FetchKey::Canonical(full_id), 4, 6)
            .map(|maybe_stream| {
                maybe_stream.map(|(stream, _size)| {
                    stream
                        .fold(BytesMut::new(), |mut buff, chunk| {
                            buff.extend_from_slice(&chunk);
                            Result::<_, Error>::Ok(buff)
                        })
                        .map(BytesMut::freeze)
                })
            })
            .flatten(),
    );

    println!("res = {:#?}", res);

    assert_eq!(res?, Some(Bytes::from(&b"arbazq"[..])));

    Ok(())
}

#[fbinit::test]
fn filestore_rebuild_metadata(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);
    let metadata: ContentMetadataId = content_id.clone().into();

    let expected = Some(ContentMetadata {
        total_size: HELLO_WORLD_LENGTH,
        content_id,
        sha1: *HELLO_WORLD_SHA1,
        git_sha1: *HELLO_WORLD_GIT_SHA1,
        sha256: *HELLO_WORLD_SHA256,
    });

    let blob = memblob::LazyMemblob::new();
    let ctx = CoreContext::test_mock(fb);

    rt.block_on(filestore::store(
        blob.clone(),
        DEFAULT_CONFIG,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ))?;

    // Remove the metadata
    assert!(blob.remove(&metadata.blobstore_key()).is_some());

    // Getting the metadata should cause it to get recomputed
    let res = rt.block_on(filestore::get_metadata(
        &blob,
        ctx.clone(),
        &FetchKey::Canonical(content_id),
    ));
    println!("res = {:#?}", res);
    assert_eq!(res?, expected);

    // Now, delete the content (this shouldn't normally happen, but we're injecting failure here).
    assert!(blob.remove(&content_id.blobstore_key()).is_some());

    // Query the metadata again. It should succeed because it's saved.
    let res = rt.block_on(filestore::get_metadata(
        &blob,
        ctx.clone(),
        &FetchKey::Canonical(content_id),
    ));
    println!("res = {:#?}", res);
    assert_eq!(res?, expected);

    // Delete the metadata now.
    assert!(blob.remove(&metadata.blobstore_key()).is_some());

    // And then, query it again. This should now return None, because the metadata isn't there,
    // and we can't recreate it.
    let res = rt.block_on(filestore::get_metadata(
        &blob,
        ctx.clone(),
        &FetchKey::Canonical(content_id),
    ));
    println!("res = {:#?}", res);
    assert_eq!(res?, None);

    Ok(())
}

#[fbinit::test]
fn filestore_test_missing_metadata(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let content_id = canonical(HELLO_WORLD);

    let blob = memblob::LazyMemblob::new();
    let ctx = CoreContext::test_mock(fb);

    // No matter the Fetchkey, querying the metadata should return None.

    let res = rt.block_on(filestore::get_metadata(
        &blob,
        ctx.clone(),
        &FetchKey::Canonical(content_id),
    ));
    println!("res = {:#?}", res);
    assert_eq!(res?, None);

    let res = rt.block_on(filestore::get_metadata(
        &blob,
        ctx.clone(),
        &FetchKey::Aliased(Alias::Sha1(*HELLO_WORLD_SHA1)),
    ));
    println!("res = {:#?}", res);
    assert_eq!(res?, None);

    let res = rt.block_on(filestore::get_metadata(
        &blob,
        ctx.clone(),
        &FetchKey::Aliased(Alias::Sha256(*HELLO_WORLD_SHA256)),
    ));
    println!("res = {:#?}", res);
    assert_eq!(res?, None);

    let res = rt.block_on(filestore::get_metadata(
        &blob,
        ctx.clone(),
        &FetchKey::Aliased(Alias::GitSha1(HELLO_WORLD_GIT_SHA1.sha1())),
    ));
    println!("res = {:#?}", res);
    assert_eq!(res?, None);

    Ok(())
}

#[fbinit::test]
fn filestore_test_peek(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);

    let blob = memblob::LazyMemblob::new();
    let ctx = CoreContext::test_mock(fb);

    rt.block_on(filestore::store(
        blob.clone(),
        DEFAULT_CONFIG,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ))?;

    let res = rt.block_on(filestore::peek(
        &blob,
        ctx,
        &FetchKey::Canonical(content_id),
        3,
    ));
    println!("res = {:#?}", res);

    let expected: &[u8] = b"hel";
    assert_eq!(res?, Some(Bytes::from(expected)));

    Ok(())
}

#[fbinit::test]
fn filestore_test_chunked_peek(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);

    let small = FilestoreConfig {
        chunk_size: Some(1),
        concurrency: 5,
    };

    let blob = memblob::LazyMemblob::new();
    let ctx = CoreContext::test_mock(fb);

    rt.block_on(filestore::store(
        blob.clone(),
        small,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ))?;

    let res = rt.block_on(filestore::peek(
        &blob,
        ctx,
        &FetchKey::Canonical(content_id),
        3,
    ));
    println!("res = {:#?}", res);

    let expected: &[u8] = b"hel";
    assert_eq!(res?, Some(Bytes::from(expected)));

    Ok(())
}

#[fbinit::test]
fn filestore_test_short_peek(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);

    let blob = memblob::LazyMemblob::new();
    let ctx = CoreContext::test_mock(fb);

    rt.block_on(filestore::store(
        blob.clone(),
        DEFAULT_CONFIG,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ))?;

    let res = rt.block_on(filestore::peek(
        &blob,
        ctx,
        &FetchKey::Canonical(content_id),
        128,
    ));
    println!("res = {:#?}", res);

    assert_eq!(res?, Some(Bytes::from(HELLO_WORLD)));

    Ok(())
}

#[fbinit::test]
fn filestore_test_empty_peek(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let bytes = Bytes::new();

    let req = request(&bytes);
    let content_id = canonical(&bytes);

    let blob = memblob::LazyMemblob::new();
    let ctx = CoreContext::test_mock(fb);

    rt.block_on(filestore::store(
        blob.clone(),
        DEFAULT_CONFIG,
        ctx.clone(),
        &req,
        stream::once(Ok(bytes.clone())),
    ))?;

    let res = rt.block_on(filestore::peek(
        &blob,
        ctx,
        &FetchKey::Canonical(content_id),
        128,
    ));
    println!("res = {:#?}", res);

    assert_eq!(res?, Some(bytes.clone()));

    Ok(())
}

#[fbinit::test]
fn filestore_store_bytes(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let blob = memblob::LazyMemblob::new();

    let ctx = CoreContext::test_mock(fb);
    let ((content_id, _size), fut) = filestore::store_bytes(
        blob.clone(),
        DEFAULT_CONFIG,
        ctx.clone(),
        Bytes::from(HELLO_WORLD),
    );
    assert_eq!(content_id, canonical(HELLO_WORLD));

    rt.block_on(fut)?;

    let res = rt.block_on(filestore::fetch_concat_opt(
        &blob,
        ctx,
        &FetchKey::Canonical(content_id),
    ));

    println!("res = {:#?}", res);

    assert_eq!(res?, Some(Bytes::from(HELLO_WORLD)));
    Ok(())
}

#[fbinit::test]
fn filestore_store_error(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let memblob = memblob::LazyMemblob::new();
    let blob = FailingBlobstore::new(memblob.clone(), 1.0, 0.0); // Blobstore you can't write to.

    let config = FilestoreConfig {
        chunk_size: Some(1),
        concurrency: 5,
    };

    let res = rt.block_on(filestore::store(
        blob,
        config,
        CoreContext::test_mock(fb),
        &request(HELLO_WORLD),
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ));

    println!("res = {:#?}", res);
    assert_matches!(
        res.unwrap_err().downcast::<FailingBlobstoreError>(),
        Ok(FailingBlobstoreError)
    );
    Ok(())
}

#[fbinit::test]
fn filestore_test_rechunk(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let blob = memblob::LazyMemblob::new();

    let small = FilestoreConfig {
        chunk_size: Some(1),
        concurrency: 5,
    };
    let large = FilestoreConfig {
        chunk_size: Some(3),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    let full_data = &b"foobar"[..];
    let full_key = request(full_data);
    let full_id = canonical(full_data);

    // Store in 3-byte chunks
    rt.block_on(filestore::store(
        blob.clone(),
        large,
        ctx.clone(),
        &full_key,
        stream::once(Ok(Bytes::from(full_data))),
    ))?;

    assert_fetches_as(&mut rt, ctx.clone(), &blob, full_id, vec!["foo", "bar"])?;

    // Rechunk the file into 1 byte sections
    rt.block_on(filestore::rechunk::force_rechunk(
        blob.clone(),
        small,
        ctx.clone(),
        full_id,
    ))?;

    assert_fetches_as(
        &mut rt,
        ctx,
        &blob,
        full_id,
        vec!["f", "o", "o", "b", "a", "r"],
    )
}

#[fbinit::test]
fn filestore_test_rechunk_larger(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let blob = memblob::LazyMemblob::new();

    let small = FilestoreConfig {
        chunk_size: Some(1),
        concurrency: 5,
    };
    let large = FilestoreConfig {
        chunk_size: Some(3),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    let full_data = &b"foobar"[..];
    let full_key = request(full_data);
    let full_id = canonical(full_data);

    // Store in 1 byte chunks
    rt.block_on(filestore::store(
        blob.clone(),
        small,
        ctx.clone(),
        &full_key,
        stream::once(Ok(Bytes::from(full_data))),
    ))?;

    assert_fetches_as(
        &mut rt,
        ctx.clone(),
        &blob,
        full_id,
        vec!["f", "o", "o", "b", "a", "r"],
    )?;

    // Rechunk the file into 3 byte sections
    rt.block_on(filestore::rechunk::force_rechunk(
        blob.clone(),
        large,
        ctx.clone(),
        full_id,
    ))?;

    assert_fetches_as(&mut rt, ctx, &blob, full_id, vec!["foo", "bar"])
}

#[fbinit::test]
fn filestore_test_rechunk_unchunked(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let blob = memblob::LazyMemblob::new();

    let small = FilestoreConfig {
        chunk_size: Some(1),
        concurrency: 5,
    };
    // This is large enough that the data we upload won't be chunked.
    let large = FilestoreConfig {
        chunk_size: Some(100),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    let full_data = &b"foobar"[..];
    let full_key = request(full_data);
    let full_id = canonical(full_data);

    // Don't chunk
    rt.block_on(filestore::store(
        blob.clone(),
        large,
        ctx.clone(),
        &full_key,
        stream::once(Ok(Bytes::from(full_data))),
    ))?;

    // Rechunk the file into 1 byte sections
    rt.block_on(filestore::rechunk::force_rechunk(
        blob.clone(),
        small,
        ctx.clone(),
        full_id,
    ))?;

    assert_fetches_as(
        &mut rt,
        ctx,
        &blob,
        full_id,
        vec!["f", "o", "o", "b", "a", "r"],
    )
}

#[fbinit::test]
fn filestore_test_rechunk_missing_content(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let blob = memblob::LazyMemblob::new();

    let conf = FilestoreConfig {
        chunk_size: Some(1),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    let full_data = &b"foobar"[..];
    let full_id = canonical(full_data);

    // Attempt to rechunk the file into 1 byte sections
    let res = rt.block_on(filestore::rechunk::force_rechunk(
        blob.clone(),
        conf,
        ctx.clone(),
        full_id,
    ));

    println!("res = {:#?}", res);
    assert_matches!(
        res.unwrap_err().downcast::<filestore::rechunk::ErrorKind>(),
        Ok(filestore::rechunk::ErrorKind::ContentNotFound(..))
    );

    Ok(())
}

#[fbinit::test]
fn filestore_chunked_put_get_with_size(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);

    let blob = memblob::LazyMemblob::new();
    let config = FilestoreConfig {
        chunk_size: Some(1),
        concurrency: 5,
    };

    let ctx = CoreContext::test_mock(fb);

    rt.block_on(filestore::store(
        blob.clone(),
        config,
        ctx.clone(),
        &req,
        stream::once(Ok(Bytes::from(HELLO_WORLD))),
    ))?;

    let res = rt.block_on(filestore::fetch_with_size(
        &blob,
        ctx,
        &FetchKey::Canonical(content_id),
    ));

    let (stream, size) = res?.unwrap();

    let fut = stream
        .fold(BytesMut::new(), |mut buff, chunk| {
            buff.extend_from_slice(&chunk);
            Result::<_, Error>::Ok(buff)
        })
        .map(BytesMut::freeze);

    let bytes = rt.block_on(fut);

    println!("{:?}", bytes);

    assert_eq!(bytes?, Bytes::from(HELLO_WORLD));
    assert_eq!(size, HELLO_WORLD_LENGTH);
    Ok(())
}

#[fbinit::test]
/// Test a case, where both old and new filestore config do not require
/// chunking of the file (e.g. file size is smaller than a single chunk)
fn filestore_test_rechunk_if_needed_tiny_unchunked_file(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let blob = memblob::LazyMemblob::new();

    let large1 = FilestoreConfig {
        chunk_size: Some(100),
        concurrency: 5,
    };
    let large2 = FilestoreConfig {
        chunk_size: Some(200),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    let full_data = &b"foobar"[..];
    let full_key = request(full_data);
    let full_id = canonical(full_data);

    // Don't chunk
    rt.block_on(filestore::store(
        blob.clone(),
        large1,
        ctx.clone(),
        &full_key,
        stream::once(Ok(Bytes::from(full_data))),
    ))?;

    assert_fetches_as(&mut rt, ctx.clone(), &blob, full_id, vec!["foobar"])?;

    // We expect that rechunk is not needed
    rt.block_on(filestore::rechunk::rechunk(
        FailingBlobstore::new(blob.clone(), 1.0, 0.0),
        large2,
        ctx.clone(),
        full_id,
    ))?;

    assert_fetches_as(&mut rt, ctx, &blob, full_id, vec!["foobar"])
}

#[fbinit::test]
fn filestore_test_rechunk_if_needed_large_unchunked_file(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let blob = memblob::LazyMemblob::new();

    let large = FilestoreConfig {
        chunk_size: Some(100),
        concurrency: 5,
    };
    let small = FilestoreConfig {
        chunk_size: Some(1),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    let full_data = &b"foobar"[..];
    let full_key = request(full_data);
    let full_id = canonical(full_data);

    // Don't chunk
    rt.block_on(filestore::store(
        blob.clone(),
        large,
        ctx.clone(),
        &full_key,
        stream::once(Ok(Bytes::from(full_data))),
    ))?;

    assert_fetches_as(&mut rt, ctx.clone(), &blob, full_id, vec!["foobar"])?;

    // We expect the rechunk is needed
    let (_, rechunked) = rt.block_on(filestore::rechunk::rechunk(
        blob.clone(),
        small,
        ctx.clone(),
        full_id,
    ))?;
    assert!(rechunked);

    assert_fetches_as(
        &mut rt,
        ctx,
        &blob,
        full_id,
        vec!["f", "o", "o", "b", "a", "r"],
    )
}

#[fbinit::test]
fn filestore_test_rechunk_if_needed_large_chunks(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let blob = memblob::LazyMemblob::new();

    let large = FilestoreConfig {
        chunk_size: Some(5),
        concurrency: 5,
    };
    let small = FilestoreConfig {
        chunk_size: Some(1),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    let full_data = &b"foobar"[..];
    let full_key = request(full_data);
    let full_id = canonical(full_data);

    // Chunk with larger chunks
    rt.block_on(filestore::store(
        blob.clone(),
        large,
        ctx.clone(),
        &full_key,
        stream::once(Ok(Bytes::from(full_data))),
    ))?;

    assert_fetches_as(&mut rt, ctx.clone(), &blob, full_id, vec!["fooba", "r"])?;

    // We expect the rechunk is needed
    let (_, rechunked) = rt.block_on(filestore::rechunk::rechunk(
        blob.clone(),
        small,
        ctx.clone(),
        full_id,
    ))?;
    assert!(rechunked);

    assert_fetches_as(
        &mut rt,
        ctx,
        &blob,
        full_id,
        vec!["f", "o", "o", "b", "a", "r"],
    )
}

#[fbinit::test]
fn filestore_test_rechunk_if_needed_tiny_chunks(fb: FacebookInit) -> Result<()> {
    let mut rt = tokio_compat::runtime::Runtime::new()?;

    let blob = memblob::LazyMemblob::new();

    let large = FilestoreConfig {
        chunk_size: Some(4),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    let full_data = &b"foobar"[..];
    let full_key = request(full_data);
    let full_id = canonical(full_data);

    // Chunk
    rt.block_on(filestore::store(
        blob.clone(),
        large,
        ctx.clone(),
        &full_key,
        stream::once(Ok(Bytes::from(full_data))),
    ))?;

    assert_fetches_as(&mut rt, ctx.clone(), &blob, full_id, vec!["foob", "ar"])?;

    // We expect the rechunk is not needed
    let (_, rechunked) = rt.block_on(filestore::rechunk::rechunk(
        FailingBlobstore::new(blob.clone(), 1.0, 0.0),
        large,
        ctx.clone(),
        full_id,
    ))?;
    assert!(!rechunked);
    assert_fetches_as(&mut rt, ctx, &blob, full_id, vec!["foob", "ar"])
}

fn assert_fetches_as<B: Blobstore + Clone>(
    rt: &mut tokio_compat::runtime::Runtime,
    ctx: CoreContext,
    blobstore: &B,
    content_id: ContentId,
    expected: Vec<&'static str>,
) -> Result<()> {
    let res = rt.block_on(
        filestore::fetch(blobstore, ctx, &FetchKey::Canonical(content_id))
            .map(|maybe_stream| maybe_stream.map(|s| s.collect()))
            .flatten(),
    );
    println!("res = {:#?}", res);
    let expected = expected.into_iter().map(Bytes::from).collect();
    assert_eq!(res?, Some(expected));
    Ok(())
}
