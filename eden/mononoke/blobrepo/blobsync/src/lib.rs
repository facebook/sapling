/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
#![type_length_limit = "1817182"]

use anyhow::{format_err, Error};
use blobstore::Blobstore;
use cloned::cloned;
use context::CoreContext;
use filestore::{exists, fetch, get_metadata, store, FetchKey, FilestoreConfig, StoreRequest};
use futures::future::TryFutureExt;
use futures_ext::{BoxFuture, FutureExt};
use futures_old::future::{err, ok, Future};
use mononoke_types::ContentId;
use repo_blobstore::RepoBlobstore;

/// Copy a blob with a key `key` from `src_blobstore` to `dst_blobstore`
pub fn copy_blob(
    ctx: CoreContext,
    src_blobstore: RepoBlobstore,
    dst_blobstore: RepoBlobstore,
    key: String,
) -> impl Future<Item = (), Error = Error> {
    // TODO(ikostia, T48858215): for cases when remote copy is possible, utilize it
    src_blobstore
        .get(ctx.clone(), key.clone())
        .compat()
        .and_then(move |maybe_blobstore_bytes| match maybe_blobstore_bytes {
            Some(srcdata) => dst_blobstore
                .put(ctx, key, srcdata.into())
                .compat()
                .left_future(),
            None => err(format_err!("Key {} is missing in the original store", key)).right_future(),
        })
}

pub fn copy_content(
    ctx: CoreContext,
    src_blobstore: RepoBlobstore,
    dst_blobstore: RepoBlobstore,
    dst_filestore_config: FilestoreConfig,
    key: ContentId,
) -> BoxFuture<(), Error> {
    let fetch_key = FetchKey::Canonical(key.clone());
    exists(&dst_blobstore, ctx.clone(), &fetch_key)
        .and_then(move |exists| {
            if exists {
                ok(()).boxify()
            } else {
                get_metadata(&src_blobstore, ctx.clone(), &fetch_key.clone())
                    .and_then({
                        cloned!(ctx, src_blobstore, fetch_key, key);
                        move |maybe_content_metadata| {
                            let store_request = match maybe_content_metadata {
                                Some(content_metadata) => {
                                    StoreRequest::with_canonical(content_metadata.total_size, key)
                                }
                                None => {
                                    return err(format_err!(
                                        "File not found for fetch key: {:?}",
                                        fetch_key
                                    ))
                                    .left_future()
                                }
                            };

                            fetch(&src_blobstore, ctx, &fetch_key)
                                .and_then(move |maybe_byte_stream| match maybe_byte_stream {
                                    None => {
                                        return err(format_err!(
                                            "File not found for fetch key: {:?}",
                                            fetch_key
                                        ))
                                        .left_future()
                                    }
                                    Some(byte_stream) => {
                                        ok((store_request, byte_stream)).right_future()
                                    }
                                })
                                .right_future()
                        }
                    })
                    .and_then({
                        move |(store_request, byte_stream)| {
                            store(
                                dst_blobstore,
                                dst_filestore_config,
                                ctx,
                                &store_request,
                                byte_stream,
                            )
                            .map(|_| ())
                        }
                    })
                    .boxify()
            }
        })
        .boxify()
}

#[cfg(test)]
mod test {
    #![allow(warnings)]

    use super::*;
    use bytes::Bytes;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use futures::compat::Future01CompatExt;
    use futures_old::stream;
    use memblob::EagerMemblob;
    use mononoke_types::{typed_hash, BlobstoreBytes, ContentMetadata, RepositoryId};
    use redactedblobstore::RedactedBlobstore;
    use repo_blobstore::RepoBlobstoreArgs;
    use scuba_ext::ScubaSampleBuilder;
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

    #[fbinit::compat_test]
    async fn test_copy_blob(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);

        let inner1 = Arc::new(EagerMemblob::new());
        let inner2 = Arc::new(EagerMemblob::new());

        let bs1 = RepoBlobstoreArgs::new(
            inner1,
            None,
            RepositoryId::new(1),
            ScubaSampleBuilder::with_discard(),
        )
        .into_blobrepo_parts()
        .0;

        let bs2 = RepoBlobstoreArgs::new(
            inner2,
            None,
            RepositoryId::new(2),
            ScubaSampleBuilder::with_discard(),
        )
        .into_blobrepo_parts()
        .0;

        let key: String = "key".into();
        let blob = BlobstoreBytes::from_bytes("blob");
        assert!(
            bs1.put(ctx.clone(), key.clone(), blob.clone())
                .await
                .is_ok(),
            "failed to put things into a blobstore"
        );
        assert!(
            copy_blob(ctx.clone(), bs1.clone(), bs2.clone(), key.clone())
                .compat()
                .await
                .is_ok(),
            "failed to copy between blobstores"
        );
        let res = bs2.get(ctx.clone(), key.clone()).await;
        assert!(
            res.unwrap() == Some(blob.into()),
            "failed to get a copied blob from the second blobstore"
        );

        assert!(
            copy_blob(
                ctx.clone(),
                bs1.clone(),
                bs2.clone(),
                "non-existing key".to_string()
            )
            .compat()
            .await
            .is_err(),
            "did not err while trying to copy a non-existing key"
        )
    }

    #[fbinit::compat_test]
    async fn test_copy_content(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let inner1 = Arc::new(EagerMemblob::new());
        let inner2 = Arc::new(EagerMemblob::new());

        let bs1 = RepoBlobstoreArgs::new(
            inner1,
            None,
            RepositoryId::new(1),
            ScubaSampleBuilder::with_discard(),
        )
        .into_blobrepo_parts()
        .0;

        let bs2 = RepoBlobstoreArgs::new(
            inner2,
            None,
            RepositoryId::new(2),
            ScubaSampleBuilder::with_discard(),
        )
        .into_blobrepo_parts()
        .0;

        let default_filestore_config = FilestoreConfig::default();

        let bytes = b"hello world";
        let req = request(bytes);
        let cid = canonical(bytes);

        store(
            bs1.clone(),
            default_filestore_config,
            ctx.clone(),
            &req,
            stream::once(Ok(Bytes::from(&bytes[..]))),
        )
        .compat()
        .await?;
        copy_content(
            ctx.clone(),
            bs1.clone(),
            bs2.clone(),
            default_filestore_config.clone(),
            cid,
        )
        .compat()
        .await?;
        let maybe_copy_meta = get_metadata(&bs2.clone(), ctx.clone(), &FetchKey::Canonical(cid))
            .compat()
            .await?;

        let copy_meta =
            maybe_copy_meta.expect("Copied file not found in the destination filestore");
        assert_eq!(copy_meta.total_size, bytes.len() as u64);
        assert_eq!(copy_meta.content_id, cid);
        Ok(())
    }
}
