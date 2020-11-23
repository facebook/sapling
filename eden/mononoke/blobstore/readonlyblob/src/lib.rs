/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use blobstore::{Blobstore, BlobstoreGetData, BlobstorePutOps, OverwriteStatus, PutBehaviour};
use context::CoreContext;
use mononoke_types::BlobstoreBytes;
mod errors;
pub use crate::errors::ErrorKind;

/// A layer over an existing blobstore that prevents writes.
#[derive(Clone, Debug)]
pub struct ReadOnlyBlobstore<T> {
    blobstore: T,
}

impl<T> ReadOnlyBlobstore<T> {
    pub fn new(blobstore: T) -> Self {
        Self { blobstore }
    }
}

#[async_trait]
impl<T: Blobstore> Blobstore for ReadOnlyBlobstore<T> {
    #[inline]
    async fn get(&self, ctx: CoreContext, key: String) -> Result<Option<BlobstoreGetData>> {
        self.blobstore.get(ctx, key).await
    }

    #[inline]
    async fn put(&self, _ctx: CoreContext, key: String, _value: BlobstoreBytes) -> Result<()> {
        Err(ErrorKind::ReadOnlyPut(key).into())
    }

    #[inline]
    async fn is_present(&self, ctx: CoreContext, key: String) -> Result<bool> {
        self.blobstore.is_present(ctx, key).await
    }
}

#[async_trait]
impl<T: BlobstorePutOps> BlobstorePutOps for ReadOnlyBlobstore<T> {
    async fn put_explicit(
        &self,
        _ctx: CoreContext,
        key: String,
        _value: BlobstoreBytes,
        _put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        Err(ErrorKind::ReadOnlyPut(key).into())
    }

    async fn put_with_status(
        &self,
        _ctx: CoreContext,
        key: String,
        _value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        Err(ErrorKind::ReadOnlyPut(key).into())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use fbinit::FacebookInit;

    use memblob::Memblob;

    #[fbinit::test]
    async fn test_error_on_write(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let base = Memblob::default();
        let wrapper = ReadOnlyBlobstore::new(base.clone());
        let key = "foobar".to_string();

        let r = wrapper
            .put(
                ctx.clone(),
                key.clone(),
                BlobstoreBytes::from_bytes("test foobar"),
            )
            .await;
        assert!(!r.is_ok());
        let base_present = base.is_present(ctx, key.clone()).await.unwrap();
        assert!(!base_present);
    }

    #[fbinit::test]
    async fn test_error_on_put_with_status(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let base = Memblob::default();
        let wrapper = ReadOnlyBlobstore::new(base.clone());
        let key = "foobar".to_string();

        let r = wrapper
            .put_with_status(
                ctx.clone(),
                key.clone(),
                BlobstoreBytes::from_bytes("test foobar"),
            )
            .await;
        assert!(!r.is_ok());
        let base_present = base.is_present(ctx, key.clone()).await.unwrap();
        assert!(!base_present);
    }
}
