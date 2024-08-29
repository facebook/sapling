/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![type_length_limit = "1817182"]

use anyhow::anyhow;
use anyhow::Result;
use context::CoreContext;
use filestore::FetchKey;
use filestore::FilestoreConfig;
use mononoke_types::ContentId;
use repo_blobstore::RepoBlobstore;

pub async fn copy_content(
    ctx: &CoreContext,
    src_blobstore: &RepoBlobstore,
    dst_blobstore: &RepoBlobstore,
    dst_filestore_config: FilestoreConfig,
    key: ContentId,
) -> Result<()> {
    let fetch_key = FetchKey::Canonical(key.clone());
    if filestore::exists(dst_blobstore, ctx, &fetch_key).await? {
        return Ok(());
    }

    let content_metadata = filestore::get_metadata(src_blobstore, ctx, &fetch_key)
        .await?
        .ok_or_else(|| anyhow!("File not found for fetch key: {:?}", fetch_key))?;

    filestore::copy(
        src_blobstore,
        &src_blobstore.copier_to(dst_blobstore),
        dst_filestore_config,
        ctx,
        &content_metadata,
    )
    .await
}

#[cfg(test)]
mod test {
    #![allow(warnings)]

    use std::collections::HashMap;
    use std::sync::Arc;

    use borrowed::borrowed;
    use bytes::Bytes;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use filestore::StoreRequest;
    use futures::stream;
    use memblob::Memblob;
    use mononoke_macros::mononoke;
    use mononoke_types::typed_hash;
    use mononoke_types::BlobstoreBytes;
    use mononoke_types::ContentMetadataV2;
    use mononoke_types::RepositoryId;
    use redactedblobstore::RedactedBlobstore;
    use repo_blobstore::RepoBlobstore;
    use scuba_ext::MononokeScubaSampleBuilder;

    use super::*;

    fn request(data: impl AsRef<[u8]>) -> StoreRequest {
        StoreRequest::new(data.as_ref().len() as u64)
    }

    fn canonical(data: impl AsRef<[u8]>) -> ContentId {
        let mut ctx = typed_hash::ContentIdContext::new();
        ctx.update(data.as_ref());
        ctx.finish()
    }

    #[mononoke::fbinit_test]
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

        filestore::store(
            bs1,
            default_filestore_config,
            ctx,
            &req,
            stream::once(async { Ok(Bytes::from(&bytes[..])) }),
        )
        .await?;
        copy_content(ctx, bs1, bs2, default_filestore_config.clone(), cid).await?;
        let maybe_copy_meta = filestore::get_metadata(bs2, ctx, &FetchKey::Canonical(cid)).await?;

        let copy_meta =
            maybe_copy_meta.expect("Copied file not found in the destination filestore");
        assert_eq!(copy_meta.total_size, bytes.len() as u64);
        assert_eq!(copy_meta.content_id, cid);
        Ok(())
    }
}
