/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::canonical;
use super::chunk;
use super::request;
use crate as filestore;
use crate::errors;
use crate::Alias;
use crate::FetchKey;
use crate::FilestoreConfig;
use crate::StoreRequest;

use super::failing_blobstore::FailingBlobstore;
use super::failing_blobstore::FailingBlobstoreError;
use anyhow::Error;
use anyhow::Result;
use assert_matches::assert_matches;
use blobstore::Blobstore;
use blobstore::PutBehaviour;
use borrowed::borrowed;
use bytes::Bytes;
use bytes::BytesMut;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::future;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::TryStreamExt;
use lazy_static::lazy_static;
use mononoke_types::hash;
use mononoke_types::typed_hash::BlobstoreKey;
use mononoke_types::ContentId;
use mononoke_types::ContentMetadata;
use mononoke_types::ContentMetadataId;
use mononoke_types_mocks::contentid::ONES_CTID;

const HELLO_WORLD: &[u8] = b"hello, world";
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
async fn filestore_put_alias(fb: FacebookInit) -> Result<()> {
    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);

    let blob = memblob::Memblob::default();
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob, req);

    filestore::store(
        blob,
        DEFAULT_CONFIG,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await?;
    let res = filestore::get_metadata(blob, ctx, &FetchKey::Canonical(content_id)).await;

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
async fn filestore_put_get_canon(fb: FacebookInit) -> Result<()> {
    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);

    let blob = memblob::Memblob::default();
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob, req);

    filestore::store(
        blob,
        DEFAULT_CONFIG,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await?;

    let res = filestore::fetch_concat_opt(blob, ctx, &FetchKey::Canonical(content_id)).await;

    println!("res = {:#?}", res);

    assert_eq!(res?, Some(Bytes::from(HELLO_WORLD)));

    Ok(())
}

#[fbinit::test]
async fn filestore_put_get_sha1(fb: FacebookInit) -> Result<()> {
    let req = request(HELLO_WORLD);

    let blob = memblob::Memblob::default();
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob, req);

    filestore::store(
        blob,
        DEFAULT_CONFIG,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await?;

    let res = filestore::fetch_concat_opt(
        blob,
        ctx,
        &FetchKey::Aliased(Alias::Sha1(*HELLO_WORLD_SHA1)),
    )
    .await;

    println!("res = {:#?}", res);

    assert_eq!(res?, Some(Bytes::from(HELLO_WORLD)));
    Ok(())
}

#[fbinit::test]
async fn filestore_put_get_git_sha1(fb: FacebookInit) -> Result<()> {
    let req = request(HELLO_WORLD);

    let blob = memblob::Memblob::default();
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob, req);

    filestore::store(
        blob,
        DEFAULT_CONFIG,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await?;

    let res = filestore::fetch_concat_opt(
        blob,
        ctx,
        &FetchKey::Aliased(Alias::GitSha1(HELLO_WORLD_GIT_SHA1.sha1())),
    )
    .await;

    println!("res = {:#?}", res);

    assert_eq!(res?, Some(Bytes::from(HELLO_WORLD)));
    Ok(())
}

#[fbinit::test]
async fn filestore_put_get_sha256(fb: FacebookInit) -> Result<()> {
    let req = request(HELLO_WORLD);

    let blob = memblob::Memblob::default();
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob, req);

    filestore::store(
        blob,
        DEFAULT_CONFIG,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await?;

    let res = filestore::fetch_concat_opt(
        blob,
        ctx,
        &FetchKey::Aliased(Alias::Sha256(*HELLO_WORLD_SHA256)),
    )
    .await;

    println!("res = {:#?}", res);

    assert_eq!(res?, Some(Bytes::from(HELLO_WORLD)));
    Ok(())
}

#[fbinit::test]
async fn filestore_chunked_put_get(fb: FacebookInit) -> Result<()> {
    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);

    let blob = memblob::Memblob::default();
    let config = FilestoreConfig {
        chunk_size: Some(1),
        concurrency: 5,
    };

    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob, req);

    filestore::store(
        blob,
        config,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await?;

    let res = filestore::fetch_concat_opt(blob, ctx, &FetchKey::Canonical(content_id)).await;

    println!("res = {:#?}", res);

    assert_eq!(res?, Some(Bytes::from(HELLO_WORLD)));
    Ok(())
}

#[fbinit::test]
async fn filestore_chunked_put_get_nested(fb: FacebookInit) -> Result<()> {
    let blob = memblob::Memblob::default();

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
    borrowed!(ctx, blob, full_key, part_key);

    // Store in 3-byte chunks
    filestore::store(
        blob,
        large,
        ctx,
        full_key,
        stream::once(future::ready(Ok(Bytes::from(full_data)))),
    )
    .await?;

    // Now, go and split up one chunk into 1-byte parts.
    filestore::store(
        blob,
        small,
        ctx,
        part_key,
        stream::once(future::ready(Ok(Bytes::from(part_data)))),
    )
    .await?;

    assert_fetches_as(ctx, blob, full_id, vec!["foo", "bar"]).await?;
    Ok(())
}

#[fbinit::test]
async fn filestore_content_not_found(fb: FacebookInit) -> Result<()> {
    let blob = memblob::Memblob::default();
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob);

    // Missing content shouldn't throw an error

    let data = &b"foobar"[..];
    let content_id = canonical(data);

    // Verify that we can still read the full thing.
    let res = filestore::fetch_concat_opt(blob, ctx, &FetchKey::Canonical(content_id)).await;

    println!("res = {:#?}", res);
    assert_eq!(res?, None);
    Ok(())
}

#[fbinit::test]
async fn filestore_chunk_not_found(fb: FacebookInit) -> Result<()> {
    let blob = memblob::Memblob::default();
    let config = FilestoreConfig {
        chunk_size: Some(3),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    let data = &b"foobar"[..];
    let req = request(data);
    let content_id = canonical(data);
    borrowed!(ctx, blob, req);

    let part = &b"foo"[..];
    let part_id = chunk(part);

    filestore::store(
        blob,
        config,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(data)))),
    )
    .await?;

    assert!(
        blob.unlink(part_id.blobstore_key())
            .await
            .unwrap()
            .is_some()
    );

    // This should fail
    let res = filestore::fetch_concat_opt(&blob, ctx, &FetchKey::Canonical(content_id)).await;

    println!("res = {:#?}", res);
    assert!(res.is_err());
    Ok(())
}

#[fbinit::test]
async fn filestore_put_invalid_size(fb: FacebookInit) -> Result<()> {
    let blob = memblob::Memblob::default();
    let config = FilestoreConfig {
        chunk_size: Some(3),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    let data = &b"foobar"[..];
    let req = StoreRequest::new(123);
    borrowed!(ctx, blob, req);

    let res = filestore::store(
        blob,
        config,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(data)))),
    )
    .await;
    println!("res = {:#?}", res);
    assert_matches!(
        res.unwrap_err().downcast::<errors::ErrorKind>(),
        Ok(errors::ErrorKind::InvalidSize(..))
    );
    Ok(())
}

#[fbinit::test]
async fn filestore_put_content_id(fb: FacebookInit) -> Result<()> {
    let blob = memblob::Memblob::default();
    let config = FilestoreConfig {
        chunk_size: Some(3),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    // Bad Content Id should fail
    let req = StoreRequest::with_canonical(HELLO_WORLD_LENGTH, ONES_CTID);
    borrowed!(ctx, blob, req);

    let res = filestore::store(
        blob,
        config,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await;
    println!("res = {:#?}", res);
    assert_matches!(
        res.unwrap_err().downcast::<errors::ErrorKind>(),
        Ok(errors::ErrorKind::InvalidContentId(..))
    );

    // Correct content Id should succeed
    let req = StoreRequest::with_canonical(HELLO_WORLD_LENGTH, canonical(HELLO_WORLD));
    let res = filestore::store(
        blob,
        config,
        ctx,
        &req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await;
    println!("res = {:#?}", res);
    assert!(res.is_ok());

    Ok(())
}

#[fbinit::test]
async fn filestore_put_sha1(fb: FacebookInit) -> Result<()> {
    let blob = memblob::Memblob::default();
    let config = FilestoreConfig {
        chunk_size: Some(3),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    // Bad Content Id should fail
    let req = StoreRequest::with_sha1(HELLO_WORLD_LENGTH, hash::Sha1::from_byte_array([0x00; 20]));
    borrowed!(ctx, blob, req);
    let res = filestore::store(
        blob,
        config,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await;
    println!("res = {:#?}", res);
    assert_matches!(
        res.unwrap_err().downcast::<errors::ErrorKind>(),
        Ok(errors::ErrorKind::InvalidSha1(..))
    );

    // Correct content Id should succeed
    let req = StoreRequest::with_sha1(HELLO_WORLD_LENGTH, *HELLO_WORLD_SHA1);
    let res = filestore::store(
        blob,
        config,
        ctx,
        &req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await;
    println!("res = {:#?}", res);
    assert!(res.is_ok());

    Ok(())
}

#[fbinit::test]
async fn filestore_put_git_sha1(fb: FacebookInit) -> Result<()> {
    let blob = memblob::Memblob::default();
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
    borrowed!(ctx, blob, req);

    let res = filestore::store(
        blob,
        config,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await;
    println!("res = {:#?}", res);
    assert_matches!(
        res.unwrap_err().downcast::<errors::ErrorKind>(),
        Ok(errors::ErrorKind::InvalidGitSha1(..))
    );

    // Correct content Id should succeed
    let req = StoreRequest::with_git_sha1(HELLO_WORLD_LENGTH, *HELLO_WORLD_GIT_SHA1);
    let res = filestore::store(
        blob,
        config,
        ctx,
        &req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await;
    println!("res = {:#?}", res);
    assert!(res.is_ok());

    Ok(())
}

#[fbinit::test]
async fn filestore_put_sha256(fb: FacebookInit) -> Result<()> {
    let blob = memblob::Memblob::default();
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
    borrowed!(ctx, blob, req);

    let res = filestore::store(
        blob,
        config,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await;
    println!("res = {:#?}", res);
    assert_matches!(
        res.unwrap_err().downcast::<errors::ErrorKind>(),
        Ok(errors::ErrorKind::InvalidSha256(..))
    );

    // Correct content Id should succeed
    let req = StoreRequest::with_sha256(HELLO_WORLD_LENGTH, *HELLO_WORLD_SHA256);
    let res = filestore::store(
        blob,
        config,
        ctx,
        &req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await;
    println!("res = {:#?}", res);
    assert!(res.is_ok());

    Ok(())
}

#[fbinit::test]
async fn filestore_get_range(fb: FacebookInit) -> Result<()> {
    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);

    let blob = memblob::Memblob::default();
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob, req);

    filestore::store(
        blob,
        DEFAULT_CONFIG,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await?;

    let res = async {
        let stream = filestore::fetch_range_with_size(
            &blob,
            ctx,
            &FetchKey::Canonical(content_id),
            filestore::Range::sized(7, 5),
        )
        .await?
        .ok_or_else(|| Error::msg("Object does not exist"))?
        .0;

        let bytes = stream
            .try_fold(BytesMut::new(), |mut buff, chunk| async move {
                buff.extend_from_slice(&chunk);
                Result::<_, Error>::Ok(buff)
            })
            .await?
            .freeze();

        Result::<_, Error>::Ok(bytes)
    }
    .await;

    println!("res = {:#?}", res);

    assert_eq!(res?, Bytes::from(&HELLO_WORLD[7..]));

    Ok(())
}

#[fbinit::test]
async fn filestore_get_invalid_range(fb: FacebookInit) -> Result<()> {
    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);

    let blob = memblob::Memblob::default();
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob, req);

    filestore::store(
        blob,
        DEFAULT_CONFIG,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await?;

    let res = filestore::fetch_range_with_size(
        &blob,
        ctx,
        &FetchKey::Canonical(content_id),
        filestore::Range::sized(0, 40),
    )
    .await;

    assert!(res.is_ok());

    let res = filestore::fetch_range_with_size(
        &blob,
        ctx,
        &FetchKey::Canonical(content_id),
        filestore::Range::sized(0, HELLO_WORLD.len() as u64).strict(),
    )
    .await;

    assert!(res.is_ok());

    let res = filestore::fetch_range_with_size(
        &blob,
        ctx,
        &FetchKey::Canonical(content_id),
        filestore::Range::sized(0, 40).strict(),
    )
    .await;

    assert!(res.is_err());

    Ok(())
}

#[fbinit::test]
async fn filestore_get_chunked_range(fb: FacebookInit) -> Result<()> {
    let small = FilestoreConfig {
        chunk_size: Some(3),
        concurrency: 5,
    };

    let blob = memblob::Memblob::default();
    let ctx = CoreContext::test_mock(fb);

    let full_data = &b"foobarbazquxxyz"[..];
    let full_key = request(full_data);
    let full_id = canonical(full_data);
    borrowed!(ctx, blob, full_key);

    // Store in 3-byte chunks
    filestore::store(
        blob,
        small,
        ctx,
        full_key,
        stream::once(future::ready(Ok(Bytes::from(full_data)))),
    )
    .await?;

    // Check that we get the data we expect (6 bytes starting from the one at offset 4, i.e. the
    // 5th one).
    let res = async {
        let stream = filestore::fetch_range_with_size(
            blob,
            ctx,
            &FetchKey::Canonical(full_id),
            filestore::Range::sized(4, 6),
        )
        .await?
        .ok_or_else(|| Error::msg("Object does not exist"))?
        .0;

        let bytes = stream
            .try_fold(BytesMut::new(), |mut buff, chunk| async move {
                buff.extend_from_slice(&chunk);
                Result::<_, Error>::Ok(buff)
            })
            .await?
            .freeze();

        Result::<_, Error>::Ok(bytes)
    }
    .await;

    println!("res = {:#?}", res);

    assert_eq!(res?, Bytes::from(&b"arbazq"[..]));

    // Check that we don't fetch things we do not need (extra chunks to the left).
    let res = async {
        let stream = filestore::fetch_range_with_size(
            blob,
            ctx,
            &FetchKey::Canonical(full_id),
            filestore::Range::sized(3, 2),
        )
        .await?
        .ok_or_else(|| Error::msg("Object does not exist"))?
        .0;

        stream.try_collect::<Vec<_>>().await
    }
    .await;

    println!("res = {:#?}", res);

    let res = res?;
    assert_eq!(res.len(), 1);
    assert_eq!(res[0], Bytes::from(&b"ba"[..]));

    // Check that we don't fetch things we do not need (extra chunks to the right).
    let res = async {
        let stream = filestore::fetch_range_with_size(
            blob,
            ctx,
            &FetchKey::Canonical(full_id),
            filestore::Range::sized(0, 3),
        )
        .await?
        .ok_or_else(|| Error::msg("Object does not exist"))?
        .0;

        stream.try_collect::<Vec<_>>().await
    }
    .await;

    println!("res = {:#?}", res);

    let res = res?;
    assert_eq!(res.len(), 1);
    assert_eq!(res[0], Bytes::from(&b"foo"[..]));

    Ok(())
}

#[fbinit::test]
async fn filestore_rebuild_metadata(fb: FacebookInit) -> Result<()> {
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

    let blob = memblob::Memblob::default();
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob, req);

    filestore::store(
        blob,
        DEFAULT_CONFIG,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await?;

    // Remove the metadata
    assert!(
        blob.unlink(metadata.blobstore_key())
            .await
            .unwrap()
            .is_some()
    );

    // Getting the metadata should cause it to get recomputed
    let res = filestore::get_metadata(blob, ctx, &FetchKey::Canonical(content_id)).await;
    println!("res = {:#?}", res);
    assert_eq!(res?, expected);

    // Now, delete the content (this shouldn't normally happen, but we're injecting failure here).
    assert!(
        blob.unlink(content_id.blobstore_key())
            .await
            .unwrap()
            .is_some()
    );

    // Query the metadata again. It should succeed because it's saved.
    let res = filestore::get_metadata(blob, ctx, &FetchKey::Canonical(content_id)).await;
    println!("res = {:#?}", res);
    assert_eq!(res?, expected);

    // Delete the metadata now.
    assert!(
        blob.unlink(metadata.blobstore_key())
            .await
            .unwrap()
            .is_some()
    );

    // And then, query it again. This should now return None, because the metadata isn't there,
    // and we can't recreate it.
    let res = filestore::get_metadata(blob, ctx, &FetchKey::Canonical(content_id)).await;
    println!("res = {:#?}", res);
    assert_eq!(res?, None);

    Ok(())
}

#[fbinit::test]
async fn filestore_test_missing_metadata(fb: FacebookInit) -> Result<()> {
    let content_id = canonical(HELLO_WORLD);

    let blob = memblob::Memblob::default();
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob);

    // No matter the Fetchkey, querying the metadata should return None.

    let res = filestore::get_metadata(blob, ctx, &FetchKey::Canonical(content_id)).await;
    println!("res = {:#?}", res);
    assert_eq!(res?, None);

    let res = filestore::get_metadata(
        blob,
        ctx,
        &FetchKey::Aliased(Alias::Sha1(*HELLO_WORLD_SHA1)),
    )
    .await;
    println!("res = {:#?}", res);
    assert_eq!(res?, None);

    let res = filestore::get_metadata(
        blob,
        ctx,
        &FetchKey::Aliased(Alias::Sha256(*HELLO_WORLD_SHA256)),
    )
    .await;
    println!("res = {:#?}", res);
    assert_eq!(res?, None);

    let res = filestore::get_metadata(
        blob,
        ctx,
        &FetchKey::Aliased(Alias::GitSha1(HELLO_WORLD_GIT_SHA1.sha1())),
    )
    .await;
    println!("res = {:#?}", res);
    assert_eq!(res?, None);

    Ok(())
}

#[fbinit::test]
async fn filestore_test_peek(fb: FacebookInit) -> Result<()> {
    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);

    let blob = memblob::Memblob::default();
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob, req);

    filestore::store(
        blob,
        DEFAULT_CONFIG,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await?;

    let res = filestore::peek(blob, ctx, &FetchKey::Canonical(content_id), 3).await;
    println!("res = {:#?}", res);

    let expected: &[u8] = b"hel";
    assert_eq!(res?, Some(Bytes::from(expected)));

    Ok(())
}

#[fbinit::test]
async fn filestore_test_chunked_peek(fb: FacebookInit) -> Result<()> {
    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);

    let small = FilestoreConfig {
        chunk_size: Some(1),
        concurrency: 5,
    };

    let blob = memblob::Memblob::default();
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob, req);

    filestore::store(
        blob,
        small,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await?;

    let res = filestore::peek(blob, ctx, &FetchKey::Canonical(content_id), 3).await;
    println!("res = {:#?}", res);

    let expected: &[u8] = b"hel";
    assert_eq!(res?, Some(Bytes::from(expected)));

    Ok(())
}

#[fbinit::test]
async fn filestore_test_short_peek(fb: FacebookInit) -> Result<()> {
    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);

    let blob = memblob::Memblob::default();
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob, req);

    filestore::store(
        blob,
        DEFAULT_CONFIG,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await?;

    let res = filestore::peek(blob, ctx, &FetchKey::Canonical(content_id), 128).await;
    println!("res = {:#?}", res);

    assert_eq!(res?, Some(Bytes::from(HELLO_WORLD)));

    Ok(())
}

#[fbinit::test]
async fn filestore_test_empty_peek(fb: FacebookInit) -> Result<()> {
    let bytes = Bytes::new();

    let req = request(&bytes);
    let content_id = canonical(&bytes);

    let blob = memblob::Memblob::default();
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob, req);

    filestore::store(
        blob,
        DEFAULT_CONFIG,
        ctx,
        req,
        stream::once(future::ready(Ok(bytes.clone()))),
    )
    .await?;

    let res = filestore::peek(blob, ctx, &FetchKey::Canonical(content_id), 128).await;
    println!("res = {:#?}", res);

    assert_eq!(res?, Some(bytes.clone()));

    Ok(())
}

#[fbinit::test]
async fn filestore_store_bytes(fb: FacebookInit) -> Result<()> {
    let blob: memblob::Memblob = memblob::Memblob::default();

    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob);
    let ((content_id, _size), fut) =
        filestore::store_bytes(blob, DEFAULT_CONFIG, ctx, Bytes::from(HELLO_WORLD));
    assert_eq!(content_id, canonical(HELLO_WORLD));

    fut.await?;

    let res = filestore::fetch_concat_opt(blob, ctx, &FetchKey::Canonical(content_id)).await;

    println!("res = {:#?}", res);

    assert_eq!(res?, Some(Bytes::from(HELLO_WORLD)));
    Ok(())
}

#[fbinit::test]
async fn filestore_store_error(fb: FacebookInit) -> Result<()> {
    let blob = memblob::Memblob::default();
    let blob = FailingBlobstore::new(blob, 1.0, 0.0); // Blobstore you can't write to.
    let ctx = CoreContext::test_mock(fb);
    let req = request(HELLO_WORLD);
    borrowed!(ctx, blob, req);

    let config = FilestoreConfig {
        chunk_size: Some(1),
        concurrency: 5,
    };

    let res = filestore::store(
        blob,
        config,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await;

    println!("res = {:#?}", res);
    assert_matches!(
        res.unwrap_err().downcast::<FailingBlobstoreError>(),
        Ok(FailingBlobstoreError)
    );
    Ok(())
}

#[fbinit::test]
async fn filestore_test_rechunk(fb: FacebookInit) -> Result<()> {
    let blob = memblob::Memblob::new(PutBehaviour::Overwrite);

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
    borrowed!(ctx, blob, full_key);

    // Store in 3-byte chunks
    filestore::store(
        blob,
        large,
        ctx,
        full_key,
        stream::once(future::ready(Ok(Bytes::from(full_data)))),
    )
    .await?;

    assert_fetches_as(ctx, blob, full_id, vec!["foo", "bar"]).await?;

    // Rechunk the file into 1 byte sections
    filestore::rechunk::force_rechunk(blob, small, ctx, full_id).await?;

    assert_fetches_as(ctx, blob, full_id, vec!["f", "o", "o", "b", "a", "r"]).await
}

#[fbinit::test]
async fn filestore_test_rechunk_larger(fb: FacebookInit) -> Result<()> {
    let blob = memblob::Memblob::new(PutBehaviour::Overwrite);

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
    borrowed!(ctx, blob, full_key);

    // Store in 1 byte chunks
    filestore::store(
        blob,
        small,
        ctx,
        full_key,
        stream::once(future::ready(Ok(Bytes::from(full_data)))),
    )
    .await?;

    assert_fetches_as(ctx, blob, full_id, vec!["f", "o", "o", "b", "a", "r"]).await?;

    // Rechunk the file into 3 byte sections
    filestore::rechunk::force_rechunk(blob, large, ctx, full_id).await?;

    assert_fetches_as(ctx, blob, full_id, vec!["foo", "bar"]).await
}

#[fbinit::test]
async fn filestore_test_rechunk_unchunked(fb: FacebookInit) -> Result<()> {
    let blob = memblob::Memblob::new(PutBehaviour::Overwrite);

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
    borrowed!(ctx, blob, full_key);

    // Don't chunk
    filestore::store(
        blob,
        large,
        ctx,
        full_key,
        stream::once(future::ready(Ok(Bytes::from(full_data)))),
    )
    .await?;

    // Rechunk the file into 1 byte sections
    filestore::rechunk::force_rechunk(blob, small, ctx, full_id).await?;

    assert_fetches_as(ctx, blob, full_id, vec!["f", "o", "o", "b", "a", "r"]).await
}

#[fbinit::test]
async fn filestore_test_rechunk_missing_content(fb: FacebookInit) -> Result<()> {
    let blob = memblob::Memblob::default();

    let conf = FilestoreConfig {
        chunk_size: Some(1),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob);

    let full_data = &b"foobar"[..];
    let full_id = canonical(full_data);

    // Attempt to rechunk the file into 1 byte sections
    let res = filestore::rechunk::force_rechunk(blob, conf, ctx, full_id).await;

    println!("res = {:#?}", res);
    assert_matches!(
        res.unwrap_err().downcast::<filestore::rechunk::ErrorKind>(),
        Ok(filestore::rechunk::ErrorKind::ContentNotFound(..))
    );

    Ok(())
}

#[fbinit::test]
async fn filestore_chunked_put_get_with_size(fb: FacebookInit) -> Result<()> {
    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);

    let blob = memblob::Memblob::default();
    let config = FilestoreConfig {
        chunk_size: Some(1),
        concurrency: 5,
    };

    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob, req);

    filestore::store(
        blob,
        config,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await?;

    let res = filestore::fetch_with_size(blob, ctx, &FetchKey::Canonical(content_id)).await;

    let (stream, size) = res?.unwrap();

    let fut = stream
        .try_fold(BytesMut::new(), |mut buff, chunk| async move {
            buff.extend_from_slice(&chunk);
            Result::<_, Error>::Ok(buff)
        })
        .map_ok(BytesMut::freeze);

    let bytes = fut.await;

    println!("{:?}", bytes);

    assert_eq!(bytes?, Bytes::from(HELLO_WORLD));
    assert_eq!(size, HELLO_WORLD_LENGTH);
    Ok(())
}

#[fbinit::test]
/// Test a case, where both old and new filestore config do not require
/// chunking of the file (e.g. file size is smaller than a single chunk)
async fn filestore_test_rechunk_if_needed_tiny_unchunked_file(fb: FacebookInit) -> Result<()> {
    let blob = memblob::Memblob::default();

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
    borrowed!(ctx, blob, full_key);

    // Don't chunk
    filestore::store(
        blob,
        large1,
        ctx,
        full_key,
        stream::once(future::ready(Ok(Bytes::from(full_data)))),
    )
    .await?;

    assert_fetches_as(ctx, blob, full_id, vec!["foobar"]).await?;

    // We expect that rechunk is not needed
    filestore::rechunk::rechunk(
        &FailingBlobstore::new(blob.clone(), 1.0, 0.0),
        large2,
        ctx,
        full_id,
    )
    .await?;

    assert_fetches_as(ctx, blob, full_id, vec!["foobar"]).await
}

#[fbinit::test]
async fn filestore_test_rechunk_if_needed_large_unchunked_file(fb: FacebookInit) -> Result<()> {
    let blob = memblob::Memblob::new(PutBehaviour::Overwrite);

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
    borrowed!(ctx, blob, full_key);

    // Don't chunk
    filestore::store(
        blob,
        large,
        ctx,
        full_key,
        stream::once(future::ready(Ok(Bytes::from(full_data)))),
    )
    .await?;

    assert_fetches_as(ctx, blob, full_id, vec!["foobar"]).await?;

    // We expect the rechunk is needed
    let (_, rechunked) = filestore::rechunk::rechunk(blob, small, ctx, full_id).await?;
    assert!(rechunked);

    assert_fetches_as(ctx, blob, full_id, vec!["f", "o", "o", "b", "a", "r"]).await
}

#[fbinit::test]
async fn filestore_test_rechunk_if_needed_large_chunks(fb: FacebookInit) -> Result<()> {
    let blob = memblob::Memblob::new(PutBehaviour::Overwrite);

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
    borrowed!(ctx, blob, full_key);

    // Chunk with larger chunks
    filestore::store(
        blob,
        large,
        ctx,
        full_key,
        stream::once(future::ready(Ok(Bytes::from(full_data)))),
    )
    .await?;

    assert_fetches_as(ctx, blob, full_id, vec!["fooba", "r"]).await?;

    // We expect the rechunk is needed
    let (_, rechunked) = filestore::rechunk::rechunk(blob, small, ctx, full_id).await?;
    assert!(rechunked);

    assert_fetches_as(ctx, blob, full_id, vec!["f", "o", "o", "b", "a", "r"]).await
}

#[fbinit::test]
async fn filestore_test_rechunk_if_needed_tiny_chunks(fb: FacebookInit) -> Result<()> {
    let blob = memblob::Memblob::default();

    let large = FilestoreConfig {
        chunk_size: Some(4),
        concurrency: 5,
    };
    let ctx = CoreContext::test_mock(fb);

    let full_data = &b"foobar"[..];
    let full_key = request(full_data);
    let full_id = canonical(full_data);
    borrowed!(ctx, blob, full_key);

    // Chunk
    filestore::store(
        blob,
        large,
        ctx,
        full_key,
        stream::once(future::ready(Ok(Bytes::from(full_data)))),
    )
    .await?;
    assert_fetches_as(ctx, blob, full_id, vec!["foob", "ar"]).await?;

    // We expect the rechunk is not needed
    let (_, rechunked) = filestore::rechunk::rechunk(
        &FailingBlobstore::new(blob.clone(), 1.0, 0.0),
        large,
        ctx,
        full_id,
    )
    .await?;
    assert!(!rechunked);
    assert_fetches_as(ctx, blob, full_id, vec!["foob", "ar"]).await
}

async fn assert_fetches_as<B: Blobstore, S: Into<Bytes>>(
    ctx: &CoreContext,
    blobstore: &B,
    content_id: ContentId,
    expected: Vec<S>,
) -> Result<()> {
    let expected = expected.into_iter().map(|s| s.into()).collect();
    let key = FetchKey::Canonical(content_id);
    let maybe_stream = filestore::fetch(blobstore, ctx, &key).await?;
    let res = match maybe_stream {
        Some(stream) => Some(stream.try_collect::<Vec<_>>().await?),
        None => None,
    };

    println!("res = {:#?}", res);
    assert_eq!(res, Some(expected));
    Ok(())
}

#[fbinit::test]
async fn filestore_exists(fb: FacebookInit) -> Result<()> {
    let req = request(HELLO_WORLD);
    let content_id = canonical(HELLO_WORLD);
    let alias = Alias::Sha1(*HELLO_WORLD_SHA1);

    let blob = memblob::Memblob::default();
    let ctx = CoreContext::test_mock(fb);
    borrowed!(ctx, blob, req);

    assert!(!filestore::exists(blob, ctx, &content_id.into()).await?);
    assert!(!filestore::exists(blob, ctx, &alias.into()).await?);

    filestore::store(
        blob,
        DEFAULT_CONFIG,
        ctx,
        req,
        stream::once(future::ready(Ok(Bytes::from(HELLO_WORLD)))),
    )
    .await?;

    assert!(filestore::exists(blob, ctx, &content_id.into()).await?);
    assert!(filestore::exists(blob, ctx, &alias.into()).await?);

    Ok(())
}
