/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use blobstore::Blobstore;
use context::CoreContext;
use filestore::FetchKey;
use filestore::FilestoreConfig;
use filestore::StoreRequest;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::hash::Sha256;
use std::sync::Arc;

#[derive(Clone)]
pub struct DarkstormVerifier {
    origin_blobstore: Arc<dyn Blobstore>,
    backup_blobstore: Arc<dyn Blobstore>,
    backup_filestore_config: FilestoreConfig,
}

impl DarkstormVerifier {
    pub fn new(
        origin_blobstore: Arc<dyn Blobstore>,
        backup_blobstore: Arc<dyn Blobstore>,
        backup_filestore_config: FilestoreConfig,
    ) -> Self {
        Self {
            origin_blobstore,
            backup_blobstore,
            backup_filestore_config,
        }
    }

    pub async fn upload(&self, ctx: CoreContext, blobs: &[(Sha256, u64)]) -> Result<(), Error> {
        let ctx = &ctx;

        stream::iter(blobs.iter().copied())
            .map(Ok)
            .try_for_each_concurrent(50, async move |(key, size)| -> Result<(), Error> {
                let blob = filestore::fetch_stream(
                    self.origin_blobstore.clone(),
                    ctx,
                    FetchKey::from(key),
                );
                let request = StoreRequest::with_sha256(size, key);
                filestore::store(
                    &self.backup_blobstore.clone(),
                    self.backup_filestore_config,
                    ctx,
                    &request,
                    blob,
                )
                .await
                .with_context(|| format!("while syncing LFS entry {:?}", key))?;
                Ok(())
            })
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    #![allow(warnings)]
    use super::*;
    use fbinit::FacebookInit;
    use futures::stream;
    use futures::TryFutureExt;
    use futures::TryStreamExt;
    use memblob::Memblob;

    #[fbinit::test]
    async fn test_upload(fb: FacebookInit) -> Result<(), anyhow::Error> {
        let ctx = CoreContext::test_mock(fb);

        let origin = Arc::new(Memblob::default());
        let backup = Arc::new(Memblob::default());
        let filestore = FilestoreConfig::no_chunking_filestore();

        let verifier = DarkstormVerifier::new(origin.clone(), backup.clone(), filestore);

        let bytes = b"hello world";
        let size = bytes.len().try_into().unwrap();
        let req = StoreRequest::new(size);

        let res = filestore::store(
            &origin,
            filestore,
            &ctx,
            &req,
            stream::once(async { Ok(bytes::Bytes::from(&bytes[..])) }),
        )
        .await?;

        let alias = res.sha256;

        verifier
            .upload(ctx.clone(), &vec![(alias, size)][..])
            .await?;

        let uploaded_bytes = filestore::fetch(backup, ctx, &FetchKey::from(alias))
            .map_ok(|maybe_stream| async move {
                let res: Vec<bytes::Bytes> = maybe_stream.unwrap().try_collect().await?;
                Result::<_, Error>::Ok(res)
            })
            .try_flatten()
            .await?;
        assert_eq!(vec![bytes::Bytes::from(&bytes[..])], uploaded_bytes);
        Ok(())
    }
}
