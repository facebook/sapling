/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Result;
use async_trait::async_trait;
use inlinable_string::InlinableString;

use context::CoreContext;

use blobstore::{Blobstore, BlobstoreGetData, BlobstorePutOps, OverwriteStatus, PutBehaviour};
use mononoke_types::BlobstoreBytes;

/// A layer over an existing blobstore that prepends a fixed string to each get and put.
#[derive(Clone, Debug)]
pub struct PrefixBlobstore<T> {
    // Try to inline the prefix to ensure copies remain cheap. Most prefixes are short anyway.
    prefix: InlinableString,
    blobstore: T,
}

impl<T> PrefixBlobstore<T> {
    pub fn into_inner(self) -> T {
        self.blobstore
    }

    pub fn as_inner(&self) -> &T {
        &self.blobstore
    }
}

impl<T> PrefixBlobstore<T> {
    pub fn new<S: Into<InlinableString>>(blobstore: T, prefix: S) -> Self {
        let prefix = prefix.into();
        Self { prefix, blobstore }
    }

    #[inline]
    pub fn prepend(&self, key: String) -> String {
        [&self.prefix, key.as_str()].concat()
    }
}

#[async_trait]
impl<T: Blobstore> Blobstore for PrefixBlobstore<T> {
    #[inline]
    async fn get(&self, ctx: CoreContext, key: String) -> Result<Option<BlobstoreGetData>> {
        self.blobstore.get(ctx, self.prepend(key)).await
    }

    #[inline]
    async fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> Result<()> {
        self.blobstore.put(ctx, self.prepend(key), value).await
    }

    #[inline]
    async fn is_present(&self, ctx: CoreContext, key: String) -> Result<bool> {
        self.blobstore.is_present(ctx, self.prepend(key)).await
    }
}

#[async_trait]
impl<T: BlobstorePutOps> BlobstorePutOps for PrefixBlobstore<T> {
    async fn put_explicit(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        self.blobstore
            .put_explicit(ctx, self.prepend(key), value, put_behaviour)
            .await
    }

    async fn put_with_status(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        self.blobstore
            .put_with_status(ctx, self.prepend(key), value)
            .await
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use bytes::Bytes;
    use fbinit::FacebookInit;

    use memblob::Memblob;

    #[fbinit::compat_test]
    async fn test_prefix(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let base = Memblob::default();
        let prefixed = PrefixBlobstore::new(base.clone(), "prefix123-");
        let unprefixed_key = "foobar".to_string();
        let prefixed_key = "prefix123-foobar".to_string();

        prefixed
            .put(
                ctx.clone(),
                unprefixed_key.clone(),
                BlobstoreBytes::from_bytes("test foobar"),
            )
            .await
            .expect("put should succeed");

        // Test that both the prefixed and the unprefixed stores can access the key.
        assert_eq!(
            prefixed
                .get(ctx.clone(), unprefixed_key.clone())
                .await
                .expect("get should succeed")
                .expect("value should be present")
                .into_raw_bytes(),
            Bytes::from("test foobar"),
        );
        assert_eq!(
            base.get(ctx.clone(), prefixed_key.clone())
                .await
                .expect("get should succeed")
                .expect("value should be present")
                .into_raw_bytes(),
            Bytes::from("test foobar"),
        );

        // Test that is_present works for both the prefixed and unprefixed stores.
        assert!(
            prefixed
                .is_present(ctx.clone(), unprefixed_key.clone())
                .await
                .expect("is_present should succeed")
        );
        assert!(
            base.is_present(ctx.clone(), prefixed_key.clone())
                .await
                .expect("is_present should succeed")
        );
    }
}
