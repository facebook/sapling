/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![type_length_limit = "1817182"]

use anyhow::anyhow;
use anyhow::Result;
use blobstore::Blobstore;
use context::CoreContext;
use filestore::exists;
use filestore::fetch;
use filestore::get_metadata;
use filestore::store;
use filestore::FetchKey;
use filestore::FilestoreConfig;
use filestore::StoreRequest;
use mononoke_types::ContentId;
use repo_blobstore::RepoBlobstore;

/// Copy a blob with a key `key` from `src_blobstore` to `dst_blobstore`
pub async fn copy_blob(
    ctx: &CoreContext,
    src_blobstore: &RepoBlobstore,
    dst_blobstore: &RepoBlobstore,
    key: String,
) -> Result<()> {
    // TODO(ikostia, T48858215): for cases when remote copy is possible, utilize it
    let srcdata = src_blobstore
        .get(ctx, &key)
        .await?
        .ok_or_else(|| anyhow!("Key {} is missing in the original store", key))?;
    dst_blobstore.put(ctx, key, srcdata.into()).await
}

pub async fn copy_content(
    ctx: &CoreContext,
    src_blobstore: &RepoBlobstore,
    dst_blobstore: &RepoBlobstore,
    dst_filestore_config: FilestoreConfig,
    key: ContentId,
) -> Result<()> {
    let fetch_key = FetchKey::Canonical(key.clone());
    if exists(dst_blobstore, ctx, &fetch_key).await? {
        return Ok(());
    }

    let content_metadata = get_metadata(src_blobstore, ctx, &fetch_key)
        .await?
        .ok_or_else(|| anyhow!("File not found for fetch key: {:?}", fetch_key))?;

    let store_request = StoreRequest::with_canonical(content_metadata.total_size, key);

    let byte_stream = fetch(src_blobstore, ctx, &fetch_key)
        .await?
        .ok_or_else(|| anyhow!("File not found for fetch key: {:?}", fetch_key))?;

    store(
        dst_blobstore,
        dst_filestore_config,
        ctx,
        &store_request,
        byte_stream,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod test {
    #![allow(warnings)]

    use super::*;
    use borrowed::borrowed;
    use bytes::Bytes;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use futures::stream;
    use memblob::Memblob;
    use mononoke_types::typed_hash;
    use mononoke_types::BlobstoreBytes;
    use mononoke_types::ContentMetadata;
    use mononoke_types::RepositoryId;
    use redactedblobstore::RedactedBlobstore;
    use repo_blobstore::RepoBlobstore;
    use scuba_ext::MononokeScubaSampleBuilder;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn request(data: impl AsRef<[u8]>) -> StoreRequest {
        StoreRequest::new(data.as_ref().len() as u64)
    }

    fn canonical(data: impl AsRef<[u8]>) -> ContentId {
        let mut ctx = typed_hash::ContentIdContext::new();
        ctx.update(data.as_ref());
        ctx.finish()
    }

    #[fbinit::test]
    async fn test_copy_blob(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let inner1 = Arc::new(Memblob::default());
        let inner2 = Arc::new(Memblob::default());

        let bs1 = RepoBlobstore::new(
            inner1,
            None,
            RepositoryId::new(1),
            MononokeScubaSampleBuilder::with_discard(),
        );

        let bs2 = RepoBlobstore::new(
            inner2,
            None,
            RepositoryId::new(2),
            MononokeScubaSampleBuilder::with_discard(),
        );

        borrowed!(ctx, bs1, bs2);

        let key = "key";
        let blob = BlobstoreBytes::from_bytes("blob");
        assert!(
            bs1.put(ctx, key.to_owned(), blob.clone()).await.is_ok(),
            "failed to put things into a blobstore"
        );
        assert!(
            copy_blob(ctx, bs1, bs2, key.to_owned()).await.is_ok(),
            "failed to copy between blobstores"
        );
        let res = bs2.get(ctx, key).await;
        assert!(
            res.unwrap() == Some(blob.into()),
            "failed to get a copied blob from the second blobstore"
        );

        assert!(
            copy_blob(ctx, bs1, bs2, "non-existing key".to_string())
                .await
                .is_err(),
            "did not err while trying to copy a non-existing key"
        )
    }

    #[fbinit::test]
    async fn test_copy_content(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let inner1 = Arc::new(Memblob::default());
        let inner2 = Arc::new(Memblob::default());

        let bs1 = RepoBlobstore::new(
            inner1,
            None,
            RepositoryId::new(1),
            MononokeScubaSampleBuilder::with_discard(),
        );

        let bs2 = RepoBlobstore::new(
            inner2,
            None,
            RepositoryId::new(2),
            MononokeScubaSampleBuilder::with_discard(),
        );

        borrowed!(ctx, bs1, bs2);

        let default_filestore_config = FilestoreConfig::no_chunking_filestore();

        let bytes = b"hello world";
        let req = request(bytes);
        let cid = canonical(bytes);

        store(
            bs1,
            default_filestore_config,
            ctx,
            &req,
            stream::once(async { Ok(Bytes::from(&bytes[..])) }),
        )
        .await?;
        copy_content(ctx, bs1, bs2, default_filestore_config.clone(), cid).await?;
        let maybe_copy_meta = get_metadata(bs2, ctx, &FetchKey::Canonical(cid)).await?;

        let copy_meta =
            maybe_copy_meta.expect("Copied file not found in the destination filestore");
        assert_eq!(copy_meta.total_size, bytes.len() as u64);
        assert_eq!(copy_meta.content_id, cid);
        Ok(())
    }
}
