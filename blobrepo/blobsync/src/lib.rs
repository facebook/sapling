// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use blobstore::Blobstore;
use cloned::cloned;
use context::CoreContext;
use failure::{format_err, Error};
use filestore::{fetch, get_metadata, store, FetchKey, FilestoreConfig, StoreRequest};
use futures::future::{err, ok, Future};
use futures_ext::{BoxFuture, FutureExt};
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
        .and_then(move |maybe_blobstore_bytes| match maybe_blobstore_bytes {
            Some(blobstore_bytes) => dst_blobstore.put(ctx, key, blobstore_bytes).left_future(),
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
    get_metadata(&src_blobstore, ctx.clone(), &fetch_key.clone())
        .and_then({
            cloned!(ctx, src_blobstore, fetch_key, key);
            move |maybe_content_metadata| {
                let store_request = match maybe_content_metadata {
                    Some(content_metadata) => {
                        StoreRequest::with_canonical(content_metadata.total_size, key)
                    }
                    None => {
                        return err(format_err!("File not found for fetch key: {:?}", fetch_key))
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
                        Some(byte_stream) => ok((store_request, byte_stream)).right_future(),
                    })
                    .right_future()
            }
        })
        .and_then({
            move |(store_request, byte_stream)| {
                store(
                    dst_blobstore,
                    &dst_filestore_config,
                    ctx,
                    &store_request,
                    byte_stream,
                )
                .map(|_| ())
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
    use futures::stream;
    use memblob::EagerMemblob;
    use mononoke_types::{typed_hash, BlobstoreBytes, ContentMetadata};
    use prefixblob::PrefixBlobstore;
    use redactedblobstore::RedactedBlobstore;
    use scuba_ext::ScubaSampleBuilder;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::runtime::Runtime;

    fn request(data: impl AsRef<[u8]>) -> StoreRequest {
        StoreRequest::new(data.as_ref().len() as u64)
    }

    fn canonical(data: impl AsRef<[u8]>) -> ContentId {
        let mut ctx = typed_hash::ContentIdContext::new();
        ctx.update(data.as_ref());
        ctx.finish()
    }

    #[fbinit::test]
    fn test_copy_blob(fb: FacebookInit) {
        let mut rt = Runtime::new().unwrap();

        let ctx = CoreContext::test_mock(fb);

        let inner1 = Arc::new(EagerMemblob::new());
        let inner2 = Arc::new(EagerMemblob::new());

        let bs1: RepoBlobstore = RedactedBlobstore::new(
            PrefixBlobstore::new(inner1, "prefix1"),
            Some(HashMap::new()),
            ScubaSampleBuilder::with_discard(),
        );

        let bs2: RepoBlobstore = RedactedBlobstore::new(
            PrefixBlobstore::new(inner2, "prefix2"),
            Some(HashMap::new()),
            ScubaSampleBuilder::with_discard(),
        );

        let key: String = "key".into();
        let blob = BlobstoreBytes::from_bytes("blob");
        assert!(
            rt.block_on(bs1.put(ctx.clone(), key.clone(), blob.clone()))
                .is_ok(),
            "failed to put things into a blobstore"
        );
        assert!(
            rt.block_on(copy_blob(
                ctx.clone(),
                bs1.clone(),
                bs2.clone(),
                key.clone()
            ))
            .is_ok(),
            "failed to copy between blobstores"
        );
        let res = rt.block_on(bs2.get(ctx.clone(), key.clone()));
        assert!(
            res.unwrap() == Some(blob),
            "failed to get a copied blob from the second blobstore"
        );

        assert!(
            rt.block_on(copy_blob(
                ctx.clone(),
                bs1.clone(),
                bs2.clone(),
                "non-existing key".to_string()
            ))
            .is_err(),
            "did not err while trying to copy a non-existing key"
        )
    }

    #[fbinit::test]
    fn test_copy_content(fb: FacebookInit) -> Result<(), Error> {
        let mut rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);
        let inner1 = Arc::new(EagerMemblob::new());
        let inner2 = Arc::new(EagerMemblob::new());
        let bs1: RepoBlobstore = RedactedBlobstore::new(
            PrefixBlobstore::new(inner1, "prefix1"),
            Some(HashMap::new()),
            ScubaSampleBuilder::with_discard(),
        );

        let bs2: RepoBlobstore = RedactedBlobstore::new(
            PrefixBlobstore::new(inner2, "prefix2"),
            Some(HashMap::new()),
            ScubaSampleBuilder::with_discard(),
        );

        let default_filestore_config = FilestoreConfig::default();

        let bytes = b"hello world";
        let req = request(bytes);
        let cid = canonical(bytes);

        rt.block_on(store(
            bs1.clone(),
            &default_filestore_config,
            ctx.clone(),
            &req,
            stream::once(Ok(Bytes::from(&bytes[..]))),
        ))?;
        rt.block_on(copy_content(
            ctx.clone(),
            bs1.clone(),
            bs2.clone(),
            default_filestore_config.clone(),
            cid,
        ))?;
        let maybe_copy_meta = rt.block_on(get_metadata(
            &bs2.clone(),
            ctx.clone(),
            &FetchKey::Canonical(cid),
        ))?;

        let copy_meta =
            maybe_copy_meta.expect("Copied file not found in the destination filestore");
        assert_eq!(copy_meta.total_size, bytes.len() as u64);
        assert_eq!(copy_meta.content_id, cid);
        Ok(())
    }
}
