// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use blobstore::Blobstore;
use context::CoreContext;
use failure::{format_err, Error};
use futures::future::{err, Future};
use futures_ext::FutureExt;
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

#[cfg(test)]
mod test {
    use super::*;
    use censoredblob::CensoredBlob;
    use context::CoreContext;
    use memblob::EagerMemblob;
    use mononoke_types::BlobstoreBytes;
    use prefixblob::PrefixBlobstore;
    use scuba_ext::ScubaSampleBuilder;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::runtime::Runtime;

    #[test]
    fn test_copy_blob() {
        let mut rt = Runtime::new().unwrap();

        let ctx = CoreContext::test_mock();

        let inner1 = Arc::new(EagerMemblob::new());
        let inner2 = Arc::new(EagerMemblob::new());

        let bs1: RepoBlobstore = CensoredBlob::new(
            PrefixBlobstore::new(inner1, "prefix1"),
            Some(HashMap::new()),
            ScubaSampleBuilder::with_discard(),
        );

        let bs2: RepoBlobstore = CensoredBlob::new(
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
}
